//! Lattice — access to the node's one state file, addressed per
//! Layer 2 (docs/LAYER2.md, v2).
//!
//! The rules this module implements, from the corrected spec:
//! - Addresses are PLACES, chosen for meaning, and a coordinate
//!   is always 9D — all nine components, every time; there are
//!   no partial addresses anywhere in this crate's output. Each
//!   state family gets a patterned, roomy homeland, named by its
//!   anchor (the full 9D address of its index scroll):
//!   1.1.1/1.1.1/1.1.1 home/index/identity, 2.2.2/2.2.2/2.2.2
//!   keys & trust, 3.3.3/3.3.3/3.3.3 peers, 4.4.4/4.4.4/4.4.4
//!   personas, 5.5.5/5.5.5/5.5.5 plan, 6.6.6/6.6.6/6.6.6 ledgers,
//!   7.7.7/7.7.7/7.7.7 mailbox & sessions, 8.8.8/8.8.8/8.8.8
//!   telemetry, 9.9.9/9.9.9/9.9.9 config & wiring. Records live
//!   at full 9D addresses in the anchor's vicinity. No
//!   hash-derived coordinates, no counters-as-addresses, no
//!   arithmetic sequence splits.
//! - A record is ONE scroll; its fields are plain text lines
//!   inside it (`key: value`), the way CYOA records carry theirs.
//!   Documents cite-able in parts take adjacent scroll runs.
//! - The home scroll (1.1.1/1.1.1/1.1.1) is a prose INDEX of the
//!   homelands; each homeland's first scroll is its own index.
//!   Records are truth; indexes are derived prose, regenerated
//!   from the records by `regen_indexes` — commands call it after
//!   every mutation, and `qexo doctor --fix` calls it to repair.
//! - Growth is by adjacency: a new record takes the next free
//!   Book in its family's cluster, beside its kin. Identity
//!   (names, fingerprints) lives as text inside the records and
//!   the index scrolls — finding is navigation plus the index.
//! - Revisions edit scrolls in place; git carries history.
//!   Retirement is a `status:` line in the record.
//!
//! The state's substance: every scrap of node state — metadata,
//! keys, grants, personas, plan, ledgers, sessions — is scrolls
//! in this one file. qexo has no other state format. (The rule,
//! stated once: no JSON anywhere in qexo's own machinery; the
//! single foreign edge that speaks it is `infer`.)

use crate::phext::{Coord, Phext};
use std::path::PathBuf;

// ----------------------------------------------------------------
// Homelands (Layer 2): [Library, Shelf, Series] prefixes.
// ----------------------------------------------------------------

pub const HL_HOME: [u32; 3] = [1, 1, 1];
pub const HL_KEYS: [u32; 3] = [2, 2, 2];
pub const HL_PEERS: [u32; 3] = [3, 3, 3];
pub const HL_PERSONAS: [u32; 3] = [4, 4, 4];
pub const HL_PLAN: [u32; 3] = [5, 5, 5];
pub const HL_LEDGERS: [u32; 3] = [6, 6, 6];
pub const HL_MAIL: [u32; 3] = [7, 7, 7];
pub const HL_TELEMETRY: [u32; 3] = [8, 8, 8];
pub const HL_CONFIG: [u32; 3] = [9, 9, 9];

/// The home scroll: prose index of the whole state file.
pub const HOME: Coord = [1, 1, 1, 1, 1, 1, 1, 1, 1];
/// Node identity record, adjacent to home in the home run.
pub const IDENTITY: Coord = [1, 1, 1, 1, 1, 1, 1, 1, 2];

/// Clusters (Collection) inside keys & trust (anchor 2.2.2/2.2.2/2.2.2), neighboring
/// runs in the order the brief names them: the node key first,
/// then persona keys, peer keys, grants, delegations, policy.
pub const KEYS_INDEX_C: u32 = 1;
pub const KEYS_NODE_C: u32 = 2;
pub const KEYS_PERSONA_C: u32 = 3;
pub const KEYS_PEER_C: u32 = 4;
pub const KEYS_GRANTS_C: u32 = 5;
pub const KEYS_DELEGATIONS_C: u32 = 6;
pub const KEYS_POLICY_C: u32 = 7;

/// The node key record and its armored public key (adjacent scroll).
pub const NODE_KEY_RECORD: Coord = [2, 2, 2, KEYS_NODE_C, 1, 1, 1, 1, 1];
pub const NODE_KEY_PUBLIC: Coord = [2, 2, 2, KEYS_NODE_C, 1, 1, 1, 1, 2];

/// Ledger kinds as clusters (Collection) inside 6.6.6.
pub const LEDGER_RECEIPTS: u32 = 2;
pub const LEDGER_TELEPORTS: u32 = 3;
pub const LEDGER_INSTANCES: u32 = 4;
pub const LEDGER_EMOTION: u32 = 5;
pub const LEDGER_GOAL_ACTIVITY: u32 = 6;
pub const LEDGER_SYNC: u32 = 7;
/// Release announcements (v1.1.0): one Book per announced release
/// — the manifest record at Scroll 1, its detached signature at
/// Scroll 2. The manifest is the phextnet's trust object: it is
/// what travels and what gets verified; the binary bytes ride
/// the wire beside the state file, never inside it.
pub const LEDGER_RELEASES: u32 = 8;

pub fn ledger_kind_name(cluster: u32) -> Option<&'static str> {
    match cluster {
        LEDGER_RECEIPTS => Some("receipts"),
        LEDGER_TELEPORTS => Some("teleports"),
        LEDGER_INSTANCES => Some("instances"),
        LEDGER_EMOTION => Some("emotion"),
        LEDGER_GOAL_ACTIVITY => Some("goal-activity"),
        LEDGER_SYNC => Some("sync-receipts"),
        LEDGER_RELEASES => Some("releases"),
        _ => None,
    }
}

/// Sessions (Collection 2) and mailbox (Collection 3) inside 7.7.7.
pub const MAIL_SESSIONS_C: u32 = 2;
pub const MAIL_BOX_C: u32 = 3;

/// Telemetry: disk observations (Collection 2) inside 8.8.8.
pub const TELEM_DISK_C: u32 = 2;

/// Plan (anchor 5.5.5/5.5.5/5.5.5): the current plan lives in Collection 2 (fields at
/// Scroll 1, the human rendering at Scroll 2, adjacent); history is
/// a run of Books in Collection 3, one Book per emitted plan.
pub const PLAN_CURRENT: Coord = [5, 5, 5, 2, 1, 1, 1, 1, 1];
pub const PLAN_CURRENT_RENDER: Coord = [5, 5, 5, 2, 1, 1, 1, 1, 2];
pub const PLAN_HISTORY_C: u32 = 3;

/// qexo's own config record: the node's settings as field lines —
/// no config file anywhere else. It lives at Collection 2 of the
/// 9.9.9/9.9.9/9.9.9 homeland; Collection 1's first scroll is the homeland
/// index, here as in every homeland (the two must never collide:
/// an index that reports on the config record cannot BE it).
pub const CONFIG_RECORD: Coord = [9, 9, 9, 2, 1, 1, 1, 1, 1];

// ----------------------------------------------------------------
// Address helpers
// ----------------------------------------------------------------

/// Compose an address from a homeland and the remaining six
/// components [Collection, Volume, Book, Chapter, Section, Scroll].
pub fn at(h: [u32; 3], rest: [u32; 6]) -> Coord {
    [h[0], h[1], h[2], rest[0], rest[1], rest[2], rest[3], rest[4], rest[5]]
}

/// A homeland's ANCHOR: the full 9D address of its index scroll —
/// the patterned address the region is named by (Layer 2 v2):
/// the homeland digit in all nine components, e.g. keys & trust
/// anchors at 2.2.2/2.2.2/2.2.2. (For the home homeland this is
/// 1.1.1/1.1.1/1.1.1 itself — the home scroll IS its anchor.)
/// Records live in the anchor's vicinity: they share the homeland
/// triple and place themselves in the remaining six components,
/// which never reach the anchor's own full-pattern address.
pub fn index_addr(h: [u32; 3]) -> Coord {
    [h[0], h[1], h[2], h[0], h[1], h[2], h[0], h[1], h[2]]
}

/// Every scroll whose address starts with `prefix`, in ascending
/// (coordinate) order — the range read of a run or a homeland.
pub fn prefix_scrolls<'a>(px: &'a Phext, prefix: &[u32]) -> Vec<(Coord, &'a String)> {
    px.scrolls
        .iter()
        .filter(|(c, _)| c[..prefix.len()] == *prefix)
        .map(|(c, t)| (*c, t))
        .collect()
}

/// Distinct values of component `idx` among scrolls under
/// `prefix`, sorted — e.g. the Books present in a cluster.
pub fn component_values(px: &Phext, prefix: &[u32], idx: usize) -> Vec<u32> {
    let mut out: Vec<u32> = prefix_scrolls(px, prefix).iter().map(|(c, _)| c[idx]).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The next free Book (>= start) in the cluster addressed by
/// [Library, Shelf, Series, Collection, Volume]: the adjacency
/// rule — a new record goes beside its kin, at the end of the
/// family's local run.
pub fn next_free_book(px: &Phext, first5: [u32; 5], start: u32) -> u32 {
    let used = component_values(px, &first5, 5);
    let mut b = start.max(1);
    while used.contains(&b) {
        b += 1;
    }
    b
}

/// The next free Collection (>= start) in a homeland: a new
/// entity cluster beside the existing ones. A cluster counts as
/// used when anything lives under [homeland, cluster, Volume 1]
/// — records and their runs all sit at Volume 1, while the
/// homeland's anchor scroll (at the full-pattern address, whose
/// Volume is the homeland digit) never masquerades as a record
/// cluster.
pub fn next_free_cluster(px: &Phext, h: [u32; 3], start: u32) -> u32 {
    let mut c = start.max(1);
    loop {
        if prefix_scrolls(px, &[h[0], h[1], h[2], c, 1]).is_empty() {
            return c;
        }
        c += 1;
    }
}

/// The next free Scroll (>= start) in the run addressed by the
/// first eight components.
pub fn next_free_scroll(px: &Phext, first8: [u32; 8], start: u32) -> Coord {
    let mut s = start.max(1);
    loop {
        let c: Coord = [first8[0], first8[1], first8[2], first8[3], first8[4], first8[5], first8[6], first8[7], s];
        if !px.scrolls.contains_key(&c) {
            return c;
        }
        s += 1;
    }
}

// ----------------------------------------------------------------
// Record fields: plain text lines inside one scroll.
// ----------------------------------------------------------------

/// Read `key` from a record scroll's `key: value` lines.
pub fn field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix(&prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Set `key` in a record scroll: replace its line if present,
/// append it if not. Edits the scroll in place, as Layer 2 asks.
pub fn set_field(text: &str, key: &str, value: &str) -> String {
    let prefix = format!("{}:", key);
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    for line in text.lines() {
        if !done && line.trim_start().starts_with(&prefix) {
            out.push(format!("{}: {}", key, value));
            done = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !done {
        out.push(format!("{}: {}", key, value));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Build a record scroll from field lines.
pub fn record(fields: &[(&str, &str)]) -> String {
    let mut s = String::new();
    for (k, v) in fields {
        s.push_str(&format!("{}: {}\n", k, v));
    }
    s
}

/// One-line shortening of a fingerprint for prose.
pub fn fp_short(fp: &str) -> String {
    if fp.len() > 16 {
        format!("{}…{}", &fp[..8], &fp[fp.len() - 8..])
    } else {
        fp.to_string()
    }
}

// ----------------------------------------------------------------
// Record enumeration (truth lives in the records; doctor and the
// index regenerator share this scan)
// ----------------------------------------------------------------

pub struct RecordRef {
    pub kind: &'static str,
    pub name: String,
    pub coord: Coord,
    pub status: String,
}

/// Enumerate peer, persona, and session records across the
/// homelands, in address order.
pub fn records(px: &Phext) -> Vec<RecordRef> {
    let mut out = Vec::new();
    for c in component_values(px, &HL_PEERS, 3) {
        if c < 2 {
            continue;
        }
        let addr = at(HL_PEERS, [c, 1, 1, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push(RecordRef {
                kind: "peer",
                name: field(t, "name").unwrap_or_default(),
                coord: addr,
                status: field(t, "status").unwrap_or_default(),
            });
        }
    }
    for c in component_values(px, &HL_PERSONAS, 3) {
        if c < 2 {
            continue;
        }
        let addr = at(HL_PERSONAS, [c, 1, 1, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push(RecordRef {
                kind: "persona",
                name: field(t, "name").unwrap_or_default(),
                coord: addr,
                status: field(t, "status").unwrap_or_default(),
            });
        }
    }
    for b in component_values(px, &[7, 7, 7, MAIL_SESSIONS_C, 1], 5) {
        let addr = at(HL_MAIL, [MAIL_SESSIONS_C, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push(RecordRef {
                kind: "session",
                name: field(t, "persona").unwrap_or_default(),
                coord: addr,
                status: field(t, "status").unwrap_or_default(),
            });
        }
    }
    out
}

// ----------------------------------------------------------------
// Index regeneration: derived prose, rebuilt from the records.
// ----------------------------------------------------------------

fn count_cluster_books(px: &Phext, first5: [u32; 5]) -> usize {
    component_values(px, &first5, 5).len()
}

/// Regenerate the home index and every homeland index scroll from
/// the records present. Called by config at birth, by commands
/// after every mutation, and by `doctor --fix` as its repair.
pub fn regen_indexes(px: &mut Phext) {
    let recs = records(px);
    let peers: Vec<&RecordRef> = recs.iter().filter(|r| r.kind == "peer").collect();
    let personas: Vec<&RecordRef> = recs.iter().filter(|r| r.kind == "persona").collect();
    let sessions: Vec<&RecordRef> = recs.iter().filter(|r| r.kind == "session" && r.status == "open").collect();

    // ---- keys & trust index ----
    let mut keys = String::new();
    keys.push_str("keys & trust — homeland anchor 2.2.2/2.2.2/2.2.2\n\n");
    keys.push_str("Two parties, a key between them: the node's own key first,\nthen persona keys, peer keys, grants, delegations, and policy,\nas neighboring runs.\n\n");
    if let Some(t) = px.get(&NODE_KEY_RECORD) {
        keys.push_str(&format!(
            "- node key {} — fingerprint {} — status {}\n",
            crate::phext::fmt_coord(&NODE_KEY_RECORD),
            field(t, "fingerprint").unwrap_or_default(),
            field(t, "status").unwrap_or_default()
        ));
    } else {
        keys.push_str("- node key: not generated yet\n");
    }
    for (c, label) in [
        (KEYS_PERSONA_C, "persona key"),
        (KEYS_PEER_C, "peer key"),
        (KEYS_GRANTS_C, "grant"),
        (KEYS_DELEGATIONS_C, "delegation"),
    ] {
        for b in component_values(px, &[2, 2, 2, c, 1], 5) {
            let addr = at(HL_KEYS, [c, 1, b, 1, 1, 1]);
            if let Some(t) = px.get(&addr) {
                keys.push_str(&format!(
                    "- {} {} — {} {}\n",
                    label,
                    crate::phext::fmt_coord(&addr),
                    field(t, "name").or_else(|| field(t, "persona")).or_else(|| field(t, "peer")).unwrap_or_default(),
                    field(t, "fingerprint").map(|f| format!("fingerprint {}", fp_short(&f))).unwrap_or_default()
                ));
            }
        }
    }
    let policy_rules = prefix_scrolls(px, &[2, 2, 2, KEYS_POLICY_C, 1, 1, 1, 1]).len();
    keys.push_str(&format!(
        "- policy: {} rule scroll(s) in the run from {}\n",
        policy_rules,
        crate::phext::fmt_coord(&at(HL_KEYS, [KEYS_POLICY_C, 1, 1, 1, 1, 1]))
    ));
    px.insert(index_addr(HL_KEYS), keys);

    // ---- peers index ----
    let mut peers_txt = String::new();
    peers_txt.push_str("peers — homeland anchor 3.3.3/3.3.3/3.3.3\n\n");
    peers_txt.push_str("Three-way handshake country: one cluster per registered peer,\nits record first and its armored public key beside it.\n\n");
    if peers.is_empty() {
        peers_txt.push_str("no peers registered yet — qexo register <pubkey> adds one.\n");
    }
    for r in &peers {
        peers_txt.push_str(&format!("- {} — {} — status {}\n", r.name, crate::phext::fmt_coord(&r.coord), r.status));
    }
    px.insert(index_addr(HL_PEERS), peers_txt);

    // ---- personas index ----
    let mut per = String::new();
    per.push_str("personas — homeland anchor 4.4.4/4.4.4/4.4.4\n\n");
    per.push_str("One cluster per persona: record, boot text as an adjacent\nscroll run, live state, and transcript.\n\n");
    if personas.is_empty() {
        per.push_str("no personas yet — qexo host <persona> brings one home.\n");
    }
    for r in &personas {
        per.push_str(&format!("- {} — {} — status {}\n", r.name, crate::phext::fmt_coord(&r.coord), r.status));
    }
    px.insert(index_addr(HL_PERSONAS), per);

    // ---- plan index ----
    let mut plan_txt = String::new();
    plan_txt.push_str("plan — homeland anchor 5.5.5/5.5.5/5.5.5\n\n");
    if let Some(t) = px.get(&PLAN_CURRENT) {
        plan_txt.push_str(&format!(
            "current plan: {} (rendering beside it, at {})\n",
            crate::phext::fmt_coord(&PLAN_CURRENT),
            crate::phext::fmt_coord(&PLAN_CURRENT_RENDER)
        ));
        plan_txt.push_str(&format!(
            "emitted: {}\ndisk gate: {}\nopen tasks: {}\n",
            field(t, "emitted").unwrap_or_default(),
            field(t, "disk gate").unwrap_or_default(),
            field(t, "open tasks").unwrap_or_default()
        ));
    } else {
        plan_txt.push_str("no plan emitted yet — bare qexo computes the first.\n");
    }
    plan_txt.push_str(&format!(
        "history: {} emitted plan(s) in the run from {}\n",
        count_cluster_books(px, [5, 5, 5, PLAN_HISTORY_C, 1]),
        crate::phext::fmt_coord(&at(HL_PLAN, [PLAN_HISTORY_C, 1, 1, 1, 1, 1]))
    ));
    px.insert(index_addr(HL_PLAN), plan_txt);

    // ---- ledgers index ----
    let mut led = String::new();
    led.push_str("ledgers & receipts — homeland anchor 6.6.6/6.6.6/6.6.6\n\nThe beast keeps the books: each kind its own run of entry\nscrolls, appended beside its kin.\n\n");
    for c in [LEDGER_RECEIPTS, LEDGER_TELEPORTS, LEDGER_INSTANCES, LEDGER_EMOTION, LEDGER_GOAL_ACTIVITY, LEDGER_SYNC, LEDGER_RELEASES] {
        led.push_str(&format!(
            "- {}: {} entr(ies) from {}\n",
            ledger_kind_name(c).unwrap_or("?"),
            count_cluster_books(px, [6, 6, 6, c, 1]),
            crate::phext::fmt_coord(&at(HL_LEDGERS, [c, 1, 1, 1, 1, 1]))
        ));
    }
    px.insert(index_addr(HL_LEDGERS), led);

    // ---- mailbox & sessions index ----
    let mut mail = String::new();
    mail.push_str("mailbox & sessions — homeland anchor 7.7.7/7.7.7/7.7.7\n\n");
    if sessions.is_empty() {
        mail.push_str("hosted now: none.\n");
    }
    for r in &sessions {
        mail.push_str(&format!("- hosted: {} — session {} — status {}\n", r.name, crate::phext::fmt_coord(&r.coord), r.status));
    }
    mail.push_str(&format!(
        "mailbox: {} message scroll(s) in the run from {}\n",
        count_cluster_books(px, [7, 7, 7, MAIL_BOX_C, 1]),
        crate::phext::fmt_coord(&at(HL_MAIL, [MAIL_BOX_C, 1, 1, 1, 1, 1]))
    ));
    px.insert(index_addr(HL_MAIL), mail);

    // ---- telemetry index ----
    let obs = component_values(px, &[8, 8, 8, TELEM_DISK_C, 1], 5);
    let mut tel = String::new();
    tel.push_str("telemetry & forecasts — homeland anchor 8.8.8/8.8.8/8.8.8\n\nSignals country: disk observations as a run of scrolls; the\nforecast gate reads the last two, as it always has.\n\n");
    tel.push_str(&format!("disk observations: {}\n", obs.len()));
    if let Some(last) = obs.last() {
        let addr = at(HL_TELEMETRY, [TELEM_DISK_C, 1, *last, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            tel.push_str(&format!(
                "latest: {} — day {}, {} GB, {} files\n",
                crate::phext::fmt_coord(&addr),
                field(t, "day").unwrap_or_default(),
                field(t, "used_gb").unwrap_or_default(),
                field(t, "files").unwrap_or_default()
            ));
        }
    }
    px.insert(index_addr(HL_TELEMETRY), tel);

    // ---- config & wiring index ----
    let mut cfg = String::new();
    cfg.push_str("config & wiring — homeland anchor 9.9.9/9.9.9/9.9.9\n\nThe far corner is the machine room: qexo's own settings live\nin the config record here, as field lines — there is no config\nfile anywhere else. The wiring registry (one scroll per module)\nfollows it.\n\n");
    if px.get(&CONFIG_RECORD).is_some() {
        cfg.push_str(&format!("config record: {}\n", crate::phext::fmt_coord(&CONFIG_RECORD)));
    } else {
        cfg.push_str("config record: not written yet\n");
    }
    px.insert(index_addr(HL_CONFIG), cfg);

    // ---- home index (written last: it summarizes the rest) ----
    let id = px.get(&IDENTITY);
    let (node_line, born_line) = match id {
        Some(t) => (
            format!(
                "{} ({})",
                field(t, "name").unwrap_or_else(|| "unnamed".into()),
                field(t, "node").unwrap_or_default()
            ),
            field(t, "born").unwrap_or_default(),
        ),
        None => ("(not born)".to_string(), String::new()),
    };
    let mut home = String::new();
    home.push_str("qexo state — one address space\n\n");
    home.push_str("This file is the node's whole state: every scrap of it —\nmetadata, keys, grants, personas, plan, ledgers, sessions —\nas scrolls in one phext, addressed by meaning (Layer 2).\nAddresses are places; this scroll is the map, and the code\nkeeps it current. Records are truth; indexes are derived\nprose, rebuilt from the records by `qexo doctor --fix`.\n\n");
    home.push_str(&format!("node: {}\nborn: {}\nidentity: {}\n\n", node_line, born_line, crate::phext::fmt_coord(&IDENTITY)));
    home.push_str("Homelands:\n");
    home.push_str(&format!(
        "1.1.1/1.1.1/1.1.1 — this index; node identity beside it at {}\n",
        crate::phext::fmt_coord(&IDENTITY)
    ));
    home.push_str(&format!(
        "{} — keys & trust: node key {}, {} record(s) in the neighboring runs\n",
        crate::phext::fmt_coord(&index_addr(HL_KEYS)),
        px.get(&NODE_KEY_RECORD)
            .and_then(|t| field(t, "fingerprint"))
            .map(|f| fp_short(&f))
            .unwrap_or_else(|| "none".into()),
        count_cluster_books(px, [2, 2, 2, KEYS_PERSONA_C, 1])
            + count_cluster_books(px, [2, 2, 2, KEYS_PEER_C, 1])
            + count_cluster_books(px, [2, 2, 2, KEYS_GRANTS_C, 1])
            + count_cluster_books(px, [2, 2, 2, KEYS_DELEGATIONS_C, 1])
    ));
    home.push_str(&format!(
        "{} — peers: {} registered\n",
        crate::phext::fmt_coord(&index_addr(HL_PEERS)),
        peers.len()
    ));
    home.push_str(&format!(
        "{} — personas: {}\n",
        crate::phext::fmt_coord(&index_addr(HL_PERSONAS)),
        if personas.is_empty() {
            "none yet".to_string()
        } else {
            personas.iter().map(|r| r.name.clone()).collect::<Vec<_>>().join(", ")
        }
    ));
    home.push_str(&format!(
        "{} — plan: {}\n",
        crate::phext::fmt_coord(&index_addr(HL_PLAN)),
        if px.get(&PLAN_CURRENT).is_some() { "current plan present" } else { "no plan yet" }
    ));
    let ledger_total: usize = [LEDGER_RECEIPTS, LEDGER_TELEPORTS, LEDGER_INSTANCES, LEDGER_EMOTION, LEDGER_GOAL_ACTIVITY, LEDGER_SYNC, LEDGER_RELEASES]
        .iter()
        .map(|c| count_cluster_books(px, [6, 6, 6, *c, 1]))
        .sum();
    home.push_str(&format!(
        "{} — ledgers & receipts: {} entr(ies) across all kinds\n",
        crate::phext::fmt_coord(&index_addr(HL_LEDGERS)),
        ledger_total
    ));
    home.push_str(&format!(
        "{} — mailbox & sessions: {} hosted now\n",
        crate::phext::fmt_coord(&index_addr(HL_MAIL)),
        sessions.len()
    ));
    home.push_str(&format!(
        "{} — telemetry & forecasts: {} disk observation(s)\n",
        crate::phext::fmt_coord(&index_addr(HL_TELEMETRY)),
        obs.len()
    ));
    home.push_str(&format!(
        "{} — config & wiring: the machine room\n",
        crate::phext::fmt_coord(&index_addr(HL_CONFIG))
    ));
    px.insert(HOME, home);
}

// ----------------------------------------------------------------
// The state file itself
// ----------------------------------------------------------------

/// The canonical state path: /var/lib/qexo/state.phext.
/// QEXO_STATE overrides it for development and tests only.
pub fn state_path() -> PathBuf {
    match std::env::var("QEXO_STATE") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("/var/lib/qexo/state.phext"),
    }
}

/// The directory holding the state file (and the GPG keyring, which
/// is the gpg tool's own machinery and lives beside the file it
/// serves).
pub fn state_dir() -> PathBuf {
    state_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/var/lib/qexo"))
}

/// The loaded state document. All reads and writes of node state go
/// through this type; no other module touches the file directly.
pub struct Lattice {
    pub px: Phext,
    path: PathBuf,
}

impl Lattice {
    /// An unborn state document bound to `path` (config's birth).
    pub fn born(path: PathBuf) -> Lattice {
        Lattice { px: Phext::new(), path }
    }

    /// Load the state file if it exists. Ok(None) = not born yet
    /// (no file): callers treat that as "run config", never as an
    /// occasion to invent a substitute store.
    pub fn load() -> Result<Option<Lattice>, String> {
        Self::load_from(&state_path())
    }

    /// Load from an explicit path (tests, doctor).
    pub fn load_from(path: &PathBuf) -> Result<Option<Lattice>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        Ok(Some(Lattice { px: Phext::parse(&text), path: path.clone() }))
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Persist the whole document. Phext serializes canonically
    /// (ascending coordinates, delimiter ladder), so a save is a
    /// rewrite of one file — the format's write model, accepted.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        std::fs::write(&self.path, self.px.serialize())
            .map_err(|e| format!("{}: {}", self.path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("qexo-lattice-test-{}", std::process::id()));
        let path = dir.join("state.phext");
        let mut px = Phext::new();
        px.insert(HOME, "home".to_string());
        px.insert(IDENTITY, "second".to_string());
        let lat = Lattice { px, path: path.clone() };
        lat.save().unwrap();
        let back = Lattice::load_from(&path).unwrap().expect("state exists");
        assert_eq!(back.px.scrolls, lat.px.scrolls);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none_not_error() {
        let path = std::env::temp_dir().join("qexo-lattice-definitely-absent-xyz/state.phext");
        assert!(Lattice::load_from(&path).unwrap().is_none());
    }

    #[test]
    fn fields_roundtrip_and_edit_in_place() {
        let t = record(&[("kind", "peer"), ("name", "peer-1"), ("status", "registered")]);
        assert_eq!(field(&t, "name").as_deref(), Some("peer-1"));
        let t2 = set_field(&t, "status", "retired");
        assert_eq!(field(&t2, "status").as_deref(), Some("retired"));
        assert_eq!(field(&t2, "name").as_deref(), Some("peer-1"));
        let t3 = set_field(&t2, "closed", "later");
        assert_eq!(field(&t3, "closed").as_deref(), Some("later"));
    }

    #[test]
    fn adjacency_helpers_find_next_free() {
        let mut px = Phext::new();
        px.insert(at(HL_PEERS, [2, 1, 1, 1, 1, 1]), "a".into());
        px.insert(at(HL_PEERS, [3, 1, 1, 1, 1, 1]), "b".into());
        assert_eq!(next_free_cluster(&px, HL_PEERS, 2), 4);
        px.insert(at(HL_LEDGERS, [LEDGER_RECEIPTS, 1, 1, 1, 1, 1]), "r1".into());
        px.insert(at(HL_LEDGERS, [LEDGER_RECEIPTS, 1, 2, 1, 1, 1]), "r2".into());
        assert_eq!(next_free_book(&px, [6, 6, 6, LEDGER_RECEIPTS, 1], 1), 3);
        let run8 = [3, 3, 3, 2, 1, 1, 1, 1];
        px.insert([3, 3, 3, 2, 1, 1, 1, 1, 1], "x".into());
        assert_eq!(next_free_scroll(&px, run8, 1), [3, 3, 3, 2, 1, 1, 1, 1, 2]);
    }

    #[test]
    fn indexes_regenerate_from_records() {
        let mut px = Phext::new();
        px.insert(IDENTITY, record(&[("name", "Test Node"), ("node", "testbox"), ("born", "now")]));
        px.insert(at(HL_PEERS, [2, 1, 1, 1, 1, 1]), record(&[("kind", "peer"), ("name", "peer-ABCD"), ("fingerprint", "FF".repeat(20).as_str()), ("status", "registered")]));
        regen_indexes(&mut px);
        let home = px.get(&HOME).expect("home index written");
        assert!(home.contains("3.3.3/3.3.3/3.3.3 — peers"), "{}", home);
        assert!(!home.contains("/…"), "no partial addresses anywhere: {}", home);
        // the peers index lives at the homeland's full 9D anchor
        assert!(px.get(&[3, 3, 3, 3, 3, 3, 3, 3, 3]).is_some());
        assert_eq!(index_addr(HL_PEERS), [3, 3, 3, 3, 3, 3, 3, 3, 3]);
        assert_eq!(index_addr(HL_KEYS), [2, 2, 2, 2, 2, 2, 2, 2, 2]);
        assert_eq!(index_addr(HL_CONFIG), [9, 9, 9, 9, 9, 9, 9, 9, 9]);
        assert_eq!(index_addr(HL_HOME), HOME);
        assert!(home.contains("1 registered"), "{}", home);
        let peers_idx = px.get(&index_addr(HL_PEERS)).expect("peers index");
        assert!(peers_idx.contains("peer-ABCD"), "{}", peers_idx);
        assert!(peers_idx.contains("3.3.3/2.1.1/1.1.1"), "{}", peers_idx);
        // idempotent: regenerating again changes nothing
        let before = px.serialize();
        regen_indexes(&mut px);
        assert_eq!(px.serialize(), before);
    }
}

