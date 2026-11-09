# Evaluations

Four-currency snapshots and counterfactual planner reviews, one section
per night (skill: nightly-orchestrator).

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
