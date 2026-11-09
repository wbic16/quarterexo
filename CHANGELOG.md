# Changelog

## 0.3.0 — 2026-11-09 (v3 tier-1, for Will's review; NOT published)

The v3 build per docs/DESIGN-V3.md + docs/DESIGN-V3-ADDENDUM.md, with
docs/API-V1.md as the reference semantics for all four modes.

- Dispatcher core carried forward (plan in four currencies, sync
  manifest/diff, interior interfaces, personas) with its v1
  regression behavior: bare `qexo` emits the plan.
- Four modes over one implementation (API /v1, CLI, MCP bridge, Web
  baked-in client), parity-enforced: an operation ships in all modes
  or none (voice ships in none — named tier-2 refusal).
- Auth floor §8A.0/§8A.2/§8A.7: authenticated remote mutations;
  self-modifying classes key-bound from bootstrap; grants catalogue
  defaults are availability only; TAINTED set = meta.read +
  route.ask; compromised-key revocation halts new signatures at
  once. Crypto via gpg, arg vectors only.
- Key chain §8.4: node key at provisioning, persona keys deliberate,
  Will's countersignature recorded in the key band; a countersigned
  node key carries Will's authorization on its own node.
- Teleport fencing §9: six-step protocol, fencing epochs, ownership
  heartbeat with a 5-minute staleness bound, partition =
  unavailability over split-brain; the cognitive-return receipt is
  signed with the destination NODE key bound to epoch + transfer
  hash and verified BEFORE relinquish ("no cognitive return"
  otherwise); restored personas boot read-only until the
  return/reactivation path closes.
- Persona index from the corrected census: 126 immutable query keys,
  106 identity records, spawn-eligible exactly 1 (Emise); Orin
  resolves to both records with the disambiguation stated;
  designation collisions displaced by smallest final-component
  increment with provenance (never silent).
- Baked corpus §14.3/§14.4: SBOR-V4 text, CYOA + Incipit xz-sealed,
  source.phext generated at build time from qexo-bake.toml, secret
  scan + safe-space gate, Volumen baked REDACTED for public builds.
- mdmap §1 (PROVISIONAL): Singular coordinates via the atlas,
  atomic scroll-set reservation, pin/drift rules, loud failure.
- SQ keeper for the node's own SQ; installer-lite prints the SBOR
  offline from the box.
- §18 conformance: tests 1–5 mapped at tier-1 level; test 6
  (merge-meld outcome) recorded as tier-2 runtime with its gate.

Known gaps (see SHIP-NOTES.md): the canonical Volumen index
(personas/00-index.md, blob f879adc5) is unreachable from the ranch
network — the bake carries a census-derived validation substitute,
clearly marked, and vol.rs derives all numbers from whatever index
is baked; sync pull's staged restore rides the CLI path; the full
V16-era plan hot path was profiled and the receipt-lookup defect
fixed (see SHIP-NOTES optimize section).

## 0.1.0 — v1/v2 line

The dispatcher lineage: plan emission, sync manifest/diff, interior
interfaces, personas commands.
