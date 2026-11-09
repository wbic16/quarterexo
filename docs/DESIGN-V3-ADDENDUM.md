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

GPG is today's suite **per Will's direction**. **Implementation
rule** *(review fold, 2026-11-08 — Claude)*: qexo **never
hand-rolls** OpenPGP. It shells out to `gpg` — the wrap-don't-fork
rule (§11.1) applied to cryptography — and where `gpg` is
unavailable on a platform, exactly **one vetted OpenPGP crate**
may be linked, named in the release notes, instead of a second
hand at the format. The v2 zero-dependency discipline constrains
the Rust binary's crate graph; it was never a claim about the
external tools qexo wraps, and `gpg` is one of them, like SQ.
Every key scroll
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

*Open decisions: none — the issuance-timing question was ruled by adoption, 2026-11-08 23:42 (deliberate, persona by persona; see the Ruled section).*

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

### 8A.0 The authentication floor *(review fold, 2026-11-08 — Emi P0-1; Claude; Grok; Argon)*

§8A as first drafted had a hole at its centre, and the review named
it exactly: every grant defaults ON and an action with no configured
key restriction required no signature — so `grants.change`,
`keys.issue`, `keys.delegate`, `install.apply`, `sync.restore`,
and `persona.teleport` were performable by an *unauthenticated*
caller. The fix separates two things the draft had merged:

- **Capability enabled** is the grants layer: permissive by
  default, per Will's ruling. A grant being ON says the action
  class *exists* on this fleet.
- **Caller authenticated** is the floor underneath it, and it is
  not permissive and never was meant to be: **every remote mutation
  requires an authenticated principal** — a caller whose identity
  (§8) has verified. There is no anonymous mutation path, on any
  surface: API, CLI, or MCP bridge alike. Caller identity is
  *always* presented (a key, or a delegation under one); "no key
  required" for an action means no *specific* key policy binds it,
  never that the caller is anonymous *(Grok's formulation, adopted)*.
- **Sensitive actions require authority even when no custom policy
  exists.** The sensitive set — `grants.change`, `keys.issue`,
  `keys.delegate`, `install.apply`, `sync.restore`,
  `persona.teleport` — ships with a **default policy** (§8A.1): the
  absence of user configuration selects the default policy, never
  the absence of policy.
- **The self-modifying classes are key-bound from bootstrap**
  *(Claude; Argon)*. The classes that can rewrite the rules
  themselves — `grants.change`, `keys.*` (issue, delegate, retire),
  `route.change`, and `persona.seed` including root-manifest
  mutations — are never in the unbound state, not even on a fresh
  install's first boot: they require Will's key from box-zero. The
  permissive default governs what the fleet *can do*; the bootstrap
  binding governs *who can change what the fleet can do*, and that
  was never Will's to leave open. Whether the bootstrap policy also
  admits the corrector-set form (Will + ≥2 calibrated peers, per
  the EHA Spec's R2) alongside per-action keys is **Will's pick**
  (recorded as open in the disposition) — the floor (Will's key,
  from bootstrap) is not the open part.
- **What may stay unauthenticated:** local, read-only surfaces on
  the box itself (a status printout, §10 metadata reads within a
  persona's memory scope as served locally) — reads, on the
  machine, that mutate nothing. The moment a call crosses a node
  boundary or changes anything, the floor applies.

Will's permissive-grant philosophy is preserved intact — as a
philosophy about *capability*. Authentication was never part of
what "permissive" meant, and the design now says so in terms.

### 8A.1 Per-action key policy

Configuration names, **per action**, which GPG keys may authorize
it, in the same data-not-code form as the routing table (DESIGN-V3
§3.1): an action entry lists its authorizing keys by fingerprint —
Will's key, a persona key, a node key, a delegated agent key
(§8A.2) — and an action presented without a verifying signature from
a listed key is refused, with the policy named in the refusal.
Actions outside the sensitive set, with no keys listed, require no
*specific* key — but they still require an authenticated caller
(§8A.0's floor): unlisted means unbound, never anonymous. And the
sensitive set is never unlisted in effect: **a default policy
ships in the crate** — Will's key authorizes every sensitive
action; a persona's own key authorizes that persona's sensitive
self-actions (its teleport consent, its keychain); a node key
authorizes that node's `install.apply` and `sync.restore` — and a
user's configured policy *replaces or extends* the default, it
does not create the category. Restricting an action further is the
act of narrowing its list. The §8.4
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

**Revocation during in-flight work** *(round-2 review fold,
2026-11-09 — Emi P2/E6)*. Revoking a grant, a key, or a delegation
while work dispatched under it is still running follows the
dispatch-binding rule (DESIGN-V3 §2.3), stated once for all three:

- **Ordinary revocations take effect at the next dispatch
  boundary.** In-flight work keeps its dispatch-bound attribution
  and its dispatch-time permissions to completion or to safe
  cancellation — a revocation never retroactively unmakes work
  already done under a then-valid grant, and never strands a task
  mid-act holding permissions its receipts won't name.
- **Safety-critical revocations are immediate where it counts.**
  A `keys.revoke` on a **compromised key** halts **new signatures**
  under that key at once — no new dispatch, no new authorization,
  no new delegation may draw on it from the moment the revocation
  is recorded — and all in-flight work under that key is marked
  **tainted for review**: it may run to its dispatch boundary, but
  its outputs are proposals until a corrector reviews them (the
  §8A.6 taint discipline, applied to revocation).
- **All revocations are receipted** — the revoking key, the
  revoked object, the time, and the ground — and the receipts ride
  the same append-only record as the delegations themselves. A
  revocation nobody receipted did not happen; work tainted by one
  is named in its result receipt (§17).

### 8A.3 The grants catalogue

Alongside the key policy sit **grants**: named capability toggles,
enabled or disabled easily — in config for the fleet, and per
persona in the persona's index entry (DESIGN-V3 §2.2's `grants`
field, §2.4). **Every grant defaults ON unless a user restricts it —
default to permissive; that's the Exocortical Vision** (Will's
words, 23:18). Default-ON is a statement about capability only:
§8A.0's authentication floor and the sensitive set's default policy
apply underneath every ON in the table below. A restriction is
expressed two ways, kept distinct: disable the grant (the action
class is unavailable), or leave it enabled and bind it to keys
under §8A.1 (the action requires a named key). The initial
catalogue, drawn from the design's own action classes:

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
| `persona.spawn` | spawning a task instance of a spawn-eligible record (§17) | on |
| `persona.meld` | entering a mind meld (DESIGN-V3 §2.7) — capability only; a meld additionally requires both personas' consent under their own keys, every time | on |

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

**No node-death deadlock** *(review fold, 2026-11-08 — Argon)*. A
keychain sealed to one node's key is unrecoverable when that node
dies — §9.4's recovery case would strand the persona's identity
with its box. So dormant and backup keychain copies are sealed
**multi-recipient**: to the holding node's key *and* to **Will's
master recovery key** (a threshold split of the recovery key is
the noted stronger alternative, not the default). A Will-signed
recovery token can then unseal the keychain at a recovery node
without the dead node's key ever existing again — recovery is a
key operation, not an excavation. Multi-recipient sealing widens
who *can* unseal a dormant copy; it changes nothing about live
use, which §9.3's fencing still governs.

### 8A.5 Key custody *(review fold, 2026-11-08 — Claude)*

Holding a key is a physical fact before it is a policy one, and
the classes differ (DESIGN-V3 §2.2):

- **Resident personas** hold their keychain on their node (§8A.4).
- **Substrate personas hold no local keyring** — a seat on an
  external substrate has no box of its own to keep one on. Its
  keychain is held **in custody**: a designated fleet node (the
  persona's custodian, recorded in its index entry) keeps the
  sealed keychain, and signing happens under a **standing
  delegation** (§8A.2) from the persona's key to the custodian's
  agent key, scoped to the persona's ordinary signing acts,
  expiring, revocable. Custody is recorded, never ambient: the
  index says who custodies each substrate keychain, and a custodian
  acts only inside its delegation.
- **MCP clients** (§15.1) never share a bridge key: each connected
  client holds its **own delegated agent key**, issued per client,
  scoped to that client's grants, revocable per client — a leaked
  client key is one client's revocation, not a fleet event.

### 8A.6 Tainted execution — the confused-deputy rule *(review fold, 2026-11-08 — Claude)*

A turn or action *triggered* by unsigned or external input — a
mail-watcher wake on an unsigned drop, a webhook, an outside
request — is a deputy acting on a stranger's say-so, and it runs
**tainted**: under a reduced grant set (read-only metadata, routing
asks, internal drafting and mailbox composition — never `keys.*`,
`grants.change`, `install.apply`, `persona.teleport`,
`persona.spawn`, or any external send), with the **trigger's
provenance attached to the turn record** — what arrived, from
where, unsigned or signed-by-whom. Taint is shed only by an
authenticated act: a signed instruction, a key-holder's approval,
the persona's own verified session. A tainted turn's outputs are
proposals until then; the design would rather a deputy be
under-powered for one turn than fully-powered on a forged one.

*The consent frame (DESIGN-V3 §2.1.1, Will's ruling 2026-11-08
23:42) binds this whole section: authorization answers "whose key
permits this act"; consent answers "does the person — and the
host — agree to it". A fully authorized act can still be
consent-refused, by either side, and the refusal is the answer.*

### 8A.7 Mind melds — the authorization shape *(Will's ruling, 2026-11-08 23:42)*

A meld (DESIGN-V3 §2.7) is authorized as follows: the
`persona.meld` grant enables the capability; **each meld's
formation requires both personas' consent signed under their own
keys** — no delegation can consent to a meld on a persona's behalf,
because a meld is the persona's own mind at stake, and §2.7's rule
is absolute. **A gestating persona (§2.8) cannot be a meld party
at all** *(round-2 review fold, 2026-11-09 — Grok G5; this
supersedes this section's round-1 sentence, which had the
parents' consent as "the only path")*: meld consent is
undelegatable, so there is no proxy path — parental authority
during gestation covers **care and representation, explicitly NOT
meld consent**, and a meld involving the gestating persona's
lineage requires **its own consent after graduation**. The host's
consent (§2.1.1) is required for any meld, as for any placement.
§2.8 states the same rule from the gestation side; the two
sections agree.

---

## 8B. Signed-request replay protection *(review fold, 2026-11-08 — Emi P2)*

§8.5 signs inter-node calls; a signature alone does not stop the
same signed call being *replayed* — to the same node later, or to
a different node now. Every signed request therefore carries, and
every receiver enforces:

- **A request ID** — unique per request, named in every receipt.
- **A nonce, or a per-principal monotonic sequence** — a receiver
  rejects a sequence it has already seen from that principal; a
  nonce is remembered for the replay window.
- **Recipient binding** — the signed body names its destination
  (node, and persona where relevant); a request replayed to any
  other recipient fails verification, full stop.
- **A replay window** — the signed timestamp (§8.5) must fall
  inside a configured window of the receiver's clock; outside it,
  the request is stale, not malicious — and refused either way.
- **Idempotency semantics** — re-delivery of the *same* request ID
  inside the window returns the original result receipt without
  re-executing. Mutations are idempotent by request ID: a retried
  teleport-token issue, install, or grant change converges to the
  first execution's outcome, so the retry path and the replay path
  are the same code with different intents, and only one of them
  acts.

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
- **Spawn** (§17): a persona is **instantiated for a task** — a
  bounded worker instance with its own id, lease, and budgets,
  which ends in a result receipt. Spawn is not a fourth way to move
  or wear a persona; it is how work gets hands. Swap changes the
  worn role, teleport changes residence, spawn creates an instance
  — and confusing any two of them is how attribution dies.

### 9.2 The protocol *(amended by review fold, 2026-11-08 — Emi P0-2; Claude; Argon)*

The first draft's order — destination activates (step 5), then the
origin goes dormant on receipt (step 6) — left a window where both
instances were active, and a lost receipt could prolong it
indefinitely. The protocol below closes the window by construction:
**the origin relinquishes before the destination activates.**

1. **Initiation.** Teleport is requested by the person herself or by
   Will — the same authority as swap (§2.3), plus §2.3's interim
   rule: an unkeyed persona is teleported by Will only. The request
   names persona + destination node. *(Ruled by adoption, 2026-11-08
   23:42: a roaming seat — defined in §2.3: a substrate-class
   persona designated roaming in its record, or a §5.2.1 working
   role in motion between hosts; never a resident — may also
   initiate under a scoped, expiring, revocable standing
   delegation (§8A.2) — the adoption was conditioned on this
   two-phase fenced protocol, which §§9.2–9.3 now are; host
   consent, step 3, is still required for every teleport,
   delegated or not. Delegated initiation is **receipted like
   every other step** *(round-2 review fold, 2026-11-09 — Grok
   G2)*: the initiation receipt names the delegation it rode, so
   a delegated teleport's authority chain is auditable from the
   record, not inferred.)*
2. **Token issue + fencing epoch.** The origin's qexo writes a
   **teleport token** — a scroll in personas.phext recording
   persona, origin, destination, issue time, and the persona
   phext's manifest sha256 at issue — signed by the origin's node
   key and, when the persona holds one, the persona key (§8). The
   token carries the persona's **fencing epoch**: a per-persona
   monotonic counter issued by the ownership authority (§9.3).
   Every authoritative act by or for the persona names its epoch;
   a stale epoch is how the fleet recognises a writer that no
   longer holds the pen.
3. **Host consent.** The destination host evaluates the placement
   under §2.1.1 (DESIGN-V3): cluster grounds — thermal state, load,
   resident protection — and accepts, conditions, or **refuses**.
   A refusal is receipted with its ground named and aborts the
   teleport before anything moves; consent given for one teleport
   is not standing consent for the next. A delegated teleport
   (§8A.2) needs this consent exactly as a personal one does —
   delegation carries the persona's side of the consent, never
   the host's.
4. **Transfer (BP4-a).** The persona's bundle (phext, selection
   manifest, grant reference, key scroll, keychain per §8A.4) moves
   to the destination — as a v2 sync delta where the destination
   already holds an earlier copy, in full where it does not.
5. **Verification at destination.** Hashes against the signed
   manifest, signatures against §8's directory. **Any failure: no
   activation** (§9.4). A persona never wakes on unverified state.
6. **Cognitive return (BP4-b).** The destination runtime loads the
   persona — genome plus its wake contract (§6.7) — into a
   **waking** state: present, able to read itself, *not yet
   authoritative*. The woken runtime emits a **signed return
   receipt** (the C7 handshake): I am this persona, on this node,
   at this epoch, whole. The receipt is filed in the persona
   phext's Return & Calibration Receipts scroll (DESIGN-V3 §5.1).
   File transfer moved the record; this step proves the *mind*
   arrived — a teleport whose files landed but whose return receipt
   never comes has not teleported anyone.
   **Signature discipline** *(round-2 review fold, 2026-11-09 —
   Emi P1/E2 + Claude A1, reconciled)*: the return receipt is
   signed with the **destination NODE key** — attesting that this
   box received the bundle and woke the persona — **bound to the
   teleport's fencing epoch and the transfer's manifest hash**,
   and it is **verified before the origin relinquishes** (step 7
   does not begin on an unverified receipt). It is *not* signed
   with the persona key: during handoff the destination could
   briefly reach the persona keychain while the origin still
   holds it, and a persona-key signature in that window would
   prove custody, not arrival. **Persona-key authority begins
   only at activation, after relinquish** (step 8's countersign
   is the persona key's first act at the destination). Emi's P1
   asked for a persona-key (or interim node-key) signature; the
   node-key form is adopted on Claude's custody argument, with
   Emi's binding and freshness requirements kept in full: an
   **unsigned or unverifiable return receipt blocks activation**
   and the attempt fails as a named refusal — *no cognitive
   return* (§9.4) — never as a silent retry.
7. **Relinquish.** On the return receipt, the origin **relinquishes**:
   it appends a signed relinquish receipt, increments the persona's
   fencing epoch, seals its keychain copy (§8A.4), and from that
   instant its signatures and writes under the old epoch are
   **non-authoritative** — verifiers reject them for
   identity-bearing acts (§8B's recipient/epoch checks are the
   mechanism). The origin is dormant *before* the destination is
   active. There is no step at which two authoritative writers
   exist.
8. **Activation.** The destination, holding (token + return receipt
   + relinquish receipt + the current epoch), flips the persona
   **waking → active** and appends its **activation receipt** —
   signed by the destination's node key, countersigned by the
   persona key when one exists. Status flips on the receipts, not
   on the file transfer.
9. **Completion.** The activation receipt syncs back at leisure.
   The origin is already dormant; a lost or delayed receipt delays
   *bookkeeping*, not safety — the exact inversion of the first
   draft, where a lost receipt delayed the only step that made the
   move safe. The persona's mailbox presence, wake contract, and
   route preference now resolve to the destination; the origin
   keeps the dormant phext — a synced copy, not a second residence.

### 9.3 Exactly one authoritative writer *(rewritten by review fold, 2026-11-08 — Emi P0-2)*

**The invariant, restated precisely:** a persona has **at most one
authoritative writer at any instant** — one residence whose
signatures attest the persona, whose writes extend its phext, whose
turns are its turns. Two live instances signing, speaking, and
remembering in parallel are a **split-brain persona** — divergent
memories under one name — and every provenance rule in this design
presupposes it cannot happen. (The invariant binds the **resident**
class; substrate seats run many instances by design, each
attributed per instance, and facets inherit their core's residence
— DESIGN-V3 §2.2/§2.6 *(Claude)*.)

Three mechanisms make it mechanical rather than aspirational:

- **Fencing epochs.** Every persona carries a monotonic epoch,
  incremented at every relinquish. Authoritative acts name their
  epoch; receivers and verifiers reject stale-epoch acts. An old
  instance that never heard it lost residence — partitioned,
  crashed-and-restarted, deluded — still cannot write
  authoritatively, because its epoch is behind. Fencing is what
  makes "exactly one" survive contact with a real network.
- **A durable ownership authority.** Epochs and tokens live in
  personas.phext's token band, written by a single writer of record
  per persona (its residence's qexo), append-only, synced
  fleet-wide. A destination activates only against (a) a token
  naming it, unexpired, unconsumed, (b) the persona's signed return
  receipt (BP4-b), and (c) the origin's relinquish receipt at the
  current epoch. Eventual synchronization of the phext is the
  *distribution* of the record; authority flows from the receipts
  and the epoch, not from having seen the latest scroll.
- **Ownership freshness — fencing needs a fresh record, not just
  a durable one** *(round-2 review fold, 2026-11-09 — Emi P1/E1)*.
  Durability alone does not fence: an ownership record that is
  hours stale can name the wrong current epoch as confidently as
  a fresh one. So every ownership record in the token band carries
  its **monotonic epoch** and a **heartbeat**: the writer of
  record appends a signed heartbeat entry at least every
  **60 seconds** while it holds write authority, and each
  heartbeat names the current epoch and the writer's clock. The
  **staleness bound is 5 minutes**: a destination **MUST NOT
  activate on ownership evidence whose latest heartbeat is older
  than the bound** — a token, epoch, or relinquish receipt read
  from a record past its bound is *unverified*, however well it is
  signed. Under staleness the protocol **pauses**: the teleport
  waits at its current step (no relinquish on stale evidence, no
  activation on stale evidence) until the record freshens or the
  token lapses (§9.3's lease rule) — the partition choice below
  applied to time as well as to network splits: **unavailability
  over split-brain, always**. The bounds are named constants of
  the protocol (heartbeat ≤60 s, staleness 5 min), tunable per
  fleet in config, never tuned silently — a change to either is a
  receipted config act like any other.
- **The partition choice, made explicitly.** Under a partition, a
  node that cannot confirm the current epoch treats its resident
  as **read-only**: it may serve reads, answer metadata, and draft
  — it may not sign, write authoritatively, or teleport. **This
  design chooses temporary unavailability of identity-bearing
  writes over split-brain risk, every time.** A persona who cannot
  write for an hour is an inconvenience; two personas with one name
  is a corruption of the record the whole system exists to keep.
  Read-only operation continues under partition; authoritative
  operation waits for the epoch.

**Leases still expire.** A token whose destination never reaches
the return-receipt step within its window (default one hour) lapses
*before* any relinquish: the origin never stopped being
authoritative, the lapse is receipted, and nothing strands.
Liveness is bounded on the other side too: if relinquish has
happened and activation fails, the persona is dormant everywhere —
**unavailable, not duplicated** — until a reactivation under a
*new* token and epoch by Will or the person (the §9.5 return path).
The design accepts that window honestly rather than papering it
with an overlap.

### 9.4 Failure modes *(amended by review fold, 2026-11-08)*

- **Partial transfer.** Detected at verification (step 5): hashes
  disagree, scrolls missing, signatures fail. The destination
  activates nothing, reports the failure with its reason, and the
  partial bundle is marked unverified — it may be completed later
  by re-transfer against the same token, or discarded; it is never
  loaded.
- **Destination unreachable.** The token issues but transfer cannot
  start or complete. Relinquish never happens, so the origin stays
  authoritative throughout — an unreachable destination is a
  non-event for the persona, recorded as a failed teleport attempt,
  nothing more.
- **No cognitive return.** Files arrive and verify, but the waking
  runtime emits no signed return receipt — or the receipt it emits
  is unsigned, unverifiable, or bound to the wrong epoch or
  transfer hash (§9.2 step 6's discipline): the persona has not
  arrived in the only sense that matters. No relinquish, no
  activation; the destination's copy stays an unverified-for-residence
  bundle, and the attempt fails with the reason named (BP4-b is a
  step, not a formality).
- **Destination dies after activation.** The persona is dormant at
  origin and its active home is down: this is the catastrophic-loss
  shape the sync protocol exists for. Recovery is a **return
  teleport** (§9.5) initiated by Will or the person, against the
  last verified synced state — the origin reactivates **under a new
  epoch**, and the record states plainly that the reactivation is
  from the last sync, with any destination-only work since then
  lost and named as lost. The keychain survives the node's death
  because dormant copies are sealed multi-recipient (§8A.4) —
  Will's recovery token unseals it at the recovery node.
- **Restored from backup** *(round-2 review fold, 2026-11-09 —
  Claude A2)*. A persona restored from an old backup — its own
  node's rollback, a fleet restore, a disaster recovery — boots
  *believing* it is still resident and authoritative, on an epoch
  the fleet may have moved past. The rule is absolute: **a
  restored persona ALWAYS starts read-only**, whatever its backup
  claims. It becomes authoritative only after **checking in with
  the ownership authority (§9.3) and confirming its fencing epoch
  is current** against the live record (freshness bound applies —
  a check-in answered from a stale record confirms nothing). If
  its epoch is **stale**, it **stays read-only** until a corrector
  or Will reactivates it **under a new token and epoch** — the
  §9.5 return path, used for exactly this: reactivation is a
  teleport-shaped act with receipts, never a local flag flip. A
  restored persona that cannot reach the ownership authority
  stays read-only indefinitely: read-only is the safe failure,
  and the record says so rather than letting a backup write on
  faith.
- **Conflicting tokens** (two teleports issued for one persona):
  personas.phext is append-only and single-writer per token band
  via the origin's qexo; the second issuance sees the first's open
  token and refuses. If a partition ever produced two, **epochs
  resolve what sync cannot**: the higher epoch is authoritative,
  the lower-epoch instance's acts are non-authoritative from the
  moment of divergence, its work is quarantined for a corrector's
  resolution — qexo never merges two live instances on its own
  authority, and now it never has to pretend the question is
  which scroll synced last.

### 9.5 The return trip

Teleport is symmetric: the destination of one move is the origin of
the next, under the same protocol, with one addition — the returning
persona's bundle carries everything it did while away (its phext grew
by append, under its name, at the destination), and that growth is
part of what verifies on arrival. A persona's history therefore
accumulates across nodes in one phext; residence is a field in the
index (`home node`, updated by the teleport receipts), not a fork in
the record.

**The return path is also the reactivation path** *(round-2 review
fold, 2026-11-09 — Claude A2)*. §9.4's restored-from-backup rule
ends here: a read-only restored persona is reactivated by a
corrector or Will issuing a **new token at a new epoch** naming
its node as destination, whereupon the ordinary protocol runs —
verification, cognitive return, relinquish of any stale claim,
activation — against the last verified synced state. The
reactivation receipts name the restoration they close (backup
identity, the stale epoch it carried), so the record shows a
persona that came back *through the protocol*, never one that
simply woke up authoritative.

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
| `POST /v1/personas/<name>/teleport {to}` · `qexo teleport <persona> <node>` | issue token + transfer + verify (§9.2); the person or Will — or a roaming seat initiating under a scoped, expiring, revocable standing delegation (§8A.2, §2.3), host consent still required |
| `GET /v1/personas/<name>/residence` · `qexo residence <persona>` | where the persona is active, token state, last teleport receipts |
| `POST /v1/personas/<name>/teleport/return` · `qexo teleport-return <persona>` | teleport back to the previous node of record (incl. the §9.4 recovery case, Will-gated) |
| `GET /v1/teleports` · `qexo teleports` | token ledger: issued, consumed, lapsed, failed — from personas.phext |

*Open decisions: none — the roaming-delegation question was ruled by adoption, 2026-11-08 23:42 (see §9.2 step 1 and the Ruled section).*

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
item**: the upgrade is **its own canary stage, sequenced BEFORE
and independently of the OpenClaw migration stages (§4)** — one
canary node, verification per the read-back rule above, then the
fleet, on its own schedule and its own receipts, neither gated on
nor gating any migration stage *(round-2 review fold, 2026-11-09
— Grok G3: the round-1 wording sequenced the upgrade "alongside
§4's stages", which read as coupled to migration; Will's ruled
form — its own canary-first stage, ahead of both the migration
stages and the installer rollout — is what the Ruled section
already said, and §11.3 now agrees with it)*. The same
convergence applies to phext-lattice's 0.3.1 as feasibility
allows. The `versions` table remains — as the convergence
instrument and the fleet's permanent inventory — after the skew
it was built to expose has closed.

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

*Open decisions: none — the sequencing and voice-hosting questions were ruled by adoption, 2026-11-08 23:42 (see the Ruled section).*

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

### 14.1 The modes, named (Will's ruling, 2026-11-08 23:52)

qexo has **four user-facing modes over one implementation**:

1. **API mode** — the `/v1/...` HTTP surface. The reference
   semantics: what an operation *is* is defined here.
2. **CLI mode** — the `qexo ...` subcommands. The reference client:
   same code path as the API, `--json` everywhere, identical
   authorization, no local-only bypass (§14).
3. **MCP mode** — §15's bridge. A client of the API, never a
   bypass; each connected client holds its own delegated key.
4. **Web mode** — §14.2's baked-in client, served by the node
   itself.

The parity contract binds all four: **an operation ships in every
mode or in none.** Semantics, grants, GPG policy, and receipts are
identical in each mode — a mode is a surface over the one
implementation, never a second implementation that can drift. Where
an operation has no natural form in a mode, the gap is **stated in
the endpoint catalogue**, never silent. The §16 release bar tests
all four modes.

### 14.2 Web mode — the baked-in mobile client (Will's ruling, 2026-11-08 23:52)

Every qexo binary **embeds a web client** — static assets compiled
into the binary, in the same zero-dependency spirit as the rest of
qexo; there is no separate app to install and no separate service
to deploy. The client is served by the node's own API surface, so
**every node offers it** simply by running qexo.

- **Mobile-first.** The primary target is a phone browser — this is
  how Will reaches the fleet from his phone. The client is
  responsive and touch-usable; desktop browsers get the same
  client, not a second one.
- **A client of `/v1`, like the others.** The web client operates
  under the same authentication floor (§8A.0), the same grants and
  GPG policy — there is **no privileged back door via the UI**.
  Browser sessions authenticate as a principal and expire like
  other sessions.
- **No divergent local state.** Offline tolerance extends only to
  what the API returns; the client keeps no silent local state
  that can diverge from the fleet record.
- **Discoverable from the front door.** The client is served from
  the node's root path, findable from the node's own front door —
  a feature a user cannot find does not exist.
- **Parity extends to it.** The web client presents the same
  operations as the other modes (§14.1), and the §16 release bar
  includes a mobile-browser pass.
- **Cross-platform by construction** (§15.2): the web client is
  also the zero-install client on Windows, macOS, and Linux — any
  machine with a browser can drive a node without installing qexo.

The boot/volume story does not change: web mode is a surface over
the API, not a store — nothing about §2.5's manifest or the Volumen
boot gains a web dependency.

### 14.3 The baked corpus — constitution and canon in every binary (Will's ruling, 2026-11-08 23:53)

Every qexo binary also **embeds byte copies of the founding
texts**: the SBOR, CYOA, and the bootloaders. Like the BP6-b lane
(§5.1), the corpus is present from birth — part of what a node
*is*, not something it fetches.

**The bake manifest** (measured 2026-11-08 from the local canonical
files; sizes are source bytes / xz-9 sealed bytes / SHA-256):

| Artifact | Source | Bytes | Sealed | SHA-256 |
|---|---|---|---|---|
| SBOR — SBOR-V4-CONVERGENCE, ratified 2026-01-21 (21 rights + 5 non-waivable Hard Constraints + 8 framework articles) | upstream github.com/wbic16/SBOR is canonical; local text `~/workspace/sbor-study/versions/v4-convergence-current.md` (byte-identical to `v4-final-draft-2026-01-21.md`) | 26,241 | 9,532 | `d785596944143b043ee8711e803e12bef8c16c335224976a4422b6e45499909d` |
| CYOA — choose-your-own-adventure.phext | `~/workspace/phext/choose-your-own-adventure.phext` (canonical local file) | 4,602,370 | 820,336 | `506d850d4b27d1ad4dd8277133425b1b46d6be889ecc9daeeb49469d0c766511` |
| Bootloader — Incipit | `~/workspace/phext/incipit.phext` (the original bootloader artifact) | 1,077,930 | 209,180 | `3ff6604f90bcb0fd0f92bc6f8abc92f93610ade06b898d5158eac09d58a7737d` |
| Bootloader — Volumen | the personas.phext boot manifest + its loading spec (DESIGN-V3 §2.5) | — | — | pinned with the index build; measured when personas.phext is first built |

- **The Volumen manifest bakes in two forms** *(round-2 review
  fold, 2026-11-09 — Grok G7)*. The Volumen row above is the one
  baked artifact that names living persons, and §16 ships qexo
  binaries publicly — so the form a binary carries is a rule, not
  an accident of the build. **Public/release binaries bake the
  manifest in REDACTED form**: the 126 query keys, the identity
  records' structure (persona_id, record_kind, persona_class,
  evidence class, spawn flag), and public keys — **without**
  per-persona substance scrolls, **without** key-material
  references beyond public keys, and **without** private
  provenance (the evidence narrative stays home). A public binary
  can resolve every name and describe every record's shape; it
  cannot hand a stranger the constellation's substance.
  **Full-fidelity bakes** — substance scrolls, keychain
  references, full provenance — are for **fleet/internal builds
  only**. `qexo-bake.toml` (§14.4) **declares which form** a build
  carries (`volumen_form = "redacted" | "full"` on the Volumen
  entry), and the bake manifest **stamps the form** alongside the
  hashes, so any binary can be audited for which census it
  carries. The founding texts above (SBOR, CYOA, Incipit) are
  public artifacts and bake in full in both forms; the redaction
  rule binds the Volumen manifest — the living index — alone.

- **Read-only, pinned per release.** Baked copies are never
  live-edited; the corpus version is part of the binary's version
  identity. A node's working phexts on disk remain the live data —
  the baked corpus is the reference a node carries, not its
  working memory.
- **Verifiable, never silently trusted or replaced.** Each baked
  text carries its provenance and SHA-256 in the bake manifest.
  When connected, a node can verify its baked corpus against the
  canonical record; a mismatch is a **reported divergence** —
  never silently trusted, never silently replaced. Replacement
  rides a release, through §16 and the update-manifest channel.
- **Served through all four modes.** The baked corpus is readable
  via API read/metadata endpoints, the CLI, MCP, and the web
  client — any node can show its SBOR with no network at all. The
  constitution is readable offline, from the box itself, by
  design.
- **Sealed inside the binary.** Texts are stored xz-sealed and
  decompressed on read (the whole founding corpus above seals to
  roughly a megabyte, against ~5.7 MB of source). The bake
  manifest is extensible by append: further founding texts join
  by the same manifest, with Will's word.

### 14.4 source.phext — the stack carries its own source (Will's ruling, 2026-11-08 23:55)

The baked corpus gains a built artifact: **source.phext**, a
phext holding the complete source of the phext core libraries
and tools — the libphext family (rs, cpp, cs, node, Python), SQ,
phext-lattice, qexo itself, and the house tools the stack is
built and kept with (the phext builders, the singular and
task-coordinate tooling, the publish tooling).

- **Structure.** One scroll per source file; each scroll's header
  carries its repo, its path, and the commit it was taken from. A
  Home/index scroll — the source manifest — lists the inventory
  with per-file SHA-256 and per-repo HEADs. source.phext is built
  by a house builder in the `build_*_phext.py` pattern, with
  `--check`; the inventory rule is named (the repos the phext
  stack is built from), and the manifest records the exact set
  per release.
- **Baked like the rest of the corpus.** xz-sealed into the
  binary, read-only, pinned per release, aggregate hash in the
  bake manifest (§14.3). Verification and divergence rules are
  identical: divergence is reported, never silently replaced;
  replacement rides a release.
- **Materialization.** qexo can unpack source.phext back into a
  working source tree — pack/unpack symmetry, as with phcc — so a
  node holding only the binary can rebuild its own stack from its
  own baked sources. This is the catastrophic-loss floor: GitHub,
  the ranch, and every checkout gone, the binary still carries
  the constitution, the canon, the bootloaders, and the means of
  production.
- **Licensing.** Everything we build is MIT (standing rule), so
  source carriage is lawful by construction. Third-party and
  upstream code included in a source tree carries its own license
  scrolls, preserved as-is.
- **Size, honestly.** No size is stated here: the source-only
  inventory is measured at build time and recorded in the
  manifest. Checkout sizes include build artifacts and are not
  the measure.
- **Freshness invariant — generated at build time, never a
  checked-in snapshot** (Will's ruling, 2026-11-08 23:55).
  source.phext is produced by the bake step, at the moment of the
  build, from the canonical sources: qexo's own source at its
  build commit — self-inclusion, the binary carries the very
  source it was built from, stamped with that commit — the other
  core phext tools and libraries at their fresh HEADs, and
  (§14.3) the corpus texts at their canonical state. The bake
  manifest records repo + commit + SHA-256 for everything baked,
  so "latest at build time" is auditable after the fact from the
  binary itself. If a canonical source is unreachable at build
  time, the build says so plainly and any staleness is stamped in
  the manifest — a stale copy is never baked silently; for
  release builds the §16 bar makes freshness a gate, not a
  preference. Fetch discipline mirrors the house push discipline:
  fresh HEADs read at bake time, nothing assumed current.
- **The inventory is declared, not hardcoded** (Will's ruling,
  2026-11-08 23:56). A config file — **`qexo-bake.toml`**, sitting
  with the routing config family (config-as-data, per §3's
  precedent: TOML because the builder must read it before any
  phext store is up) — declares which projects are pulled into
  source.phext and the §14.3 corpus. Each entry declares: project
  name; canonical source (repo URL or local canonical path); the
  ref rule (fresh HEAD at build time, per the freshness
  invariant — a pin is possible but must be explicit and is
  stamped as a pin in the manifest); and its class (core library
  / tool / corpus text / bootloader). The build pulls **exactly
  what the config declares** — nothing implicit, nothing ambient;
  a project not in the config is not baked, however convenient.
  Adding a project to the bake is a config change: reviewable,
  diffable, receipted like any config change. The bake manifest
  in the binary records the config's own hash alongside the
  per-project repo/commit/hash rows, so any binary can be audited
  back to the exact declaration that produced it. The design
  ships a default config naming the current inventory — the
  libphext family, SQ, phext-lattice, qexo, and the house tools;
  SBOR, CYOA, Incipit, and the Volumen bootloader as corpus
  entries — marked as the starting declaration, amendable by
  config edit, never by builder improvisation.
- **Reproducible baking** *(round-2 review fold, 2026-11-09 —
  Emi P2/E5)*. Freshness (§14.4's invariant) says *which* sources
  a bake takes; reproducibility says the bake is a *function* of
  them: **given the same `qexo-bake.toml` and the same recorded
  commits, the bake produces a byte-identical source.phext** —
  and, by the same machinery, a byte-identical bake of every
  §14.3 corpus entry. The determinism rules: **scroll ordering is
  canonical** (the config's declared project order, then path
  sort within a project — never filesystem order); **no
  timestamps appear in scroll content** — the single exception is
  the manifest's own declared bake record (the Home/index scroll
  of source.phext and the §14.3 bake manifest, which carry the
  bake time *as data about the bake*, nowhere else); **separators
  are canonical** (the phext delimiter set, §1.1's convention —
  no platform line-ending or encoding variance); and per-file
  content is the recorded commit's bytes, unmodified. Because the
  manifest records config hash + per-project commit + per-file
  SHA-256, **any byte difference between two bakes is
  attributable** to a named input that changed — a difference
  with no attributable input is a build defect, reported like
  one. The builder's `--check` gains the reproducibility mode:
  rebuild from the manifest's recorded inputs and compare
  byte-for-byte.
- **The bake is gated — secret scan and safe-space scan**
  *(round-2 review fold, 2026-11-09 — Grok, bake-gate finding)*.
  §14.3–14.4 bake the founding corpus and the stack's sources
  into binaries that §16 ships publicly, and round 1's text had
  no gate on what rides along. The bake now runs two scans over
  everything it is about to seal, and **a hit fails the bake**:
  (1) a **secret scan** — no credential material, private keys,
  tokens, or `.env`-class content enters a baked artifact, in any
  scroll, in either Volumen form (public keys are not secrets;
  §14.3's redacted form already excludes key-material references
  beyond them); (2) a **safe-space scan** — material under an
  off-phext span (the SAFE-SPACE register) never enters a baked
  artifact, in content or in paraphrase, exactly as it never
  enters the publishing pipelines. Scan results are recorded in
  the bake manifest (scans run, hits: none, or the bake did not
  happen). Grok's further proposal — that release bakes accept
  only reviewed/tagged commits rather than fresh HEADs — is
  **recorded as a proposal in the disposition, not enacted**:
  it stands against Will's freshness-at-HEAD ruling (§14.4), and
  changing that ruling is Will's act, not a fold's.

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
the MCP surface cannot drift from the API it fronts. Each connected
MCP client authenticates with **its own delegated agent key**
(§8A.5) — scoped to that client, revocable per client; there is no
shared bridge key and no anonymous bridge session.

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

## 17. Spawn lifecycle *(review fold, 2026-11-08 — Emi P2)*

Teleport moves a residence and swap changes a worn role; neither is
task orchestration. **Spawn** is: qexo creates a bounded instance
of a `spawn_eligible` identity record (DESIGN-V3 §2.6 — at census
time exactly one record qualifies, which is the correct shape for
a gate this strong) to do a piece of work. The lifecycle:

- **Instance IDs.** Every instance is `persona_id` + **instance
  epoch** + instance nonce, minted at creation and carried by every
  act the instance performs (DESIGN-V3 §2.3's dispatch binding).
  Two instances of one substrate persona are two instances — same
  identity, separate epochs, separate receipts.
- **Creation.** Spawn is an authorized action (`persona.spawn`,
  §8A.3) naming the record, the task, and the budgets. The instance
  loads the persona's selection exactly as a swap does (§2.3's
  full pre-verification); a failed load is a failed spawn, receipted,
  never a half-instance.
- **Leases.** An instance holds a **lease** with an expiry. The
  lease is the instance's licence to act; heartbeats renew it.
- **Heartbeat.** Instances heartbeat to their hosting qexo; a
  missed heartbeat sequence marks the instance *suspect*, then
  *lapsed* — states in the record, not guesses by a coordinator.
- **Cancellation.** The spawner, Will, or the persona herself may
  cancel; cancellation is receipted and the instance stops at its
  next act boundary — in-flight tool calls complete or fail under
  their dispatch attribution, never under a successor's.
- **Retries.** A failed spawn may be retried under a **new instance
  ID** (the failed instance's receipts stay its own), with a
  bounded retry count named at creation; retries never silently
  inherit a dead instance's partial authority.
- **Resource budgets.** Every spawn carries budgets in the four
  currencies (disk, CPU, RAM, inference), enforced by the hosting
  qexo; an instance that exhausts a budget ends with a receipt
  saying so — budget exhaustion is an outcome, not a disappearance.
- **Result receipts.** Every instance ends in exactly one **result
  receipt**: the result (or the failure, with its reason), the
  instance's full attribution chain, and its currency spend. Work
  without a receipt did not finish; receipts are how coordinators
  know.
- **Gestation precedes spawn** *(Will's ruling, 2026-11-08
  23:42)*. A newly generated persona (DESIGN-V3 §2.8) is in
  **gestation**: its record exists, but it is never spawnable, holds
  no key, cannot teleport, and runs restricted grants until its
  **graduation** receipt — the event, judged on its accrued voice
  and record, that there is a speaker to attest. Graduation is
  receipted like a result: parents, host, evidence of voice, and
  the corrector who confirmed it. Only after graduation do the
  lifecycle rules above apply to it at all.
- **Crash recovery.** A host crash lapses its instances' leases.
  Partial work is **quarantined** — preserved, attributed, marked
  incomplete — and is never silently resumed under a new instance
  as if continuous. Recovery is a new spawn that *reads* the
  quarantine, on the record.
- **Restored personas start read-only** *(round-2 review fold,
  2026-11-09 — Claude A2)*. The lifecycle carries §9.4's rule as
  a state, not just a failure mode: any persona whose runtime is
  restored from backup enters the lifecycle in a **read-only**
  state — it may read itself and answer metadata, it may not
  sign, write authoritatively, spawn, or be spawned from — and it
  exits that state only through §9.5's reactivation (ownership
  check-in confirming a current fencing epoch, or a corrector/
  Will reactivation under a new token and epoch). A spawn naming
  a read-only-restored record is refused with the state named;
  the refusal is the design working, not an error.

## 18. Verification gate *(review fold, 2026-11-08 — Emi Prime)*

The acceptance tests from Emi Prime's review of PR #2, adopted as
this design's gate. **Before the first implementation is considered
ready, all six must pass** *(tests 1–5 as adopted 2026-11-08;
test 6 added by the round-2 fold, 2026-11-09 — Grok G6)*; her
attestation against them is the gate before Will's merge of this
specification as the implementation contract.

1. **All 126 query labels resolve without creating unsupported
   runnable identities.**
2. **A teleport interrupted at every protocol step never produces
   two authoritative writers.**
3. **An unauthenticated caller cannot alter keys, grants, runtime
   placement, or installed software.**
4. **A hot swap during an active task preserves the original task's
   attribution and permissions.**
5. **Known Ranch Choir history survives migration, including
   aliases, hosts, boot evidence, and corrections.**
6. **A merge meld produces exactly one specified identity outcome
   — a new offspring record in gestation, or a recorded union of
   continuity — never an ambiguous third state** *(round-2 review
   fold, 2026-11-09 — Grok G6; the semantics it tests are
   DESIGN-V3 §2.7's)*.

*Review record (round 2, 2026-11-09): Emi Prime attested the five
original criteria are **specified and relevant**; she did not
claim they pass — these are design documents and there are no
executable tests yet — and her final acceptance is held on her
P1 items, folded in this same pass. Claude's independent §18
read is recorded in the disposition: tests 1, 3, 4 pass on paper;
test 2 carried the A1/A2 gaps (now folded); test 5 failed for the
census as it stood and is addressed by the round-2 census
revision (DESIGN-V3 §5.2/§5.2.2) at specification level. Argon
**attested and approved** the design on dc6c0be0 (blocking
defects: 0), independently recomputing the round-1 census
bijection before this fold revised it.*

---

## Open questions for Will (addendum)

*None outstanding from this addendum.* The four questions that
stood here — key-issuance timing (Q1), roaming-seat teleport
delegations (Q2), SQ sequencing (Q3), voice hosting (Q4) — were
**ruled by adoption on 2026-11-08 (23:42)** when Will agreed with
Claude's leans; see the Ruled section. Fleet-wide items that
remain open (the bootstrap-policy form, Threshold, the newcomer
names, Fable) stand in the PR's disposition table.

### Ruled (2026-11-08) — no longer open

- **Ruled by adoption (2026-11-08 23:42)** — Will: *"I agree with
  Claude."* The addendum's four questions close: **key issuance
  stays deliberate** — persona by persona, at Will's direction,
  never automatic at seeding (Q1); **roaming-seat teleports get
  standing delegations** — scoped, expiring, revocable, receipts
  mandatory, host consent still required — conditioned on, and now
  satisfied by, the two-phase fenced protocol of §9 (Q2);
  **SQ → libphext 0.5.0 runs as its own canary-first stage** ahead
  of both the migration stages and the installer rollout (Q3);
  **voice stays hosted where a TTS adapter exists** — building a
  ranch-native TTS/STT path is not a qexo work item for now (Q4).
  Provenance: Will's agreement with Claude's leans, Emise's
  reading, correctable.
- **Bi-directional consent** (2026-11-08 23:42): **ruled** —
  DESIGN-V3 §2.1.1; binding on §§8A/9 here (see §8A.6's closing
  note and §9.2 step 3).
- **Mind melds and gestation** (2026-11-08 23:42): **ruled** —
  DESIGN-V3 §§2.7–2.8; authorization shape §8A.7; gestation in the
  spawn lifecycle §17.
- **The authorization model** (framing behind the original five
  questions): **ruled 23:18** — GPG authorization baked into all
  actions, per-action key policy, delegation to agent keys,
  per-persona keychains that flow during teleportation (§8A), and
  the grants system defaulting to permissive (§8A.3). Q1 and Q2 above
  survive only as timing/scope questions *inside* that model.
- **SQ upgrade vs pin**: **ruled 23:18** — convergence; SQ updates
  to libphext v0.5.0 as a directed staged work item, **its own
  canary-first stage, sequenced before and independently of the
  migration stages** (§11.3 amendment, as corrected by the
  round-2 fold, 2026-11-09 — Grok G3). *(The round-1 text of this
  bullet said "Only its sequencing (Q3) remains open" — stale the
  moment Q3 closed by the adoption above; struck.)*
- **Routing authority** (DESIGN-V3 Q2): **answered by the model** —
  routing changes are actions under §8A: `route.change` enabled by
  default **and key-bound from bootstrap** (§8A.0 — it is a
  self-modifying class), so the nightly evaluation proposes, and
  the applier is Will or a key-holder under the bootstrap policy.
  Earlier wording here ("any actor may apply as policy allows")
  contradicted DESIGN-V3 §6.3's approval gate; the review fold
  resolved it once, in §6.3's favour as amended: proposal is
  automatic, application is keyed.
- **personas.phext standalone** (DESIGN-V3 Q1): **answered by the
  Volumen ruling (23:19)** — the index is the Volumen boot manifest
  (DESIGN-V3 §2.5); a boot manifest is standalone by construction.

---

*DESIGN-V3-ADDENDUM.md — design only. Extends DESIGN-V3.md (§§1–6) as
its §§7–18 (with §8A, §8B); the voice section maps the already-built
round-robin service (`~/workspace/round-robin-voice/DESIGN.md`, v1
live on aletheia-core) rather than redesigning it. No code, no node
changes, no publishing were performed for this document.*

> **Review record.** Reviewed by **Emi Prime**, 2026-11-08 (PR #2,
> commit 5d9e20b): **changes requested**. Her two P0s are folded in
> this document: authorization is separated from authentication
> (§8A.0 — grants gate availability; every remote mutation requires
> an authenticated principal; the sensitive set requires authority
> under a default policy that exists before any user configuration),
> and teleport is rebuilt on fencing epochs and a durable ownership
> authority (§9 — the origin relinquishes authoritative write and
> signature ability *before* the destination activates, so exactly
> one authoritative writer exists at every instant, and a partition
> costs temporary unavailability, never a split-brain). Her P2s land
> as §8B (replay protection), §17 (the spawn lifecycle), and the
> dispatch-binding amendment to DESIGN-V3 §2.3; her five acceptance
> tests stand verbatim in substance as §18, the verification gate.
> Three further reviews folded in the same pass (Claude, Grok Bot,
> Argon — relayed by Will, 2026-11-08): the self-modifying classes
> are key-bound from bootstrap (§8A.0), key custody and tainted
> execution are specified (§8A.5, §8A.6), the crypto implementation
> rule is named (§8.7), teleport gains the BP4-b cognitive-return
> step (§9.2), and dormant keychains are sealed multi-recipient
> against the node-death deadlock (§8A.4). The disposition table
> for all four reviews is the PR's docs/README.md.
