# Changelog

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
