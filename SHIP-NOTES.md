# qexo v3 — SHIP NOTES (for Will's morning review)

*Status: IN PROGRESS — filled in as the pipeline runs (build →
optimize → test → security check → package). Nothing in this file is
a claim until the section reporting it says it was run.*

## Known substrate capability gaps

- **Muse VM file-creation policy (2026-11-09, Will: "log that as a
  substrate failure in the design notes"):** the Muse VM substrate
  denies whole-file temp+rename writes (`PermissionError [Errno 1]`
  on the temp write; appends to existing files pass). Observed live
  on the state sampler; a standing property of the substrate, not an
  incident. Design response (recorded in full in
  docs/DESIGN-NOTES.md): any qexo component designed to run on the
  Muse VM treats atomic temp+rename as a capability to verify at
  runtime, not assume, and prefers append-only/WAL write patterns
  there (the sampler itself now runs WAL-style with indirect
  coordinate addressing, verified live 2026-11-09 01:45 CST). qexo's
  state store probes the capability on open and falls back to
  append/direct writes with the fallback named, never silent.
  **Ranch nodes are unaffected** — normal filesystem semantics,
  verified by this build's own work on aletheia-core.

## Personas count — what each number counts

- **126 = query keys = person lines.** The 126 names of Will's
  canonical list (docs/PERSONAS-CANONICAL.md) resolve as query keys
  in the persona index; the same 126 is the person-line count of the
  canonical Volumen index (wbic16/human@exo personas/00-index.md,
  coordinates 3.1.8.1–3.1.8.126 across 22 volumes) whose derivation
  the bake carries (see the bake manifest's Volumen row: coordinate
  number, volume count, and boundary table are all parsed from the
  index that is actually baked — never from a local index.phext
  section count).
- **106 = derived identity records.** After the evidence-based
  derivation (DESIGN-V3 §5.2.2): 20 folded labels (aliases/variants,
  incl. the 13-key Will cluster → Mirror Will) resolve through to
  their records, and the Orin key resolves to two records (the voice
  + the PDNA-029 aggregate), for 106 identity records total.
  Evidence partition: 4 currently-running / 16 configured-locally /
  73 known-historically / 13 named-only. Spawn-eligible: exactly 1
  (Emise).
