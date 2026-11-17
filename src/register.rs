//! The register link (dispatch core, slice 3 — spec §10).
//!
//! The register (register.phext.io) is the durable queue
//! surface: entries go in, qexo drains them into the local
//! queue, and resolutions come back as write-backs. This
//! module is the whole of that conversation:
//!
//! - `qexo drain [--check] [--url <url>]` (§10.3): one pass
//!   reads the drain record (11.11.11, Collection 6 —
//!   watermarks, register URL, last-drain cite), pulls the
//!   entry and resolution atoms past the watermarks with
//!   the drain credential, files entries as queue items
//!   (`origin: register`, `register-seq: <seq>`, the brief
//!   verbatim), closes the items resolutions name through
//!   the state machine's register-close path (a
//!   `register-close` audit citing the resolution seq,
//!   §7.2), files an audit with no home for a resolution
//!   whose number no item carries, and only then — in the
//!   same save (§4.1) — advances the watermarks. A crashed
//!   or repeated drain re-reads the same atoms and dedups
//!   by `register-seq:` / resolution seq: draining is
//!   idempotent by construction.
//! - Write-back (§10.4): a local terminal close of a
//!   register-numbered item posts its resolution back
//!   (`POST /api/items/{n}/done|cancel`) under the qexo
//!   service token, and entries can be appended under the
//!   same token (the ratified Q2 scope: resolutions +
//!   entries-append, Will's scope only). The cancel verb
//!   attempts its write-back immediately; whatever is
//!   still owed rides the item record as
//!   `register-writeback: pending` and the next drain's
//!   sweep retries it — a write-back is never lost
//!   silently.
//!
//! Credentials are configuration, never state (§10.4):
//! both tokens live in an env file beside the state —
//! `/var/lib/qexo/qexo.env`, mode 0600, root-owned on the
//! drainer node; `QEXO_ENV` overrides the path for
//! dev/tests. Nothing secret is ever written to the
//! phext, a receipt, an audit, or a log line: receipts
//! and audits cite credentials by name only
//! (`credential: drain-token (env)`).
//!
//! Scope (§10.2) is the credential's, not a parameter's:
//! the tokens qexo holds are bound to Will's scope, the
//! drain returns only that scope's atoms, and there is no
//! request shape here that names another scope — the
//! scope is not widenable by configuration (§10.5).
//!
//! The wire: `http://` rides a small TcpStream client in
//! the house pattern (the register answers on the tailnet
//! and localhost in dev); `https://` goes through curl
//! with its config on stdin, so the credential header
//! never appears in a process list or a log. JSON is the
//! interop gasket at this edge only — atoms are parsed
//! here and cross into state as phext records, never as
//! JSON.

use crate::core::{append_receipt, civil_from_days, now_stamp, require_born};
use crate::dispatch::{self, Actor, ItemState, Transition};
use crate::lattice::{self, Lattice};
use crate::phext::{self, Coord};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

// ----------------------------------------------------------------
// The drain record (11.11.11, Collection 6, Book 1, Scroll 1)
// ----------------------------------------------------------------

pub fn drain_record_addr() -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_REGISTER_C, 1, 1, 1, 1, 1])
}

/// The register link's standing facts: where the register
/// is, how far the drain has read, and the last pass's
/// cite. Calculated views (lag, health) derive from this
/// record and the service's high-water seqs — never the
/// other way round.
#[derive(Debug, Clone, PartialEq)]
pub struct DrainRecord {
    pub url: String,
    pub entries_watermark: u64,
    pub resolutions_watermark: u64,
    pub last_drain: Option<String>,
}

pub fn load_drain_record(px: &phext::Phext) -> Option<DrainRecord> {
    let text = px.get(&drain_record_addr())?;
    let url = lattice::field(&text, "register-url")?;
    let num = |k: &str| lattice::field(&text, k).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    Some(DrainRecord {
        url,
        entries_watermark: num("entries-watermark"),
        resolutions_watermark: num("resolutions-watermark"),
        last_drain: lattice::field(&text, "last-drain"),
    })
}

fn drain_record_text(rec: &DrainRecord) -> String {
    let ew = rec.entries_watermark.to_string();
    let rw = rec.resolutions_watermark.to_string();
    let mut fields: Vec<(&str, &str)> = vec![
        ("register-url", rec.url.as_str()),
        ("entries-watermark", ew.as_str()),
        ("resolutions-watermark", rw.as_str()),
    ];
    if let Some(l) = &rec.last_drain {
        fields.push(("last-drain", l));
    }
    let mut text = String::from("register link\n");
    text.push_str(&lattice::record(&fields));
    text
}

// ----------------------------------------------------------------
// Credentials — env file only, never state (§10.4)
// ----------------------------------------------------------------

/// The register credentials, by name. Values are held
/// only in memory, only for the call that presents them;
/// nothing in this module prints, stores, or logs them —
/// the struct deliberately does not derive Debug, so a
/// token is never one careless print away from a log.
#[derive(Clone, Default)]
pub struct Credentials {
    pub drain_token: Option<String>,
    pub qexo_token: Option<String>,
}

pub fn credentials_path() -> PathBuf {
    match std::env::var("QEXO_ENV") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => PathBuf::from("/var/lib/qexo/qexo.env"),
    }
}

pub fn load_credentials() -> Credentials {
    match std::fs::read_to_string(credentials_path()) {
        Ok(text) => parse_credentials(&text),
        Err(_) => Credentials::default(),
    }
}

/// Parse the env file's `KEY=value` lines. The keys are
/// the register service's own env names (§10.4) — the
/// drainer node's file carries the same names the box
/// carries, so a value is provisioned once, by Will's
/// hand, in one pass.
pub fn parse_credentials(text: &str) -> Credentials {
    let mut out = Credentials::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            if v.is_empty() {
                continue;
            }
            match k.trim() {
                "REGSVC_DRAIN_TOKEN" => out.drain_token = Some(v.to_string()),
                "REGSVC_QEXO_TOKEN" => out.qexo_token = Some(v.to_string()),
                _ => {}
            }
        }
    }
    out
}

// ----------------------------------------------------------------
// Stamps
// ----------------------------------------------------------------

/// The house stamp for an epoch-seconds instant — the
/// same reckoning `now_stamp` writes and `stamp_minutes`
/// parses back, applied to a register atom's `ts`.
pub fn stamp_of_epoch(ts: i64) -> String {
    let day = ts.div_euclid(86400);
    let sod = ts.rem_euclid(86400);
    let (y, m, d) = civil_from_days(day);
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m, d, sod / 3600, (sod % 3600) / 60)
}

// ----------------------------------------------------------------
// The wire
// ----------------------------------------------------------------

/// One request against the register. `path` is the
/// API path (`/api/...`); `base` is the register URL from
/// the drain record (an origin, with any path prefix the
/// deployment carries). Returns (status, body).
fn http_request(
    base: &str,
    method: &str,
    path: &str,
    header: Option<(&str, &str)>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let (https, rest) = if let Some(r) = base.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = base.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("register URL '{}' must start with http:// or https://", base));
    };
    let (hostport, prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let full_path = format!("{}{}", prefix.trim_end_matches('/'), path);
    if https {
        let url = format!("https://{}{}", hostport, full_path);
        curl_request(&url, method, header, body)
    } else {
        tcp_request(hostport, method, &full_path, header, body)
    }
}

/// Minimal HTTP/1.1 over TcpStream — the core's phextnet
/// client pattern, extended with a method, one credential
/// header, and a body (the register's writes are POSTs).
fn tcp_request(
    hostport: &str,
    method: &str,
    path: &str,
    header: Option<(&str, &str)>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let connect_addr = if hostport.contains(':') {
        hostport.to_string()
    } else {
        format!("{}:80", hostport)
    };
    let mut stream = TcpStream::connect(&connect_addr)
        .map_err(|e| format!("connect {}: {}", connect_addr, e))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", method, path, hostport);
    if let Some((k, v)) = header {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if let Some(b) = body {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    if let Some(b) = body {
        req.push_str(b);
    }
    stream.write_all(req.as_bytes()).map_err(|e| format!("write {}: {}", connect_addr, e))?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| format!("read {}: {}", connect_addr, e))?;
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response (no header terminator)".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "malformed HTTP status line".to_string())?;
    let body_bytes = &raw[split + 4..];
    let decoded;
    let body: &str = if head.to_lowercase().contains("transfer-encoding: chunked") {
        decoded = dechunk(body_bytes);
        match std::str::from_utf8(&decoded) {
            Ok(s) => s,
            Err(_) => return Err("chunked response body is not UTF-8".into()),
        }
    } else {
        match std::str::from_utf8(body_bytes) {
            Ok(s) => s,
            Err(_) => return Err("response body is not UTF-8".into()),
        }
    };
    Ok((status, body.to_string()))
}

fn dechunk(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = bytes;
    loop {
        let eol = match rest.windows(2).position(|w| w == b"\r\n") {
            Some(p) => p,
            None => break,
        };
        let size_text = String::from_utf8_lossy(&rest[..eol]).to_string();
        let size = match usize::from_str_radix(size_text.trim().split(';').next().unwrap_or(""), 16) {
            Ok(n) => n,
            Err(_) => break,
        };
        rest = &rest[eol + 2..];
        if size == 0 || rest.len() < size {
            break;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size..];
        if rest.starts_with(b"\r\n") {
            rest = &rest[2..];
        }
    }
    out
}

/// The https leg: curl, with its config carried on stdin
/// (`curl -K -`) so the credential header is never in the
/// process list, never in a log, never in a file. The body
/// is data qexo is posting to the register (an entry's
/// text, a close's note) — it rides argv the way curl
/// users have always passed it; the secret never does.
fn curl_request(
    url: &str,
    method: &str,
    header: Option<(&str, &str)>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let mut cfg = String::from("silent\nshow-error\n");
    if let Some((k, v)) = header {
        cfg.push_str(&format!("header = \"{}: {}\"\n", k, cfg_escape(v)));
    }
    if body.is_some() {
        cfg.push_str("header = \"Content-Type: application/json\"\n");
    }
    cfg.push_str("write-out = \"\\n__QEXO_STATUS__:%{http_code}\"\n");
    let mut cmd = std::process::Command::new("curl");
    cmd.arg("-sS").arg("-K").arg("-").arg("-X").arg(method);
    if let Some(b) = body {
        cmd.arg("--data-binary").arg(b);
    }
    cmd.arg(url);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot run curl for the register's https leg: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cfg.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("curl for the register's https leg: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "curl: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    match text.rsplit_once("__QEXO_STATUS__:") {
        Some((body, code)) => {
            let status: u16 = code
                .trim()
                .parse()
                .map_err(|_| "curl response carried an unparseable status".to_string())?;
            Ok((status, body.trim_end_matches('\n').to_string()))
        }
        None => Err("curl response carried no status".into()),
    }
}

fn cfg_escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "")
        .replace('\r', "")
}

// ----------------------------------------------------------------
// JSON at the edge — atoms in, records out; JSON never
// crosses into state (§10's gasket).
// ----------------------------------------------------------------

/// Read a `"key": "string"` field, unescaped. Absent or
/// non-string values are None.
fn json_string_field(obj: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let mut search_from = 0;
    while let Some(rel) = obj[search_from..].find(&needle) {
        let at = search_from + rel;
        let after = obj[at + needle.len()..].trim_start();
        if let Some(rest) = after.strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(s) = rest.strip_prefix('"') {
                return Some(parse_json_string(s));
            }
            return None;
        }
        search_from = at + needle.len();
    }
    None
}

/// Read a `"key": 123` field; a quoted number reads too
/// (foreign writers differ; the value is what matters).
fn json_number_field(obj: &str, key: &str) -> Option<i64> {
    if let Some(s) = json_string_field(obj, key) {
        return s.trim().parse().ok();
    }
    let needle = format!("\"{}\"", key);
    let at = obj.find(&needle)?;
    let after = obj[at + needle.len()..].trim_start().strip_prefix(':')?.trim_start();
    let num: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    num.parse().ok()
}

/// Parse a JSON string's contents (the input starts just
/// past the opening quote) up to its closing quote.
fn parse_json_string(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('b') => out.push('\u{0008}'),
                Some('f') => out.push('\u{000C}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let mut cp = u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD);
                    if (0xD800..0xDC00).contains(&cp) {
                        let mut peek = chars.clone();
                        if peek.next() == Some('\\') && peek.next() == Some('u') {
                            let hex2: String = peek.by_ref().take(4).collect();
                            if let Ok(lo) = u32::from_str_radix(&hex2, 16) {
                                if (0xDC00..0xE000).contains(&lo) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    chars = peek;
                                }
                            }
                        }
                    }
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

/// Escape a string for a JSON body qexo builds.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The object texts of the response's `atoms` array, in
/// order. A brace-matching scan that respects strings —
/// enough for the register's flat atoms, and nothing more
/// is claimed for it.
fn json_atoms(body: &str) -> Result<Vec<String>, String> {
    let at = body.find("\"atoms\"").ok_or("the register's response carries no atoms array")?;
    let open = at + body[at..].find('[').ok_or("the atoms field is not an array")?;
    let bytes = body.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    let mut start: Option<usize> = None;
    let mut out = Vec::new();
    let mut i = open + 1;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => {
                    if depth == 0 {
                        start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start.take() {
                            out.push(body[s..=i].to_string());
                        }
                    }
                }
                ']' => {
                    if depth == 0 {
                        return Ok(out);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    Err("the atoms array is not terminated".into())
}

// ----------------------------------------------------------------
// Atoms
// ----------------------------------------------------------------

/// A register entry atom (§10.1): text in, from a user, at
/// a time, at a seq. The seq is the drain's dedup key.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryAtom {
    pub seq: u64,
    pub text: String,
    pub user: String,
    pub ts: i64,
}

/// A register resolution atom (§10.1): register number,
/// outcome, the user who resolved it, an optional note.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionAtom {
    pub seq: u64,
    pub register_number: String,
    pub user: String,
    pub outcome: String,
    pub note: Option<String>,
    pub ts: i64,
}

fn parse_entry_atom(obj: &str) -> Result<EntryAtom, String> {
    if let Some(t) = json_string_field(obj, "type") {
        if t != "entry" {
            return Err(format!("an atom in the entries log is typed '{}', not 'entry'", t));
        }
    }
    let seq = json_number_field(obj, "seq").ok_or("an entry atom carries no seq")?;
    Ok(EntryAtom {
        seq: seq as u64,
        text: json_string_field(obj, "text").unwrap_or_default(),
        user: json_string_field(obj, "user").unwrap_or_default(),
        ts: json_number_field(obj, "ts").unwrap_or(0),
    })
}

fn parse_resolution_atom(obj: &str) -> Result<ResolutionAtom, String> {
    if let Some(t) = json_string_field(obj, "type") {
        if t != "resolution" {
            return Err(format!("an atom in the resolutions log is typed '{}', not 'resolution'", t));
        }
    }
    let seq = json_number_field(obj, "seq").ok_or("a resolution atom carries no seq")?;
    let number = json_number_field(obj, "register_number")
        .map(|n| n.to_string())
        .or_else(|| json_string_field(obj, "register_number"))
        .ok_or("a resolution atom carries no register_number")?;
    let outcome = json_string_field(obj, "outcome").unwrap_or_default();
    if outcome != "done" && outcome != "cancelled" {
        return Err(format!("a resolution atom's outcome is '{}' — done or cancelled are the register's outcomes", outcome));
    }
    Ok(ResolutionAtom {
        seq: seq as u64,
        register_number: number,
        user: json_string_field(obj, "user").unwrap_or_default(),
        outcome,
        note: json_string_field(obj, "note").filter(|n| !n.trim().is_empty()),
        ts: json_number_field(obj, "ts").unwrap_or(0),
    })
}

/// What one drain pass pulled: both logs past the
/// watermarks, and each log's high-water seq (the
/// service's own `last_seq` — where the watermarks may
/// advance to once everything is filed).
#[derive(Debug, Clone, Default)]
pub struct Fetched {
    pub entries: Vec<EntryAtom>,
    pub resolutions: Vec<ResolutionAtom>,
    pub entries_last_seq: u64,
    pub resolutions_last_seq: u64,
}

fn refusal(what: &str, status: u16, body: &str) -> String {
    match json_string_field(body, "error") {
        Some(e) => format!("the register refused the {} (HTTP {}): {}", what, status, e),
        None => format!("the register refused the {} (HTTP {})", what, status),
    }
}

fn fetch(base: &str, drain_token: &str, after_entries: u64, after_resolutions: u64) -> Result<Fetched, String> {
    let (status, body) = http_request(
        base,
        "GET",
        &format!("/api/entries/pending?after_seq={}", after_entries),
        Some(("X-Drain-Token", drain_token)),
        None,
    )?;
    if status != 200 {
        return Err(refusal("entries drain", status, &body));
    }
    let mut entries = Vec::new();
    for obj in json_atoms(&body)? {
        entries.push(parse_entry_atom(&obj)?);
    }
    entries.sort_by_key(|a| a.seq);
    let entries_last_seq = json_number_field(&body, "last_seq")
        .map(|n| n.max(0) as u64)
        .unwrap_or(after_entries);

    let (status, body) = http_request(
        base,
        "GET",
        &format!("/api/resolutions?after_seq={}", after_resolutions),
        Some(("X-Drain-Token", drain_token)),
        None,
    )?;
    if status != 200 {
        return Err(refusal("resolutions drain", status, &body));
    }
    let mut resolutions = Vec::new();
    for obj in json_atoms(&body)? {
        resolutions.push(parse_resolution_atom(&obj)?);
    }
    resolutions.sort_by_key(|a| a.seq);
    let resolutions_last_seq = json_number_field(&body, "last_seq")
        .map(|n| n.max(0) as u64)
        .unwrap_or(after_resolutions);

    Ok(Fetched { entries, resolutions, entries_last_seq, resolutions_last_seq })
}

// ----------------------------------------------------------------
// The drain engine — pure state work over a fetched pass.
// The caller saves; a refusal here means nothing was
// saved and the watermarks never moved (§4.1).
// ----------------------------------------------------------------

/// A write-back the drain found still owed: a terminal
/// register item whose resolution is not yet recorded at
/// the register. The sweep after the pull retries these.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingWriteback {
    pub book: u32,
    pub addr: String,
    pub number: String,
    pub outcome: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DrainOutcome {
    pub filed: Vec<String>,
    pub closed: Vec<String>,
    pub orphan_audits: Vec<String>,
    pub deduped_entries: usize,
    pub duplicate_resolutions: usize,
    pub writeback_pending: Vec<PendingWriteback>,
}

/// The item record a drained entry is born as (§10.3):
/// the spec's §3.2 shape with the register's fields —
/// `register-seq` in the register slot, `origin: register`,
/// `enqueued:` from the atom's ts, `enqueued-by:` the
/// atom's user. No coordinate, no register number, no
/// value/cost: those are enrichment, a field edit + audit
/// Emise makes afterward — never a rewrite of the entry
/// text, which rides verbatim as the brief (Scroll 2).
fn register_item_record(atom: &EntryAtom) -> String {
    let title = atom.text.lines().next().unwrap_or("").trim();
    let title = if title.is_empty() { "untitled" } else { title };
    let seq = atom.seq.to_string();
    let enqueued = stamp_of_epoch(atom.ts);
    let fields: Vec<(&str, &str)> = vec![
        ("title", title),
        ("register-seq", seq.as_str()),
        ("origin", "register"),
        ("enqueued", enqueued.as_str()),
        ("enqueued-by", atom.user.as_str()),
        ("state", "queued"),
        ("goal", "standing"),
        ("attempts", "0"),
        ("failures", "0"),
    ];
    let mut text = String::from("queue item\n");
    text.push_str(&lattice::record(&fields));
    text
}

pub fn apply_drain(lat: &mut Lattice, fetched: &Fetched, actor: &str) -> Result<DrainOutcome, String> {
    let mut out = DrainOutcome::default();
    let items = dispatch::load_items(&lat.px);
    let mut seen: HashSet<String> = items.iter().filter_map(|i| i.register_seq.clone()).collect();

    // Entries first (§10.3 step 2): each atom becomes an
    // item, unless its seq already stands on one.
    for atom in &fetched.entries {
        let seq_s = atom.seq.to_string();
        if seen.contains(&seq_s) {
            out.deduped_entries += 1;
            continue;
        }
        let book = lattice::next_free_book(&lat.px, [11, 11, 11, lattice::DISPATCH_ITEMS_C, 1], 1);
        lat.px.insert(dispatch::item_addr(book), register_item_record(atom));
        lat.px.insert(dispatch::brief_addr(book), atom.text.clone());
        seen.insert(seq_s);
        out.filed.push(phext::fmt_coord(&dispatch::item_addr(book)));
    }

    // Resolutions (§10.3 step 3): a resolution closes the
    // item that cites its register number. Duplicates are
    // no-ops (the item already stands terminal); a number
    // no item carries files an audit with no home —
    // visible, never silently dropped.
    for atom in &fetched.resolutions {
        let number = atom.register_number.trim();
        let items = dispatch::load_items(&lat.px);
        let target = items.iter().find(|i| i.register.as_deref().map(|r| r.trim()) == Some(number));
        match target {
            None => {
                let showed = format!(
                    "resolution {} for register #{} recorded by {} at {} — no queue item cites register: {}",
                    atom.outcome,
                    number,
                    atom.user,
                    stamp_of_epoch(atom.ts),
                    number
                );
                let addr = dispatch::append_audit(
                    &mut lat.px,
                    "register-close",
                    &format!("register:{}", number),
                    &format!("register resolution seq {} (register link)", atom.seq),
                    &showed,
                    "no-home",
                    actor,
                );
                out.orphan_audits.push(phext::fmt_coord(&addr));
            }
            Some(item) if item.state.map(|s| s.is_terminal()).unwrap_or(false) => {
                out.duplicate_resolutions += 1;
            }
            Some(item) => {
                let book = item.book;
                let addr_s = phext::fmt_coord(&item.addr);
                let to = if atom.outcome == "done" { ItemState::Done } else { ItemState::Cancelled };
                let mut t = Transition::new(to, Actor::Coordinator, actor);
                t.register_close = true;
                t.evidence = match (&atom.note, atom.outcome.as_str()) {
                    (Some(n), _) => n.clone(),
                    (None, _) => format!("register resolution seq {}", atom.seq),
                };
                t.checked = Some(format!(
                    "register resolution seq {} (register link) — the register of record, read at drain time",
                    atom.seq
                ));
                t.showed = Some(match &atom.note {
                    Some(n) => format!(
                        "the register records register #{} {} by {} — note: {}",
                        number, atom.outcome, atom.user, n
                    ),
                    None => format!("the register records register #{} {} by {}", number, atom.outcome, atom.user),
                });
                t.verdict = Some(if atom.outcome == "done" { "closed-settled".to_string() } else { "cancelled".to_string() });
                dispatch::apply_transition(lat, book, &t).map_err(|e| {
                    format!(
                        "resolution seq {} for register #{} could not close {}: {} — nothing was saved; the watermarks never moved",
                        atom.seq, number, addr_s, e
                    )
                })?;
                out.closed.push(addr_s);
            }
        }
    }

    // The write-back sweep's list (§10.4): terminal
    // register items whose resolution is not yet recorded.
    // Items this pass closed came FROM resolutions — the
    // register already holds them; they are never owed.
    let items = dispatch::load_items(&lat.px);
    let receipts = dispatch::load_transition_receipts(&lat.px);
    for item in &items {
        if item.register_writeback.as_deref() != Some("pending") {
            continue;
        }
        let (Some(number), Some(state)) = (&item.register, item.state) else {
            continue;
        };
        if !state.is_terminal() {
            continue;
        }
        let addr_s = phext::fmt_coord(&item.addr);
        let note = if state == ItemState::Cancelled {
            receipts
                .iter()
                .filter(|r| r.item.as_deref() == Some(addr_s.as_str()) && r.to.as_deref() == Some(state.name()))
                .last()
                .and_then(|r| r.evidence.clone())
                .filter(|e| !e.trim().is_empty())
        } else {
            None
        };
        out.writeback_pending.push(PendingWriteback {
            book: item.book,
            addr: addr_s,
            number: number.clone(),
            outcome: state.name().to_string(),
            note,
        });
    }
    Ok(out)
}

// ----------------------------------------------------------------
// Write-back (§10.4) — the qexo service token's legs
// ----------------------------------------------------------------

/// POST a resolution for register number `n` under the
/// qexo service token. Returns the resolution atom's seq
/// at the register. The token authorizes this and the
/// entries-append — nothing else (§10.4, Q2).
pub fn post_resolution(
    base: &str,
    qexo_token: &str,
    number: &str,
    outcome: &str,
    note: Option<&str>,
) -> Result<u64, String> {
    let leg = if outcome == "done" { "done" } else { "cancel" };
    let body = match note {
        Some(n) if !n.trim().is_empty() => format!("{{\"note\":\"{}\"}}", escape_json(n)),
        _ => "{}".to_string(),
    };
    let (status, resp) = http_request(
        base,
        "POST",
        &format!("/api/items/{}/{}", number, leg),
        Some(("X-Qexo-Token", qexo_token)),
        Some(&body),
    )?;
    if status == 201 {
        json_number_field(&resp, "seq")
            .map(|n| n.max(0) as u64)
            .ok_or_else(|| "the register accepted the write-back but returned no seq".to_string())
    } else {
        Err(refusal("write-back", status, &resp))
    }
}

/// POST an entry under the qexo service token (the
/// ratified Q2 leg: items born on the phone appear in the
/// register web view). Returns the entry atom's seq. Its
/// production caller is the phone/app surface (slice 7);
/// slice 3 proves the leg against the double in tests.
#[allow(dead_code)]
pub fn post_entry(base: &str, qexo_token: &str, text: &str) -> Result<u64, String> {
    let body = format!("{{\"text\":\"{}\"}}", escape_json(text));
    let (status, resp) = http_request(base, "POST", "/api/entries", Some(("X-Qexo-Token", qexo_token)), Some(&body))?;
    if status == 201 {
        json_number_field(&resp, "seq")
            .map(|n| n.max(0) as u64)
            .ok_or_else(|| "the register accepted the entry but returned no seq".to_string())
    } else {
        Err(refusal("entries-append", status, &resp))
    }
}

/// Record one owed write-back: post the resolution, then
/// — in one save — flip the item's field to `recorded`
/// and write the receipt that proves it (credential cited
/// by name only). A post that lands but cannot be
/// recorded is named as such; the retry it causes posts a
/// duplicate the register keeps and the drain no-ops —
/// the service's own contract (§10.1).
pub fn write_back(lat: &mut Lattice, item: &dispatch::Item, outcome: &str, note: Option<&str>) -> Result<u64, String> {
    let creds = load_credentials();
    write_back_with(lat, item, outcome, note, &creds)
}

pub fn write_back_with(
    lat: &mut Lattice,
    item: &dispatch::Item,
    outcome: &str,
    note: Option<&str>,
    creds: &Credentials,
) -> Result<u64, String> {
    let number = item
        .register
        .clone()
        .ok_or_else(|| "the item carries no register number — no write-back is owed".to_string())?;
    let record = load_drain_record(&lat.px).ok_or_else(|| {
        "no register link is recorded on this node (no drain record) — the write-back stays pending".to_string()
    })?;
    let token = creds.qexo_token.clone().ok_or_else(|| {
        format!(
            "no qexo service token is configured — {} carries no REGSVC_QEXO_TOKEN (the env file, §10.4); the write-back stays pending",
            credentials_path().display()
        )
    })?;
    let seq = post_resolution(&record.url, &token, &number, outcome, note)?;
    let addr_s = phext::fmt_coord(&item.addr);
    let text = lat
        .px
        .get(&item.addr)
        .cloned()
        .ok_or_else(|| format!("queue item {} vanished before its write-back could be recorded", addr_s))?;
    let text = lattice::set_field(&text, "register-writeback", "recorded");
    lat.px.insert(item.addr, text);
    let seq_s = seq.to_string();
    append_receipt(
        &mut lat.px,
        "register-writeback",
        &[
            ("item", addr_s.as_str()),
            ("register", number.as_str()),
            ("outcome", outcome),
            ("resolution-seq", seq_s.as_str()),
            ("credential", "qexo-token (env)"),
        ],
    );
    lattice::regen_indexes(&mut lat.px);
    lat.save().map_err(|e| {
        format!(
            "resolution seq {} was posted for register #{} but the local record could not be saved: {}",
            seq, number, e
        )
    })?;
    Ok(seq)
}

// ----------------------------------------------------------------
// The verb (§2.1): `qexo drain [--check] [--url <url>]`
// ----------------------------------------------------------------

pub fn drain(check: bool, url: Option<String>) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let creds = load_credentials();
    drain_inner(&mut lat, check, url, &creds)
}

fn drain_inner(lat: &mut Lattice, check: bool, url_arg: Option<String>, creds: &Credentials) -> i32 {
    let existing = load_drain_record(&lat.px);
    let (we, wr) = existing
        .as_ref()
        .map(|r| (r.entries_watermark, r.resolutions_watermark))
        .unwrap_or((0, 0));
    let url = match url_arg.or_else(|| existing.as_ref().map(|r| r.url.clone())) {
        Some(u) => u,
        None => {
            if check {
                println!(
                    "drain --check: no register link is recorded on this node — a first drain names it: qexo drain --url <register-url>"
                );
                return 0;
            }
            eprintln!(
                "qexo drain: no register URL is recorded and none was given — run once with --url <register-url>; it is kept in the drain record (11.11.11/6.1.1/1.1.1)"
            );
            return 2;
        }
    };
    // The drain credential gates the whole pass: without
    // it the drain refuses before anything is read or
    // written — no partial state, ever (§10.4).
    let drain_token = match &creds.drain_token {
        Some(t) => t.clone(),
        None => {
            eprintln!(
                "qexo drain: no drain credential is configured — {} carries no REGSVC_DRAIN_TOKEN (credentials live in the env file, never in state, §10.4)",
                credentials_path().display()
            );
            return 1;
        }
    };
    let fetched = match fetch(&url, &drain_token, we, wr) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("qexo drain: {}", e);
            return 1;
        }
    };
    let new_we = we.max(fetched.entries_last_seq);
    let new_wr = wr.max(fetched.resolutions_last_seq);

    if check {
        // Report what a drain would do, computed against
        // the standing records. Writes nothing — the
        // state's bytes are the proof.
        let items = dispatch::load_items(&lat.px);
        let mut seen: HashSet<String> = items.iter().filter_map(|i| i.register_seq.clone()).collect();
        let (mut would_file, mut deduped) = (0usize, 0usize);
        for a in &fetched.entries {
            if seen.insert(a.seq.to_string()) {
                would_file += 1;
            } else {
                deduped += 1;
            }
        }
        let (mut closes, mut dups, mut orphans) = (0usize, 0usize, 0usize);
        let mut sim_closed: HashSet<String> = HashSet::new();
        for atom in &fetched.resolutions {
            let n = atom.register_number.trim();
            match items.iter().find(|i| i.register.as_deref().map(|r| r.trim()) == Some(n)) {
                None => orphans += 1,
                Some(item) => {
                    let addr_s = phext::fmt_coord(&item.addr);
                    if item.state.map(|s| s.is_terminal()).unwrap_or(false) || sim_closed.contains(&addr_s) {
                        dups += 1;
                    } else {
                        closes += 1;
                        sim_closed.insert(addr_s);
                    }
                }
            }
        }
        let pending = items.iter().filter(|i| i.register_writeback.as_deref() == Some("pending")).count();
        println!("drain --check: register {} — watermarks: entries {}, resolutions {}", url, we, wr);
        println!(
            "drain --check: {} entr(ies) past the watermark: {} would file, {} already filed",
            fetched.entries.len(),
            would_file,
            deduped
        );
        println!(
            "drain --check: {} resolution(s): {} would close, {} duplicate(s), {} with no home (audited, never dropped)",
            fetched.resolutions.len(),
            closes,
            dups,
            orphans
        );
        println!("drain --check: watermarks would advance to entries {}, resolutions {}", new_we, new_wr);
        println!("drain --check: {} write-back(s) pending — a live drain retries them", pending);
        return 0;
    }

    let actor = dispatch::local_actor(&lat.px);
    let outcome = match apply_drain(lat, &fetched, &actor) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("qexo drain: {}", e);
            return 1;
        }
    };
    // Items, audits, and receipts are in the document;
    // the drain receipt and the advanced watermarks join
    // them in the same save (§4.1) — a crash before this
    // save loses nothing but the pass, and the re-drain
    // dedups what it re-reads.
    let (_, stamp) = now_stamp();
    let filed_n = outcome.filed.len().to_string();
    let deduped_n = outcome.deduped_entries.to_string();
    let closed_n = outcome.closed.len().to_string();
    let dup_n = outcome.duplicate_resolutions.to_string();
    let orphan_n = outcome.orphan_audits.len().to_string();
    let receipt = append_receipt(
        &mut lat.px,
        "drain",
        &[
            ("register-url", url.as_str()),
            ("entries-filed", filed_n.as_str()),
            ("entries-deduped", deduped_n.as_str()),
            ("resolutions-closed", closed_n.as_str()),
            ("resolutions-duplicate", dup_n.as_str()),
            ("audits-no-home", orphan_n.as_str()),
            ("entries-watermark", format!("{} -> {}", we, new_we).as_str()),
            ("resolutions-watermark", format!("{} -> {}", wr, new_wr).as_str()),
            ("credential", "drain-token (env)"),
        ],
    );
    let record = DrainRecord {
        url: url.clone(),
        entries_watermark: new_we,
        resolutions_watermark: new_wr,
        last_drain: Some(format!("{} — receipt {}", stamp, phext::fmt_coord(&receipt))),
    };
    lat.px.insert(drain_record_addr(), drain_record_text(&record));
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo drain: cannot save state: {}", e);
        return 1;
    }
    println!(
        "drain: filed {} item(s), closed {}, {} duplicate resolution(s), {} resolution(s) with no home — receipt {}",
        filed_n,
        closed_n,
        dup_n,
        orphan_n,
        phext::fmt_coord(&receipt)
    );
    println!("drain: watermarks — entries {} -> {}, resolutions {} -> {}", we, new_we, wr, new_wr);

    // The write-back sweep (§10.4): what the queue still
    // owes the register, retried now. Each success is its
    // own save (inside write_back_with); each failure
    // stays pending, named, and costs the pass its zero.
    let mut code = 0;
    if !outcome.writeback_pending.is_empty() {
        if creds.qexo_token.is_none() {
            println!(
                "drain: {} write-back(s) stay pending — no qexo service token is configured (env)",
                outcome.writeback_pending.len()
            );
        } else {
            for p in &outcome.writeback_pending {
                let items = dispatch::load_items(&lat.px);
                let item = match items.iter().find(|i| i.book == p.book) {
                    Some(i) => i.clone(),
                    None => continue,
                };
                match write_back_with(lat, &item, &p.outcome, p.note.as_deref(), creds) {
                    Ok(seq) => println!(
                        "drain: write-back recorded — register #{} {} as resolution seq {} (credential: qexo-token (env))",
                        p.number, p.outcome, seq
                    ),
                    Err(e) => {
                        eprintln!("drain: write-back for register #{} failed: {}", p.number, e);
                        code = 1;
                    }
                }
            }
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    // --------------------------------------------------------
    // Units: credentials, stamps, the JSON edge.
    // --------------------------------------------------------

    #[test]
    fn credentials_parse_by_the_services_own_names() {
        let c = parse_credentials(
            "# qexo credentials — provisioned by hand\nREGSVC_DRAIN_TOKEN=drain-value\nREGSVC_QEXO_TOKEN=\"qexo-value\"\nOTHER=ignored\nREGSVC_DRAIN_TOKEN=\n",
        );
        assert_eq!(c.drain_token.as_deref(), Some("drain-value"));
        assert_eq!(c.qexo_token.as_deref(), Some("qexo-value"));
        let none = parse_credentials("");
        assert!(none.drain_token.is_none() && none.qexo_token.is_none());
    }

    #[test]
    fn stamp_of_epoch_uses_the_house_reckoning() {
        assert_eq!(stamp_of_epoch(0), "1970-01-01 00:00 UTC");
        assert_eq!(stamp_of_epoch(1794916800), "2026-11-17 12:00 UTC");
        assert_eq!(stamp_of_epoch(1794920400), "2026-11-17 13:00 UTC");
    }

    #[test]
    fn atoms_parse_escapes_unicode_and_quoted_numbers() {
        let e = parse_entry_atom(
            "{\"type\":\"entry\",\"text\":\"Fix \\\"the thing\\\"\\nsecond line — caf\\u00e9\",\"user\":\"will\",\"ts\":1794916800,\"seq\":3}",
        )
        .unwrap();
        assert_eq!(e.seq, 3);
        assert_eq!(e.text, "Fix \"the thing\"\nsecond line — café");
        assert_eq!(e.user, "will");
        assert_eq!(e.ts, 1794916800);
        let r = parse_resolution_atom(
            "{\"type\":\"resolution\",\"register_number\":\"42\",\"user\":\"will\",\"outcome\":\"cancelled\",\"note\":\"moot\",\"ts\":1794920400,\"seq\":\"5\"}",
        )
        .unwrap();
        assert_eq!(r.register_number, "42");
        assert_eq!(r.outcome, "cancelled");
        assert_eq!(r.note.as_deref(), Some("moot"));
        assert_eq!(r.seq, 5);
    }

    #[test]
    fn atoms_refuse_foreign_shapes() {
        assert!(parse_entry_atom("{\"type\":\"resolution\",\"seq\":1}").is_err());
        assert!(parse_resolution_atom("{\"type\":\"resolution\",\"register_number\":1,\"outcome\":\"postponed\",\"seq\":1}").is_err());
        assert!(parse_entry_atom("{\"type\":\"entry\",\"text\":\"x\"}").is_err());
        let body = "{\"atoms\":[{\"a\":1},{\"b\":\"x } y\"},{\"c\":3}],\"last_seq\":3}";
        assert_eq!(json_atoms(body).unwrap().len(), 3);
        assert_eq!(json_number_field(body, "last_seq"), Some(3));
    }

    // --------------------------------------------------------
    // The drain engine, against born lattices (no wire).
    // --------------------------------------------------------

    fn born_lat(tag: &str) -> Lattice {
        let dir = std::env::temp_dir().join(format!("qexo-register-test-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Lattice::born(dir.join("state.phext"))
    }

    fn entry(seq: u64, text: &str) -> EntryAtom {
        EntryAtom { seq, text: text.to_string(), user: "will".to_string(), ts: 1794916800 }
    }

    fn resolution(seq: u64, number: &str, outcome: &str, note: Option<&str>) -> ResolutionAtom {
        ResolutionAtom {
            seq,
            register_number: number.to_string(),
            user: "will".to_string(),
            outcome: outcome.to_string(),
            note: note.map(|n| n.to_string()),
            ts: 1794920400,
        }
    }

    #[test]
    fn drain_files_each_entry_once_with_its_text_verbatim() {
        let mut lat = born_lat("file-once");
        let fetched = Fetched {
            entries: vec![entry(1, "Write the thing\nwith details"), entry(2, "Second entry")],
            resolutions: vec![],
            entries_last_seq: 2,
            resolutions_last_seq: 0,
        };
        let out = apply_drain(&mut lat, &fetched, "test-node TESTFP").unwrap();
        assert_eq!(out.filed.len(), 2);
        // a re-drain of the same atoms files nothing twice
        let out = apply_drain(&mut lat, &fetched, "test-node TESTFP").unwrap();
        assert_eq!(out.filed.len(), 0);
        assert_eq!(out.deduped_entries, 2);
        let items = dispatch::load_items(&lat.px);
        assert_eq!(items.len(), 2);
        let one = items.iter().find(|i| i.register_seq.as_deref() == Some("1")).unwrap();
        assert_eq!(one.origin, "register");
        assert_eq!(one.state, Some(ItemState::Queued));
        assert_eq!(one.enqueued, "2026-11-17 12:00 UTC");
        assert_eq!(one.enqueued_by, "will");
        assert_eq!(one.title, "Write the thing");
        assert_eq!(one.register, None);
        assert_eq!(
            lat.px.get(&dispatch::brief_addr(one.book)).map(|s| s.as_str()),
            Some("Write the thing\nwith details")
        );
    }

    #[test]
    fn drain_closes_matches_audits_orphans_and_noops_duplicates() {
        let mut lat = born_lat("closes");
        let rec = dispatch::item_record(
            "Standing wait",
            None,
            Some("42"),
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            None,
            None,
            None,
            None,
        );
        lat.px.insert(dispatch::item_addr(1), rec);
        let fetched = Fetched {
            entries: vec![],
            resolutions: vec![resolution(1, "42", "done", None), resolution(2, "99", "cancelled", Some("gone"))],
            entries_last_seq: 0,
            resolutions_last_seq: 2,
        };
        let out = apply_drain(&mut lat, &fetched, "test-node TESTFP").unwrap();
        assert_eq!(out.closed.len(), 1);
        assert_eq!(out.orphan_audits.len(), 1);
        let items = dispatch::load_items(&lat.px);
        assert_eq!(items[0].state, Some(ItemState::Done));
        assert_eq!(items[0].register_writeback.as_deref(), Some("recorded"));
        let audits = dispatch::load_audits(&lat.px);
        assert!(audits.iter().any(|a| a.kind == "register-close" && a.item.as_deref() == Some("11.11.11/2.1.1/1.1.1")));
        assert!(audits.iter().any(|a| a.item.as_deref() == Some("register:99") && a.verdict.as_deref() == Some("no-home")));
        // the same resolution again: a no-op, not a second close
        let out = apply_drain(&mut lat, &fetched, "test-node TESTFP").unwrap();
        assert_eq!(out.closed.len(), 0);
        assert_eq!(out.duplicate_resolutions, 1);
    }

    #[test]
    fn drain_collects_pending_writebacks_with_the_cancel_note() {
        let mut lat = born_lat("pending");
        let rec = dispatch::item_record(
            "Local wait",
            None,
            Some("7"),
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            None,
            None,
            None,
            None,
        );
        lat.px.insert(dispatch::item_addr(1), rec);
        let mut cancel = Transition::new(ItemState::Cancelled, Actor::WillEmise, "test-node TESTFP");
        cancel.evidence = "moot".into();
        dispatch::apply_transition(&mut lat, 1, &cancel).unwrap();
        let out = apply_drain(&mut lat, &Fetched::default(), "test-node TESTFP").unwrap();
        assert_eq!(out.writeback_pending.len(), 1);
        let p = &out.writeback_pending[0];
        assert_eq!(p.number, "7");
        assert_eq!(p.outcome, "cancelled");
        assert_eq!(p.note.as_deref(), Some("moot"));
    }

    // --------------------------------------------------------
    // The scoped double: the register service's §10.2
    // semantics in miniature — per-scope logs, credentials
    // bound to a scope, the drain token read-only, the
    // qexo token write-only, fail-closed throughout.
    // --------------------------------------------------------

    #[derive(Default)]
    struct Scope {
        entries: Vec<String>,
        resolutions: Vec<String>,
        numbers: Vec<i64>,
    }

    #[derive(Default)]
    struct DoubleState {
        will: Scope,
        other: Scope,
    }

    fn entry_atom(seq: i64, text: &str, user: &str, ts: i64) -> String {
        format!(
            "{{\"type\":\"entry\",\"text\":\"{}\",\"user\":\"{}\",\"ts\":{},\"seq\":{}}}",
            escape_json(text),
            escape_json(user),
            ts,
            seq
        )
    }

    fn resolution_atom(seq: i64, number: i64, user: &str, outcome: &str, note: Option<&str>, ts: i64) -> String {
        let note = match note {
            Some(n) => format!(",\"note\":\"{}\"", escape_json(n)),
            None => String::new(),
        };
        format!(
            "{{\"type\":\"resolution\",\"register_number\":{},\"user\":\"{}\",\"outcome\":\"{}\"{},\"ts\":{},\"seq\":{}}}",
            number,
            escape_json(user),
            outcome,
            note,
            ts,
            seq
        )
    }

    fn scope_for<'a>(st: &'a mut DoubleState, kind: &str, token: Option<&str>) -> Option<&'a mut Scope> {
        match (kind, token) {
            ("drain", Some("will-drain-token")) => Some(&mut st.will),
            ("drain", Some("other-drain-token")) => Some(&mut st.other),
            ("qexo", Some("will-qexo-token")) => Some(&mut st.will),
            ("qexo", Some("other-qexo-token")) => Some(&mut st.other),
            _ => None,
        }
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn route(method: &str, path: &str, headers: &[(String, String)], body: &str, st: &mut DoubleState) -> (u16, String) {
        let err = |status: u16, msg: &str| (status, format!("{{\"error\":\"{}\"}}", msg));
        if method == "GET" && path == "/api/health" {
            return (200, "{\"ok\":true}".to_string());
        }
        if method == "GET" && (path.starts_with("/api/entries/pending") || path.starts_with("/api/resolutions")) {
            let scope = match scope_for(st, "drain", header(headers, "x-drain-token")) {
                Some(s) => s,
                None => return err(401, "not logged in"),
            };
            let after: u64 = path
                .split("after_seq=")
                .nth(1)
                .and_then(|rest| {
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    digits.parse().ok()
                })
                .unwrap_or(0);
            let log = if path.starts_with("/api/entries") { &scope.entries } else { &scope.resolutions };
            let atoms: Vec<&String> = log
                .iter()
                .filter(|a| json_number_field(a, "seq").map(|s| s.max(0) as u64).unwrap_or(0) > after)
                .collect();
            let last = log.iter().filter_map(|a| json_number_field(a, "seq")).max().unwrap_or(0);
            let joined = atoms.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(",");
            return (200, format!("{{\"atoms\":[{}],\"last_seq\":{}}}", joined, last));
        }
        if method == "POST" && path == "/api/entries" {
            let scope = match scope_for(st, "qexo", header(headers, "x-qexo-token")) {
                Some(s) => s,
                None => return err(401, "not logged in"),
            };
            let text = json_string_field(body, "text").unwrap_or_default();
            if text.trim().is_empty() {
                return err(400, "text required");
            }
            let seq = scope.entries.len() as i64 + 1;
            scope.entries.push(entry_atom(seq, &text, "qexo", 1794916800));
            return (201, format!("{{\"ok\":true,\"seq\":{}}}", seq));
        }
        if method == "POST" && path.starts_with("/api/items/") {
            let parts: Vec<&str> = path.split('/').collect();
            let scope = match scope_for(st, "qexo", header(headers, "x-qexo-token")) {
                Some(s) => s,
                None => return err(401, "not logged in"),
            };
            let number: i64 = parts.get(3).and_then(|p| p.parse().ok()).unwrap_or(-1);
            if !scope.numbers.contains(&number) {
                return err(404, "unknown register number");
            }
            let outcome = if parts.get(4) == Some(&"done") { "done" } else { "cancelled" };
            let note = json_string_field(body, "note");
            let seq = scope.resolutions.len() as i64 + 1;
            scope.resolutions.push(resolution_atom(seq, number, "qexo", outcome, note.as_deref(), 1794916800));
            return (201, format!("{{\"ok\":true,\"seq\":{}}}", seq));
        }
        err(404, "no such endpoint")
    }

    fn read_request(stream: &mut TcpStream) -> (String, String, Vec<(String, String)>, String) {
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        let head_end = loop {
            if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                break p + 4;
            }
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => return (String::new(), String::new(), Vec::new(), String::new()),
                Ok(n) => raw.extend_from_slice(&buf[..n]),
            }
        };
        let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
        let mut lines = head.lines();
        let first = lines.next().unwrap_or_default().to_string();
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let mut headers = Vec::new();
        let mut content_length = 0usize;
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim().to_lowercase();
                let v = v.trim().to_string();
                if k == "content-length" {
                    content_length = v.parse().unwrap_or(0);
                }
                headers.push((k, v));
            }
        }
        let mut body = raw[head_end..].to_vec();
        while body.len() < content_length {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
            }
        }
        body.truncate(content_length);
        (method, path, headers, String::from_utf8_lossy(&body).to_string())
    }

    fn start_double(state: DoubleState) -> (String, Arc<Mutex<DoubleState>>) {
        let st = Arc::new(Mutex::new(state));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let serve = st.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut s) => {
                        let (method, path, headers, body) = read_request(&mut s);
                        if method.is_empty() {
                            continue;
                        }
                        let (status, resp) = {
                            let mut guard = serve.lock().unwrap();
                            route(&method, &path, &headers, &body, &mut guard)
                        };
                        let out = format!(
                            "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            status,
                            resp.len(),
                            resp
                        );
                        let _ = s.write_all(out.as_bytes());
                    }
                    Err(_) => break,
                }
            }
        });
        (format!("http://127.0.0.1:{}", port), st)
    }

    fn will_creds() -> Credentials {
        Credentials {
            drain_token: Some("will-drain-token".to_string()),
            qexo_token: Some("will-qexo-token".to_string()),
        }
    }

    #[test]
    fn the_full_drain_cycle_against_the_scoped_double() {
        let (base, st) = start_double(DoubleState {
            will: Scope {
                entries: vec![
                    entry_atom(1, "Write the thing\nwith details", "will", 1794916800),
                    entry_atom(2, "Second entry", "will", 1794920400),
                ],
                resolutions: vec![],
                numbers: vec![42, 7],
            },
            other: Scope {
                entries: vec![entry_atom(1, "Foreign entry", "other", 1794916800)],
                resolutions: vec![],
                numbers: vec![],
            },
        });
        let creds = will_creds();
        let dir = std::env::temp_dir().join(format!("qexo-register-cycle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lat = Lattice::born(dir.join("state.phext"));
        let rec = dispatch::item_record(
            "Standing wait",
            None,
            Some("42"),
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            None,
            None,
            None,
            None,
        );
        lat.px.insert(dispatch::item_addr(1), rec);
        lat.save().unwrap();

        // Pass 1: entries file as items; the standing item
        // stands; the foreign scope never surfaces.
        assert_eq!(drain_inner(&mut lat, false, Some(base.clone()), &creds), 0);
        let items = dispatch::load_items(&lat.px);
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.title != "Foreign entry"));
        let one = items.iter().find(|i| i.register_seq.as_deref() == Some("1")).unwrap();
        assert_eq!(one.enqueued, "2026-11-17 12:00 UTC");
        assert_eq!(
            lat.px.get(&dispatch::brief_addr(one.book)).map(|s| s.as_str()),
            Some("Write the thing\nwith details")
        );
        let rec_now = load_drain_record(&lat.px).unwrap();
        assert_eq!((rec_now.entries_watermark, rec_now.resolutions_watermark), (2, 0));
        assert_eq!(rec_now.url, base);

        // Pass 2: idempotent — a double drain files nothing twice.
        assert_eq!(drain_inner(&mut lat, false, None, &creds), 0);
        assert_eq!(dispatch::load_items(&lat.px).len(), 3);

        // --check is byte-inert: the state file does not move.
        let path = dir.join("state.phext");
        let before = std::fs::read(&path).unwrap();
        assert_eq!(drain_inner(&mut lat, true, None, &creds), 0);
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // A resolution for #42 closes the standing item with
        // its register-close audit — and is NOT echoed back.
        st.lock().unwrap().will.resolutions.push(resolution_atom(1, 42, "will", "done", None, 1794920400));
        assert_eq!(drain_inner(&mut lat, false, None, &creds), 0);
        let items = dispatch::load_items(&lat.px);
        let standing = items.iter().find(|i| i.register.as_deref() == Some("42")).unwrap();
        assert_eq!(standing.state, Some(ItemState::Done));
        let audits = dispatch::load_audits(&lat.px);
        assert!(audits
            .iter()
            .any(|a| a.kind == "register-close" && a.item.as_deref() == Some("11.11.11/2.1.1/1.1.1")));
        assert_eq!(st.lock().unwrap().will.resolutions.len(), 1);

        // A resolution with no home: audited, never dropped.
        st.lock().unwrap().will.resolutions.push(resolution_atom(2, 99, "will", "cancelled", Some("gone"), 1794920400));
        assert_eq!(drain_inner(&mut lat, false, None, &creds), 0);
        let audits = dispatch::load_audits(&lat.px);
        assert!(audits
            .iter()
            .any(|a| a.item.as_deref() == Some("register:99") && a.verdict.as_deref() == Some("no-home")));
        let rec_now = load_drain_record(&lat.px).unwrap();
        assert_eq!((rec_now.entries_watermark, rec_now.resolutions_watermark), (2, 2));

        // Write-back: a local cancel of #7 posts its
        // resolution under the token's identity.
        let rec7 = dispatch::item_record(
            "Local wait",
            None,
            Some("7"),
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            None,
            None,
            None,
            None,
        );
        let book7 = lattice::next_free_book(&lat.px, [11, 11, 11, lattice::DISPATCH_ITEMS_C, 1], 1);
        lat.px.insert(dispatch::item_addr(book7), rec7);
        let mut cancel = Transition::new(ItemState::Cancelled, Actor::WillEmise, "test-node TESTFP");
        cancel.evidence = "moot".into();
        dispatch::apply_transition(&mut lat, book7, &cancel).unwrap();
        lat.save().unwrap();
        let items = dispatch::load_items(&lat.px);
        let item7 = items.iter().find(|i| i.book == book7).unwrap().clone();
        assert_eq!(item7.register_writeback.as_deref(), Some("pending"));
        let seq = write_back_with(&mut lat, &item7, "cancelled", Some("moot"), &creds).unwrap();
        assert_eq!(seq, 3);
        {
            let guard = st.lock().unwrap();
            assert_eq!(guard.will.resolutions.len(), 3);
            let atom = &guard.will.resolutions[2];
            assert_eq!(json_string_field(atom, "user").as_deref(), Some("qexo"));
            assert_eq!(json_string_field(atom, "outcome").as_deref(), Some("cancelled"));
            assert_eq!(json_string_field(atom, "note").as_deref(), Some("moot"));
            assert_eq!(guard.other.resolutions.len(), 0);
        }
        let items = dispatch::load_items(&lat.px);
        assert_eq!(
            items.iter().find(|i| i.book == book7).unwrap().register_writeback.as_deref(),
            Some("recorded")
        );

        // Entries-append (Q2): the token's entry lands in
        // Will's scope under its identity; the next drain
        // files it.
        let seq = post_entry(&base, "will-qexo-token", "Phone-born item").unwrap();
        assert_eq!(seq, 3);
        {
            let guard = st.lock().unwrap();
            assert_eq!(json_string_field(&guard.will.entries[2], "user").as_deref(), Some("qexo"));
            assert_eq!(guard.other.entries.len(), 1);
        }
        assert_eq!(drain_inner(&mut lat, false, None, &creds), 0);
        let items = dispatch::load_items(&lat.px);
        assert!(items.iter().any(|i| i.register_seq.as_deref() == Some("3") && i.title == "Phone-born item"));

        // Scope negatives at the wire: the qexo token
        // cannot read, the drain token cannot write, a
        // wrong token does nothing, and the other scope's
        // drain reads only its own atoms.
        let (status, _) = http_request(
            &base,
            "GET",
            "/api/entries/pending?after_seq=0",
            Some(("X-Qexo-Token", "will-qexo-token")),
            None,
        )
        .unwrap();
        assert_eq!(status, 401);
        let (status, _) = http_request(
            &base,
            "POST",
            "/api/entries",
            Some(("X-Drain-Token", "will-drain-token")),
            Some("{\"text\":\"sneak\"}"),
        )
        .unwrap();
        assert_eq!(status, 401);
        let (status, _) = http_request(
            &base,
            "POST",
            "/api/entries",
            Some(("X-Qexo-Token", "wrong-token")),
            Some("{\"text\":\"sneak\"}"),
        )
        .unwrap();
        assert_eq!(status, 401);
        let (status, body) = http_request(
            &base,
            "GET",
            "/api/entries/pending?after_seq=0",
            Some(("X-Drain-Token", "other-drain-token")),
            None,
        )
        .unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("Foreign entry") && !body.contains("Write the thing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drain_refuses_cleanly_without_a_token_and_when_unreachable() {
        let (base, _st) = start_double(DoubleState::default());
        let dir = std::env::temp_dir().join(format!("qexo-register-refuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.phext");
        let mut lat = Lattice::born(path.clone());
        lat.save().unwrap();
        let before = std::fs::read(&path).unwrap();
        // no credentials at all: a clean refusal, no state change
        assert_eq!(drain_inner(&mut lat, false, Some(base.clone()), &Credentials::default()), 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // a drain token but a dead register: same discipline
        let creds = will_creds();
        assert_eq!(drain_inner(&mut lat, false, Some("http://127.0.0.1:1".to_string()), &creds), 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // and with no URL anywhere: a named refusal, exit 2
        assert_eq!(drain_inner(&mut lat, false, None, &creds), 2);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_foreign_scope_fixture_drains_to_empty() {
        // The presented credential's scope is empty; the
        // foreign scope holds atoms. The drain must file
        // nothing — scope is the credential's, and this
        // credential's scope has nothing in it.
        let (base, _st) = start_double(DoubleState {
            will: Scope::default(),
            other: Scope {
                entries: vec![
                    entry_atom(1, "Foreign one", "other", 1794916800),
                    entry_atom(2, "Foreign two", "other", 1794916800),
                ],
                resolutions: vec![resolution_atom(1, 5, "other", "done", None, 1794916800)],
                numbers: vec![5],
            },
        });
        let dir = std::env::temp_dir().join(format!("qexo-register-foreign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lat = Lattice::born(dir.join("state.phext"));
        lat.save().unwrap();
        assert_eq!(drain_inner(&mut lat, false, Some(base), &will_creds()), 0);
        assert_eq!(dispatch::load_items(&lat.px).len(), 0);
        assert_eq!(dispatch::load_audits(&lat.px).len(), 0);
        let rec = load_drain_record(&lat.px).unwrap();
        assert_eq!((rec.entries_watermark, rec.resolutions_watermark), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
