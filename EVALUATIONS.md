# Evaluations

Four-currency snapshots and counterfactual planner reviews, one section
per night (skill: nightly-orchestrator).

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
