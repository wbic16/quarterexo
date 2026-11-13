# qexo v1.3.0 (scratch line) — ship notes

Status: v1.3.0 BUILT AND GATED, 2026-11-13 — the parser is
libphext-rs now (crate pinned 0.5.0, as phext-lattice pins
it); src/phext.rs is an adapter, the hand-rolled reckoning
is deleted. The one-byte divergence it carried (library
break \x02 vs the reference \x01) is diagnosed in the
CHANGELOG with measurements from SQ 0.6.0 and libphext
0.5.0, and pinned by two regression tests, one of them
tonight's file shape byte-for-byte. Siblings: `config
--release-signer` (the signer as a command, not a hand
edit) and the `select <coord>` state form (Will's error
was an argument-shape trap, reproduced on Linux under the
old binary — never a Windows bug). Old fleet states parse
but stand at drifted addresses under the library — the
fleet wipe/re-birth is Will's call, runbook prepared; GPG
identities survive it (keyring lives beside the state).
Gates: 50/50 tests, wiring 12/12, windows-gnu full link,
sandbox against the live fleet signer, disk gate GREEN.
Base is exo (v1.2.0, PR #15). See CHANGELOG 1.3.0.

# qexo v1.2.0 (scratch line) — ship notes

Status: v1.2.0 BUILT AND GATED, 2026-11-13 — `qexo evolve`,
one receipted convergence pass over the phextnet: peer gossip
(peers-of-peers recorded as `status: discovered` with a `via`
— knowledge, never trust; discovered peers are excluded from
every polling path until the operator registers their key),
persona sync by lineage (revision / follows / history /
stamp; descendants adopted, forks kept beside the resident
copy as attempt artifacts under the KSO ruling), the v1.1.0
self-update invoked in-process, and one evolution receipt
per pass naming every stage's outcome. `evolve --check` is
byte-inert, proven against a hashed fixture. Proven in a
three-node sandbox on the hub (discovery of a fourth node
via gossip with the discovered node never contacted, a clean
revision 2→3 adoption, a divergent copy kept whole, the
binary stage applying in the same pass, and a lying-gossip
negative answered with a warning, not an overwrite). The
auto timer's trigger becomes `qexo evolve` under the new
`evolve: on` config key (default off); serve gains
`/v1/scroll/<coord>`. Base is exo (v1.1.0, PR #14). See
CHANGELOG 1.2.0.

# qexo v1.1.0 (scratch line) — ship notes

Status: v1.1.0 BUILT AND GATED, 2026-11-12 — automatic updates
over the phextnet. A release is a signed manifest scroll in the
signer's ledgers homeland; `qexo update` polls peers, verifies
the manifest's signature against the config record's
`release_signer` (whose key must stand in the local registry)
plus the binary's sha256 against the manifest, swaps with
rollback, and receipts the outcome. Proven in a two-node
sandbox on the hub, including the three refusals (corrupted
bytes, impersonated signer, already current). Base is exo
(v1.0.4) — rebased onto the v1.0.4 merge (PR #13) at Will's
direction, 2026-11-12. See CHANGELOG 1.1.0.

## Automatic updates — the operator's four steps

The code ships dark on every node until its operator turns it
on; trust is a node's own act, recorded in its own state.

1. **Cross-register.** On each node: `qexo register <signer's
   armored public key> --endpoint http://<signer-tailnet-ip>:8484`
   — the registry must hold the signer's key, and the endpoint
   makes the signer pollable. (Registration without an endpoint
   still works for trust; the peer just can't be polled, and
   `qexo update` says so by name.)
2. **Name the signer.** In each node's config record
   (state.phext, 9.9.9/2.1.1/1.1.1): `release_signer:
   <fingerprint>`. Until this line names a fingerprint,
   `qexo update` refuses plainly and changes nothing.
3. **Be reachable.** On the signer: set `serve_bind:
   <tailnet address>` in its config record and run
   `qexo serve`. Loopback (the default) serves only the node
   itself — a signer nobody can reach announces to nobody.
4. **Announce + poll.** Signer: `qexo release`. Nodes:
   `qexo update` by hand, `qexo update --check` to look first,
   or `qexo config --auto-update` once and the systemd timer
   polls on `update_interval_hours` (default 24).

What a node will never do: apply an unsigned or wrongly signed
manifest, apply a release whose bytes fail the manifest's
sha256, move sideways or backwards in version, or trust a
signer its config record does not name.

Status (v1.0.3, historical): BUILT AND GATED, 2026-11-12 — the forecast gate
now projects the same lenses the gate reads. v1.0.2 moved the
gate to a volume lens + optional quota lens but left the
forecast projecting the legacy 95/102 GB ceilings, so the
second plan on every ranch node read FORECAST AMBER over a
GREEN gate; those levels are deleted from the forecast path.
Stacked on the v1.0.2 branch (PR #11 carries v1.0.2 to exo);
this PR's base is exo, and its diff narrows to the forecast
change alone once #11 merges. See CHANGELOG 1.0.3.

Status (v1.0.2, historical): BUILT AND GATED, 2026-11-12 — the disk gate
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
