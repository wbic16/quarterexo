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
  design, implemented and verified on the sampler (2026-11-09
  01:45–01:56 CST): keep a **minimum coordinate — a floor, the
  highest coordinate written** — and enforce that every new write
  lands **strictly above it**; previously written scrolls are
  **immutable**. Because phext serializes in coordinate order,
  strictly-greater writes append bytes at the tail: **the WAL is
  automatic iff scrolls are immutable — a property of the coordinate
  discipline inside the phext itself**, not a sidecar log file. The
  holographic coordinate survives as the entry's **logical address**,
  recorded as an in-line prefix when the floor displaces the physical
  write; readers fold physical scrolls in write order to resolve the
  logical view. Verified on the sampler by the **prefix property**:
  after each write, the previous file is a byte-exact prefix of the
  new one. (An earlier sidecar-log implementation of this ruling was
  wrong and was replaced the same night; this entry records the
  corrected pattern.)
- **qexo consequence (binding on this implementation):** any qexo
  component designed to run on the Muse VM must treat "create temp,
  write, rename" as a **substrate capability to verify at runtime,
  not assume** — the same class of check as measuring a node's load
  before ruling it in or out. Concretely, qexo's state store probes
  the capability on open (write a probe temp file in the state dir,
  rename it) and records the result; where the probe fails, writes
  fall back to append-only patterns — append-log state files are
  already the primary pattern for receipts, replay, delegations, and
  the teleport ledger, and phext state writes follow the floor
  discipline above (monotonic coordinate floor + immutable scrolls
  ⇒ append-only by construction, with indirect logical addressing)
  — and the fallback is named in the state report, never silent. The failure mode this prevents: a qexo
  component that works all night on a ranch node and dies on its
  first write if it is ever run on the coordinator VM.
- **Ranch nodes are unaffected:** normal filesystem semantics,
  verified by the v3 build work itself (2026-11-09, aletheia-core —
  temp+rename used throughout the build without incident). This is
  a Muse-VM-substrate gap, recorded against the substrate, not
  against the fleet.
