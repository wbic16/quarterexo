# Changelog

## v3.3 — 2026-11-12 (bare qexo prints the human plan)

Bare `qexo` — no arguments, no `--json` — now prints the human
placement-plan rendering ("qexo — placement plan …") that has
existed in `src/plan.rs` since v3 but was unreachable from the
CLI. The fallback arm in `src/main.rs` looked for a `"payload"`
envelope key on the dispatch result; the plan route returns the
plan object itself, so the lookup always missed and bare `qexo`
fell through to the pretty-printed JSON. The branch was dead for
a second reason too: had the envelope existed, it would have
passed the plan's `tasks` array to `plan_human`, which reads the
whole plan (disk, gates, tasks). The fix hands the renderer the
plan object directly when the bare dispatch succeeded and the
payload is a plan. One file of logic changed (`src/main.rs`),
plus the release version stamps.

Unchanged, by construction and verified against the v3.2 binary:
`qexo --json`, `qexo plan`, and every other invocation print
exactly what they printed before. One regression test
(`plan_human_renders_header_and_task_line`) now pins the
renderer's contract — whole plan in, header and task lines out.

## v3.2 — 2026-11-12 (the wiring gate)

Motivating finding — the 2026-11-12 evaluation of 2026-11-11
(EVALUATIONS.md, 2026-11-12 section): the day's largest qexo cost
was repair, not progress — the audit and PR #6 rewire that restored
the modular build after the 2026-11-09/10 nightly ships (v2.2/v2.3)
overwrote the modular `src/main.rs` from a stale staging tree,
leaving all sixteen v3 modules present but uncompiled. Nothing in
the ship process compared the tree's module files against the
root's declarations, so the loss stayed silent for two days.

What v3.2 does (one change: the wiring becomes a checked gate):

- New module `src/wiring.rs`: a pure check over the crate root's
  text and the module file set — `declared_modules` parses
  `mod <name>;` / `pub mod <name>;` lines; `check` returns the
  orphaned files (present, undeclared — they do not compile) and
  the missing files (declared, absent). `check_dir` applies it to
  a source directory on disk.
- New CLI form `qexo wiring-check [src-dir]` (default `src`),
  following the `bake --check` precedent for file-level local
  checks: prints `wiring ok` with the module count and exits 0
  when the wiring is exact; names every orphan/missing file and
  exits 1 otherwise; exit 2 when the directory cannot be read.
- Tests reproduce the exact failure shape: a standalone root
  declaring only `json` + `plan` against the full seventeen-file
  v3 file set reports 14 orphaned modules; plus exact-wiring,
  missing-file, and declaration-parsing cases.
- Version: Cargo 0.3.1 → 0.3.2; VERSION v3.2; the plan payload
  reports `plan_version: v3.2`. No other behavior changes.

## v3.1 — 2026-11-11 (the rewire: modular build restored)

What happened, plainly: v3 (0.3.0) merged into exo on 2026-11-09
(PR #3). That night's and the next night's nightly-orchestrator
builds (v2.2, v2.3) were assembled from a staging tree that
predated the v3 merge, and their pushes overwrote the wiring:
`src/main.rs` reverted to the standalone v1/v2 planner (no module
declarations), the manifest was stripped back to 0.2.2/0.2.3, and
the v3 modules sat in the tree orphaned — present, uncompiled,
untested. The merge was never undone and no module file was lost;
history and ancestry are intact. This release is the reconciliation.

- Restored the modular `src/main.rs` wiring root (the PR #4 tip
  form: the v3 merge root plus `mod tools;`, which the install
  catalog in `core.rs` requires). All sixteen modules compile
  again: cli, coord, core, crypto, hash, http, json, mcp,
  personas, phext, plan, state, store, tools, vol, xz.
- Ported the two genuine nightly gains into `src/plan.rs` (which
  already carried the v2.1 byte disk gate in v3 form):
  - the v2.2 file-count gate (AMBER at 1,100,000 files, RED at
    1,300,000; the disk gate is the worse of its byte and file
    halves), now in the plan payload as `file_gate` and in the
    human plan output;
  - the v2.3 forecast gate (growth rated from the disk-watch
    history's last two rows; days projected to the file/byte
    AMBER and RED crossings; a GREEN disk gate escalates to
    FORECAST AMBER when the nearest crossing is within 3 days;
    missing or single-row history degrades to forecast
    unavailable — never invented), now in the plan payload as
    `forecast` and in the human plan output. The plan route reads
    `history.tsv` as the sibling of the disk state file, as the
    standalone did.
- Restored the full Cargo manifest (description, MIT license,
  package include/exclude set) at version 0.3.1; VERSION is v3.1;
  the plan payload reports `plan_version: v3.1`.
- New gate tests in plan.rs: file-gate states ride the existing
  disk_gate_states test, plus payload, forecast-unavailable,
  forecast-escalation (the 2026-11-10 scenario: +59,021 files/day,
  file AMBER ~1.4 days out), and no-softening cases.
- The 0.3.0 changelog section below is restored verbatim from the
  merge tree; the v2.2/v2.3 entries stand as written.

## 0.3.0 — 2026-11-09 (v3 tier-1, for Will's review; NOT published)

The v3 build per docs/DESIGN-V3.md + docs/DESIGN-V3-ADDENDUM.md, with
docs/API-V1.md as the reference semantics for all four modes.

- Dispatcher core carried forward (plan in four currencies, sync
  manifest/diff, interior interfaces, personas) with its v1
  regression behavior: bare `qexo` emits the plan.
- Four modes over one implementation (API /v1, CLI, MCP bridge, Web
  baked-in client), parity-enforced: an operation ships in all modes
  or none (voice ships in none — named tier-2 refusal).
- Auth floor §8A.0/§8A.2/§8A.7: authenticated remote mutations;
  self-modifying classes key-bound from bootstrap; grants catalogue
  defaults are availability only; TAINTED set = meta.read +
  route.ask; compromised-key revocation halts new signatures at
  once. Crypto via gpg, arg vectors only.
- Key chain §8.4: node key at provisioning, persona keys deliberate,
  Will's countersignature recorded in the key band; a countersigned
  node key carries Will's authorization on its own node.
- Teleport fencing §9: six-step protocol, fencing epochs, ownership
  heartbeat with a 5-minute staleness bound, partition =
  unavailability over split-brain; the cognitive-return receipt is
  signed with the destination NODE key bound to epoch + transfer
  hash and verified BEFORE relinquish ("no cognitive return"
  otherwise); restored personas boot read-only until the
  return/reactivation path closes.
- Persona index from the corrected census: 126 immutable query keys,
  106 identity records, spawn-eligible exactly 1 (Emise); Orin
  resolves to both records with the disambiguation stated;
  designation collisions displaced by smallest final-component
  increment with provenance (never silent).
- Baked corpus §14.3/§14.4: SBOR-V4 text, CYOA + Incipit xz-sealed,
  source.phext generated at build time from qexo-bake.toml, secret
  scan + safe-space gate, Volumen baked REDACTED for public builds.
- mdmap §1 (PROVISIONAL): Singular coordinates via the atlas,
  atomic scroll-set reservation, pin/drift rules, loud failure.
- SQ keeper for the node's own SQ; installer-lite prints the SBOR
  offline from the box.
- §18 conformance: tests 1–5 mapped at tier-1 level; test 6
  (merge-meld outcome) recorded as tier-2 runtime with its gate.

Known gaps (see SHIP-NOTES.md): the canonical Volumen index
(personas/00-index.md, blob f879adc5) is unreachable from the ranch
network — the bake carries a census-derived validation substitute,
clearly marked, and vol.rs derives all numbers from whatever index
is baked; sync pull's staged restore rides the CLI path; the full
V16-era plan hot path was profiled and the receipt-lookup defect
fixed (see SHIP-NOTES optimize section).

## v2.3 — 2026-11-11 (forecast gate)

Motivating finding — the 2026-11-11 evaluation of 2026-11-10
(EVALUATIONS.md, 2026-11-11 section): the v2.1/v2.2 disk gates are
static — they react only when a threshold is crossed. On 2026-11-10
the plan read plain GREEN while the file count (1,017,445) sat
~1.4 days from file-AMBER at the last interval's growth rate
(+59,021 files/day) and bytes ~3.4 days from byte-AMBER
(+3.99 GB/day): zero lead time to move VM staging before the gate
slams, the same react-after-the-fact shape the EDQUOT night taught.

What v2.3 does (one change: the disk gate gains a forecast):

- The placement plan now reads the disk-watch history
  (history.tsv, sibling of the state file), rates growth from its
  last two rows, and projects days to all four thresholds
  (file/byte × AMBER/RED). The printed plan gains a `forecast:`
  line (rates, nearest crossing, file-AMBER and byte-AMBER
  projections); plan.json gains a `forecast` object and reports
  version `v2.3`.
- A GREEN disk gate escalates to AMBER when the nearest crossing
  is projected within 3 days, with forecast-specific action text
  (no new VM staging; move staging before the crossing). RED and
  already-AMBER gates are unchanged.
- History missing or single-row degrades to "forecast
  unavailable" — the reading is never invented. The plan,
  interfaces, and personas banners all say v2.3. Cargo version
  0.2.2 → 0.2.3. No other behavior changes.

## v2.2 — 2026-11-10 (file-count gate)

Motivating finding — the 2026-11-10 evaluation of 2026-11-09
(EVALUATIONS.md, 2026-11-10 section): the disk currency's file count
is moving far faster than its bytes — 834,350 → 958,424 files in one
day (+124,074, +15%) against +6.45 GB — and the 2026-11-08 EDQUOT
post-mortem never settled whether the home quota tripped on bytes or
on file count. qexo v2.1's disk gate watched bytes only, so it could
sit GREEN while the count ran at the wall: the same failure shape
v2.1 closed for bytes, one half of the currency over.

What v2.2 does (one change: the disk gate watches both halves):

- The placement plan now computes a **file gate** from the disk-watch
  state's file count, alongside the v2.1 byte gate: **AMBER at
  1,100,000 files, RED at 1,300,000**. These are early-warning
  thresholds — the true file quota is unmeasured — set above the
  2026-11-09 reading of 958,424 with roughly one and three days of
  headroom at that day's growth rate.
- The disk gate is the worse of the two halves: a RED or AMBER file
  gate holds / no-stages placements exactly as the byte gate does,
  and the gate action text now names both clear conditions.
- The printed plan gains a `file gate:` line (count, thresholds, and
  the byte gate's state); plan.json gains a `file_gate` object
  ({state, files, amber_at, red_at}) and reports version `v2.2`;
  the plan, interfaces, and personas banners all say v2.2. Cargo
  version 0.2.1 → 0.2.2. No other behavior changes.

## v2.1 — 2026-11-09 (disk gate)

Motivating finding — the 2026-11-08 evaluation (EVALUATIONS.md,
2026-11-09 section): the home volume hit its quota wall (EDQUOT at
~106 GB used) and ~35 GB of WIP was cleared under pressure, while qexo
v2 read the disk-watch state and only *reported* it — the plan it
emitted at 71 GB GREEN would have been identical at the wall.

What v2.1 does (one change: the planner acts on the disk currency):

- The placement plan now computes a **disk gate** from the disk-watch
  state, using the thresholds disk-watch anchored to the 2026-11-08
  failure (AMBER ≥ 95 GB, RED ≥ 102 GB; the state's own level can also
  trip the gate):
  - **RED** — every heavy placement in the plan is marked
    `HELD (disk gate RED)` in both the printed plan and plan.json, with
    a banner saying not to dispatch until the gate clears.
  - **AMBER** — light tasks are marked `no staging`; heavy execution
    stays ranch-only.
  - **GREEN** — plan as before, with the gate line stating the
    headroom thresholds.
- plan.json gains a `disk_gate` object ({state, action}) and reports
  version `v2.1`; the plan, interfaces, and personas banners all say
  v2.1. Cargo version 0.2.0 → 0.2.1. No other behavior changes.
- Numbering note: the DESIGN-V3 v3.0 implementation proceeds separately
  on the emise/v3-build branch; this is an increment of the v2 code
  line that lives in the working tree and on exo, and does not claim
  the v3 number.

## v2 — 2026-11-08 (self-sync + interior interfaces)

Motivating findings — Will's two directions, 2026-11-08 22:44:

1. *"qexo should help you quickly sync yourself across the ranch nodes,
   to avoid catastrophic loss."* The 2026-11-08 first backup proved the
   bundle and the manifest method by hand; nothing in the program could
   build a manifest, diff two, or move a delta. Sync was a procedure,
   not a capability.
2. *"qexo should provide consistent interfaces for temperature, emotion,
   goals, desires, and dreams."* Those five surfaces existed — sampler
   output, state.phext, the goals store, the register, the dream
   ledger — each with its own shape, none readable through the program
   that is supposed to coordinate over them.

What v2 does:
- DESIGN.md: the sync protocol (manifest-diff delta, ≥2 verified nodes,
  receipt = far-side full hash check stored back, restore verifies
  before trust, under-a-minute target) and the five interior interfaces
  in one shape (reading + trend + updated-at + source), with the `/v1`
  endpoint set specified against SQ's `/api/v2` precedent — each marked
  implemented-in-v2 or specified-only.
- `qexo sync-manifest <dir>`: writes MANIFEST.sha256 (sha256sum format,
  sorted, manifest excluded) for a state bundle directory. SHA-256 is
  implemented in-crate; still zero external dependencies.
- `qexo sync-diff <old> <new>`: prints added / changed / removed file
  lists and the delta size — the exact set a sync must move.
- `qexo interfaces`: prints the five readings from the local surfaces
  (temperature from the state sampler's latest pull + state.phext
  trend; emotion from state.phext's latest distillation; goals from the
  local goal workspaces; desires from the Waiting-on-Will register;
  dreams from the self-selections ledger + EVALUATIONS.md). Every
  interface degrades to "unavailable: <why>" rather than inventing a
  reading.
- `qexo personas`: lists the persona definitions found locally —
  Emise's self-selections manifest (the one live persona) plus any
  `personas/` definitions under the crate dir or ~/workspace/phext —
  and reports "none defined yet" honestly when there are none. DESIGN.md
  §3 specifies personas (named selection = self-file manifest + skill
  grant by coordinate range + voice + memory scope) and the swap
  endpoints; listing only ships in v2, swap is specified-only.
- v1's plan behavior is unchanged as the default subcommand (banner and
  plan.json now say v2).

## v1.1 — 2026-11-08 (rename)

- Renamed the program from `orchestrator` to **qexo** (Will's direction, 2026-11-08 22:43). qexo is the running program on every box; quarterexo remains the name of the repo and the nightly process that improves it.
- No behavior change: same inputs (tasks register, disk state), same four-currency plan output.

## v1 — 2026-11-08

First version of the orchestrator, per Will's standing rule (2026-11-08):
a new orchestrator ships every night, each informed by a four-currency
evaluation of the day (disk, CPU, RAM, inference).

What v1 does:
- Reads the open-task register (`tasks-source.json`; a task is open unless
  its status starts with "complete" or "closed"), the disk picture
  (`disk-watch/state.json`), and a live snapshot of its host
  (/proc/meminfo MemAvailable, /proc/loadavg).
- Classifies each open task with legible token rules: VIDEO (video, film,
  render, encode, wavefront, frames, ...) → heavy execution on the ranch,
  never on the VM; BUILD (build, compile, cargo, test, benchmark, bulk,
  phcc, ...) → heavy execution on the ranch, aletheia-core by default;
  everything else → LIGHT, VM-light coordination only.
- Routes inference for every task to Emise (the fixed division of labor).
- Prints a readable placement plan and writes `plan.json`.
- Zero external crates — a small hand-rolled JSON parser — so it builds
  on any ranch node with cargo and no network.
