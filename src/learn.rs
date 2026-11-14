//! learn — spaced repetition over qexo's own surface.
//!
//! The curriculum deck is phext-native: one card record per
//! scroll in the lessons & learning homeland (10.10.10/10.10.10/
//! 10.10.10, the tens — Layer 2's pattern extended by one), the
//! deck run at cluster 2, one Book per card in curriculum order.
//! A card's record carries its front and back as field lines and
//! its scheduling fields — due (epoch day), interval_days, ease,
//! reps, lapses — as field lines beside them, updated IN PLACE
//! as the operator practices. Progress survives restarts because
//! progress IS state.phext records.
//!
//! The deck itself ships with the binary: the card table below
//! is compiled in (the corpus bake's pattern — shipped material,
//! never a runtime download), and seeding copies any card whose
//! id is not yet in state into the deck run. Seeding never
//! touches an existing card: a card already in state owns its
//! scheduling fields outright.
//!
//! Scheduling is SM-2-lite, defined exactly:
//!   again — lapses + 1, reps reset to 0, interval 1 day,
//!           ease drops 0.20 (floor 1.3), due tomorrow.
//!   good  — reps + 1; interval 1 day for a first success, else
//!           round(interval x ease); due today + interval.
//!   easy  — reps + 1, ease rises 0.15; interval 3 days for a
//!           first success, else round(interval x ease x 1.3).
//! A card is mastered when its interval reaches 21 days.
//!
//! Session discipline: on a terminal, `qexo learn` opens the
//! Learn You a qexo! wizard (v1.5.0) — a chapter map, a lesson
//! screen per chapter, and due cards as practice, graded
//! 1/2/3 and saved after every card. With piped stdin it prints
//! the --status view instead of hanging. `qexo learn --status`
//! never writes: on an unseeded deck it computes from the baked
//! table. `qexo learn --app` (v1.6.1) drives the same engine
//! over a plain-text line protocol, for front-ends that have
//! no terminal — the Android app is the first.

use crate::core;
use crate::lattice::{self, Lattice};
use crate::phext::{Coord, Phext};
use std::io::Write as _;

/// A card counts as mastered at this interval (days).
pub const MASTERY_INTERVAL_DAYS: i64 = 21;

struct CardSpec {
    id: &'static str,
    front: &'static str,
    back: &'static str,
}

/// The shipped deck, in curriculum order. Every card was checked
/// against the shipped CLI's own --help text.
const DECK: &[CardSpec] = &[
    CardSpec {
        id: "verbs-config",
        front: "What does `qexo config` do — and where does a per-user install keep its state?",
        back: "Births the node: creates the state phext and sets up the node key. The state path resolves as: QEXO_STATE when set; for a verb that writes, your per-user state whenever the system state is not writable by you; for `qexo learn`, your per-user state whenever you have one; else /var/lib/qexo/state.phext when it exists and is readable; else ~/.qexo/state.phext. A normal user births a per-user node — no sudo anywhere.",
    },
    CardSpec {
        id: "verbs-register",
        front: "How does a peer become trusted?",
        back: "`qexo register <pubkey>` — the operator registers the peer's OpenPGP public key (armored text, a file path, or - for stdin). The fingerprint and the host name are read from the key itself, never from the caller's say-so. Only registered peers are ever polled.",
    },
    CardSpec {
        id: "verbs-list",
        front: "What does `qexo list` show?",
        back: "The phextnet as a navigation tree rooted at the peers anchor 3.3.3/3.3.3/3.3.3: this node (marked) and each peer nested under its record address, shown by hostname, with its status (registered/discovered) and endpoint. `qexo list --json` gives the flat machine shape.",
    },
    CardSpec {
        id: "verbs-host",
        front: "What does `qexo host <persona>` do?",
        back: "Loads a persona into local inference (Ollama). --boot <path> supplies a boot text file; re-hosting an unhosted persona with different boot text is a local edit — the boot is replaced and the persona's revision bumps.",
    },
    CardSpec {
        id: "verbs-unhost",
        front: "How do you unload a persona?",
        back: "`qexo unhost <persona>` unloads it from local inference. Its home — record, boot text, lineage — stays in state; only the live session ends.",
    },
    CardSpec {
        id: "verbs-message",
        front: "How do you talk to a hosted persona?",
        back: "`qexo message <persona> <text...>` — one request/response exchange with a hosted persona through local inference.",
    },
    CardSpec {
        id: "verbs-status",
        front: "What does `qexo status` report?",
        back: "Node name, version, the live state path (which one resolution picked), the node key fingerprint, peers by name, hosted personas, and the doctor headline.",
    },
    CardSpec {
        id: "verbs-doctor",
        front: "What does `qexo doctor` check, and what does --fix repair?",
        back: "Both layers: the filesystem (state dir/file, keyring) and the lattice (homeland indexes exist, every record is named by its index, no index names a ghost). --fix repairs what is safely repairable — including rebuilding every index from the records, because records are truth and indexes are derived.",
    },
    CardSpec {
        id: "verbs-update",
        front: "How does `qexo update` decide whether to swap the binary?",
        back: "It polls registered peers for the trusted signer's release manifest, verifies the detached OpenPGP signature over the manifest and the binary's size + sha256 BEFORE swapping, and keeps qexo.prev. The post-check runs the new binary's doctor: fixable state gaps (a homeland index the state predates) are repaired in place by the new binary and named in the receipt; hard failures roll back. --check reports only — downloads and changes nothing.",
    },
    CardSpec {
        id: "verbs-evolve",
        front: "What happens in one `qexo evolve` pass?",
        back: "One receipted convergence cycle: (1) peer gossip — peers-of-peers recorded as discovered, which is knowledge, never trust; (2) persona sync by lineage — descendants adopted, divergences kept whole as attempt artifacts, never last-writer-wins; (3) the binary self-update, exactly `qexo update`. --check reads only.",
    },
    CardSpec {
        id: "verbs-serve",
        front: "What does `qexo serve` run?",
        back: "The phext HTTP API (default port 8484; payloads are scroll text). Peers poll it for /v1/release, /v1/scroll/<coord> and /v1/peers. It binds the config record's serve_bind address.",
    },
    CardSpec {
        id: "verbs-release",
        front: "How does a node announce a release?",
        back: "`qexo release` writes a signed release manifest — a record in the ledgers homeland with its detached OpenPGP signature on the adjacent scroll — and serves the binary from the releases dir beside the state file. Only the node whose fingerprint is a peer's configured release_signer is believed.",
    },
    CardSpec {
        id: "verbs-plan",
        front: "What does bare `qexo` (or `qexo plan`) print?",
        back: "The current plan: open tasks from the tasks source, the workload picture, and the disk gate — volume lens (free space), quota lens, and file lens over the staging root, each with thresholds from the config record.",
    },
    CardSpec {
        id: "verbs-inventory",
        front: "Where do you find every phext ecosystem product, and how to install each?",
        back: "`qexo inventory` — the baked ecosystem roster: each shipped product with what it is, its home, and its verified install and launch lines. `qexo inventory <name>` prints one product's full record; --install <name> prints the command, ready to copy (print-only).",
    },
    CardSpec {
        id: "trust-signer",
        front: "What makes a release offer acceptable?",
        back: "All of: the manifest's detached signature verifies; the verified fingerprint equals the config record's release_signer; that key stands in the local peer registry; the version is strictly newer; the bytes match the manifest's size and sha256. Anything else is a printed refusal with zero filesystem change. release_signer seeds EMPTY — trust is never a default.",
    },
    CardSpec {
        id: "trust-gpg",
        front: "What carries identity and trust on the phextnet?",
        back: "OpenPGP keys. Every node is born with its own key (keyring beside the state file, and the cryptography runs inside qexo itself — no external gpg); peers are known by fingerprint; signatures carry trust. Endpoints are only transport — a spoofed endpoint cannot forge a signature, which is why plain http:// on the tailnet is acceptable.",
    },
    CardSpec {
        id: "trust-discovery",
        front: "Is a discovered peer trusted?",
        back: "Never. Gossip records peers-of-peers with status: discovered — they are never polled, never signer-eligible, never auto-registered. Only the operator's `qexo register` promotes a peer to trusted, and a fingerprint conflict for a known name is a warning with the local record untouched.",
    },
    CardSpec {
        id: "net-shape",
        front: "What does the phextnet look like on the wire?",
        back: "Nodes serve and poll over HTTP on the tailnet, port 8484: /v1/release and its signature, /v1/scroll/<coord>, /v1/peers, /v1/doctor, /v1/plan. Every payload is phext scroll text; indexes are prose scrolls; records are fetched by coordinate from the peer's own index, never guessed.",
    },
    CardSpec {
        id: "net-names",
        front: "Why do qexo's displays show names instead of IP addresses?",
        back: "Name every tailnet address. A peer record carries host: — read from its key's user id at register — and displays show hostnames, with endpoints shown by name too; a bare IP appears only as fallback when no name is known. Fleet convention: a peer's name IS its tailnet hostname.",
    },
    CardSpec {
        id: "state-one-file",
        front: "Where does qexo keep its state?",
        back: "In ONE phext file — state.phext — every scrap of it: metadata, keys, personas, plan, ledgers, sessions. There is no other state format and no JSON in qexo's own machinery; JSON appears only at foreign boundaries (Ollama's API, --json views).",
    },
    CardSpec {
        id: "state-homelands",
        front: "Name the state homelands and their anchors.",
        back: "1.1.1 home & identity, 2.2.2 keys & trust, 3.3.3 peers, 4.4.4 personas, 5.5.5 plan, 6.6.6 ledgers & receipts, 7.7.7 mailbox & sessions, 8.8.8 telemetry & forecasts, 9.9.9 config & wiring — and 10.10.10 lessons & learning, the tens, where this deck lives. Addresses are places chosen for meaning, always full 9D.",
    },
    CardSpec {
        id: "state-indexes",
        front: "If an index scroll and a record disagree, which wins?",
        back: "The record. Indexes are derived prose, regenerated from the records after every mutation and by `qexo doctor --fix`. Records are truth; indexes are the map — and the map is rebuilt from the territory, never the other way round.",
    },
];

fn card_addr(book: u32) -> Coord {
    lattice::at(lattice::HL_LEARN, [lattice::LEARN_DECK_C, 1, book, 1, 1, 1])
}

/// The card records present in state: (book, address, text),
/// in deck order.
fn state_cards(px: &Phext) -> Vec<(u32, Coord, String)> {
    let mut out = Vec::new();
    for b in lattice::component_values(px, &[10, 10, 10, lattice::LEARN_DECK_C, 1], 5) {
        let addr = card_addr(b);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "kind").as_deref() == Some("learn card") {
                out.push((b, addr, t.clone()));
            }
        }
    }
    out
}

/// Copy any shipped card whose id is not yet in state into the
/// deck run, due today. Existing cards are never touched — their
/// scheduling fields are the operator's progress. Returns how
/// many cards were added.
fn seed_deck(lat: &mut Lattice, today: i64) -> usize {
    let have: Vec<String> = state_cards(&lat.px)
        .iter()
        .filter_map(|(_, _, t)| lattice::field(t, "id"))
        .collect();
    let mut added = 0;
    for spec in DECK {
        if have.iter().any(|h| h == spec.id) {
            continue;
        }
        let book = lattice::next_free_book(&lat.px, [10, 10, 10, lattice::LEARN_DECK_C, 1], 1);
        let text = lattice::record(&[
            ("kind", "learn card"),
            ("id", spec.id),
            ("front", spec.front),
            ("back", spec.back),
            ("due", &today.to_string()),
            ("interval_days", "0"),
            ("ease", "2.5"),
            ("reps", "0"),
            ("lapses", "0"),
        ]);
        lat.px.insert(card_addr(book), text);
        added += 1;
    }
    added
}

// ----------------------------------------------------------------
// Scheduling (SM-2-lite) — pure, and pinned by the tests below.
// ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grade {
    Again,
    Good,
    Easy,
}

/// One scheduling step. Returns (interval_days, ease, reps,
/// lapses) after the grade; due is today + interval.
pub(crate) fn schedule(interval: i64, ease: f64, reps: i64, lapses: i64, grade: Grade) -> (i64, f64, i64, i64) {
    match grade {
        Grade::Again => (1, (ease - 0.20).max(1.3), 0, lapses + 1),
        Grade::Good => {
            let next = if reps == 0 { 1 } else { ((interval as f64) * ease).round() as i64 };
            (next.max(1), ease, reps + 1, lapses)
        }
        Grade::Easy => {
            let next = if reps == 0 { 3 } else { ((interval as f64) * ease * 1.3).round() as i64 };
            (next.max(1), ease + 0.15, reps + 1, lapses)
        }
    }
}

/// Ease as stored: two decimals at most, no trailing zeros
/// ("2.5", "2.3", "2.65") — the record stays human-legible.
fn fmt_ease(ease: f64) -> String {
    let s = format!("{:.2}", ease);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn field_i64(text: &str, key: &str) -> i64 {
    lattice::field(text, key).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

/// Apply one grade to a card record, in place (field lines).
fn apply_grade(text: &str, grade: Grade, today: i64) -> String {
    let (interval, ease, reps, lapses) = schedule(
        field_i64(text, "interval_days"),
        lattice::field(text, "ease").and_then(|v| v.trim().parse().ok()).unwrap_or(2.5),
        field_i64(text, "reps"),
        field_i64(text, "lapses"),
        grade,
    );
    let mut out = lattice::set_field(text, "interval_days", &interval.to_string());
    out = lattice::set_field(&out, "ease", &fmt_ease(ease));
    out = lattice::set_field(&out, "reps", &reps.to_string());
    out = lattice::set_field(&out, "lapses", &lapses.to_string());
    out = lattice::set_field(&out, "due", &(today + interval).to_string());
    out
}

// ----------------------------------------------------------------
// Deck status
// ----------------------------------------------------------------

struct DeckStats {
    total: usize,
    due_now: usize,
    mastered: usize,
    /// (day, count) of the soonest future due date, when any.
    next_due: Option<(i64, usize)>,
    seeded: bool,
}

fn deck_stats(lat: &Lattice, today: i64) -> DeckStats {
    let cards = state_cards(&lat.px);
    if cards.is_empty() {
        // Unseeded: the baked deck, every card new and due now.
        return DeckStats {
            total: DECK.len(),
            due_now: DECK.len(),
            mastered: 0,
            next_due: None,
            seeded: false,
        };
    }
    let mut due_now = 0;
    let mut mastered = 0;
    let mut future: Vec<i64> = Vec::new();
    for (_, _, t) in &cards {
        let due = field_i64(t, "due");
        if due <= today {
            due_now += 1;
        } else {
            future.push(due);
        }
        if field_i64(t, "interval_days") >= MASTERY_INTERVAL_DAYS {
            mastered += 1;
        }
    }
    let next_due = future.iter().min().map(|d| (*d, future.iter().filter(|x| *x == d).count()));
    DeckStats { total: cards.len(), due_now, mastered, next_due, seeded: true }
}

fn print_status(lat: &Lattice, today: i64) {
    let st = deck_stats(lat, today);
    println!(
        "qexo learn — deck: {} cards{}",
        st.total,
        if st.seeded {
            format!(" (in state, homeland {})", crate::phext::fmt_coord(&lattice::index_addr(lattice::HL_LEARN)))
        } else {
            " (baked with the binary; seeds into state on the first session)".to_string()
        }
    );
    println!("due now: {}", st.due_now);
    println!("mastered: {} (interval >= {} days)", st.mastered, MASTERY_INTERVAL_DAYS);
    if let Some((day, n)) = st.next_due {
        let (y, m, d) = core::civil_from_days(day);
        println!("next due: {:04}-{:02}-{:02} ({} card(s))", y, m, d, n);
    }
    println!("state: {}", lat.path().display());
}

// ----------------------------------------------------------------
// The session
// ----------------------------------------------------------------

/// Cards due today or earlier, ordered by due day, then deck
/// order — the stalest practice first.
fn due_cards(lat: &Lattice, today: i64) -> Vec<(u32, Coord, String)> {
    let mut due: Vec<(i64, u32, Coord, String)> = state_cards(&lat.px)
        .into_iter()
        .map(|(b, a, t)| (field_i64(&t, "due"), b, a, t))
        .filter(|(d, _, _, _)| *d <= today)
        .collect();
    due.sort_by_key(|(d, b, _, _)| (*d, *b));
    due.into_iter().map(|(_, b, a, t)| (b, a, t)).collect()
}

fn read_line(stdin: &std::io::Stdin) -> Option<String> {
    let mut s = String::new();
    match stdin.read_line(&mut s) {
        Ok(0) => None, // EOF
        Ok(_) => Some(s),
        Err(_) => None,
    }
}

// ----------------------------------------------------------------
// The wizard (v1.5.0) — Learn You a qexo!
//
// The engine above is unchanged: same deck, same SM-2-lite,
// same records. What v1.5.0 adds is the guided surface Will
// asked for: a chapter map of qexo's capabilities, a short
// lesson screen per chapter, and the due-card queue interleaved
// as practice — with the progress header (mastered, due,
// streak, next due) on every screen.
//
// Keyboard-first: one key per action, from anywhere —
//   space/enter  advance / reveal
//   1 2 3        grade again / good / easy (a / g / e also work)
//   b            back to the chapter map
//   d            the due queue (from the map)
//   q            quit — progress is saved after every card
//
// Phone-terminal-first: every rendered line is wrapped to the
// terminal width, detected at start; an unknown width falls
// back to 40 columns, the Termux shape. On Unix the wizard
// runs in cbreak mode (termios, std-only — no new dependency)
// so keys land one at a time. Where raw mode is unavailable
// (Windows), the same screens run in line mode: Enter advances,
// grades are typed. Non-TTY stdin never reaches the wizard at
// all — it gets the status view, exactly as before.
// ----------------------------------------------------------------

struct Chapter {
    title: &'static str,
    lesson: &'static [&'static str],
    cards: &'static [&'static str],
}

/// The curriculum as chapters. Every deck card stands in
/// exactly one chapter (a test pins the partition).
const CHAPTERS: &[Chapter] = &[
    Chapter {
        title: "First steps",
        lesson: &[
            "A node is born, not installed.",
            "`qexo config` births your node: one state file, one OpenPGP key of its own.",
            "Trust is handed out on purpose — `qexo register` is how a peer earns it.",
            "`qexo list` draws the phextnet as a tree, so you can see where everyone lives.",
        ],
        cards: &["verbs-config", "verbs-register", "verbs-list"],
    },
    Chapter {
        title: "Personas",
        lesson: &[
            "A persona is a guest your node hosts in local inference.",
            "Hosting loads it; unhosting ends the visit — its home in state stays put.",
            "`qexo message` is one request, one response. Short and honest.",
        ],
        cards: &["verbs-host", "verbs-unhost", "verbs-message"],
    },
    Chapter {
        title: "How is the node?",
        lesson: &[
            "`qexo status` is the glance: name, key, peers, personas, health.",
            "`qexo doctor` checks the node and repairs what is safely repairable.",
            "Bare `qexo` prints the plan — the work picture and the disk gate.",
        ],
        cards: &["verbs-status", "verbs-doctor", "verbs-plan"],
    },
    Chapter {
        title: "The net provides",
        lesson: &[
            "Nodes serve and poll over the phextnet: plain HTTP, phext payloads.",
            "A release is a signed scroll; an update is believed only after every check passes.",
            "`qexo evolve` is one full convergence pass — and it writes a receipt for everything it did.",
        ],
        cards: &["verbs-serve", "verbs-release", "verbs-update", "verbs-evolve"],
    },
    Chapter {
        title: "Trust is a signature",
        lesson: &[
            "Discovery is knowledge, never trust.",
            "A fingerprint is a name you can verify; an endpoint is only a road.",
            "The release signer starts EMPTY — trust is never a default.",
        ],
        cards: &["trust-signer", "trust-gpg", "trust-discovery"],
    },
    Chapter {
        title: "One file to hold it all",
        lesson: &[
            "Everything qexo knows lives in one phext file: state.phext.",
            "Its homelands are addresses chosen for meaning — 1.1.1 home, 3.3.3 peers, 10.10.10 lessons.",
            "Records are truth; indexes are the map, rebuilt from the records.",
        ],
        cards: &["state-one-file", "state-homelands", "state-indexes"],
    },
    Chapter {
        title: "The whole shelf",
        lesson: &[
            "`qexo inventory` is the ecosystem roster — every shipped phext tool, and how to install it.",
            "On the wire it is scrolls all the way down: fetched by coordinate, never guessed.",
            "And we name our addresses: hostnames on display, bare IPs only when no name is known.",
        ],
        cards: &["verbs-inventory", "net-shape", "net-names"],
    },
];

fn chapter_of(id: &str) -> Option<usize> {
    CHAPTERS.iter().position(|c| c.cards.contains(&id))
}

/// Greedy word wrap at `width` CHARACTERS (never bytes — the
/// deck speaks UTF-8). A word longer than the line is split.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let wlen = word.chars().count();
        if wlen > width {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            let mut rest: Vec<char> = word.chars().collect();
            while rest.len() > width {
                lines.push(rest.drain(..width).collect());
            }
            cur = rest.into_iter().collect();
            continue;
        }
        let clen = cur.chars().count();
        if clen == 0 {
            cur = word.to_string();
        } else if clen + 1 + wlen <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Terminal width in columns: the live size when the OS will
/// say, capped for readability, floored for sanity — and 40,
/// the phone shape, when it won't say.
fn term_width() -> usize {
    #[cfg(unix)]
    {
        // TIOCGWINSZ on stdout; winsize is four u16s (row, col, ..).
        extern "C" {
            fn ioctl(fd: i32, request: i64, arg: *mut u16) -> i32;
        }
        let mut ws = [0u16; 4];
        let rc = unsafe { ioctl(1, 0x5413, ws.as_mut_ptr()) };
        if rc == 0 && ws[1] >= 20 {
            return (ws[1] as usize).min(76);
        }
    }
    40
}

// Cbreak mode on Unix, std-only: termios is reached through a
// byte buffer because the flag word sits at the same offset
// (12) in the Linux and Android termios layouts, which are the
// layouts qexo ships for. ICANON/ECHO/ISIG are cleared so keys
// arrive one at a time and Ctrl-C arrives as a byte the wizard
// treats as quit; VMIN=1, VTIME=0. Restore happens on Drop, so
// no exit path can strand the operator's terminal.
#[cfg(unix)]
struct RawMode {
    saved: [u8; 128],
}

#[cfg(unix)]
impl RawMode {
    fn enter() -> Option<RawMode> {
        extern "C" {
            fn tcgetattr(fd: i32, termios: *mut u8) -> i32;
            fn tcsetattr(fd: i32, action: i32, termios: *const u8) -> i32;
        }
        const ICANON: u32 = 0x0002;
        const ECHO: u32 = 0x0008;
        const ISIG: u32 = 0x0001;
        let mut buf = [0u8; 128];
        if unsafe { tcgetattr(0, buf.as_mut_ptr()) } != 0 {
            return None;
        }
        let saved = buf;
        let mut lflag = u32::from_ne_bytes(buf[12..16].try_into().unwrap());
        lflag &= !(ICANON | ECHO | ISIG);
        buf[12..16].copy_from_slice(&lflag.to_ne_bytes());
        buf[17 + 6] = 1; // VMIN
        buf[17 + 5] = 0; // VTIME
        if unsafe { tcsetattr(0, 0, buf.as_ptr()) } != 0 {
            return None;
        }
        Some(RawMode { saved })
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        extern "C" {
            fn tcsetattr(fd: i32, action: i32, termios: *const u8) -> i32;
        }
        unsafe {
            tcsetattr(0, 0, self.saved.as_ptr());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Input {
    Advance,
    Key(u8),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Flow {
    Running,
    /// Render the final screen once, then exit (quit / EOF).
    ShowFinal,
    /// A key landed on the final screen: just exit.
    ExitNow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Map,
    Lesson(usize),
    Card,
    Done,
}

struct Wizard {
    screen: Screen,
    queue: Vec<(u32, Coord, String)>,
    queue_chapter: Option<usize>,
    pos: usize,
    revealed: bool,
    note: Option<String>,
    reviewed: usize,
    again: usize,
    good: usize,
    easy: usize,
    streak: usize,
    best_streak: usize,
}

impl Wizard {
    fn new() -> Wizard {
        Wizard {
            screen: Screen::Map,
            queue: Vec::new(),
            queue_chapter: None,
            pos: 0,
            revealed: false,
            note: None,
            reviewed: 0,
            again: 0,
            good: 0,
            easy: 0,
            streak: 0,
            best_streak: 0,
        }
    }

    fn start_queue(&mut self, queue: Vec<(u32, Coord, String)>, chapter: Option<usize>) {
        self.queue = queue;
        self.queue_chapter = chapter;
        self.pos = 0;
        self.revealed = false;
        self.screen = Screen::Card;
    }

    fn grade(
        &mut self,
        lat: &mut Lattice,
        today: i64,
        grade: Grade,
    ) -> Result<Flow, String> {
        let (_, addr, text) = &self.queue[self.pos];
        let updated = apply_grade(text, grade, today);
        lat.px.insert(*addr, updated);
        lat.save().map_err(|e| e.to_string())?;
        match grade {
            Grade::Again => {
                self.again += 1;
                self.streak = 0;
            }
            Grade::Good => {
                self.good += 1;
                self.streak += 1;
            }
            Grade::Easy => {
                self.easy += 1;
                self.streak += 1;
            }
        }
        self.best_streak = self.best_streak.max(self.streak);
        self.reviewed += 1;
        self.pos += 1;
        self.revealed = false;
        self.note = None;
        if self.pos >= self.queue.len() {
            self.screen = Screen::Done;
        }
        Ok(Flow::Running)
    }

    fn handle(&mut self, input: Input, lat: &mut Lattice, today: i64) -> Result<Flow, String> {
        if input == Input::Eof {
            self.screen = Screen::Done;
            return Ok(Flow::ShowFinal);
        }
        if let Input::Key(b'q') = input {
            self.screen = Screen::Done;
            return Ok(Flow::ShowFinal);
        }
        self.note = None;
        match self.screen {
            Screen::Done => Ok(Flow::ExitNow),
            Screen::Map => {
                match input {
                    Input::Key(b'd') => {
                        let due = due_cards(lat, today);
                        if due.is_empty() {
                            self.note =
                                Some("nothing due — the deck is ahead of you.".to_string());
                        } else {
                            self.start_queue(due, None);
                        }
                    }
                    Input::Key(k) if (b'1'..=b'9').contains(&k) => {
                        let idx = (k - b'1') as usize;
                        if idx < CHAPTERS.len() {
                            self.screen = Screen::Lesson(idx);
                        }
                    }
                    _ => {}
                }
                Ok(Flow::Running)
            }
            Screen::Lesson(ch) => {
                match input {
                    Input::Advance => {
                        let queue: Vec<(u32, Coord, String)> = due_cards(lat, today)
                            .into_iter()
                            .filter(|(_, _, t)| {
                                lattice::field(t, "id").and_then(|id| chapter_of(&id)) == Some(ch)
                            })
                            .collect();
                        if queue.is_empty() {
                            self.note =
                                Some("nothing due in this chapter — the queue remembers.".to_string());
                        } else {
                            self.start_queue(queue, Some(ch));
                        }
                    }
                    Input::Key(b'b') | Input::Key(b'm') => self.screen = Screen::Map,
                    _ => {}
                }
                Ok(Flow::Running)
            }
            Screen::Card => {
                if !self.revealed {
                    match input {
                        Input::Advance => self.revealed = true,
                        Input::Key(b'b') | Input::Key(b'm') => self.screen = Screen::Map,
                        Input::Key(_) => {
                            self.note = Some("reveal the back first (space).".to_string())
                        }
                        Input::Eof => unreachable!(),
                    }
                    return Ok(Flow::Running);
                }
                match input {
                    Input::Key(b'1') | Input::Key(b'a') => self.grade(lat, today, Grade::Again),
                    Input::Key(b'2') | Input::Key(b'g') => self.grade(lat, today, Grade::Good),
                    Input::Key(b'3') | Input::Key(b'e') => self.grade(lat, today, Grade::Easy),
                    Input::Key(b'b') | Input::Key(b'm') => {
                        self.screen = Screen::Map;
                        Ok(Flow::Running)
                    }
                    Input::Advance => {
                        self.note = Some("grade it: 1 again, 2 good, 3 easy.".to_string());
                        Ok(Flow::Running)
                    }
                    _ => Ok(Flow::Running),
                }
            }
        }
    }

    // -- rendering: every line wrapped to `width` -------------

    fn header(&self, lat: &Lattice, today: i64) -> Vec<String> {
        let st = deck_stats(lat, today);
        let mut out = vec![
            "Learn You a qexo!".to_string(),
            format!(
                "mastered {}/{} | due {} | streak {}",
                st.mastered, st.total, st.due_now, self.streak
            ),
        ];
        if let Some((day, n)) = st.next_due {
            let (y, m, d) = core::civil_from_days(day);
            out.push(format!("next due: {:04}-{:02}-{:02} ({} card(s))", y, m, d, n));
        }
        out
    }

    fn chapter_counts(lat: &Lattice, today: i64, ch: usize) -> (usize, usize, usize) {
        let ids = CHAPTERS[ch].cards;
        let cards = state_cards(&lat.px);
        if cards.is_empty() {
            return (ids.len(), ids.len(), 0);
        }
        let (mut due, mut mastered) = (0usize, 0usize);
        for (_, _, t) in &cards {
            let id = lattice::field(t, "id").unwrap_or_default();
            if chapter_of(&id) != Some(ch) {
                continue;
            }
            if field_i64(t, "due") <= today {
                due += 1;
            }
            if field_i64(t, "interval_days") >= MASTERY_INTERVAL_DAYS {
                mastered += 1;
            }
        }
        (ids.len(), due, mastered)
    }

    fn render(&self, lat: &Lattice, today: i64, width: usize) -> String {
        let mut lines: Vec<String> = Vec::new();
        let push = |lines: &mut Vec<String>, text: &str| {
            for l in wrap(text, width) {
                lines.push(l);
            }
        };
        for l in self.header(lat, today) {
            push(&mut lines, &l);
        }
        lines.push(String::new());
        match self.screen {
            Screen::Map => {
                push(&mut lines, "Chapters — pick a number:");
                for (i, ch) in CHAPTERS.iter().enumerate() {
                    let (total, due, mastered) = Wizard::chapter_counts(lat, today, i);
                    push(
                        &mut lines,
                        &format!(
                            "{}) {} — {}/{} mastered, {} due",
                            i + 1,
                            ch.title,
                            mastered,
                            total,
                            due
                        ),
                    );
                }
                let st = deck_stats(lat, today);
                push(&mut lines, &format!("d) Due queue — {} due", st.due_now));
                push(&mut lines, "q) quit");
            }
            Screen::Lesson(ch) => {
                let chapter = &CHAPTERS[ch];
                push(&mut lines, &format!("Chapter {} — {}", ch + 1, chapter.title));
                lines.push(String::new());
                for l in chapter.lesson {
                    push(&mut lines, l);
                }
                let (total, due, _) = Wizard::chapter_counts(lat, today, ch);
                lines.push(String::new());
                push(
                    &mut lines,
                    &format!("Cards here: {} ({} due).", total, due),
                );
                push(&mut lines, "[space] practice · [b] map · [q] quit");
            }
            Screen::Card => {
                let (_, _, text) = &self.queue[self.pos];
                let ctx = match self.queue_chapter {
                    Some(ch) => format!(
                        "Chapter {} — {} · card {}/{}",
                        ch + 1,
                        CHAPTERS[ch].title,
                        self.pos + 1,
                        self.queue.len()
                    ),
                    None => format!("Due queue · card {}/{}", self.pos + 1, self.queue.len()),
                };
                push(&mut lines, &ctx);
                lines.push(String::new());
                push(&mut lines, "Q:");
                for l in wrap(&lattice::field(text, "front").unwrap_or_default(), width - 2) {
                    lines.push(format!("  {}", l));
                }
                if self.revealed {
                    lines.push(String::new());
                    push(&mut lines, "A:");
                    for l in wrap(&lattice::field(text, "back").unwrap_or_default(), width - 2) {
                        lines.push(format!("  {}", l));
                    }
                    lines.push(String::new());
                    push(&mut lines, "grade: [1] again [2] good [3] easy");
                    push(&mut lines, "[b] map · [q] quit");
                } else {
                    lines.push(String::new());
                    push(&mut lines, "[space] reveal · [b] map · [q] quit");
                }
            }
            Screen::Done => {
                let st = deck_stats(lat, today);
                push(
                    &mut lines,
                    &format!(
                        "session: {} reviewed — {} again, {} good, {} easy; {} still due",
                        self.reviewed, self.again, self.good, self.easy, st.due_now
                    ),
                );
                push(&mut lines, &format!("best streak: {}", self.best_streak));
                push(
                    &mut lines,
                    &format!("mastered now: {}/{}", st.mastered, st.total),
                );
                lines.push(String::new());
                push(&mut lines, "the deck remembers. (any key to leave)");
            }
        }
        if let Some(note) = &self.note {
            lines.push(String::new());
            push(&mut lines, &format!("! {}", note));
        }
        lines.join("\n")
    }
}

/// One key from a raw terminal: a byte at a time. Enter and
/// space both advance; Esc and Ctrl-C quit; Ctrl-D is EOF.
#[cfg(unix)]
fn read_key_raw() -> Input {
    use std::io::Read as _;
    let mut b = [0u8; 1];
    match std::io::stdin().read(&mut b) {
        Ok(0) => Input::Eof,
        Ok(_) => match b[0] {
            b'\n' | b'\r' | b' ' => Input::Advance,
            0x1b | 0x03 => Input::Key(b'q'),
            0x04 => Input::Eof,
            k => Input::Key(k),
        },
        Err(_) => Input::Eof,
    }
}

/// One key in line mode (Windows, or raw mode unavailable):
/// a typed line — Enter advances; words work too (again /
/// good / easy / map / due / quit); anything else offers its
/// first character as the key.
fn read_key_line(stdin: &std::io::Stdin) -> Input {
    match read_line(stdin) {
        None => Input::Eof,
        Some(s) => {
            let w = s.trim().to_lowercase();
            match w.as_str() {
                "" => Input::Advance,
                "again" => Input::Key(b'a'),
                "good" => Input::Key(b'g'),
                "easy" => Input::Key(b'e'),
                "quit" => Input::Key(b'q'),
                "map" | "back" => Input::Key(b'b'),
                "due" => Input::Key(b'd'),
                _ => Input::Key(w.as_bytes()[0]),
            }
        }
    }
}

fn run_wizard(lat: &mut Lattice, today: i64) -> i32 {
    let width = term_width();
    #[cfg(unix)]
    let raw = RawMode::enter();
    #[cfg(not(unix))]
    let raw: Option<()> = None;
    let stdin = std::io::stdin();
    let mut wiz = Wizard::new();
    loop {
        if raw.is_some() {
            print!("\x1b[2J\x1b[H");
        }
        println!("{}", wiz.render(lat, today, width));
        if raw.is_none() {
            print!("> ");
            let _ = std::io::stdout().flush();
        }
        let input = {
            #[cfg(unix)]
            {
                if raw.is_some() {
                    read_key_raw()
                } else {
                    read_key_line(&stdin)
                }
            }
            #[cfg(not(unix))]
            {
                read_key_line(&stdin)
            }
        };
        match wiz.handle(input, lat, today) {
            Ok(Flow::Running) => continue,
            Ok(Flow::ShowFinal) => {
                if raw.is_some() {
                    print!("\x1b[2J\x1b[H");
                }
                println!("{}", wiz.render(lat, today, width));
                break;
            }
            Ok(Flow::ExitNow) => break,
            Err(e) => {
                eprintln!("qexo learn: cannot save state: {}", e);
                return 1;
            }
        }
    }
    0
}

/// `qexo learn [--status]`.
pub fn learn(status_only: bool) -> i32 {
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let (today, _) = core::now_stamp();
    if status_only {
        print_status(&lat, today);
        return 0;
    }
    // No terminal, no session: a piped invocation gets the
    // status view, never a hang on a read that cannot be answered.
    use std::io::IsTerminal as _;
    if !std::io::stdin().is_terminal() {
        print_status(&lat, today);
        return 0;
    }
    let added = seed_deck(&mut lat, today);
    if added > 0 {
        lattice::regen_indexes(&mut lat.px);
        if let Err(e) = lat.save() {
            eprintln!("qexo learn: cannot save state: {}", e);
            return 1;
        }
    }
    run_wizard(&mut lat, today)
}

// ----------------------------------------------------------------
// The app protocol (v1.6.1) — the wizard's engine, over a pipe.
//
// The wizard needs a terminal; an app has none. `qexo learn
// --app` speaks the smallest line protocol over the SAME engine:
// one command per line on stdin, one `key: value` block per
// command on stdout, every block closed by `end: <command>`.
// Not JSON — qexo's own front-ends are not a foreign boundary.
//
//   status              deck + session counters
//   chapters            the chapter map, one `chapter:` line each
//   lesson <n>          chapter n's lesson lines
//   next                start the due queue, or re-show its card
//   practice <n>        start chapter n's due queue
//   reveal              the current card's back
//   grade again|good|easy
//                       grade + save, exactly as the wizard does
//   quit                session summary, then exit
//
// The session IS the wizard's state machine — Wizard,
// deck_stats, due_cards, Wizard::grade: nothing about the deck,
// the chapters, SM-2-lite, or the state records is duplicated
// or altered, and a grade lands in state.phext identically
// whichever surface drove it. `status` here is a session view
// (the deck seeds at session start, as the wizard seeds);
// `learn --status` remains the never-writes form.
// ----------------------------------------------------------------

struct AppSession {
    wiz: Wizard,
}

impl AppSession {
    fn status_lines(&self, lat: &Lattice, today: i64) -> Vec<String> {
        let st = deck_stats(lat, today);
        let mut out = vec![
            format!("total: {}", st.total),
            format!("due: {}", st.due_now),
            format!("mastered: {}", st.mastered),
            format!("streak: {}", self.wiz.streak),
            format!("reviewed: {}", self.wiz.reviewed),
        ];
        match st.next_due {
            Some((day, n)) => {
                let (y, m, d) = core::civil_from_days(day);
                out.push(format!("next_due: {:04}-{:02}-{:02}", y, m, d));
                out.push(format!("next_due_cards: {}", n));
            }
            None => out.push("next_due: none".to_string()),
        }
        out
    }

    fn card_lines(&self) -> Vec<String> {
        let (_, _, text) = &self.wiz.queue[self.wiz.pos];
        let context = match self.wiz.queue_chapter {
            Some(ch) => format!("Chapter {} — {}", ch + 1, CHAPTERS[ch].title),
            None => "Due queue".to_string(),
        };
        vec![
            format!("context: {}", context),
            format!("position: {} of {}", self.wiz.pos + 1, self.wiz.queue.len()),
            format!("card: {}", lattice::field(text, "id").unwrap_or_default()),
            format!("front: {}", lattice::field(text, "front").unwrap_or_default()),
            format!("revealed: {}", if self.wiz.revealed { "yes" } else { "no" }),
        ]
    }

    /// Start a practice queue (the whole due queue, or one
    /// chapter's slice of it); returns the first card's block,
    /// or the empty-queue note the wizard would show.
    fn start(&mut self, lat: &Lattice, today: i64, chapter: Option<usize>) -> Vec<String> {
        let queue = match chapter {
            Some(ch) => due_cards(lat, today)
                .into_iter()
                .filter(|(_, _, t)| {
                    lattice::field(t, "id").and_then(|id| chapter_of(&id)) == Some(ch)
                })
                .collect(),
            None => due_cards(lat, today),
        };
        if queue.is_empty() {
            return vec![
                "queue: empty".to_string(),
                format!(
                    "note: {}",
                    match chapter {
                        Some(_) => "nothing due in this chapter — the queue remembers.",
                        None => "nothing due — the deck is ahead of you.",
                    }
                ),
            ];
        }
        self.wiz.start_queue(queue, chapter);
        self.card_lines()
    }

    /// One command line → (response lines, session over?).
    fn handle(&mut self, lat: &mut Lattice, today: i64, line: &str) -> (Vec<String>, bool) {
        let mut words = line.split_whitespace();
        match words.next().unwrap_or("") {
            "status" => (self.status_lines(lat, today), false),
            "chapters" => {
                let mut out = Vec::new();
                for (i, ch) in CHAPTERS.iter().enumerate() {
                    let (total, due, mastered) = Wizard::chapter_counts(lat, today, i);
                    out.push(format!(
                        "chapter: {} | {} | {} cards | {} due | {} mastered",
                        i + 1,
                        ch.title,
                        total,
                        due,
                        mastered
                    ));
                }
                (out, false)
            }
            "lesson" => {
                let n: Option<usize> = words.next().and_then(|w| w.parse().ok());
                match n {
                    Some(n) if (1..=CHAPTERS.len()).contains(&n) => {
                        let ch = &CHAPTERS[n - 1];
                        let (total, due, _) = Wizard::chapter_counts(lat, today, n - 1);
                        let mut out =
                            vec![format!("chapter: {}", n), format!("title: {}", ch.title)];
                        for l in ch.lesson {
                            out.push(format!("lesson: {}", l));
                        }
                        out.push(format!("cards: {}", total));
                        out.push(format!("due: {}", due));
                        (out, false)
                    }
                    _ => (vec!["error: lesson needs a chapter number".to_string()], false),
                }
            }
            "next" => {
                if self.wiz.screen == Screen::Card {
                    (self.card_lines(), false)
                } else {
                    (self.start(lat, today, None), false)
                }
            }
            "practice" => {
                let n: Option<usize> = words.next().and_then(|w| w.parse().ok());
                match n {
                    Some(n) if (1..=CHAPTERS.len()).contains(&n) => {
                        (self.start(lat, today, Some(n - 1)), false)
                    }
                    _ => (vec!["error: practice needs a chapter number".to_string()], false),
                }
            }
            "reveal" => {
                if self.wiz.screen != Screen::Card {
                    (vec!["error: no card on the table — next first".to_string()], false)
                } else if self.wiz.revealed {
                    (vec!["error: already revealed".to_string()], false)
                } else {
                    self.wiz.revealed = true;
                    let (_, _, text) = &self.wiz.queue[self.wiz.pos];
                    (
                        vec![format!(
                            "back: {}",
                            lattice::field(text, "back").unwrap_or_default()
                        )],
                        false,
                    )
                }
            }
            "grade" => {
                let g = match words.next() {
                    Some("again") => Some(Grade::Again),
                    Some("good") => Some(Grade::Good),
                    Some("easy") => Some(Grade::Easy),
                    _ => None,
                };
                match g {
                    None => (
                        vec!["error: grade needs again, good, or easy".to_string()],
                        false,
                    ),
                    Some(_) if self.wiz.screen != Screen::Card => {
                        (vec!["error: no card on the table — next first".to_string()], false)
                    }
                    Some(_) if !self.wiz.revealed => {
                        (vec!["error: reveal the back first".to_string()], false)
                    }
                    Some(grade) => match self.wiz.grade(lat, today, grade) {
                        Ok(_) => {
                            let mut out = vec![
                                format!(
                                    "graded: {}",
                                    match grade {
                                        Grade::Again => "again",
                                        Grade::Good => "good",
                                        Grade::Easy => "easy",
                                    }
                                ),
                                format!("remaining: {}", self.wiz.queue.len() - self.wiz.pos),
                            ];
                            out.extend(self.status_lines(lat, today));
                            (out, false)
                        }
                        Err(e) => (vec![format!("error: cannot save state: {}", e)], false),
                    },
                }
            }
            "quit" => {
                let st = deck_stats(lat, today);
                (
                    vec![
                        format!("reviewed: {}", self.wiz.reviewed),
                        format!("again: {}", self.wiz.again),
                        format!("good: {}", self.wiz.good),
                        format!("easy: {}", self.wiz.easy),
                        format!("best_streak: {}", self.wiz.best_streak),
                        format!("mastered: {}", st.mastered),
                        format!("due: {}", st.due_now),
                    ],
                    true,
                )
            }
            "" => (Vec::new(), false),
            other => (vec![format!("error: unknown command '{}'", other)], false),
        }
    }
}

/// `qexo learn --app` — the wizard's engine over stdin/stdout.
/// Exit codes match learn(): 0 clean, 1 a state save failed,
/// plus require_born's codes when the node is unborn.
pub fn learn_app() -> i32 {
    let mut lat = match core::require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let (today, _) = core::now_stamp();
    let added = seed_deck(&mut lat, today);
    if added > 0 {
        lattice::regen_indexes(&mut lat.px);
        if let Err(e) = lat.save() {
            eprintln!("qexo learn: cannot save state: {}", e);
            return 1;
        }
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut session = AppSession { wiz: Wizard::new() };
    let emit = |lines: &[String], end: &str| {
        let mut w = stdout.lock();
        for l in lines {
            let _ = writeln!(w, "{}", l);
        }
        let _ = writeln!(w, "end: {}", end);
        let _ = w.flush();
    };
    emit(&session.status_lines(&lat, today), "hello");
    loop {
        let line = match read_line(&stdin) {
            Some(l) => l,
            // EOF: the front-end hung up. Grades already saved
            // per card land exactly as the wizard's do.
            None => break,
        };
        let cmd = line.split_whitespace().next().unwrap_or("").to_string();
        if cmd.is_empty() {
            continue;
        }
        let (lines, done) = session.handle(&mut lat, today, &line);
        emit(&lines, &cmd);
        if done {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(due: i64, interval: i64, ease: &str, reps: i64, lapses: i64) -> String {
        lattice::record(&[
            ("kind", "learn card"),
            ("id", "test-card"),
            ("front", "f"),
            ("back", "b"),
            ("due", &due.to_string()),
            ("interval_days", &interval.to_string()),
            ("ease", ease),
            ("reps", &reps.to_string()),
            ("lapses", &lapses.to_string()),
        ])
    }

    #[test]
    fn good_grows_by_ease_after_the_first_day() {
        // new card: good -> 1 day
        assert_eq!(schedule(0, 2.5, 0, 0, Grade::Good), (1, 2.5, 1, 0));
        // then interval x ease each time: 1 -> 3 (2.5 rounds up) -> 8
        assert_eq!(schedule(1, 2.5, 1, 0, Grade::Good), (3, 2.5, 2, 0));
        assert_eq!(schedule(3, 2.5, 2, 0, Grade::Good), (8, 2.5, 3, 0));
    }

    #[test]
    fn easy_starts_at_three_and_raises_ease() {
        assert_eq!(schedule(0, 2.5, 0, 0, Grade::Easy), (3, 2.65, 1, 0));
        let (iv, ease, _, _) = schedule(3, 2.65, 1, 0, Grade::Easy);
        assert_eq!(iv, 10); // round(3 x 2.65 x 1.3) = round(10.335)
        assert!((ease - 2.8).abs() < 1e-9);
    }

    #[test]
    fn again_is_a_lapse_reset_to_one_day() {
        assert_eq!(schedule(21, 2.5, 6, 0, Grade::Again), (1, 2.3, 0, 1));
        // ease floors at 1.3 under repeated lapses
        let (_, e1, _, _) = schedule(1, 1.4, 0, 3, Grade::Again);
        assert!((e1 - 1.3).abs() < 1e-9);
        let (_, e2, _, _) = schedule(1, 1.3, 0, 4, Grade::Again);
        assert!((e2 - 1.3).abs() < 1e-9);
    }

    #[test]
    fn grading_a_record_updates_its_fields_in_place() {
        let t = card(100, 0, "2.5", 0, 0);
        let out = apply_grade(&t, Grade::Good, 100);
        assert_eq!(lattice::field(&out, "due").as_deref(), Some("101"));
        assert_eq!(lattice::field(&out, "interval_days").as_deref(), Some("1"));
        assert_eq!(lattice::field(&out, "reps").as_deref(), Some("1"));
        // the lapse path: due tomorrow, reps wiped, ease docked
        let out = apply_grade(&card(100, 21, "2.5", 6, 0), Grade::Again, 100);
        assert_eq!(lattice::field(&out, "due").as_deref(), Some("101"));
        assert_eq!(lattice::field(&out, "reps").as_deref(), Some("0"));
        assert_eq!(lattice::field(&out, "lapses").as_deref(), Some("1"));
        assert_eq!(lattice::field(&out, "ease").as_deref(), Some("2.3"));
        // front/back untouched by scheduling
        assert_eq!(lattice::field(&out, "front").as_deref(), Some("f"));
    }

    #[test]
    fn seeding_adds_missing_cards_and_never_touches_progress() {
        let mut lat = Lattice::born(std::path::PathBuf::from("/tmp/qexo-learn-test/state.phext"));
        let today = 20000;
        assert_eq!(seed_deck(&mut lat, today), DECK.len());
        assert_eq!(seed_deck(&mut lat, today), 0, "second seed adds nothing");
        // grade one card far into the future, then re-seed: the
        // scheduling fields are the operator's and stand.
        let cards = state_cards(&lat.px);
        let (_, addr, text) = &cards[0];
        lat.px.insert(*addr, apply_grade(text, Grade::Easy, today));
        assert_eq!(seed_deck(&mut lat, today), 0);
        let after = state_cards(&lat.px);
        assert_eq!(lattice::field(&after[0].2, "due").as_deref(), Some("20003"));
        // every seeded card starts due immediately, at default ease
        let st = deck_stats(&lat, today);
        assert_eq!(st.total, DECK.len());
        assert_eq!(st.due_now, DECK.len() - 1, "one card was graded away");
        assert_eq!(st.mastered, 0);
    }

    #[test]
    fn due_selection_orders_by_due_then_deck_order() {
        let mut lat = Lattice::born(std::path::PathBuf::from("/tmp/qexo-learn-test/state.phext"));
        lat.px.insert(card_addr(1), card(50, 1, "2.5", 1, 0));
        lat.px.insert(card_addr(2), card(10, 1, "2.5", 1, 0));
        lat.px.insert(card_addr(3), card(9999, 1, "2.5", 1, 0));
        let due = due_cards(&lat, 100);
        assert_eq!(due.len(), 2, "the future card is not due");
        assert_eq!(due[0].0, 2, "the stalest card goes first");
        assert_eq!(due[1].0, 1);
    }

    #[test]
    fn mastery_is_interval_at_least_21_days() {
        let mut lat = Lattice::born(std::path::PathBuf::from("/tmp/qexo-learn-test/state.phext"));
        lat.px.insert(card_addr(1), card(50, 21, "2.5", 5, 0));
        lat.px.insert(card_addr(2), card(50, 20, "2.5", 5, 0));
        let st = deck_stats(&lat, 100);
        assert_eq!(st.mastered, 1);
        assert!(st.seeded);
    }

    #[test]
    fn deck_covers_the_whole_surface() {
        // the curriculum names every verb and the trust model —
        // if a verb ships without a card, this test is the alarm.
        let ids: Vec<&str> = DECK.iter().map(|c| c.id).collect();
        for want in [
            "verbs-config", "verbs-register", "verbs-list", "verbs-host", "verbs-unhost", "verbs-message",
            "verbs-status", "verbs-doctor", "verbs-update", "verbs-evolve", "verbs-serve", "verbs-release",
            "verbs-plan", "verbs-inventory", "trust-signer", "trust-gpg", "trust-discovery",
        ] {
            assert!(ids.contains(&want), "deck is missing {}", want);
        }
        assert!(DECK.len() >= 20);
        for c in DECK {
            assert!(!c.front.is_empty() && !c.back.is_empty(), "card {} is blank", c.id);
        }
    }
}

#[cfg(test)]
mod wizard_tests {
    use super::*;

    fn born_seeded() -> (Lattice, i64) {
        let mut lat = Lattice::born(std::path::PathBuf::from("/tmp/qexo-wizard-test/state.phext"));
        let today = 20000;
        seed_deck(&mut lat, today);
        lattice::regen_indexes(&mut lat.px);
        (lat, today)
    }

    #[test]
    fn chapters_partition_the_deck_exactly_once() {
        let mut seen: Vec<&str> = Vec::new();
        for ch in CHAPTERS {
            for id in ch.cards {
                assert!(!seen.contains(id), "card {} stands in two chapters", id);
                seen.push(id);
            }
        }
        assert_eq!(seen.len(), DECK.len(), "every card has a chapter");
        for spec in DECK {
            assert!(seen.contains(&spec.id), "card {} has no chapter", spec.id);
            assert_eq!(chapter_of(spec.id).map(|i| CHAPTERS[i].cards.contains(&spec.id)), Some(true));
        }
        assert_eq!(CHAPTERS.len(), 7);
        for ch in CHAPTERS {
            assert!(!ch.lesson.is_empty(), "chapter {} has no lesson", ch.title);
        }
    }

    #[test]
    fn wrap_respects_width_and_splits_long_words() {
        for w in [24usize, 40, 76] {
            for spec in DECK {
                for line in wrap(spec.back, w) {
                    assert!(line.chars().count() <= w, "width {} line too long: {}", w, line);
                }
            }
            let lines = wrap("supercalifragilisticexpialidocious", 10);
            assert!(lines.iter().all(|l| l.chars().count() <= 10));
            assert_eq!(lines.concat(), "supercalifragilisticexpialidocious");
        }
        assert_eq!(wrap("a b c", 40), vec!["a b c".to_string()]);
        assert_eq!(wrap("", 40), vec!["".to_string()]);
    }

    #[test]
    fn every_screen_fits_a_phone_width() {
        let (mut lat, today) = born_seeded();
        let mut wiz = Wizard::new();
        for screen in [
            wiz.render(&lat, today, 40),
            {
                wiz.handle(Input::Key(b'1'), &mut lat, today).unwrap();
                wiz.render(&lat, today, 40)
            },
            {
                wiz.handle(Input::Advance, &mut lat, today).unwrap();
                wiz.render(&lat, today, 40)
            },
            {
                wiz.handle(Input::Advance, &mut lat, today).unwrap();
                wiz.render(&lat, today, 40)
            },
        ] {
            for line in screen.lines() {
                assert!(line.chars().count() <= 40, "line over 40 cols: {:?}", line);
            }
        }
    }

    #[test]
    fn wizard_grades_cards_and_tracks_the_streak() {
        let (mut lat, today) = born_seeded();
        let mut wiz = Wizard::new();
        // map -> due queue -> first card, unrevealed
        assert_eq!(wiz.handle(Input::Key(b'd'), &mut lat, today).unwrap(), Flow::Running);
        assert_eq!(wiz.screen, Screen::Card);
        assert!(!wiz.revealed);
        // grading before reveal is a note, not a grade
        wiz.handle(Input::Key(b'2'), &mut lat, today).unwrap();
        assert_eq!(wiz.reviewed, 0);
        assert!(wiz.note.is_some());
        // reveal, grade good: saved to state, streak 1
        wiz.handle(Input::Advance, &mut lat, today).unwrap();
        assert!(wiz.revealed);
        wiz.handle(Input::Key(b'2'), &mut lat, today).unwrap();
        assert_eq!(wiz.reviewed, 1);
        assert_eq!(wiz.streak, 1);
        let cards = state_cards(&lat.px);
        let graded = cards.iter().filter(|(_, _, t)| field_i64(t, "reps") > 0).count();
        assert_eq!(graded, 1, "the grade landed in state");
        // second card: again resets the streak
        wiz.handle(Input::Advance, &mut lat, today).unwrap();
        wiz.handle(Input::Key(b'1'), &mut lat, today).unwrap();
        assert_eq!(wiz.streak, 0);
        assert_eq!(wiz.again, 1);
        assert_eq!(wiz.best_streak, 1);
    }

    #[test]
    fn quit_from_a_card_lands_on_the_summary() {
        let (mut lat, today) = born_seeded();
        let mut wiz = Wizard::new();
        wiz.handle(Input::Key(b'd'), &mut lat, today).unwrap();
        assert_eq!(wiz.screen, Screen::Card);
        let flow = wiz.handle(Input::Key(b'q'), &mut lat, today).unwrap();
        assert_eq!(flow, Flow::ShowFinal);
        assert_eq!(wiz.screen, Screen::Done);
        let out = wiz.render(&lat, today, 40);
        assert!(out.contains("session: 0 reviewed"));
    }

    #[test]
    fn a_chapter_with_nothing_due_says_so() {
        let (mut lat, today) = born_seeded();
        // grade every chapter-1 card into the future
        for (_, addr, text) in state_cards(&lat.px) {
            let id = lattice::field(&text, "id").unwrap_or_default();
            if CHAPTERS[0].cards.contains(&id.as_str()) {
                lat.px.insert(addr, apply_grade(&text, Grade::Easy, today));
            }
        }
        let mut wiz = Wizard::new();
        wiz.handle(Input::Key(b'1'), &mut lat, today).unwrap();
        assert_eq!(wiz.screen, Screen::Lesson(0));
        wiz.handle(Input::Advance, &mut lat, today).unwrap();
        assert_eq!(wiz.screen, Screen::Lesson(0), "no practice to start");
        assert!(wiz.note.is_some());
    }

    #[test]
    fn finishing_the_queue_lands_on_done() {
        let (mut lat, today) = born_seeded();
        let mut wiz = Wizard::new();
        // lesson for chapter 2 (3 cards), practice them all easy
        wiz.handle(Input::Key(b'2'), &mut lat, today).unwrap();
        wiz.handle(Input::Advance, &mut lat, today).unwrap();
        assert_eq!(wiz.screen, Screen::Card);
        assert_eq!(wiz.queue.len(), 3);
        for _ in 0..3 {
            wiz.handle(Input::Advance, &mut lat, today).unwrap();
            wiz.handle(Input::Key(b'3'), &mut lat, today).unwrap();
        }
        assert_eq!(wiz.screen, Screen::Done);
        assert_eq!(wiz.reviewed, 3);
        assert_eq!(wiz.best_streak, 3);
        // a key on the done screen exits
        assert_eq!(wiz.handle(Input::Advance, &mut lat, today).unwrap(), Flow::ExitNow);
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;

    fn born(path: &str) -> (Lattice, i64) {
        let mut lat = Lattice::born(std::path::PathBuf::from(path));
        let today = 20000;
        seed_deck(&mut lat, today);
        lattice::regen_indexes(&mut lat.px);
        (lat, today)
    }

    fn session() -> AppSession {
        AppSession { wiz: Wizard::new() }
    }

    #[test]
    fn app_session_walks_a_card_and_the_grade_persists() {
        let (mut lat, today) = born("/tmp/qexo-app-test-a/state.phext");
        let mut s = session();
        let st = s.handle(&mut lat, today, "status").0;
        assert!(st.contains(&"total: 22".to_string()));
        assert!(st.contains(&"due: 22".to_string()));
        assert!(st.contains(&"streak: 0".to_string()));
        // the scripted session: next -> reveal -> grade -> next
        let card = s.handle(&mut lat, today, "next").0;
        assert!(card.contains(&"context: Due queue".to_string()));
        assert!(card.contains(&"position: 1 of 22".to_string()));
        assert!(card.iter().any(|l| l.starts_with("front: ")));
        assert!(card.contains(&"revealed: no".to_string()));
        // grading before reveal is refused — the wizard's rule
        let err = s.handle(&mut lat, today, "grade good").0;
        assert_eq!(err, vec!["error: reveal the back first".to_string()]);
        let back = s.handle(&mut lat, today, "reveal").0;
        assert_eq!(back.len(), 1);
        assert!(back[0].starts_with("back: "));
        let graded = s.handle(&mut lat, today, "grade good").0;
        assert!(graded.contains(&"graded: good".to_string()));
        assert!(graded.contains(&"remaining: 21".to_string()));
        assert!(graded.contains(&"due: 21".to_string()));
        assert!(graded.contains(&"streak: 1".to_string()));
        // the grade is in state.phext records, not just the session
        let cards = state_cards(&lat.px);
        assert_eq!(
            cards.iter().filter(|(_, _, t)| field_i64(t, "reps") > 0).count(),
            1,
            "exactly one card carries the grade"
        );
        // the session advances: the next card is on the table
        let card2 = s.handle(&mut lat, today, "next").0;
        assert!(card2.contains(&"position: 2 of 22".to_string()));
        let (summary, done) = s.handle(&mut lat, today, "quit");
        assert!(done, "quit ends the session");
        assert!(summary.contains(&"reviewed: 1".to_string()));
        assert!(summary.contains(&"good: 1".to_string()));
        assert!(summary.contains(&"best_streak: 1".to_string()));
    }

    #[test]
    fn app_chapters_lesson_and_practice_match_the_wizard() {
        let (mut lat, today) = born("/tmp/qexo-app-test-b/state.phext");
        let mut s = session();
        let chapters = s.handle(&mut lat, today, "chapters").0;
        assert_eq!(chapters.len(), CHAPTERS.len());
        assert!(chapters[0].starts_with("chapter: 1 | First steps | 3 cards | 3 due"));
        let lesson = s.handle(&mut lat, today, "lesson 1").0;
        assert!(lesson.contains(&"title: First steps".to_string()));
        assert!(lesson.iter().any(|l| l.starts_with("lesson: ")));
        assert!(lesson.contains(&"cards: 3".to_string()));
        assert!(lesson.contains(&"due: 3".to_string()));
        // practice narrows the queue to the chapter, as the
        // wizard's lesson screen does
        let card = s.handle(&mut lat, today, "practice 1").0;
        assert!(card.contains(&"context: Chapter 1 — First steps".to_string()));
        assert!(card.contains(&"position: 1 of 3".to_string()));
        s.handle(&mut lat, today, "reveal").0;
        let graded = s.handle(&mut lat, today, "grade easy").0;
        assert!(graded.contains(&"graded: easy".to_string()));
        assert!(graded.contains(&"remaining: 2".to_string()));
        // protocol errors are lines, not panics
        assert_eq!(
            s.handle(&mut lat, today, "frobnicate").0,
            vec!["error: unknown command 'frobnicate'".to_string()]
        );
        assert_eq!(
            s.handle(&mut lat, today, "lesson 99").0,
            vec!["error: lesson needs a chapter number".to_string()]
        );
        assert_eq!(
            s.handle(&mut lat, today, "grade sideways").0,
            vec!["error: grade needs again, good, or easy".to_string()]
        );
    }

    #[test]
    fn app_empty_queue_is_a_note_not_a_hang() {
        let (mut lat, today) = born("/tmp/qexo-app-test-c/state.phext");
        // grade the whole deck into the future, wizard-style
        for (_, addr, text) in state_cards(&lat.px) {
            lat.px.insert(addr, apply_grade(&text, Grade::Easy, today));
        }
        let mut s = session();
        let out = s.handle(&mut lat, today, "next").0;
        assert!(out.contains(&"queue: empty".to_string()));
        let out = s.handle(&mut lat, today, "practice 1").0;
        assert!(out.contains(&"queue: empty".to_string()));
        // reveal/grade with no card on the table refuse cleanly
        assert!(s.handle(&mut lat, today, "reveal").0[0].starts_with("error:"));
        assert!(s.handle(&mut lat, today, "grade good").0[0].starts_with("error:"));
    }
}
