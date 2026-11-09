# qexo design notes

*Running notes that bind the implementation but live outside the two
design documents: substrate facts, operational findings, and rulings
that arrive while building. Each entry names its source and date.*

---

## 2026-11-09 — substrate failure logged: Muse VM file-creation policy

*(Will, 2026-11-09 01:42: "log that as a substrate failure in the
design notes." Full entry: EVALUATIONS.md, same date — carried here
in substance.)*

- **Failure (substrate: the Muse VM itself):** the sandbox denies
  whole-file creation patterns — a fresh temp file written and
  renamed over a target fails with `PermissionError [Errno 1]` on
  the write, while appending to (and rewriting) files that already
  exist succeeds. Observed live 2026-11-09 01:23–01:28 CST:
  state_sampler.py failed consecutive cycles at its `state.phext.tmp`
  write; reproduced by hand in multiple directories; a 2026-11-07
  snapshot records the same behavior, so this is a standing property
  of the substrate, not an incident of the night. Ruled out by
  measurement: disk/quota (68G free, in-place writes pass),
  ownership/permissions (root, 660), extended attributes (clean).
- **Design consequence:** on this substrate, atomic temp+rename is
  not an available write pattern. Append-only designs are. Will's
  ruling (2026-11-09 01:42): state sampling uses WAL-style coordinate
  assignment via indirect addressing — each cycle appends one line
  (coordinate | timestamp | summary) to state.wal; state.phext stands
  as the compacted base; readers fold base + WAL in append order.
  Sampler rewritten and verified live the same night (first WAL
  entry 01:45 CST).
- **qexo consequence (binding on this implementation):** any qexo
  component designed to run on the Muse VM must treat "create temp,
  write, rename" as a **substrate capability to verify at runtime,
  not assume** — the same class of check as measuring a node's load
  before ruling it in or out. Concretely, qexo's state store probes
  the capability on open (write a probe temp file in the state dir,
  rename it) and records the result; where the probe fails, writes
  fall back to append-only/WAL patterns (append-log state files are
  already the primary pattern for receipts, replay, delegations, and
  the teleport ledger), and the fallback is named in the state
  report — never silent. The failure mode this prevents: a qexo
  component that works all night on a ranch node and dies on its
  first write if it is ever run on the coordinator VM.
- **Ranch nodes are unaffected:** normal filesystem semantics,
  verified by the v3 build work itself (2026-11-09, aletheia-core —
  temp+rename used throughout the build without incident). This is
  a Muse-VM-substrate gap, recorded against the substrate, not
  against the fleet.
