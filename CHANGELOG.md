# Changelog

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
