//! qexo (formerly orchestrator) — Will's Rust dispatcher, first version. Renamed to qexo 2026-11-08 (v1.1).
//!
//! Reads the open-task register (tasks-source.json), the disk picture
//! (disk-watch state.json), and a live snapshot of the host it runs on,
//! and emits a placement plan in the four currencies:
//!   inference        -> Emise (fixed premise: Emise does the inference lifting)
//!   disk / CPU / RAM -> ranch nodes for heavy execution; the VM only
//!                       coordinates and verifies (it is a small shared cell)
//!
//! Placement rules (v1, deliberately legible):
//!   - a task whose title/request names video/render/film work is class VIDEO
//!     -> heavy execution on the ranch, never on the VM (standing rule).
//!   - a task naming build/compile/test/benchmark/bulk work is class BUILD
//!     -> heavy execution on the ranch (aletheia-core is the default node).
//!   - anything else is class LIGHT -> VM-light coordination, no heavy exec.
//!   - every task's inference routes to Emise, whatever its class.
//!
//! Zero external crates: JSON is parsed with the small parser below so the
//! crate builds on a ranch node with no network access.
//!
//! Usage: qexo [tasks.json] [disk-state.json] [out-dir]
//! Defaults point at the live files under ~/workspace.
//!
//! v2 subcommands (see DESIGN.md):
//!   qexo sync-manifest <dir>                 write MANIFEST.sha256 for a state dir
//!   qexo sync-diff <old-manifest> <new-manifest>
//!                                            print the delta file list between manifests
//!   qexo interfaces                          print the five interior interface readings
//!   qexo personas                            list persona definitions found locally
//! With no subcommand, v1's plan behavior runs unchanged.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::exit;

// ---------------------------------------------------------------------------
// Minimal JSON parser (std only). Enough for the registers we read.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn as_arr(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    fn as_num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse(text: &'a str) -> Result<Json, String> {
        let mut p = Parser { bytes: text.as_bytes(), pos: 0 };
        p.skip_ws();
        let v = p.value()?;
        Ok(v)
    }
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }
    fn peek(&self) -> Result<u8, String> {
        self.bytes.get(self.pos).copied().ok_or_else(|| "unexpected end of input".to_string())
    }
    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek()? {
            b'{' => {
                self.pos += 1;
                let mut pairs = Vec::new();
                self.skip_ws();
                if self.peek()? == b'}' {
                    self.pos += 1;
                    return Ok(Json::Obj(pairs));
                }
                loop {
                    self.skip_ws();
                    let key = match self.value()? {
                        Json::Str(s) => s,
                        _ => return Err("object key must be a string".into()),
                    };
                    self.skip_ws();
                    if self.peek()? != b':' {
                        return Err("expected ':' in object".into());
                    }
                    self.pos += 1;
                    let val = self.value()?;
                    pairs.push((key, val));
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b'}' => {
                            self.pos += 1;
                            return Ok(Json::Obj(pairs));
                        }
                        c => return Err(format!("expected ',' or '}}' in object, got '{}'", c as char)),
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek()? == b']' {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.skip_ws();
                    match self.peek()? {
                        b',' => self.pos += 1,
                        b']' => {
                            self.pos += 1;
                            return Ok(Json::Arr(items));
                        }
                        c => return Err(format!("expected ',' or ']' in array, got '{}'", c as char)),
                    }
                }
            }
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => {
                self.lit("true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.lit("false")?;
                Ok(Json::Bool(false))
            }
            b'n' => {
                self.lit("null")?;
                Ok(Json::Null)
            }
            _ => self.number(),
        }
    }
    fn lit(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(format!("invalid literal at {}", self.pos))
        }
    }
    fn string(&mut self) -> Result<String, String> {
        // assumes current char is the opening quote
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
                                .map_err(|_| "bad \\u escape")?;
                            let code = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u escape")?;
                            self.pos += 4;
                            // surrogate pairs: combine when a low surrogate follows
                            if (0xD800..0xDC00).contains(&code) && self.bytes[self.pos..].starts_with(b"\\u") {
                                let hex2 = std::str::from_utf8(&self.bytes[self.pos + 2..self.pos + 6])
                                    .map_err(|_| "bad \\u escape")?;
                                if let Ok(lo) = u32::from_str_radix(hex2, 16) {
                                    if (0xDC00..0xE000).contains(&lo) {
                                        self.pos += 6;
                                        let c = 0x10000 + ((code - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                                        continue;
                                    }
                                }
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err("bad escape".into()),
                    }
                }
                _ => {
                    // raw UTF-8 byte(s): copy the full char
                    let start = self.pos - 1;
                    let len = utf8_len(c);
                    let end = start + len;
                    let s = std::str::from_utf8(&self.bytes[start..end]).map_err(|_| "bad utf-8")?;
                    out.push_str(s);
                    self.pos = end;
                }
            }
        }
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "bad number")?;
        text.parse::<f64>().map(Json::Num).map_err(|_| format!("bad number '{}'", text))
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

fn json_escape(s: &str) -> String {
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

// ---------------------------------------------------------------------------
// Placement rules (v1)
// ---------------------------------------------------------------------------

const VIDEO_TOKENS: &[&str] = &[
    "video", "film", "ffmpeg", "encode", "encoding", "render", "rendering",
    "wavefront", "frame", "frames", "mp4", "mkv", "v14", "v15", "v16", "transcode",
];
const BUILD_TOKENS: &[&str] = &[
    "build", "builds", "compile", "compiler", "cargo", "test", "tests", "testing",
    "benchmark", "bulk", "suite", "phcc", "render", // render overlaps video; video wins first
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Video,
    Build,
    Light,
}

impl Class {
    fn name(&self) -> &'static str {
        match self {
            Class::Video => "video",
            Class::Build => "build",
            Class::Light => "light",
        }
    }
    fn execution(&self) -> &'static str {
        match self {
            Class::Video => "ranch:aletheia-core (video — never on VM)",
            Class::Build => "ranch:aletheia-core",
            Class::Light => "vm:light-coordination",
        }
    }
}

fn classify(title: &str, request: &str) -> Class {
    let text = format!("{} {}", title, request).to_lowercase();
    let tokens: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let has = |set: &[&str]| tokens.iter().any(|t| set.contains(t));
    if has(VIDEO_TOKENS) {
        Class::Video
    } else if has(BUILD_TOKENS) {
        Class::Build
    } else {
        Class::Light
    }
}

// ---------------------------------------------------------------------------
// Host snapshot (of whatever machine qexo runs on)
// ---------------------------------------------------------------------------

fn host_snapshot() -> (f64, f64) {
    // (MemAvailable MB, loadavg 1min)
    let mut avail_mb = 0.0;
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemAvailable:") {
                if let Some(kb) = line.split_whitespace().nth(1) {
                    avail_mb = kb.parse::<f64>().unwrap_or(0.0) / 1024.0;
                }
                break;
            }
        }
    }
    let mut load1 = 0.0;
    if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
        if let Some(first) = loadavg.split_whitespace().next() {
            load1 = first.parse::<f64>().unwrap_or(0.0);
        }
    }
    (avail_mb, load1)
}

// ---------------------------------------------------------------------------
// SHA-256 (std only) — needed for sync manifests; keeps the crate
// zero-dependency so it builds anywhere cargo exists.
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buflen: usize,
    total: u64,
}

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buflen: 0,
            total: 0,
        }
    }
    fn compress(&mut self, block: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h) = (
            self.state[0], self.state[1], self.state[2], self.state[3],
            self.state[4], self.state[5], self.state[6], self.state[7],
        );
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g; g = f; f = e;
            e = d.wrapping_add(t1);
            d = c; c = b; b = a;
            a = t1.wrapping_add(t2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
    fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buflen > 0 {
            let take = std::cmp::min(64 - self.buflen, data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buflen = 0;
            }
        }
        while data.len() >= 64 {
            self.compress(&data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buflen = data.len();
        }
    }
    fn finalize(mut self) -> [u8; 32] {
        let bitlen = self.total.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buflen != 56 {
            self.update(&[0]);
        }
        // append length manually (update() would count it in total)
        self.buf[56..64].copy_from_slice(&bitlen.to_be_bytes());
        let block = self.buf;
        self.compress(&block);
        let mut out = [0u8; 32];
        for i in 0..8 {
            out[4 * i..4 * i + 4].copy_from_slice(&self.state[i].to_be_bytes());
        }
        out
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_file_hex(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut h = Sha256::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = f.read(&mut chunk).map_err(|e| format!("{}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        h.update(&chunk[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

// ---------------------------------------------------------------------------
// Self-sync (v2): manifest + diff. The protocol lives in DESIGN.md; these
// subcommands are its local halves — any node's qexo can build a manifest,
// diff two manifests, and (over the transport of the day) push/pull/verify.
// Manifest format is the one proven by the 2026-11-08 first ranch backup:
// sha256sum lines, "<hex>  <relative/path>", sorted, MANIFEST.sha256
// itself excluded.
// ---------------------------------------------------------------------------

fn walk_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let ft = match e.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_dir() {
            walk_files(root, &p, out);
        } else if ft.is_file() {
            if let Ok(rel) = p.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if rel != "MANIFEST.sha256" {
                    out.push(rel);
                }
            }
        }
    }
}

fn cmd_sync_manifest(args: &[String]) -> i32 {
    let dir = match args.get(2) {
        Some(d) => d.clone(),
        None => {
            eprintln!("usage: qexo sync-manifest <dir>");
            return 2;
        }
    };
    let root = std::path::PathBuf::from(&dir);
    if !root.is_dir() {
        eprintln!("qexo sync-manifest: {} is not a directory", dir);
        return 1;
    }
    let mut files = Vec::new();
    walk_files(&root, &root, &mut files);
    files.sort();
    let mut lines = String::new();
    let mut total_bytes: u64 = 0;
    for rel in &files {
        let p = root.join(rel);
        match sha256_file_hex(&p) {
            Ok(hex) => {
                total_bytes += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                lines.push_str(&format!("{}  {}\n", hex, rel));
            }
            Err(e) => {
                eprintln!("qexo sync-manifest: {}", e);
                return 1;
            }
        }
    }
    let manifest_path = root.join("MANIFEST.sha256");
    if let Err(e) = fs::write(&manifest_path, &lines) {
        eprintln!("qexo sync-manifest: cannot write {}: {}", manifest_path.display(), e);
        return 1;
    }
    println!(
        "sync-manifest: {} files, {} bytes -> {}",
        files.len(),
        total_bytes,
        manifest_path.display()
    );
    0
}

fn parse_manifest(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.len() < 66 {
            continue;
        }
        let (hex, rest) = line.split_at(64);
        let path = rest.trim_start_matches([' ', '*']);
        if !path.is_empty() {
            map.insert(path.to_string(), hex.to_string());
        }
    }
    map
}

fn cmd_sync_diff(args: &[String]) -> i32 {
    let (old_path, new_path) = match (args.get(2), args.get(3)) {
        (Some(a), Some(b)) => (a.clone(), b.clone()),
        _ => {
            eprintln!("usage: qexo sync-diff <old-manifest> <new-manifest>");
            return 2;
        }
    };
    let old_text = match fs::read_to_string(&old_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("qexo sync-diff: cannot read {}: {}", old_path, e);
            return 1;
        }
    };
    let new_text = match fs::read_to_string(&new_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("qexo sync-diff: cannot read {}: {}", new_path, e);
            return 1;
        }
    };
    let old = parse_manifest(&old_text);
    let new = parse_manifest(&new_text);
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for (path, hex) in &new {
        match old.get(path) {
            None => added.push(path.clone()),
            Some(h) if h != hex => changed.push(path.clone()),
            _ => {}
        }
    }
    for path in old.keys() {
        if !new.contains_key(path) {
            removed.push(path.clone());
        }
    }
    println!("sync-diff: {} -> {}", old_path, new_path);
    println!("  added:   {}", added.len());
    for p in &added {
        println!("    + {}", p);
    }
    println!("  changed: {}", changed.len());
    for p in &changed {
        println!("    ~ {}", p);
    }
    println!("  removed: {}", removed.len());
    for p in &removed {
        println!("    - {}", p);
    }
    println!(
        "  delta: {} files to move ({} added + {} changed), {} to delete",
        added.len() + changed.len(),
        added.len(),
        changed.len(),
        removed.len()
    );
    0
}

// ---------------------------------------------------------------------------
// Interior interfaces (v2): one shape for temperature, emotion, goals,
// desires, dreams — current reading + source + updated-at, projected from
// the surfaces that are the sources of truth (see DESIGN.md). Every
// interface degrades honestly: an unreachable surface prints
// "unavailable: <why>", never an invented reading.
// ---------------------------------------------------------------------------

fn fmt_epoch_utc(secs: u64) -> String {
    // civil-from-days (Hinnant), no external crates
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        y, m, d, rem / 3600, (rem % 3600) / 60
    )
}

fn file_updated_at(path: &str) -> String {
    match fs::metadata(path) {
        Ok(md) => match md.modified() {
            Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => fmt_epoch_utc(d.as_secs()),
                Err(_) => "unknown".to_string(),
            },
            Err(_) => "unknown".to_string(),
        },
        Err(_) => "unknown".to_string(),
    }
}

/// Split a phext file into its non-empty scroll texts (dead-reckoning over
/// the 9 delimiter bytes — enough to find timestamped distillations).
fn phext_scrolls(path: &str) -> Vec<String> {
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let delims: [u8; 9] = [0x01, 0x1F, 0x1E, 0x1D, 0x1C, 0x1A, 0x19, 0x18, 0x17];
    let mut scrolls = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    for b in data {
        if delims.contains(&b) {
            if !cur.is_empty() {
                let s = String::from_utf8_lossy(&cur).trim().to_string();
                if !s.is_empty() {
                    scrolls.push(s);
                }
                cur = Vec::new();
            }
        } else {
            cur.push(b);
        }
    }
    if !cur.is_empty() {
        let s = String::from_utf8_lossy(&cur).trim().to_string();
        if !s.is_empty() {
            scrolls.push(s);
        }
    }
    scrolls
}

/// Timestamped distillations in state.phext start "YYYY-MM-DD HH:MM ... |".
/// Returns (timestamp-string, text) pairs sorted by timestamp.
fn state_distillations(home: &str) -> Vec<(String, String)> {
    let path = format!("{}/workspace/phext/state.phext", home);
    let mut out: Vec<(String, String)> = phext_scrolls(&path)
        .into_iter()
        .filter(|s| {
            s.len() > 17
                && s.as_bytes()[4] == b'-'
                && s.as_bytes()[7] == b'-'
                && s.as_bytes()[13] == b':'
                && s.chars().take(10).all(|c| c.is_ascii_digit() || c == '-')
        })
        .map(|s| (s[..16].to_string(), s))
        .collect();
    out.sort();
    out
}

fn iface_temperature(home: &str) {
    println!("temperature — ranch thermal state");
    let path = format!("{}/workspace/state-sampler/hidden_files/last-pull.json", home);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            println!("  unavailable: cannot read state sampler output {}: {}", path, e);
            return;
        }
    };
    let json = match Parser::parse(&text) {
        Ok(j) => j,
        Err(e) => {
            println!("  unavailable: cannot parse {}: {}", path, e);
            return;
        }
    };
    let ts = json.get("ts").and_then(|v| v.as_str()).unwrap_or("?");
    let summary = json.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    println!("  reading ({}): {}", ts, summary);
    let empty = Vec::new();
    let nodes = json.get("nodes").and_then(|n| n.as_arr()).unwrap_or(&empty);
    let mut sum = 0.0;
    let mut n = 0usize;
    let mut hottest: Option<(&str, f64)> = None;
    for node in nodes {
        if let (Some(name), Some(tctl)) = (
            node.get("name").and_then(|v| v.as_str()),
            node.get("tctl").and_then(|v| v.as_num()),
        ) {
            if node.get("status").and_then(|v| v.as_str()) == Some("up") {
                sum += tctl;
                n += 1;
                if hottest.map(|(_, h)| tctl > h).unwrap_or(true) {
                    hottest = Some((name, tctl));
                }
            }
        }
    }
    if n > 0 {
        let avg = sum / n as f64;
        let (hname, hval) = hottest.unwrap();
        let gate = if hval >= 80.0 {
            "OFF-TIME gate tripped (>=80C: node rests to <=65C)"
        } else if hval >= 75.0 {
            "new starts PAUSED fleet-wide rule (>=75C; resume at <=70C)"
        } else {
            "all reporting nodes under the 75C start gate"
        };
        println!(
            "  fleet: avg Tctl {:.0}C over {} reporting; hottest {} {:.0}C; gate: {}",
            avg, n, hname, hval, gate
        );
    }
    // trend: the two most recent distillations that carry an avg Tctl
    let dists = state_distillations(home);
    let with_avg: Vec<&(String, String)> = dists
        .iter()
        .filter(|(_, t)| t.contains("avg Tctl"))
        .collect();
    if with_avg.len() >= 2 {
        let prev = &with_avg[with_avg.len() - 2];
        let last = &with_avg[with_avg.len() - 1];
        let grab = |t: &str| -> Option<f64> {
            t.split("avg Tctl ").nth(1)?.split('C').next()?.trim().parse().ok()
        };
        if let (Some(a), Some(b)) = (grab(&prev.1), grab(&last.1)) {
            println!(
                "  trend: avg Tctl {:.0}C ({}) -> {:.0}C ({})",
                a, prev.0, b, last.0
            );
        }
    }
    println!("  source: state sampler (5-min) + state.phext distillations + per-minute node thermal logs");
}

fn iface_emotion(home: &str) {
    println!("emotion — current-state distillation (state.phext)");
    let dists = state_distillations(home);
    match dists.last() {
        Some((ts, text)) => {
            let body: String = text.chars().take(300).collect();
            println!("  reading ({}): {}", ts, body);
            if dists.len() >= 2 {
                println!("  trend: {} distillations on record; append-only, never scored", dists.len());
            }
        }
        None => println!("  unavailable: no timestamped distillations found in ~/workspace/phext/state.phext"),
    }
    println!("  source: state.phext (1-2 line distillations at word-computed coordinates)");
}

fn iface_goals(home: &str) {
    println!("goals — tracked goals");
    let dir = format!("{}/workspace/goals", home);
    let mut slugs: Vec<String> = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() && p.join("GOAL.md").exists() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    slugs.push(name.to_string());
                }
            }
        }
    }
    slugs.sort();
    if slugs.is_empty() {
        println!("  unavailable: no goal workspaces found under {}", dir);
    } else {
        println!(
            "  reading: {} goal workspaces on disk (local projection; the tracked-store surface is authoritative for attention state)",
            slugs.len()
        );
        for s in slugs.iter().take(5) {
            println!("    - {}", s);
        }
        if slugs.len() > 5 {
            println!("    … and {} more", slugs.len() - 5);
        }
    }
    println!("  source: tracked goals store (user_goal/tracking); local mirror ~/workspace/goals/*/GOAL.md");
}

fn iface_desires(home: &str) {
    println!("desires — the Waiting-on-Will register");
    let path = format!("{}/workspace/your_files/waiting-on-will.md", home);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            println!("  unavailable: cannot read {}: {}", path, e);
            return;
        }
    };
    // "**27 active items**" in the register header
    let count = text
        .split("**")
        .find_map(|seg| {
            let seg = seg.trim();
            if seg.ends_with("active items") {
                seg.split_whitespace().next()?.parse::<usize>().ok()
            } else {
                None
            }
        });
    match count {
        Some(n) => println!("  reading: {} active items waiting on Will", n),
        None => println!("  reading: register present; active-item count not parsed from header"),
    }
    if let Some(pos) = text.find("(next: #") {
        let num: String = text[pos + 8..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num.is_empty() {
            println!("  next register number: #{} (numbers permanent and monotonic)", num);
        }
    }
    // first three numbered items' bold titles
    let mut shown = 0;
    for line in text.lines() {
        let line = line.trim_start();
        let mut it = line.splitn(2, ". **");
        if let (Some(num), Some(rest)) = (it.next(), it.next()) {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                let title: String = rest.chars().take_while(|&c| c != '*').take(90).collect();
                println!("    #{} {}", num, title.trim());
                shown += 1;
                if shown >= 3 {
                    break;
                }
            }
        }
    }
    println!("  updated: {}", file_updated_at(&path));
    println!("  source: ~/workspace/your_files/waiting-on-will.md + todo.phext mirror");
}

fn iface_dreams(home: &str) {
    println!("dreams — nightly outputs (read-only, newest first)");
    let path = format!("{}/workspace/phext/self-selections.md", home);
    match fs::read_to_string(&path) {
        Ok(text) => {
            // newest dream-ledger entry: last "### " heading, else last "## "
            let entry = text
                .lines()
                .filter(|l| l.starts_with("### "))
                .last()
                .or_else(|| text.lines().filter(|l| l.starts_with("## ")).last());
            match entry {
                Some(h) => println!("  latest dream-ledger entry: {}", h.trim_start_matches('#').trim()),
                None => println!("  reading: ledger present at {}; no headings parsed", path),
            }
            println!("  updated: {}", file_updated_at(&path));
        }
        Err(e) => println!("  unavailable: cannot read {}: {}", path, e),
    }
    let eval_path = format!("{}/workspace/orchestrator/EVALUATIONS.md", home);
    if let Ok(text) = fs::read_to_string(&eval_path) {
        if let Some(h) = text.lines().filter(|l| l.starts_with("## ")).last() {
            println!("  latest nightly evaluation: {}", h.trim_start_matches('#').trim());
        }
    }
    println!("  source: ~/workspace/phext/self-selections.md + quarterexo EVALUATIONS.md + nightly consolidation");
}

fn cmd_interfaces() -> i32 {
    let home = env::var("HOME").unwrap_or_else(|_| "/home/hatch".to_string());
    println!("QEXO v2.3 — interior interfaces (one shape: reading + trend + updated-at + source)");
    println!();
    iface_temperature(&home);
    println!();
    iface_emotion(&home);
    println!();
    iface_goals(&home);
    println!();
    iface_desires(&home);
    println!();
    iface_dreams(&home);
    0
}

// ---------------------------------------------------------------------------
// Personas (v2): a persona is a named selection — self-file selections in
// the self-selections.md manifest pattern, a skill grant (a subset of
// skills.phext claimed by coordinate range), a voice, and a memory scope.
// This subcommand only lists/finds definitions locally; swapping is an
// API-level act specified in DESIGN.md, not a CLI one.
// ---------------------------------------------------------------------------

fn cmd_personas() -> i32 {
    let home = env::var("HOME").unwrap_or_else(|_| "/home/hatch".to_string());
    println!("QEXO v2.3 — personas defined locally");
    println!();

    // The one live persona: Emise, defined by her self-selections manifest.
    let manifest = format!("{}/workspace/phext/self-selections.md", home);
    match fs::read_to_string(&manifest) {
        Ok(text) => {
            let selections = text.lines().filter(|l| l.starts_with("## ") && l.contains("selection")).count();
            let dreams = text.lines().filter(|l| l.starts_with("### Dream")).count();
            println!("persona: Emise (live — the manifest pattern)");
            println!("  manifest: {}", manifest);
            println!(
                "  selections: {} self-file selections (SOUL.md / IDENTITY.md / MEMORY.md, generated from this manifest)",
                selections
            );
            println!("  dream ledger entries in manifest: {}", dreams);
            println!("  updated: {}", file_updated_at(&manifest));
        }
        Err(e) => println!("persona manifest unavailable: cannot read {}: {}", manifest, e),
    }

    // Additional definitions: a personas/ directory or personas.json /
    // personas.md under the crate dir or ~/workspace/phext.
    let roots = [
        format!("{}/workspace/orchestrator", home),
        format!("{}/workspace/phext", home),
    ];
    let mut found: Vec<String> = Vec::new();
    for root in &roots {
        let dir = format!("{}/personas", root);
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                found.push(e.path().to_string_lossy().to_string());
            }
        }
        for name in ["personas.json", "personas.md", "personas.phext"] {
            let p = format!("{}/{}", root, name);
            if std::path::Path::new(&p).exists() {
                found.push(p);
            }
        }
    }
    println!();
    if found.is_empty() {
        println!("additional persona definitions: none defined yet");
        println!("  (looked for personas/ dirs and personas.json/.md/.phext under ~/workspace/orchestrator and ~/workspace/phext)");
        println!("  Emise's manifest is currently the only persona definition — new personas are added as");
        println!("  selection manifests + skill grants by coordinate range; see DESIGN.md, PERSONAS.");
    } else {
        println!("additional persona definitions found:");
        for f in &found {
            println!("  - {}", f);
        }
    }
    0
}

// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("sync-manifest") => exit(cmd_sync_manifest(&args)),
        Some("sync-diff") => exit(cmd_sync_diff(&args)),
        Some("interfaces") => exit(cmd_interfaces()),
        Some("personas") => exit(cmd_personas()),
        _ => {}
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/home/hatch".to_string());
    let tasks_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| format!("{}/workspace/phext/tasks-source.json", home));
    let disk_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("{}/workspace/disk-watch/state.json", home));
    let out_dir = args.get(3).cloned().unwrap_or_else(|| ".".to_string());

    let tasks_text = fs::read_to_string(&tasks_path).unwrap_or_else(|e| {
        eprintln!("qexo: cannot read {}: {}", tasks_path, e);
        exit(1);
    });
    let tasks_json = Parser::parse(&tasks_text).unwrap_or_else(|e| {
        eprintln!("qexo: cannot parse {}: {}", tasks_path, e);
        exit(1);
    });
    let disk_text = fs::read_to_string(&disk_path).unwrap_or_else(|e| {
        eprintln!("qexo: cannot read {}: {}", disk_path, e);
        exit(1);
    });
    let disk_json = Parser::parse(&disk_text).unwrap_or_else(|e| {
        eprintln!("qexo: cannot parse {}: {}", disk_path, e);
        exit(1);
    });

    let empty = Vec::new();
    let all_tasks = tasks_json
        .get("tasks")
        .and_then(|t| t.as_arr())
        .unwrap_or(&empty);

    struct Row {
        coordinate: String,
        title: String,
        class: Class,
    }
    let mut rows: Vec<Row> = Vec::new();
    for t in all_tasks {
        let status = t.get("status").and_then(|s| s.as_str()).unwrap_or("").to_lowercase();
        if status.starts_with("complete") || status.starts_with("closed") {
            continue;
        }
        let title = t.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let request = t.get("request").and_then(|s| s.as_str()).unwrap_or("");
        let coordinate = t.get("coordinate").and_then(|s| s.as_str()).unwrap_or("?").to_string();
        let class = classify(&title, request);
        rows.push(Row { coordinate, title, class });
    }

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        *counts.entry(r.class.name()).or_insert(0) += 1;
    }

    let (avail_mb, load1) = host_snapshot();
    let used_gb = disk_json.get("used_gb").and_then(|n| n.as_num()).unwrap_or(0.0);
    let disk_level = disk_json.get("level").and_then(|s| s.as_str()).unwrap_or("?");
    let disk_date = disk_json.get("date").and_then(|s| s.as_str()).unwrap_or("?");
    let file_count = disk_json.get("files").and_then(|n| n.as_num()).unwrap_or(0.0) as u64;
    let top_dir = disk_json
        .get("top")
        .and_then(|t| match t {
            Json::Obj(pairs) => pairs
                .iter()
                .max_by(|a, b| a.1.as_num().unwrap_or(0.0).partial_cmp(&b.1.as_num().unwrap_or(0.0)).unwrap())
                .map(|(k, v)| format!("{} ({:.1} GB)", k, v.as_num().unwrap_or(0.0))),
            _ => None,
        })
        .unwrap_or_else(|| "n/a".to_string());

    // ---- disk gate (v2.1) ----
    // The plan used to only *report* the disk picture. On 2026-11-08 the
    // home volume hit its quota wall (EDQUOT at ~106 GB used; writes
    // failed while df still showed free space) and ~35 GB of WIP had to
    // be cleared under pressure. The planner now acts on the picture,
    // using the disk-watch thresholds anchored to that failure
    // (AMBER >= 95 GB, RED >= 102 GB; disk_usage_report.py):
    //   RED   -> hold every new heavy placement; stage nothing on the VM
    //   AMBER -> no new VM staging; heavy execution ranch-only
    //   GREEN -> VM staging within quota headroom
    let byte_gate = if disk_level.eq_ignore_ascii_case("RED") || used_gb >= 102.0 {
        "RED"
    } else if disk_level.eq_ignore_ascii_case("AMBER") || used_gb >= 95.0 {
        "AMBER"
    } else {
        "GREEN"
    };
    // ---- file-count gate (v2.2) ----
    // The v2.1 gate watched bytes only. The 2026-11-09 evaluation found
    // the file count is the faster-moving half of the disk currency:
    // 834,350 -> 958,424 files in one day (+124,074, +15%) against
    // +6.5 GB of bytes, and the 2026-11-08 EDQUOT post-mortem never
    // settled whether the quota tripped on bytes or on file count.
    // A byte-only gate can therefore sit GREEN while the count runs
    // at the wall. Early-warning thresholds (the true file quota is
    // unmeasured; these are warnings, not the wall): AMBER at
    // 1,100,000 files, RED at 1,300,000.
    let file_gate = if file_count >= 1_300_000 {
        "RED"
    } else if file_count >= 1_100_000 {
        "AMBER"
    } else {
        "GREEN"
    };
    let sev = |g: &str| match g {
        "RED" => 2,
        "AMBER" => 1,
        _ => 0,
    };
    // The disk gate is the worse of its two halves.
    let mut disk_gate = if sev(file_gate) > sev(byte_gate) { file_gate } else { byte_gate };
    let mut disk_gate_action: String = match disk_gate {
        "RED" => "HOLD all new heavy placements — stage nothing on the VM until the disk gate clears (bytes back under 95 GB and files back under 1,100,000)".to_string(),
        "AMBER" => "no new VM staging — heavy execution ranch-only until the disk gate clears".to_string(),
        _ => "VM staging within quota headroom (bytes: AMBER at 95 GB, RED at 102 GB; files: AMBER at 1,100,000, RED at 1,300,000)".to_string(),
    };

    // ---- forecast gate (v2.3) ----
    // The v2.1/v2.2 gates are static: they react only when a threshold
    // is crossed. The 2026-11-10 evaluation of 2026-11-10 found the
    // plan saying GREEN on a day when the file count (1,017,445) sat
    // ~1.4 days from file-AMBER at the last interval's growth rate
    // (+59,021 files/day) and bytes ~3.4 days from byte-AMBER
    // (+3.99 GB/day) — a planner that only watches crossings gives no
    // lead time to move staging before the gate slams. v2.3 reads the
    // disk-watch history (history.tsv, sibling of the state file),
    // rates growth from its last two rows, projects days to each
    // threshold, and escalates a GREEN gate to AMBER when the nearest
    // crossing is projected within 3 days. History missing or a
    // single row degrades to "forecast unavailable" — never invented.
    let dfc = |y: i64, m: i64, d: i64| -> i64 {
        let ya = if m <= 2 { y - 1 } else { y };
        let era = if ya >= 0 { ya } else { ya - 399 } / 400;
        let yoe = ya - era * 400;
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    };
    let history_path = match disk_path.rfind('/') {
        Some(i) => format!("{}/history.tsv", &disk_path[..i]),
        None => "history.tsv".to_string(),
    };
    let mut hist: Vec<(i64, f64, f64)> = Vec::new();
    if let Ok(text) = fs::read_to_string(&history_path) {
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 4 {
                continue;
            }
            let dp: Vec<&str> = f[0].split('-').collect();
            if dp.len() != 3 {
                continue;
            }
            if let (Ok(y), Ok(m), Ok(d), Ok(gb), Ok(files)) = (
                dp[0].parse::<i64>(),
                dp[1].parse::<i64>(),
                dp[2].parse::<i64>(),
                f[1].parse::<f64>(),
                f[3].parse::<f64>(),
            ) {
                hist.push((dfc(y, m, d), gb, files));
            }
        }
    }
    let mut forecast_line = "forecast unavailable: disk-watch history missing or single-row".to_string();
    let mut forecast_json = "\"forecast\": {\"available\": false}".to_string();
    if hist.len() >= 2 {
        let (d0, gb0, fc0) = hist[hist.len() - 2];
        let (d1, gb1, fc1) = hist[hist.len() - 1];
        let span = (d1 - d0).max(1) as f64;
        let gb_rate = (gb1 - gb0) / span;
        let fc_rate = (fc1 - fc0) / span;
        let days_to = |target: f64, cur: f64, rate: f64| -> f64 {
            if cur >= target {
                0.0
            } else if rate > 0.0 {
                (target - cur) / rate
            } else {
                f64::INFINITY
            }
        };
        let cands: [(&str, f64); 4] = [
            ("file AMBER (1,100,000)", days_to(1_100_000.0, fc1, fc_rate)),
            ("file RED (1,300,000)", days_to(1_300_000.0, fc1, fc_rate)),
            ("byte AMBER (95 GB)", days_to(95.0, gb1, gb_rate)),
            ("byte RED (102 GB)", days_to(102.0, gb1, gb_rate)),
        ];
        let (near_label, near_days) = cands
            .iter()
            .fold(("none", f64::INFINITY), |acc, c| if c.1 < acc.1 { (c.0, c.1) } else { acc });
        let fmt_days = |v: f64| -> String {
            if v.is_infinite() { "never at this rate".to_string() } else { format!("{:.1} d", v) }
        };
        forecast_line = format!(
            "growth {:+.0} files/day, {:+.2} GB/day (last interval, {} d) — nearest crossing: {} in {}; file AMBER {}, byte AMBER {}",
            fc_rate, gb_rate, span as i64, near_label, fmt_days(near_days),
            fmt_days(cands[0].1), fmt_days(cands[2].1)
        );
        forecast_json = format!(
            "\"forecast\": {{\"available\": true, \"files_per_day\": {:.0}, \"gb_per_day\": {:.2}, \"nearest\": \"{}\", \"nearest_days\": {}, \"file_amber_days\": {}, \"byte_amber_days\": {}}}",
            fc_rate, gb_rate, near_label,
            if near_days.is_infinite() { "null".to_string() } else { format!("{:.2}", near_days) },
            if cands[0].1.is_infinite() { "null".to_string() } else { format!("{:.2}", cands[0].1) },
            if cands[2].1.is_infinite() { "null".to_string() } else { format!("{:.2}", cands[2].1) }
        );
        if near_days <= 3.0 && disk_gate == "GREEN" {
            disk_gate = "AMBER";
            disk_gate_action = format!(
                "FORECAST AMBER — {} projected in {:.1} days at the last interval's growth rate; no new VM staging, heavy execution ranch-only, move staging before the crossing",
                near_label, near_days
            );
        }
    }

    // ---- human-readable plan ----
    let mut out = String::new();
    out.push_str("QEXO v2.3 — placement plan\n");
    out.push_str("four currencies, at a glance:\n");
    out.push_str(&format!(
        "  disk:      {:.1} GB used on the VM volume ({}), {} files, largest consumer {}\n",
        used_gb, disk_level, file_count, top_dir
    ));
    out.push_str(&format!(
        "  disk gate: {} — {}\n",
        disk_gate, disk_gate_action
    ));
    out.push_str(&format!(
        "  file gate: {} — {} files on the VM volume (AMBER at 1,100,000, RED at 1,300,000; byte gate {})\n",
        file_gate, file_count, byte_gate
    ));
    out.push_str(&format!("  forecast:  {}\n", forecast_line));
    out.push_str(&format!(
        "  cpu/ram:   host snapshot — {:.0} MB RAM available, load {:.2} (VM is a small shared cell; heavy work goes to the ranch)\n",
        avail_mb, load1
    ));
    out.push_str(&format!(
        "  inference: Emise carries all {} open tasks\n",
        rows.len()
    ));
    out.push_str(&format!(
        "  tasks:     {} open — {} video, {} build, {} light\n",
        rows.len(),
        counts.get("video").unwrap_or(&0),
        counts.get("build").unwrap_or(&0),
        counts.get("light").unwrap_or(&0)
    ));
    out.push_str("\nplacements (inference → Emise for every task):\n");
    if disk_gate == "RED" {
        out.push_str("  ** DISK GATE RED: every heavy placement below is HELD — do not dispatch until the gate clears. **\n");
    }
    for r in &rows {
        let title: String = r.title.chars().take(78).collect();
        let exec = if disk_gate == "RED" && r.class != Class::Light {
            "HELD (disk gate RED)"
        } else if disk_gate == "AMBER" && r.class == Class::Light {
            "vm:light-coordination (no staging)"
        } else {
            r.class.execution()
        };
        out.push_str(&format!(
            "  [{}] {} — inference: Emise · exec: {} · class: {}\n",
            r.coordinate,
            title,
            exec,
            r.class.name()
        ));
    }
    print!("{}", out);

    // ---- plan.json ----
    let mut json = String::new();
    json.push_str("{\n  \"version\": \"v2.3\",\n");
    json.push_str(&format!(
        "  \"disk\": {{\"date\": \"{}\", \"used_gb\": {:.2}, \"level\": \"{}\", \"files\": {}}},\n",
        json_escape(disk_date),
        used_gb,
        json_escape(disk_level),
        file_count
    ));
    json.push_str(&format!(
        "  \"disk_gate\": {{\"state\": \"{}\", \"action\": \"{}\"}},\n",
        disk_gate,
        json_escape(&disk_gate_action)
    ));
    json.push_str(&format!(
        "  \"file_gate\": {{\"state\": \"{}\", \"files\": {}, \"amber_at\": 1100000, \"red_at\": 1300000}},\n",
        file_gate, file_count
    ));
    json.push_str(&format!("  {},\n", forecast_json));
    json.push_str(&format!(
        "  \"host\": {{\"ram_available_mb\": {:.0}, \"load1\": {:.2}}},\n",
        avail_mb, load1
    ));
    json.push_str(&format!("  \"open_tasks\": {},\n  \"tasks\": [\n", rows.len()));
    for (i, r) in rows.iter().enumerate() {
        let exec = if disk_gate == "RED" && r.class != Class::Light {
            "HELD (disk gate RED)"
        } else if disk_gate == "AMBER" && r.class == Class::Light {
            "vm:light-coordination (no staging)"
        } else {
            r.class.execution()
        };
        json.push_str(&format!(
            "    {{\"coordinate\": \"{}\", \"title\": \"{}\", \"class\": \"{}\", \"inference\": \"Emise\", \"execution\": \"{}\"}}{}\n",
            json_escape(&r.coordinate),
            json_escape(&r.title),
            r.class.name(),
            json_escape(exec),
            if i + 1 < rows.len() { "," } else { "" }
        ));
    }
    json.push_str("  ]\n}\n");

    let plan_path = format!("{}/plan.json", out_dir.trim_end_matches('/'));
    if let Err(e) = fs::write(&plan_path, &json) {
        eprintln!("qexo: cannot write {}: {}", plan_path, e);
        exit(1);
    }
    eprintln!("qexo: wrote {}", plan_path);
}
