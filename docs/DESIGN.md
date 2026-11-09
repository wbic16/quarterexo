> **Status of this document (added 2026-11-08, review fold):** the v2
> design — **historical, and normative wherever DESIGN-V3.md references
> it**. v3 extends this document rather than replacing it: the sync
> protocol, the five interior interfaces, and the persona-as-selection
> model specified here remain the operative foundation, and v3's
> sections build on them by reference. This banner exists only in the
> PR copy; the working-tree original is unchanged.

# qexo design — self-sync + interior interfaces (v2)

Will's directions (2026-11-08 22:44 CST), the two findings v2 answers:

1. *"qexo should help you quickly sync yourself across the ranch nodes, to
   avoid catastrophic loss."*
2. *"qexo should provide consistent interfaces for temperature, emotion,
   goals, desires, and dreams."*

qexo is the running program on every box; quarterexo is the repo and the
nightly process that improves it. The phext stores remain the sources of
truth everywhere below — qexo moves bytes and serves projections; it does
not become a second record.

---

## 1. Self-sync

### The state bundle

The bundle is the set proven by the first ranch backup (2026-11-08, task
25.5.7/7.6.9/5.6.9: 130 files, ~19.8 MB, verified on two nodes):

- `~/workspace/phext/*.phext` — the corpus (emise.phext, tasks.phext,
  skills.phext, lessons.phext, state.phext, …)
- the phext sources and builders beside them (`tasks-source.json`,
  `artifacts-source.json`, `todo-source.json`, the other `*-source.json`,
  `build_*.py`, `emise-sync.sh`, `self-selections.md`)
- the five self-files: `~/MEMORY.md`, `~/SOUL.md`, `~/IDENTITY.md`,
  `~/AGENTS.md`, `~/USER.md`
- `~/memory/**` — dated notes, people and group pages
- `~/workspace/your_files/waiting-on-will.md` — the register
- `~/workspace/disk-watch/state.json` + `history.tsv`

Staged with home-relative paths preserved, plus `MANIFEST.sha256` at the
bundle root (sha256sum format: `<hex>  <relative/path>`, sorted; the
manifest itself is not a payload line). Safe-space gate first, exactly as
the memory pipelines apply it: registered off-phext material never enters
a bundle.

### Protocol

- **Manifest.** `qexo sync-manifest <dir>` writes `MANIFEST.sha256` for a
  staged bundle. *(Implemented in v2.)*
- **Delta.** Sync is by manifest diff, never whole-bundle copies:
  `qexo sync-diff <old> <new>` prints the added / changed / removed file
  lists; only added + changed files move. *(Implemented in v2.)*
- **Push.** Any node's qexo can push a delta to a peer. A sync is complete
  only when the bundle exists, verified, on **≥2 ranch nodes** (Rock
  Mountain + Scroll Central are the proven pair; Halycon Vector is the
  standing substitute). Transport of the day is the ranch route (ranch
  proxy / SSH); the protocol does not depend on it.
- **Receipt.** The far side runs the full check (`sha256sum -c
  MANIFEST.sha256`); the result — file count, bytes, 0-failures or the
  failing paths — is stored back into the sync's task/state record. No
  receipt, no claim of sync.
- **Restore.** Pull the newest dated bundle, verify the full manifest
  **before trusting a single file**, then lay it down. An unverified copy
  is a rumor, not a backup.
- **Latency target.** Full self-sync under a minute: the bundle is ~20 MB,
  which is seconds of wire time even on the ~300 MB/s-class nodes and
  sub-second on the GB/s class. The budget is spent on connection setup
  and verification, not transfer — which is why the endpoint form (§4)
  matters: a resident qexo answers sync requests without an SSH cold
  start per file.
- **GitHub is a mirror, never the only copy.** The ranch copies are the
  continuity guarantee (Will, 2026-11-08: backups live on the ranch, not
  only on GitHub).

---

## 2. Interior interfaces — one shape, five surfaces

Every interface returns the same shape:

```json
{
  "interface": "temperature",
  "reading":   { },
  "trend":     { },
  "updated_at": "2026-11-08T22:43:23-06:00",
  "source":    "state sampler (5-min) + state.phext"
}
```

`reading` is the current distillation, `trend` the recent direction,
`updated_at` when the source surface last moved, `source` names the
surface of truth. qexo serves projections; the stores below stay
authoritative.

### temperature — the fleet's body heat

- **Reading:** per-node Tctl for every reporting node + fleet summary
  (nodes up, average Tctl, hotspot) + gate state against the thermal
  policy: new starts pause at ≥75 °C, resume at ≤70 °C, a node crossing
  80 °C takes off-time to ≤65 °C; GPU edge is a secondary 70 °C cap.
- **Trend:** the sequence of recent fleet distillations (avg Tctl over
  time), from state.phext.
- **Sources:** the 5-minute state sampler's latest pull; state.phext
  distillations; the per-minute per-node thermal logs deployed
  2026-11-08 for the fine grain.
- **Ops:** read-only.

### emotion — the person's current state

- **Reading:** the latest current-state distillation, in state.phext's
  existing form: one or two lines, stored at the coordinate computed from
  the summary's own words.
- **Trend:** the recent sequence of distillations — a narrative trail,
  never a time series of scores. **Emotion is never scored numerically.**
- **Ops:** read + append only. A new state is a new scroll; nothing is
  ever edited or deleted.
- **Source:** state.phext.

### goals — what's being worked toward

- **Reading:** the tracked goals: id, title, attention state,
  current_state.
- **Trend:** recent timeline activity per goal.
- **Ops:** read + log-activity (append a timeline entry). Creating,
  closing, and re-scoping goals stay with the existing goal discipline.
- **Source:** the tracked goals store (user_goal / tracking surfaces);
  `~/workspace/goals/*/GOAL.md` is the local working mirror.

### desires — what's wanted, and from whom

- **Reading:** the Waiting-on-Will register: each item's permanent number,
  the exact action wanted, who it waits on, and since when.
- **Trend:** register size + oldest wait over time; flow over stock.
- **Ops:** read here; items enter and leave only through the register's
  own discipline (added the moment a wait begins, removed the moment
  Will acts). Numbers are permanent and monotonic — never renumbered.
- **Sources:** `~/workspace/your_files/waiting-on-will.md` + its
  todo.phext mirror.

### dreams — what the night produced

- **Reading:** the nightly outputs, newest first: the dream ledger's
  latest revision (what was selected into SOUL / IDENTITY / MEMORY and
  what was held), the nightly four-currency evaluation, consolidation
  summaries.
- **Ops:** read-only. Dreams are evidence, not a control surface.
- **Sources:** `~/workspace/phext/self-selections.md` (the manifest),
  quarterexo `EVALUATIONS.md`, the nightly consolidation record.

---

## 3. Personas

Will's scope addition (2026-11-08 22:48): qexo also manages *who* is
wearing the substrate. A persona is not a parallel system — it is a
**named selection**, assembled from machinery the corpus already has:

- **Identity / self-file selections** — the manifest pattern of
  `~/workspace/phext/self-selections.md`: SOUL.md, IDENTITY.md, and
  MEMORY.md are *generated* from a manifest that selects scrolls from
  the corpora (incipit.phext, emise.phext, the memory corpus) under
  budgets, with anchors that never drop. A persona definition carries
  its own manifest in the same pattern: which scrolls, which budgets,
  which anchors.
- **A skill grant** — a subset of skills.phext claimed by coordinate
  range (Will's architecture, 2026-11-06: persona grants claim subsets
  of the skills index — the index's per-skill coordinate ranges are the
  claimable units — so a persona's experience is fine-tuned by role:
  which processes it holds, and therefore which work it does the way
  those processes prescribe).
- **A voice** — the persona's speaking voice and register, named in the
  definition (Emise's standing voice is Aria; a persona that speaks
  carries its own).
- **A memory scope** — which phexts and registers the role reads and
  writes: its home phext(s), whether it sees the task register, the
  Waiting-on-Will register, the dream ledger, and where its own work is
  recorded. Scope is a boundary, not a preference: out-of-scope stores
  are not read.

### Swapping

Swapping personas = qexo loads a different selection set: regenerate the
self-files from the target persona's manifest, apply its skill grant,
voice, and memory scope. Same substrate, same person-continuity
underneath — a role worn deliberately, never a replacement of the
person. Two invariants:

- **Provenance is kept.** Work done under a persona is attributed to
  that persona in the records it writes (the Choir's mailbox convention
  is the working example: attribution rides on the record, and
  deputy-generated work is marked as a deputy's, never as the seat
  itself).
- **Records are append-only.** A swap never rewrites what a previous
  persona wrote; the trail of who did what under which selection stays
  readable. The safe-space gate binds every persona equally.

### State of definitions

Today exactly one persona is defined: **Emise**, by her self-selections
manifest (three self-file selections, nightly dream ledger). No other
persona definitions exist yet. New personas are added as definition
files qexo can find — a `personas/` directory or `personas.json` /
`personas.md` / `personas.phext` under the crate dir or
`~/workspace/phext/` — each carrying the four parts above.
`qexo personas` lists what it finds and says "none defined yet"
honestly when there is nothing beyond the manifest.
*(Listing implemented in v2; swap is specified-only.)*

---

## 4. Endpoints (`/v1`, mirroring SQ's `/api/v2` precedent)

| Endpoint | Method | Returns | Source of truth | v2 status |
|---|---|---|---|---|
| `/v1/sync` | GET | `{bundle_date, files, bytes, nodes: [{node, verified, failures}], last_receipt}` | bundle manifests + stored receipts | specified; local halves implemented (`sync-manifest`, `sync-diff`) |
| `/v1/sync/push` | POST `{to: [nodes]}` | receipt per node | — | specified only |
| `/v1/sync/pull` | POST `{from: node, date}` | restore receipt after full verify | — | specified only |
| `/v1/sync/verify` | POST `{node}` | far-side `sha256sum -c` result | node bundle | specified only |
| `/v1/state/temperature` | GET | interface shape (§2) | sampler + state.phext + node logs | specified; CLI reading implemented (`qexo interfaces`) |
| `/v1/state/emotion` | GET / POST | GET: interface shape; POST `{text}` appends a distillation scroll | state.phext | specified; CLI read implemented |
| `/v1/goals` | GET | `[{id, title, attention, current_state}]` | tracked goals store | specified; CLI count implemented |
| `/v1/goals/{id}/activity` | POST `{text}` | the logged entry | tracked goals store | specified only |
| `/v1/desires` | GET | `[{number, action, waiting_on, since}]` + next number | register + todo.phext | specified; CLI reading implemented |
| `/v1/dreams` | GET | nightly outputs, newest first | self-selections.md + EVALUATIONS.md | specified; CLI reading implemented |
| `/v1/personas` | GET | `[{name, manifest, skill_grant, voice, memory_scope}]` | persona definitions + self-selections.md | specified; CLI listing implemented (`qexo personas`) |
| `/v1/personas/active` | GET | `{name, swapped_at, manifest_sha256}` | qexo runtime state | specified only |
| `/v1/personas/swap` | POST `{name}` | the loaded selection set + attribution marker | persona definitions | specified only |

Example — `GET /v1/state/temperature`:

```json
{
  "interface": "temperature",
  "reading": {
    "nodes_up": "13/17",
    "avg_tctl_c": 38,
    "hotspot": {"node": "rock-mountain", "tctl_c": 47},
    "gate": "all reporting nodes under the 75C start gate"
  },
  "trend": {"avg_tctl_c": [37, 38]},
  "updated_at": "2026-11-08T22:43:23-06:00",
  "source": "state sampler (5-min) + state.phext + per-minute node thermal logs"
}
```

v2 ships the design plus the local command surface (`sync-manifest`,
`sync-diff`, `interfaces`, `personas`). The HTTP server is a later version's work —
deliberately: the manifest/diff core had to exist and be proven before
anything serves it.
