# qexo v1.0.2 (scratch line) — ship notes

Status: v1.0.2 BUILT AND GATED, 2026-11-12 — the disk gate
measures the volume it stands on: a volume lens (free fraction)
plus a quota lens that exists only where a node configures it,
every threshold seeded visibly into the config record in
state.phext. On aletheia-core's 3.6 TB volume the incident
reading (809 GB used, 2.7 TB free) is GREEN, as it should have
been. Stacked on PR #9 (v1.0.1); base retargets to exo when #9
merges. See CHANGELOG 1.0.2.

Status (v1.0.1, historical): BUILT AND GATED, 2026-11-12 — a Windows
correctness fix for doctor on top of v1.0: per-platform lock
liveness (a /proc-only probe could have deleted a live
process's lock on Windows) and per-platform keyring-permission
semantics in the check and its test. No behavior change on
unix. See CHANGELOG 1.0.1.

Status (v1.0, historical): BUILT AND GATED, 2026-11-12. This branch replaces the
tree's source wholesale: qexo starts over as a phext-native
tool with no JSON in its own machinery and all of its state in
one file, /var/lib/qexo/state.phext. Not a migration — there
are no qexo deployments anywhere to be compatible with.

## What shipped

- src/phext.rs, src/hash.rs, src/xz.rs — carried verbatim from
  the old line (the phext wire shape does not change), with ONE
  fix to phext.rs forced by the battery: see below.
- src/crypto.rs — GPG mechanics carried; constructor rooted at
  the state dir; fingerprint_of added (a key's identity read
  without importing it).
- src/infer.rs — the Ollama edge, new: std-only, the crate's
  only JSON speaker, a translation module and nothing else.
- src/plan.rs — the four-currency gate core as pure functions
  over values; tests ported from the old battery.
- src/lattice.rs — the state layer, new: the homeland map
  (docs/LAYER2.md v2), records as single scrolls, derived prose
  indexes, adjacency growth.
- src/core.rs — the state commands, new: config, register,
  list, host, unhost, message, status, plan, serve — all on
  the lattice.
- src/doctor.rs — filesystem layer carried; widened with the
  lattice checks (missing homeland anchors, orphan records,
  ghost index lines) and index-rebuild fixes.
- src/cli.rs — argument surface + grouped --help, new.
- src/wiring.rs — the wiring-check gate, carried from v3.2.

## Build history, plainly

The build ran in two passes. The map-free parts were built and
gated first, with the state commands stubbed at a hold. The
first Layer 2 map (patterned domains, per-field scrolls,
hash-derived entity addresses, arithmetic ledger splits) was
withdrawn by Will the day it was drawn — it treated
coordinates as a key-space. v2 was derived instead from the
primary source: CYOA's 531-commit history and live structure,
read through SQ. In v2, addresses are places; records are
whole scrolls; the home scroll is the prose index; growth is
by adjacency. The 9D correction landed during the
continuation: every coordinate is all nine components, and
each homeland is named by its anchor — the full 9D address of
its index scroll (keys & trust 2.2.2/2.2.2/2.2.2, peers
3.3.3/3.3.3/3.3.3, and so on).

## The format fix the battery forced

The inherited phext model's serializer emitted only the
highest changed component's delimiter between scrolls — one
delimiter, one step — so any address jump of more than 1 in
any component landed at the wrong address on re-read, and its
parser minted empty scrolls at intermediate positions.
Patterned homeland addresses were unrepresentable: a command
wrote a record and the next process could not find it. The
serializer now walks the dead-reckoning path (a stack of
delimiters per step upward, as CYOA's own bytes do) and the
parser holds the existence rule: a scroll exists when it
holds at least a byte. Regression test:
sparse_patterned_addresses_roundtrip.

## Not in this slice

- MCP — out of the scratch core pending Will's decision.
- The wider 0.x API verbs (swap, spawn, teleport, instances,
  sync, dashboard): each returns as a follow-on slice on this
  lattice, its own PR, when its turn comes.
- Corpus/volumen persona boot-text lookup: bake/ is carried,
  read by nothing yet; boot text here comes from --boot files
  and state-dir sidecar phexts.

## Gate record (aletheia-core, 2026-11-12)

- cargo test: 29/29.
- cargo build --release: ok.
- wiring-check src: ok (12 module files).
- CLI battery: 59/59 — help/file ops, unborn refusals, config
  birth + idempotence, register/list at full 9D addresses,
  dead-endpoint host refusal, stub-Ollama host/message with
  the boot text proven in the stub's request log, the human
  plan from bare qexo, doctor lattice damage → precise report
  → --fix index rebuild → clean, serve smoke (scroll-text
  bodies), and no .json/.jsonl anywhere in the state sandbox.
- json grep audit: 'json' in src/ only inside src/infer.rs
  (the translation module) and one rule-stating comment in
  src/lattice.rs.
