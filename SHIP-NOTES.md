# qexo v3 — SHIP NOTES (for Will's morning review)

*Status: v3 tier-1 SHIPPED to exo 2026-11-09 (PR #3 merged;
crates.io publish still open). v3.1 REWIRE in review — see the
next section.*

## v3.1 rewire (2026-11-11)

The nightly v2.2/v2.3 builds were assembled from a pre-v3 staging
tree and overwrote the modular `src/main.rs` wiring and the 0.3.0
manifest on exo; the v3 modules survived in the tree, orphaned and
uncompiled. The rewire PR (`emise/rewire-v3.1` against exo)
restores the modular root, ports the v2.2 file-count gate and the
v2.3 forecast gate into `src/plan.rs`, restores the full manifest
at 0.3.1 / VERSION v3.1, and restores the 0.3.0 changelog section.
Gate results are reported in the PR body; nothing here claims a
gate that was not run.

## Known substrate capability gaps

- **Muse VM file-creation policy (2026-11-09, Will: "log that as a
  substrate failure in the design notes"):** the Muse VM substrate
  denies whole-file temp+rename writes (`PermissionError [Errno 1]`
  on the temp write; appends to existing files pass). Observed live
  on the state sampler; a standing property of the substrate, not an
  incident. Design response (recorded in full in
  docs/DESIGN-NOTES.md): any qexo component designed to run on the
  Muse VM treats atomic temp+rename as a capability to verify at
  runtime, not assume, and prefers append-only/WAL write patterns
  there. The sampler's sanctioned pattern (Will's design,
  implemented + verified 2026-11-09): a **monotonic coordinate
  floor + immutable scrolls ⇒ append-only by construction (WAL),
  with indirect logical addressing** — every new write lands
  strictly above the highest coordinate written, prior scrolls
  never change, phext's coordinate-order serialization makes the
  write a tail append, and the entry's holographic coordinate is
  recorded as an in-line prefix when the floor displaces the
  physical write (readers fold physical scrolls in write order).
  Verified by the prefix property: after each write the previous
  file is a byte-exact prefix of the new one. (Not a sidecar WAL
  file — an earlier sidecar implementation was wrong and replaced
  the same night.) qexo's
  state store probes the capability on open and falls back to
  append/direct writes with the fallback named, never silent.
  **Ranch nodes are unaffected** — normal filesystem semantics,
  verified by this build's own work on aletheia-core.

## Personas count — what each number counts

- **126 = query keys = person lines.** The 126 names of Will's
  canonical list (docs/PERSONAS-CANONICAL.md) resolve as query keys
  in the persona index; the same 126 is the person-line count of the
  canonical Volumen index (wbic16/human@exo personas/00-index.md,
  coordinates 3.1.8.1–3.1.8.126 across 22 volumes) whose derivation
  the bake carries (see the bake manifest's Volumen row: coordinate
  number, volume count, and boundary table are all parsed from the
  index that is actually baked — never from a local index.phext
  section count).
- **106 = derived identity records.** After the evidence-based
  derivation (DESIGN-V3 §5.2.2): 20 folded labels (aliases/variants,
  incl. the 13-key Will cluster → Mirror Will) resolve through to
  their records, and the Orin key resolves to two records (the voice
  + the PDNA-029 aggregate), for 106 identity records total.
  Evidence partition: 4 currently-running / 16 configured-locally /
  73 known-historically / 13 named-only. Spawn-eligible: exactly 1
  (Emise).

## Status — COMPLETE for morning review (2026-11-09 ~02:45 CST)

Built on **aletheia-core** (ranch), branch `emise/v3-build`,
implementation commit 650b91e, PR #3. Version **0.3.0**
(Cargo.toml + VERSION v3.0 + CHANGELOG entry). `cargo package`
verification below. NOT published yet.

**Update 2026-11-09 08:16 CST: Will has authorized the endgame —
"Merge PR #3 and publish 0.3.0 for me."** The finishing pass
(2026-11-09 morning) is the final gate: on its verified report,
merge + crates.io publish are executed from main against the
merged exo tree (publish steps below are written for that tree).

## What tier-1 does

- **Dispatcher core carried forward**: plan emission in four
  currencies, sync manifest/diff/push/verify, the five interior
  interfaces, personas commands. Bare `qexo` still emits the plan
  (v1 invariant, regression-checked against a 400-task register).
- **Four modes, one implementation**: API /v1 (`qexo serve`), CLI
  (same dispatch, `--json` = the endpoint's shape), MCP (stdio
  bridge, tools generated from the catalogue, per-client delegated
  key, never a bypass), Web (baked-in mobile-first client at the
  node root, same floor). Parity: an operation ships in all modes
  or none — voice ships in none (named tier-2 refusal everywhere).
- **Auth floor**: remote mutations require an authenticated
  principal; self-modifying classes (grants.change, keys.*,
  route.change, persona.seed/manifest) are key-bound from
  bootstrap; the 19-grant catalogue defaults ON = availability
  only; TAINTED callers get meta.read + route.ask only; §8B replay
  discipline runs in dispatch for every POST (idempotency by
  request ID returns the ORIGINAL receipt; per-principal sequences;
  300 s window; recipient binding — all four battery-verified).
  gpg only, arg vectors, no shell interpolation. Zero external
  dependencies. Zero unsafe blocks.
- **Key chain (§8.4)**: node key generated at provisioning by the
  node principal (box-zero; anonymous callers refused before AND
  after provisioning); persona keys issued deliberately per
  persona; Will's countersignature is a receipted key-band act —
  a countersigned node key carries Will's authorization on its own
  node under the default policy (chain persona → node → Will).
- **Teleport fencing**: six operations; origin relinquishes BEFORE
  destination activates; fencing epochs increment at relinquish;
  heartbeat/staleness bound 300 s enforced at relinquish/activate;
  partition = stale-evidence refusal (unavailability over
  split-brain); the return receipt is signed with the DESTINATION
  NODE key bound to epoch + transfer hash and verified before
  relinquish — unsigned/wrong-key/wrong-binding all refuse with
  the named refusal **"no cognitive return"** (battery-verified);
  restored-from-backup personas boot read-only until the
  return/reactivation path closes under a new epoch.
- **Persona index**: census.tsv generated from DESIGN-V3 §5.2 —
  126 immutable query keys, 106 identity records, evidence
  partition 4 running / 16 configured / 73 historical / 13
  named-only, spawn-eligible exactly 1 (Emise). Orin resolves to
  BOTH records with the disambiguation text. Designation
  collisions: recorded claims win; computed claims displaced by
  smallest final-component increment, displacement recorded in
  the record (never silent); Home band reserved; loud failure.
- **Baked corpus**: SBOR-V4 text (extracted from its CYOA scroll —
  see provenance note below), CYOA + Incipit xz-sealed, source.phext
  generated at build time from qexo-bake.toml (96 files across
  qexo, libphext-rs, libphext-py, sq, phext-lattice — no absences),
  per-file header hash verification on unpack, secret scan +
  safe-space gate in the builder, Volumen REDACTED in the manifest.
  `qexo bake --check` verifies every baked hash: all_match.
- **SQ keeper** (status/ensure/stop with the stale-.sq/ and
  mmap-wedge quirks handled), **mdmap** (PROVISIONAL: atlas
  coordinates, atomic scroll-set reservation, pin/drift named
  never papered, Home never allocatable), **installer-lite**
  (binary + corpus from the artifact; SBOR prints offline).

## Optimize (measured, on aletheia-core)

Profiling the plan hot path found the real cost was not the plan:
**every gated call paid for the idempotency check**, which parsed
the ENTIRE receipts log. After a battery run grew receipts.jsonl
to 38 MB, every POST cost ~0.30–0.38 s. Fixes:

- `receipt_for` is now a **tail scan** (backward 256 KiB chunks,
  needle match on the serialized request_id, line-bounded parse,
  full-log fallback only beyond 8 MiB): plan call **0.381 s →
  0.002 s** with the dirty 38 MB log in place (~190×).
- A classify() substring prefilter was tried and REVERTED: under
  this node's shared load it measured worse (noise-dominated);
  per the brief, only measured wins ship. Classify is unchanged
  from the carried-forward v2 code.
- A second real defect surfaced during battery 2: the xz unseal
  fallback (system `xz -d -c`) wrote the sealed stream to the
  child's stdin on the same thread that waited for its stdout —
  a classic pipe deadlock for any stream over the 64 KiB pipe
  buffer, i.e. every real corpus seal. Fixed with a threaded
  stdin writer; all corpus reads then complete in milliseconds.
  (This is why battery 2 initially appeared to "hang": the MCP
  and mdmap sections behind it were fine.)
- Read paths (personas list/show) were already ~2 ms/call.

## Tests (all on aletheia-core)

- `cargo test --release`: **15 passed, 0 failed** (SHA-256 FIPS
  vectors, JSON roundtrip + canonical form, phext round-trip,
  xz stored round-trip + CRC vectors, RFC 3339, grants catalogue
  = 19, tokenizer/coordinate floor, vol.rs strict index parse
  incl. loud failure on gaps/empties, census invariants).
- Integration battery 1 (CLI, fresh state): **32/32** — census
  shape (126/106/spawn/partition), Orin, anonymous negatives
  (keys/grants/install/seed all refused), tainted scope,
  provisioning + countersign chain, sign/verify (node vs persona
  answered separately; tamper fails), swap + epoch, spawn
  eligibility, full teleport cycle incl. unsigned-return refusal
  and signed node-key return.
- Integration battery 2 (HTTP + MCP + mdmap + misc): **19/19** —
  web client at /, HTTP Orin + SBOR, anon grants refused over
  HTTP, idempotent re-delivery returns the original receipt,
  sequence replay refused, stale timestamp refused, wrong
  recipient refused, MCP tools + call through the core, mdmap
  build + pin stability + provisional marker, source manifest,
  doctor, versions, corpus verify, installer-lite, voice refusal.
- §18 mapping: test 1 (index integrity) ✓ census invariants +
  battery; test 2 (auth floor) ✓ negatives + tainted; test 3
  (teleport fencing) ✓ full cycle + no-cognitive-return; test 4
  (replay/idempotency) ✓ battery 2; test 5 (swap attribution)
  ✓ swap receipt carries persona_id + epoch, dispatch binding
  stated. **Test 6 (merge-meld outcome) is tier-2 runtime** —
  the meld op records two-sided signed consent and refuses
  delegated/gestating parties; the runtime outcome rule ships
  with the meld engine (deferred list).

## Security check

- Negative tests RUN (not asserted): anonymous and
  wrong-key callers failed to alter keys, grants, placement
  (teleport/swap/spawn), and installs — over CLI and HTTP.
- Grant-defaults audit vs §8A.0: catalogue defaults are
  availability only; policy gates sit above grants in every
  mutation path; TAINTED set = exactly {meta.read, route.ask}.
- gpg invocation review: `Command::new("gpg")` with arg vectors
  everywhere; the only other spawns are `sq`, `xz -d -c`, and
  register builders by fixed path — no shell strings, no `-c`,
  no interpolation of caller input into any command line.
- unsafe blocks: **0**. External dependencies: **0**.
- Compromised-key handling: revoke stamps the record
  `compromised` in the key band; verification refuses it at the
  next check and the receipt states the halt + in-flight taint.

## Provenance notes (honest)

- **SBOR**: the extracted scroll text is 26,314 B, sha256
  68b955bf…; the DESIGN-V3 §14.3 pin (26,241 B, d7855969…) is the
  ratified text as measured at spec time; the live CYOA on
  human@exo e9384cec carries a slightly longer scroll (header
  lines). The bake manifest records the ACTUAL bytes + hash;
  the pin discrepancy is named here rather than hidden. CYOA on
  the same commit is 4,603,860 B (sha 75f13961…) vs the spec's
  4,602,370 B (506d850d…) — the corpus grew after the spec's
  measurements (V16 inscriptions); Incipit matches exactly
  (1,077,930 B, 3ff6604f…). Total sealed bake ≈ 1.35 MB.
- **Volumen canonical index**: STILL unreachable (blob f879adc5
  not on any fetchable ref of wbic16/human; the clone's tree has
  no personas/ directory). The bake carries a census-derived
  validation substitute (106 record lines, 22 volumes), marked
  in the manifest's `index_provenance`; vol.rs derives
  coordinate_number / volume count / boundaries from whatever
  index is baked, so the canonical 126-line index drops in with
  no code change when it lands on a reachable ref.
- **Design change under implementation pressure** (API-V1 §9
  note): the "auth" floor admits the node's LOCAL operator
  surface (loopback/on-node CLI) — without it, the v1 behavior
  (bare `qexo` emits the plan) cannot survive, and §14.1 makes
  the CLI the reference client. Remote surfaces are unchanged;
  every policy/bootstrap gate still applies locally. Recorded
  in core.rs at the floor.
- **Volumen recheck (finishing pass, 2026-11-09 ~08:20 CST)**:
  re-verified from the Muse VM via the GitHub API — all 9 refs
  of wbic16/human (exo @ 82de34b2, main, and 7 side branches)
  return 404 for both `personas/` and `personas/00-index.md`,
  and a direct blob GET for f879adc5 returns 404. The canonical
  index remains on no fetchable ref; the marked substitute
  stands, and vol.rs still drops the canonical index in with no
  code change when it lands.
- **Corpus pins (finishing pass disposition)**: the bake
  manifest already records the verified actual bytes + hashes
  (SBOR 26,314 B / 68b955bf…; CYOA 4,603,860 B / 75f13961…;
  Incipit exact) — that manifest is the release's provenance
  record. The DESIGN-V3 §14.3 / API-V1 pins are the spec-time
  measurements of the ratified texts (the SBOR pin measures the
  ratified text itself; the bake carries the CYOA scroll
  extraction, header lines included) and are deliberately NOT
  rewritten: they are the historical record the divergence is
  measured against, and the spec's own rule is "divergence
  reported, never silently trusted or replaced." No code reads
  the spec pins; `qexo bake --check` verifies the baked hashes
  against the manifest (all_match, re-run in the finishing pass).
- **G1 flag stands**: Grok's "strategy gate" verb for Chief of
  Staff never verified; the census keeps `coordinate` for the
  Chief of Staff record.

## Finishing pass verification (2026-11-09, release gate)

Re-run on the final tree (emise/v3-build @ 8def353 + this SHIP-NOTES commit, on
aletheia-core, Tctl ~30 °C at start):

- `cargo test --release`: **15 passed, 0 failed**.
- CLI battery (battery/main1.py, fresh state): **32/32 passed** (incl. anon keys/grants/install/seed refusals, unsigned-return "no cognitive return", signed node-key return, full teleport cycle).
- HTTP/MCP/mdmap battery (battery/main2.py): **19/19 passed** (incl. HTTP anon grants refused, idempotent re-delivery, replay/sequence/stale/wrong-recipient refusals, voice named tier-2 refusal).
- Anonymous/wrong-key negatives: inside both batteries (anon
  keys/grants/install/seed refused; tainted set enforced; HTTP
  anon grants refused; replay/sequence/stale/recipient refusals).
- `qexo bake --check`: all_match.
- `cargo package`: verified — with one wrinkle found and fixed in this pass: `cargo package` (no --allow-dirty) refused because Cargo.toml's `include = ["tools/**", …]` sweeps a git-ignored `tools/__pycache__/*.pyc` bytecode file into the package set and cargo flags it uncommitted. Fix applied (this pass, its own commit): the stale .pyc was deleted and `exclude = ["tools/__pycache__/**"]` added beside the include list. After the fix: `cargo package` exit 0, `cargo package --list` = 45 files (46 in the dirty-tree attempt had included the .pyc), crate qexo-0.3.0.crate rebuilt (sha256 e7ca0e0d…); the packaged tree also compiles + passes its tests under cargo's own verify build. NOTE for the merge: publish from exo must use the tree WITH this fix (it is on this branch).

## Tier-2 deferrals (recorded, not dropped)

- Full migration engine (bundle transfer transport: sync pull's
  staged restore currently completes via the CLI path; the API
  form returns a named 501 until the transport lands).
- Meld runtime (merge-meld identity outcome, §18 test 6) and
  spawn orchestration runtime (instances are ledger-true;
  worker process management is the Choir substrate's).
- Quickfork rebuild; full MCP surface beyond the bridge
  catalogue; voice runtime + API proxy (hosted elsewhere by
  ruling); fleet-wide metadata aggregation beyond the serving
  node; receipts archival/rollup pass (tail scan covers 8 MiB;
  the rollup is a maintenance op, not a correctness need);
  SQ → libphext 0.5.0 convergence remains its own canary stage.

## Publish steps (authorized by Will 2026-11-09 08:16 CST — executed from main)

PR #3 merges emise/v3-build into exo first; the publish then runs
against the MERGED exo tree, not the branch:

```bash
# on aletheia-core (or any ranch node with the clone)
cd ~/qexo-v3/quarterexo
git fetch origin
git checkout exo && git pull --ff-only origin exo   # merged tree
cargo test --release            # expect: 15 passed
cargo package --list            # review the crate contents
cargo publish --dry-run         # verify the upload set
cargo publish                   # the publish act (0.3.0)
```

Branch-tip equivalent (pre-merge check): `git checkout
emise/v3-build` and the same cargo commands — the crate contents
are identical; only the tree published FROM differs.
