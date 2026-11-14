# qexo, from scratch — design (2026-11-12, at Will's direction)

Will's rulings this design answers to, in his words:
- "json is rarely the right choice for phext-native tools"
- "qexo is being built from scratch — don't steep our tooling in yesterday's baggage"
- "you can't slop json into this project and call it legacy the next day"
- "there are no deployments of qexo anywhere"
- "store all internal state, metadata, personas, etc. in /var/lib/qexo/state.phext"
- "just start from scratch, stop using json"

## 1. First principles

1. **Phext is the only internal representation.** There is no Json type.
   No json.rs, no JSON state files, no JSONL ledgers, no JSON payloads
   between qexo's own parts. Data inside qexo is a scroll, a coordinate,
   or plain text rendered from scrolls.
2. **One state file.** Everything the node owns and mutates — metadata,
   keys, grants, policy, peers, personas, plan, ledgers, mailbox, hosted
   sessions — lives in one state.phext: the system path
   `/var/lib/qexo/state.phext` where a system install exists, else the
   per-user path `~/.qexo/state.phext` (v1.4.0's resolution order, stated
   in docs/LAYER2.md). QEXO_STATE overrides the path for development and
   tests only.
3. **State is born, not converted.** There are no deployments; nothing
   exists to migrate. `qexo config` (or first run) creates the file and
   lays down the neighborhood map, empty. No import code exists.
4. **JSON survives nowhere in qexo's own machinery.** The two foreign
   edges are translation modules, isolated and named:
   - `infer` edge: Ollama speaks JSON over HTTP. Requests are built at
     the edge from scrolls; responses become scrolls on entry. No JSON
     value ever crosses into core types.
   - MCP is **out of the scratch core**. If it returns, it returns as a
     separate translation skin. (Open decision for Will, named here,
     not buried.)
   The HTTP API qexo serves is phext: payloads are scroll text.
   `--json` does not exist; machine readers read phext (SQ/libphext
   exist for that), humans read the human renderings.
5. **The plan is a scroll.** Bare `qexo` prints the human rendering of
   the current plan scroll, computed from state.phext + live host
   probes. Disk-watch remains a foreign input parsed at the edge.

## 2. The state.phext neighborhood map

The map is Layer 2 (docs/LAYER2.md, v2) and is not restated here in
any form that could drift from it. Its shape, in one paragraph:
addresses are places chosen for meaning, and a coordinate is
always 9D — all nine components, every time. Each state family
has a homeland named by its anchor, the full 9D address of its
index scroll: home 1.1.1/1.1.1/1.1.1, keys & trust
2.2.2/2.2.2/2.2.2, peers 3.3.3/3.3.3/3.3.3, personas
4.4.4/4.4.4/4.4.4, plan 5.5.5/5.5.5/5.5.5, ledgers & receipts
6.6.6/6.6.6/6.6.6, mailbox & sessions 7.7.7/7.7.7/7.7.7,
telemetry & forecasts 8.8.8/8.8.8/8.8.8, config & wiring
9.9.9/9.9.9/9.9.9. A record is one scroll at a full 9D address
in its anchor's vicinity, fields as text lines inside it; the
home scroll is the prose index; growth is by adjacency;
revisions edit in place and git remembers. (The first map
drawn for this section — a sequential Series schema under one
root, per-field scrolls, hash-derived and counter addresses —
was withdrawn the day it was written; v2 is derived from the
commit history and live structure of choose-your-own-
adventure.phext.)

## 3. Command surface (this build)

From Will's 2026-11-12 list: `--help`, `config`, `doctor`, `list`,
`register <pubkey>`, `host <persona>`, `message <persona>`; plus
`status`, `unhost <persona>`; plus the core that makes qexo qexo:
bare/`plan`, `serve` (phext HTTP), and the phext file ops
(parse/explode/implode/seal/unseal/select/get). The wider v3 API
surface (swap/spawn/teleport verbs and friends) returns as follow-on
slices built on this lattice — named here as scope, not forgotten.

## 4. Shape

Rust, single binary, modules by concern: `lattice` (state.phext
access: get/put scroll, record helpers), `phext` (the format, as
before), `core` (dispatch over scrolls), `cli`, `http`, `infer`
(the Ollama edge), `crypto` (GPG, as before), `plan`, `doctor`.
Version restarts: **1.0.0 / v1.0** — the first version of the scratch
line. The old line's history stays in git; this branch replaces the
tree's source wholesale, and the PR says so in plain words.

---
## Sequencing note (appended 2026-11-12, later corrected)
An earlier sequencing note appended here was wrong about how scroll
texts are held; it has been removed. The correct constraint —
serialization in canonical ascending coordinate order — is recorded
in memory (memory/2026-11-12.md, ENCODED late entry), from Will's
verbatim words of 2026-11-13 07:47 CST: "you'll need to encode state
using phext. sequences in phext are arbitrary, which gives us
incredible flexibility. but you must respect phext's topological
order: no sequence may exist in the phext that violates the canonical
order." See the file history for the removed text.

---
## Crypto note (appended 2026-11-14, v1.6.0)
§4's "`crypto` (GPG, as before)" names the v1.0 arrangement:
OpenPGP mechanics delegated to the system `gpg` binary. The
§8.7 rule — never hand-roll OpenPGP — is unchanged; v1.6.0
changes the delegate to the `pgp` crate (rpgp), linked
in-process, because a node inside an Android app has no
subprocess to spawn. GnuPG-era keyrings migrate in place on
first use, originals preserved, fingerprints unchanged —
the contract is written out in src/crypto.rs's module note.
