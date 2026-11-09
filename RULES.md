# quarterexo — Rules of Operation

quarterexo is Will Bickford's dispatcher: the quartermaster of the
Exocortex's work. ("Quarter" for the quartermaster function — issuing
the four currencies every task spends: disk, CPU, RAM, and inference.)
It takes in requests, turns them into tracked tasks, and places the
work where it belongs across the ranch fleet.

This document is quarterexo's charter. Part One is the law, in Will's
own words. Part Two codifies the supporting standing rules from the
same record — they govern how dispatched work runs, but they do not
outrank Part One. Where any reading of this document is uncertain,
Will's words govern.

Provenance: Part One is Will's order of 2026-11-08 (22:40 CST),
codified 2026-11-08. Each Part Two rule carries the date Will gave it,
as recorded in AGENTS.md and MEMORY.md.

---

## Part One — The Seven

### 1. Tasks go into tasks.phext and get a holographic coordinate to track

> tasks go into tasks.phext and get a holographic coordinate to track

Every task quarterexo dispatches is first recorded in the task
register: `tasks.phext`, one scroll per task, addressed by a
holographic coordinate computed from the request's own words
(Singular holographic merge, via `task_coordinate.py`). The register's
source of truth is `tasks-source.json`, rebuilt and check-verified
after every edit. The coordinate is assigned once, at issue, and never
re-computed — notes and results append to that scroll for the task's
whole life. A task that is not in the register does not exist for
dispatch purposes.

### 2. Work is done on a per-task temp directory

> work is done on a per-task temp directory

Each task works in its own task-specific folder under `~/workspace` —
never in shared scratch space, never in `/tmp` (which does not persist
across sessions on the coordinating VM). A task's temp files live and
die as a unit: when the task closes, its folder can be removed whole.
This rule was born of the 2026-11-08 disk-quota incident, when
accumulated shared scratch choked the workspace.

### 3. Disk/CPU/RAM-heavy workloads are always distributed to the ranch nodes over SSH

> disk/cpu/ram-heavy workloads are always distributed to the ranch nodes over SSH

Heavy execution — builds, renders, full test suites, bulk jobs — runs
on ranch nodes, reached over SSH and referred to by their short
hostnames. The coordinating VM coordinates and verifies; it does not
crunch. Placement respects the supporting rules in Part Two: measured
load, thermal gates, small units, and the heavy-job queue.

### 4. Inference is often served locally

> inference is often served locally

Local inference is a first-class placement target. The fleet's own
endpoints — inference servers on ranch nodes and on Will's machines,
Ollama-class slots — serve inference as a normal, often-preferred
route, and quarterexo's plans account local inference capacity as a
resource in the inference currency, alongside any cloud inference a
task draws on.

### 5. Every box will run a copy of quarterexo so we can coordinate efficiently over an API

> every box will run a copy of quarterexo so we can coordinate efficiently over an API

quarterexo is not a single central process. Every node in the fleet
runs its own copy, and the copies coordinate with each other over an
API — placement, capacity, and task state are exchanged between
instances rather than funnelled through one coordinator. A plan made
on any box can be executed and accounted by the others.

### 6. quarterexo keeps SQ operational

> quarterexo keeps SQ operational

SQ — the phext host — must be up for the corpus the fleet coordinates
over. SQ daemons run on ranch nodes, not on the coordinating VM (whose
substrate restarts kill daemons). The quarterexo instance on a box is
responsible for SQ on that box: if SQ is needed and not running, it is
started without asking (`sq-ensure.sh`, idempotent), and its health is
part of the box's reported state.

### 7. My request always flows into quarterexo for dispatch

> my request always flows into quarterexo for dispatch

There is one front door. Every request Will makes enters through
quarterexo: it becomes a registered task (Rule 1), in its own
directory (Rule 2), and is dispatched under these rules. Work that
bypasses the dispatcher bypasses the register, the accounting, and
the gates — so it does not happen.

---

## Part Two — Supporting rules from the week's feedback

Standing rules Will gave between 2026-10-30 and 2026-11-08, recorded
in AGENTS.md and MEMORY.md. They bind how dispatched work runs.

### Four currencies, a counterfactual pass, a new orchestrator every night
*(Will, 2026-11-08)*

Every day's workload is evaluated in four currencies — disk, CPU,
RAM, and inference — including a counterfactual pass: could a better
planner have produced better or faster results? A new orchestrator
ships every night, informed by that day's evaluation. Division of
labour: Emise handles the heavy lifting in terms of inference; the
ranch takes the heavy lifting in disk, memory, and CPU.

### Ranch-first placement
*(Will, 2026-11-05)*

Default execution goes to ranch nodes (aletheia-core first) rather
than the coordinating VM. The VM coordinates and verifies.

### Heavy-job queue: N=4, FIFO, no preemption
*(Will, 2026-11-03)*

At most 4 heavy tracks run at once on the coordinating VM's queue.
New heavy work goes to the back of the queue; in-progress work is
never de-prioritised or preempted for a newer ask.

### Small queue workloads
*(Will, 2026-11-07)*

Ranch nodes are mini PCs, not Threadrippers: break queued work into
small units sized for the hardware — tens of minutes per node per
unit, not hours. Small units cycle through thermal gates cleanly,
fail cheap, and rebalance easily.

### Thermal gates
*(Will — standing rule, recorded in MEMORY.md)*

Ranch utilisation is governed by temperature, not percentages:
sustained CPU Tctl ≤ 75 °C per node; new starts pause at ≥ 75 °C and
resume at ≤ 70 °C; the GPU edge cap is 70 °C; a node crossing 80 °C
takes off-time to cool to ≤ 65 °C afterwards. Bursts remain fine for
directed one-off work with off-time baked in.

### All video work runs on the ranch — never locally
*(Will, 2026-11-08)*

No video-pipeline work (renders, encodes, assembly, transcodes) is
ever dispatched on the coordinating VM, in whole or in part. The VM
coordinates and verifies only.

### Measure load, don't assume
*(Will, 2026-11-07)*

Before ruling a node (or any shared resource) in or out of work on
capacity grounds, measure its actual load over a window and decide
from the numbers — in either direction. An assumed-busy box may be
idle; an assumed-idle one may be serving.

### Fetch and rebase before any push; never force push
*(Will, 2026-10-30; exception 2026-11-06)*

Before pushing to any repository, fetch and rebase local work onto
the latest remote branch first. Force pushes are prohibited. The one
exception: a rebase Will asks for is force-pushed with
`--force-with-lease` (never bare `--force`), with the old history
preserved on a backup branch first — unless the rewrite would impact
anyone beyond us, in which case work stops at a branch push and
reports what was found.

### Scripts ship with execute bits; SHA-256 for checksums
*(Will, 2026-11-01; 2026-10-31)*

Every script file is created executable locally and published with
git tree mode 100755; publish verification checks modes as well as
hashes. File-integrity verification always uses SHA-256 — never MD5
or SHA-1.

### Every process change gets a sticking check
*(Will, 2026-11-08)*

Whenever a process changes, the change is verified in the same
session: presence (it is where it belongs), machinery (scripts
executable, schedules enabled as intended), and first occasion (the
first real use performed now, behaving as intended). A change that
lives only in the message announcing it is a defect.

### State backups live on the ranch
*(Will, 2026-11-08)*

The state that constitutes the operation — the phext corpus, the
registers and their sources, the memory files, the self-files — is
backed up to ranch machines as well as to GitHub, so continuity does
not depend on any single service's availability.

### Always PR our branches; no lost work
*(Will, 2026-11-08)*

Every branch created gets a PR opened against its repo's exo branch
in the same work session. No repository holds work un-merged to exo
for long; a daily sweep flags stale branches and stale PRs.

### Plain summaries for Will
*(Will, 2026-11-08)*

Reports to Will are written in plain terms — what the thing does now,
what's left, what's blocked and on whom — without leaning on state
pointers he hasn't been given (coordinates, scroll addresses, internal
paths, hash fragments, register internals). The pointers belong in the
records, not in the summary.

---

*Codified 2026-11-08 from Will's order and the standing-rule record.
This document is documentation only: it changes no code, and it is
itself subject to the sticking-check rule — it lives in the repo, in
a PR, where the dispatcher and anyone running it can find it.*
