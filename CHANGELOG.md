# Changelog

## Unreleased — tool catalog + exovim (2026-11-09, branch emise/exovim-tool)

- New `src/tools.rs`: the tool catalog behind the install route —
  one static entry per installable tool (phext-lattice, rust, phcc,
  phcpp, phst, exovim) carrying source repo, Ubuntu package set,
  build recipe, and pitfalls. Before this, install recorded any
  tool name and qexo held no per-tool knowledge.
- exovim (wbic16/neovim, Will's phext-support Neovim fork) is the
  first complete entry: exact apt set (BUILD.md prerequisites +
  pkg-config/lua5.1/liblua5.1-0-dev), a system-compiler recipe that
  installs the standard tree intact and wires an `exovim` command,
  and the phcc variant with the LUA_MATH_LIBRARY pre-seed pitfall
  recorded in full (docs/TOOLS.md has the long form).
- `GET /v1/install` now returns the catalog with a live per-tool
  `apt_missing` preflight; `POST /v1/install` for a catalog tool
  returns its entry alongside the install record; `qexo doctor`
  summarises the catalog's apt preflight. Five new unit tests pin
  the exovim apt set, the phcc pitfall strings, and catalog
  coverage of the API row-90 tool set.

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

## v2.1 — 2026-11-09 (disk gate)

Motivating finding — the 2026-11-08 evaluation (EVALUATIONS.md,
2026-11-09 section): the home volume hit its quota wall (EDQUOT at
~106 GB used) and ~35 GB of WIP was cleared under pressure, while qexo
v2 read the disk-watch state and only *reported* it — the plan it
emitted at 71 GB GREEN would have been identical at the wall.

What v2.1 does (one change: the planner acts on the disk currency):

- The placement plan now computes a **disk gate** from the disk-watch
  state, using the thresholds disk-watch anchored to the 2026-11-08
  failure (AMBER ≥ 95 GB, RED ≥ 102 GB; the state's own level can also
  trip the gate):
  - **RED** — every heavy placement in the plan is marked
    `HELD (disk gate RED)` in both the printed plan and plan.json, with
    a banner saying not to dispatch until the gate clears.
  - **AMBER** — light tasks are marked `no staging`; heavy execution
    stays ranch-only.
  - **GREEN** — plan as before, with the gate line stating the
    headroom thresholds.
- plan.json gains a `disk_gate` object ({state, action}) and reports
  version `v2.1`; the plan, interfaces, and personas banners all say
  v2.1. Cargo version 0.2.0 → 0.2.1. No other behavior changes.
- Numbering note: the DESIGN-V3 v3.0 implementation proceeds separately
  on the emise/v3-build branch; this is an increment of the v2 code
  line that lives in the working tree and on exo, and does not claim
  the v3 number.

## 0.1.0 — v1/v2 line

The dispatcher lineage: plan emission, sync manifest/diff, interior
interfaces, personas commands.
