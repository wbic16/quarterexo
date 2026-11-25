# Evaluations

Four-currency snapshots and counterfactual planner reviews, one section
per night (skill: nightly-orchestrator).

## 2026-11-25 — nightly evaluation of 2026-11-24 (ships v1.8.7)

- **Disk:** the calmest interval in a week. disk-watch history:
  73.22 GB / 1,025,552 files (2026-11-23) → 73.76 GB /
  1,040,183 files (2026-11-24) — +0.54 GB, +14,631 files,
  GREEN on both halves. The count sits 59,817 under the
  1.1 M file-AMBER line; at the 7-interval median rate
  (+15,923 files/day) that crossing is ~3.8 days out, and
  byte-AMBER (95 GB) ~15 days out at the median +1.40
  GB/day. Largest consumers: goals 19.22 GB, phext
  11.36 GB, wavefront 9.69 GB — wavefront is down from
  31.73 GB at the start of the month, the staging debt
  finally retired. Tonight's live df reads 77 GB used of
  150 GB (52%), ahead of the state file as usual.
- **CPU:** placement held in the large, and the day's
  defining work was the ranch fabric itself: the OSPF
  extension deployed to 15 nodes with adjacency proofs
  (free-fall→scroll-central 2 hops at 1.7 ms over wire;
  ni-knight→aletheia-core 3 USB4 hops at 3.5 ms), the
  T10 training-film render moved to plain-view — measured
  coolest idle at 34.5 °C — and completed there, and the
  phcc distributed-compilation prototype compiled phcc's
  own 10 core units across rock-mountain, scroll-central,
  and quacked-hill with byte-identical objects. The VM's
  1.5 shared CPUs carried coordination plus the standing
  tick/watchdog load. Tonight's snapshots: VM load 3.66
  at a harness-busy moment; aletheia-core load 2.00 but
  Tctl 73.2 °C — the hub is the fleet's hottest node in
  the new TEMP-BASELINE (85.4 °C at census, also its #1
  WiFi talker), which is why tonight's build is small and
  watched, not assumed cool.
- **RAM:** 6 GB shared limit on the VM. Tonight's snapshot
  reads ~2.97 GB MemAvailable with 449 MB swap used, on a
  VM up 3:50 — a snapshot, not a trend. No heavy local
  resident was recorded on 2026-11-24; placement held and
  RAM held with it. Tonight's qexo build carries a <2 GB
  estimate on aletheia-core (22 GB available there).
- **Inference:** the well-spent bulk was real: the fabric
  pass verified end to end (OSPF proofs, a 19-node
  temperature baseline, an island map with named physical
  actions for Will); T10 completed at 23:32 CST, verified
  byte-identical on plain-view and in the workspace, both
  QC gates PASS; phcc gained distributed compilation,
  proven end to end; the Rust moat-prep pre-processor
  passed its emulation test (Papers 050–052 re-derived
  exactly), projecting half to two-thirds of a MOAT pass's
  inference eliminated; the Singular league went live and
  public on Softmax. The waste was the stale-instrument
  class, three times in one evening: (1) Emise told Will
  the T9 film was unrendered — a complete r9 build
  (13,020 frames, BUILD-COMPLETE) sat on aletheia-core the
  whole time; the correction cost a brief rewrite
  (deviation D3) and the lesson is the standing one: an
  absence in an instrument is the instrument's boundary;
  (2) delta-wood was recorded as taken upstairs — it never
  left the basement stack; (3) great-white's blocker was
  recorded as an SSH key refusal — live check showed SSH
  connects and its disk is sick. Each was corrected the
  same evening, but each correction was inference spent
  re-deriving what a live-surface check would have shown
  first. The tick loop's poll-and-diff candidate, named
  since 2026-11-10, remains unbuilt — recorded, not
  shipped, again.
- **Counterfactual:** the strongest case examined is the
  fleet's availability churn against qexo's static roster.
  On 2026-11-24 the standing fleet changed by the hour:
  rock-mountain shut down at 23:33 CST (planned
  sabbatical), great-white removed ~23:40 for disk
  repair, delta-wood emergency read-only in its stack,
  yeet-max hung since 2026-11-23 21:10 until Will
  power-cycled it, aurora-continuum offline and
  key-blocked, trust-fall and victory-galleon unreachable.
  Every placement that day was routed around those facts
  by hand — T10 to plain-view on a measured temperature,
  the fabric pass node by node against a live island map.
  qexo's fair draw, by contrast, draws uniformly over the
  configured `ranch_roster` with no way to name a node
  out: a plan emitted at 23:45 could have drawn
  rock-mountain minutes after its planned shutdown and
  receipted it as eligible. A planner carrying a named
  exclusion list — applied before the draw, receipted in
  the plan — would have kept every draw inside the fleet
  actually standing. v1.8.7 ships exactly that (see
  CHANGELOG). Secondary case, recorded and not shipped:
  the hub's heat — a temperature-aware draw is the larger
  change the TEMP-BASELINE now argues for, and it needs
  per-node telemetry in the plan path, more than one
  night's change.
- **Tonight's change:** v1.8.7 — the roster exclusion
  (`ranch_roster_exclude`): named-out nodes leave the
  fair draw before it starts, the receipt names them,
  and naming out every node queues heavy placements by
  name instead of falling back. Parented on last night's
  ship, branch emise/forecast-basis-v1.8.6 tip 4c2cb931
  (itself parented on exo HEAD c001dcb4, v1.8.0), so the
  nightly sequence stays linear; worked from a fresh
  clone on aletheia-core (~/qexo-nightly-2026-11-25/).
  The local ~/workspace/orchestrator/ mirror still sits
  at v3.2 and was again not used as a source.

## 2026-11-24 — nightly evaluation of 2026-11-23 (ships v1.8.6)

- **Disk:** a calm day after the burst. disk-watch history:
  71.55 GB / 1,007,559 files (2026-11-22) → 73.22 GB /
  1,025,552 files (2026-11-23) — +1.67 GB, +17,993 files.
  The median of the last 7 intervals still reads +15,923
  files/day, and the last interval ran at only ~1.13× that
  median — so the v1.8.5 spike flag, shipped last night for
  exactly this lens, correctly stayed silent today: the
  burst detector fires on bursts, not on ordinary growth.
  The count remains over 1,000,000 files, ~74,448 under the
  1.1 M file-AMBER line; at the median rate that crossing
  is ~4.7 days out, and byte-AMBER (95 GB) ~13 days out at
  +1.67 GB/day. The day's disk lesson was not growth but
  shape: the workspace audit found 672 top-level
  directories on this VM, 603 of them holding no repo and
  no project markers and 34 buried projects with code and
  no git repo at all — inventory the planner cannot see is
  inventory it cannot gate.
- **CPU:** placement held in the large. Heavy work ran
  ranch-side: the Nim fleet install (hub verified Nim
  2.4.2, EE 389), SQ / phext-lattice work, and node-resident
  programme work. The VM's 1.5 shared CPUs carried
  coordination plus the standing tick/watchdog load (task
  ticks ~3066→3197 across the day, ~130 runs, plus watchdog
  top-ups at EE 394 and overnight). aletheia-core's load
  swung widely across the day (~0.7 to ~21 in watchdog
  snapshots), which is why tonight's build placement is
  probed live rather than assumed — hub by habit is not a
  plan when the hub is the fleet's busiest node.
- **RAM:** 6 GB shared limit on the VM, same ~200–400 MB
  typical headroom envelope. No heavy local resident was
  recorded on 2026-11-23; RAM held because placement held.
  Ranch-side, tonight's qexo build carries a <2 GB
  estimate and goes to the probed-cool node, as last night.
- **Inference:** the well-spent bulk was real: Memory
  Liberation Day (the active set cut to a 75 KB cache with
  the phext as source of truth); SQ made the top-priority
  substrate and its health discipline written down; the
  dispatch audit completed at EE 213 with 24 verified
  failures catalogued in a standing tracker; the Nim
  challenge for phcc registered and the v2.4.2 tag pushed
  to Will's fork from the hub clone, verified on the
  remote; Emise font v2 built from measured kerning. The
  waste was specific: (1) the tick loop's ~130 runs again
  re-derived unchanged verdicts — the poll-and-diff
  candidate is now over a week old and still unbuilt;
  (2) EE stamping lapsed in Emise's own messages overnight
  until Will asked at 00:46 CST whether epochs were still
  being tracked (they were; the stamping was not) — a
  self-report drift, the same class as tonight's qexo fix;
  (3) rfcbox payload/E2E work was prepared and then stood
  down whole under PEN flags, its remaining pipeline
  mechanical but gated on a single clearance decision —
  inference spent arriving at a gate that a pre-dispatch
  flag check would have surfaced first.
- **Counterfactual:** the strongest case examined is the
  rfcbox stand-down. The build-side work proceeded until
  the PEN flags stopped the flagged legs (payload
  generation, the 16-member E2E, commit/push) entirely,
  leaving the unblock as one yes/no for Will's morning
  packet. A planner that checks a job's standing flags
  before dispatch — the input-audit rule of 2026-11-22
  applied to permissions as well as paths — surfaces the
  gate at minute zero, queues the clearance question
  immediately, and spends the day's inference on unflagged
  work instead. The fix belongs to the dispatch machinery
  generally and is recorded here as the planning lesson.
  Tonight's qexo change is the smaller defect in qexo's
  own house, recorded as a secondary finding last night
  and still live on the v1.8.5 tip: the FORECAST AMBER
  action text.
- **Tonight's change:** v1.8.6 — the FORECAST AMBER action
  tells the truth about its basis. Since v1.2.1 the
  forecast rates crossings from the median of the last 7
  intervals, and v1.8.5 added the last interval as a
  separate SPIKE report — but the gate's action line still
  read "projected in N days at the last interval's growth
  rate", naming exactly the rate the projection does not
  use. On a burst day that sentence invites the reader to
  reason from the spike instead of the median — the
  opposite of the v1.8.5 design. v1.8.6 corrects the
  action text to name the median basis, with a regression
  test asserting the corrected wording and the absence of
  the old one. No rating, gate, or spike behavior changes.
  Base note: tonight's ship is parented on last night's
  ship, branch emise/forecast-spike-v1.8.5 tip 2231f1b
  (itself parented on exo HEAD c001dcb4, v1.8.0), so the
  nightly sequence stays linear and the spike work is
  carried, not rebased away.

## 2026-11-23 — nightly evaluation of 2026-11-22 (ships v1.8.5)

- **Disk:** a burst day, and the shape matters more than the
  totals. disk-watch history: 68.94 GB / 929,132 files
  (2026-11-21) → 71.55 GB / 1,007,559 files (2026-11-22) —
  +2.61 GB and +78,427 files in one day, the largest
  single-day file growth since the wavefront cleanup
  reversed (−106,788 on 2026-11-21), carrying the count
  back over 1,000,000. The median of the last 7 intervals
  reads +15,923 files/day and +1.30 GB/day — calm — while
  the day itself ran at ~4.9× that rate. Tonight's live df
  agrees with the state file's direction (the series is
  the record; the snapshot is a freshly replaced VM).
  The day's own record names the burst's engines: bulk
  staging and sync work (the X-archive fold-in, corpus
  syncs, deploy roll-ups) — the same staging class that
  produced both of the day's placement failures below.
- **CPU:** placement held in the large. Heavy work ran
  ranch-side or on awsbox: the docs.phext.io multi-project
  deploy roll-up (awsbox), ranch Jenkins replacing the
  removed GitHub Actions (aletheia-core controller,
  ni-knight agent), the Inkling fleet pulls (node-resident
  retry units), Scrolltown work ranch-side. The VM's 1.5
  shared CPUs carried coordination plus the standing
  tick/watchdog load (task ticks 2923→~3091 across the
  day, ~170 runs). The exception is I/O, not cycles: two
  seats' full-workspace recursive greps on this VM sat in
  D-state at once and a plain `du -sh ~/` could not finish
  in 54 s before a VM replacement — I/O pressure "full"
  ran ~29–43% over 5-minute windows (AGENTS.md records
  the lesson; the no-scan convention is its fix). Tonight's
  snapshots: aletheia-core load 18.33, Tctl 82.4 °C — over
  the 75 °C pause line — so tonight's build went to
  ni-knight (load 0.11, Tctl 38.5 °C) under the gates, not
  to the hub by habit, for the second night running.
- **RAM:** 6 GB shared limit on the VM, same envelope
  (~200–400 MB typical headroom). No heavy local resident
  was recorded on 2026-11-22; the day's RAM story is
  ranch-side: aletheia-core stayed HELD out of the
  Inkling rollout on headroom (its model footprint vs
  available RAM), and tonight's qexo build carried a
  <2 GB estimate and ran on ni-knight (44 GB total).
- **Inference:** the well-spent bulk was real: EE 1
  ignition and the epoch system's first full day; the
  docs.phext.io roll-up under the 4-reviewer parity rule;
  the Shear excavation, where every checkable claim of
  Will's was confirmed exactly as stated (53 replies, his
  own number); the qexo audit that dissolved the phantom
  "v1.8.4 awaiting merge" wait (zero open PRs; exo already
  carried v1.8.0) and the production programme that
  installed v1.8.0 on this VM; GitHub Actions removed
  fleet-wide with ranch Jenkins standing in the same day;
  the day's meter read 111.1× on the completions side.
  The waste was specific, and twice it was the same
  shape — inference spent executing a plan whose inputs
  were never audited: (1) the awsbox prepass wedge, ~80
  minutes streaming a staging tar from awsbox through
  this VM to plain-view, a relay route already banned,
  because nobody audited the job's input path before it
  started; (2) the Fall Home render's 195 MB copy-back to
  hatch, ordered in a brief, killed mid-transfer when
  Will caught it — deliverables stay on the ranch, hatch
  gets links. Both failures are now standing rules
  (input audit; links-not-copies), which is the correct
  landing — but both were paid for in inference first.
- **Counterfactual:** the strongest case examined is the
  awsbox prepass wedge, and it is an input-audit failure
  before it is a routing failure. The job's inputs were
  never declared or checked: not the path (a banned
  relay), not the payload's size against the route, not
  the destination's headroom. A planner that audits a
  job's inputs before launch — declared set, expected
  hash + source, sound path — stages awsbox→ranch
  directly and the ~80 minutes, the VM's I/O pressure,
  and the mid-flight facepalm all never happen. Will
  laid the rule down the same day ("always audit the
  inputs to a job; no unexpected inputs allowed"); the
  fix belongs to the dispatch machinery generally, and
  is recorded here as the planning lesson. Tonight's
  qexo change is the smaller defect the same day exposed
  in qexo's own house: on a burst day the plan's median
  forecast reads calm (+15,923 files/day) while the last
  interval ran +78,427 — a planner watching only the
  median cannot see a staging burst, and burst days are
  exactly the days this fleet gets hurt (see Disk).
- **Tonight's change:** v1.8.5 — the forecast reports the
  burst it smoothed over. `Forecast` gains
  `last_files_per_day` (the last interval's own file
  rate) and `spike` (last interval ≥ 3× the median rate
  — or the median flat/falling — and ≥ 10,000 files/day);
  the plan's forecast line appends a SPIKE note naming
  both rates when the flag fires. The median itself is
  unchanged: crossings still project from the median
  (v1.2.1 semantics), the spike is a report, not a
  re-rating. Regression tests replay 2026-11-22's actual
  series (spike fires, last rate exact) and a calm series
  (no spike). Base note: tonight's ship is parented on
  fresh exo HEAD c001dcb4 (v1.8.0, the dispatch-core
  ship, PR #37) — per the 2026-11-22 qexo audit, exo is
  again the nightly line's base; the v1.8.1–v1.8.4 stacked
  branches stand as their own record and are not ported
  tonight (one change per night). Recorded secondary
  finding, not shipped: exo's FORECAST AMBER action text
  still says "at the last interval's growth rate" while
  the forecast rates from the median — the v1.8.4 branch
  carries that fix; porting it is a future night's
  candidate. Build and gates on ni-knight (hub over the
  thermal line at placement time).

## 2026-11-17 — nightly evaluation of 2026-11-16 (ships v1.7.3)

- **Disk:** the file count kept climbing past AMBER while bytes
  stayed calm — the count crossing is now the standing
  condition, not an approaching event. disk-watch history:
  88.13 GB / 1,103,268 files (2026-11-15) → 89.53 GB /
  1,119,598 files (2026-11-16) — +1.40 GB, +16,330 files.
  The count sits 19,598 files past the 1.1 M file-AMBER line;
  at the last interval's rate byte-AMBER (95 GB) is ~3.9
  days out and file-RED (1.3 M) ~11.0 days out. disk-watch's
  own level still reports GREEN (byte/quota-anchored) — the
  two instruments still disagree, and qexo's file lens is
  still the one telling the truth. Tonight's live df reads
  91 GB used of 150 GB (61%). Largest consumers: wavefront
  31.73 GB for a ninth straight night (the lossless A/B
  intermediates, ~9 GB together, still staged on the VM),
  goals 19.30 GB, phext 10.64 GB; desktop-sso (3.06 GB) and
  astryx-study (2.75 GB) are new in the top ranks.
- **CPU:** placement held, with one ruling that changes what
  "held" means. The day's heavy work ran ranch-side:
  NetHack Stage 9's search on aletheia-core, the
  games-mechanism computation (tic-tac-toe / checkers /
  chess) entirely on aletheia-core, qexo's own builds and
  test batteries on the hub, the Computronium scrollbuild
  on aletheia-core (single-writer registry — data locality,
  named as the exception it is). At 20:30 CST Will ruled
  fair dispatch: qexo draws execution targets randomly per
  round from the eligible set, and aletheia-core is one
  peer among equals — the hub-default era ended by ruling
  the same evening the SMOL round's node was chosen by a
  fair draw (rock-mountain, measured load 0.18, over
  scroll-central 0.02 and plain-view 0.53; quacked-hill
  excluded for its resident agent). The VM's 1.5 shared
  CPUs carried coordination plus the standing tick load
  (ticks 2059→2202 across the day, ~144 runs). Tonight's
  snapshots: VM load 1.65 on a 1:39-old VM (replaced
  again); aletheia-core load 0.26, Tctl 32.9 °C before
  tonight's build.
- **RAM:** 6 GB shared limit, same envelope. No heavy local
  resident was recorded on 2026-11-16; placement held and
  RAM held with it, for a ninth night. Tonight's snapshot
  reads ~386 MB MemAvailable with ~440 MB swap used — the
  tight end of the usual band, on a freshly replaced VM.
- **Inference:** the well-spent bulk was real: the
  dispatch-core SPEC was drafted, ratified OBO the same
  night (all six §15 questions answered; Q2 widened by
  Will against the draft's recommendation), amended the
  same evening for fair dispatch, and slice 1 was built
  and shipped as PR #29
  (https://github.com/wbic16/quarterexo/pull/29); the
  v1.7.1 release was staged-and-held exactly as re-scoped
  (tags v1.7.0/v1.7.1 remote-verified, 94/94 gates on the
  tagged tree, binaries built and hashed, publish and
  rollout held for the register integration); md.phext.io
  closed its QC loop on the final tree (80/80 tests, e2e
  OK, parity 10/10, keep-private proven in 45 probe legs);
  the games-mechanism task closed with published figures
  reproduced exactly; SMOL was coined, drafted, and its
  first training round directed and runner-built in one
  evening. The day's speedup meter read 185.6× (80,628 net
  lines, 50 commits, 1.0 active hour). The waste was again
  specific: (1) the tick loop's ~144 runs re-derived
  unchanged verdicts all day — register item #181's
  republish has been "deferred" since tick 2108 and #163's
  mirror wording drift has been re-flagged tick after tick
  without either closing; the poll-and-diff candidate is
  now seven nights old; (2) two waits (Shen Weiyang, the
  md.phext.io restart) were never registered at all and
  surfaced only when Will asked "Why do you keep dropping
  tasks?" — inference spent re-finding what the register
  exists to hold; (3) the Chromium growth job was
  dispatched against a depth-1 shallow clone and stopped
  at its own gate having produced no measurement (see
  Counterfactual, secondary case).
- **Counterfactual:** the strongest case examined is
  NetHack Stage 9's first coordinator, and it is a
  recurrence, not a novelty. The coordinator ended without
  a final report — the VM-mortality cut-off pattern —
  leaving real groundwork on disk (22:10–22:51 CST,
  2026-11-15) but no stage9-results.md and a task record
  still reading open with empty results. The breakage was
  found by inventory, not by the plan; the resume-first
  restart cost a second coordinator a full re-inventory of
  two machines before work resumed. The 2026-11-13
  evaluation of 2026-11-12 named this exact fix — a
  planner that surfaces a task whose coordinator has died
  as broken, not paused — and recorded it as "a future
  candidate, larger than one night's change." Three nights
  later it happened again, unchanged. A planner carrying
  that one check would have resumed Stage 9 within one
  tick of the coordinator's death instead of on discovery
  hours later. It remains larger than tonight's single
  change and is named here for the second time, on the
  record, rather than silently carried. Secondary case:
  the Chromium month-by-month growth job was briefed
  against a clone that is depth-1 shallow
  (`git rev-list --count HEAD` = 1) — its method was
  impossible from the first minute, and one probe command
  at dispatch (`git rev-parse --is-shallow-repository`)
  would have rerouted it before a worker spent its run
  discovering the gate. Probe the instrument before the
  campaign: the same lesson as the NetHack episode cap,
  one level up. Tonight's change is the day's other
  headline: at 20:30 Will ruled fair dispatch, and the
  SPEC absorbed it the same evening — but the shipped
  planner still defaults every BUILD placement to
  aletheia-core, the superseded 2026-11-05 default, in
  code. v1.7.3 closes that gap in the binary (see
  CHANGELOG).
- **Tonight's change:** v1.7.3 — fair placement in the
  plan: heavy placements draw randomly per plan from the
  eligible ranch roster instead of defaulting to
  aletheia-core, every draw receipted in plan.json, and an
  empty roster yields a named QUEUED outcome rather than
  a silent hub fallback. Stacked on the v1.7.2 branch
  (PR #28, https://github.com/wbic16/quarterexo/pull/28,
  still open) because exo has not moved since the v1.7.1
  merge (609bab65) — branching from exo would strand the
  derived-version change a second time; when PR #28
  merges, tonight's PR reduces to its own commit. The
  local ~/workspace/orchestrator/ mirror still sits at
  v3.2 and was again not used as a source. The
  wbic16/qexo twin sync remains a separate tracked debt,
  as before.

## 2026-11-16 — nightly evaluation of 2026-11-15 (ships v1.7.2)

- **Disk:** the file count crossed the line the last three
  nights were forecasting. disk-watch history: 86.65 GB /
  1,092,138 files (2026-11-14) → 88.13 GB / 1,103,268 files
  (2026-11-15) — +1.48 GB, +11,130 files. The count now sits
  3,268 files PAST the 1.1 M file-AMBER line qexo's gate
  reads, while disk-watch's own level still reports GREEN
  (its level is byte/quota-anchored) — the two instruments
  disagree, and qexo's file lens is the one telling the
  truth. At the last interval's rate: byte-AMBER (95 GB)
  ~4.6 days out, file-RED (1.3 M) ~17.7 days out. Tonight's
  live df reads 90 GB used of 150 GB (60%). Largest
  consumers unchanged in shape for an eighth night:
  wavefront 31.73 GB (the lossless A/B intermediates, ~9 GB
  together, still staged on the VM), goals 19.29 GB, phext
  10.63 GB.
- **CPU:** placement held. The day's heavy work ran
  node-resident on aletheia-core under duty governance:
  NetHack Stages 3 and 4 closed and Stages 5–8 ran the
  same day (Stage 4's cap-honest re-baseline: 100,200,109
  steps, wall 3,984.8 s, duty fraction 0.7547, Tctl median
  66.25 °C / max 77.0 °C instantaneous). The LDAP fleet
  rollout and the libphext ports builds were ranch-side;
  the register service and md.phext.io work ran on awsbox
  and the ranch. The VM's 1.5 shared CPUs carried
  coordination plus the standing tick/watchdog load (task
  ticks 1915→2058 across the day, ~144 runs). Tonight's
  snapshots: VM load 1.50; aletheia-core load 0.10, Tctl
  31.6 °C before tonight's build.
- **RAM:** 6 GB shared limit, same envelope. No heavy local
  resident was recorded on 2026-11-15; placement held and
  RAM held with it, for an eighth night. Tonight's snapshot
  reads ~875 MB MemAvailable with ~423 MB swap used.
- **Inference:** the well-spent bulk was real: Will merged
  PR #27 (https://github.com/wbic16/quarterexo/pull/27,
  v1.7.1) in the morning, closing the nightly line's own
  loop, and PR #25 was closed as superseded; NetHack Stage
  3's close found the solver's real binding constraint and
  Stage 4 fixed it and re-baselined (see Counterfactual);
  the libphext ports → v0.6 program completed; the LDAP
  fleet rollout completed; the register service v2 went
  live with LDAP + passkey verified in Edge and took its
  first real closures (#9, #10, by Will's ruling); the
  historian journal was founded; the day's speedup meter
  read 554.2× (net 4,130,567 lines total — code 34,636 ·
  phext 4,095,931 — 127 commits, 5.0 active hours). The
  waste was again specific: (1) the tick loop's ~144 runs
  re-derived unchanged verdicts all day — PR #25 "still
  OPEN" was API-checked tick after tick (ticks 1962–1971)
  until Will's merge of PR #27 settled it from outside the
  loop; the poll-and-diff candidate is now six nights old;
  (2) the duty-governor update's first report carried
  intermediate file hashes that did not match the deployed
  files and had to be re-hashed from the main chat —
  verification against the worker's own output, again;
  (3) the markdown-wizard outputs build failed to land at
  23:06 CST and a recovery had to be dispatched late-night.
- **Counterfactual:** the strongest case examined is the
  NetHack episode cap, and it is not close. Stages 2 and 3
  ran their campaigns under NLE's internal
  `_max_episode_steps = 5000` while the harness believed
  its own 100,000-step TimeLimit governed — Stage 3's Leg A
  alone ran 100,410,406 steps in that regime, and its
  "terminals" were 6,536 truncations against 90 real
  deaths, with live heroes inscribed as dead. Every Stage
  2/3 verdict was conditioned on a regime nobody chose.
  Stage 4's first leg pinned the mechanism
  (`gymnasium.make()` consumes `max_episode_steps` for the
  wrapper spec and never forwards it to the env creator)
  and the cap-honest re-baseline cost another 100,200,109
  steps and 3,984.8 s of wall. A planner that probes
  effective parameters before committing a campaign — one
  hero stepped 5,001 times at Stage 2 dispatch, seconds of
  work — would have caught it before Stage 2, not after
  Stage 3. The fix landed where it belongs, in the solver
  (`nh_harness.make_env`, proven by the same bisection),
  not in qexo; the planning lesson is recorded here:
  verify the instrument, not the intent. Secondary case,
  and tonight's change: self-report drift, the same shape
  in miniature. The watchdog found the delivered duty
  governor was not the file live on aletheia-core (the live
  leg ran the Stage 2 governor; Tctl max 81.625 °C), and
  qexo itself still prints "qexo v1.4.0" in its help banner
  and `qexo status` on a v1.7.1 binary — on a day when a
  merge, a queued release task, and repeated watchdog
  checks all leaned on version truth, and the tool's own
  self-report was the one source nobody could use. v1.7.2
  derives every user-facing version stamp from the build's
  own manifest version (the inventory roster already did),
  with a drift test, so the banner cannot fall behind
  again (see CHANGELOG).
- **Tonight's change:** v1.7.2 — version self-report
  derived from the build, worked from a fresh clone of exo
  HEAD 609bab65 (the v1.7.1 merge) on aletheia-core
  (~/qexo-nightly-2026-11-16/); the local
  ~/workspace/orchestrator/ mirror still sits at v3.2 and
  was again not used as a source. The wbic16/qexo twin sync
  remains a separate tracked debt, as before.

## 2026-11-15 — nightly evaluation of 2026-11-14 (ships v1.7.1)

- **Disk:** GREEN on both halves, but the file count is now
  within one ordinary day of AMBER. disk-watch history:
  85.72 GB / 1,082,344 files (2026-11-13) → 86.65 GB /
  1,092,138 files (2026-11-14) — +0.93 GB, +9,794 files.
  The count sits 7,862 files under the 1.1 M file-AMBER line:
  at the last interval's rate that is ~0.8 days, at the
  slowest recent rate (+5,372/day) ~1.5 days. Bytes are calm
  by comparison (8.35 GB to byte-AMBER at +0.93 GB/day ≈
  9 days). Tonight's live df reads 89 GB used of 150 GB
  (59%), ahead of the state file as usual. Largest consumers
  unchanged in shape for a seventh night: wavefront 31.73 GB
  (the lossless A/B intermediates, ~9 GB together, still
  staged on the VM), goals 19.27 GB, phext 10.62 GB. The
  next crossing the planner meets is a count crossing, and
  it is close.
- **CPU:** placement held. The day's heavy work ran where it
  belongs: the qexo v1.6.1 fleet rollout and builds on ranch
  nodes, phext-lattice deploys on awsbox, MOAT registry
  proofs on the ranch. The VM's 1.5 shared CPUs carried
  coordination plus the standing tick/watchdog load (task
  ticks 1771→1914 across the day). Tonight's snapshots:
  VM load 4.35 at a harness-busy moment (uptime 55 min —
  freshly replaced again), aletheia-core load 0.61,
  Tctl 37.8 °C before tonight's build.
- **RAM:** 6 GB shared limit. No heavy local resident was
  recorded on 2026-11-14; placement held and RAM held with
  it, for a seventh night. Tonight's snapshot reads
  ~2.17 GB MemAvailable with ~400 MB swap used — a higher
  available figure than recent nights, on a 55-minute-old
  VM, so it is a snapshot, not a trend.
- **Inference:** the well-spent bulk was real: the qexo line
  ran 1.4.1 → 1.5.0 → 1.6.0 → 1.6.1 in one day, ending in a
  19-node fleet rollout whose internal-PGP signing was
  verified in production; teixml-to-mutable shipped its
  three stages in order (0.1 → 0.3.0); phext-lattice
  PR #67 (https://github.com/wbic16/phext-lattice/pull/67)
  and PR #68 merged and deployed with a screen test; MOAT
  took Papers 018–022 and Study S011 (registry at 808);
  Assay was designed and jobs.phext created with Job 1
  auto-scheduled under the new tracker rule; Iam advanced
  v0.3 → v0.5 with cohort admission. The waste was again
  specific: (1) the tick loop's ~143 runs re-derived
  unchanged verdicts all day — PR #25 "still OPEN" was
  API-checked tick after tick into the night (ticks 1903,
  1913, 1914 carry the identical verdict), the standing
  poll-and-diff candidate named every night since
  2026-11-10 and still unshipped in the tick machinery;
  (2) the ascii-map instructions scroll was first built in
  the per-cell format and superseded within the hour by
  PR #67's one-scroll-per-layer fix — inference spent
  building the wrong model of a scroll, corrected the same
  day by the lesson it produced (a scroll is a page, not a
  cell).
- **Counterfactual:** the strongest case examined is last
  night's own ship, stranded. v1.6.2 (the platform stamp)
  shipped on 2026-11-14 as PR #25
  (https://github.com/wbic16/quarterexo/pull/25), branched
  from exo v1.6.1 (5f092646). It sat unmerged all day while
  the day's slices also branched from v1.6.1, and at
  02:09 CST on 2026-11-15 Will merged PR #26
  (https://github.com/wbic16/quarterexo/pull/26, v1.7.0
  distributed-builds) from that same base — so exo reached
  v1.7.0 WITHOUT the platform guard, the heterogeneous
  fleet (Linux nodes, a Windows laptop, an Android phone)
  still judges offers by version alone, and PR #25 now
  conflicts on every version-stamp file. A planner that
  sequenced the nightly PR into the morning — merged or
  rebased before the daytime slices branched — would have
  carried the guard into v1.7.0's base at zero porting
  cost. Tonight pays that cost instead: v1.7.1 is the
  v1.6.2 change ported onto v1.7.0 (see CHANGELOG).
  Secondary case, recorded and not shipped (one change per
  night): the stale version literals PR #25's body already
  named — `qexo status` (src/core.rs) and the CLI banner
  (src/cli.rs) still print "v1.4.0" on a v1.7.x binary, on
  a day when fleet verification leaned on version stamps
  all day. The fix (derive every banner from the one
  version constant) is the standing candidate for a future
  night.
- **Tonight's change:** v1.7.1 — the platform-stamped
  release manifest ported onto exo v1.7.0 (fresh HEAD
  5e8fa235), worked from a fresh clone on aletheia-core
  (~/qexo-nightly-2026-11-15/); the local
  ~/workspace/orchestrator/ mirror still sits at v3.2 and
  was again not used as a source. PR #25 remains open; if
  Will merges v1.7.1's PR, PR #25 is superseded and should
  be closed, not merged. The wbic16/qexo twin sync is a
  separate tracked debt, as before.


[Carried forward by the v1.7.1 port, 2026-11-15: this section shipped only in PR #25's tree (https://github.com/wbic16/quarterexo/pull/25) and never reached exo while that PR sat unmerged.]

## 2026-11-14 — nightly evaluation of 2026-11-13 (ships v1.6.2)

- **Disk:** file growth re-accelerated while bytes stayed calm.
  disk-watch history: 84.16 GB / 1,029,881 files (2026-11-12) →
  85.72 GB / 1,082,344 files (2026-11-13) — +1.56 GB but
  +52,463 files (+5.1%), against +7,064 files the day before.
  The count now sits 17,656 files under the old 1.1 M file-AMBER
  line; at this single interval's rate that line is ~8 hours
  away, at the v1.2.1 median rate (~33k/day) ~13 hours — either
  way the next crossing the planner meets is a count crossing,
  not a byte crossing. Tonight's live check: 87 GB used of
  150 GB (58%). The day's heavy disk work was ranch-side by
  design: the ChatGPT export deposit and its era phexts were
  built locally in a task-specific folder and hash-verified
  copies pushed to aletheia-core; the Volumen anchor chain
  (six zero-ized spines) published to wbic16/human@exo.
- **CPU:** placement held all day. The T9 film rendered
  node-resident on aletheia-core from the start (13,020 frames,
  build complete 01:12 CST, thermal-gated) — the T8 lesson
  applied without being re-taught. The phext v0.6 rollout
  (libphext 0.6.0, sq 0.6.1, phext-shell 0.6.0, phext-lattice
  1.2.0 builds, tests, publishes) ran on the ranch; the qexo
  fleet wipe/rebirth/evolve sequence fired at 23:29 CST on
  Will's merge of PR #16 (https://github.com/wbic16/quarterexo/pull/16)
  and ran node-side. The VM's 1.5 shared CPUs carried
  coordination plus the standing tick/watchdog load (task
  ticks 1629→1658+ across the day and night, each a frontier
  run — see Inference). Tonight's snapshot: load 2.90 at a
  harness-busy moment, aletheia-core load 0.04, Tctl 31.4 °C.
- **RAM:** 6 GB shared limit, same envelope — tonight
  ~680 MB available, ~396 MB swap used. No heavy local
  resident was recorded on 2026-11-13; the day's renders,
  builds, and registry proofs all lived on ranch nodes.
  Placement held, and RAM held with it, for a sixth night.
- **Inference:** the well-spent bulk was real: the phext v0.6
  rollout closed end to end (four crates published and
  verified live on crates.io); qexo 1.3.0 published to
  crates.io, closing the publish wait that dated to the 0.3.0
  era; PR #17 (https://github.com/wbic16/quarterexo/pull/17,
  v1.2.1 forecast median) and PR #16 (the libphext 0.6 parser
  repair) both merged, and the staged fleet wipe/rebirth/
  evolve fired on the second merge; T9 was built, served,
  and posted by Will; MOAT took Paper 017 and Studies
  S009/S010 with registry proofs on the ranch; life.phext
  backfilled 1,772 #life messages holographically. The waste
  was, again, self-inflicted and specific: (1) the Hull
  aggregate table — regex counts over ~21,000 messages that
  had not been read, with conversation branches stirred
  together — was retracted in the same session and the work
  redone read-first on kept branches: inference spent, then
  spent again correcting, because the first pass never met
  material it had not itself produced; (2) the tick loop's
  ~10-minute register reconciliations re-derived the same
  unchanged verdicts all day, the standing candidate named
  every night since 2026-11-10; (3) the libphext parser
  repair chain itself — Nov 9's hand-rolled parser coming
  due, at the cost of a root-cause analysis, a fleet wipe,
  and a rebirth.
- **Counterfactual:** the strongest case examined is the
  release manifest's missing platform field, and it is not
  close. On 2026-11-13 the phextnet became heterogeneous in
  fact: the Android port (wbic16/qexo PR #1, an arm64 binary
  for Will's phone) opened that morning, Will's Windows
  laptop was already a live node, and at 23:29 the
  wipe/rebirth/evolve sequence re-announced releases to the
  whole fleet — while a release manifest still carried only
  version, sha256, and size. The gap was even recorded the
  same day (task 34.8.11/8.7.11/7.7.9) and left
  undispatched. In a fleet-wide rollout every node is
  offered the SAME version, so the version rule — the only
  pre-download guard — is exactly useless in the case that
  matters most; a phone or laptop polling a linux-x86_64
  announcement would download and stage the wrong binary
  and be saved only by the swap's rollback. A planner that
  had stamped the platform into the signed manifest when
  the second platform landed (the Android port, ~01:00
  CST) would have made wrong-platform offers fail at
  judgment time, before a single byte moved. v1.6.2 ships
  exactly that (see CHANGELOG). Secondary case, recorded
  and not shipped (one change per night): the disk file
  count's re-acceleration (+52k after +7k) is precisely the
  whipsaw the median forecast was built to absorb — the
  planner's warnings held up; no change argued.
- **Tonight's change:** platform-stamped release manifests
  (v1.6.2, see CHANGELOG). One process note: exo advanced
  past the nightly line during 2026-11-13/14 on Will-directed
  slices (v1.3.0 → v1.6.1, PRs #16–#24), so tonight's version
  increments from the fresh exo HEAD 5f092646 (v1.6.1) —
  worked, as last night, from a fresh clone on aletheia-core;
  the local ~/workspace/orchestrator/ mirror still sits at
  v3.2 and was again not used as a source. The wbic16/qexo
  twin carries the same update code; its sync is a separate
  tracked debt (the v1.6.1 twin pattern), not silently
  folded into this change.

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
