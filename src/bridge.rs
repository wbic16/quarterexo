//! F6 — stable bridges to all inference sources (spec §9).
//!
//! A bridge is a durable, health-checked route to an
//! inference source. Bridge records live in the dispatch
//! homeland (11.11.11, Collection 5): one Book per bridge,
//! Scroll 1 = the record (§9.1), Scroll 2+ = the health
//! log — one scroll per probe, appended, never summarized.
//! Health is **calculated** from those atoms on every read:
//! a bridge is *live* iff its latest probe succeeded within
//! its `freshness:` window. There is no stored `health: ok`
//! field anywhere, and nothing in this module creates a
//! bridge implicitly — a bridge exists only because someone
//! ran `bridge --add` (§9.2: qexo never creates a local
//! bridge on its own).
//!
//! Routing (§9.2): an item whose brief carries `bridge:`
//! lines names its acceptable bridges in preference order.
//! Dispatch walks the order, probes a candidate whose
//! evidence is stale (or whose last probe failed), and
//! routes only over a bridge verified live *for this
//! dispatch*; the probe and the choice are cited in the
//! dispatch receipt (wire.rs). Every candidate walked past
//! is a `bridge-failover` receipt (from, to, evidence), so
//! the failover chain is complete in the ledger. A
//! `burn: never` bridge is never routed to — and enqueue
//! refuses a brief that names one whose record already
//! carries the mark (dispatch.rs).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::{append_receipt, now_stamp, require_born};
use crate::lattice::{self, Lattice};
use crate::phext::{self, Coord, Phext};

/// The kind enum (§9.1) — the model line (`tlm`, `slm`,
/// `llm`) is in it from the first bridge slice, so the
/// local line registers the day each can answer a probe,
/// with no schema change waiting for it.
pub const KINDS: [&str; 9] = [
    "claude", "grok", "chatgpt", "ollama", "tlm", "slm", "llm", "sq", "custom",
];

pub const DEFAULT_FRESHNESS: u64 = 900;

const USAGE_BRIDGE: &str = "usage: qexo bridge [--add <name> --kind <kind> --endpoint <where> [--capability <text>]... [--burn metered|never] [--reset <text>] [--freshness <seconds>]] [--check [<name>]] [--retire <name>]";

/// One bridge record (Collection 5, Scroll 1 of its Book).
#[derive(Debug, Clone)]
pub struct Bridge {
    pub book: u32,
    pub addr: Coord,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub burn: String,
    pub reset: Option<String>,
    pub freshness: u64,
    pub status: String,
}

/// The address of bridge Book `book`'s record scroll.
pub fn bridge_addr(book: u32) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_BRIDGES_C, 1, book, 1, 1, 1])
}

/// The address of a health-log scroll in bridge Book `book`.
#[allow(dead_code)]
pub fn health_addr(book: u32, scroll: u32) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_BRIDGES_C, 1, book, 1, 1, scroll])
}

pub fn parse_bridge(book: u32, text: &str) -> Bridge {
    Bridge {
        book,
        addr: bridge_addr(book),
        name: lattice::field(text, "name").unwrap_or_default(),
        kind: lattice::field(text, "kind").unwrap_or_default(),
        endpoint: lattice::field(text, "endpoint").unwrap_or_default(),
        capabilities: crate::wire::field_all(text, "capability"),
        burn: lattice::field(text, "burn").unwrap_or_else(|| "metered".to_string()),
        reset: lattice::field(text, "reset"),
        freshness: lattice::field(text, "freshness")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_FRESHNESS),
        status: lattice::field(text, "status").unwrap_or_else(|| "active".to_string()),
    }
}

/// Every bridge record in the state, in Book order.
pub fn load_bridges(px: &Phext) -> Vec<Bridge> {
    let books = lattice::component_values(px, &[11, 11, 11, lattice::DISPATCH_BRIDGES_C, 1], 5);
    let mut out = Vec::new();
    for b in books {
        if let Some(t) = px.get(&bridge_addr(b)) {
            out.push(parse_bridge(b, t));
        }
    }
    out
}

pub fn find_bridge(px: &Phext, name: &str) -> Option<Bridge> {
    load_bridges(px).into_iter().find(|b| b.name == name)
}

/// The `bridge:` preference lines of a brief, in order,
/// duplicates removed (the brief_facts precedent).
pub fn brief_prefs(brief: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in crate::wire::field_all(brief, "bridge") {
        let name = name.trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

// ----------------------------------------------------------------
// The health log (§9.1) — atoms, and the calculation over them.
// ----------------------------------------------------------------

/// One health-log atom (a probe and its outcome).
#[derive(Debug, Clone)]
pub struct HealthRec {
    pub addr: Coord,
    pub epoch: i64,
    pub probe: String,
    pub ok: bool,
    pub detail: String,
    pub latency_ms: u128,
}

/// A bridge Book's health log, in scroll order.
pub fn load_health(px: &Phext, book: u32) -> Vec<HealthRec> {
    let mut out = Vec::new();
    for (addr, text) in lattice::prefix_scrolls(px, &[11, 11, 11, lattice::DISPATCH_BRIDGES_C, 1, book, 1, 1]) {
        if addr[8] < 2 {
            continue; // Scroll 1 is the record itself.
        }
        out.push(HealthRec {
            addr,
            epoch: lattice::field(text, "epoch").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0),
            probe: lattice::field(text, "probe").unwrap_or_default(),
            ok: lattice::field(text, "result").as_deref() == Some("ok"),
            detail: lattice::field(text, "detail").unwrap_or_default(),
            latency_ms: lattice::field(text, "latency-ms").and_then(|s| s.parse::<u128>().ok()).unwrap_or(0),
        });
    }
    out
}

fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// What the health log says about a bridge right now —
/// calculated on every call, never stored (§9.1).
#[derive(Debug, Clone, PartialEq)]
pub enum Liveness {
    /// Latest probe succeeded within the freshness window.
    Live(Coord),
    /// Latest probe succeeded, but outside the window.
    Stale(Coord),
    /// Latest probe failed (at any age): a failure is not
    /// liveness evidence, however fresh.
    Down(Coord),
    /// No probe has ever been filed.
    NeverProbed,
}

pub fn liveness(px: &Phext, bridge: &Bridge) -> Liveness {
    let log = load_health(px, bridge.book);
    match log.last() {
        None => Liveness::NeverProbed,
        Some(h) if !h.ok => Liveness::Down(h.addr),
        Some(h) => {
            if now_epoch() - h.epoch <= bridge.freshness as i64 {
                Liveness::Live(h.addr)
            } else {
                Liveness::Stale(h.addr)
            }
        }
    }
}

/// A human rendering of the calculated health, for the
/// `qexo bridge` list and the dispatch --check report.
pub fn health_label(px: &Phext, bridge: &Bridge) -> String {
    if bridge.status == "retired" {
        return "retired".to_string();
    }
    let log = load_health(px, bridge.book);
    match log.last() {
        None => "never probed".to_string(),
        Some(h) => {
            let age = now_epoch() - h.epoch;
            let what = format!("{} {}, {}ms", h.probe, h.detail, h.latency_ms);
            if !h.ok {
                format!("down — last probe failed {}s ago ({})", age, what)
            } else if age <= bridge.freshness as i64 {
                format!("live — probed {}s ago ({})", age, what)
            } else {
                format!("stale — last ok probe {}s ago, freshness {}s", age, bridge.freshness)
            }
        }
    }
}

// ----------------------------------------------------------------
// Probing. A probe is a real reach for the endpoint, timed.
// The sandbox gates stand HTTP doubles on localhost; the
// production bridge endpoints are reached the same way.
// ----------------------------------------------------------------

pub struct ProbeOutcome {
    pub kind: &'static str,
    pub ok: bool,
    pub detail: String,
    pub latency_ms: u128,
}

fn http_probe(authority: &str, path: &str) -> ProbeOutcome {
    let started = Instant::now();
    let addr = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{}:80", authority)
    };
    let sock = match addr.to_socket_addrs().ok().and_then(|mut a| a.next()) {
        Some(s) => s,
        None => {
            return ProbeOutcome {
                kind: "http",
                ok: false,
                detail: format!("cannot resolve {}", authority),
                latency_ms: started.elapsed().as_millis(),
            }
        }
    };
    let mut stream = match TcpStream::connect_timeout(&sock, Duration::from_secs(5)) {
        Ok(s) => s,
        Err(e) => {
            return ProbeOutcome {
                kind: "http",
                ok: false,
                detail: format!("connect {}: {}", authority, e),
                latency_ms: started.elapsed().as_millis(),
            }
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let req = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        if path.is_empty() { "/" } else { path },
        authority
    );
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return ProbeOutcome {
            kind: "http",
            ok: false,
            detail: format!("write {}: {}", authority, e),
            latency_ms: started.elapsed().as_millis(),
        };
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 16384 || buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u32>().ok());
    match status {
        Some(code) => ProbeOutcome {
            kind: "http",
            ok: (200..400).contains(&code),
            detail: format!("HTTP {}", code),
            latency_ms: started.elapsed().as_millis(),
        },
        None => ProbeOutcome {
            kind: "http",
            ok: false,
            detail: "no HTTP response".to_string(),
            latency_ms: started.elapsed().as_millis(),
        },
    }
}

fn https_probe(endpoint: &str) -> ProbeOutcome {
    // The register integration's precedent (register.rs):
    // https goes through curl. No credential rides a probe.
    let started = Instant::now();
    let out = std::process::Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "10", endpoint])
        .output();
    match out {
        Ok(o) => {
            let code_s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            match code_s.parse::<u32>() {
                Ok(code) if code > 0 => ProbeOutcome {
                    kind: "https",
                    ok: (200..400).contains(&code),
                    detail: format!("HTTP {}", code),
                    latency_ms: started.elapsed().as_millis(),
                },
                _ => ProbeOutcome {
                    kind: "https",
                    ok: false,
                    detail: format!(
                        "curl: {}",
                        String::from_utf8_lossy(&o.stderr).trim().lines().next().unwrap_or("no response")
                    ),
                    latency_ms: started.elapsed().as_millis(),
                },
            }
        }
        Err(e) => ProbeOutcome {
            kind: "https",
            ok: false,
            detail: format!("curl unavailable: {}", e),
            latency_ms: started.elapsed().as_millis(),
        },
    }
}

/// Probe an endpoint, by its shape: http/https URLs are
/// fetched, a bare host:port is connected to, anything
/// else is treated as a local path (a gateway path, a
/// mailbox directory) and checked for presence.
pub fn probe(endpoint: &str) -> ProbeOutcome {
    if let Some(rest) = endpoint.strip_prefix("http://") {
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        http_probe(authority, path)
    } else if endpoint.starts_with("https://") {
        https_probe(endpoint)
    } else if endpoint.contains("://") {
        ProbeOutcome {
            kind: "unknown",
            ok: false,
            detail: "endpoint scheme is not probeable here".to_string(),
            latency_ms: 0,
        }
    } else if let Some((host, port)) = endpoint.rsplit_once(':') {
        if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            let started = Instant::now();
            let addr = format!("{}:{}", host, port);
            return match addr.to_socket_addrs().ok().and_then(|mut a| a.next()) {
                Some(sock) => match TcpStream::connect_timeout(&sock, Duration::from_secs(5)) {
                    Ok(_) => ProbeOutcome {
                        kind: "tcp",
                        ok: true,
                        detail: "tcp connect ok".to_string(),
                        latency_ms: started.elapsed().as_millis(),
                    },
                    Err(e) => ProbeOutcome {
                        kind: "tcp",
                        ok: false,
                        detail: format!("connect {}: {}", addr, e),
                        latency_ms: started.elapsed().as_millis(),
                    },
                },
                None => ProbeOutcome {
                    kind: "tcp",
                    ok: false,
                    detail: format!("cannot resolve {}", host),
                    latency_ms: started.elapsed().as_millis(),
                },
            };
        }
        path_probe(endpoint)
    } else {
        path_probe(endpoint)
    }
}

fn path_probe(endpoint: &str) -> ProbeOutcome {
    let started = Instant::now();
    let present = std::path::Path::new(endpoint).exists();
    ProbeOutcome {
        kind: "path",
        ok: present,
        detail: if present { "path present".to_string() } else { "path absent".to_string() },
        latency_ms: started.elapsed().as_millis(),
    }
}

/// Append one probe outcome to a bridge's health log;
/// returns the scroll's address (the probe cite routing
/// puts in the dispatch receipt).
pub fn append_health(px: &mut Phext, bridge: &Bridge, out: &ProbeOutcome) -> Coord {
    let addr = lattice::next_free_scroll(
        px,
        [11, 11, 11, lattice::DISPATCH_BRIDGES_C, 1, bridge.book, 1, 1],
        2,
    );
    let (_, stamp) = now_stamp();
    let text = format!(
        "bridge health\n{}",
        lattice::record(&[
            ("bridge", &bridge.name),
            ("at", &stamp),
            ("epoch", &now_epoch().to_string()),
            ("probe", out.kind),
            ("result", if out.ok { "ok" } else { "fail" }),
            ("detail", &out.detail),
            ("latency-ms", &out.latency_ms.to_string()),
        ])
    );
    px.insert(addr, text);
    addr
}

// ----------------------------------------------------------------
// Routing (§9.2) — the preference walk.
// ----------------------------------------------------------------

/// A completed routing: the bridge chosen, and the probe
/// scroll that verifies it live for this dispatch.
pub struct Chosen {
    pub name: String,
    pub endpoint: String,
    pub probe: Coord,
}

pub enum Route {
    Chosen(Chosen),
    /// Live mode: no candidate is routable. Every
    /// candidate walked past carries its failover
    /// receipt already (appended into the state); the
    /// string is the reason the item waits with.
    Wait(String),
    /// Check mode only: the walk reached a candidate
    /// whose evidence a live dispatch would probe first.
    WouldProbe(String),
    /// Check mode only: nothing is choosable on the
    /// evidence as it stands.
    NoRoute(String),
}

/// Bridges this item must not be re-routed to on its
/// next dispatch: the `from` of a `bridge-failover`
/// receipt filed at the item's last closed attempt —
/// the mid-execution advance (§9.2), calculated from
/// the ledger like everything else.
pub fn mid_execution_skips(px: &Phext, item_s: &str, last_attempt: u32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let books = lattice::component_values(px, &[6, 6, 6, lattice::LEDGER_RECEIPTS, 1], 5);
    for b in books {
        let addr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RECEIPTS, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "action").as_deref() == Some("bridge-failover")
                && lattice::field(t, "item").as_deref() == Some(item_s)
                && lattice::field(t, "attempt").and_then(|a| a.parse::<u32>().ok()) == Some(last_attempt)
            {
                if let Some(from) = lattice::field(t, "from") {
                    if !out.contains(&from) {
                        out.push(from);
                    }
                }
            }
        }
    }
    out
}

/// Walk an item's bridge preferences (§9.2). With
/// `probe_now`, stale or failed evidence is probed on
/// the spot (health scrolls and failover receipts are
/// appended into `px`; the caller saves). Without it
/// (dispatch --check, byte-inert) the walk reads the
/// evidence as it stands and writes nothing.
pub fn route(
    px: &mut Phext,
    item_s: &str,
    attempt: u32,
    prefs: &[String],
    skip: &[String],
    probe_now: bool,
) -> Route {
    let mut walked: Vec<String> = Vec::new();
    for (i, name) in prefs.iter().enumerate() {
        if skip.contains(name) {
            walked.push(format!("{} — failed mid-execution on the last attempt; the failover receipt advances past it", name));
            continue;
        }
        // The failover receipt's `to`: the next preference
        // this walk could still choose, or none.
        let to = prefs[i + 1..]
            .iter()
            .find(|n| !skip.contains(n))
            .cloned()
            .unwrap_or_else(|| "none".to_string());
        let failover = |px: &mut Phext, evidence: &str| {
            append_receipt(
                px,
                "bridge-failover",
                &[
                    ("item", item_s),
                    ("attempt", &attempt.to_string()),
                    ("from", name.as_str()),
                    ("to", to.as_str()),
                    ("evidence", evidence),
                ],
            );
        };
        let bridge = match find_bridge(px, name) {
            Some(b) => b,
            None => {
                if probe_now {
                    failover(px, "no bridge record on this node");
                }
                walked.push(format!("{} — no bridge record", name));
                continue;
            }
        };
        if bridge.status == "retired" {
            if probe_now {
                failover(px, "the bridge is retired");
            }
            walked.push(format!("{} — retired", name));
            continue;
        }
        if bridge.burn == "never" {
            if probe_now {
                failover(px, "burn: never — dispatch never routes work over a burn: never bridge (§9.2)");
            }
            walked.push(format!("{} — burn: never", name));
            continue;
        }
        match liveness(px, &bridge) {
            Liveness::Live(cite) => {
                return Route::Chosen(Chosen {
                    name: bridge.name.clone(),
                    endpoint: bridge.endpoint.clone(),
                    probe: cite,
                });
            }
            live => {
                if !probe_now {
                    return Route::WouldProbe(name.clone());
                }
                let _ = live;
                let out = probe(&bridge.endpoint);
                let cite = append_health(px, &bridge, &out);
                if out.ok {
                    return Route::Chosen(Chosen {
                        name: bridge.name.clone(),
                        endpoint: bridge.endpoint.clone(),
                        probe: cite,
                    });
                }
                failover(px, &format!("probe failed — {} ({})", out.detail, out.kind));
                walked.push(format!("{} — probe failed ({})", name, out.detail));
                continue;
            }
        }
    }
    let reason = format!("no live bridge — {}", walked.join("; "));
    if probe_now {
        Route::Wait(reason)
    } else {
        Route::NoRoute(reason)
    }
}

// ----------------------------------------------------------------
// The verb (§2.1): list, --add, --check, --retire.
// ----------------------------------------------------------------

struct AddArgs {
    name: String,
    kind: String,
    endpoint: String,
    capabilities: Vec<String>,
    burn: String,
    reset: Option<String>,
    freshness: u64,
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cmd_add(lat: &mut Lattice, a: AddArgs) -> i32 {
    if a.name.is_empty() || a.name.contains(char::is_whitespace) {
        eprintln!("qexo bridge: a bridge name is one word (got '{}')", a.name);
        return 2;
    }
    if !KINDS.contains(&a.kind.as_str()) {
        eprintln!(
            "qexo bridge: kind '{}' is not in the enum ({})",
            a.kind,
            KINDS.join(" | ")
        );
        return 2;
    }
    if a.endpoint.is_empty() {
        eprintln!("qexo bridge: --endpoint is required");
        return 2;
    }
    if a.burn != "metered" && a.burn != "never" {
        eprintln!("qexo bridge: --burn is metered | never (got '{}')", a.burn);
        return 2;
    }
    if find_bridge(&lat.px, &a.name).is_some() {
        eprintln!(
            "qexo bridge: a bridge named '{}' already exists — a name is a bridge's identity; retire it there, do not re-mint it",
            a.name
        );
        return 1;
    }
    let mut fields: Vec<(&str, String)> = vec![
        ("name", a.name.clone()),
        ("kind", a.kind.clone()),
        ("endpoint", a.endpoint.clone()),
    ];
    for c in &a.capabilities {
        fields.push(("capability", c.clone()));
    }
    fields.push(("burn", a.burn.clone()));
    if let Some(r) = &a.reset {
        fields.push(("reset", r.clone()));
    }
    fields.push(("freshness", a.freshness.to_string()));
    fields.push(("status", "active".to_string()));
    let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let text = format!("bridge\n{}", lattice::record(&refs));
    let book = lattice::next_free_book(&lat.px, [11, 11, 11, lattice::DISPATCH_BRIDGES_C, 1], 1);
    let addr = bridge_addr(book);
    lat.px.insert(addr, text);
    lattice::regen_indexes(&mut lat.px);
    // Write-before-ack (§4.1): the address prints only
    // after the save that carries the record.
    if let Err(e) = lat.save() {
        eprintln!("qexo bridge: cannot save state: {}", e);
        return 1;
    }
    println!("{} — bridge {} ({}) at {}", phext::fmt_coord(&addr), a.name, a.kind, a.endpoint);
    0
}

fn cmd_check(lat: &mut Lattice, name: Option<&str>) -> i32 {
    let bridges = load_bridges(&lat.px);
    let targets: Vec<Bridge> = match name {
        Some(n) => match bridges.iter().find(|b| b.name == n) {
            Some(b) if b.status != "retired" => vec![(*b).clone()],
            Some(_) => {
                eprintln!("qexo bridge: '{}' is retired — a retired bridge is not probed", n);
                return 1;
            }
            None => {
                eprintln!("qexo bridge: no bridge named '{}'", n);
                return 1;
            }
        },
        None => bridges.iter().filter(|b| b.status != "retired").cloned().collect(),
    };
    if targets.is_empty() {
        println!("no active bridges to probe");
        return 0;
    }
    let mut lines: Vec<String> = Vec::new();
    for b in &targets {
        let out = probe(&b.endpoint);
        let cite = append_health(&mut lat.px, b, &out);
        lines.push(format!(
            "{} — probe {} {} ({}) — {}",
            b.name,
            out.kind,
            if out.ok { "ok" } else { "FAIL" },
            out.detail,
            health_label(&lat.px, b),
        ));
        let _ = cite;
    }
    if let Err(e) = lat.save() {
        eprintln!("qexo bridge: cannot save state: {}", e);
        return 1;
    }
    for l in lines {
        println!("{}", l);
    }
    0
}

fn cmd_retire(lat: &mut Lattice, name: &str) -> i32 {
    let bridge = match find_bridge(&lat.px, name) {
        Some(b) => b,
        None => {
            eprintln!("qexo bridge: no bridge named '{}'", name);
            return 1;
        }
    };
    if bridge.status == "retired" {
        println!("bridge {} is already retired", name);
        return 0;
    }
    let text = lat.px.get(&bridge.addr).cloned().unwrap_or_default();
    let text = lattice::set_field(&text, "status", "retired");
    lat.px.insert(bridge.addr, text);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo bridge: cannot save state: {}", e);
        return 1;
    }
    println!("bridge {} retired — its health log stands; it routes nothing more", name);
    0
}

fn cmd_list(lat: &Lattice) -> i32 {
    let bridges = load_bridges(&lat.px);
    if bridges.is_empty() {
        println!("no bridges registered — qexo bridge --add <name> --kind <kind> --endpoint <url> registers one (§9.1)");
        return 0;
    }
    println!("bridges — health is calculated from each Book's health log (§9.1), never stored");
    for b in &bridges {
        let mut line = format!(
            "- {} ({}) — {} — burn {} — {} — {}",
            b.name,
            b.kind,
            b.endpoint,
            b.burn,
            b.status,
            health_label(&lat.px, b)
        );
        if let Some(r) = &b.reset {
            line.push_str(&format!(" — reset {}", r));
        }
        for c in &b.capabilities {
            line.push_str(&format!("\n    capability: {}", c));
        }
        println!("{}", line);
    }
    0
}

pub fn cmd_bridge(args: &[String]) -> i32 {
    // Flag scan (the cli.rs house style: flags for
    // setters, one positional for --add's name and
    // --check's optional name).
    let mut add: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut endpoint: Option<String> = None;
    let mut capabilities: Vec<String> = Vec::new();
    let mut burn: Option<String> = None;
    let mut reset: Option<String> = None;
    let mut freshness: Option<String> = None;
    let mut check = false;
    let mut check_name: Option<String> = None;
    let mut retire: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let take = |i: &mut usize| -> Option<String> {
            if *i + 1 < args.len() {
                *i += 1;
                Some(args[*i].clone())
            } else {
                None
            }
        };
        match a {
            "--add" => add = take(&mut i),
            "--kind" => kind = take(&mut i),
            "--endpoint" => endpoint = take(&mut i),
            "--capability" => {
                if let Some(v) = take(&mut i) {
                    capabilities.push(one_line(&v));
                }
            }
            "--burn" => burn = take(&mut i),
            "--reset" => reset = take(&mut i),
            "--freshness" => freshness = take(&mut i),
            "--retire" => retire = take(&mut i),
            "--check" => {
                check = true;
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                    check_name = Some(args[i].clone());
                }
            }
            other if !other.starts_with("--") && add.is_none() && !check && retire.is_none() => {
                eprintln!("qexo bridge: unexpected argument '{}'\n{}", other, USAGE_BRIDGE);
                return 2;
            }
            other => {
                eprintln!("qexo bridge: unexpected argument '{}'\n{}", other, USAGE_BRIDGE);
                return 2;
            }
        }
        i += 1;
    }
    let modes = [add.is_some(), check, retire.is_some()].iter().filter(|m| **m).count();
    if modes > 1 {
        eprintln!("qexo bridge: --add, --check, and --retire are separate acts\n{}", USAGE_BRIDGE);
        return 2;
    }
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    if let Some(name) = retire {
        return cmd_retire(&mut lat, &name);
    }
    if check {
        return cmd_check(&mut lat, check_name.as_deref());
    }
    if let Some(name) = add {
        let freshness = match freshness {
            None => DEFAULT_FRESHNESS,
            Some(s) => match s.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    eprintln!("qexo bridge: --freshness '{}' is not a number of seconds", s);
                    return 2;
                }
            },
        };
        return cmd_add(
            &mut lat,
            AddArgs {
                name: one_line(&name),
                kind: kind.unwrap_or_default(),
                endpoint: endpoint.unwrap_or_default(),
                capabilities,
                burn: burn.unwrap_or_else(|| "metered".to_string()),
                reset: reset.map(|r| one_line(&r)),
                freshness,
            },
        );
    }
    cmd_list(&lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_bridge(burn: &str, status: &str) -> Phext {
        let mut px = Phext::new();
        let text = format!(
            "bridge\n{}",
            lattice::record(&[
                ("name", "claude"),
                ("kind", "claude"),
                ("endpoint", "http://127.0.0.1:9/"),
                ("burn", burn),
                ("freshness", "900"),
                ("status", status),
            ])
        );
        px.insert(bridge_addr(1), text);
        px
    }

    fn health(px: &mut Phext, book: u32, epoch: i64, ok: bool) {
        let addr = health_addr(book, 2 + load_health(px, book).len() as u32);
        let text = format!(
            "bridge health\n{}",
            lattice::record(&[
                ("bridge", "claude"),
                ("at", "2026-11-17 00:00 UTC"),
                ("epoch", &epoch.to_string()),
                ("probe", "http"),
                ("result", if ok { "ok" } else { "fail" }),
                ("detail", "HTTP 200"),
                ("latency-ms", "3"),
            ])
        );
        px.insert(addr, text);
    }

    #[test]
    fn health_is_calculated_never_stored() {
        let mut px = state_with_bridge("metered", "active");
        let b = find_bridge(&px, "claude").unwrap();
        assert_eq!(liveness(&px, &b), Liveness::NeverProbed);
        health(&mut px, 1, now_epoch(), true);
        assert!(matches!(liveness(&px, &b), Liveness::Live(_)));
        // An old success is stale, not live.
        let mut px2 = state_with_bridge("metered", "active");
        health(&mut px2, 1, now_epoch() - 5000, true);
        assert!(matches!(liveness(&px2, &b), Liveness::Stale(_)));
        // A fresh failure is down, never live.
        let mut px3 = state_with_bridge("metered", "active");
        health(&mut px3, 1, now_epoch(), false);
        assert!(matches!(liveness(&px3, &b), Liveness::Down(_)));
    }

    #[test]
    fn route_walks_past_a_dead_bridge_to_the_live_one() {
        // Two bridges: dead (fresh failed probe — check
        // mode re-probe is reported, so use live mode
        // against a listener double for the second).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let mut px = state_with_bridge("metered", "active");
        // Bridge 1 "claude" points at a dead port (9 =
        // discard, closed on any sane host is not
        // guaranteed; use a bound-then-dropped port).
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = dead.local_addr().unwrap().port();
        drop(dead);
        let rec = lat_text_with_endpoint(&px, &format!("http://127.0.0.1:{}/", dead_port));
        px.insert(bridge_addr(1), rec);
        // Bridge 2 "grok" is the live double.
        let text2 = format!(
            "bridge\n{}",
            lattice::record(&[
                ("name", "grok"),
                ("kind", "grok"),
                ("endpoint", &format!("http://127.0.0.1:{}/", port)),
                ("burn", "metered"),
                ("freshness", "900"),
                ("status", "active"),
            ])
        );
        px.insert(bridge_addr(2), text2);
        let prefs = vec!["claude".to_string(), "grok".to_string()];
        match route(&mut px, "11.11.11/2.1.1/1.1.1", 1, &prefs, &[], true) {
            Route::Chosen(c) => {
                assert_eq!(c.name, "grok");
                // The failover receipt for claude stands.
                let mut found = false;
                for b in lattice::component_values(&px, &[6, 6, 6, lattice::LEDGER_RECEIPTS, 1], 5) {
                    let a = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RECEIPTS, 1, b, 1, 1, 1]);
                    if let Some(t) = px.get(&a) {
                        if lattice::field(t, "action").as_deref() == Some("bridge-failover")
                            && lattice::field(t, "from").as_deref() == Some("claude")
                            && lattice::field(t, "to").as_deref() == Some("grok")
                        {
                            found = true;
                        }
                    }
                }
                assert!(found, "the failover receipt chain names claude -> grok");
                // And grok's health log gained the probe.
                let grok = find_bridge(&px, "grok").unwrap();
                assert_eq!(load_health(&px, grok.book).len(), 1);
            }
            _ => panic!("the walk must choose the live bridge"),
        }
        let _ = server.join();
    }

    fn lat_text_with_endpoint(px: &Phext, endpoint: &str) -> String {
        let b = find_bridge(px, "claude").unwrap();
        let text = px.get(&b.addr).cloned().unwrap();
        lattice::set_field(&text, "endpoint", endpoint)
    }

    #[test]
    fn burn_never_and_retired_are_never_chosen() {
        for (burn, status, frag) in [("never", "active", "burn: never"), ("metered", "retired", "retired")] {
            let mut px = state_with_bridge(burn, status);
            let prefs = vec!["claude".to_string()];
            match route(&mut px, "11.11.11/2.1.1/1.1.1", 1, &prefs, &[], true) {
                Route::Wait(reason) => assert!(reason.contains(frag), "{} in {}", frag, reason),
                _ => panic!("a {} bridge must not route", frag),
            }
        }
    }

    #[test]
    fn check_mode_writes_nothing_and_reports_the_probe_it_would_make() {
        let mut px = state_with_bridge("metered", "active");
        let before = px.serialize();
        let prefs = vec!["claude".to_string()];
        match route(&mut px, "11.11.11/2.1.1/1.1.1", 1, &prefs, &[], false) {
            Route::WouldProbe(n) => assert_eq!(n, "claude"),
            _ => panic!("stale evidence in check mode must report WouldProbe"),
        }
        assert_eq!(px.serialize(), before, "--check routing is byte-inert");
    }
}
