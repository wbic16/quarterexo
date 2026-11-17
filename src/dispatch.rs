//! Dispatch — the queue's local verbs (dispatch core,
//! slices 1–2).
//!
//! Slice 1 laid the foundation: item records are born in the
//! dispatch homeland (11.11.11, cluster 2) via `enqueue`,
//! listed by `queue`, and read by `show`. Slice 2 adds the
//! queue semantics (spec §13.3 slice 2): the §4.6 state
//! machine as code (one legality oracle every transition
//! passes through), transitions and receipts as one write
//! (§4.2 — audits in the same save where §7 requires them),
//! the `pause`/`resume`/`cancel` verbs, the calculated ROI
//! ordering with `--explain` (§5), and the `audit` view over
//! the ledgers' audits cluster (6.6.6, Collection 9). The
//! doctor's integrity reconciliation (§4.5) lives in
//! doctor.rs and reads the same records through this
//! module's loaders. Still no network behavior and no
//! register calls (the drain is slice 3); transitions whose
//! drivers arrive with wire dispatch (slice 4) and F5
//! (slice 5) are encoded in the machine's table now, so
//! those slices call the same oracle instead of growing a
//! second one. What slice 1 must already honor, because
//! later slices build on it:
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

// ----------------------------------------------------------------
// Slice 2 — the queue state machine (spec §4.6), transitions
// and receipts as one write (§4.2), and the record loaders the
// doctor's reconciliation (§4.5) reads through.
// ----------------------------------------------------------------

/// The eight queue states (spec §4.6). `dispatched` = bundle
/// accepted, execution not yet evidenced; `waiting` = blocked
/// on Will, cites its register number, dispatch skips it;
/// `paused` = manual; `paused-auto` = the §8 auto-pause after
/// the third consecutive failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Queued,
    Dispatched,
    Running,
    Waiting,
    Paused,
    PausedAuto,
    Done,
    Cancelled,
}

pub const ALL_STATES: [ItemState; 8] = [
    ItemState::Queued,
    ItemState::Dispatched,
    ItemState::Running,
    ItemState::Waiting,
    ItemState::Paused,
    ItemState::PausedAuto,
    ItemState::Done,
    ItemState::Cancelled,
];

impl ItemState {
    pub fn parse(s: &str) -> Option<ItemState> {
        match s {
            "queued" => Some(ItemState::Queued),
            "dispatched" => Some(ItemState::Dispatched),
            "running" => Some(ItemState::Running),
            "waiting" => Some(ItemState::Waiting),
            "paused" => Some(ItemState::Paused),
            "paused-auto" => Some(ItemState::PausedAuto),
            "done" => Some(ItemState::Done),
            "cancelled" => Some(ItemState::Cancelled),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ItemState::Queued => "queued",
            ItemState::Dispatched => "dispatched",
            ItemState::Running => "running",
            ItemState::Waiting => "waiting",
            ItemState::Paused => "paused",
            ItemState::PausedAuto => "paused-auto",
            ItemState::Done => "done",
            ItemState::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, ItemState::Done | ItemState::Cancelled)
    }
}

/// Who performs a transition (spec §4.6 names an actor per
/// transition; actors are checked, not trusted — illegal
/// combination 4 is a cancel by the wrong actor). The CLI
/// verbs act as Manual (pause/resume) and WillEmise (cancel:
/// the operator at their own node). The remaining kinds are
/// constructed by the transition engine's callers in slices
/// 3–5 (drain, wire dispatch, F5) — the table below is the
/// one oracle they will all pass through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Actor {
    Enqueuer,
    Dispatch,
    Executor,
    Coordinator,
    Manual,
    WillEmise,
}

/// A queue item record (§3.2), parsed. Raw field strings ride
/// beside the parsed values so the doctor can flag a corrupt
/// count or state instead of silently reckoning it as zero.
/// Every §3.2 field is parsed even where slice 2 does not
/// read it yet — the record's shape is the contract, and
/// slices 3–4 read the rest.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Item {
    pub book: u32,
    pub addr: Coord,
    pub title: String,
    pub coordinate: Option<String>,
    pub register: Option<String>,
    /// The register entry seq this item was drained at
    /// (§10.3) — the drain's dedup key. Register-born items
    /// carry it from birth; other items never do.
    pub register_seq: Option<String>,
    /// §10.4 write-back bookkeeping: `pending` when a local
    /// terminal close owes the register its resolution,
    /// `recorded` once the resolution stands at the
    /// register. Absent when no write-back is owed.
    pub register_writeback: Option<String>,
    pub origin: String,
    pub enqueued: String,
    pub enqueued_min: Option<i64>,
    pub enqueued_by: String,
    pub state_raw: String,
    pub state: Option<ItemState>,
    pub goal: String,
    pub value: Option<u32>,
    pub cost: Option<u32>,
    pub after: Option<String>,
    pub attempts_raw: String,
    pub attempts: u32,
    pub failures_raw: String,
    pub failures: u32,
    pub bundle_sha256: Option<String>,
    pub pin: Option<String>,
    pub home: Option<String>,
    pub exit: Option<String>,
}

pub fn parse_item(book: u32, text: &str) -> Item {
    let f = |k: &str| lattice::field(text, k);
    let attempts_raw = f("attempts").unwrap_or_default();
    let failures_raw = f("failures").unwrap_or_default();
    let state_raw = f("state").unwrap_or_default();
    let enqueued = f("enqueued").unwrap_or_default();
    Item {
        book,
        addr: item_addr(book),
        title: f("title").unwrap_or_else(|| "untitled".to_string()),
        coordinate: f("coordinate"),
        register: f("register"),
        register_seq: f("register-seq"),
        register_writeback: f("register-writeback"),
        origin: f("origin").unwrap_or_default(),
        enqueued_min: crate::core::stamp_minutes(&enqueued),
        enqueued,
        enqueued_by: f("enqueued-by").unwrap_or_default(),
        state: ItemState::parse(&state_raw),
        state_raw,
        goal: f("goal").unwrap_or_else(|| "standing".to_string()),
        value: f("value").and_then(|v| v.parse().ok()),
        cost: f("cost").and_then(|v| v.parse().ok()),
        after: f("after"),
        attempts: attempts_raw.parse().unwrap_or(0),
        attempts_raw,
        failures: failures_raw.parse().unwrap_or(0),
        failures_raw,
        bundle_sha256: f("bundle-sha256"),
        pin: f("pin"),
        home: f("home"),
        exit: f("exit"),
    }
}

/// Every queue item in the document, in Book (arrival) order.
pub fn load_items(px: &phext::Phext) -> Vec<Item> {
    let books = lattice::component_values(px, &[11, 11, 11, lattice::DISPATCH_ITEMS_C, 1], 5);
    let mut out = Vec::new();
    for b in books {
        if let Some(t) = px.get(&item_addr(b)) {
            out.push(parse_item(b, t));
        }
    }
    out
}

/// This node's actor string — node name + full fingerprint,
/// the shape `enqueued-by` already carries (receipts cite
/// actors the same way, §4.2).
pub fn local_actor(px: &phext::Phext) -> String {
    let node = px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .unwrap_or_else(|| "unknown-node".to_string());
    let fp = px
        .get(&lattice::NODE_KEY_RECORD)
        .and_then(|t| lattice::field(t, "fingerprint"))
        .unwrap_or_default();
    if fp.is_empty() {
        node
    } else {
        format!("{} {}", node, fp)
    }
}

/// One line, always: a field value carrying a newline would
/// break the record's field lines, so transition text is
/// flattened at the boundary.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A transition request (spec §4.6). `evidence` is the note,
/// the failure evidence, or the resolution evidence, per the
/// transition; `checked`/`showed`/`verdict` are the F4
/// live-surface evidence shape (§7) a `done` requires;
/// `register` supplies the register cite a move to `waiting`
/// needs when the record does not already carry one;
/// `manual` distinguishes a human resume from any automatic
/// one — `paused-auto` yields only to the manual kind (§8).
#[derive(Debug, Clone)]
pub struct Transition {
    pub to: ItemState,
    pub actor: Actor,
    pub actor_name: String,
    pub evidence: String,
    pub checked: Option<String>,
    pub showed: Option<String>,
    pub verdict: Option<String>,
    pub register: Option<String>,
    pub manual: bool,
    /// A register resolution is closing this item (§10.3):
    /// the drain's path, gated on the resolution's F4 cite.
    /// No other caller sets this — every other road to a
    /// terminal state keeps the matrix below.
    pub register_close: bool,
}

impl Transition {
    pub fn new(to: ItemState, actor: Actor, actor_name: &str) -> Transition {
        Transition {
            to,
            actor,
            actor_name: actor_name.to_string(),
            evidence: String::new(),
            checked: None,
            showed: None,
            verdict: None,
            register: None,
            manual: false,
            register_close: false,
        }
    }
}

/// What a legal transition will do, before anything is
/// written: the state it lands in (a third consecutive
/// failure lands in `paused-auto`, not `queued`, §8), the
/// counters after it, the register cite it sets, and the
/// audit it owes (§7.2), if any.
pub struct Plan {
    to: ItemState,
    attempts: u32,
    failures: u32,
    register: Option<String>,
    audit: Option<AuditOwed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOwed {
    NonPingClose,
    AutoPause,
    Override,
    /// A closure the register itself ordered (§10.3): the
    /// audit kind is `register-close`, citing the
    /// resolution seq as its live-surface evidence (§7.2).
    RegisterClose,
}

/// The legality oracle (spec §4.6): is this transition
/// allowed from the item's current state, by this actor,
/// with this evidence? Every refusal names the rule it
/// breaks. Pure — it reads the item and the request and
/// decides; `apply_transition` is the only writer.
pub fn check_transition(item: &Item, t: &Transition) -> Result<Plan, String> {
    let from = match item.state {
        Some(s) => s,
        None => {
            return Err(format!(
                "item {} is in unknown state '{}' — no transition is legal until the record is repaired",
                phext::fmt_coord(&item.addr),
                item.state_raw
            ))
        }
    };
    if from.is_terminal() {
        return Err(format!(
            "{} is terminal — no transition leaves it (§4.6)",
            from.name()
        ));
    }
    let plain = |to: ItemState| Plan {
        to,
        attempts: item.attempts,
        failures: item.failures,
        register: None,
        audit: None,
    };
    if t.register_close {
        // §10.3: a register resolution closes the matching
        // item where it stands. The register of record is
        // the live surface and its resolution atom is the
        // F4 evidence — checked/showed/verdict cite it, and
        // the audit kind is register-close (§7.2). This
        // path is the drain's alone: the coordinator acting
        // on a resolution. It exists because the register
        // is an authority the matrix's roads do not model —
        // an item Will resolves in the register closes even
        // if it never ran. Every other road to done or
        // cancelled keeps the matrix below, unchanged.
        if !matches!(t.to, ItemState::Done | ItemState::Cancelled) {
            return Err("a register close lands in done or cancelled, nothing else (§10.3)".into());
        }
        if t.actor != Actor::Coordinator {
            return Err("a register close is the coordinator's act — the drain applying a resolution (§10.3)".into());
        }
        if t.evidence.trim().is_empty() {
            return Err("a register close cites its resolution — the note or the seq (§10.3)".into());
        }
        let f4 = [&t.checked, &t.showed, &t.verdict];
        if f4.iter().any(|f| f.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)) {
            return Err("a register close carries the F4 shape — checked, showed, verdict (§7.2)".into());
        }
        let mut p = plain(t.to);
        p.audit = Some(AuditOwed::RegisterClose);
        return Ok(p);
    }
    let need_actor = |ok: bool, who: &str| -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err(format!("{} → {} is {}'s to make, not this actor's (§4.6)", from.name(), t.to.name(), who))
        }
    };
    match (from, t.to) {
        // enqueue → queued is birth (slice 1); it is not a
        // transition and never reaches this oracle.
        (ItemState::Queued, ItemState::Dispatched) => {
            need_actor(matches!(t.actor, Actor::Dispatch | Actor::Coordinator), "the dispatch verb")?;
            let mut p = plain(ItemState::Dispatched);
            p.attempts = item.attempts + 1; // an attempt begins at dispatch (§3.2)
            Ok(p)
        }
        (ItemState::Dispatched, ItemState::Running) => {
            need_actor(t.actor == Actor::Executor, "the executor (its first heartbeat)")?;
            Ok(plain(ItemState::Running))
        }
        (ItemState::Running, ItemState::Running) => {
            // A checkpoint append: state does not move; the
            // receipt is the transition's record in this
            // slice (the attempt-universe checkpoint scrolls
            // land with wire dispatch, slice 4).
            need_actor(t.actor == Actor::Executor, "the executor")?;
            Ok(plain(ItemState::Running))
        }
        (ItemState::Running, ItemState::Done) => {
            need_actor(t.actor == Actor::Coordinator, "the coordinator (executor's completion receipt + live-surface verification)")?;
            let f4 = [&t.checked, &t.showed, &t.verdict];
            if f4.iter().any(|f| f.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)) {
                return Err("done requires live-surface evidence in the F4 shape — checked, showed, verdict (§4.4, §7)".into());
            }
            let mut p = plain(ItemState::Done);
            p.audit = Some(AuditOwed::NonPingClose);
            Ok(p)
        }
        (ItemState::Running, ItemState::Queued) | (ItemState::Dispatched, ItemState::Queued) => {
            // Failure: the attempt closes failed, failures+1;
            // the failure that would make failures 3 lands in
            // paused-auto instead (§8), with its audit.
            need_actor(matches!(t.actor, Actor::Coordinator | Actor::Executor), "the coordinator or the executor (its failed receipt)")?;
            if t.evidence.trim().is_empty() {
                return Err("a failure transition carries its evidence — the executor's failed receipt or the detection note (§8)".into());
            }
            let failures = item.failures + 1;
            let mut p = plain(if failures >= 3 { ItemState::PausedAuto } else { ItemState::Queued });
            p.failures = failures;
            if failures >= 3 {
                p.audit = Some(AuditOwed::AutoPause);
            }
            Ok(p)
        }
        (ItemState::Queued, ItemState::Waiting) => {
            need_actor(matches!(t.actor, Actor::Coordinator | Actor::Manual | Actor::WillEmise), "the coordinator")?;
            let reg = t.register.clone().or_else(|| item.register.clone());
            match reg {
                Some(r) if !r.trim().is_empty() => {
                    let mut p = plain(ItemState::Waiting);
                    if item.register.as_deref() != Some(r.as_str()) {
                        p.register = Some(r);
                    }
                    Ok(p)
                }
                _ => Err("waiting cites its register number — none on the record and none supplied (§4.6)".into()),
            }
        }
        (ItemState::Waiting, ItemState::Queued) => {
            need_actor(matches!(t.actor, Actor::WillEmise | Actor::Coordinator | Actor::Manual), "Will or the coordinator")?;
            if t.evidence.trim().is_empty() {
                return Err("waiting → queued carries its evidence: the register resolution or Will's word (§4.6)".into());
            }
            Ok(plain(ItemState::Queued))
        }
        (ItemState::Paused, ItemState::Queued) => {
            need_actor(matches!(t.actor, Actor::Manual | Actor::WillEmise), "a manual resume")?;
            let mut p = plain(ItemState::Queued);
            p.failures = 0; // a manual resume resets the consecutive-failure counter (§8)
            Ok(p)
        }
        (ItemState::PausedAuto, ItemState::Queued) => {
            need_actor(matches!(t.actor, Actor::Manual | Actor::WillEmise), "a manual resume")?;
            if !t.manual {
                return Err("paused-auto yields to a manual resume only (§8)".into());
            }
            let mut p = plain(ItemState::Queued);
            p.failures = 0;
            p.audit = Some(AuditOwed::Override);
            Ok(p)
        }
        (_, ItemState::Paused) if matches!(from, ItemState::Queued | ItemState::Dispatched | ItemState::Running | ItemState::Waiting) => {
            need_actor(matches!(t.actor, Actor::Manual | Actor::WillEmise), "a manual pause")?;
            Ok(plain(ItemState::Paused))
        }
        (_, ItemState::Cancelled) => {
            need_actor(t.actor == Actor::WillEmise, "Will/Emise")?;
            if t.evidence.trim().is_empty() {
                return Err("cancel requires a note — the note is the receipt's evidence (§2.1, §4.6)".into());
            }
            Ok(plain(ItemState::Cancelled))
        }
        _ => Err(format!("illegal transition {} → {} (§4.6)", from.name(), t.to.name())),
    }
}

/// Append an audit scroll to the audits run (6.6.6,
/// Collection 9, spec §7.1) and return its coordinate. The
/// record is the spec's shape: a header line, then fields.
pub fn append_audit(
    px: &mut phext::Phext,
    kind: &str,
    item: &str,
    checked: &str,
    showed: &str,
    verdict: &str,
    actor: &str,
) -> Coord {
    let b = lattice::next_free_book(px, [6, 6, 6, lattice::LEDGER_AUDITS, 1], 1);
    let addr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_AUDITS, 1, b, 1, 1, 1]);
    let (_, stamp) = now_stamp();
    let fields: Vec<(&str, &str)> = vec![
        ("kind", kind),
        ("item", item),
        ("checked", checked),
        ("showed", showed),
        ("verdict", verdict),
        ("actor", actor),
        ("at", &stamp),
    ];
    let mut text = String::from("audit\n");
    text.push_str(&lattice::record(&fields));
    px.insert(addr, text);
    addr
}

/// Apply a transition: the item's new state, its receipt,
/// and its audit (where one is owed) are mutated into the
/// document together; the CALLER saves once — so the save
/// either contains all of it or none of it (§4.2's one-write
/// rule), and a verb acks only after that save (§4.1).
/// Returns the receipt's coordinate.
pub fn apply_transition(lat: &mut Lattice, book: u32, t: &Transition) -> Result<Coord, String> {
    let addr = item_addr(book);
    let addr_s = phext::fmt_coord(&addr);
    let text = lat
        .px
        .get(&addr)
        .cloned()
        .ok_or_else(|| format!("no queue item at {}", addr_s))?;
    let item = parse_item(book, &text);
    let plan = check_transition(&item, t)?;
    let from_name = item.state.map(|s| s.name()).unwrap_or(&item.state_raw);

    let mut new_text = text;
    new_text = lattice::set_field(&new_text, "state", plan.to.name());
    if plan.attempts != item.attempts {
        new_text = lattice::set_field(&new_text, "attempts", &plan.attempts.to_string());
    }
    if plan.failures != item.failures {
        new_text = lattice::set_field(&new_text, "failures", &plan.failures.to_string());
    }
    if let Some(r) = &plan.register {
        new_text = lattice::set_field(&new_text, "register", r);
    }
    // §10.4 write-back bookkeeping, in the same write: a
    // LOCAL terminal close of a register-numbered item owes
    // the register its resolution — the record says
    // `pending` until the write-back lands (§10.4). A
    // register-driven close is the register's own act; the
    // resolution already stands there, so the field records
    // it settled.
    if plan.to.is_terminal() && item.register.is_some() {
        if t.register_close {
            new_text = lattice::set_field(&new_text, "register-writeback", "recorded");
        } else if item.register_writeback.as_deref() != Some("recorded") {
            new_text = lattice::set_field(&new_text, "register-writeback", "pending");
        }
    }
    lat.px.insert(addr, new_text);

    // The receipt (§4.2): item, from→to, actor, evidence —
    // and the bundle hash where one exists.
    let evidence = if plan.to == ItemState::Done {
        format!(
            "checked: {}; showed: {}; verdict: {}",
            one_line(t.checked.as_deref().unwrap_or_default()),
            one_line(t.showed.as_deref().unwrap_or_default()),
            one_line(t.verdict.as_deref().unwrap_or_default())
        )
    } else {
        one_line(&t.evidence)
    };
    let actor_name = one_line(&t.actor_name);
    let mut extras: Vec<(&str, &str)> = vec![
        ("item", &addr_s),
        ("from", from_name),
        ("to", plan.to.name()),
        ("actor", &actor_name),
        ("evidence", &evidence),
    ];
    if let Some(h) = &item.bundle_sha256 {
        extras.push(("bundle-sha256", h));
    }
    let receipt = crate::core::append_receipt(&mut lat.px, "transition", &extras);

    // The audit, in the same save, where §7 owes one.
    match plan.audit {
        Some(AuditOwed::NonPingClose) => {
            append_audit(
                &mut lat.px,
                "non-ping-close",
                &addr_s,
                &one_line(t.checked.as_deref().unwrap_or_default()),
                &one_line(t.showed.as_deref().unwrap_or_default()),
                &one_line(t.verdict.as_deref().unwrap_or_default()),
                &actor_name,
            );
        }
        Some(AuditOwed::RegisterClose) => {
            // §10.3/§7.2: the register-close audit — its
            // checked field cites the resolution seq, the
            // live-surface evidence the close stands on.
            append_audit(
                &mut lat.px,
                "register-close",
                &addr_s,
                &one_line(t.checked.as_deref().unwrap_or_default()),
                &one_line(t.showed.as_deref().unwrap_or_default()),
                &one_line(t.verdict.as_deref().unwrap_or_default()),
                &actor_name,
            );
        }
        Some(AuditOwed::AutoPause) => {
            // §8: the auto-pause audit cites all three
            // failures' evidence — the failure receipts are
            // the record; the third is the receipt just
            // written above.
            let receipts = load_transition_receipts(&lat.px);
            let fails: Vec<&TransitionReceipt> = receipts
                .iter()
                .filter(|r| {
                    r.item.as_deref() == Some(addr_s.as_str())
                        && matches!(r.from.as_deref(), Some("running") | Some("dispatched"))
                        && matches!(r.to.as_deref(), Some("queued") | Some("paused-auto"))
                })
                .collect();
            let last3: Vec<&TransitionReceipt> = fails.iter().rev().take(3).rev().cloned().collect();
            let cites = last3
                .iter()
                .map(|r| phext::fmt_coord(&r.addr))
                .collect::<Vec<_>>()
                .join(", ");
            let evidences = last3
                .iter()
                .map(|r| r.evidence.clone().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" | ");
            append_audit(
                &mut lat.px,
                "auto-pause",
                &addr_s,
                &format!("failure receipts {}", cites),
                &evidences,
                "auto-paused",
                &actor_name,
            );
        }
        Some(AuditOwed::Override) => {
            append_audit(
                &mut lat.px,
                "auto-pause",
                &addr_s,
                &format!("queue item {} — state paused-auto, failures: {}", addr_s, item.failures),
                &format!("manual resume by {} — consecutive-failure counter reset to 0", actor_name),
                "resumed",
                &actor_name,
            );
        }
        None => {}
    }

    lattice::regen_indexes(&mut lat.px);
    Ok(receipt)
}

/// A transition receipt (6.6.6, Collection 2, action:
/// transition), parsed — the doctor's evidence (§4.5).
#[derive(Debug, Clone)]
pub struct TransitionReceipt {
    pub addr: Coord,
    pub item: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub actor: Option<String>,
    pub evidence: Option<String>,
}

pub fn load_transition_receipts(px: &phext::Phext) -> Vec<TransitionReceipt> {
    let books = lattice::component_values(px, &[6, 6, 6, lattice::LEDGER_RECEIPTS, 1], 5);
    let mut out = Vec::new();
    for b in books {
        let addr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RECEIPTS, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "action").as_deref() == Some("transition") {
                out.push(TransitionReceipt {
                    addr,
                    item: lattice::field(t, "item"),
                    from: lattice::field(t, "from"),
                    to: lattice::field(t, "to"),
                    actor: lattice::field(t, "actor"),
                    evidence: lattice::field(t, "evidence"),
                });
            }
        }
    }
    out
}

/// An audit record (6.6.6, Collection 9, §7.1), parsed.
#[derive(Debug, Clone)]
pub struct AuditRec {
    pub addr: Coord,
    pub kind: String,
    pub item: Option<String>,
    pub checked: Option<String>,
    pub showed: Option<String>,
    pub verdict: Option<String>,
    pub actor: Option<String>,
    pub at: String,
}

pub fn load_audits(px: &phext::Phext) -> Vec<AuditRec> {
    let books = lattice::component_values(px, &[6, 6, 6, lattice::LEDGER_AUDITS, 1], 5);
    let mut out = Vec::new();
    for b in books {
        let addr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_AUDITS, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push(AuditRec {
                addr,
                kind: lattice::field(t, "kind").unwrap_or_default(),
                item: lattice::field(t, "item"),
                checked: lattice::field(t, "checked"),
                showed: lattice::field(t, "showed"),
                verdict: lattice::field(t, "verdict"),
                actor: lattice::field(t, "actor"),
                at: lattice::field(t, "at").unwrap_or_default(),
            });
        }
    }
    out
}

/// An assignment record (11.11.11, Collection 3, §3.1),
/// parsed leniently — slice 4 writes these; the doctor
/// reads them now. An assignment is LIVE while it carries
/// no `status:` line: the status line is its retirement
/// marker (§3.1: "an assignment Book retires (status line)
/// when its attempt closes").
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AssignmentRec {
    pub book: u32,
    pub addr: Coord,
    pub item: Option<String>,
    pub attempt: Option<String>,
    pub heartbeat: Option<String>,
    pub live: bool,
}

pub fn load_assignments(px: &phext::Phext) -> Vec<AssignmentRec> {
    let books = lattice::component_values(px, &[11, 11, 11, lattice::DISPATCH_ASSIGN_C, 1], 5);
    let mut out = Vec::new();
    for b in books {
        let addr = lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push(AssignmentRec {
                book: b,
                addr,
                item: lattice::field(t, "item"),
                attempt: lattice::field(t, "attempt"),
                heartbeat: lattice::field(t, "heartbeat"),
                live: lattice::field(t, "status").is_none(),
            });
        }
    }
    out
}

/// A SHA-256 digest's shape: 64 hex characters. (The
/// digest's truth against the canonical bundle is slice
/// 4's check, where the bundle builder lands; the shape is
/// what a hand-corrupted field breaks today.)
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ----------------------------------------------------------------
// Slice 2 — the calculated order (spec §5) and the verbs:
// queue (+ --explain), pause, resume, cancel, audit.
// ----------------------------------------------------------------

/// The node's current daily goal: a field on the config
/// record (`daily-goal:` in 9.9.9, Collection 2), set at the
/// morning synthesis. Absent = no daily class today.
pub fn daily_goal(px: &phext::Phext) -> Option<String> {
    px.get(&lattice::CONFIG_RECORD)
        .and_then(|t| lattice::field(t, "daily-goal"))
        .filter(|s| !s.is_empty())
}

/// The §5.1 goal class: 0 = names the current daily goal,
/// 1 = standing, 2 = background (names some other goal).
pub fn goal_class(item: &Item, daily_goal: Option<&str>) -> u8 {
    if let Some(d) = daily_goal {
        if !d.is_empty() && item.goal == d {
            return 0;
        }
    }
    if item.goal == "standing" {
        1
    } else {
        2
    }
}

pub fn goal_class_name(class: u8) -> &'static str {
    match class {
        0 => "daily goal",
        1 => "standing",
        _ => "background",
    }
}

fn estimated(item: &Item) -> bool {
    item.value.is_some() && item.cost.is_some()
}

/// The §5.1 calculation, in its exact sequence: goal class,
/// then ROI ratio descending (calculated from the two stored
/// atoms at every read — never stored), then age ascending.
/// Unestimated items sort below estimated ones within their
/// class, by age among themselves (§5.1 rule 2). Ratio
/// comparison cross-multiplies: no floats, no rounding.
fn cmp_calculated(a: &Item, b: &Item, daily: Option<&str>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let by_age = match (a.enqueued_min, b.enqueued_min) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    goal_class(a, daily)
        .cmp(&goal_class(b, daily))
        .then_with(|| estimated(b).cmp(&estimated(a)))
        .then_with(|| match (a.value, a.cost, b.value, b.cost) {
            (Some(av), Some(ac), Some(bv), Some(bc)) => (bv * ac).cmp(&(av * bc)),
            _ => Ordering::Equal,
        })
        .then(by_age)
        .then_with(|| a.book.cmp(&b.book))
}

#[derive(Debug, Clone, PartialEq)]
enum PinKind {
    None,
    Head,
    After(String),
}

fn pin_kind(item: &Item) -> PinKind {
    match item.pin.as_deref() {
        Some("head") => PinKind::Head,
        Some(p) => {
            if let Some(t) = p.strip_prefix("after ") {
                let t = t.trim();
                if phext::parse_coord(t).is_some() {
                    return PinKind::After(t.to_string());
                }
            }
            PinKind::None
        }
        None => PinKind::None,
    }
}

/// The queue's full order (spec §5 + §2.1): paused-auto
/// items pin to the head, flagged, always; `pin: head`
/// items follow, rendered as the pins they are (§5.2);
/// `pin: after <address>` items sit immediately after
/// their target; everything else takes its calculated
/// position. A pin whose target is not in this rendering
/// keeps its calculated position — the pin still renders,
/// so the anomaly is visible rather than silent.
pub fn order_items(items: Vec<Item>, daily: Option<&str>) -> Vec<Item> {
    let mut keyed = items;
    keyed.sort_by(|a, b| cmp_calculated(a, b, daily));
    let (auto, rest): (Vec<Item>, Vec<Item>) =
        keyed.into_iter().partition(|i| i.state == Some(ItemState::PausedAuto));
    let (pins, body): (Vec<Item>, Vec<Item>) = rest.into_iter().partition(|i| pin_kind(i) == PinKind::Head);
    let mut out: Vec<Item> = auto;
    out.extend(pins);
    let mut deferred: Vec<(String, Item)> = Vec::new();
    for it in body {
        match pin_kind(&it) {
            PinKind::After(t) => deferred.push((t, it)),
            _ => out.push(it),
        }
    }
    let mut last_pos: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (target, it) in deferred {
        let start = match last_pos.get(&target) {
            Some(p) => Some(*p),
            None => out.iter().position(|x| phext::fmt_coord(&x.addr) == target),
        };
        match start {
            Some(p) => {
                out.insert(p + 1, it);
                last_pos.insert(target, p + 1);
            }
            None => out.push(it),
        }
    }
    out
}

/// An item's age at read time, from its `enqueued:` stamp:
/// "3d 4h", "5h 12m", "9m". Unreckonable stamps render as
/// the stamp itself — visible, never invented.
fn age_text(item: &Item) -> String {
    match item.enqueued_min {
        Some(m) => {
            let diff = (crate::core::now_minutes() - m).max(0);
            let (d, h, mm) = (diff / 1440, (diff % 1440) / 60, diff % 60);
            if d > 0 {
                format!("age {}d {}h", d, h)
            } else if h > 0 {
                format!("age {}h {}m", h, mm)
            } else {
                format!("age {}m", mm)
            }
        }
        None => format!("enqueued {}", item.enqueued),
    }
}

fn roi_text(item: &Item) -> String {
    match (item.value, item.cost) {
        (Some(v), Some(c)) => format!("value {} / cost {} (roi {:.2})", v, c, v as f64 / c as f64),
        _ => "roi: unestimated".to_string(),
    }
}

fn render_queue_line(item: &Item, daily: Option<&str>) -> String {
    let mut line = format!(
        "{} — {} — {} — goal: {} ({}) — {} — {} — failures {} — attempts {}",
        phext::fmt_coord(&item.addr),
        item.state_raw,
        item.title,
        item.goal,
        goal_class_name(goal_class(item, daily)),
        roi_text(item),
        age_text(item),
        item.failures,
        item.attempts
    );
    if let Some(p) = &item.pin {
        line.push_str(&format!(" — pin: {}", p));
    }
    if item.state == Some(ItemState::PausedAuto) {
        line.push_str(" — PAUSED-AUTO: pinned to head");
    }
    if item.register_writeback.as_deref() == Some("pending") {
        line.push_str(" — register write-back pending");
    }
    line
}

pub fn queue(all: bool, node: Option<String>, explain: Option<String>) -> i32 {
    if let Some(addr_s) = explain {
        return explain_item(&addr_s);
    }
    let lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let daily = daily_goal(&lat.px);
    let mut items = load_items(&lat.px);
    if let Some(want) = &node {
        items.retain(|i| i.enqueued_by.split_whitespace().next().unwrap_or("") == want.as_str());
    }
    if !all {
        items.retain(|i| !matches!(i.state, Some(ItemState::Done) | Some(ItemState::Cancelled)));
    }
    let ordered = order_items(items, daily.as_deref());
    println!("queue: {} item(s)", ordered.len());
    for it in &ordered {
        println!("{}", render_queue_line(it, daily.as_deref()));
    }
    0
}

/// `qexo queue --explain <item-address>` (spec §2.1, §5):
/// the item's sort keys, its calculated position, and its
/// re-order audit trail.
fn explain_item(addr_s: &str) -> i32 {
    let (universe, coord) = match phext::parse_universe_addr(addr_s) {
        Some(x) => x,
        None => {
            eprintln!("qexo queue --explain: '{}' is not a full 9D address", addr_s);
            return 2;
        }
    };
    if universe != 1 {
        eprintln!("qexo queue --explain: queue items live in universe 1 — ${} names an attempt universe", universe);
        return 2;
    }
    let lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let items = load_items(&lat.px);
    let item = match items.iter().find(|i| i.addr == coord) {
        Some(i) => i.clone(),
        None => {
            eprintln!("qexo queue --explain: no queue item at {}", addr_s);
            return 1;
        }
    };
    let daily = daily_goal(&lat.px);
    println!("item: {}", phext::fmt_coord(&item.addr));
    println!("title: {}", item.title);
    println!("state: {}", item.state_raw);
    println!(
        "goal: {} (class: {})",
        item.goal,
        goal_class_name(goal_class(&item, daily.as_deref()))
    );
    match &daily {
        Some(d) => println!("daily goal on this node: {}", d),
        None => println!("daily goal on this node: (none set)"),
    }
    println!("roi: {}", roi_text(&item));
    println!("enqueued: {} ({})", item.enqueued, age_text(&item));
    println!("attempts: {} — failures: {}", item.attempts, item.failures);
    match &item.pin {
        Some(p) => println!("pin: {}", p),
        None => println!("pin: none"),
    }
    let working: Vec<Item> = items
        .into_iter()
        .filter(|i| !matches!(i.state, Some(ItemState::Done) | Some(ItemState::Cancelled)))
        .collect();
    let ordered = order_items(working, daily.as_deref());
    match ordered.iter().position(|i| i.addr == coord) {
        Some(p) => println!("position: {} of {} in the calculated order", p + 1, ordered.len()),
        None => println!("position: terminal — outside the working order"),
    }
    let audits = load_audits(&lat.px);
    let trail: Vec<&AuditRec> = audits
        .iter()
        .filter(|a| a.kind == "reorder" && a.item.as_deref() == Some(addr_s))
        .collect();
    if trail.is_empty() {
        println!("re-order audit trail: none recorded");
    } else {
        println!("re-order audit trail:");
        for a in trail {
            println!(
                "  {} — {} — checked {} — showed {} — verdict {}",
                a.at,
                a.actor.clone().unwrap_or_default(),
                a.checked.clone().unwrap_or_default(),
                a.showed.clone().unwrap_or_default(),
                a.verdict.clone().unwrap_or_default()
            );
        }
    }
    0
}

/// The item Book an address names, when the address is a
/// queue item record's own address (11.11.11/2.1.<book>/
/// 1.1.1). Anything else is a named refusal, never a guess.
fn item_book(addr_s: &str) -> Result<u32, String> {
    let coord = phext::parse_coord(addr_s)
        .ok_or_else(|| format!("'{}' is not a full 9D address", addr_s))?;
    if coord[0..3] == [11, 11, 11]
        && coord[3] == lattice::DISPATCH_ITEMS_C
        && coord[4] == 1
        && coord[6..9] == [1, 1, 1]
    {
        Ok(coord[5])
    } else {
        Err(format!("'{}' is not a queue item address (11.11.11/2.1.<book>/1.1.1)", addr_s))
    }
}

/// Parse a transition verb's tail: one item address, plus
/// `--note <text>` where the verb takes one.
fn parse_item_verb(verb: &str, args: &[String], allow_note: bool) -> Result<(String, Option<String>), String> {
    let mut addr: Option<String> = None;
    let mut note: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--note" {
            if !allow_note {
                return Err(format!("qexo {} takes no --note", verb));
            }
            if i + 1 >= args.len() {
                return Err("--note needs a value".into());
            }
            note = Some(args[i + 1].clone());
            i += 2;
        } else if a.starts_with("--") {
            return Err(format!("unknown flag {}", a));
        } else if addr.is_none() {
            addr = Some(a.clone());
            i += 1;
        } else {
            return Err(format!("qexo {} takes exactly one item address", verb));
        }
    }
    match addr {
        Some(a) => Ok((a, note)),
        None => Err(format!("usage: qexo {} <item-address>{}", verb, if allow_note { " [--note <text>]" } else { "" })),
    }
}

/// Run one transition verb end to end: validate, apply
/// (state + receipt + audit in the document together),
/// save once, and only then ack — printing the receipt's
/// address as the proof the transition is durable (§4.1).
fn run_transition(verb: &str, past: &str, args: &[String], allow_note: bool, build: impl Fn(&str, Option<String>, &Lattice) -> Result<Transition, String>) -> i32 {
    let (addr_s, note) = match parse_item_verb(verb, args, allow_note) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("qexo {}: {}", verb, e);
            return 2;
        }
    };
    let book = match item_book(&addr_s) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("qexo {}: {}", verb, e);
            return 2;
        }
    };
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let t = match build(&addr_s, note, &lat) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("qexo {}: {}", verb, e);
            return 2;
        }
    };
    match apply_transition(&mut lat, book, &t) {
        Ok(receipt) => {
            if let Err(e) = lat.save() {
                eprintln!("qexo {}: cannot save state: {}", verb, e);
                return 1;
            }
            println!("{} {} — receipt {}", past, addr_s, phext::fmt_coord(&receipt));
            0
        }
        Err(e) => {
            eprintln!("qexo {}: {}", verb, e);
            1
        }
    }
}

pub fn pause(args: &[String]) -> i32 {
    run_transition("pause", "paused", args, true, |_addr, note, lat| {
        let mut t = Transition::new(ItemState::Paused, Actor::Manual, &local_actor(&lat.px));
        t.evidence = note.unwrap_or_default();
        t.manual = true;
        Ok(t)
    })
}

pub fn resume(args: &[String]) -> i32 {
    run_transition("resume", "resumed", args, false, |_addr, _note, lat| {
        let mut t = Transition::new(ItemState::Queued, Actor::Manual, &local_actor(&lat.px));
        t.manual = true; // a CLI resume is a human act — the only kind paused-auto yields to
        Ok(t)
    })
}

pub fn cancel(args: &[String]) -> i32 {
    let code = run_transition("cancel", "cancelled", args, true, |_addr, note, lat| {
        match note {
            Some(n) if !n.trim().is_empty() => {
                let mut t = Transition::new(ItemState::Cancelled, Actor::WillEmise, &local_actor(&lat.px));
                t.evidence = n;
                Ok(t)
            }
            _ => Err("cancel requires --note <text> — the note is the receipt's evidence".into()),
        }
    });
    if code != 0 {
        return code;
    }
    // §2.1/§10.4: cancelling a register-numbered item is
    // the act that writes the register resolution. The
    // close above is durable (its receipt proves it); the
    // write-back is attempted now and its outcome is
    // printed either way. A failure leaves the record's
    // `register-writeback: pending` standing — the next
    // drain's sweep retries it, and the queue renders it.
    let (addr_s, note) = match parse_item_verb("cancel", args, true) {
        Ok(x) => x,
        Err(_) => return code,
    };
    let book = match item_book(&addr_s) {
        Ok(b) => b,
        Err(_) => return code,
    };
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let items = load_items(&lat.px);
    let item = match items.iter().find(|i| i.book == book) {
        Some(i) => i.clone(),
        None => return code,
    };
    let number = match &item.register {
        Some(n) if item.register_writeback.as_deref() == Some("pending") => n.clone(),
        _ => return code,
    };
    match crate::register::write_back(&mut lat, &item, "cancelled", note.as_deref()) {
        Ok(seq) => {
            println!(
                "register write-back: resolution seq {} recorded for register #{} (credential: qexo-token (env))",
                seq, number
            );
            0
        }
        Err(e) => {
            eprintln!(
                "qexo cancel: {} — the local close is durable; the write-back stays pending",
                e
            );
            1
        }
    }
}

const USAGE_AUDIT: &str = "usage: qexo audit [--item <address>] [--kind <kind>] [--since <date>] | qexo audit show <address>";

/// `qexo audit` (spec §2.1, §7.2): Will's window on the
/// audit cluster — list with filters, newest last (ledger
/// order); `audit show` renders one in full. If it isn't
/// in the audit cluster, it didn't happen quietly.
pub fn audit(args: &[String]) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("show") {
        let addr_s = match args.get(1) {
            Some(a) => a.clone(),
            None => {
                eprintln!("qexo audit: {}\n{}", "show needs an address", USAGE_AUDIT);
                return 2;
            }
        };
        let coord = match phext::parse_coord(&addr_s) {
            Some(c) => c,
            None => {
                eprintln!("qexo audit show: '{}' is not a full 9D address", addr_s);
                return 2;
            }
        };
        let lat = match require_born() {
            Ok(l) => l,
            Err(code) => return code,
        };
        // `audit show` shows an audit record by its ledger
        // address (§2.1) — not any scroll that happens to
        // sit at the address given.
        let audits = load_audits(&lat.px);
        return match audits.iter().find(|a| a.addr == coord) {
            Some(_) => {
                let body = lat.px.get(&coord).cloned().unwrap_or_default();
                print!("{}", body);
                if !body.ends_with('\n') {
                    println!();
                }
                0
            }
            None => {
                eprintln!("qexo audit show: no audit record at {}", addr_s);
                1
            }
        };
    }
    let mut item_f: Option<String> = None;
    let mut kind_f: Option<String> = None;
    let mut since_f: Option<String> = None;
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
        let r = match a.as_str() {
            "--item" => take(&mut item_f),
            "--kind" => take(&mut kind_f),
            "--since" => take(&mut since_f),
            _ => Err(format!("unknown flag {}", a)),
        };
        if let Err(e) = r {
            eprintln!("qexo audit: {}\n{}", e, USAGE_AUDIT);
            return 2;
        }
    }
    let lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let audits = load_audits(&lat.px);
    let shown: Vec<&AuditRec> = audits
        .iter()
        .filter(|a| item_f.as_deref().map(|w| a.item.as_deref() == Some(w)).unwrap_or(true))
        .filter(|a| kind_f.as_deref().map(|w| a.kind == w).unwrap_or(true))
        .filter(|a| since_f.as_deref().map(|w| a.at.get(..10).unwrap_or("") >= w).unwrap_or(true))
        .collect();
    println!("audit: {} record(s)", shown.len());
    for a in shown {
        println!(
            "{} — {} — item {} — {} — {}",
            phext::fmt_coord(&a.addr),
            a.kind,
            a.item.clone().unwrap_or_else(|| "(none)".into()),
            a.actor.clone().unwrap_or_default(),
            a.at
        );
    }
    0
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

    // --------------------------------------------------------
    // Slice 2: the state machine, the ordering, the writes.
    // --------------------------------------------------------

    fn record_in_state(state: &str, failures: u32) -> String {
        let t = item_record(
            "Do the thing",
            Some("26.5.9/7.6.11/5.5.10"),
            None,
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            Some(7),
            Some(2),
            None,
            None,
        );
        let t = lattice::set_field(&t, "state", state);
        lattice::set_field(&t, "failures", &failures.to_string())
    }

    fn item_in(state: &str, failures: u32) -> Item {
        parse_item(1, &record_in_state(state, failures))
    }

    fn tr(to: ItemState, actor: Actor) -> Transition {
        Transition::new(to, actor, "test-node TESTFP")
    }

    #[test]
    fn legal_transitions_are_accepted() {
        // queued → dispatched (dispatch verb / coordinator)
        assert!(check_transition(&item_in("queued", 0), &tr(ItemState::Dispatched, Actor::Dispatch)).is_ok());
        assert!(check_transition(&item_in("queued", 0), &tr(ItemState::Dispatched, Actor::Coordinator)).is_ok());
        // dispatched → running (executor's first heartbeat)
        assert!(check_transition(&item_in("dispatched", 0), &tr(ItemState::Running, Actor::Executor)).is_ok());
        // running → running (checkpoint)
        assert!(check_transition(&item_in("running", 0), &tr(ItemState::Running, Actor::Executor)).is_ok());
        // running → done (coordinator, F4 evidence complete)
        let mut done = tr(ItemState::Done, Actor::Coordinator);
        done.checked = Some("repo wbic16/quarterexo".into());
        done.showed = Some("PR merged".into());
        done.verdict = Some("closed-settled".into());
        assert!(check_transition(&item_in("running", 0), &done).is_ok());
        // failure: running/dispatched → queued with evidence
        let mut fail = tr(ItemState::Queued, Actor::Coordinator);
        fail.evidence = "executor died".into();
        assert!(check_transition(&item_in("running", 0), &fail).is_ok());
        assert!(check_transition(&item_in("dispatched", 0), &fail).is_ok());
        // queued → waiting with a register cite supplied
        let mut wait = tr(ItemState::Waiting, Actor::Coordinator);
        wait.register = Some("181".into());
        assert!(check_transition(&item_in("queued", 0), &wait).is_ok());
        // waiting → queued with the resolution as evidence
        let mut unwait = tr(ItemState::Queued, Actor::WillEmise);
        unwait.evidence = "register #181 resolved: merged".into();
        assert!(check_transition(&item_in("waiting", 0), &unwait).is_ok());
        // manual pause from every non-terminal working state
        for s in ["queued", "dispatched", "running", "waiting"] {
            assert!(check_transition(&item_in(s, 0), &tr(ItemState::Paused, Actor::Manual)).is_ok(), "pause from {}", s);
        }
        // resume: paused → queued; paused-auto → queued (manual)
        assert!(check_transition(&item_in("paused", 2), &tr(ItemState::Queued, Actor::Manual)).is_ok());
        let mut mres = tr(ItemState::Queued, Actor::Manual);
        mres.manual = true;
        assert!(check_transition(&item_in("paused-auto", 3), &mres).is_ok());
        // cancel from any non-terminal state, Will/Emise + note
        let mut cancel = tr(ItemState::Cancelled, Actor::WillEmise);
        cancel.evidence = "moot — superseded".into();
        for s in ["queued", "dispatched", "running", "waiting", "paused", "paused-auto"] {
            assert!(check_transition(&item_in(s, 0), &cancel).is_ok(), "cancel from {}", s);
        }
    }

    #[test]
    fn illegal_transitions_are_refused() {
        let cases: Vec<(Item, Transition)> = vec![
            (item_in("queued", 0), tr(ItemState::Running, Actor::Executor)), // skips dispatched
            (item_in("queued", 0), tr(ItemState::Done, Actor::Coordinator)), // skips everything
            (item_in("queued", 0), tr(ItemState::Queued, Actor::Manual)),    // no self-transition
            (item_in("queued", 0), tr(ItemState::PausedAuto, Actor::Coordinator)), // only via failure #3
            (item_in("dispatched", 0), tr(ItemState::Done, Actor::Coordinator)),   // must run first
            (item_in("dispatched", 0), tr(ItemState::PausedAuto, Actor::Coordinator)),
            (item_in("running", 0), tr(ItemState::Waiting, Actor::Coordinator)),   // waiting is from queued
            (item_in("running", 0), tr(ItemState::Dispatched, Actor::Dispatch)),   // no way back
            (item_in("waiting", 0), tr(ItemState::Dispatched, Actor::Dispatch)),   // must requeue first
            (item_in("waiting", 0), tr(ItemState::Running, Actor::Executor)),
            (item_in("paused", 0), tr(ItemState::Running, Actor::Executor)),       // must resume first
            (item_in("paused", 0), tr(ItemState::Dispatched, Actor::Dispatch)),
            (item_in("paused", 0), tr(ItemState::Done, Actor::Coordinator)),
            (item_in("paused-auto", 3), tr(ItemState::Paused, Actor::Manual)),      // auto-pause exits only via resume
            (item_in("paused-auto", 3), tr(ItemState::Dispatched, Actor::Dispatch)),
            (item_in("done", 0), tr(ItemState::Queued, Actor::Manual)),            // terminal is final
            (item_in("cancelled", 0), tr(ItemState::Queued, Actor::Manual)),
            (item_in("done", 0), tr(ItemState::Cancelled, Actor::WillEmise)),
        ];
        for (item, t) in &cases {
            assert!(
                check_transition(item, t).is_err(),
                "{} → {} must be refused",
                item.state_raw,
                t.to.name()
            );
        }
    }

    #[test]
    fn actor_and_evidence_rules_are_checked_not_trusted() {
        // cancel by an executor or a manual actor: refused
        let mut cancel = tr(ItemState::Cancelled, Actor::Executor);
        cancel.evidence = "note".into();
        assert!(check_transition(&item_in("queued", 0), &cancel).is_err());
        let mut cancel2 = tr(ItemState::Cancelled, Actor::Manual);
        cancel2.evidence = "note".into();
        assert!(check_transition(&item_in("queued", 0), &cancel2).is_err());
        // cancel without a note: refused
        assert!(check_transition(&item_in("queued", 0), &tr(ItemState::Cancelled, Actor::WillEmise)).is_err());
        // done by the executor alone: refused (needs the coordinator's verification)
        let mut done = tr(ItemState::Done, Actor::Executor);
        done.checked = Some("x".into());
        done.showed = Some("y".into());
        done.verdict = Some("z".into());
        assert!(check_transition(&item_in("running", 0), &done).is_err());
        // done with an incomplete F4 shape: refused
        let mut done2 = tr(ItemState::Done, Actor::Coordinator);
        done2.checked = Some("x".into());
        assert!(check_transition(&item_in("running", 0), &done2).is_err());
        // dispatch by an executor: refused
        assert!(check_transition(&item_in("queued", 0), &tr(ItemState::Dispatched, Actor::Executor)).is_err());
        // failure without evidence: refused
        assert!(check_transition(&item_in("running", 0), &tr(ItemState::Queued, Actor::Coordinator)).is_err());
        // waiting with no register cite anywhere: refused
        assert!(check_transition(&item_in("queued", 0), &tr(ItemState::Waiting, Actor::Coordinator)).is_err());
        // waiting → queued without the resolution evidence: refused
        assert!(check_transition(&item_in("waiting", 0), &tr(ItemState::Queued, Actor::WillEmise)).is_err());
        // paused-auto with an automatic (non-manual) resume: refused
        assert!(check_transition(&item_in("paused-auto", 3), &tr(ItemState::Queued, Actor::Manual)).is_err());
        // an item in a corrupted state accepts nothing
        assert!(check_transition(&item_in("sideways", 0), &tr(ItemState::Queued, Actor::Manual)).is_err());
    }

    #[test]
    fn dispatch_counts_the_attempt_failure_counts_the_failure() {
        let p = check_transition(&item_in("queued", 0), &tr(ItemState::Dispatched, Actor::Dispatch)).unwrap();
        assert_eq!(p.attempts, 1);
        assert_eq!(p.failures, 0);
        let mut fail = tr(ItemState::Queued, Actor::Coordinator);
        fail.evidence = "boom".into();
        let p = check_transition(&item_in("running", 1), &fail).unwrap();
        assert_eq!(p.to, ItemState::Queued);
        assert_eq!(p.failures, 2);
        // the third consecutive failure lands in paused-auto
        let p = check_transition(&item_in("running", 2), &fail).unwrap();
        assert_eq!(p.to, ItemState::PausedAuto);
        assert_eq!(p.failures, 3);
        assert_eq!(p.audit, Some(AuditOwed::AutoPause));
        // a manual resume resets the counter (§8)
        let mut mres = tr(ItemState::Queued, Actor::Manual);
        mres.manual = true;
        let p = check_transition(&item_in("paused-auto", 3), &mres).unwrap();
        assert_eq!(p.failures, 0);
        assert_eq!(p.audit, Some(AuditOwed::Override));
    }

    fn born_lat_with_item(state: &str, failures: u32) -> (Lattice, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("qexo-dispatch-test-{}-{}", std::process::id(), state));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state.phext");
        let mut lat = Lattice::born(path.clone());
        lat.px.insert(item_addr(1), record_in_state(state, failures));
        (lat, path)
    }

    #[test]
    fn a_transition_and_its_receipt_are_one_write() {
        let (mut lat, _path) = born_lat_with_item("queued", 0);
        let mut t = Transition::new(ItemState::Paused, Actor::Manual, "test-node TESTFP");
        t.evidence = "after the release".into();
        let receipt = apply_transition(&mut lat, 1, &t).unwrap();
        // the record moved
        let rec = lat.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec, "state").as_deref(), Some("paused"));
        // and the receipt exists in the same document, citing it
        let receipts = load_transition_receipts(&lat.px);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].addr, receipt);
        assert_eq!(receipts[0].item.as_deref(), Some("11.11.11/2.1.1/1.1.1"));
        assert_eq!(receipts[0].from.as_deref(), Some("queued"));
        assert_eq!(receipts[0].to.as_deref(), Some("paused"));
        assert_eq!(receipts[0].actor.as_deref(), Some("test-node TESTFP"));
        assert_eq!(receipts[0].evidence.as_deref(), Some("after the release"));
        // a refused transition writes neither half
        let bad = Transition::new(ItemState::Running, Actor::Executor, "test-node TESTFP");
        assert!(apply_transition(&mut lat, 1, &bad).is_err());
        assert_eq!(load_transition_receipts(&lat.px).len(), 1);
        assert_eq!(lattice::field(lat.px.get(&item_addr(1)).unwrap(), "state").as_deref(), Some("paused"));
    }

    #[test]
    fn the_failure_saga_lands_in_paused_auto_with_its_audit() {
        let (mut lat, _path) = born_lat_with_item("queued", 0);
        let dispatch = |lat: &mut Lattice| {
            apply_transition(lat, 1, &Transition::new(ItemState::Dispatched, Actor::Dispatch, "coord FP")).unwrap();
            apply_transition(lat, 1, &Transition::new(ItemState::Running, Actor::Executor, "exec FP")).unwrap();
        };
        let fail = |lat: &mut Lattice, ev: &str| {
            let mut t = Transition::new(ItemState::Queued, Actor::Coordinator, "coord FP");
            t.evidence = ev.into();
            apply_transition(lat, 1, &t).unwrap();
        };
        dispatch(&mut lat);
        fail(&mut lat, "boom 1");
        dispatch(&mut lat);
        fail(&mut lat, "boom 2");
        dispatch(&mut lat);
        fail(&mut lat, "boom 3");
        let rec = lat.px.get(&item_addr(1)).unwrap().clone();
        assert_eq!(lattice::field(&rec, "state").as_deref(), Some("paused-auto"));
        assert_eq!(lattice::field(&rec, "failures").as_deref(), Some("3"));
        assert_eq!(lattice::field(&rec, "attempts").as_deref(), Some("3"));
        // the auto-pause audit cites all three failures' evidence
        let audits = load_audits(&lat.px);
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].kind, "auto-pause");
        let showed = audits[0].showed.clone().unwrap_or_default();
        assert!(showed.contains("boom 1") && showed.contains("boom 2") && showed.contains("boom 3"), "{}", showed);
        // an automatic resume is refused; a manual one lands,
        // resets the counter, and writes the override audit
        let auto = Transition::new(ItemState::Queued, Actor::Manual, "coord FP");
        assert!(apply_transition(&mut lat, 1, &auto).is_err());
        let mut manual = Transition::new(ItemState::Queued, Actor::Manual, "test-node TESTFP");
        manual.manual = true;
        apply_transition(&mut lat, 1, &manual).unwrap();
        let rec = lat.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec, "state").as_deref(), Some("queued"));
        assert_eq!(lattice::field(rec, "failures").as_deref(), Some("0"));
        let audits = load_audits(&lat.px);
        assert_eq!(audits.len(), 2);
        assert_eq!(audits[1].verdict.as_deref(), Some("resumed"));
    }

    #[test]
    fn done_writes_its_audit_in_the_same_save() {
        let (mut lat, _path) = born_lat_with_item("running", 0);
        let mut t = Transition::new(ItemState::Done, Actor::Coordinator, "coord FP");
        t.checked = Some("github PR #29".into());
        t.showed = Some("merged at 0c845ab".into());
        t.verdict = Some("closed-settled".into());
        apply_transition(&mut lat, 1, &t).unwrap();
        let audits = load_audits(&lat.px);
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].kind, "non-ping-close");
        assert_eq!(audits[0].item.as_deref(), Some("11.11.11/2.1.1/1.1.1"));
        let receipts = load_transition_receipts(&lat.px);
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].evidence.clone().unwrap_or_default().contains("PR #29"));
    }

    #[test]
    fn waiting_sets_the_register_cite_on_the_record() {
        let (mut lat, _path) = born_lat_with_item("queued", 0);
        let mut t = Transition::new(ItemState::Waiting, Actor::Coordinator, "coord FP");
        t.register = Some("181".into());
        apply_transition(&mut lat, 1, &t).unwrap();
        let rec = lat.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec, "state").as_deref(), Some("waiting"));
        assert_eq!(lattice::field(rec, "register").as_deref(), Some("181"));
    }

    // --------------------------------------------------------
    // Slice 3: the register close (§10.3) and write-back
    // bookkeeping (§10.4).
    // --------------------------------------------------------

    fn record_with_register(state: &str, number: &str) -> String {
        let t = item_record(
            "Do the thing",
            None,
            Some(number),
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            None,
            None,
            None,
            None,
        );
        lattice::set_field(&t, "state", state)
    }

    fn register_close(to: ItemState, actor: Actor) -> Transition {
        let mut t = Transition::new(to, actor, "test-node TESTFP");
        t.register_close = true;
        t.evidence = "register resolution seq 4".into();
        t.checked = Some("register resolution seq 4 (register link)".into());
        t.showed = Some("the register records the resolution".into());
        t.verdict = Some("closed-settled".into());
        t
    }

    #[test]
    fn a_register_resolution_closes_an_item_where_it_stands() {
        // queued → done is refused on every other road (the
        // matrix has no such row); the register's resolution
        // is the one authority that closes it (§10.3).
        for s in ["queued", "waiting", "paused", "paused-auto", "dispatched", "running"] {
            let item = parse_item(1, &record_with_register(s, "42"));
            assert!(
                check_transition(&item, &register_close(ItemState::Done, Actor::Coordinator)).is_ok(),
                "register close from {}",
                s
            );
            assert!(
                check_transition(&item, &register_close(ItemState::Cancelled, Actor::Coordinator)).is_ok(),
                "register cancel from {}",
                s
            );
        }
        // and the same request WITHOUT the register-close
        // flag keeps the matrix's refusal — the flag is the
        // whole difference.
        let item = parse_item(1, &record_with_register("queued", "42"));
        let mut plain_done = register_close(ItemState::Done, Actor::Coordinator);
        plain_done.register_close = false;
        assert!(check_transition(&item, &plain_done).is_err());
    }

    #[test]
    fn register_close_refusals_name_their_rule() {
        let item = parse_item(1, &record_with_register("queued", "42"));
        // only done/cancelled landings
        assert!(check_transition(&item, &register_close(ItemState::Queued, Actor::Coordinator)).is_err());
        // the coordinator's act alone (the drain)
        assert!(check_transition(&item, &register_close(ItemState::Done, Actor::Executor)).is_err());
        assert!(check_transition(&item, &register_close(ItemState::Done, Actor::WillEmise)).is_err());
        // the resolution cite is required evidence
        let mut no_evidence = register_close(ItemState::Done, Actor::Coordinator);
        no_evidence.evidence = String::new();
        assert!(check_transition(&item, &no_evidence).is_err());
        // the F4 shape is required (§7.2)
        let mut no_f4 = register_close(ItemState::Cancelled, Actor::Coordinator);
        no_f4.verdict = None;
        assert!(check_transition(&item, &no_f4).is_err());
        // terminal items refuse even the register's word
        let done_item = parse_item(1, &record_with_register("done", "42"));
        assert!(check_transition(&done_item, &register_close(ItemState::Cancelled, Actor::Coordinator)).is_err());
    }

    #[test]
    fn a_register_close_writes_its_audit_and_settles_the_writeback_field() {
        let dir = std::env::temp_dir().join(format!("qexo-dispatch-test-{}-regclose", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut lat = Lattice::born(dir.join("state.phext"));
        lat.px.insert(item_addr(1), record_with_register("queued", "42"));
        apply_transition(&mut lat, 1, &register_close(ItemState::Done, Actor::Coordinator)).unwrap();
        let rec = lat.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec, "state").as_deref(), Some("done"));
        assert_eq!(lattice::field(rec, "register-writeback").as_deref(), Some("recorded"));
        let audits = load_audits(&lat.px);
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].kind, "register-close");
        assert_eq!(audits[0].item.as_deref(), Some("11.11.11/2.1.1/1.1.1"));
        assert!(audits[0].checked.clone().unwrap_or_default().contains("seq 4"));
        let receipts = load_transition_receipts(&lat.px);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].to.as_deref(), Some("done"));
    }

    #[test]
    fn a_local_close_of_a_register_item_marks_the_writeback_pending() {
        let dir = std::env::temp_dir().join(format!("qexo-dispatch-test-{}-wbpend", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut lat = Lattice::born(dir.join("state.phext"));
        lat.px.insert(item_addr(1), record_with_register("queued", "42"));
        let mut t = Transition::new(ItemState::Cancelled, Actor::WillEmise, "test-node TESTFP");
        t.evidence = "moot".into();
        apply_transition(&mut lat, 1, &t).unwrap();
        let rec = lat.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec, "state").as_deref(), Some("cancelled"));
        assert_eq!(lattice::field(rec, "register-writeback").as_deref(), Some("pending"));
        // an item with no register number owes no write-back
        let (mut lat2, _p) = born_lat_with_item("queued", 0);
        apply_transition(&mut lat2, 1, &t).unwrap();
        let rec2 = lat2.px.get(&item_addr(1)).unwrap();
        assert_eq!(lattice::field(rec2, "register-writeback"), None);
    }

    fn bare_item(book: u32, goal: &str, value: Option<u32>, cost: Option<u32>, enqueued: &str, state: &str, pin: Option<&str>) -> Item {
        let mut fields: Vec<(&str, &str)> = vec![
            ("title", "T"),
            ("origin", "emise"),
            ("enqueued", enqueued),
            ("enqueued-by", "test-node TESTFP"),
            ("state", state),
            ("goal", goal),
            ("attempts", "0"),
            ("failures", "0"),
        ];
        let vs = value.map(|v| v.to_string());
        let cs = cost.map(|c| c.to_string());
        if let Some(v) = &vs {
            fields.push(("value", v));
        }
        if let Some(c) = &cs {
            fields.push(("cost", c));
        }
        if let Some(p) = pin {
            fields.push(("pin", p));
        }
        let mut text = String::from("queue item\n");
        text.push_str(&lattice::record(&fields));
        parse_item(book, &text)
    }

    #[test]
    fn the_calculated_order_is_class_then_ratio_then_age() {
        let daily = Some("ship the kit");
        let items = vec![
            bare_item(1, "standing", Some(9), Some(1), "2026-11-16 09:00 UTC", "queued", None),
            bare_item(2, "ship the kit", Some(1), Some(9), "2026-11-16 10:00 UTC", "queued", None),
            bare_item(3, "standing", Some(3), Some(1), "2026-11-16 08:00 UTC", "queued", None),
            bare_item(4, "standing", None, None, "2026-11-16 07:00 UTC", "queued", None),
            bare_item(5, "other work", Some(9), Some(1), "2026-11-16 06:00 UTC", "queued", None),
            bare_item(6, "other work", None, None, "2026-11-16 06:30 UTC", "paused-auto", None),
            bare_item(7, "other work", None, None, "2026-11-16 06:45 UTC", "queued", Some("head")),
            bare_item(8, "other work", None, None, "2026-11-16 06:50 UTC", "queued", Some("after 11.11.11/2.1.1/1.1.1")),
        ];
        let ordered = order_items(items, daily);
        let books: Vec<u32> = ordered.iter().map(|i| i.book).collect();
        // paused-auto pins the head; the explicit head pin
        // follows; then the calculation: daily class first
        // (book 2, despite the worst ratio), standing by
        // ratio (9 before 3), the after-pin riding book 1,
        // unestimated below estimated within class, and the
        // background item last.
        assert_eq!(books, vec![6, 7, 2, 1, 8, 3, 4, 5]);
    }

    #[test]
    fn audit_records_roundtrip_and_sha_shape() {
        let mut px = phext::Phext::new();
        let addr = append_audit(&mut px, "reorder", "11.11.11/2.1.4/1.1.1", "the queue", "keys changed", "confirmed-open", "emise");
        assert_eq!(phext::fmt_coord(&addr), "6.6.6/9.1.1/1.1.1");
        let audits = load_audits(&px);
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].kind, "reorder");
        assert_eq!(audits[0].item.as_deref(), Some("11.11.11/2.1.4/1.1.1"));
        assert!(is_sha256_hex("b2025be90de1cd3e668a1e36dfdd15527d853a5a7ab14283d6a5e78dc7e912da"));
        assert!(!is_sha256_hex("b2025be9"));
        assert!(!is_sha256_hex("zzzz5be90de1cd3e668a1e36dfdd15527d853a5a7ab14283d6a5e78dc7e912da00"));
    }
}
