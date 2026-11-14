# qexo changelog — the scratch line

## 1.5.0 (v1.5.0) — Learn You a qexo!, 2026-11-14

**The learn wizard.** `qexo learn` on a terminal is now a
guided wizard in the Learn You a Haskell spirit, built at
Will's direction ("qexo learn needs to be a learnyouqexo
wizard TUI"). The engine is untouched — same 22-card deck,
same SM-2-lite scheduling, same records in state.phext —
only the surface changed:

- A chapter map of qexo's capabilities: seven chapters from
  First steps to The whole shelf, each with its mastery and
  due counts.
- A short lesson screen per chapter, in a playful-but-plain
  voice drawn only from the shipped CLI's actual behavior.
- Practice interleaved with the lessons: a chapter's due
  cards, or the whole due queue (stalest first, as before).
- The progress header rides every screen: mastered, due,
  session streak, next due.
- Keyboard-first: space reveals, 1/2/3 grade (a/g/e too),
  b returns to the map, d jumps to the due queue, q quits
  from anywhere. Progress saves after every card, as before.
- Phone-terminal-first: every line wraps to the terminal
  width (40 columns when the size is unknown — the Termux
  shape). Unix runs cbreak mode via std-only termios; where
  raw mode is unavailable (Windows) the same screens run in
  line mode. Piped stdin still gets the status view and
  --status still never writes.

## 1.4.2 (v1.4.2) — the learn-resolution refinement, 2026-11-14

**A regression v1.4.1 introduced, caught by its own
crates.io consumer proof:** v1.4.1 classified `qexo learn
--status` as write-intending. On a machine with a readable
root-owned system state but NO per-user state yet, resolution
diverted to the missing per-user path and `learn --status`
errored "not born yet" — where v1.4.0 answered from the
readable deck. A consumer installing qexo on a fleet node
met a wall where 1.4.0 had given an answer.

**The refinement:** learn carries a preference, not a write
intent, in its `--status` form. When a per-user state
EXISTS, `qexo learn` (both forms) selects it over the system
state — the deck is the operator's own, which was defect
#3's substance. When none exists, learn falls through to
the shipped resolution order and answers from whatever deck
is readable, exactly as v1.4.0 did. Sessions remain
write-intending (they save grades), so a session with no
node of one's own still points at `qexo config` — which
births the per-user node — instead of dying on a save.
Everything else in 1.4.1 stands unchanged.

## 1.4.1 (v1.4.1) — the upgrade-path hotfix, 2026-11-14

**The defect, stated plainly:** v1.4.0's update post-check ran
doctor in check mode against the state the node inherited.
States born under 1.3.0 carry no lessons homeland (10.10.10),
so the new doctor's only complaint was a missing homeland
index — a finding doctor itself marks fixable. The post-check
failed anyway, the rollback restored 1.3.0, and the rollback
removed the only binary that could run the fix. The fleet
refused 1.4.0 fleet-wide, cleanly, with receipts. The battery
had never updated a predecessor-born state — every test state
was 1.4.0-born.

**The fix is two layers.** (1) State top-up on write: every
`Lattice::save` first seeds any homeland index scroll the
current Layer 2 layout defines that the document lacks — an
upgraded state becomes whole by the running binary's own hand,
idempotently, touching nothing that already exists. Read and
--check paths never write, so their byte-inert guarantees
stand. (2) Post-check repair distinction: when the swapped
binary's doctor fails, the new binary's own `doctor --fix`
runs, repairs only what doctor labels fixable, and the check
runs again; repairs are named in the update receipt and in the
console. Hard problems (an unparseable state, a missing
keyring) still roll back. A reload-before-receipt step keeps
the receipt save from clobbering repairs the new binary made.

**The missing regression gate is now standing:** a real
1.3.0-born state (tests/fixtures/state-1.3.0.phext, born by
the published 1.3.0 binary's own `qexo config`, never
hand-edited) rides a full signed two-node update in the
battery — the offer must be accepted, the learn index present
after, doctor clean, the repair on the receipt — and the same
fixture is an in-crate test for every future version.

**Three field defects from Will's v1.4.0 install, folded in.**
(1) URLs from `qexo list` didn't connect from aletheia-core.
Diagnosis on the node: a bare hostname resolves to the
machine's LAN address (aletheia-core → 127.0.1.1, rock-mountain
→ its 192.168 address), where serve does not listen — and the
live fleet display printed `http://peer-768b2a06:8484`, a
synthetic record name dressed as a hostname, which names
nothing at all. Only the fully-qualified tailnet name
(`*.pizzly-halfmoon.ts.net`, carried in resolv.conf's search
line) resolved to where serve listens. Now: `list` prints the
fully-qualified tailnet form where a hostname is known
(host: from the key, else a real record name), the recorded
endpoint verbatim where no hostname exists (a tailnet IP
connects as printed), and the node's own serve URL on its
self line; `register` records a bare tailnet-IP endpoint in
its DNS form when the hostname is known. (2) Public keys are
never shortened: fingerprints print in full — all 40 hex —
everywhere a human reads them (list, status, register, update,
evolve, receipts as printed, index scrolls). (3) `qexo learn`
couldn't run as a standard user: resolution tested existence
plus readability, which hands an unprivileged user the
fleet's root-owned state — and the first save dies with
Permission denied. Resolution is intent-aware now: a
write-intending verb in unprivileged hands selects the
per-user state whenever the system state is not writable by
it (learn in both forms, config, register, plan, the apply
modes, doctor --fix); read-only verbs keep the v1.4.0 order
exactly, and root's fleet behavior is unchanged.

## 1.4.0 (v1.4.0) — built and gated, 2026-11-14

**The install model changes: qexo no longer assumes root.**
Through v1.3.0 the state path was /var/lib/qexo/state.phext,
full stop — so the first person to `cargo install qexo` and
run it as themselves met permission walls: read verbs wanted
sudo, and a fresh `qexo config` needed root to birth. v1.4.0
resolves the state path, in this order: QEXO_STATE when set;
the system path /var/lib/qexo/state.phext when it exists and
is readable; the per-user path ~/.qexo/state.phext (Windows:
under the user profile) when a per-user state exists; the
system path again when it exists unreadable with no per-user
state (the honest error names the real file); and for a fresh
birth, the system path for a privileged process, the per-user
path otherwise — with a birth-time fallback to the per-user
path, announced out loud, when the system directory cannot be
created by the user running `qexo config`. Read-only verbs
never require sudo and never fail on system-path permissions
when a per-user state exists; a per-user node is whole — state
and GPG keyring side by side under ~/.qexo. A node has ONE
state: resolution finds the first that exists, `qexo status`
names the live path, and there is no migration — no per-user
deployments existed to migrate from. System installs are
unchanged, byte-for-byte: root's /var/lib/qexo stays canonical
wherever it exists and is readable (the fleet runs exactly
that, untouched).

**`qexo list` is a navigation tree now.** The phextnet renders
rooted at the peers anchor 3.3.3/3.3.3/3.3.3: this node first
and marked, each peer nested under its record address with its
name, status (registered/discovered), and endpoint — Phext
Notepad style, plain ASCII-safe branch marks. `list --json`
keeps the flat machine shape (JSON is the interop gasket,
unchanged); the serve route /v1/peers keeps the flat text form.

**Peers have names: the host: field.** At register, the peer's
host name is read from its key's user id ("qexo node <host>
(qexo node key)") and recorded as the record's host: field;
re-registering backfills it on records that predate it, and
gossip carries it into discovered records. Displays — the list
tree, status, update and evolve output — show hostnames, and
an endpoint whose authority is a bare IPv4 address is shown
by name instead; the recorded endpoint is never rewritten
(polling dials what the operator registered) and a bare IP
surfaces only when no name is known. Records that predate
host: display by their record name with no rewrite — for the
fleet those names ARE the tailnet hostnames. The fleet
convention (name = tailnet hostname; register by hostname
endpoint) is written into the register/config help text.

**`qexo learn` — spaced repetition over qexo's own surface.**
A 22-card curriculum deck lives in a new Layer 2 homeland —
10.10.10/10.10.10/10.10.10, lessons & learning (the tens:
practice, repeated) — one scroll per card at cluster 2, front
and back as field lines, scheduling fields (due,
interval_days, ease, reps, lapses) updated in place by
practice. The deck ships with the binary and seeds into state
on the first session, never overwriting a card already in
state. Cards cover every verb, the trust model (signer, GPG,
discovery-is-never-trust), the phextnet shape, and the state
homelands, each checked against --help. SM-2-lite: again =
lapse, interval 1d, ease −0.20 (floor 1.3), reps reset; good =
1d then interval × ease; easy = 3d then interval × ease × 1.3,
ease +0.15. Mastered = interval ≥ 21 days. Interactive
sessions run due cards (Enter to reveal, grade 1/2/3, saved
after every card); --status reports deck/due/mastered without
writing; piped stdin prints the status view instead of
hanging.

**`qexo inventory` (alias `catalog`) — the ecosystem roster.**
A baked, phext-native roster of the phext ecosystem: every
product with what it is, its home, and its install/launch
lines. The roster is a census, not a hand list — all 201
wbic16 GitHub repos were enumerated and their outputs judged;
only products with a verified published artifact are baked
(crates.io / PyPI / npm packages, GitHub release assets, a
live hosted product, or a documented library): 11 entries,
each carrying its verification evidence and the census date
(2026-11-14). `qexo inventory <name>` prints one full record;
`--install <name>` is print-only — qexo never installs
ecosystem products silently. Notable exclusions, all in the
census record: phext-mcp (its README documents a cargo
install command for a crate that does not exist on crates.io),
md2phext and the Android port and exo-search (real tools, no
published artifact yet), and three name-collision crates
(weft, tersify, hello-phext) that are NOT Will's.

The parser layer is untouched: the conformance census against
the libphext 0.6.0 probe is unchanged from v1.3.0.

## 1.3.0 (v1.3.0) — built and gated, 2026-11-13

**libphext-rs is the parser now.** qexo no longer reckons
phext itself: `src/phext.rs` is a thin adapter over the
`libphext` crate (pinned `0.6.0` from the crates.io registry;
the slice was first built against 0.5.0 and re-gated on
0.6.0 — the chains release: 0x02 chain break, 0x03 chain
close, $N: universe addressing — before merge), and address
computation, delimiter emission, and duplicate-address
behavior are the library's, full stop. The adapter's used
surface (`explode` / `implode` / `Coordinate`) is unchanged
in 0.6.0, so the bump is the pin alone; qexo builds no chain
features in this slice.

Under 0.6.0 the old-state verdict sharpens: a state written
by the hand-rolled serializer is, to a chain-aware reader, a
chain — its 0x02 library breaks are chain separators — and
`parse_chains` recovers each old library whole as its own
universe, every record at its exact universe-relative
address (the universe index carries the old Library number;
measured on aletheia-core's fleet state: 9 members, 106
scrolls, config record at $9:1.9.9/2.1.1/1.1.1 with its
signer intact — see the build record). The chain reads as
unclosed (the old serializer never wrote a 0x03), which the
format flags as possibly truncated; the nine complete
homelands say otherwise for these files. qexo's own reader
stays the single-phext `explode`, under which old states
still parse to drifted addresses and remain unusable as
state; the fleet wipe and rebirth stands. The chain reading
matters as archaeology: nothing in an old state is lost to
a reader willing to read it as a chain.

Why, in one paragraph: through v1.2.0 qexo carried a
hand-rolled parser whose delimiter table diverged from
libphext in exactly one byte — its library break was \x02
(STX) where the reference uses \x01 (SOH); the other eight
breaks agreed. The divergence was invisible for seven
versions because qexo only ever read files qexo wrote. It
surfaced on 2026-11-13 when Will edited his laptop node's
config record in Phext Notepad to set `release_signer`, and
`qexo update` kept refusing: the editor's append (SOH×10,
US×8, RS×8, GS×1, then the filled signer scroll, appended
after the 9.9.9 homeland anchor) landed at 9.17.9/2.1.1/1.1.1
under qexo's reckoning, at 11.9.9/2.1.1/1.1.1 under
libphext's (measured identically with SQ 0.6.0 / libphext
0.3.3 and with libphext 0.5.0 before this slice was built) —
and under neither reckoning does the append shadow the
config record, whose signer line stayed empty. A file is
only phext if the reference implementation reads it the
same way; now there is one reckoning in the codebase,
imported, not re-derived.

What the adapter keeps: qexo's document model (a BTreeMap
of [u32; 9] coordinates to scroll texts — the shape
lattice.rs and the Layer 2 addressing are written against),
its coordinate string grammar, and the format's own rule
that a scroll exists when it holds at least a byte
(libphext's explode also names the empty stops on a climb;
the adapter keeps only scrolls with text). Nothing else in
the old phext.rs was genuinely qexo-specific.

Regressions added: `library_break_is_soh_not_stx` (the
one-byte divergence, pinned) and
`editor_append_lands_where_libphext_puts_it` (tonight's
file shape byte-for-byte as a fixture: the signer scroll at
11.9.9/2.1.1/1.1.1, nothing at the hand-rolled parser's
9.17.9/2.1.1/1.1.1, the config record unshadowed and itself
found at 1.37.9/2.1.1/1.1.1 under the library's walk of an
old-serializer file). All 48 pre-existing tests pass
unmodified — none encoded the divergent behavior, which is
precisely how it survived: every test roundtripped through
qexo's own serializer. 50/50 with the two new ones.

Siblings in this slice:

- `qexo config --release-signer <FP>` — the trusted signer
  fingerprint becomes a command (written into the config
  record via the lattice field editor, on birth and on
  re-run alike) instead of a hand edit of the scroll from
  another tool — the gap that started the evening.
- `qexo select <coord>` — the verb gains the state form
  Will reached for: one argument that parses as a coordinate
  reads this node's state. The error he hit ("<coord>: The
  system cannot find the path specified. (os error 3)") was
  never a platform bug: the verb took its first argument as
  a file path unconditionally, so the coordinate was opened
  as a path. Reproduced on Linux with the v1.2.0 binary
  ("No such file or directory (os error 2)", exit 2); the
  two-argument file form is unchanged, and a bare verb now
  prints usage instead of inventing a path.

Old-state verdict (states written by the hand-rolled
serializer — i.e. every fleet node today), measured with
this build against a copy of aletheia-core's live state:
the file parses cleanly and every scroll's text survives
(106/106), but the records stand at drifted addresses under
the library's walk — doctor finds the home index (Library
1 is unaffected) and reports all eight higher homeland
indexes missing; the node key record, the peer registry,
and the config record (signer included) are unreadable at
their Layer 2 addresses. A v1.2.0 state is therefore not
salvageable as state: the fleet will be wiped and re-born
under the corrected parser (runbook prepared separately).
What survives a wipe untouched: the node's GPG identity —
the keyring lives in /var/lib/qexo/gnupg beside the state,
and config reuses it on rebirth, so node fingerprints stand
— plus every .asc and fingerprint in the fleet registry,
from which the peer wiring is rebuilt.

Version note: wbic16/qexo's Android port also wears 1.3.0
in its own repo lineage — versions are per-repo; this is
quarterexo's 1.3.0.

Gates: cargo test 50/50; release build; wiring-check 12/12;
full windows-gnu LINK (qexo.exe, GetDiskFreeSpaceExW
imported, no statvfs); sandbox — birth, the new setter,
both select forms, register + `update --check` against the
live fleet signer ("already current — this node runs qexo
1.3.0; the trusted signer offers 1.2.0"), `evolve --check`
gossiping the real fleet, bare plan disk gate GREEN on the
ranch volume (74% free, volume lens).

## 1.2.1 (v1.2.1) — built and gated, 2026-11-13

The forecast stops whipsawing: growth is rated from the
median interval, not the last one. The nightly cycle's one
change, from the 2026-11-13 evaluation of 2026-11-12
(EVALUATIONS.md): v1.2.0's `forecast_gate` rated growth from
the history's last two observations, so the nearest-crossing
estimate swung with whatever the last day happened to do —
file AMBER read ~1.4 days out on the 2026-11-10 disk-watch
interval, ~12–14 days out on 2026-11-11's, and ~9.9 days out
on 2026-11-12's, while the underlying series never changed
character. A warning that moves ten days in a night is not a
warning anyone can plan a staging move around.

What changes, all inside `src/plan.rs`:

- The rates are the median of the per-interval rates over
  the most recent 7 intervals (`FORECAST_WINDOW_INTERVALS`);
  older intervals fall out of the window instead of anchoring
  the rate forever. A history with exactly two observations
  keeps the pre-v1.2.1 single-interval rate unchanged — every
  pre-existing forecast test passes unmodified.
- `Forecast` gains `intervals` (how many interval rates the
  reported rates are the median of), and the rendered
  forecast line names its basis: "median of N intervals,
  X d" (or "last interval, X d" for the two-observation
  case). `span_days` is now the window's span.
- On the real disk-watch series (2026-11-08 → 2026-11-12:
  file intervals +124,074 / +59,021 / +5,372 / +7,064 per
  day) the median rates +33,043 files/day and +2.84 GB/day
  and projects file AMBER ~2.1 days out — against the
  single-interval +7,064/day and ~9.9 days the same history
  produced the night before.
- New tests: `forecast_median_damps_single_interval_whipsaw`
  (the real series above, with the single-interval contrast
  asserted in the same test) and
  `forecast_window_excludes_old_spike` (a +400,000-file
  interval eight intervals back is outside the window, not
  merely damped).
- Nothing else changes: the gate's lenses, thresholds, and
  verdicts are v1.2.0's; the forecast still escalates GREEN
  only, still projects only the node's configured lenses
  (v1.0.3), and still degrades to unavailable on fewer than
  two observations.
- Version: Cargo 1.2.0 → 1.2.1; VERSION v1.2.1; the plan
  payload reports `plan_version: v1.2.1`.
- Ordering note: PR #16 (v1.3.0, libphext-rs as the parser)
  is open against the same base (exo @ 833e2f34). This
  change does not touch `src/phext.rs`; the only overlap is
  the version stamps. Merge this PR before #16, or rebase
  its stamps to v1.3.1 after — either order lands both.

## 1.2.0 (v1.2.0) — built and gated, 2026-11-13

`qexo evolve` — one receipted convergence pass over the
phextnet. The reading, in one paragraph (Will's order was two
words, "qexo evolve"; this reading is Emise's, stated so it
can be struck): `update` moves the binary; `evolve` is the
node's full convergence pass — everything a node can learn
from its peers, applied in one receipted cycle. What a node
knows, what its personas have become elsewhere, and what its
binary should be are three faces of one question — how far
has this node drifted from the network it belongs to — and
evolve answers it in a single pass that leaves a single
receipt.

The four stages, in order, each independently failable — a
stage's failure never blocks the next, and every outcome
lands in the receipt:

1. **Peer gossip.** For each trusted peer with an endpoint,
   the pass reads its peers index (serve gains a general
   scroll-by-coordinate route, `/v1/scroll/<coord>`, to carry
   it) and fetches each record the index names.
   Peers-of-peers this node does not know are recorded in the
   peers homeland as `status: discovered`, with a `via` field
   naming the reporting peer and the reported fingerprint
   (plus endpoint when the report carries one).
2. **Persona sync.** Persona records now carry a lineage —
   `revision` (1 at birth, +1 per local edit), `follows` (the
   revision and stamp the copy directly succeeds — the scroll
   names the revision it follows), `history` (the stamp
   chain), and `stamp` (a content address: sha256 over name +
   revision + follows + boot). For each persona this node
   holds, trusted peers are polled for their copy (found
   through the peer's own personas index). A copy whose
   history extends ours is a descendant and is adopted whole;
   a copy behind ours is noted; anything else is a fork.
3. **Self-update.** The v1.1.0 mechanism, invoked in-process
   — same trust rule, same swap, rollback, and receipts;
   evolve's receipt references its outcome.
4. **The evolution receipt.** One scroll per pass in the
   ledgers homeland's receipts family: peers polled / learned
   / confirmed, personas considered / adopted, divergences
   kept, the binary outcome, and every warning, each named.

The two trust rules. **Discovery is knowledge, not trust:** a
discovered peer is never polled for releases, never synced
from, and never eligible as `release_signer` — every polling
path iterates only records whose status is `registered`, and
a discovered record becomes trusted only when the operator
registers its key (`qexo register` then promotes the record
in place, keeping its `via` provenance). A gossip report that
matches a trusted peer's fingerprint is a consistency check;
a fingerprint conflict for a known name is a warning line in
the receipt, never a silent overwrite. **Divergence keeps
both:** when two nodes advance the same persona from a common
ancestor differently, the incoming copy lands at a sibling
address in the same persona cluster as an attempt artifact
named by its origin node (the KSO ruling) — never
last-writer-wins, never a silent drop, and the receipt names
every divergence kept.

`qexo evolve --check` performs every read and prints the
plan, and writes nothing at all: the knowledge stages run
against a throwaway copy of the lattice and the binary stage
runs in a judge-only mode that suppresses even the refusal
receipts `qexo update --check` has always written — verified
byte-for-byte against a hashed fixture. `update ⊂ evolve`:
the v1.1.0 auto timer's trigger becomes `qexo evolve` when
the new config key `evolve: on` is set (`qexo config
--evolve`; default off; flipping it while the timer is
installed reinstalls the unit with the new trigger — a unit
names its command, so the key alone never pretends to
rewrite it). Manual `qexo evolve` always converges, whatever
the key says; the key only steers the timer. Persona records
born before lineage existed are not rewritten: they read as
revision-1 roots whose stamp is computed from their content,
and identical roots share ancestry by construction.

## 1.1.0 (v1.1.0) — built and gated, 2026-11-12

Automatic updates over the phextnet: qexo nodes now update
themselves through qexo's own peer network — not through
GitHub, a package repo, or anyone's hands.

The mechanism. A release is announced as a signed scroll: the
signer runs `qexo release`, which stages the running binary in
a `releases/` directory beside its state file and writes a
manifest record into the ledgers homeland of its state.phext —
version, sha256, size, built-from commit, signer fingerprint —
with a detached GPG signature over the manifest scroll itself
in the adjacent scroll. `qexo serve` carries the manifest and
its signature as scroll text (the existing routes' shape) and
the binary at one byte route, `/release/qexo-<version>`. The
manifest is the trust object — what travels and what gets
verified; the bytes are just bytes, and integrity never
depends on the transport.

The trust rule. `qexo update` polls the registered peers'
serve endpoints and applies a release only when ALL of these
hold: the manifest's signature verifies, and the fingerprint
it verifies under is exactly the config record's
`release_signer` AND that key stands in the node's own peer
registry; the offered version is strictly newer than the
running binary (never a downgrade, never a same-version swap);
and the downloaded bytes hash to the manifest's sha256. The
swap is renames within the binary's directory: the old binary
keeps as `qexo.prev`, the new one must speak its version and
pass doctor against the node's state, and any failure rolls
back to `qexo.prev`. Every applied, rolled-back, or refused
attempt writes a receipt scroll. An offer that CLAIMS the
trusted signer but fails verification is a hard stop with a
receipt — that shape is impersonation, and the node does not
shop around. Offers signed by anyone else are noted and
ignored: each node's trust is its own config record's word.
`qexo update --check` reports only. `qexo config
--auto-update` sets `auto_update: on` and installs a systemd
timer (`qexo update` every `update_interval_hours`) where
systemd + root allow — and says plainly, with the unit files
printed, where it can't; the timer is a thin trigger and every
decision stays inside `qexo update`.

Supporting changes. Peer records gain an `endpoint` field —
`qexo register <pubkey> --endpoint <url>` records it, and
re-registering a known key backfills it; `qexo update` names
every registered peer that has no endpoint rather than
guessing one. serve's bind address becomes the config key
`serve_bind` (default 127.0.0.1, as before): a node that wants
phextnet traffic sets it to its tailnet address. The config
record gains the update keys visibly at birth and by backfill:
serve_bind, auto_update (off), update_interval_hours (24),
release_signer (EMPTY — trust is an operator's act, never a
default).

Fleet prerequisite, stated as operator steps, not fine print:
(1) cross-register the nodes' public keys so each node's
registry holds the signer's key; (2) register with endpoints
(or backfill them) so peers are pollable; (3) set
`release_signer: <fingerprint>` in each node's config record;
(4) on the signer, set `serve_bind` to its tailnet address and
run serve where peers can reach it. Until steps 1–3 are done,
`qexo update` answers with the plain refusal those steps
answer — "no trusted release signer configured" — which is
the fleet's true state tonight: keys exist on 19 nodes,
cross-registration has not run.
## 1.0.4 (v1.0.4) — built and gated, 2026-11-12

The disk probe declared POSIX on every platform. probe_disk
read its volume through an unconditional `extern "C" statvfs`
— a symbol Windows does not have — so the Windows build of
v1.0.3 failed at link time with `unresolved external symbol
statvfs`, reported by Will from exo 1b819c2. The unix build
linked fine, and the v1.0.1 gate set could not have caught
it: those gates ran `cargo check` for the Windows targets,
and check never links, so an unsatisfiable extern sails
through. The gate set now includes a full Windows link
(`cargo build --target x86_64-pc-windows-gnu`, producing
qexo.exe), not just checks.

The volume read is now one function per platform, behind
`volume_at(root) -> Option<Volume>`: unix keeps the statvfs
read with identical semantics (total = f_blocks, used =
f_blocks − f_bfree, free = f_bavail — df's Used/Avail);
Windows reads the volume through kernel32
GetDiskFreeSpaceExW, mapped into the same terms (used =
total − total-free-bytes; free = bytes available to the
caller, the quota-aware f_bavail analog), so the gate cannot
tell which platform measured; any other platform — and any
failure — yields no volume, which plan already tolerates
(every consumer reads the volume through Option). The
mapping is a pure function, unit-tested on every platform.
The file-count half of the probe is unchanged, as is all
unix behavior. An audit of the rest of src/ found no other
unconditional unix extern: doctor's kernel32 block is
cfg(windows) (v1.0.1), the PermissionsExt uses sit inside
cfg(unix) blocks, and the /proc reads (hostname, meminfo,
loadavg) are ordinary file reads with graceful fallbacks —
on Windows they simply find nothing, which is a snapshot
that reads empty, not a build that fails.

## 1.0.3 (v1.0.3) — built and gated, 2026-11-12

The forecast kept the old VM ceilings after the gate moved.
v1.0.2 re-based the disk gate on the volume a node stands on —
a volume lens, a quota lens only where configured — but the
forecast gate was left untouched, still projecting crossings of
the legacy absolute levels: byte AMBER at 95 GB, byte RED at
102 GB. Every ranch volume already sits past 95 GB used, so on
the 2026-11-12 fleet rollout the second plan read on any node
flipped its headline to "AMBER — FORECAST AMBER — byte AMBER
(95 GB) projected in 0.0 days" over a gate that was GREEN with
66–92% of the volume free. A forecast that contradicts the gate
it forecasts is a bug, and this corrects it.

The forecast now projects the crossings of the same lenses the
gate reads, from the same config record: the volume lens's
levels are the used amounts at which this volume's free
fraction crosses the configured AMBER/RED percentages; the
quota lens contributes crossings only on nodes that have it on,
and they are named as quota crossings in the node's own
numbers; the file lens reads its thresholds from the config
record as before. The hardcoded 95/102 levels are deleted from
the forecast path — the only place those numbers remain is as
the quota lens's config defaults, off by default.

Two projection rules changed with the levels, both forced by
the same evidence. A crossing the current reading has already
reached is the gate's verdict, not a forecast, and is no longer
projected at 0.0 days. And projections are reported only
within a one-year horizon: a linear rate is a short-horizon
instrument, and a crossing tens of thousands of days out (the
ranch case: ~2,900 GB away at +0.06 GB/day) is reported as no
crossing rather than lent a day count it has not earned. The
old forecast tests that asserted the legacy levels were
rewritten to the lens semantics; the ported file-growth
scenario (+59,021 files/day, file AMBER ~1.4 days out, forecast
escalation) passes unchanged, because the file lens never moved.

## 1.0.2 (v1.0.2) — built and gated, 2026-11-12

The disk gate judged every node by the Muse VM's quota. Its
absolute thresholds (AMBER at 95 GB used, RED at 102 GB) were
calibrated to that one machine's staging quota — the 2026-11-08
EDQUOT failure — and ported value-for-value through every line
since, while the probe measured used bytes on whatever volume
it happened to stand on. Run on aletheia-core, the gate read
RED at 809 GB used on a 3.6 TB volume with 2.7 TB free and held
every placement on a healthy machine.

Now the gate measures the volume it stands on. The probe returns
the volume's total/used/free bytes (statvfs, as before, but the
probe no longer carries verdicts); the gate reads through two
lenses — a volume lens (AMBER under 25% free, RED under 10%
free) and a quota lens (the old absolute ceiling, on only where
a node configures it, because the VM's quota is a real ceiling
on that machine and nowhere else) — plus the file lens, which
now counts files under a named staging root and says so. Every
threshold lives in the node's config record in state.phext,
seeded visibly by `qexo config`: volume_lens,
disk_gate_free_amber_pct, disk_gate_free_red_pct, quota_lens,
quota_amber_gb, quota_red_gb, staging_root, file_amber,
file_red. Absent keys mean the defaults (volume lens only).

The plan renders the volume in GiB — the units df speaks,
labeled — names the measured root and the counted root, and
states which lens fired and the numbers it fired on. The HOLD
copy names the measured root instead of "the VM" wherever the
binary runs. The level recorded with each telemetry observation
is the gate's own evaluation under the node's configuration;
the old mechanism of an external watcher's level word forcing
the gate is retired (the series is qexo's own now). Forecast
gate behavior is unchanged. Gates: the decisive trio as unit
tests (ranch volume GREEN at the incident's own numbers; VM
ceiling AMBER/RED by configuration alone; a 5%-free volume RED
regardless of absolute size), the ported battery, cargo test,
release build, wiring check — all on aletheia-core. Stacked on
the v1.0.1 doctor slice (PR #9); the base retargets to exo when
that merges.

## 1.0.1 (v1.0.1) — built and gated, 2026-11-12

A Windows correctness fix for doctor, in two parts. The doctor
test encoded unix permission semantics as if they were
universal: it seeded the keyring at mode 0755 and asserted a
repair to 0700 — on Windows there are no permission modes to
read or set, so the test could never pass there. And behind the
test sat a real bug: lock liveness was judged by /proc/{pid},
which never exists on Windows, so a lock held by a *live* qexo
process would have been judged stale and doctor --fix would
have deleted it.

Now: liveness is probed per platform (unix /proc; Windows
OpenProcess + GetExitCodeProcess via kernel32, std-only), and
a pid whose liveness cannot be judged reads as alive — doctor
never deletes a lock it cannot judge. The keyring check on
non-unix platforms reports itself present and ok, with the
detail that permission modes are a unix concept and do not
apply there; the repair path claims nothing it cannot do.
The test asserts the platform-neutral repairs (missing
directory, stale lock) everywhere and the mode repair on unix
only, and asserts the not-applicable keyring finding exists
off-unix. No behavior change on unix. Gates: cargo test on
the ranch (unix), release build, wiring check, and a clean
`cargo check --tests` for the x86_64-pc-windows-gnu target;
Windows runtime execution is verified on Will's machine, not
the ranch.

## 1.0.0 (v1.0) — built and gated, 2026-11-12

A from-scratch line. The old line (0.x, in git history) kept its
node state in JSON documents and JSONL ledgers; JSON was the wrong
substrate for a phext-native tool, so this line replaces it rather
than decorating it: all node state lives in one phext file,
/var/lib/qexo/state.phext, and there is no JSON anywhere in qexo's
own machinery — the one foreign edge that speaks it (Ollama) is a
translation module, isolated and named.

There are no deployments of the old line, so nothing migrates:
state is born at `qexo config`, empty, with the neighborhood map
laid down.

Built in two passes: the map-free parts first (format, seal,
hash, GPG mechanics, the Ollama edge, the plan's pure core,
doctor's filesystem layer, the CLI, the wiring gate) with the
state commands held, then the addressing layer and the state
commands under the corrected Layer 2 map (docs/LAYER2.md v2 —
the first map was withdrawn the day it was issued because it
treated coordinates as a key-space instead of places). One
format fix landed on the way: the phext serializer now walks
the dead-reckoning path in delimiter stacks and the parser
holds the scroll-existence rule — the single-delimiter form
could not represent the homeland addresses at all. Gates:
29/29 cargo tests, 59/59 CLI battery, wiring ok, json confined
to the inference edge and one rule-stating comment.

