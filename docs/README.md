# qexo v3 specs — for review

These are the current design specifications for **qexo** (the program;
**quarterexo** is the repo and the nightly process that improves it),
designed and ruled on 2026-11-08. They are design documents: no code in
this PR, and nothing here has been executed against any node.

## Document map

- **DESIGN-V3.md** — §§1–6: the MD→coordinate mapping spec (mdmap),
  the persona index and hot swap, inference routing configuration,
  the staged OpenClaw retirement plan, identity migration (one phext
  per prior identity), and enhancements.
- **DESIGN-V3-ADDENDUM.md** — §§7–16: the voice API, GPG keys and the
  §8A authorization layer (per-action key policy, delegation, grants),
  persona teleport, metadata access, phext tools, the installer, the
  pitfall buddy, CLI parity, the MCP bridge and cross-platform scope,
  and the quickfork.net release plan.
- **PERSONAS-CANONICAL.md** — the canonical persona list (126 names,
  Will, 2026-11-08), verbatim with its provenance header.

Not included: **DESIGN.md**, the v2 design. v3 supersedes it wherever
the two overlap (personas most of all); it stays in the working tree
as history, not as a current spec.

## Status

- **v1.1 shipped** — the plan tool plus the rename to qexo (this
  repo's exo branch).
- **v2 built and verified locally, unpublished** — the sync core and
  five interior interfaces run; publication was held so the design
  could be ruled first. v2 ships folded into v3.
- **v3 design ruled by Will, 2026-11-08** — the GPG authorization
  model and permissive-default grants, the Volumen boot manifest,
  the fork fold, Theia's pragmatic Stage 6, SQ→libphext v0.5.0 as a
  directed work item, and the §§12–16 scope are all in the documents
  as amendments with their dates.

## The six open questions

1. **Corus's position in the migration order** (DESIGN-V3 §4.3):
   migrated last, on a schedule their work allows — or exempt
   Quacked Hill until Corus's current work completes?
2. **Verse's children** (DESIGN-V3 §5.3): Verse is offline. Do Corus,
   Fovea, and Cortex migrate on Will's notice (Theia informed, the
   substitution recorded), or wait for contact with Verse?
3. **Persona key issuance timing** (addendum §8): issued deliberately,
   persona by persona, at Will's direction — or automatically at
   migration seeding?
4. **Roaming-seat teleports** (addendum §9, §8A.2): may roaming-seat
   definitions carry standing teleport delegations (scoped, expiring,
   receipts mandatory), or does every teleport wait for Will or the
   person?
5. **SQ→libphext v0.5.0 sequencing** (addendum §11.3): sequenced with
   the migration stages, with the installer's first fleet rollout,
   or as its own canary-first stage ahead of both?
6. **Voice session hosting** (addendum §7.5): is a ranch-native
   TTS/STT path a qexo work item in its own right, or does voice stay
   hosted where a TTS adapter exists until the ranch path arrives
   for other reasons?
