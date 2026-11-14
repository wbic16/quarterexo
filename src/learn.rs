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
//! Session discipline: `qexo learn` runs due cards only when
//! stdin is a terminal (prompt, wait for Enter, reveal, grade
//! 1/2/3, save after every card). With piped stdin it prints the
//! --status view instead of hanging. `qexo learn --status`
//! never writes: on an unseeded deck it computes from the baked
//! table.

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
        back: "Births the node: creates the state phext and sets up the node GPG key. The state path resolves as: QEXO_STATE when set, else /var/lib/qexo/state.phext when it exists and is readable, else ~/.qexo/state.phext. A normal user births a per-user node — no sudo anywhere.",
    },
    CardSpec {
        id: "verbs-register",
        front: "How does a peer become trusted?",
        back: "`qexo register <pubkey>` — the operator registers the peer's GPG public key (armored text, a file path, or - for stdin). The fingerprint and the host name are read from the key itself, never from the caller's say-so. Only registered peers are ever polled.",
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
        back: "It polls registered peers for the trusted signer's release manifest, verifies the detached GPG signature over the manifest and the binary's size + sha256 BEFORE swapping, keeps qexo.prev, and rolls back if the swapped binary fails its post-check. --check reports only — downloads and changes nothing.",
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
        back: "`qexo release` writes a signed release manifest — a record in the ledgers homeland with its detached GPG signature on the adjacent scroll — and serves the binary from the releases dir beside the state file. Only the node whose fingerprint is a peer's configured release_signer is believed.",
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
        back: "GPG keys. Every node is born with its own key (keyring beside the state file); peers are known by fingerprint; signatures carry trust. Endpoints are only transport — a spoofed endpoint cannot forge a signature, which is why plain http:// on the tailnet is acceptable.",
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

fn run_session(lat: &mut Lattice, today: i64) -> i32 {
    let due = due_cards(lat, today);
    if due.is_empty() {
        println!("nothing due — the deck is ahead of you.");
        print_status(lat, today);
        return 0;
    }
    let total = due.len();
    let stdin = std::io::stdin();
    let (mut again, mut good, mut easy) = (0usize, 0usize, 0usize);
    let mut reviewed = 0usize;
    'cards: for (i, (_, addr, text)) in due.iter().enumerate() {
        println!("\n[{}/{}] {}", i + 1, total, lattice::field(text, "front").unwrap_or_default());
        println!("(press Enter to reveal — Ctrl-D ends the session)");
        if read_line(&stdin).is_none() {
            break;
        }
        println!("{}", lattice::field(text, "back").unwrap_or_default());
        let grade = loop {
            print!("grade [1 again / 2 good / 3 easy]: ");
            let _ = std::io::stdout().flush();
            match read_line(&stdin) {
                None => break 'cards,
                Some(s) => match s.trim() {
                    "1" | "again" => break Grade::Again,
                    "2" | "good" => break Grade::Good,
                    "3" | "easy" => break Grade::Easy,
                    _ => {
                        println!("grade with 1, 2 or 3");
                        continue;
                    }
                },
            }
        };
        match grade {
            Grade::Again => again += 1,
            Grade::Good => good += 1,
            Grade::Easy => easy += 1,
        }
        let updated = apply_grade(text, grade, today);
        lat.px.insert(*addr, updated);
        if let Err(e) = lat.save() {
            eprintln!("qexo learn: cannot save state: {}", e);
            return 1;
        }
        reviewed += 1;
    }
    let st = deck_stats(lat, today);
    println!(
        "\nsession: {} reviewed — {} again, {} good, {} easy; {} still due",
        reviewed, again, good, easy, st.due_now
    );
    if let Some((day, n)) = st.next_due {
        let (y, m, d) = core::civil_from_days(day);
        println!("next due: {:04}-{:02}-{:02} ({} card(s))", y, m, d, n);
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
    run_session(&mut lat, today)
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
