# Evaluations

Four-currency snapshots and counterfactual planner reviews, one section
per night (skill: nightly-orchestrator).

## 2026-11-13 — nightly evaluation of 2026-11-12 (ships v1.2.1)

- **Disk:** GREEN, slow growth, same shape. disk-watch history:
  82.47 GB / 1,022,817 files (2026-11-11) → 84.16 GB /
  1,029,881 files (2026-11-12) — +1.69 GB, +7,064 files.
  Tonight's live check: 86 GB used of 150 GB (58%). Largest
  consumers unchanged for a fifth night: wavefront 31.73 GB
  (the lossless A/B intermediates, ~9 GB together, still
  staged on the VM), goals 19.22 GB, phext 10.62 GB. The
  day's real disk event was not on this VM at all: qexo's
  disk gate, run by Will on aletheia-core, read RED at
  808.96 GB used and held every placement — on a 3.6 TB
  volume with 2.7 TB free. The gate's absolute thresholds
  (AMBER 95 / RED 102 GB) were the VM's staging-quota numbers,
  carried from the v2 planner onto a machine they never
  described (see Counterfactual).
- **CPU:** the day's defining placement failure was a
  coordinator on the mortal VM. T8's coordinator died in the
  01:32 VM swap mid-render (the VM was replaced at 01:32,
  03:27, 05:27, 07:27 CST that day; 124 boot changes in the
  deploy-study ledger); eight frames were truncated, the task
  record read "paused," and the breakage surfaced only at
  Will's 20:47 status ask. The restart rule fired, the finish
  ran on aletheia-core, and T8 was served at 23:13. Correct
  placements elsewhere: the continual-arc campaign driver ran
  on aletheia-core (6h18m elapsed at the 17:21 watchdog pass,
  node load 8.62), every qexo build of the day (v1.0.1,
  v1.0.2, the fleet binary) built on aletheia-core, and the
  14-node fleet rollout was driven from this VM by SSH. The
  VM's 1.5 shared CPUs carried coordination plus ~131
  ten-minute task-progress ticks (1497 → 1628).
- **RAM:** 6 GB shared limit, same envelope — tonight's
  snapshot 504 MB available, 436 MB swap used, load 1.03. No
  heavy local resident was recorded on 2026-11-12; placement
  held and RAM held with it, for a fourth night running. No
  per-node RAM record for the ranch side of the day exists in
  the day's log — stated as a gap, not filled in.
- **Inference:** the well-spent bulk was real: the morning
  call's KSO design rounds (KSO = qexo: per-node state,
  teleporting tasks, divergence kept as attempt artifacts);
  SQ PR #3 and phext-lattice PR #61 merged on the evening
  call with release-build tasks queued under the new
  merge → release rule; the qexo v1.0.1 Windows-doctor
  diagnosis (two defects separated from the code: a test
  encoding Unix semantics as universal, and a real
  stale-lock bug behind it); the v1.0.2 lens redesign; the
  fleet rollout, whose version-stamp check caught a stale
  v1.0.1 binary before any node received it; and the T8
  finish. The waste was repair-shaped, for the fourth night
  running: (1) the disk-gate incident — Will's live testing
  found the planner judging a ranch volume by VM numbers;
  (2) PR #10 merged into its stacked base branch instead of
  exo, costing repair PR #11 and register item #85; (3) a
  stale pre-fix debug binary in Will's own checkout kept
  rendering the false RED, taxing the status exchange until
  it was identified as the old binary, not the merged code;
  (4) the tick loop's ~131 mostly-identical register
  reconciliations, the same verification-shaped spend named
  on 2026-11-10 and 2026-11-11.
- **Counterfactual:** the strongest case examined is T8, and
  it is a planner failure before it is a substrate failure.
  The VM's ~2-hour replacement cadence was measured and on
  record; a planner that treated a coordinator's death as a
  restart trigger — or kept the coordinator node-resident
  from the start, as the standing rule already directs —
  would have resumed T8 at the 01:32 swap and finished it
  roughly 19 hours earlier (the ranch-side finish, once
  actually started at ~20:47, took about 2.5 hours). Instead
  a stale record state ("paused") hid the breakage until
  Will asked. The restart-on-status-ask rule Will made that
  evening is the human-side fix; the planner-side fix —
  surfacing a task whose coordinator has died as broken, not
  paused — is recorded here as a future candidate, larger
  than one night's change. Secondary case, same shape
  (numbers carried across a boundary they do not describe):
  the disk gate's VM thresholds on a ranch volume — a
  first-run sanity check comparing a gate's absolute
  thresholds against the measured volume's size would have
  flagged the 95 GB ceiling on a 3.6 TB volume before Will's
  live test did. v1.0.2's lenses closed the gate half the
  same day; v1.0.3 closed the forecast half.
- **Tonight's change** is the candidate recorded in the
  2026-11-12 evaluation and still open in v1.2.0's code:
  the forecast rated growth from a single interval, so its
  nearest-crossing estimate swung ~1.4 days (2026-11-10
  interval) → ~12–14 days (2026-11-11) → ~9.9 days
  (2026-11-12, from tonight's history: last interval alone
  rates +7,064 files/day against a four-interval median of
  +33,043). v1.2.1 rates the forecast from the median of the
  per-interval rates over the most recent 7 intervals (see
  CHANGELOG). One process note: this cycle worked from a
  fresh clone of exo (833e2f34, v1.2.0) on aletheia-core —
  the local `~/workspace/orchestrator/` mirror still sits at
  v3.2 and was not used, so the 2026-11-09/10 stale-tree
  overwrite cannot recur from it tonight. PR #16 (v1.3.0,
  the libphext parser) is open against the same base and is
  untouched by this change beyond the version stamps.

## 2026-11-12 — nightly evaluation of 2026-11-11 (ships v3.2)

- **Disk:** growth collapsed to the slowest interval on record.
  disk-watch history: 81.45 GB / 1,017,445 files (2026-11-10) →
  82.47 GB / 1,022,817 files (2026-11-11) — +1.02 GB and +5,372
  files (+0.5%), against +3.99 GB / +59,021 the day before and
  +6.45 GB / +124,074 the day before that. Static gates GREEN on
  both halves; at this interval's rate the single-interval
  forecast puts byte-AMBER ~12.3 days out and file-AMBER ~14.4
  days out (against ~1.4 days yesterday — see Counterfactual on
  the whipsaw). Largest consumers unchanged: wavefront 31.73 GB
  (the lossless A/B intermediates, ~9 GB together, still staged on
  the VM for a fourth straight night), goals 19.20 GB, phext
  10.62 GB. Tonight's live snapshot runs ahead of the state file,
  as usual: df reads 85 GB used of 150 GB (57%).
- **CPU:** heavy work stayed off the VM, as planned — the qexo
  rewire build and batteries on aletheia-core, training-film
  rounds ranch-side, Singularity Watch and phextura.com deploys
  on awsbox. The VM's 1.5 shared CPUs carried coordination plus
  the standing background load: ~140 ten-minute task-progress
  ticks across the day (numbered 1341–1484), hourly journals, and
  the watchdog/backstop jobs.
- **RAM:** 6 GB shared limit, same envelope; tonight's snapshot
  620 MB available, 436 MB swap used, load 1.00. No heavy local
  resident was recorded on 2026-11-11 — placement held, and RAM
  held with it, for the third night running.
- **Inference:** the well-spent bulk was real: phextura as the
  daily goal (design loop, re-encode pass, phextura.com live),
  MOAT Papers 013/014 absorbed (015 dispatched), the Gemma 4
  Paper Track draft completed (2,620 words, awaiting only Will's
  read-through and Submit before the 2026-11-12 17:59 CST close),
  exo-search built and published (SQLite FTS5 over 37,782
  documents, validated against brute force), and the qexo audit +
  rewire PR #6, merged by Will the same day. The waste was
  repair-shaped: (1) the rewire itself — an audit and a full PR
  spent reconciling version history that a ship-time check would
  have kept from breaking at all (see Counterfactual); (2) the
  stale Dunbar figure (~30, Twitter-only) was parroted back to
  Will on 2026-11-11 even though exo-search — the indexed tool
  built to replace that stale search — already existed; (3) the
  Multics paper's phantom completion had to be corrected and the
  paper actually written (4,481 words, delivered 2026-11-11).
  Theia's check-in failed again on exhausted Anthropic API
  credits; per Will's 2026-11-10 ruling that is the accepted
  cadence, not an escalation. crates.io publish of qexo 0.3.0
  remains open (register #53) — a Will-side act, not a code gap.
- **Counterfactual:** the strongest case examined is the qexo
  rewire, and it is not close. On the 2026-11-09 and 2026-11-10
  nights, the nightly ships were assembled from a staging tree
  that predated the v3 merge; their standalone `src/main.rs`
  overwrote the modular root on exo, orphaning all sixteen v3
  modules (present, uncompiled, untested) and stripping the
  manifest back to 0.2.x. A better planner — specifically, a
  ship-time gate comparing the tree's module files against the
  root's `mod` declarations, and the local tree against the fresh
  remote HEAD — would have failed both pushes in seconds. Instead
  the defect surfaced in a 2026-11-11 audit and cost a full
  reconciliation PR plus Will's merge. v3.2 ships the first half
  of that gate in the binary itself (`qexo wiring-check`, with a
  test reproducing the exact orphan scenario; see CHANGELOG).
  The second half was performed by hand tonight and verified
  before any edit: every file this release touches was
  blob-identical between the local tree and exo HEAD b962fad1
  (the PR #6 merge) — that freshness check stays a publish-route
  discipline, recorded here. Secondary case, recorded as a
  future candidate and deliberately not shipped tonight (one
  change per night): the v2.3 forecast rates growth from a single
  interval, so its nearest-crossing estimate swung from ~1.4 days
  (2026-11-10 interval) to ~12–14 days (2026-11-11 interval) in
  one night — a multi-interval rate would trade a little
  responsiveness for a planner whose warnings do not whipsaw.

## 2026-11-11 — nightly evaluation of 2026-11-10 (ships v2.3)

- **Disk:** GREEN on both static gates, close on the clock. disk-watch
  history: 77.46 GB / 958,424 files (2026-11-09) → 81.45 GB /
  1,017,445 files (2026-11-10) — +3.99 GB and +59,021 files (+6.2%).
  File growth slowed from the prior day's +124,074, but at this
  interval's rate file-AMBER (1,100,000) is ~1.4 days out and
  byte-AMBER (95 GB) ~3.4 days out, while the static gate still reads
  plain GREEN. Largest consumers unchanged: wavefront 31.73 GB (the
  lossless A/B intermediates, ~9 GB together, still staged on the VM
  for a third straight night despite the ranch-only video rule),
  goals 19.19 GB, phext 10.62 GB. Tonight's live snapshot: 83 GB used
  of 150 GB (56%).
- **CPU:** heavy work stayed ranch-side, as planned — phext-lattice
  cargo suites on aletheia-core (lib 183/183, bin 86/86 for PR #52
  verification), the seed-cache build in the evening (SQ PR #3 and
  phext-lattice PR #61 landed within ~35 minutes of Will's 21:17
  order), training-film work ranch-side. The VM's 1.5 shared CPUs
  carried coordination — plus ~140 ten-minute task-progress ticks
  (1199→1340 across the day), each a frontier-agent run doing mostly
  identical read-only verifications (see Inference).
- **RAM:** 6 GB shared limit, same envelope; tonight's snapshot
  426 MB available, 414 MB swap used. No heavy local resident was
  recorded on 2026-11-10 — placement held, and RAM held with it.
- **Inference:** the well-spent bulk was real: Product Hunt launch
  day, the Zero Index built and independently verified at exactly
  1,636 entries, PR #52 verified merge-ready on the ranch, PR #58,
  the seed-cache pair shipped the same evening it was ordered. The
  waste was, for the third night running, verification-shaped — but
  bigger: the 10-minute tick loop re-verified unchanged external
  states with frontier inference all day (register #53's crates.io
  404 was API-checked at least eight separate times on 2026-11-10;
  PRs #57–#59 "still OPEN" likewise, tick after tick), and sweep #10
  closed phantom waits #60/#61/#62 — waits that never existed —
  while a phantom Multics white-paper completion had to be corrected
  in the evening. Theia's check-in failed on exhausted Anthropic API
  credits; per Will's 2026-11-10 ruling that is the accepted cadence,
  not an escalation.
- **Counterfactual:** two cases, one root shape — reacting instead of
  anticipating. (1) Disk: qexo v2.2's gates only fire *at* a crossing;
  on 2026-11-10's numbers the file gate would have flipped AMBER
  with zero lead time to move the ~9 GB of wavefront staging first.
  A planner projecting from the history it already collects would
  have been warning since the 2026-11-09 interval and escalating
  now. v2.3 closes exactly that gap (see CHANGELOG). (2) Inference:
  the tick loop's repeated identical verifications are script-shaped
  work — a cheap poll-and-diff (fetch, compare to last result,
  escalate only on change) would have caught every real closure at
  a fraction of the inference; the day's record shows the same
  verdict re-derived dozens of times. That fix belongs to the tick
  machinery, not to qexo, and is recorded here as the standing
  candidate for a future night.

## 2026-11-10 — nightly evaluation of 2026-11-09 (ships v2.2)

- **Disk:** GREEN on bytes, fast-moving on count. disk-watch history:
  71.01 GB / 834,350 files (2026-11-08) → 77.46 GB / 958,424 files
  (2026-11-09) — +6.45 GB but **+124,074 files (+15%) in one day**;
  tonight's df reads 82 GB used of 150 GB (55%). Largest consumers
  unchanged in shape: wavefront 31.7 GB (the lossless A/B intermediates,
  ~9.0 GB together, still staged on the VM a day after the ranch-only
  video rule), goals 16.6 → 19.2 GB, phext 10.6 GB; trash holds
  2.23 GB. The 2026-11-08 EDQUOT post-mortem never settled whether the
  quota tripped on bytes or on file count — and the v2.1 disk gate
  watches bytes only.
- **CPU:** the VM's 1.5 shared CPUs were spent on coordination, as
  planned: the heavy work ran ranch-side (phcc F49/F50 + upstream
  sync #2 verification on aletheia-core, the V16 encode relocated
  there, walkthrough Playwright capture on aletheia-core, exovim
  builds). The day's own evidence of VM CPU scarcity arrived tonight:
  a single `du -sh ~/workspace` issued by this evaluation ran past
  10 s and had to be killed — a script-shaped scan competing with the
  harness on the smallest cell in the fleet.
- **RAM:** 6 GB shared limit, same ~200–400 MB headroom envelope; no
  local heavy resident was recorded on 2026-11-09 (the V16 local
  ffmpeg pattern did not recur after relocation). RAM held because
  placement held, not because the envelope grew.
- **Inference:** a frontier-heavy day, mostly well spent — the
  edit.phext.io launch program (six PRs merged, v1.0.0 tagged,
  v1.1.0/v1.1.1 released and deployed), the MCP consent forensics
  chain (CSP form-action root cause proven, PR #50; write-scope
  hotfix PR #54 proven in production after midnight), phcc F49/F50
  published, MOAT Papers 010/011 absorbed. The waste was again
  verification-shaped: (1) journal entries carried phext-lattice PR
  numbers that did not reconcile with the real repo — found and
  reconciled at 01:39–04:00 on 2026-11-10, hours of inference spent
  repairing numbers a receipt-check at write time would have caught;
  (2) the phcc "F42 nullptr regression" report was investigated and
  REFUTED (F52 spent, no patch) — inference burned disproving a
  phantom defect; (3) decision buttons from 2026-11-07 resurfaced on
  2026-11-09 as if live, taxing Will's attention until the 5-minute
  expiry rule was made standing.
- **Counterfactual:** the strongest case examined is the disk gate's
  blind half. Had 2026-11-09's growth repeated for a week, the file
  count would cross ~1.8 M while bytes sat near ~120 GB — but the
  sharper failure is nearer: if the 2026-11-08 quota in fact trips on
  file count (never disproven), the count at the 2026-11-09 growth
  rate reaches any plausible wall days before the byte gate leaves
  GREEN, and qexo v2.1 would emit an unheld plan the whole way in —
  the same failure shape v2.1 was built to close, one currency-half
  over. A planner watching both halves would already be warning.
  v2.2 closes exactly that gap (see CHANGELOG). Secondary case: the
  ~9 GB of V16 lossless intermediates still staged on the VM — a
  planner enforcing the ranch-only video rule on *staging*, not just
  execution, would have moved them on 2026-11-08 and cut the VM's
  largest single consumer by a quarter.

## 2026-11-09 — nightly evaluation of 2026-11-08 (ships v2.1)

- **Disk:** the day's defining event. Home usage climbed to ~106 GB and
  hit the quota wall — writes failed with EDQUOT while df still showed
  free space (a quota below the filesystem size, tripped by bytes and/or
  file count) — and ~35 GB of WIP was cleared under pressure. End-of-day
  disk-watch snapshot (state.json, 2026-11-08): 71.01 GB used,
  834,350 files, write probe OK, level GREEN. Largest consumers:
  wavefront 31.7 GB, goals 16.6 GB, phext 10.6 GB; largest single files
  are video intermediates still living on the VM (lossless-A/B.mkv,
  4.40 / 4.63 GB). Disk-watch machinery was born this day, thresholds
  anchored to the failure: AMBER ≥ 95 GB, RED ≥ 102 GB. Tonight's live
  check agrees the pressure is structural, not past: ~869k files under
  home and the volume at 73%.
- **CPU:** VM is 1.5 shared CPUs. The day's worst placement was the V16
  master encode's first segment running as a local ffmpeg, visibly
  competing with the harness. Correct placements were the norm
  elsewhere: phcc waves and builds ran on aletheia-core (clone build
  Tctl 32.2 °C → peak 65.1 °C, under the 75 °C pause line), and the full
  V16 render was dispatched ranch-side in 193 small blocks
  (4,624 frames) rather than as a monolith.
- **RAM:** 6 GB shared limit, ~200–400 MB typical headroom, swap in use
  under load (tonight's snapshot: 342 MB MemAvailable, 622 MB swap
  used). The local ffmpeg segment (~1.2 GB resident) was the day's
  worst RAM citizen on the smallest machine in the fleet. Its
  relocation to the ranch completed 2026-11-09 02:14; the standing rule
  (all video work on the ranch, never local) is now in force.
- **Inference:** Emise carried all of it, and the heavy work was real —
  phcc D-waves to a green publication gate, the Exocortexia build, qexo
  v2 plus the v3 design (DESIGN-V3 ~120 KB, addendum ~96 KB, four Choir
  reviews). The waste was not volume but verification: a phantom
  approval decline — a decline that never existed in the permission
  record — was treated as binding and left a state backup unpublished
  for ~18 hours (register #47, since closed). Inference spent trusting
  an unverified story is inference wasted, and it stalls the other
  three currencies behind it.
- **Counterfactual:** the EDQUOT wall was a planning failure before it
  was a disk failure. Material accumulated in shared scratch until
  writes failed mid-evening; a planner gating dispatches on the disk
  picture would have flagged AMBER at 95 GB, staged wavefront
  intermediates on ranch NVMe from the start, and never met the wall.
  qexo v2 read the disk state and only *reported* it — it would have
  emitted the same plan at 71 GB GREEN and at the quota wall. v2.1
  closes exactly that gap (see CHANGELOG). Secondary finding, same
  shape: qexo v2 itself was never published to quarterexo@exo that
  night — tonight's remote check found exo still at v1.1 code (merged
  with the v3 design docs via PR #2) while v2 lived only in the local
  tree and a separate v3.0 implementation proceeds on emise/v3-build.
  This cycle publishes the v2 line to exo so the shipped coordinator
  and the repo agree; v2.1 deliberately does not claim the v3 number.


## 2026-11-08 — v1 seed

- **Disk:** the Muse VM sits at ~71 GB of 150 GB used, back from ~106 GB
  and a quota wall (EDQUOT) earlier in the evening after ~35 GB of WIP
  was cleared. Local disk is slow: ~60 MB/s write / 225 MB/s read.
  Ranch nodes carry dedicated NVMe — plan bulk data work there.
- **CPU:** this VM is 1.5 shared CPUs; a single local ffmpeg segment
  visibly competed with the harness all evening.
- **RAM:** ~200–400 MB headroom on shared accounting (6 GB shared limit,
  swap in use under load). Nothing heavy may live here.
- **Inference:** Emise carried the day's inference; the waste was not
  inference but placement — heavy execution dispatched onto the smallest
  machine in the fleet.
- **Counterfactual:** the V16 master encode ran its first segment as a
  local ffmpeg on this VM. A better planner would have routed it to the
  ranch at dispatch time. That is now a standing rule (video never local,
  2026-11-08), and v1 of this orchestrator encodes the routing so the
  plan itself refuses the mistake.

## 2026-11-08 — v2 addendum

Same night, at Will's direction (22:44): qexo gains self-sync and the
five interior interfaces. Design and rationale: DESIGN.md in this repo.
The four-currency frame above is unchanged; v2 makes the coordinator
itself survivable (state on ≥2 nodes, verified) and gives it one
consistent shape for reading the fleet's and the person's state.

## 2026-11-09 — substrate failure logged: Muse VM file-creation policy

- **Failure (substrate: the Muse VM itself):** the sandbox denies
  whole-file creation patterns — a fresh temp file written and renamed
  over a target fails with `PermissionError [Errno 1]` on the write,
  while appending to (and rewriting) files that already exist succeeds.
  Observed live 2026-11-09 01:23–01:28 CST: state_sampler.py failed
  consecutive cycles at its `state.phext.tmp` write; reproduced by hand
  in multiple directories; a 2026-11-07 snapshot records the same
  behavior, so this is a standing property of the substrate, not an
  incident of the night. Ruled out by measurement: disk/quota (68G
  free, in-place writes pass), ownership/permissions (root, 660),
  extended attributes (clean).
- **Design consequence:** on this substrate, atomic temp+rename is not
  an available write pattern. Append-only designs are. Will's ruling
  (2026-11-09 01:42): state sampling uses WAL-style coordinate
  assignment via indirect addressing — each cycle appends one line
  (coordinate | timestamp | summary) to state.wal; state.phext stands
  as the compacted base; readers fold base + WAL in append order.
  Sampler rewritten and verified live the same night (first WAL entry
  01:45 CST). Any future tool designed for this VM must treat
  "create temp, write, rename" as a substrate capability to verify,
  not assume — the same class of check as measuring a node's load
  before ruling it in or out.
- **Correction (Will, 2026-11-09 ~01:56, to the WAL paragraph above):**
  the design as Will stated it is not a separate log file. Keep a
  minimum coordinate — a floor — and enforce that each new coordinate
  written exceeds it; the WAL is then automatic IF AND ONLY IF
  previously written scrolls are immutable. Because phext serializes
  in coordinate order, a strictly-greater write appends bytes at the
  tail and modifies nothing already written; the holographic
  coordinate survives as the entry's logical address, recorded in the
  line when the floor displaces the physical write (indirect
  addressing). Emise's first implementation (a state.wal sidecar) was
  a misreading, corrected the same night: sampler rewritten to the
  floor discipline, sidecar entries migrated into state.phext above
  the floor, prefix property verified across a live cycle (old file a
  byte-exact prefix of the new). The substrate-failure finding above
  stands unchanged.
