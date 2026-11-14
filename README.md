# qexo

qexo is the Exocortex coordinator: a single Rust binary that keeps
a node's working state — identity, keys, peers, personas, plan,
ledgers — and offers it to the operator and to peer nodes.

This line is phext-native from scratch. All node state lives
in **one phext file** — the system path `/var/lib/qexo/state.phext`
for system installs, the per-user path `~/.qexo/state.phext` for a
plain `cargo install qexo` (resolved in that order; see
docs/LAYER2.md, "Where the file lives") — addressed in the
Layer 2 scheme over phext's nine dimensions. qexo has no JSON
state, no JSONL ledgers, and no JSON API: its HTTP surface serves
scroll text, and machine readers use phext tooling (SQ/libphext).
The single foreign protocol qexo speaks is Ollama's, confined to
one translation module (`src/infer.rs`).

`QEXO_STATE` overrides the state path for development and tests.

Build: `cargo build --release`. Gate: `cargo test` and
`qexo wiring-check` (every module declared, every declaration
filed — the check that would have caught the v2.2/v2.3 orphaning).

The 0.x line's history remains in this repository's git history;
it is not the ancestor of this source tree, which was written
fresh against the design in the project's scratch record.
