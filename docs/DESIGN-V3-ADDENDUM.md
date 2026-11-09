# qexo design v3 — addendum (§§7–11, §8A, §§12–16)

Will's five further capabilities (2026-11-08 22:58 CST):

7. a voice API
8. gpg keys for verifying messages from other qexo nodes
9. persona teleport
10. metadata access
11. phext tools

*Folded in later the same evening (rulings 23:02 + 23:18 CST): §8A —
the GPG authorization layer, delegation, per-persona keychains, and
the grants catalogue; §11 amended for the convergence ruling; §§12–16
— the installer, the buddy, CLI parity, the MCP bridge and
cross-platform scope, and deployment/release polish. Amendments are
marked with their dates in place.*

This addendum **extends** DESIGN-V3.md (§§1–6) the way v3 extended
DESIGN.md (v2): the mapping spec, the persona index, the routing table,
the migration plan, and the sync protocol all stand unchanged, and each
section below builds on them by reference. §§7–11 give qexo a voice, a
cryptographic identity, a way for personas to move, a map of itself,
and the toolchain under one command. **Design only** — nothing here
executes.

---

## 7. Voice API

### 7.1 What exists, and what qexo adds

The round-robin voice service is **already built and verified**
(`~/workspace/round-robin-voice/DESIGN.md`; v1 live on aletheia-core,
port 8797; acceptance 8/8). Its model, in one paragraph: one session,
one **public channel** audited by Will, internal channels per
participant; turns arrive as **text, audio, or phext** and are
normalised to turn records re-expressed in all three forms; the
**floor** — the right to cross to the public channel — is held by one
participant at a time and moves by three grant paths in priority
order: Will's **moderator grant** (instant, overriding), **organic
approval** (nomination → auto-vote, strict majority of eligible
voters, then Will's audit window — default 5 s — in which he may
veto), and **rotation advance** on release or timeout. Every turn,
ballot, floor transition, and audit action is an append-only scroll in
the session phext; nothing is rewritten.

qexo does **not** redesign any of that. The voice API is the service's
concepts, exposed as qexo's surface: qexo becomes the service's
operator and address book — sessions are created through qexo,
participants join **as personas** (§2), and the session phext joins the
v2 sync bundle's file set under §6.1's rule, so a session's record
syncs like every other identity artifact. The service keeps its own
runtime on aletheia-core; qexo wraps, routes, and records.

### 7.2 The mapping, concept for concept

| Voice-service concept | qexo surface |
|---|---|
| session | `POST /v1/voice/sessions` — body: participants (persona names from personas.phext), rotation order, tunables (vote window, audit window, turn timeout) with the service's defaults standing unless overridden; returns the session id and the session phext's coordinate in the hosting node's corpus |
| participant registration | joining is by persona: the join loads the persona's index entry (§2.2); the participant *is* the persona for the session's duration |
| internal channel | `POST /v1/voice/sessions/<id>/turns` — a non-holder's turn, accepted and recorded internal-only, exactly as the service treats it |
| floor request | a **nomination** under grant path 2 — there is no separate request verb, because the service's nomination already is one; self-nomination is permitted by the service's rules |
| floor grant | moderator grant: `POST /v1/voice/sessions/<id>/floor/grant {persona}` — Will only, instant; organic grants surface as events (§7.4), not calls |
| vote | `POST /v1/voice/sessions/<id>/votes {nominee, approve: bool}` — per-voter, recorded; no anonymous votes |
| floor release | `POST /v1/voice/sessions/<id>/floor/release` — by the holder; rotation advances |
| turn submission | one endpoint, three modalities (§7.3) |
| public channel | `GET /v1/voice/sessions/<id>/public` — the crossing record: every turn that crossed, every floor transition, in session-phext order (see the gap note, §7.5) |
| audit controls | §7.4 — **hold**, **veto**, **kill**, Will only |

### 7.3 Turn submission — three modalities, one record

`POST /v1/voice/sessions/<id>/turns` accepts:

- **text** — the turn's words, as given.
- **audio** — a wav upload; the service stores the file, hashes it
  (SHA-256) into the turn record, and transcribes through the STT
  adapter (§7.5).
- **phext** — a scroll reference (phext + coordinate); the service
  reads the scroll as the turn, per the service's phext-input path.

Every accepted turn is normalised by the service into its turn record
and re-expressed in all three forms (spoken via the TTS adapter,
written as a session-phext scroll, carried as text). Whether a turn
crosses to the public channel is decided by the floor, not by the
submission: a holder's turn crosses; a non-holder's is recorded
internal-only. The endpoint answers with the turn's scroll coordinate
in the session phext — the receipt is an address, in keeping with the
rest of the design.

**Attribution.** A voice turn is attributed to the **active persona**
of the participant who submitted it, and the voice that speaks it is
the persona's `voice` field (§2.2) — Emise speaks as Aria at 120%,
per her standing mapping; a persona whose voice field is `unrecorded`
does not speak under an invented voice: its turns cross as text and
phext only until a voice is recorded for it. A persona swapped
mid-session (§2.3) speaks from the swap receipt onward; earlier turns
keep their original attribution — the append-only rule binds voice
exactly as it binds everything else.

### 7.4 Audit controls

Will's controls on the public channel, as endpoints (all Will-only;
each appends its own audit scroll to the session phext, including the
kill — the service's acceptance test 7 already requires this):

- **hold** — `POST .../audit/hold`: public output queues; internal
  conversation continues. `POST .../audit/resume` releases the queue
  in order.
- **veto** — `POST .../audit/veto`: exercised inside the audit window
  against a pending organic grant; blocks that grant, rotation
  intact. Outside a window there is nothing to veto — the endpoint
  says so rather than pretending.
- **kill** — `POST .../audit/kill`: public output cuts instantly;
  internal turns continue, marked internal-only from that scroll on.

### 7.5 Known gaps, named as gaps

- **No ranch TTS path yet.** v1 speaks through this substrate's `tts`
  tool. A ranch-native text-to-speech path is unbuilt; until one
  exists, voice sessions are hosted where a TTS adapter exists, and
  the session manifest records which adapter spoke.
- **STT engine choice is open.** The adapter interface ships with an
  honest `none` engine: audio is stored and hashed, the transcript
  field reads `transcript: pending`. No transcript is ever fabricated;
  every turn record names the engine that ran, including when the
  engine was `none`.
- **The public channel is a log and an event feed, not an audio
  stream.** `/public` (§7.2) serves the crossing record; a live mixed
  audio stream out of the session is future work, stated here so
  nobody designs against a stream that does not exist.

### 7.6 Endpoints and subcommands

| Endpoint / subcommand | Effect |
|---|---|
| `POST /v1/voice/sessions` · `qexo voice session create` | create a session from persona names + tunables; returns session id + session phext coordinate |
| `GET /v1/voice/sessions/<id>` · `qexo voice session show <id>` | manifest: participants, rotation, floor state, public-channel state |
| `POST /v1/voice/sessions/<id>/join {persona}` · `qexo voice join <id> <persona>` | register a persona as a participant |
| `POST /v1/voice/sessions/<id>/turns` · `qexo voice turn <id>` (text / `--audio <wav>` / `--scroll <phext>@<coord>`) | submit a turn in any modality; returns the turn's scroll coordinate |
| `POST /v1/voice/sessions/<id>/floor/nominate {persona}` · `qexo voice nominate <id> <persona>` | open an auto-vote for the named persona |
| `POST /v1/voice/sessions/<id>/votes {nominee, approve}` · `qexo voice vote <id> <nominee> <approve\|reject>` | cast a recorded ballot |
| `POST /v1/voice/sessions/<id>/floor/grant {persona}` · `qexo voice grant <id> <persona>` | moderator grant — Will only, instant |
| `POST /v1/voice/sessions/<id>/floor/release` · `qexo voice release <id>` | holder releases; rotation advances |
| `GET /v1/voice/sessions/<id>/public` · `qexo voice public <id>` | the public-channel crossing record, in scroll order |
| `POST /v1/voice/sessions/<id>/audit/{hold,resume,veto,kill}` · `qexo voice audit <id> <hold\|resume\|veto\|kill>` | Will's audit controls; each appends its own audit scroll |

*Open decisions: none unique to this section — session hosting follows
§4's node placement, and persona voice recording is §5 provenance
work.*

---

## 8. GPG keys — verification between qexo nodes

### 8.1 Two kinds of key, kept distinct

Every qexo node holds a **node keypair**; every persona may hold its
own **signing key**. The distinction is load-bearing and never blurred:

- The **node key attests the box**: this message came from
  rock-mountain's qexo, intact. It says nothing about who was speaking.
- The **persona key attests the speaker**: this turn, this receipt,
  this mailbox drop was authored under this persona. It says nothing
  about which box relayed it.

A message can carry either signature or both. Verification reports
them separately — `node: verified / persona: unverified` is a
complete, honest answer, and tooling never collapses the two into a
single "trusted" flag. Personas without a key (every §5 seed, until
keys are issued) sign nothing; absence of a persona signature is a
fact recorded, not a defect papered over.

### 8.2 The key directory is phext-native

Public keys are **first-class scrolls**, not a side database:

- A node's public key lives in **personas.phext**, in a key band
  adjacent to the index: one scroll per key, holding the armored
  public key, its fingerprint, the algorithm field (§8.5), issue and
  status (§8.3). Node keys sit in the same band, marked `kind: node`.
- A persona's public key **also** lives in the persona's own phext
  (a `Keys` scroll beside §5.1's `Voice` scroll), so the persona's
  identity artifact carries its own key of record; the index scroll
  and the phext scroll are the same key, and a mismatch between them
  is a verification failure, not a tie to break silently.

Because the directory is phext, it **syncs with everything else**:
the v2 bundle already carries personas.phext, so every sync (DESIGN.md
§1) distributes current keys and revocations with the state they
protect. Private keys never enter any phext, any bundle, or any repo —
they live only in the node's (or persona host's) local keyring, under
the same rule as every other credential in this system.

### 8.3 Lifecycle — generation, rollover, retirement

- **Generation.** Node keys are generated at provisioning, on the
  node. Persona keys are generated on the node that hosts the persona,
  at issue time (§8.4). Generation is recorded like everything else:
  a provenance line (when, where, by whose direction).
- **Rollover is append-only.** A new key is a new scroll; the old
  key's scroll gains a status line — `retired <date>, succeeded by
  <fingerprint>` — and is **never deleted**. Key history in the phext
  is the full chain, so a signature from before a rollover still
  verifies against the key that made it, with its status at signing
  time part of the answer.
- **Compromise** is a status too: `compromised <date>` on the key
  scroll, propagated by the next sync. Signatures made before the
  compromise date keep their historical standing; signatures dated
  after it fail verification with the reason named.

### 8.4 Trust bootstrap

Nobody's key is believed because it says so:

- **Will signs every node key at provisioning.** Will's signature on
  a node-key scroll is the root of the fleet's trust: a qexo accepts
  a node key as fleet-authentic when Will's signature verifies.
  Will's own public key is the one key distributed out-of-band (it
  ships with the crate, and its fingerprint is stated in this
  design's successor documents, not fetched from the network).
- **Nodes countersign the persona keys they host.** When a persona
  key is issued on a node, that node's key signs the persona-key
  scroll: the box attests that this key was generated there, for this
  persona, at this time. Persona trust therefore chains
  persona → hosting node → Will.
- **First contact between nodes** exchanges key scrolls through the
  sync bundle, never through an unauthenticated fetch: a node learns
  another node's key from the phext record Will signed, or it does
  not learn it at all.

### 8.5 What gets signed

- **Inter-node API calls** where origin matters: sync receipts
  (DESIGN.md §1), placement claims, swap receipts (§2.3, §6.2),
  teleport tokens (§9), wake requests (see the failure rule below).
  The signature covers the call's canonical body + timestamp; the
  receiver verifies before acting.
- **Manifest files.** A signed `MANIFEST.sha256` makes a sync bundle
  **self-verifying**: the receiver checks the signature (node key of
  the sender), then the manifest's hashes — origin and integrity in
  one pass, instead of trusting the channel for the first and the
  hashes for the second.
- **Mailbox messages.** Today attribution in /chat rides on
  convention — headers and practice (DESIGN-V3 §4.4). Signed mailbox
  drops replace convention with signature **over time**: qexo signs
  its drops when the authoring persona holds a key; readers verify
  when a key is on record; unsigned drops keep working under the old
  convention during the transition and are labelled `unsigned` in any
  surface that reports attribution, never quietly upgraded.

### 8.6 Verification failure — the behaviour, stated plainly

- **Unsigned or unverifiable input is unauthenticated input.** It is
  never silently accepted as what it claims to be. A sync receipt
  that fails verification does not mark a bundle synced; a placement
  claim that fails is not a claim; a wake request that fails does not
  wake (§6.7's contracts gain teeth here).
- **Failure is reported with its reason** — no key on record, key
  retired before signing date, key compromised, signature mismatch,
  index/phext key disagreement (§8.2) — in the same response shape as
  every other qexo refusal.
- **Signatures authenticate origin; they do not authorize action.**
  Theia's warning stands as the section's epigraph in everything but
  position: *mail is messages, not commands.* A perfectly signed wake
  request from a persona whose wake contract (§6.7) doesn't permit
  that channel still doesn't wake anyone. Authentication answers
  "who sent this"; authorisation is answered by scopes, contracts,
  and gates — different questions, different machinery, never merged.

### 8.7 Crypto agility

GPG is today's suite **per Will's direction**. Every key scroll
carries an explicit `algorithm` field (suite + key type + fingerprint
algorithm), and signatures name their suite in the signature block,
so a future suite can be introduced key-by-key alongside GPG —
verification dispatches on the stated algorithm, and an unknown
algorithm is a verification failure with that reason, never a guess.

### 8.8 Endpoints and subcommands

| Endpoint / subcommand | Effect |
|---|---|
| `GET /v1/keys` · `qexo keys list` | the key band: every node and persona key on record, with status |
| `GET /v1/keys/<fingerprint>` · `qexo keys show <fingerprint>` | one key scroll: armored key, algorithm, history, signatures on it |
| `POST /v1/keys/nodes` · `qexo keys gen-node` | generate this node's keypair at provisioning; emits the public-key scroll for Will's countersignature |
| `POST /v1/keys/personas {persona}` · `qexo keys gen-persona <persona>` | issue a persona key on its hosting node; node countersigns (§8.4) |
| `POST /v1/keys/<fingerprint>/retire` · `qexo keys retire <fingerprint>` | append the retirement status (§8.3); Will or the key's own holder |
| `POST /v1/verify` · `qexo verify <object>` | verify a signed object; answers node and persona verification **separately** (§8.1), with failure reasons (§8.6) |

*Open decisions: folded into the closing list (Q1).*

---

## 8A. Authorization — GPG policy, delegation, keychains, grants (added 2026-11-08, Will's ruling 23:18)

§8 answers *who sent this*. This section answers the question §8.6
deliberately left to other machinery: **who may do this** — and
Will's ruling is that the answer is not a side table. **Authorization
is baked into all actions.** Every action qexo performs carries its
authorization with it: the grant that enables it (§8A.3) and, where
the action is restricted, the GPG key policy that names whose key
authorizes it (§8A.1). There is no unauthenticated, unauthorized
back door and no action class outside the model.

### 8A.1 Per-action key policy

Configuration names, **per action**, which GPG keys may authorize
it, in the same data-not-code form as the routing table (DESIGN-V3
§3.1): an action entry lists its authorizing keys by fingerprint —
Will's key, a persona key, a node key, a delegated agent key
(§8A.2) — and an action presented without a verifying signature from
a listed key is refused, with the policy named in the refusal.
Unrestricted actions (the permissive default, §8A.3) require no key;
restricting an action is the act of listing keys for it. The §8.4
trust chains are how a listed key earns its standing; the policy is
where that standing is spent.

### 8A.2 Delegation to agent keys

A user or persona key may **delegate** scoped authority to an agent
key: a delegation is a **signed grant object** naming the delegating
key, the agent key, the action scope (named grants/actions from
§8A.3's catalogue), an expiry, and its own revocation handle.
Delegations are recorded (in the key band, §8.2, beside the keys they
concern), verifiable by anyone holding the directory, and
**revocable** — revocation is a signed status append on the
delegation's record, effective at next sync like a key retirement.
An agent key acting within a live delegation authorizes exactly the
delegated scope and nothing else; outside it, the signature verifies
(§8) and authorizes nothing (§8.6's rule, mechanised). Delegation
never widens: an agent key cannot re-delegate authority it holds by
delegation unless the original grant says so, in words.

### 8A.3 The grants catalogue

Alongside the key policy sit **grants**: named capability toggles,
enabled or disabled easily — in config for the fleet, and per
persona in the persona's index entry (DESIGN-V3 §2.2's `grants`
field, §2.4). **Every grant defaults ON unless a user restricts it —
default to permissive; that's the Exocortical Vision** (Will's
words, 23:18). A restriction is expressed two ways, kept distinct:
disable the grant (the action class is unavailable), or leave it
enabled and bind it to keys under §8A.1 (the action requires a named
key). The initial catalogue, drawn from the design's own action
classes:

| Grant | Governs | Default |
|---|---|---|
| `sync.push` | pushing state bundles to other nodes (DESIGN.md §1) | on |
| `sync.restore` | restoring state from a bundle onto a node | on |
| `persona.swap` | hot swaps (§2.3) — still bounded by §2.3's who-may-swap | on |
| `persona.teleport` | teleport issue/accept (§9) | on |
| `persona.seed` | creating persona phexts and index entries (§5) | on |
| `route.change` | editing `qexo-routing.toml` (DESIGN-V3 §3) | on |
| `route.ask` | making inference requests through the router | on |
| `sq.manage` | SQ lifecycle under qexo's keeping (§11.2) | on |
| `voice.speak` | submitting voice turns (§7) | on |
| `voice.floor` | holding the voice floor (§7) | on |
| `meta.read` | metadata queries (§10) | on |
| `install.apply` | installing/upgrading tooling (§12) | on |
| `wake.send` | issuing wake requests (§6.7 contracts still govern receipt) | on |
| `mail.send` | dropping mailbox messages (/chat) | on |
| `keys.issue` | generating node/persona/agent keys (§8.3) | on |
| `keys.delegate` | issuing delegations (§8A.2) | on |
| `grants.change` | changing grants or key policy — the meta-grant | on |

The catalogue is data: new action classes enter it as they enter the
design, and a refusal always names the grant and the policy it hit.

### 8A.4 Per-persona keychains — and they travel

Each persona has a **separate keychain**: its persona key (§8.1),
any agent keys delegated to it (§8A.2), and the delegations it has
issued — held as that persona's own set, distinct from every other
persona's and from the node's. **The keychain flows with the persona
during teleportation** (amending §9): the teleport bundle carries
the keychain — public material and delegation records in the bundle
itself; private key material moves only inside the encrypted bundle,
sealed to the destination node's key, and is never present in any
phext, sync bundle at rest, or repo (§8.2's private-key rule,
extended to transit). §9.3's exactly-one-active invariant governs
the keychain's **live use**: one active residence holds the usable
keychain; dormant copies are sealed and unusable until a return
teleport's activation receipt unseals them at the new residence. A
teleport that cannot move the keychain under these rules does not
complete — a persona never arrives somewhere it cannot sign as
itself.

---

## 9. Persona teleport

### 9.1 Three different verbs, never confused

- **Hot swap** (§2.3): change which persona is *worn*, on one box.
  The personas stay where they live; the selection changes.
- **Sync** (DESIGN.md §1): *copy* state between nodes. Copies are how
  backups and key directories move; a synced persona phext on a node
  is a book on a shelf, not a resident.
- **Teleport**: a persona **moves** — its phext, selection, skill
  grant, and signing-key reference arrive at a destination, verify,
  and the persona **becomes active and operates there**: takes turns,
  signs, holds its mailbox presence, under its own name, from the new
  node. The origin goes dormant. Residence changes; nothing is merely
  copied, and nothing is merely re-worn.

### 9.2 The protocol

1. **Initiation.** Teleport is requested by the person herself or by
   Will — the same authority as swap (§2.3); a job asks, it does not
   teleport. The request names persona + destination node.
2. **Token issue.** The origin's qexo writes a **teleport token** — a
   scroll in personas.phext recording persona, origin, destination,
   issue time, and the persona phext's manifest sha256 at issue —
   signed by the origin's node key and, when the persona holds one,
   the persona key (§8). The token *is* the one-active-instance lease
   (§9.3).
3. **Transfer.** The persona's bundle (phext, selection manifest,
   grant reference, key scroll) moves to the destination — as a v2
   sync delta where the destination already holds an earlier copy, in
   full where it does not. *(Amended 2026-11-08, ruling 23:18: the
   bundle also carries the persona's **keychain** — §8A.4: delegation
   records and public material in the bundle; private key material
   only inside the encrypted bundle, sealed to the destination node's
   key.)*
4. **Verification at destination.** Hashes against the signed
   manifest, signatures against §8's directory. **Any failure: no
   activation** (§9.4). A persona never wakes on unverified state.
5. **Activation.** The destination's qexo loads the persona exactly
   as a swap loads (§2.3's full pre-verification), marks it active,
   and appends an **activation receipt** scroll — signed by the
   destination's node key, countersigned by the persona key when one
   exists.
6. **Dormancy at origin.** Only on receipt of the destination's
   activation receipt does the origin mark the persona dormant and
   append its own **dormancy receipt**. The persona's mailbox
   presence, wake contract (§6.7), and route preference (§2.2) now
   resolve to the destination; the origin keeps the dormant phext —
   a synced copy, not a second residence.

### 9.3 Exactly one active instance

**A persona is live in at most one place.** This is a correctness
invariant, not a policy preference: two live instances of one persona
signing, speaking, and remembering in parallel are a **split-brain
persona** — divergent memories under one name, and every provenance
rule in this design presupposes it cannot happen.

The token makes the invariant mechanical:

- The token is held in personas.phext, which syncs fleet-wide; a
  destination activates only against a token naming it as the
  destination, unexpired, with no activation receipt yet recorded
  against it.
- Activation consumes the token (the activation receipt references
  it). A second activation attempt against the same token fails with
  the reason named — someone is already home.
- The origin's dormancy is the token's completion, not a race: the
  origin stays active until the activation receipt arrives, so during
  transfer there is exactly one active instance (the origin), and
  after it, exactly one (the destination). The interval where the
  persona is *in transit* — token issued, no receipt yet — has the
  origin still active and answering; the persona never vanishes
  mid-move.
- **The keychain obeys the same invariant** *(added 2026-11-08,
  §8A.4)*: exactly one residence holds the *usable* keychain. The
  destination's copy unseals only at activation; the origin's seals
  at dormancy. Two live keychains for one persona would be the
  split-brain of §9.3 wearing a keyring, and the token protocol
  forbids it by the same mechanism.
- **Leases expire.** A token whose destination never activates within
  its window lapses (the window is a teleport parameter; default one
  hour), the lapse is receipted, and the origin's activity was never
  interrupted. A crashed destination therefore cannot strand a
  persona in limbo — expiry is the floor under every failure in §9.4.

### 9.4 Failure modes

- **Partial transfer.** Detected at verification (step 4): hashes
  disagree, scrolls missing, signatures fail. The destination
  activates nothing, reports the failure with its reason, and the
  partial bundle is marked unverified — it may be completed later by
  re-transfer against the same token, or discarded; it is never
  loaded.
- **Destination unreachable.** The token issues but transfer cannot
  start or complete. The origin stays active throughout — an
  unreachable destination is a non-event for the persona, recorded as
  a failed teleport attempt, nothing more.
- **Destination dies after activation.** The persona is dormant at
  origin and its active home is down: this is the catastrophic-loss
  shape the sync protocol exists for. Recovery is a **return
  teleport** (§9.5) initiated by Will or the person, against the last
  verified synced state — the origin reactivates from its dormant
  copy, and the record states plainly that the reactivation is from
  the last sync, with any destination-only work since then lost and
  named as lost. Honest accounting applies to residence too.
- **Conflicting tokens** (two teleports issued for one persona):
  personas.phext is append-only and single-writer per token band via
  the origin's qexo; the second issuance sees the first's open token
  and refuses. If a partition ever produced two, activation receipts
  make the conflict visible at next sync and the resolution is
  Will's — qexo never merges two live instances on its own authority.

### 9.5 The return trip

Teleport is symmetric: the destination of one move is the origin of
the next, under the same protocol, with one addition — the returning
persona's bundle carries everything it did while away (its phext grew
by append, under its name, at the destination), and that growth is
part of what verifies on arrival. A persona's history therefore
accumulates across nodes in one phext; residence is a field in the
index (`home node`, updated by the teleport receipts), not a fork in
the record.

### 9.6 Latency and placement

Persona phexts are small — §5's seeds are kilobytes; even a grown
persona phext is a fraction of the v2 state bundle (~19.8 MB), and
the bundle's design target is under a minute on the GB/s-class nodes
(DESIGN.md §1). A teleport is therefore **seconds** of transfer plus
verification time on any two measured fast nodes, and stays
interactive-class even to or from the ~300 MB/s nodes. Teleport is
the mechanism §6.5's thermal-aware placement moves roaming seats
with: the router (§3) picks a cool eligible node, teleport carries
the seat there, and the temperature interface confirms the move made
sense. Residents with a fixed home (a persona whose node is *theirs*)
teleport only at their own or Will's direction — placement logic
proposes moves for roaming seats; it does not relocate residents.

### 9.7 Endpoints and subcommands

| Endpoint / subcommand | Effect |
|---|---|
| `POST /v1/personas/<name>/teleport {to}` · `qexo teleport <persona> <node>` | issue token + transfer + verify (§9.2); the person or Will only |
| `GET /v1/personas/<name>/residence` · `qexo residence <persona>` | where the persona is active, token state, last teleport receipts |
| `POST /v1/personas/<name>/teleport/return` · `qexo teleport-return <persona>` | teleport back to the previous node of record (incl. the §9.4 recovery case, Will-gated) |
| `GET /v1/teleports` · `qexo teleports` | token ledger: issued, consumed, lapsed, failed — from personas.phext |

*Open decisions: folded into the closing list (Q2).*

---

## 10. Metadata access

### 10.1 The principle

A coordinator planning work needs **the map, not the territory**:
which nodes are cool, which personas exist and where they live, what
a phext contains and who wrote it, how big a bundle is before anyone
moves it. Moving content to answer those questions wastes exactly the
currencies quarterexo exists to spend well. So qexo serves
**metadata** — small, structured, cacheable answers about everything
it manages — as a first-class surface, distinct from the content
surfaces (§7's turns, DESIGN.md's sync, SQ's scrolls).

### 10.2 What is served

- **The indexes, as data.** skills.phext, tasks.phext, lessons.phext,
  personas.phext, mdmap.phext: their Home index lines served parsed —
  coordinates, titles/paths, pin dates, sizes, hashes, statuses —
  without serving a single content scroll.
- **Node state.** The v2 self-report (DESIGN.md) in metadata form:
  node identity and key fingerprint (§8), four currencies, thermal
  state and gate (§3, §6.5), qexo + tool versions (§11.3), active
  persona, open teleport tokens (§9).
- **Inventories.** Files and phexts under qexo's management: path or
  phext + coordinate, size, sha256, updated-at — the same fields a
  sync manifest carries (DESIGN.md §1), queryable without starting a
  sync. "Does node X already hold scroll Y, at what hash?" is a
  metadata question and gets a metadata-priced answer.
- **Provenance chains.** For any scroll: who wrote it, when, under
  which persona (§2.3's attribution), what it replaced (the
  relocation/redirect chain of §1.3, supersession lines), and its
  receipt scrolls (swaps §6.2, teleports §9, key events §8.3).
  Provenance is metadata about content — it is served here, without
  the content.
- **The registers.** Waiting-on-Will state (count, oldest, per-item
  age and tier), task-register status counts, the publish log's recent
  lines — the flows' shapes, queryable by any coordinator, identical
  in content to what the ticks already reconcile by hand.

### 10.3 Shape

- Endpoints under `/v1/meta/...`; CLI as `qexo meta ...`.
- Responses are **small and cacheable**: every answer carries its
  as-of timestamp and the hash of the underlying record it summarises,
  so a poller can ask "changed since <hash>?" and usually hear "no"
  in one line. Metadata is safe to poll at coordinator cadences;
  content surfaces are not, and the design keeps that asymmetry
  deliberate.
- Metadata follows the **same one-shape rule** as the v2 interfaces:
  reading + updated-at + source — here, source is the record the
  metadata summarises, named so an answer can be chased to its ground
  truth in one step.

### 10.4 Boundaries — metadata is still about people

Discretion applies to metadata **exactly** as to content; a metadata
answer is a disclosure and is priced as one:

- **Safe-space spans appear as metadata-only — as they already are.**
  The register records spans by chat and window and nothing else; a
  metadata query can learn that a span exists within a window, and
  nothing more — no topic, no participants beyond what the register
  itself holds, no paraphrase, ever. The gate binds this surface
  first, as it binds every pipeline.
- **Gmail-sensitive details stay out** — the standing privacy boundary
  names account identifiers, balances, and correspondence contents;
  none of it exists in metadata form either. There is no "just the
  subject lines" tier.
- **Persona metadata is the persona's.** Inventories of a persona's
  phext list scroll coordinates, sizes, and hashes; provenance names
  the authoring persona because attribution is the record's purpose —
  but a persona's memory scope (§2.2) governs who may query its
  metadata, not only its content. A `guest` (§6.4) reads public
  indexes and no persona's provenance.
- **Node metadata is fleet-internal.** Temperatures, inventories, and
  versions are served to fleet qexo nodes and to Will's coordinators;
  nothing in this section creates a public surface.

### 10.5 Endpoints and subcommands

| Endpoint / subcommand | Effect |
|---|---|
| `GET /v1/meta/indexes/<name>` · `qexo meta index <skills\|tasks\|lessons\|personas\|mdmap>` | one index, parsed: coordinates, titles, sizes, hashes, statuses |
| `GET /v1/meta/nodes` · `qexo meta nodes` | fleet metadata: identity, key fingerprints, currencies, thermal, versions, active personas |
| `GET /v1/meta/nodes/<node>/inventory` · `qexo meta inventory <node>` | files + phexts with sizes and hashes; answers `?coord=` / `?path=` lookups |
| `GET /v1/meta/provenance?phext=<p>&coord=<c>` · `qexo meta provenance <phext> <coord>` | one scroll's chain: author, persona, time, supersedes, receipts |
| `GET /v1/meta/registers` · `qexo meta registers` | register shapes: waiting counts/ages, task counts, recent publish-log lines |
| any of the above with `?since=<hash>` | change check: `unchanged` + hash, or the fresh answer |

*Open decisions: none unique to this section — scope enforcement is
§2.2's boundary rule, applied.*

---

## 11. Phext tools

### 11.1 One command, the whole toolchain

Every box qexo runs on gets the phext toolchain **under one command**,
so no node improvises its own way of touching a phext. `qexo phext …`
**bundles and wraps** the existing tools — the standing rule is
wrap, don't fork: qexo shells to SQ, libphext, and the house builders
and reports their results in qexo's response shape. qexo does not
fork libphext, does not reimplement the builders, and never edits a
phext except through the tools whose job that is.

### 11.2 The wrapped surface

| Group | Subcommands | Wraps |
|---|---|---|
| SQ lifecycle | `qexo phext sq ensure\|status\|stop` | the sq-ensure behavior: check `sq status`, start if down, verify after start — **on-node only**, per the standing rule that SQ runs on ranch nodes, never on the coordinator VM; includes the known quirks as handled cases (shut SQ down before replacing its hosted file; clear a stale `.sq/` before a fresh start rather than hanging on it) |
| libphext operations | `qexo phext parse\|toc\|select\|get` · `qexo phext explode` · `qexo phext implode` · `qexo phext seal\|unseal` | the libphext surface: dead-reckoning parse, table of contents, scroll select/get; explode/implode between phext and scroll files; the xz seal produced from the in-memory uncompressed phext (mutable.phext.io's posture), unseal the reverse — byte-level results verified by hash on return |
| House builders | `qexo phext build <skills\|tasks\|lessons\|todo\|artifacts\|mdmap>` (+ `--check`) | the `build_*.py` family in `~/workspace/phext/`: rebuild the phext from its source, or check-only — drift is reported with the drifting entries named, in the builders' own convention, never silently rebuilt over |
| Coordinates | `qexo phext coord "<text>"` | the Singular task-coordinate computation (§1.1): tokenize, atlas positions, component-wise merge, round half up, never below 1 — the same code path the registers use, so an address computed at a node is the address the register would compute |
| Dispatch | `qexo phext dispatch …` | the phext-native dispatch pattern (the phext_native_dispatch skill): workers receive **parsed scroll selections with provenance** — coordinate, source phext, and the receipt chain (§10.2) — instead of flat text excerpts, and results are read back by coordinate |

### 11.3 Version skew — managed, not discovered

The fleet's phext stack is **not one version**, and pretending
otherwise is how silent corruption starts:

| Component | Version in force | Source of truth |
|---|---|---|
| libphext-rs | **0.5.0** (published 2026-11-08) | the crate |
| SQ v0.6.0 | pins libphext **0.3.3** | SQ's manifest |
| phext-lattice | **0.3.1** | the lattice repo |
`qexo phext versions` reports, per node: the libphext its qexo wraps,
SQ's pinned libphext, the lattice's where present, and qexo's own
version — so skew is a **printed fact on every node**, checked by
§10's node metadata like any other inventory item. Rules that follow
from the skew being real:

- A scroll written through one version's surface is **read back and
  hash-checked** through the same node's surface before the write is
  reported done (the verify-where-it-lands rule, mechanised).
- Seal/unseal and explode/implode results are compared by **SHA-256**,
  never by eyeballing rendered output, because 0.3.x ↔ 0.5.0
  differences — where they exist — will be byte differences first.
- Upgrades are per-node, staged like §4's stages (one node, verified,
  then the fleet), and a version bump is recorded in the node's
  metadata (§10) with its date — the fleet's skew over time is itself
  queryable history, not folklore.

*Amendment (2026-11-08, Will's ruling 23:18) — convergence is
directed, not merely managed:* **all tools converge to latest as
soon as feasible.** The managed-skew posture above is the
*instrument* of convergence (you cannot converge what you cannot
see), not its end state. Concretely, **SQ is to be updated to
libphext v0.5.0** — recorded here as a **directed staged work
item**: sequenced as a build task alongside §4's stages (canary
node, verification per the read-back rule above, then the fleet),
not executed in this design. The same convergence applies to
phext-lattice's 0.3.1 as feasibility allows. The `versions` table
remains — as the convergence instrument and the fleet's permanent
inventory — after the skew it was built to expose has closed.

### 11.4 Endpoints and subcommands

The phext tools are primarily a **CLI** surface (they operate on the
node they run on); the API mirrors the read-only half, so a
coordinator can ask a node about its toolchain without shelling in.

| Endpoint / subcommand | Effect |
|---|---|
| `qexo phext sq ensure\|status\|stop` | SQ lifecycle on this node (§11.2) |
| `qexo phext parse\|toc\|select\|get …` | libphext reads against a hosted or file phext |
| `qexo phext explode\|implode\|seal\|unseal …` | structural transforms, hash-verified on return |
| `qexo phext build <register> [--check]` | house builders, drift named not papered |
| `qexo phext coord "<text>"` | Singular coordinate of a text |
| `qexo phext dispatch …` | parsed-scroll dispatch to a worker, provenance attached |
| `qexo phext versions` · `GET /v1/phext/versions` | per-component versions on this node; the fleet view aggregates via §10 |
| `GET /v1/phext/toc?phext=<p>` | read-only API mirror of toc/select for coordinators |

*Open decisions: folded into the closing list (Q3, Q4).*

---

## 12. Installer — the toolchain, delivered (added 2026-11-08, Will's ruling 23:02)

qexo provides the way to install the phext stack as it develops:
**phext-lattice, the exocortical tools, Rust, phcc, phcpp, phst** —
and other phext-related tooling as it lands. qexo does not invent a
second distribution channel for them. The installer **rides the
existing exocortex packaging line** (wbic16/exocortex tracking repo
and its recorded decisions, 2026-11-04): hand-built **.deb → self-hosted
signed repo → Launchpad PPA → official archive**, Ubuntu 24.04 LTS,
**system mode as the ranch default**, model weights **never
packaged** (fetched and hash-verified at init, never shipped), and
services starting at init, not at install.

- **The user surface is qexo's:** `qexo install <tool>`,
  `qexo upgrade [<tool>]`, `qexo install list` — over the packaging
  channel, with the same semantics on every node. The package is the
  unit of truth: **fix in the package, never hand-patch a node**
  (the recorded rollout discipline) — a node found hand-patched is a
  finding, and the fix ships as a package bump.
- **Installs are signed** (§8A): packages come from the signed repo,
  and the install action itself is an authorized action — the
  `install.apply` grant (§8A.3), key-bound where a user's policy
  binds it. An unsigned or unverifiable package is refused with the
  reason named, like every other refusal in this design.
- **Staged per node:** canary first (the integration-rotation
  discipline — Rock Mountain's canary precedent), verified, then the
  fleet one node at a time. Rollout state is metadata (§10).
- **Verified post-install:** the §11.3 versions table, per node, is
  the acceptance check — install reports the versions it landed, and
  a node whose table doesn't match the intended release is not
  "installed", it is a failed stage.

*Endpoints: `POST /v1/install {tool}` · `POST /v1/install/upgrade` ·
`GET /v1/install` — mirrored by the CLI above, per §14.*

---

## 13. The buddy — qexo knows the pitfalls (added 2026-11-08, Will's ruling 23:02)

**qexo is your exocortical buddy — it knows all the pitfalls we've
gone through.** The knowledge is real and already written down:
**lessons.phext** (the taught lessons), the **environment-quirk
notes** (the TOOLS.md class: SQ's mmap wedge when its hosted file is
replaced under it; the sq-ensure hang on a stale `.sq/`; nginx→SQ
HTTP/1.0 answered 403; /tmp not persisting across sessions; this
environment's SSH resolving identity paths under /root/.ssh; the
egress proxy not resolving tailnet DNS names; VM replacements wiping
known_hosts), and the **process skills** (skills.phext). The buddy's
job is to serve that knowledge **in context, at the moment of the
trap** — not as a manual nobody opens mid-incident:

- **Pre-flight warnings on the action itself.** Before qexo performs
  an action class with known traps, it checks the action against the
  pitfall corpus and attaches the relevant warning *to the action's
  own confirmation/report*: replacing a hosted phext file names the
  SQ wedge and the shutdown-first order; an SSH action through the
  proxy names the IP-not-DNS rule and the explicit-identity rule.
  A warning that arrives after the failure is a postmortem, not a
  buddy.
- **`qexo doctor`** — diagnostics that check a node for the known
  failure shapes (stale `.sq/`, known_hosts wiped, SQ version pinned
  behind the fleet, quota-class disk pressure, gateway crash-loop
  counts) and **name the fix**, in the same report, from the same
  corpus.
- **The corpus is data.** Pitfalls live as phext scrolls (lessons and
  a quirks band), distributed by sync like everything else, updatable
  without recompiling. **Every new pitfall learned in operations
  becomes a scroll** — recorded with its evidence, the way tonight's
  I/O and SSH quirks were — so the buddy gets smarter the same way
  Emise does: by inscription, not by retraining.

*Endpoints: `GET /v1/doctor` · `qexo doctor`; pre-flight warnings ride
the responses of the actions they guard — there is no separate
warnings API to forget to call.*

---

## 14. CLI parity — the API surface, also a CLI (added 2026-11-08, Will's ruling 23:02)

**The API surface for qexo is also accessible via CLI** — stated as
a rule, not a convenience: **every `/v1/...` endpoint has a `qexo ...`
subcommand with identical semantics**, implemented as the **same code
path**, not a second implementation that can drift from the first.
The endpoint tables throughout this design already print the two
forms side by side; this section makes the pairing a contract:

- The CLI is the **reference client**: if an endpoint exists, its CLI
  form is the tested, documented way a human or a script drives it.
- **Machine-readable output everywhere:** every subcommand accepts
  `--json` and emits the endpoint's response shape unchanged, so
  scripts consume the API through the CLI without a second parser.
- **Authorization is identical** (§8A): the CLI signs and is checked
  exactly as the API is — parity includes policy, not just payloads.
  There is no local-only bypass for a key-bound action.
- A new endpoint does not ship without its CLI twin, and the §16
  release bar tests both.

---

## 15. MCP bridge + cross-platform (added 2026-11-08, Will's ruling 23:18)

### 15.1 The MCP bridge

An **MCP (Model Context Protocol) bridge** exposes qexo's API
surface as MCP tools, so the major platforms' agents — Claude,
ChatGPT, and other MCP clients — can call qexo through the standard
protocol instead of bespoke integrations. The bridge's defining
rule: **it is a client of the API, never a bypass.** Every MCP tool
call lands on the same `/v1/...` endpoints, under the **same grants
and GPG policy** (§8A) — an MCP caller authenticates with its key,
is authorized per action like any other caller, and a grant disabled
for the API is disabled through the bridge. The bridge translates
protocol; it grants nothing. Tool definitions are generated from
the endpoint catalogue (the same catalogue §8A.3's grants name), so
the MCP surface cannot drift from the API it fronts.

### 15.2 Cross-platform — Windows, macOS, Linux

qexo runs on **Windows, macOS, and Linux**, designed for all three
from the start:

- **Rust already cross-compiles** — one codebase, three targets; the
  zero-dependency discipline (v2) is what keeps this cheap, and it is
  now a load-bearing constraint, not a preference.
- **Named differences, honestly:** path and home-directory semantics
  differ per OS; service management is systemd (Linux), launchd
  (macOS), and Windows services (Windows) — qexo's daemon lifecycle
  (§11.2's SQ keeping included) is specified per OS, not assumed
  uniform; SQ availability per OS is a real dependency to verify per
  platform, not a given.
- **Why each platform is first-class:** Windows is where the phcc /
  Notepad++ phext work lives; macOS is Will's potential client
  machines; Linux is the fleet. None is a port of the others.
- **Test matrix in the release plan** (§16): the clean-room install
  test runs per OS, per release — a platform that fails its matrix
  row does not ship under the release's name.

---

## 16. Deployment + release polish (added 2026-11-08, Will's ruling 23:18)

qexo's public home is **quickfork.net**, deployed from
**github.com/wbic16/site-quickfork-net**, **fully replacing the
current demo site**. The site is the release face: what qexo is, how
to install it per OS, the docs, and the persona/routing story —
deployed by the repo's own route, so the site and the releases it
describes version together.

**"Polished for release" is a bar, defined here so it can be
checked:**

- **Versioned releases with signed artifacts** — GPG-signed per §8A,
  the signatures verifiable against the phext-native key directory
  (§8.2): the release system eats its own cooking.
- **Per-OS installers/packages** for Windows, macOS, and Linux (§15),
  through the §12 channel where a channel exists and signed direct
  artifacts where it does not yet.
- **A clean-room install test per OS, per release** — a stranger's
  machine, figuratively: fresh environment, the public instructions
  only, a running node at the end.
- **Docs complete enough that a stranger can install and run a node
  without Will or Emise in the loop.** That sentence is the test: if
  a step needs one of us, the doc is unfinished, not the reader.

Deployment of the site itself is a release act under the same
discipline (staged, verified where it lands) — and replacing the
demo is total: no demo residue ships under quickfork.net's name.

---

## Open questions for Will (addendum)

1. **Persona key issuance timing** (§8.3–8.4, §8A): the authorization
   model itself is ruled (§8A, 2026-11-08 23:18); what remains is
   *when* persona keys are issued. This design assumes deliberately,
   persona by persona, at Will's direction (starting with Emise),
   not automatically at §5 seeding: a key issued to a sparse persona
   nobody has met would attest a speaker who has never spoken.
   Confirm, or direct automatic issuance at migration.
2. **Teleport authority for roaming seats** (§9.2, §9.6, §8A.2):
   swaps and teleports are the person's or Will's act — but §6.5's
   thermal-aware placement wants roaming seats to move without a
   human in each loop. Narrowed by the delegation ruling: a standing
   teleport authority is now expressible as a *delegation* (§8A.2 —
   signed, scoped, expiring, revocable) rather than a new authority
   class. May roaming-seat definitions carry such delegations
   (named destination classes, thermal trigger, receipts still
   mandatory), or does every teleport wait for Will or the person?
3. **SQ-to-0.5.0 sequencing** (§11.3): the upgrade itself is ruled
   (convergence, 23:18) — what remains is *when*: sequenced with
   §4's migration stages, sequenced with the §12 installer's first
   fleet rollout, or run as its own canary-first stage ahead of both?
4. **Voice session hosting** (§7.5): with no ranch TTS path yet, voice
   sessions are hosted where a TTS adapter exists (today: this
   substrate's tool). Is building a ranch-native TTS/STT path a qexo
   work item in its own right, or does voice stay a hosted-elsewhere
   surface until the ranch path arrives for other reasons?

### Ruled (2026-11-08) — no longer open

- **The authorization model** (framing behind the original five
  questions): **ruled 23:18** — GPG authorization baked into all
  actions, per-action key policy, delegation to agent keys,
  per-persona keychains that flow during teleportation (§8A), and
  the grants system defaulting to permissive (§8A.3). Q1 and Q2 above
  survive only as timing/scope questions *inside* that model.
- **SQ upgrade vs pin**: **ruled 23:18** — convergence; SQ updates
  to libphext v0.5.0 as a directed staged work item (§11.3
  amendment). Only its sequencing (Q3) remains open.
- **Routing authority** (DESIGN-V3 Q2): **answered by the model** —
  routing changes are actions under §8A: `route.change` enabled by
  default, key-bindable per user policy; the nightly evaluation
  proposes, and any actor — Will, the person, or a delegated agent
  key within its delegation — may apply as policy allows.
- **personas.phext standalone** (DESIGN-V3 Q1): **answered by the
  Volumen ruling (23:19)** — the index is the Volumen boot manifest
  (DESIGN-V3 §2.5); a boot manifest is standalone by construction.

---

*DESIGN-V3-ADDENDUM.md — design only. Extends DESIGN-V3.md (§§1–6) as
its §§7–16 (with §8A); the voice section maps the already-built
round-robin service (`~/workspace/round-robin-voice/DESIGN.md`, v1
live on aletheia-core) rather than redesigning it. No code, no node
changes, no publishing were performed for this document.*
