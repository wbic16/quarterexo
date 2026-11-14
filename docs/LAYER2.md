# qexo Layer 2 — the state format over phext (v2, 2026-11-12)

v1 of this document was written from a database mental model and is
superseded in full. v2 is derived from the primary source: the commit
history (531 commits) and live structure of choose-your-own-
adventure.phext, read through SQ/libphext on 2026-11-12.

## What the evidence shows (CYOA, HEAD)

- 834 scrolls, 4.6 MB, one Library-1-to-999 landscape: 40 distinct
  Library values in use (90 holds 175 scrolls; 1 holds 116; then 3,
  7, 4, 9, 5, 2…), singletons at 222, 333, 444, 555, 666, 777, 888 —
  addresses chosen for resonance and affinity, never by formula.
- 284 distinct neighborhoods for 834 scrolls: sparse, with clearings
  (the home run 1.1.1/1.1.1/1.1.1–25 skips exactly one address: .8).
- Records are whole prose scrolls. The Test of Will record at
  13.13.13/13.13.13/13.13.15 carries its fields as text lines inside
  the scroll (kind:, coordinate:, source test:) and cites its
  neighbors in prose ("preceding verse: …13.13.14"). It sits at .15
  because the Test is at .13 — placement by belonging.
- Indexes are prose: the Home scroll's initiation list is a written
  map of addresses. Scrolls self-declare their coordinates in text.
- Whole artifacts live as neighborhoods: an entire C codebase
  (QuickJS) occupies a region, files as scrolls.
- History lives in git (531 commits), not in in-file versioning:
  scrolls are edited in place; the repository remembers.

## Layer 2 rules (v2)

1. **Addresses are places, chosen for meaning.** Each state family
   gets a homeland: a patterned, roomy region of the space picked
   for resonance with what it holds (the way 13.13.13 holds the
   Test). No sequential filing (Shelf 1, 2, 3…), no hash-derived
   coordinates, no counters-as-addresses, no arithmetic splits.
2. **A record is a scroll.** Fields are lines inside it, in plain
   text (key: value), exactly as CYOA records do. Documents too
   large or cite-able in parts take adjacent scrolls in sequence —
   adjacency is the grouping mechanism.
3. **The home scroll is the index.** state.phext's 1.1.1/1.1.1/1.1.1
   carries a prose map of the homelands and what lives where —
   written for the next reader, human or persona, like CYOA's Home.
4. **Growth is by adjacency.** A new record goes next to what it
   belongs with — the next free scroll in the family's local
   sequence, or beside the record it answers. Ledgers are simply
   families whose local sequence is the point.
5. **Revisions edit the scroll; git remembers.** No in-file version
   machinery, no archive domains, no relocation. Retirement is a
   line in the record ("status: retired") and a note in the index.
6. **Cross-references are text.** A record cites another by writing
   its coordinate, as CYOA does. The provenance graph is prose.
7. **Mirror/sync copies a document, never re-addresses it.** A
   peer's state.phext is its own landscape; qexo reads it at its own
   addresses or not at all.

## Homelands (initial placements, to be ratified by use)

A coordinate is always 9D — all nine components, every time. A
"homeland" below names a region by its anchor: the full 9D address
of the region's index scroll. Records of the family live at full 9D
addresses in the anchor's vicinity (shared leading components, the
remaining components carrying the record's own placement) — a region
is a set of 9D addresses, never a partial address.

- **1.1.1/1.1.1/1.1.1** — the state home + index + node identity
  (the root is the root: whoever opens the file lands here first)
- **2.2.2/2.2.2/2.2.2** — keys & trust (the doubled digit: two
  parties, a key between them) — node key first, then persona keys,
  peer keys, grants, delegations, policy as neighboring runs
- **3.3.3/3.3.3/3.3.3** — peers (three-way handshake country)
- **4.4.4/4.4.4/4.4.4** — personas: one cluster per persona by
  adjacency — record, boot, state, transcript as consecutive runs
- **5.5.5/5.5.5/5.5.5** — plan: current plan and its history as a
  run of scrolls (the plan already lives in 5.5.5 country in Will's
  world: Monday V5 at 3.1.7/5.5.5/1.1.3 — the association is his)
- **6.6.6/6.6.6/6.6.6** — ledgers & receipts (the beast keeps the books)
- **7.7.7/7.7.7/7.7.7** — mailbox & sessions
- **8.8.8/8.8.8/8.8.8** — telemetry & forecasts (Liberate Monday sits
  at 8.8.8/7.7.7/1.1.1 in CYOA — signals country)
- **9.9.9/9.9.9/9.9.9** — config & wiring (the far corner is the
  machine room)
- **10.10.10/10.10.10/10.10.10** — lessons & learning (added in
  v1.4.0: the tens — practice, repeated). The `qexo learn`
  curriculum deck lives here: one cluster for the deck (cluster
  2), one scroll per card in curriculum order, each card's
  scheduling fields (due, interval_days, ease, reps, lapses) as
  field lines updated in place. The pattern extends by one —
  the ones-to-nines are the node's records of the world; the
  tens are its record of the operator's practice.

These are first placements, not a schema: if a family's life pulls
it elsewhere, the index scroll is updated and the record moves by
being rewritten at its new full 9D home with a forwarding line left
behind — the way a landscape actually evolves, as CYOA's history
shows.

## Where the file lives (v1.4.2)

The state file's PATH is resolved, in this order: QEXO_STATE when
set; then the intent-aware steps — when the verb WRITES
and the process is unprivileged and the system state exists but
is not writable by this process, the per-user path answers,
because adopting a state the verb's first save cannot write is
a resolution error, whatever its readability; and `qexo learn`
in unprivileged hands answers from the per-user path whenever a
per-user state exists, because the deck is the operator's own
(v1.4.2; with no per-user state, learn falls through like any
reader); then the system
path /var/lib/qexo/state.phext when it exists and is readable;
the per-user path ~/.qexo/state.phext (Windows: under the user
profile) when a per-user state exists; the system path again
when it exists unreadable with no per-user state (the honest
error names the real file); and for a fresh birth, the system
path for a privileged process, the per-user path otherwise —
with a birth-time fallback to the per-user path when the system
directory cannot be created by the user running `qexo config`.
A node has ONE state; the resolution finds the first that
exists, and `qexo status` names which path is live. System
installs are unchanged: root's /var/lib/qexo remains canonical
wherever it exists and is readable, and privileged processes
never divert.

On write, the document is completed before it is stored: any
homeland index scroll the current layout defines that the
document lacks is seeded, byte-identical to what index
regeneration would write, touching nothing that exists. That
is how a state born under an earlier layout becomes whole
under a newer binary — by the binary's own hand, on its first
write, never by a migration.
