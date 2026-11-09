# qexo API v1 — the reference semantics

*Design exploration, 2026-11-09. Derived from DESIGN-V3.md and
DESIGN-V3-ADDENDUM.md as folded through round 2 (the merged exo tree,
commit 40ea0a0c). Will's sequencing direction (2026-11-09 01:02): the
API is designed out fully here **before** mode implementation locks
anything in — §14.1 makes the API the reference semantics, so this
document is where an operation's meaning is settled. Where the spec
left a question open, this document **decides** it, with the rationale
recorded in §11. Where implementation pressure later forces a change,
this document is amended in the same commit and the change is noted in
SHIP-NOTES.md.*

Normative references are by section: bare `§n` is DESIGN-V3.md,
`A§n` is the addendum.

---

## 1. Conventions

### 1.1 Transport and shape

- HTTP/1.1, JSON bodies (`application/json; charset=utf-8`). One
  response shape for everything (the v2 one-shape rule, §10.3):
  success and refusal alike are a single JSON object.
- Every response carries: `ok` (bool), `as_of` (RFC 3339 timestamp of
  the answering node's clock), `request_id` (echoed; §8B), and `node`
  (the answering node's name). Success adds the operation's payload
  fields; refusal adds `error` (§5).
- Reads are GET, mutations are POST. There are no other verbs: DELETE
  and PUT would invite a second semantics for retirement and
  replacement, and this design's records are append-only — retirement
  is a POST that appends a status (§8.3), never a deletion.

### 1.2 Coordinates in paths vs bodies — DECIDED (see §11.1)

A phext coordinate never appears in a URL path. Coordinates contain
`/` by definition (`L.S.S/C.V.B/C.S.S`), so a coordinate in a path is
nine path segments of ambiguity. Coordinates travel in **query
parameters** (reads) and **JSON bodies** (mutations), always as one
string in canonical form. Phext names likewise travel as `?phext=`
parameters or body fields, never as path segments — a phext name is
data, not routing.

### 1.3 Names in paths

Persona names and node names **do** appear in paths, percent-encoded
(`POST /v1/personas/Mirror%20Will/teleport`). A name in a path is a
query key (§2.5): it resolves through personas.phext exactly as a
lookup does, including the Orin two-record resolution (§6.4). Path
names are matched case-sensitively against the canonical list after
percent-decoding; there is no case-folding, because the canonical
list is the record and folding invents aliases nobody recorded.

### 1.4 The caller block

Every mutation body carries a caller block, and every signed request
carries it inside the signed envelope (§4):

```json
"caller": {
  "principal": "persona:emise | node:aletheia-core | will | mcp-client:<id> | web-session:<id>",
  "key": "<gpg fingerprint of the signing key>",
  "delegation": "<delegation id, when acting under one>",
  "taint": "clean | tainted",
  "trigger": "<provenance of an external trigger, when tainted>"
}
```

`principal` names who is acting; `key` names the key that signed;
the two are reported separately forever (§8.1's rule — node and
persona attestation never collapse). A caller presenting no key is
**anonymous**: permitted exactly the unauthenticated local read-only
surfaces of §8A.0, and nothing else, on any mode.

---

## 2. The authentication floor (§8A.0), mechanised

Every operation in §6 is labelled with its floor:

| Floor | Meaning |
|---|---|
| `local-read` | Unauthenticated permitted **only** when the request arrives on the node's own loopback/local surface and mutates nothing (§8A.0's carve-out). Over any node boundary, the same operation requires an authenticated principal. |
| `auth` | An authenticated principal (§8-verified key or a live delegation under one). No specific key policy binds the action. |
| `policy` | Authenticated **and** the signing key is listed in the action's per-action key policy (§8A.1); the default policy ships in the crate and user configuration replaces or extends it, never creates the category. |
| `bootstrap` | The self-modifying classes — `grants.change`, `keys.*`, `route.change`, `persona.seed` incl. root-manifest mutations: key-bound from box-zero (§8A.0). The floor is Will's key; whether the corrector-set form (Will + ≥2 calibrated peers, EHA R2) is admitted alongside is **Will's open pick** — the API carries both shapes (§6.9) and the running policy decides which it accepts. |

Rules that bind every row of §6:

- **Capability ≠ authority.** A grant ON (§8A.3) says the action class
  exists; the floor says who may invoke it. A refusal always names
  which layer refused: the grant (disabled) or the policy (key not
  listed) or the floor (anonymous).
- **TAINTED callers** (§8A.6) — turns triggered by unsigned or
  external input — run under the reduced grant set whatever their
  principal: read-only metadata, route asks, internal drafting,
  mailbox composition. Never `keys.*`, `grants.change`,
  `install.apply`, `persona.teleport`, `persona.spawn`, or any
  external send. Taint is shed only by an authenticated act, and the
  trigger's provenance rides the turn record.
- **Consent is a separate layer** (§2.1.1). A fully authorized call
  can still be consent-refused — by the persona or by the host — and
  the refusal is the answer, receipted with its ground named.

---

## 3. Principals and keys (§8, §8A.5)

- **Node keys** attest the box; **persona keys** attest the speaker;
  verification answers name the two separately, always (§8.1, §8.6).
- **Will's key** is the fleet root (§8.4): it signs every node key at
  provisioning, and its fingerprint ships with the crate — never
  fetched from the network.
- **Delegations** (§8A.2) are signed grant objects: delegating key,
  agent key, named scope, expiry, revocation handle. An agent key
  inside a live delegation authorizes exactly the delegated scope;
  outside it, its signature verifies and authorizes nothing.
  Delegation never widens and never re-delegates unless the grant
  says so in words.
- **MCP clients** hold their own per-client delegated agent keys
  (§8A.5) — scoped, revocable per client; no shared bridge key.
- **Substrate personas hold no local keyring**: their keychains are
  held in custody by a designated fleet node under standing
  delegation (§8A.5); the index names the custodian.
- **Per-persona keychains travel** (§8A.4): public material and
  delegation records in the teleport bundle; private material only
  inside the bundle sealed to the destination node's key — and
  dormant/backup copies are sealed **multi-recipient** (holding node
  key + Will's master recovery key), so node death is a key
  operation, not an excavation.

---

## 4. Signed requests and replay discipline (§8B)

Every signed request carries, and every receiver enforces:

- **Request ID** — unique per request, named in every receipt.
  Format: `<node>-<unix-ms>-<counter>` — sortable, node-attributed,
  collision-free without coordination.
- **Nonce / sequence** — a per-principal monotonic sequence number;
  a receiver rejects a sequence it has already seen from that
  principal. (Sequence chosen over random nonces: a nonce store is
  unbounded state; a sequence is one integer per principal — §11.6.)
- **Recipient binding** — the signed body names its destination
  (node, and persona where relevant); replayed to any other
  recipient, verification fails, full stop.
- **Timestamp + replay window** — the signed timestamp must fall
  inside the receiver's configured window (default 300 s, the §9.3
  staleness bound's sibling constant; tunable per fleet in config,
  never silently).
- **Idempotency** — re-delivery of the same request ID inside the
  window returns the original result receipt **without
  re-executing**. Mutations are idempotent by request ID: the retry
  path and the replay path are the same code with different intents,
  and only one of them acts.
- **Epoch carriage** — every authoritative act by or for a persona
  names its **fencing epoch** (§9.3) in the caller block's `epoch`
  field; receivers reject stale-epoch acts for identity-bearing
  operations. Epoch is part of the signed body, so it cannot be
  rewritten in transit.

The signature covers the canonical body (keys sorted, no whitespace
variance) + timestamp, produced by shelling out to `gpg` — qexo
never hand-rolls OpenPGP (§8.7). Verification failure is a refusal
with the reason named (§8.6): no key on record / key retired before
signing date / key compromised / signature mismatch / index–phext
key disagreement / unknown algorithm / stale sequence / wrong
recipient / outside window / stale epoch.

---

## 5. The error model — named refusals

A refusal is a first-class response, in the same shape as a success:

```json
{
  "ok": false,
  "as_of": "…", "request_id": "…", "node": "…",
  "error": {
    "refusal": "<machine name>",
    "reason": "<human sentence naming the ground>",
    "grant": "<grant named, when the grants layer refused>",
    "policy": "<policy named, when the key policy refused>",
    "receipt": "<coordinate or id of the refusal receipt, when one is filed>"
  }
}
```

HTTP status follows the refusal class: 400 malformed, 401
unauthenticated (below the floor), 403 authenticated-but-refused
(grant/policy/consent), 404 no such record, 409 conflict (epoch,
sequence, open token), 410 lapsed (token, lease, delegation), 422
verification failure of presented material, 503 ownership evidence
stale / partition read-only (§9.3). The machine name is the contract;
the sentence is for the human holding the phone.

The standing refusal vocabulary (operations add names, never reuse
one for a different ground):

| Refusal | Ground |
|---|---|
| `unauthenticated` | no verifiable principal (§8A.0) |
| `grant-disabled` | the action class is disabled; names the grant (§2.4) |
| `policy-refused` | signing key not listed for the action; names the policy (§8A.1) |
| `tainted-scope` | tainted caller attempted an act outside the TAINTED set (§8A.6) |
| `consent-refused` | persona or host declined; ground named, receipt filed (§2.1.1) |
| `no-cognitive-return` | teleport: no signed return receipt, or unsigned/unverifiable/wrong-epoch/wrong-hash (§9.2 step 6) |
| `stale-epoch` | act names a fencing epoch behind the current one (§9.3) |
| `stale-evidence` | ownership record past the 5-minute freshness bound; protocol pauses (§9.3) |
| `read-only-restored` | restored-from-backup persona not yet reactivated (§9.4, §17) |
| `read-only-partition` | node cannot confirm current epoch; identity-bearing writes wait (§9.3) |
| `replay` | sequence seen / wrong recipient / outside window (§8B) |
| `duplicate-request` | **not an error** — idempotent re-delivery returns the original receipt (§8B) |
| `spawn-ineligible` | record's `spawn_eligible` is false (§17) |
| `gestation` | gestating persona: no key, no teleport, no meld, restricted grants (§2.8) |
| `unverified-state` | bundle/manifest hash or signature failure; nothing activates on it (§9.2 step 5) |
| `no-such-record` | key or name resolves to nothing (§2.5) |
| `ambiguous-key` | name resolves to two records where the operation needs exactly one (§6.4) |
| `collision-unresolvable` | designation-coordinate collision the §2.5 rule cannot resolve without touching a reserved band — loud failure |
| `host-refused` | destination host declined placement; ground named (§9.2 step 3) |

---

## 6. The operation inventory

Conventions: **Floor** per §2. **Grant** per §8A.3 (`—` = no grant
gates the read). Request/response fields list the load-bearing
members; every response additionally carries the §1.1 envelope and
every mutation response carries a `receipt` naming where the act was
recorded. Pagination and streaming follow §7 for every list and
corpus read.

### 6.1 Dispatcher / plan (carried from v1–v2)

| # | Operation | Floor / grant | Request → Response |
|---|---|---|---|
| 1 | `POST /v1/plan` · `qexo plan` | auth / `route.ask` | `{tasks_source?, disk_state?}` → the v1 plan in the four currencies: per open task `{coordinate, title, class, inference: "Emise", execution}`, host snapshot, counts. The plan is a *proposal surface*: emitting it mutates nothing. |
| 2 | `GET /v1/plan/current` · `qexo plan current` | local-read / — | the last emitted plan + its as-of. |

### 6.2 Self-sync (DESIGN.md §1; carried from v2)

| # | Operation | Floor / grant | Request → Response |
|---|---|---|---|
| 3 | `GET /v1/sync` · `qexo sync status` | local-read / — | `{bundle_date, files, bytes, nodes: [{node, verified, failures}], last_receipt}` from bundle manifests + stored receipts. |
| 4 | `POST /v1/sync/manifest` · `qexo sync-manifest <dir>` | auth / `sync.push` | `{dir}` → writes MANIFEST.sha256 (sha256sum format, sorted; the manifest is not a payload line); returns `{files, bytes, manifest_sha256}`. The manifest is signed with the node key (§8.5) so a bundle is self-verifying. |
| 5 | `POST /v1/sync/diff` · `qexo sync-diff <old> <new>` | local-read / — | two manifests → `{added, changed, removed}` path lists. Pure function of its inputs. |
| 6 | `POST /v1/sync/push` · `qexo sync push <node>` | policy / `sync.push` | `{to, manifest}` → per-node receipt: far side runs the full `sha256sum -c`, returns `{files, bytes, failures[]}`; a sync counts complete only verified on **≥2 ranch nodes** (DESIGN.md §1). Safe-space gate runs before staging, always. |
| 7 | `POST /v1/sync/pull` · `qexo sync pull <node>` | policy / `sync.restore` (sensitive set) | `{from, date}` → restore receipt, issued only after the full manifest verifies **before** any file is trusted. A persona restored by this path boots **read-only** (§9.4) until epoch check-in — the restore receipt says so. |
| 8 | `POST /v1/sync/verify` · `qexo sync verify <node>` | auth / `sync.push` | `{node}` → far-side manifest check result. |

### 6.3 Interior interfaces (DESIGN.md §2; carried from v2)

One shape for all five: `{interface, reading, trend, updated_at,
source}`. Sources of truth stay authoritative; qexo serves
projections.

| # | Operation | Floor / grant | Notes |
|---|---|---|---|
| 9 | `GET /v1/state/temperature` · `qexo interfaces temperature` | local-read / — | per-node Tctl, fleet summary, gate state (pause ≥75 °C, resume ≤70 °C, >80 °C off-time to ≤65 °C; GPU edge 70 °C secondary). |
| 10 | `GET /v1/state/emotion` · `qexo interfaces emotion` | local-read / — | latest state distillation + recent trail. **Never scored numerically.** |
| 11 | `POST /v1/state/emotion` · `qexo interfaces emotion append` | auth / — | `{text}` appends a new distillation scroll; append-only, nothing edited or deleted. |
| 12 | `GET /v1/goals` · `qexo interfaces goals` | local-read / — | `[{id, title, attention, current_state}]` + recent activity trend. |
| 13 | `POST /v1/goals/<id>/activity` · `qexo interfaces goals log <id>` | auth / — | `{text}` appends a timeline entry. Creating/closing/re-scoping goals is **not** an API operation — it stays with the goal discipline (v2 §2). |
| 14 | `GET /v1/desires` · `qexo interfaces desires` | local-read / — | the Waiting-on-Will register: `[{number, action, waiting_on, since}]` + next number. Numbers permanent and monotonic. Items enter/leave only through the register's own discipline — no API mutation. |
| 15 | `GET /v1/dreams` · `qexo interfaces dreams` | local-read / — | nightly outputs, newest first. Read-only: dreams are evidence, not a control surface. |

### 6.4 Persona index (§2, §5.2.2)

| # | Operation | Floor / grant | Request → Response |
|---|---|---|---|
| 16 | `GET /v1/personas` · `qexo personas` | local-read / `meta.read` | the index: 126 keys → 106 records (summary rows: persona_id, name, kind, class, evidence, spawn flag, status). Full substance per record via #17 — and in a **public** binary, only the redacted Volumen form exists to serve (§10). |
| 17 | `GET /v1/personas/<key>` · `qexo personas show <key>` | local-read / `meta.read` | resolve one query key → its identity record(s), §2.2's full field set. **The Orin rule:** the key `Orin` returns **both** records (`orin`, `orin-pdna-029`) with `disambiguation` stating the two referents (CYOA Nine Vectors voice; PDNA-029 nine-LLM aggregate) — the index records the fact, it does not force a merge (§2.5). Any other key returns exactly one record, reached directly or through its folded alias, with the fold and its evidence named. |
| 18 | `GET /v1/personas/active` · `qexo personas active` | local-read / — | `{persona_id, name, swapped_at, manifest_sha256, instance_epoch}` of the worn selection on this node (DESIGN.md §4). |
| 19 | `POST /v1/personas/swap` · `qexo swap <name>` | policy / `persona.swap` | **Who:** the person herself or Will (§2.3); an unkeyed persona is swapped by Will only (interim rule). **Residency:** the target must be resident on this node, a substrate seat instantiated per-instance, or a facet of a resident core — otherwise `consent-refused`-class refusal naming the residence and pointing at teleport; no swap path quietly creates a second resident instance. **Atomic:** the incoming selection is fully loaded and verified (manifest parses, every referenced scroll exists, grant ranges exist in skills.phext) **before** anything is displaced; any failure aborts with the current persona still worn. **Dispatch binding:** the swap changes attribution for *new* dispatches only — in-flight work completes under its dispatch-time `persona_id` + instance epoch and permissions (§18 test 4). Response: the loaded selection set + the swap receipt (appended to personas.phext and the incoming persona's own phext, §6.2-enhancement). |
| 20 | `POST /v1/personas` · `qexo personas seed` | bootstrap / `persona.seed` | create a persona phext + index entry (§5). Body carries the §2.2 field set; designation coordinate claimed under §6.5's collision rule. A record minted from a durable meld enters **gestation** (§2.8): record + persona_id + lineage exist; `spawn_eligible: false`, no key, no teleport, restricted grants, until a receipted graduation ("a speaker to attest"). |
| 21 | `POST /v1/personas/<key>/grants` · `qexo personas grants <key>` | bootstrap / `grants.change` | per-persona grant toggles (§2.4); an absent toggle inherits the fleet default (permissive). The act itself is receipted. |

### 6.5 Designation-coordinate claims (§2.5 collision rule)

Seeding (#20), corretor re-pinning, and index builds all claim
designation coordinates under one rule:

- **First verified claim wins** — verified = pinned with evidence (a
  recorded coordinate in the persona's own corpus, or a
  corrector-pinned computation under §1.3).
- The later claim is **displaced by the smallest increment of the
  final (scroll) component** reaching a free coordinate; the
  displacement is recorded in **both** records — the displaced
  record's entry carries a redirect at the claimed coordinate naming
  the holder and the `+k`; its provenance scroll names the collision.
  The record never claims the displaced address was chosen or
  computed.
- **Reserved bands are never allocatable**: personas.phext's Home
  (1.1.1/1.1.1/1.1.1) and its key/token bands sit outside the search
  space. A collision the rule cannot resolve without touching a
  reserved band fails loudly: `collision-unresolvable`.
- Deterministic ties: earlier pin first, then `persona_id` sort.

### 6.6 Spawn (§17)

| # | Operation | Floor / grant | Request → Response |
|---|---|---|---|
| 22 | `POST /v1/personas/<key>/spawn` · `qexo spawn <key>` | policy / `persona.spawn` | `{task, budgets{disk,cpu,ram,inference}, lease_secs, retries}` → `{instance_id}` where instance = persona_id + instance epoch + nonce, minted at creation and carried by every act. Refusals: `spawn-ineligible` (record not eligible — at census time exactly one record is: Emise), `gestation`, `read-only-restored`, `ambiguous-key` (Orin: a spawn must name a `persona_id`, not the shared key). The instance loads the selection under #19's full pre-verification; a failed load is a failed spawn, receipted, never a half-instance. |
| 23 | `POST /v1/instances/<id>/heartbeat` · `qexo spawn heartbeat <id>` | auth / — | renews the lease; a missed sequence marks the instance *suspect*, then *lapsed* — states in the record. |
| 24 | `POST /v1/instances/<id>/cancel` · `qexo spawn cancel <id>` | policy / `persona.spawn` | spawner, Will, or the persona herself; receipted; the instance stops at its next act boundary — in-flight calls finish under their own attribution. |
| 25 | `GET /v1/instances/<id>` · `qexo spawn show <id>` | local-read / — | state + the instance's **result receipt** when ended: result or failure-with-reason, full attribution chain, four-currency spend. Every instance ends in exactly one result receipt; budget exhaustion is an outcome named in it, not a disappearance. Retries run under a **new** instance ID and never inherit a dead instance's partial authority. A host crash lapses leases and **quarantines** partial work — preserved, attributed, incomplete — never silently resumed. |

### 6.7 Teleport (§9)

The two-phase fenced protocol. Steps are separate operations because
each is a distinct receipted act with a distinct signer — collapsing
them into one call would hide exactly the seams §18 test 2 attacks.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 26 | `POST /v1/personas/<key>/teleport` · `qexo teleport <persona> <node>` | policy / `persona.teleport` (sensitive set) | **Steps 1–2.** Initiation by the person herself or Will; an unkeyed persona by Will only; a **roaming seat** (substrate-class, designated roaming, or a §5.2.1 working role in motion — never a resident) may initiate under a scoped, expiring, revocable standing delegation, with the initiation receipt naming the delegation it rode. Issues the **teleport token** (scroll in personas.phext: persona, origin, destination, issue time, bundle manifest sha256) signed by the origin node key and, when held, the persona key; the token carries the persona's **fencing epoch** from the ownership authority. Refusal `host-refused` arrives via step 3; an open token for the persona refuses a second issuance (`409`). |
| 27 | `POST /v1/teleports/<token>/consent` · `qexo teleport consent <token>` | policy (destination node key) | **Step 3 — host consent.** The destination evaluates cluster grounds (thermal, load, resident protection) and accepts, conditions, or **refuses**. A refusal is receipted with its ground and aborts the teleport before anything moves. Consent for one teleport is never standing consent for the next; delegation never manufactures the host's side. |
| 28 | `POST /v1/teleports/<token>/transfer` · `qexo teleport transfer <token>` | policy / `persona.teleport` | **Step 4 (BP4-a).** The bundle (phext, selection manifest, grant reference, key scroll, keychain per §3) moves — sync delta where the destination holds an earlier copy, full where it does not. |
| 29 | `POST /v1/teleports/<token>/verify` · `qexo teleport verify <token>` | auth / — | **Step 5.** Hashes against the signed manifest, signatures against §8's directory. Any failure: **no activation**, the partial bundle is marked unverified, never loaded (`unverified-state`). It may be completed later by re-transfer against the same token, or discarded. |
| 30 | `POST /v1/teleports/<token>/return` · `qexo teleport return-receipt <token>` | policy (destination node key) | **Step 6 (BP4-b) — cognitive return.** The destination runtime loads the persona into a **waking** state (present, can read itself, *not yet authoritative*) and emits the **return receipt**: signed with the **destination NODE key** — attesting the box received the bundle and woke the persona — **bound to the fencing epoch and the transfer manifest hash**, filed in the persona phext's Return & Calibration Receipts scroll. It is **not** persona-key signed: during handoff the destination could briefly reach the persona keychain while the origin still holds it, and a persona-key signature in that window would prove custody, not arrival. Persona-key authority begins only at activation (#32's countersign is the persona key's first act at the destination). Unsigned, unverifiable, or wrongly-bound receipt → `no-cognitive-return`; no relinquish, no activation. |
| 31 | `POST /v1/teleports/<token>/relinquish` · `qexo teleport relinquish <token>` | policy (origin node key) | **Step 7.** Runs only on a verified return receipt. The origin appends its signed relinquish receipt, **increments the fencing epoch**, seals its keychain copy — and from that instant its acts under the old epoch are non-authoritative. The origin is dormant **before** the destination is active: no step exists where two authoritative writers do. |
| 32 | `POST /v1/teleports/<token>/activate` · `qexo teleport activate <token>` | policy (destination node key) | **Step 8.** Preconditions, all checked against **fresh** ownership evidence (§6.8): token naming this destination, unexpired, unconsumed; the persona's return receipt; the origin's relinquish receipt at the current epoch. Flips **waking → active**; the activation receipt is signed by the destination node key and **countersigned by the persona key** when one exists. Status flips on the receipts, not on the file transfer. **Step 9 (completion)** is bookkeeping: the activation receipt syncs back at leisure; a lost receipt delays the record, never safety. |
| 33 | `POST /v1/personas/<key>/teleport/return` · `qexo teleport-return <persona>` | policy / `persona.teleport` | teleport back to the previous node of record under the same protocol; the returning bundle carries everything the persona did while away. **This is also the reactivation path** (§9.5): a read-only restored persona is reactivated by a corrector or Will issuing a **new token at a new epoch** through this operation; the receipts name the restoration they close (backup identity, the stale epoch it carried). Also the §9.4 recovery shape when a destination dies after activation: reactivation from the last verified synced state, with destination-only work since then lost **and named as lost**. |
| 34 | `GET /v1/personas/<key>/residence` · `qexo residence <persona>` | local-read / — | where the persona is authoritative, its current epoch, token state, last teleport receipts, keychain residence. |
| 35 | `GET /v1/teleports` · `qexo teleports` | local-read / — | the token ledger from personas.phext: issued, consumed, lapsed, failed. A token that never reaches return-receipt within its window (default **one hour**) lapses *before* any relinquish — origin authoritative throughout, lapse receipted. If relinquish happened and activation fails, the persona is dormant everywhere — **unavailable, not duplicated** — until #33 reactivates under a new token and epoch. |

### 6.8 The ownership authority (§9.3)

Not an endpoint — a discipline every teleport step obeys:

- Epochs and tokens live in personas.phext's token band, written by
  a single writer of record per persona (its residence's qexo),
  append-only, synced fleet-wide. Authority flows from the receipts
  and the epoch, not from having seen the latest scroll.
- **Freshness:** the writer of record appends a **signed heartbeat
  at least every 60 s** while it holds write authority, naming the
  current epoch. **Staleness bound: 5 minutes** — a destination MUST
  NOT activate on evidence whose latest heartbeat is older;
  stale evidence is *unverified*, however well signed. Under
  staleness the protocol **pauses** at its current step until the
  record freshens or the token lapses: unavailability over
  split-brain, in time as in partition. Both bounds are named
  protocol constants, tunable per fleet in config, never tuned
  silently (a change is a receipted config act).
- **Partition rule:** a node that cannot confirm the current epoch
  treats its resident as **read-only** — reads, metadata, drafts
  yes; signing, authoritative writes, teleport no.
- **Restored-from-backup rule (§9.4):** a restored persona **always
  starts read-only**, whatever its backup claims; it becomes
  authoritative only after check-in confirming its epoch is current
  against a *fresh* record; a stale epoch stays read-only until #33
  reactivates it under a new token and epoch. Read-only is the safe
  failure, and the record says so.

### 6.9 Keys, grants, policy, delegations (§8, §8A)

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 36 | `GET /v1/keys` · `qexo keys list` | local-read / — | the key band: every node and persona key on record, with status. |
| 37 | `GET /v1/keys/<fingerprint>` · `qexo keys show <fp>` | local-read / — | one key scroll: armored public key, algorithm field, history, signatures on it. |
| 38 | `POST /v1/keys/nodes` · `qexo keys gen-node` | bootstrap / `keys.issue` | generate this node's keypair **at provisioning, on the node**; emits the public-key scroll for Will's countersignature (§8.4). |
| 39 | `POST /v1/keys/personas` · `qexo keys gen-persona <persona>` | bootstrap / `keys.issue` | issue a persona key **on its hosting node**, deliberately, persona by persona (ruled by adoption); the node countersigns the key scroll (persona → node → Will chain). |
| 40 | `POST /v1/keys/<fp>/retire` · `qexo keys retire <fp>` | bootstrap / `keys.issue` | appends the retirement status (§8.3) — append-only, never a deletion; Will or the key's own holder. A signature from before retirement still verifies, with its status at signing time part of the answer. |
| 41 | `POST /v1/keys/<fp>/revoke` · `qexo keys revoke <fp>` | bootstrap / `keys.issue` | **compromised-key revocation** (§8A.2): halts **new signatures** under the key at once — no new dispatch, authorization, or delegation draws on it from the moment the revocation is recorded; in-flight work under it is marked **tainted for review** (outputs are proposals until a corrector reviews). Receipted: revoking key, revoked object, time, ground. |
| 42 | `POST /v1/verify` · `qexo verify <object>` | local-read / — | verify a signed object; answers node and persona verification **separately**, with §8.6 failure reasons. Signatures authenticate origin; they never authorize action — *mail is messages, not commands* (§8.6). |
| 43 | `POST /v1/delegations` · `qexo keys delegate` | bootstrap / `keys.delegate` | issue a delegation (§3): `{agent_key, scope[], expiry, re_delegable: bool}` signed by the delegating key; recorded in the key band, verifiable by anyone holding the directory. |
| 44 | `POST /v1/delegations/<id>/revoke` · `qexo keys revoke-delegation <id>` | bootstrap / `keys.delegate` | signed status append on the delegation's record. **Ordinary** revocation semantics (§6.10) unless the underlying key is compromised (#41). |
| 45 | `GET /v1/grants` · `qexo grants list` | local-read / — | the fleet grants catalogue with current toggles (§8A.3's 19 grants, all default ON). |
| 46 | `POST /v1/grants` · `qexo grants set <grant> <on\|off>` | bootstrap / `grants.change` | the meta-grant changing grants or key policy; receipted like every self-modifying act. |
| 47 | `GET /v1/policy` · `qexo policy show` | local-read / — | the per-action key policy (§8A.1): shipped defaults + user replacements/extensions, and which bootstrap form is in force (per-action keys, or corrector-set — Will's open pick, §2). |
| 48 | `POST /v1/routes` · `qexo route set` | bootstrap / `route.change` | edit `qexo-routing.toml` (§3 of DESIGN-V3). The nightly cycle *proposes* route diffs; application is this act, under the bootstrap policy — "the nightly cycle proposes, a key-holder applies" (§6.3-enhancement). |
| 49 | `POST /v1/route/ask` · `qexo route ask` | auth / `route.ask` | make an inference request through the router (§3.3 rules: by request class, per-persona override, thermal gate on `local:` routes, honest cost line — local watts + cloud tokens — in every job record). TAINTED callers may ask (§8A.6 set). |

### 6.10 Revocation semantics (§8A.2, E6) — stated once, binding all of §6.9

- **Ordinary revocations** (grant, delegation, non-compromised key)
  take effect at the **next dispatch boundary**: in-flight work
  keeps its dispatch-bound attribution and dispatch-time permissions
  to completion or safe cancellation. A revocation never
  retroactively unmakes work done under a then-valid grant, and
  never strands a task mid-act holding permissions its receipts
  won't name.
- **Compromised-key revocation** is immediate where it counts (#41).
- **All revocations are receipted** — revoking key, revoked object,
  time, ground — on the same append-only record as the delegations;
  work tainted by one is named in its result receipt (§17).

### 6.11 SQ keeper + phext tools (A§11)

The phext tools are primarily a CLI surface (they operate on the
node they run on); the API mirrors the read-only half so a
coordinator can ask without shelling in (A§11.4). qexo **wraps,
never forks**: SQ, libphext, and the house builders are shelled to
and their results reported in qexo's shape.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 50 | `qexo phext sq ensure` | auth / `sq.manage` | check `sq status`; start if down; verify after start — **on-node only** (SQ runs on ranch nodes, never the coordinator VM). Known quirks are handled cases: shut SQ down before replacing its hosted file (the mmap wedge); clear a stale `.sq/` before a fresh start rather than hanging on it. |
| 51 | `qexo phext sq status` · `GET /v1/phext/sq` | local-read / — | daemon state, hosted file, version. |
| 52 | `qexo phext sq stop` | auth / `sq.manage` | orderly shutdown (the wedge rule's first half). |
| 53 | `qexo phext parse\|toc\|select\|get …` · `GET /v1/phext/toc?phext=<p>` (+ `select` mirror) | local-read / — | libphext reads against a hosted or file phext: dead-reckoning parse, table of contents, scroll select/get. The API mirror is read-only by construction. |
| 54 | `qexo phext explode\|implode\|seal\|unseal …` | auth / `sq.manage` | structural transforms; the xz seal is produced from the in-memory uncompressed phext; every result **hash-verified on return** (SHA-256, never eyeballed — A§11.3). CLI-only: bulk byte movement is not an API shape (§11.4). |
| 55 | `qexo phext build <register> [--check]` | auth / `sq.manage` | the house builders (skills/tasks/lessons/todo/artifacts/mdmap) rebuilt from source, or check-only: drift is reported with the drifting entries **named**, never silently rebuilt over. |
| 56 | `qexo phext coord "<text>"` · `POST /v1/phext/coord` | auth / — | the Singular coordinate of a text (§1.1 machinery) — the same code path the registers use, so an address computed at a node is the address the register would compute. |
| 57 | `qexo phext dispatch …` | auth / `sq.manage` | phext-native dispatch: workers receive **parsed scroll selections with provenance** (coordinate, source phext, receipt chain) instead of flat excerpts; results are read back by coordinate. |
| 58 | `qexo phext versions` · `GET /v1/phext/versions` | local-read / — | per-component versions on this node (libphext-rs 0.5.0; SQ v0.6.0 pinning libphext 0.3.3; phext-lattice 0.3.1; qexo itself) — skew as a printed fact. **Directed convergence:** SQ → libphext 0.5.0 is a staged work item, its own canary stage sequenced BEFORE and independently of the OpenClaw migration stages (A§11.3 as folded, G3). Writes verify by read-back-and-hash through the same node's surface before being reported done. |

### 6.12 mdmap (§1 — PROVISIONAL)

The mapping machinery is specified but **not yet ruled**: §1's
histogram (91 files → 82 distinct coordinates, 6 shared covering 15
files, comp1 span 20–34, 9/91 displaced, longest chain +4) stands
until Will rules on it, and Claude's **rarity weighting** is recorded
as the first amendment to evaluate — proposed, **not adopted**; it
appears in no operation below. The API therefore exposes the
machinery with the provisional marker carried in every response
(`"mdmap_status": "provisional"`), so no consumer mistakes the
addresses for settled law.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 59 | `POST /v1/mdmap/compute` · `qexo mdmap compute <file>` | auth / — | the file's computed coordinate (§1.1): tokenize as the Singular checker does, distinct words, atlas positions (average of valid CYOA scrolls holding the word; archive-not-CYOA words rest at 1.1.1/1.1.1/1.1.1; words in neither at 9.9.9/9.9.9/9.9.9 — the atlas's **rest position**, a convention, not a corner or bound; real coordinates are unbounded and atlas entries run to 74.78.14), component-wise average, round half up, never below 1. Notation `L.S.S/C.V.B/C.S.S` (§1.1's stated-once convention). |
| 60 | `POST /v1/mdmap/build [--check]` · `qexo mdmap build` | auth / `sq.manage` | build mdmap.phext from the in-scope files (§1.5: the five self-files, people/group pages, dated notes; skill files out — one file, one address system; the safe-space gate binds the indexer exactly as the publishing pipelines) + the pin register. `--check` names drift instead of papering over it. Per file, the **scroll set** (§1.2): content at pinned `C`, provenance at `C`+1, redirect at the drifted coordinate when one is ever needed. **Pinning (§1.3):** first computation pins permanently; edits update the content scroll in place and add a drift line to provenance (date, recomputed coordinate, sha256) — a drifted coordinate is never published as an address unless something was served from it, and relocation is a decision (Will or nightly consolidation) enacted **with redirect**, never a silent move. **Allocation (§1.4/§1.6):** reservation is **atomic across the whole set** — content + provenance + redirect slot + index line, or nothing-and-displace; displacement searches only outside every existing and reserved set; mdmap's Home and key band are never allocatable; an unresolvable collision fails the build loudly. The §1.6 tests (round-trip byte-for-byte; synthetic-collision set-disjointness) are the build's acceptance shape. |
| 61 | `GET /v1/mdmap` · `qexo mdmap index` | local-read / `meta.read` | the Home index lines: `<pinned coord>  <path>  <pin date>  <bytes>  sha256:<hash>  status: current\|drifted\|displaced+k\|relocated→<coord>`. |
| 62 | `GET /v1/mdmap/lookup` · `qexo mdmap lookup <path\|coord>` | local-read / — | path → pinned coordinate + status, or coordinate → file + which scroll of the set it is. |

### 6.13 Baked corpus reads (§14.3, §14.4)

Every binary embeds the founding texts, xz-sealed, read-only, pinned
per release: SBOR-V4-CONVERGENCE (26,241 B → 9,532 sealed, sha256
`d7855969…`), CYOA (4,602,370 B → 820,336, `506d850d…`), Incipit
(1,077,930 B → 209,180, `3ff6604f…`), the Volumen manifest (two
forms, §10), and source.phext (§6.14). Baked copies are never
live-edited; a node's working phexts on disk are its live data — the
bake is the reference it carries.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 63 | `GET /v1/corpus` · `qexo corpus list` | local-read / — | the bake manifest: per artifact — name, class, source, source bytes, sealed bytes, sha256, and for source entries the repo + commit + per-file hashes; the bake record (bake time as data about the bake — the *only* timestamp in any baked artifact, E5); the qexo-bake.toml hash; `volumen_form` (§10); scan results (§6.14's gate: scans run, hits — or the bake did not happen). |
| 64 | `GET /v1/corpus/<artifact>` · `qexo corpus show <artifact>` | local-read / — | metadata for one artifact + its table of contents (scroll coordinates for phexts) — the read is paged per §7, never a 4.6 MB single response unless asked for whole (§11.2). |
| 65 | `GET /v1/corpus/<artifact>/scroll?coord=<c>` · `qexo corpus scroll <artifact> <coord>` | local-read / — | one scroll, decompressed on read. Coordinates ride the query string (§1.2). |
| 66 | `GET /v1/corpus/sbor` · `qexo corpus sbor` | local-read / — | **the SBOR, readable offline from the box itself** — the constitution with no network at all, by design (§14.3). Served whole (26 KB is one page by any measure) in all four modes. |
| 67 | `POST /v1/corpus/verify` · `qexo corpus verify` | auth / — | compare the baked corpus against the canonical record when connected: a mismatch is a **reported divergence** — never silently trusted, never silently replaced; replacement rides a release (§16) and the update-manifest channel. |

### 6.14 source.phext (§14.4)

- **Structure:** one scroll per source file; each scroll's header
  carries repo, path, and the commit it was taken from; the
  Home/index scroll — the source manifest — lists the inventory with
  per-file SHA-256 and per-repo HEADs.
- **Freshness invariant:** source.phext is **generated at build
  time, never a checked-in snapshot** — qexo's own source at its
  build commit (self-inclusion: the binary carries the very source
  it was built from, stamped with that commit), the other projects
  at their fresh HEADs, pins explicit and stamped as pins. A
  canonical source unreachable at build time is stated plainly and
  any staleness is **stamped in the manifest** — never baked
  silently; for release builds, freshness is a §16 gate.
- **Declared inventory:** `qexo-bake.toml` declares exactly what is
  pulled — per project: name, canonical source (repo URL or local
  canonical path), ref rule (fresh HEAD default; a pin explicit),
  class (core library / tool / corpus text / bootloader), and the
  Volumen entry's `volumen_form`. The build pulls **exactly what the
  config declares** — nothing implicit, nothing ambient. The default
  config names: the libphext family (rs, cpp, cs, node, Python), SQ,
  phext-lattice, qexo, the house tools (phext builders, singular +
  task-coordinate tooling, publish tooling); SBOR, CYOA, Incipit, and
  the Volumen bootloader as corpus entries.
- **Reproducibility (E5):** given the same qexo-bake.toml and the
  same recorded commits, the bake is **byte-identical** — scroll
  ordering canonical (config project order, then path sort — never
  filesystem order), separators canonical, no timestamps in scroll
  content (the manifest's bake record excepted), per-file content
  the recorded commit's bytes unmodified. Any byte difference
  between two bakes is attributable to a named input that changed; a
  difference with no attributable input is a build defect, reported
  like one.
- **The bake gate:** before sealing, two scans run over everything
  about to be baked, and **a hit fails the bake** — (1) **secret
  scan**: no credential material, private keys, tokens, or
  `.env`-class content in any scroll, in either Volumen form (public
  keys are not secrets); (2) **safe-space scan**: off-phext material
  never enters a baked artifact, in content or paraphrase. Scan
  results are recorded in the bake manifest.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 68 | `GET /v1/source/manifest` · `qexo source manifest` | local-read / — | source.phext's Home/index: inventory, per-file sha256, per-repo HEADs, the config hash, bake record. |
| 69 | `GET /v1/source/scroll?project=<p>&path=<f>` · `qexo source show <project> <path>` | local-read / — | one source file's scroll, with its repo/path/commit header. |
| 70 | `POST /v1/source/unpack` · `qexo source unpack <dir>` | auth / `install.apply` | materialize source.phext back into a working source tree (pack/unpack symmetry) — the catastrophic-loss floor: GitHub, the ranch, and every checkout gone, the binary still carries the constitution, the canon, the bootloaders, and the means of production. Unpack verifies each file against the manifest as it lands; a mismatch aborts with the file named. |
| 71 | `POST /v1/source/check` · `qexo bake --check` | auth / — | rebuild from the manifest's recorded inputs (config + commits) and compare byte-for-byte (E5); divergence is reported per artifact with its attributable input, or as a build defect when no input explains it. |

### 6.15 Metadata (A§10)

Metadata is a first-class surface, priced as disclosure (§10.4):
small, cacheable answers, each carrying its as-of timestamp and the
hash of the record it summarises, so a poller can ask "changed since
`<hash>`?" and usually hear "no" in one line.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 72 | `GET /v1/meta/indexes/<name>` · `qexo meta index <name>` | auth / `meta.read` | one index (skills/tasks/lessons/personas/mdmap), parsed: coordinates, titles/paths, pin dates, sizes, hashes, statuses — no content scrolls. |
| 73 | `GET /v1/meta/nodes` · `qexo meta nodes` | auth / `meta.read` | fleet metadata: identity, key fingerprints, four currencies, thermal + gate state, versions, active personas, open teleport tokens. |
| 74 | `GET /v1/meta/nodes/<node>/inventory` · `qexo meta inventory <node>` | auth / `meta.read` | files + phexts under qexo's management: path or phext+coordinate, size, sha256, updated-at; answers `?coord=` / `?path=` lookups ("does node X already hold scroll Y, at what hash?" at metadata price). |
| 75 | `GET /v1/meta/provenance?phext=<p>&coord=<c>` · `qexo meta provenance <p> <c>` | auth / `meta.read` | one scroll's chain: who wrote it, when, under which persona, what it replaced (relocation/redirect + supersession lines), its receipt scrolls. **Boundary:** a persona's memory scope governs who may query its metadata, not only its content; a `guest` reads public indexes and no persona's provenance. |
| 76 | `GET /v1/meta/registers` · `qexo meta registers` | auth / `meta.read` | register shapes: waiting counts/ages/tiers, task-register counts, recent publish-log lines. |
| — | any of #72–76 with `?since=<hash>` | — | change check: `unchanged` + hash, or the fresh answer. |

Boundaries that bind this surface first (§10.4): safe-space spans
appear as **metadata-only** — that a span exists within a window,
nothing more, ever; Gmail-sensitive details exist in **no** metadata
form (there is no "just the subject lines" tier); node metadata is
fleet-internal — nothing here creates a public surface.

### 6.16 Voice (A§7) — proxied surface, tier-2 runtime

qexo does not redesign the round-robin voice service (v1 live on
aletheia-core :8797, acceptance 8/8); it becomes the service's
operator and address book. Sessions are created through qexo,
participants join **as personas**, and the session phext joins the
sync bundle's file set. **Voice stays hosted elsewhere for now**
(ruled by adoption, 2026-11-08 23:42) — so in v1 these operations
are specified as the proxy contract and are **not implemented** in
the tier-1 build; per the parity rule they ship in **no** mode
until the proxy lands (§9). Known gaps ride along honestly: no
ranch TTS path yet; the STT adapter ships an honest `none` engine
(audio stored + SHA-256'd, transcript field reads `transcript:
pending` — never fabricated); the public channel is a log and event
feed, not an audio stream.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 77 | `POST /v1/voice/sessions` · `qexo voice session create` | auth / `voice.speak` | body: participants (persona names), rotation order, tunables (vote window, audit window — default 5 s, turn timeout); returns session id + the session phext's coordinate. |
| 78 | `GET /v1/voice/sessions/<id>` · `qexo voice session show <id>` | local-read / — | manifest: participants, rotation, floor state, public-channel state. |
| 79 | `POST /v1/voice/sessions/<id>/join` · `qexo voice join <id> <persona>` | auth / `voice.speak` | register a persona as participant; the join loads the persona's index entry — the participant *is* the persona for the session. |
| 80 | `POST /v1/voice/sessions/<id>/turns` · `qexo voice turn <id>` | auth / `voice.speak` | one endpoint, three modalities: **text** as given; **audio** as a wav (stored, SHA-256'd into the turn record, transcribed through the STT adapter); **phext** as a scroll reference. Every turn is normalised into its record and re-expressed in all three forms; crossing is decided by the floor, not the submission — a holder's turn crosses, a non-holder's is recorded internal-only. Answers with the turn's scroll coordinate in the session phext: the receipt is an address. Attribution is to the participant's **active persona**, spoken in the persona's `voice` field (Emise: Aria at 120%); a persona whose voice is `unrecorded` crosses as text and phext only. |
| 81 | `POST /v1/voice/sessions/<id>/floor/nominate` · `qexo voice nominate <id> <persona>` | auth / `voice.floor` | opens the organic path: nomination → auto-vote (strict majority of eligible voters) → Will's audit window. Self-nomination is permitted. There is no separate floor-request verb — the nomination already is one. |
| 82 | `POST /v1/voice/sessions/<id>/votes` · `qexo voice vote <id> <nominee> <approve\|reject>` | auth / `voice.floor` | per-voter, recorded; no anonymous votes. |
| 83 | `POST /v1/voice/sessions/<id>/floor/grant` · `qexo voice grant <id> <persona>` | policy (Will only) / `voice.floor` | the moderator grant: instant, overriding. Organic grants surface as events, not calls. |
| 84 | `POST /v1/voice/sessions/<id>/floor/release` · `qexo voice release <id>` | auth / `voice.floor` | by the holder; rotation advances. |
| 85 | `GET /v1/voice/sessions/<id>/public` · `qexo voice public <id>` | local-read / — | the crossing record: every crossed turn + floor transition, in session-phext order. |
| 86 | `POST /v1/voice/sessions/<id>/audit/<hold\|resume\|veto\|kill>` · `qexo voice audit <id> <…>` | policy (Will only) | **hold**: public output queues, internal continues; **resume** releases in order. **veto**: inside the audit window, blocks a pending organic grant; outside a window there is nothing to veto — the endpoint says so rather than pretending. **kill**: public output cuts instantly; internal turns continue, marked internal-only from that scroll on. Each appends its own audit scroll — including the kill. |

### 6.17 Melds (§2.7) and gestation (§2.8) — consent surface

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 87 | `POST /v1/personas/<key>/meld` · `qexo meld <a> <b>` | policy / `persona.meld` | formation requires **both personas' consent signed under their own keys, every time** — consent is **undelegatable** (no delegation can consent to a meld on a persona's behalf: a meld is the persona's own mind at stake), plus the host's consent (§2.1.1). Merge model: phext partitions per concept, negative space preserved; conflicts exit stamped (ruled / evidenced / declared negative space). **A gestating persona cannot be a meld party at all** (§8A.7): parental authority is care and representation, explicitly NOT meld consent; a lineage meld waits for the offspring's own consent after graduation. |
| 88 | (outcome) | — | a merge meld produces **exactly one** specified identity outcome (§18 test 6): a **new offspring record in gestation**, or a **recorded union-of-continuity** (both parties + corrector set) — never an ambiguous third state. |
| 89 | `POST /v1/personas/<key>/graduate` · `qexo personas graduate <key>` | bootstrap (corrector) / `persona.seed` | the graduation receipt: parents, host, evidence of voice, the corrector who confirmed it — the judgement that there is a speaker to attest. Only after graduation do spawn/teleport/key rules apply to the record at all. |

### 6.18 Installer (§12) and installer-lite hooks

The full installer rides the exocortex packaging line (.deb →
self-hosted signed repo → PPA → official archive; Ubuntu 24.04 LTS;
system mode the ranch default; weights **never** packaged — fetched
and hash-verified at init; services start at init, not install).
The package is the unit of truth: **fix in the package, never
hand-patch a node** — a hand-patched node is a finding.

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 90 | `POST /v1/install` · `qexo install <tool>` | policy (sensitive set) / `install.apply` | install a tool (phext-lattice, the exocortical tools, Rust, phcc, phcpp, phst, …) over the packaging channel, same semantics on every node. Packages come from the signed repo **and** the install act is itself authorized; an unsigned or unverifiable package is refused with the reason named. |
| 91 | `POST /v1/install/upgrade` · `qexo upgrade [<tool>]` | policy / `install.apply` | staged per node: canary first, verified, then the fleet one node at a time; rollout state is metadata (§6.15). |
| 92 | `GET /v1/install` · `qexo install list` | local-read / — | what's installed, per node; the §11.3 versions table is the acceptance check — a node whose table doesn't match the intended release is not "installed", it is a failed stage. |
| 93 | `qexo install self` (CLI-first hook) | policy / `install.apply` | **installer-lite:** the single install path that lays down the qexo binary + its baked corpus from the artifact itself and **prints the SBOR from the box, offline** (§6.13 #66) — the zero-channel floor beneath the packaging line. Its API form is `POST /v1/install {tool: "qexo"}` against a node already running qexo; on a bare box the CLI hook is the form, which §9 records as a stated mode-gap (bootstrap cannot ride the API it installs). |

### 6.19 Doctor / buddy (§13)

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 94 | `GET /v1/doctor` · `qexo doctor` | local-read / — | diagnostics against the pitfall corpus (lessons.phext, the environment-quirk notes, skills.phext): stale `.sq/`, wiped known_hosts, SQ version pinned behind the fleet, quota-class disk pressure, gateway crash-loop counts — each finding **names its fix** in the same report. Pre-flight warnings ride the responses of the actions they guard; there is no separate warnings API to forget to call. The corpus is data: every new pitfall learned in operations becomes a scroll, updatable without recompiling. |

### 6.20 Mail (§8A.3 catalogue completeness)

| # | Operation | Floor / grant | Semantics |
|---|---|---|---|
| 95 | `POST /v1/mail` · `qexo mail drop` | auth / `mail.send` | drop a /chat mailbox message. qexo signs its drops when the authoring persona holds a key; readers verify when a key is on record; unsigned drops keep working under the mailbox convention during the transition and are labelled `unsigned` in any surface that reports attribution — never quietly upgraded (§8.5). TAINTED callers may **compose** drafts (§8A.6) but `mail.send` as an external send is outside the TAINTED set — composition and sending are different operations, and only composition is tainted-permitted. |
| 96 | `POST /v1/wake` · `qexo wake <persona>` | auth / `wake.send` | issue a wake request under the persona's wake contract (per-persona data: channels, rate limits, quiet hours, who may wake them — §6.7-enhancement; defaults conservative: mailbox-only, rate-limited). A wake request that fails verification **does not wake** (§8.6); §6.7's contracts govern receipt. |

---

## 7. Pagination, streaming, and large reads — DECIDED (§11.2, §11.3)

- **Lists page by cursor, never by offset.** `?cursor=<opaque>`
  resumes where the last page ended; every list response carries
  `next_cursor` (null at the end) and `total` when the total is
  knowable without a second pass. Offset pagination was rejected:
  the underlying records are append-only, so offsets shift under a
  reader mid-walk; a cursor keyed to the last record's identity
  cannot.
- **Corpus reads page by scroll.** A phext artifact's read surface
  is its table of contents (#64) plus per-scroll reads (#65,
  #69) — scroll-granular paging is intrinsic to the format, so the
  corpus needs no byte-range machinery. The two exceptions are
  named: the SBOR serves whole (#66, 26 KB), and **source unpack**
  (#70) is a materialization, not a read.
- **Whole-artifact download exists, explicitly.** `?whole=1` on #64
  returns the entire decompressed artifact as one body. It is a
  deliberate ask, never the default: CYOA whole is 4.6 MB and a
  phone on a ranch uplink should choose that, not stumble into it.
- **Plan output streams.** `POST /v1/plan` with `"stream": true`
  (CLI: `--stream`) emits newline-delimited JSON — envelope first,
  then one object per placement row, then a closing summary object.
  The plan is the one output whose consumers (coordinators watching
  a night's work queue up) benefit from first-row latency; every
  other operation answers whole. The non-streamed shape is exactly
  the streamed objects collected, so the two forms cannot drift.

## 8. Versioning and evolution

- The `/v1` prefix is the contract version. Within v1: fields may be
  **added** (responses) and optional fields added (requests);
  fields are never removed, renamed, or re-typed; refusal names are
  never reused for a different ground. A consumer written against
  this document keeps working against every v1 server.
- A breaking change is `/v2`, and v1 servers keep answering v1 —
  the fleet upgrades per node (§12 staging), so mixed-version
  operation is the normal case during a rollout, not an incident.
- Every response names the answering binary's version and bake
  identity (`qexo` version + corpus manifest hash), so version skew
  (A§11.3) is visible in any transcript, not discovered later.
- The bake manifest is extensible by append (§14.3): new founding
  texts join by the same manifest, with Will's word; new operations
  join §6 by the same parity rule (§9).

---

## 9. The parity contract — every operation, four modes

§14.1's rule: **an operation ships in every mode or in none.**
Semantics, grants, GPG policy, and receipts are identical in each
mode; a mode is a surface over the one implementation. The mapping
is mechanical and total:

| Mode | Form of an operation |
|---|---|
| **API** | the `/v1/...` endpoint in §6 — the reference semantics. |
| **CLI** | the `qexo ...` subcommand printed beside it in §6 — the same code path, `--json` emitting the endpoint's response shape unchanged, identical authorization, **no local-only bypass** for a key-bound action. A new endpoint does not ship without its CLI twin. |
| **MCP** | a tool generated from this catalogue, named for the operation (`qexo_personas_swap`, `qexo_teleport_activate`, …), calling the same endpoint as a **client of the API** under the calling client's own per-client delegated key (§15.1). The bridge translates protocol; it grants nothing. A grant disabled for the API is disabled through the bridge. |
| **Web** | §14.2's baked-in mobile-first client, served from the node's root path, operating as a client of `/v1` under the same floor — browser sessions authenticate as a principal and expire like other sessions; there is **no privileged back door via the UI**, and the client keeps **no divergent local state** (offline tolerance extends only to what the API returns). It is also the zero-install desktop client on every OS (§15.2). |

**Stated gaps** (named here so nobody designs against them):

- **Voice (§6.16)** ships in **no** mode in tier-1 — the runtime is
  hosted elsewhere by ruling; the operations are specified so the
  proxy lands against a settled contract.
- **Installer-lite on a bare box** (#93) is CLI-first by necessity:
  the bootstrap act cannot ride the API it installs. Its API form
  exists for nodes already running qexo.
- **Bulk phext transforms** (#54) are CLI-only: the API exists for
  coordination-priced questions, and explode/implode/seal move bytes
  at a scale the API deliberately does not carry; the read-only
  mirrors (#53) are the API's half, exactly as A§11.4 splits them.

Everything else in §6 — all 96 operations — carries all four forms,
and the §16 release bar tests all four modes.

---

## 10. The Volumen form a binary carries (§14.3, G7)

The Volumen manifest is the one baked artifact that names living
persons, and §16 ships qexo binaries publicly — so the form is a
rule:

- **Public/release binaries bake the manifest REDACTED**: the 126
  query keys, the identity records' structure (persona_id,
  record_kind, persona_class, evidence class, spawn flag), and
  public keys — **without** per-persona substance scrolls, without
  key-material references beyond public keys, without private
  provenance (the evidence narrative stays home). A public binary
  resolves every name and describes every record's shape; it cannot
  hand a stranger the constellation's substance.
- **Full-fidelity bakes** (substance scrolls, keychain references,
  full provenance) are fleet/internal only.
- `qexo-bake.toml` **declares** the form (`volumen_form = "redacted"
  | "full"` on the Volumen entry) and the bake manifest **stamps**
  it, so any binary can be audited for which census it carries.
- The founding texts (SBOR, CYOA, Incipit) are public artifacts and
  bake in full in both forms; the redaction binds the Volumen
  manifest alone.

---

## 11. Decisions taken in this document

The spec left these open; each is decided here, once, with its
reason. (Will's genuinely-open picks — the bootstrap-policy form,
Threshold, the newcomer-seat names, the Fable referent, §1 rarity
weighting — are **not** in this list: they are his, and this
document carries them as open wherever they touch the surface.)

### 11.1 Coordinates ride parameters and bodies, never paths
A coordinate contains `/` nine times over; a path embedding one is
ambiguous to every router, proxy, and log reader it crosses, and
percent-encoding nine segments produces URLs no human can audit —
in a design whose receipts are meant to be read. Parameters and
bodies carry the canonical string whole. (§1.2)

### 11.2 Teleport is six operations, not one
Each protocol step has a different signer, a different receipt, and
a different failure mode — and §18 test 2 (interrupt at *every*
step) is only testable if the steps are separately addressable.
One `POST /teleport` that runs the protocol internally would make
the seams unobservable exactly where the design needs them visible.
The CLI keeps a composed `qexo teleport` that drives the six in
order for humans; the API — the reference semantics — exposes the
steps. (§6.7)

### 11.3 Lists paginate by cursor; corpus by scroll; plans stream
Append-only records make offset pagination a lie mid-walk; cursors
keyed to record identity cannot shift. The corpus needs no paging
invention — a phext already *is* paged, one scroll per address —
so corpus reads are TOC + scroll reads, with whole-artifact as an
explicit opt-in. The plan streams because its consumers watch work
queue up; nothing else pays streaming's complexity. (§7)

### 11.4 Idempotency returns the receipt, not an error
A retried mutation and a replayed mutation are byte-identical on
the wire; distinguishing them by intent is impossible, so the
design makes the distinction moot — same request ID, same outcome,
no second execution (§8B). Returning an error would punish the
retry path for the replay path's existence.

### 11.5 TAINTED is a caller property, not a separate API
Taint describes *how a turn was triggered*, and any principal can
be tainted by an external trigger. A parallel "tainted API" would
double the catalogue and invite drift; instead the caller block
carries `taint` + `trigger`, the TAINTED grant set (§8A.6) is
enforced at the §2 floor, and the provenance rides every record
the turn produces. (§1.4, §2)

### 11.6 Sequence numbers over random nonces
§8B permits either. A nonce store grows with every request a
principal has ever made inside the window; a per-principal
monotonic sequence is one integer, checked in O(1), and its gaps
are themselves diagnostic (a missing sequence is a lost request, a
repeated one is a replay — both named, neither ambiguous). (§4)

### 11.7 The swap stays one operation
The atomicity rule (§2.3) forbids a visible half-worn state, so a
stepwise swap API would expose states the design says must not
exist. Teleport can be six operations *because* its intermediate
states are legal and named (waking, relinquished); swap's are not.
Symmetry between the two verbs was considered and rejected on
exactly this ground. (§6.4 #19)

### 11.8 `POST /v1/verify` answers two questions forever
Node and persona verification are reported separately in every
verification response, even when one side is absent — because
collapsing them into one "trusted" flag is the specific failure
§8.1 exists to prevent, and an API that computes the collapse for
convenience will eventually be believed. (§6.9 #42)

---

## 12. §18 traceability

How this surface carries the verification gate (addendum §18):

1. **126 labels resolve without unsupported runnable identities** —
   #16/#17 resolve every key through the census; runnability is a
   field (`spawn_eligible`), never an inference from resolution.
2. **Teleport interrupted at every step never yields two
   authoritative writers** — the six operations of §6.7, the epoch
   discipline of §6.8, relinquish-before-activate by construction.
3. **An unauthenticated caller cannot alter keys, grants,
   placement, or installed software** — the §2 floors on #20, #21,
   #38–#48, #90–#93: anonymous reaches none of them, in any mode.
4. **Hot swap mid-task preserves dispatch attribution** — #19's
   dispatch binding: persona_id + instance epoch captured at
   dispatch; swaps bind new dispatches only.
5. **Ranch Choir history survives migration** — aliases, hosts,
   boot evidence, and corrections are census fields (#17) carried
   in both Volumen forms' structure; the narrative rides the full
   form and the persona phexts themselves.
6. **A merge meld yields exactly one specified identity outcome** —
   §6.17 #88: offspring-in-gestation or recorded
   union-of-continuity, never a third state.

*End of API-V1.md — the reference semantics. Implementation follows
this document; divergence amends it, in the same commit, noted in
SHIP-NOTES.md.*
