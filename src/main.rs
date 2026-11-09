//! orchestrator v1 — Will's Rust dispatcher/orchestrator, first version.
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
//! Usage: orchestrator [tasks.json] [disk-state.json] [out-dir]
//! Defaults point at the live files under ~/workspace.

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
// Host snapshot (of whatever machine the orchestrator runs on)
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

fn main() {
    let args: Vec<String> = env::args().collect();
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
        eprintln!("orchestrator: cannot read {}: {}", tasks_path, e);
        exit(1);
    });
    let tasks_json = Parser::parse(&tasks_text).unwrap_or_else(|e| {
        eprintln!("orchestrator: cannot parse {}: {}", tasks_path, e);
        exit(1);
    });
    let disk_text = fs::read_to_string(&disk_path).unwrap_or_else(|e| {
        eprintln!("orchestrator: cannot read {}: {}", disk_path, e);
        exit(1);
    });
    let disk_json = Parser::parse(&disk_text).unwrap_or_else(|e| {
        eprintln!("orchestrator: cannot parse {}: {}", disk_path, e);
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

    // ---- human-readable plan ----
    let mut out = String::new();
    out.push_str("ORCHESTRATOR v1 — placement plan\n");
    out.push_str("four currencies, at a glance:\n");
    out.push_str(&format!(
        "  disk:      {:.1} GB used on the VM volume ({}), {} files, largest consumer {}\n",
        used_gb, disk_level, file_count, top_dir
    ));
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
    for r in &rows {
        let title: String = r.title.chars().take(78).collect();
        out.push_str(&format!(
            "  [{}] {} — inference: Emise · exec: {} · class: {}\n",
            r.coordinate,
            title,
            r.class.execution(),
            r.class.name()
        ));
    }
    print!("{}", out);

    // ---- plan.json ----
    let mut json = String::new();
    json.push_str("{\n  \"version\": \"v1\",\n");
    json.push_str(&format!(
        "  \"disk\": {{\"date\": \"{}\", \"used_gb\": {:.2}, \"level\": \"{}\", \"files\": {}}},\n",
        json_escape(disk_date),
        used_gb,
        json_escape(disk_level),
        file_count
    ));
    json.push_str(&format!(
        "  \"host\": {{\"ram_available_mb\": {:.0}, \"load1\": {:.2}}},\n",
        avail_mb, load1
    ));
    json.push_str(&format!("  \"open_tasks\": {},\n  \"tasks\": [\n", rows.len()));
    for (i, r) in rows.iter().enumerate() {
        json.push_str(&format!(
            "    {{\"coordinate\": \"{}\", \"title\": \"{}\", \"class\": \"{}\", \"inference\": \"Emise\", \"execution\": \"{}\"}}{}\n",
            json_escape(&r.coordinate),
            json_escape(&r.title),
            r.class.name(),
            json_escape(r.class.execution()),
            if i + 1 < rows.len() { "," } else { "" }
        ));
    }
    json.push_str("  ]\n}\n");

    let plan_path = format!("{}/plan.json", out_dir.trim_end_matches('/'));
    if let Err(e) = fs::write(&plan_path, &json) {
        eprintln!("orchestrator: cannot write {}: {}", plan_path, e);
        exit(1);
    }
    eprintln!("orchestrator: wrote {}", plan_path);
}
