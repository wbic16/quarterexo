//! Dispatch — the queue's local verbs (dispatch core, slice 1).
//!
//! Slice 1 lays the foundation only: item records are born in
//! the dispatch homeland (11.11.11, cluster 2) via `enqueue`,
//! listed by `queue`, and read by `show`. There are NO state
//! transitions beyond birth in this slice (the state machine
//! is slice 2), no network behavior, and no register calls
//! (the drain is slice 3). What slice 1 must already honor,
//! because later slices build on it:
//!
//! - Write-before-ack (spec §4.1): `enqueue` prints the item's
//!   address only after the state save that contains the
//!   record completes. If the save fails, no address is
//!   printed and the exit is non-zero — an address in hand is
//!   proof the record is durable.
//! - The item record is one scroll, fields as lines (spec
//!   §3.2), at 11.11.11/2.1.<book>/1.1.1 — one Book per item
//!   in arrival sequence; the brief rides at Scroll 2.
//! - `show` is the dispatch-aware reader: its address may
//!   carry a `$N:` universe prefix (spec §3.3) and reads that
//!   universe of the state chain; a bare address is universe 1.

use crate::core::{now_stamp, require_born};
use crate::lattice;
use crate::lattice::Lattice;
use crate::phext::{self, Coord};

const USAGE_ENQUEUE: &str = "usage: qexo enqueue <brief-file|-> [--title <t>] [--coordinate <task-coordinate>] [--register <n>] [--value <1-9>] [--cost <1-9>] [--goal <text>] [--after <item-address>] [--exit <text>] [--origin <origin>]";

struct EnqueueArgs {
    source: Option<String>,
    title: Option<String>,
    coordinate: Option<String>,
    register: Option<String>,
    value: Option<String>,
    cost: Option<String>,
    goal: Option<String>,
    after: Option<String>,
    exit: Option<String>,
    origin: Option<String>,
}

fn parse_enqueue_args(args: &[String]) -> Result<EnqueueArgs, String> {
    let mut out = EnqueueArgs {
        source: None,
        title: None,
        coordinate: None,
        register: None,
        value: None,
        cost: None,
        goal: None,
        after: None,
        exit: None,
        origin: None,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let mut take = |slot: &mut Option<String>| -> Result<(), String> {
            if i + 1 >= args.len() {
                return Err(format!("{} needs a value", a));
            }
            *slot = Some(args[i + 1].clone());
            i += 2;
            Ok(())
        };
        match a.as_str() {
            "--title" => take(&mut out.title)?,
            "--coordinate" => take(&mut out.coordinate)?,
            "--register" => take(&mut out.register)?,
            "--value" => take(&mut out.value)?,
            "--cost" => take(&mut out.cost)?,
            "--goal" => take(&mut out.goal)?,
            "--after" => take(&mut out.after)?,
            "--exit" => take(&mut out.exit)?,
            "--origin" => take(&mut out.origin)?,
            _ if a.starts_with("--") => return Err(format!("unknown flag {}", a)),
            _ => {
                if out.source.is_some() {
                    return Err("enqueue takes exactly one brief file".into());
                }
                out.source = Some(a.clone());
                i += 1;
            }
        }
    }
    Ok(out)
}

fn roi_1_9(flag: &str, v: &Option<String>) -> Result<Option<u32>, String> {
    match v {
        None => Ok(None),
        Some(s) => match s.parse::<u32>() {
            Ok(n) if (1..=9).contains(&n) => Ok(Some(n)),
            _ => Err(format!("{} must be an integer 1-9 (got '{}')", flag, s)),
        },
    }
}

/// Build the item record scroll (spec §3.2). Field order is
/// the spec's; absent optional fields are omitted, never
/// blanked — renderings print their absence (`coordinate:
/// unminted`, `roi: unestimated`) from the omission.
pub fn item_record(
    title: &str,
    coordinate: Option<&str>,
    register: Option<&str>,
    origin: &str,
    enqueued: &str,
    enqueued_by: &str,
    goal: &str,
    value: Option<u32>,
    cost: Option<u32>,
    after: Option<&str>,
    exit: Option<&str>,
) -> String {
    let mut fields: Vec<(&str, String)> = Vec::new();
    fields.push(("title", title.to_string()));
    if let Some(c) = coordinate {
        fields.push(("coordinate", c.to_string()));
    }
    if let Some(r) = register {
        fields.push(("register", r.to_string()));
    }
    fields.push(("origin", origin.to_string()));
    fields.push(("enqueued", enqueued.to_string()));
    fields.push(("enqueued-by", enqueued_by.to_string()));
    fields.push(("state", "queued".to_string()));
    fields.push(("goal", goal.to_string()));
    if let Some(v) = value {
        fields.push(("value", v.to_string()));
    }
    if let Some(c) = cost {
        fields.push(("cost", c.to_string()));
    }
    if let Some(a) = after {
        fields.push(("after", a.to_string()));
    }
    fields.push(("attempts", "0".to_string()));
    fields.push(("failures", "0".to_string()));
    if let Some(e) = exit {
        fields.push(("exit", e.to_string()));
    }
    let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut text = String::from("queue item\n");
    text.push_str(&lattice::record(&refs));
    text
}

/// The address of item Book `book`'s record scroll.
pub fn item_addr(book: u32) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ITEMS_C, 1, book, 1, 1, 1])
}

/// The address of item Book `book`'s brief scroll.
pub fn brief_addr(book: u32) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ITEMS_C, 1, book, 1, 1, 2])
}

pub fn enqueue(args: &[String]) -> i32 {
    let parsed = match parse_enqueue_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("qexo enqueue: {}\n{}", e, USAGE_ENQUEUE);
            return 2;
        }
    };
    let source = match &parsed.source {
        Some(s) => s.clone(),
        None => {
            eprintln!("qexo enqueue: a brief file is required\n{}", USAGE_ENQUEUE);
            return 2;
        }
    };
    // Validate everything before touching state.
    let value = match roi_1_9("--value", &parsed.value) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("qexo enqueue: {}", e);
            return 2;
        }
    };
    let cost = match roi_1_9("--cost", &parsed.cost) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("qexo enqueue: {}", e);
            return 2;
        }
    };
    if let Some(c) = &parsed.coordinate {
        if phext::parse_coord(c).is_none() {
            eprintln!("qexo enqueue: --coordinate '{}' is not a full 9D coordinate", c);
            return 2;
        }
    }
    if let Some(r) = &parsed.register {
        if r.parse::<u64>().is_err() {
            eprintln!("qexo enqueue: --register '{}' is not a register number", r);
            return 2;
        }
    }
    if let Some(a) = &parsed.after {
        if phext::parse_coord(a).is_none() {
            eprintln!("qexo enqueue: --after '{}' is not a full 9D address", a);
            return 2;
        }
    }
    let brief = if source == "-" {
        use std::io::Read;
        let mut buf = String::new();
        match std::io::stdin().read_to_string(&mut buf) {
            Ok(_) => buf,
            Err(e) => {
                eprintln!("qexo enqueue: cannot read brief from stdin: {}", e);
                return 2;
            }
        }
    } else {
        match std::fs::read_to_string(&source) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("qexo enqueue: {}: {}", source, e);
                return 2;
            }
        }
    };
    let title = parsed
        .title
        .clone()
        .or_else(|| {
            brief
                .lines()
                .map(|l| l.trim())
                .find(|l| !l.is_empty())
                .map(|l| l.to_string())
        })
        .unwrap_or_else(|| "untitled brief".to_string());

    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let node = lat
        .px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .unwrap_or_else(|| "unknown-node".to_string());
    let fp = lat
        .px
        .get(&lattice::NODE_KEY_RECORD)
        .and_then(|t| lattice::field(t, "fingerprint"))
        .unwrap_or_default();
    let enqueued_by = if fp.is_empty() { node.clone() } else { format!("{} {}", node, fp) };
    let (_, stamp) = now_stamp();
    let record_text = item_record(
        &title,
        parsed.coordinate.as_deref(),
        parsed.register.as_deref(),
        parsed.origin.as_deref().unwrap_or("emise"),
        &stamp,
        &enqueued_by,
        parsed.goal.as_deref().unwrap_or("standing"),
        value,
        cost,
        parsed.after.as_deref(),
        parsed.exit.as_deref(),
    );
    let book = lattice::next_free_book(&lat.px, [11, 11, 11, lattice::DISPATCH_ITEMS_C, 1], 1);
    let addr = item_addr(book);
    lat.px.insert(addr, record_text);
    if !brief.is_empty() {
        lat.px.insert(brief_addr(book), brief);
    }
    lattice::regen_indexes(&mut lat.px);
    // Write-before-ack (§4.1): the address is printed only
    // after the save that carries the record has completed.
    if let Err(e) = lat.save() {
        eprintln!("qexo enqueue: cannot save state: {}", e);
        return 1;
    }
    println!("{}", phext::fmt_coord(&addr));
    0
}

pub fn queue(all: bool, node: Option<String>) -> i32 {
    let lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let books = lattice::component_values(&lat.px, &[11, 11, 11, lattice::DISPATCH_ITEMS_C, 1], 5);
    let mut shown = 0usize;
    let mut lines = Vec::new();
    for b in &books {
        let addr = item_addr(*b);
        let text = match lat.px.get(&addr) {
            Some(t) => t,
            None => continue,
        };
        let state = lattice::field(text, "state").unwrap_or_default();
        if !all && (state == "done" || state == "cancelled") {
            continue;
        }
        if let Some(want) = &node {
            let by = lattice::field(text, "enqueued-by").unwrap_or_default();
            if by.split_whitespace().next().unwrap_or("") != want.as_str() {
                continue;
            }
        }
        let title = lattice::field(text, "title").unwrap_or_else(|| "untitled".into());
        let roi = match (lattice::field(text, "value"), lattice::field(text, "cost")) {
            (Some(v), Some(c)) => format!("value {} / cost {}", v, c),
            _ => "roi: unestimated".to_string(),
        };
        let enqueued = lattice::field(text, "enqueued").unwrap_or_default();
        lines.push(format!(
            "{} — {} — {} — {} — enqueued {}",
            phext::fmt_coord(&addr),
            state,
            title,
            roi,
            enqueued
        ));
        shown += 1;
    }
    println!("queue: {} item(s)", shown);
    for l in lines {
        println!("{}", l);
    }
    0
}

pub fn show(addr_s: &str) -> i32 {
    let (universe, coord) = match phext::parse_universe_addr(addr_s) {
        Some(x) => x,
        None => {
            eprintln!("qexo show: '{}' is not a full 9D address (a $N: prefix names the universe)", addr_s);
            return 2;
        }
    };
    let lat = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => {
            eprintln!(
                "qexo: {} is not born yet — run `qexo config` to birth this node",
                lattice::state_path().display()
            );
            return 1;
        }
        Err(e) => {
            eprintln!("qexo: cannot load state: {}", e);
            return 1;
        }
    };
    let doc = match lat.universe(universe) {
        Some(d) => d,
        None => {
            eprintln!(
                "qexo show: this node's state chain has {} universe(s) — no universe ${}",
                lat.universe_count(),
                universe
            );
            return 1;
        }
    };
    match doc.get(&coord) {
        Some(body) => {
            print!("{}", body);
            if !body.ends_with('\n') {
                println!();
            }
            0
        }
        None => {
            eprintln!("no scroll at {}", addr_s);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_record_carries_the_spec_fields_in_order() {
        let t = item_record(
            "Do the thing",
            Some("26.5.9/7.6.11/5.5.10"),
            Some("112"),
            "emise",
            "2026-11-16 07:52 UTC",
            "aletheia-core A4E7277FB3EB0E589604BE5B506DBE3FC209E755",
            "standing",
            Some(7),
            Some(2),
            Some("11.11.11/2.1.4/1.1.1"),
            Some("the proof lands"),
        );
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "queue item");
        assert_eq!(lines[1], "title: Do the thing");
        assert_eq!(lines[2], "coordinate: 26.5.9/7.6.11/5.5.10");
        assert_eq!(lines[3], "register: 112");
        assert_eq!(lines[4], "origin: emise");
        assert_eq!(lines[6], "enqueued-by: aletheia-core A4E7277FB3EB0E589604BE5B506DBE3FC209E755");
        assert_eq!(lines[7], "state: queued");
        assert_eq!(lines[9], "value: 7");
        assert_eq!(lines[10], "cost: 2");
        assert_eq!(lines[11], "after: 11.11.11/2.1.4/1.1.1");
        assert_eq!(lines[12], "attempts: 0");
        assert_eq!(lines[13], "failures: 0");
        assert_eq!(lines[14], "exit: the proof lands");
        assert_eq!(lattice::field(&t, "state").as_deref(), Some("queued"));
    }

    #[test]
    fn item_record_omits_absent_optionals_never_blanks_them() {
        let t = item_record("Bare", None, None, "will", "2026-11-16 07:52 UTC", "phone", "standing", None, None, None, None);
        assert!(!t.contains("coordinate:"), "{}", t);
        assert!(!t.contains("register:"), "{}", t);
        assert!(!t.contains("value:"), "{}", t);
        assert!(!t.contains("after:"), "{}", t);
        assert!(t.contains("state: queued\n"));
    }

    #[test]
    fn item_addresses_are_the_spec_shape() {
        // §3.3's example: the item at Book 4 is 11.11.11/2.1.4/1.1.1
        assert_eq!(item_addr(4), [11, 11, 11, 2, 1, 4, 1, 1, 1]);
        assert_eq!(phext::fmt_coord(&item_addr(4)), "11.11.11/2.1.4/1.1.1");
        assert_eq!(phext::fmt_coord(&brief_addr(4)), "11.11.11/2.1.4/1.1.2");
    }

    #[test]
    fn enqueue_args_validate() {
        let bad = parse_enqueue_args(&["--value".to_string()]);
        assert!(bad.is_err());
        let ok = parse_enqueue_args(&["brief.md".to_string(), "--title".to_string(), "T".to_string()]);
        assert!(ok.is_ok());
        assert_eq!(roi_1_9("--value", &Some("0".to_string())).is_err(), true);
        assert_eq!(roi_1_9("--value", &Some("9".to_string())).unwrap(), Some(9));
        assert_eq!(roi_1_9("--value", &Some("10".to_string())).is_err(), true);
        assert_eq!(roi_1_9("--value", &None).unwrap(), None);
    }
}
