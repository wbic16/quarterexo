//! The dispatcher core (v1/v2 carried forward): plan emission in the
//! four currencies. The plan is a proposal surface — emitting it
//! mutates nothing (API-V1 section 6.1).

use crate::json::Json;
use std::fs;

const VIDEO_TOKENS: &[&str] = &[
    "video", "film", "ffmpeg", "encode", "encoding", "render", "rendering",
    "wavefront", "frame", "frames", "mp4", "mkv", "v14", "v15", "v16", "transcode",
];
const BUILD_TOKENS: &[&str] = &[
    "build", "builds", "compile", "compiler", "cargo", "test", "tests", "testing",
    "benchmark", "bulk", "suite", "phcc",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Class {
    Video,
    Build,
    Light,
}

impl Class {
    pub fn name(&self) -> &'static str {
        match self {
            Class::Video => "video",
            Class::Build => "build",
            Class::Light => "light",
        }
    }
    pub fn execution(&self) -> &'static str {
        match self {
            Class::Video => "ranch:aletheia-core (video — never on VM)",
            Class::Build => "ranch:aletheia-core",
            Class::Light => "vm:light-coordination",
        }
    }
}

pub fn classify(title: &str, request: &str) -> Class {
    // Cheap prefilter (measured win, plan hot path): scan the raw
    // combined text for any token-word as a substring first. Every
    // token in both sets is >= 4 chars and appears in classified text
    // far less often than not; only on a substring hit do we pay for
    // tokenization + lowercase allocation. Substring hits that are
    // not whole tokens fall through to the exact token pass, so
    // classification semantics are unchanged.
    let mut combined = String::with_capacity(title.len() + request.len() + 1);
    combined.push_str(title);
    combined.push(' ');
    combined.push_str(request);
    let lower = combined.to_lowercase();
    let any_hit = VIDEO_TOKENS.iter().chain(BUILD_TOKENS.iter()).any(|t| lower.contains(t));
    if !any_hit {
        return Class::Light;
    }
    let tokens: Vec<&str> = lower
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

/// (MemAvailable MB, loadavg 1min) of the host qexo runs on.
pub fn host_snapshot() -> (f64, f64) {
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

pub fn default_tasks_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    format!("{}/workspace/phext/tasks-source.json", home)
}

pub fn default_disk_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    format!("{}/workspace/disk-watch/state.json", home)
}

/// Build the plan payload (the dispatcher rows + currency summary).
/// Without disk-watch history the forecast gate degrades to
/// "forecast unavailable" — never invented (v2.3 rule).
pub fn build_plan(tasks_text: &str, disk_text: &str) -> Result<Json, String> {
    build_plan_with_history(tasks_text, disk_text, None)
}

/// As `build_plan`, with the disk-watch history (history.tsv, sibling
/// of the disk state file) supplied for the forecast gate.
pub fn build_plan_with_history(tasks_text: &str, disk_text: &str, history_text: Option<&str>) -> Result<Json, String> {
    let tasks_json = crate::json::parse(tasks_text).map_err(|e| format!("tasks parse: {}", e))?;
    let disk_json = crate::json::parse(disk_text).map_err(|e| format!("disk parse: {}", e))?;
    let empty = Vec::new();
    let all_tasks = tasks_json.get("tasks").and_then(|t| t.as_arr()).unwrap_or(&empty);

    let mut raw_rows: Vec<(String, String, Class)> = Vec::new();
    let (mut nv, mut nb, mut nl) = (0usize, 0usize, 0usize);
    for t in all_tasks {
        let status = t.get("status").and_then(|s| s.as_str()).unwrap_or("").to_lowercase();
        if status.starts_with("complete") || status.starts_with("closed") {
            continue;
        }
        let title = t.get("title").and_then(|s| s.as_str()).unwrap_or("");
        let request = t.get("request").and_then(|s| s.as_str()).unwrap_or("");
        let coordinate = t.get("coordinate").and_then(|s| s.as_str()).unwrap_or("?");
        let class = classify(title, request);
        match class {
            Class::Video => nv += 1,
            Class::Build => nb += 1,
            Class::Light => nl += 1,
        }
        raw_rows.push((coordinate.to_string(), title.to_string(), class));
    }

    let (avail_mb, load1) = host_snapshot();
    let used_gb = disk_json.get("used_gb").and_then(|n| n.as_num()).unwrap_or(0.0);
    let disk_level = disk_json.get("level").and_then(|s| s.as_str()).unwrap_or("?");
    let disk_date = disk_json.get("date").and_then(|s| s.as_str()).unwrap_or("?");
    let file_count = disk_json.get("files").and_then(|n| n.as_num()).unwrap_or(0.0) as u64;

    // ---- disk gate (v2.1 byte gate, carried into v3 by the exo merge;
    // v2.2 file-count gate and v2.3 forecast gate, ported into v3 in
    // the v3.1 rewire) ----
    // The plan used to only *report* the disk picture. On 2026-11-08 the
    // home volume hit its quota wall (EDQUOT at ~106 GB used; writes
    // failed while df still showed free space) and ~35 GB of WIP had to
    // be cleared under pressure. The planner acts on the picture, using
    // the disk-watch thresholds anchored to that failure
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
    // is crossed. The 2026-11-10 evaluation found the plan saying
    // GREEN on a day when the file count (1,017,445) sat ~1.4 days
    // from file-AMBER at the last interval's growth rate (+59,021
    // files/day) and bytes ~3.4 days from byte-AMBER (+3.99 GB/day) —
    // a planner that only watches crossings gives no lead time to
    // move staging before the gate slams. The forecast rates growth
    // from the disk-watch history's last two rows, projects days to
    // each threshold, and escalates a GREEN gate to AMBER when the
    // nearest crossing is projected within 3 days. History missing
    // or a single row degrades to "forecast unavailable" — never
    // invented.
    let forecast = forecast_gate(history_text);
    if let Some((near_label, near_days)) = forecast.nearest {
        if near_days <= 3.0 && disk_gate == "GREEN" {
            disk_gate = "AMBER";
            disk_gate_action = format!(
                "FORECAST AMBER — {} projected in {:.1} days at the last interval's growth rate; no new VM staging, heavy execution ranch-only, move staging before the crossing",
                near_label, near_days
            );
        }
    }
    let forecast_json = forecast.json;

    let rows: Vec<Json> = raw_rows
        .iter()
        .map(|(coordinate, title, class)| {
            let execution = if disk_gate == "RED" && *class != Class::Light {
                "HELD (disk gate RED)"
            } else if disk_gate == "AMBER" && *class == Class::Light {
                "vm:light-coordination (no staging)"
            } else {
                class.execution()
            };
            Json::obj(vec![
                ("coordinate", Json::s(coordinate)),
                ("title", Json::s(title)),
                ("class", Json::s(class.name())),
                ("inference", Json::s("Emise")),
                ("execution", Json::s(&execution)),
            ])
        })
        .collect();

    let total = rows.len();
    Ok(Json::obj(vec![
        ("plan_version", Json::s("v3.2")),
        ("disk", Json::obj(vec![
            ("date", Json::s(disk_date)),
            ("used_gb", Json::n(used_gb)),
            ("level", Json::s(disk_level)),
            ("files", Json::n(file_count as f64)),
        ])),
        ("disk_gate", Json::obj(vec![
            ("state", Json::s(disk_gate)),
            ("action", Json::s(disk_gate_action)),
        ])),
        ("file_gate", Json::obj(vec![
            ("state", Json::s(file_gate)),
            ("files", Json::n(file_count as f64)),
            ("amber_at", Json::n(1_100_000.0)),
            ("red_at", Json::n(1_300_000.0)),
        ])),
        ("forecast", forecast_json),
        ("host", Json::obj(vec![
            ("ram_available_mb", Json::n(avail_mb)),
            ("load1", Json::n(load1)),
        ])),
        ("currencies", Json::obj(vec![
            ("disk", Json::s("VM volume state above; ranch carries bulk")),
            ("cpu_ram", Json::s("host snapshot above; heavy work to the ranch")),
            ("inference", Json::s("Emise carries all open tasks")),
        ])),
        ("open_tasks", Json::n(total as f64)),
        ("counts", Json::obj(vec![
            ("video", Json::n(nv as f64)),
            ("build", Json::n(nb as f64)),
            ("light", Json::n(nl as f64)),
        ])),
        ("tasks", Json::Arr(rows)),
    ]))
}

/// Days from civil date (Howard Hinnant's algorithm) — the v2.3
/// standalone's date arithmetic, carried verbatim in behaviour.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let ya = if m <= 2 { y - 1 } else { y };
    let era = if ya >= 0 { ya } else { ya - 399 } / 400;
    let yoe = ya - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parse disk-watch history rows: (epoch-day, used GB, file count)
/// from tab-separated lines `date \t gb \t ... \t files`.
fn parse_history(text: &str) -> Vec<(i64, f64, f64)> {
    let mut hist: Vec<(i64, f64, f64)> = Vec::new();
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
            hist.push((days_from_civil(y, m, d), gb, files));
        }
    }
    hist
}

pub struct Forecast {
    pub json: Json,
    /// (crossing label, days) when a projection exists.
    pub nearest: Option<(String, f64)>,
}

/// The v2.3 forecast computation, in v3 (Json) form. With fewer
/// than two usable history rows the forecast is unavailable and
/// `nearest` is None — the reading is never invented.
pub fn forecast_gate(history_text: Option<&str>) -> Forecast {
    let unavailable = || Forecast {
        json: Json::obj(vec![("available", Json::Bool(false))]),
        nearest: None,
    };
    let hist = match history_text {
        Some(t) => parse_history(t),
        None => return unavailable(),
    };
    if hist.len() < 2 {
        return unavailable();
    }
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
    let num_or_null = |v: f64| -> Json {
        if v.is_infinite() { Json::Null } else { Json::n((v * 100.0).round() / 100.0) }
    };
    Forecast {
        json: Json::obj(vec![
            ("available", Json::Bool(true)),
            ("files_per_day", Json::n(fc_rate.round())),
            ("gb_per_day", Json::n((gb_rate * 100.0).round() / 100.0)),
            ("span_days", Json::n(span)),
            ("nearest", Json::s(near_label)),
            ("nearest_days", num_or_null(near_days)),
            ("file_amber_days", num_or_null(cands[0].1)),
            ("byte_amber_days", num_or_null(cands[2].1)),
        ]),
        nearest: if near_days.is_infinite() { None } else { Some((near_label.to_string(), near_days)) },
    }
}

/// The standalone forecast line, rendered from a plan payload.
fn forecast_line(plan: &Json) -> String {
    let f = match plan.get("forecast") {
        Some(f) => f,
        None => return "forecast unavailable: disk-watch history missing or single-row".to_string(),
    };
    if f.get("available").and_then(|b| b.as_bool()) != Some(true) {
        return "forecast unavailable: disk-watch history missing or single-row".to_string();
    }
    let fmt_days = |v: Option<f64>| -> String {
        match v {
            Some(d) => format!("{:.1} d", d),
            None => "never at this rate".to_string(),
        }
    };
    let days = |k: &str| f.get(k).and_then(|n| n.as_num());
    format!(
        "growth {:+.0} files/day, {:+.2} GB/day (last interval, {} d) — nearest crossing: {} in {}; file AMBER {}, byte AMBER {}",
        f.get("files_per_day").and_then(|n| n.as_num()).unwrap_or(0.0),
        f.get("gb_per_day").and_then(|n| n.as_num()).unwrap_or(0.0),
        f.get("span_days").and_then(|n| n.as_num()).unwrap_or(0.0) as i64,
        f.get("nearest").and_then(|s| s.as_str()).unwrap_or("none"),
        fmt_days(days("nearest_days")),
        fmt_days(days("file_amber_days")),
        fmt_days(days("byte_amber_days"))
    )
}

/// Human-readable rendering of a plan payload (the v1 shape, kept).
pub fn plan_human(plan: &Json) -> String {
    let mut out = String::new();
    out.push_str("qexo — placement plan\n");
    out.push_str("four currencies, at a glance:\n");
    if let Some(d) = plan.get("disk") {
        out.push_str(&format!(
            "  disk:      {} GB used ({}), {} files\n",
            d.get("used_gb").and_then(|n| n.as_num()).unwrap_or(0.0),
            d.get("level").and_then(|s| s.as_str()).unwrap_or("?"),
            d.get("files").and_then(|n| n.as_num()).unwrap_or(0.0) as u64
        ));
    }
    if let Some(g) = plan.get("disk_gate") {
        out.push_str(&format!(
            "  disk gate: {} — {}\n",
            g.get("state").and_then(|s| s.as_str()).unwrap_or("?"),
            g.get("action").and_then(|s| s.as_str()).unwrap_or("")
        ));
    }
    if let Some(fg) = plan.get("file_gate") {
        out.push_str(&format!(
            "  file gate: {} — {} files on the VM volume (AMBER at 1,100,000, RED at 1,300,000)\n",
            fg.get("state").and_then(|s| s.as_str()).unwrap_or("?"),
            fg.get("files").and_then(|n| n.as_num()).unwrap_or(0.0) as u64
        ));
    }
    out.push_str(&format!("  forecast:  {}\n", forecast_line(plan)));
    if let Some(h) = plan.get("host") {
        out.push_str(&format!(
            "  cpu/ram:   host snapshot — {:.0} MB RAM available, load {:.2}\n",
            h.get("ram_available_mb").and_then(|n| n.as_num()).unwrap_or(0.0),
            h.get("load1").and_then(|n| n.as_num()).unwrap_or(0.0)
        ));
    }
    let total = plan.get("open_tasks").and_then(|n| n.as_num()).unwrap_or(0.0) as usize;
    out.push_str(&format!("  inference: Emise carries all {} open tasks\n", total));
    out.push_str("\nplacements (inference → Emise for every task):\n");
    if plan.get("disk_gate").and_then(|g| g.get("state")).and_then(|s| s.as_str()) == Some("RED") {
        out.push_str("  ** DISK GATE RED: every heavy placement below is HELD — do not dispatch until the gate clears. **\n");
    }
    if let Some(tasks) = plan.get("tasks").and_then(|t| t.as_arr()) {
        for r in tasks {
            let title: String = r.get("title").and_then(|s| s.as_str()).unwrap_or("").chars().take(78).collect();
            out.push_str(&format!(
                "  [{}] {} — inference: Emise · exec: {} · class: {}\n",
                r.get("coordinate").and_then(|s| s.as_str()).unwrap_or("?"),
                title,
                r.get("execution").and_then(|s| s.as_str()).unwrap_or(""),
                r.get("class").and_then(|s| s.as_str()).unwrap_or("")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_precedence() {
        assert_eq!(classify("V16 film render", ""), Class::Video);
        assert_eq!(classify("cargo build the thing", ""), Class::Build);
        assert_eq!(classify("write a note", ""), Class::Light);
    }

    #[test]
    fn plan_counts_open_only() {
        let tasks = r#"{"tasks":[
          {"coordinate":"1.1.1/1.1.1/1.1.1","title":"film encode","status":"open"},
          {"coordinate":"2.2.2/2.2.2/2.2.2","title":"old","status":"complete"},
          {"coordinate":"3.3.3/3.3.3/3.3.3","title":"think","status":"in flight"}
        ]}"#;
        let disk = r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-09","files":834350}"#;
        let plan = build_plan(tasks, disk).unwrap();
        assert_eq!(plan.get("open_tasks").unwrap().as_num(), Some(2.0));
    }

    #[test]
    fn disk_gate_states() {
        let tasks = r#"{"tasks":[{"coordinate":"1.1.1/1.1.1/1.1.1","title":"film encode","status":"open"},{"coordinate":"2.2.2/2.2.2/2.2.2","title":"think","status":"open"}]}"#;
        let gate = |disk: &str| {
            let plan = build_plan(tasks, disk).unwrap();
            let state = plan.get("disk_gate").unwrap().get("state").unwrap().as_str().unwrap().to_string();
            let rows = plan.get("tasks").unwrap().as_arr().unwrap();
            let execs: Vec<String> = rows.iter().map(|r| r.get("execution").unwrap().as_str().unwrap().to_string()).collect();
            (state, execs)
        };
        // GREEN by level and size: normal placements.
        let (s, e) = gate(r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-09","files":1}"#);
        assert_eq!(s, "GREEN");
        assert_eq!(e[0], "ranch:aletheia-core (video — never on VM)");
        assert_eq!(e[1], "vm:light-coordination");
        // AMBER by size alone (>= 95 GB): light tasks lose staging.
        let (s, e) = gate(r#"{"used_gb":96.0,"level":"GREEN","date":"2026-11-09","files":1}"#);
        assert_eq!(s, "AMBER");
        assert_eq!(e[1], "vm:light-coordination (no staging)");
        // RED by level: heavy placements held, light untouched.
        let (s, e) = gate(r#"{"used_gb":50.0,"level":"RED","date":"2026-11-09","files":1}"#);
        assert_eq!(s, "RED");
        assert_eq!(e[0], "HELD (disk gate RED)");
        assert_eq!(e[1], "vm:light-coordination");
        // RED by size alone (>= 102 GB).
        let (s, _) = gate(r#"{"used_gb":103.0,"level":"GREEN","date":"2026-11-09","files":1}"#);
        assert_eq!(s, "RED");
        // File gate AMBER alone (v2.2): the disk gate is the worse of
        // its two halves, so bytes GREEN + files AMBER -> AMBER.
        let (s, e) = gate(r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-10","files":1150000}"#);
        assert_eq!(s, "AMBER");
        assert_eq!(e[1], "vm:light-coordination (no staging)");
        // File gate RED alone: heavy placements held.
        let (s, e) = gate(r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-10","files":1310000}"#);
        assert_eq!(s, "RED");
        assert_eq!(e[0], "HELD (disk gate RED)");
        // Just under file AMBER stays GREEN.
        let (s, _) = gate(r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-10","files":1099999}"#);
        assert_eq!(s, "GREEN");
    }

    #[test]
    fn file_gate_states_in_payload() {
        let tasks = r#"{"tasks":[]}"#;
        let plan = build_plan(tasks, r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-10","files":1150000}"#).unwrap();
        let fg = plan.get("file_gate").unwrap();
        assert_eq!(fg.get("state").unwrap().as_str(), Some("AMBER"));
        assert_eq!(fg.get("files").unwrap().as_num(), Some(1_150_000.0));
        assert_eq!(plan.get("plan_version").unwrap().as_str(), Some("v3.2"));
    }

    #[test]
    fn forecast_unavailable_without_history() {
        let tasks = r#"{"tasks":[]}"#;
        // No history at all.
        let plan = build_plan(tasks, r#"{"used_gb":71.0,"level":"GREEN","date":"2026-11-10","files":900000}"#).unwrap();
        assert_eq!(plan.get("forecast").unwrap().get("available").unwrap().as_bool(), Some(false));
        assert_eq!(plan.get("disk_gate").unwrap().get("state").unwrap().as_str(), Some("GREEN"));
        // Single-row history also degrades, never invents.
        let one = "2026-11-10\t88.0\tx\t1000000\n";
        let plan = build_plan_with_history(tasks, r#"{"used_gb":88.0,"level":"GREEN","date":"2026-11-10","files":1000000}"#, Some(one)).unwrap();
        assert_eq!(plan.get("forecast").unwrap().get("available").unwrap().as_bool(), Some(false));
        assert_eq!(plan.get("disk_gate").unwrap().get("state").unwrap().as_str(), Some("GREEN"));
    }

    #[test]
    fn forecast_escalates_green_to_amber() {
        // The v2.3 scenario (2026-11-10 evaluation): files grew
        // 958,424 -> 1,017,445 in one day (+59,021/day), putting
        // file AMBER ~1.4 days out — GREEN must escalate to
        // FORECAST AMBER even though no threshold is crossed yet.
        let tasks = r#"{"tasks":[{"coordinate":"1.1.1/1.1.1/1.1.1","title":"think","status":"open"}]}"#;
        let disk = r#"{"used_gb":82.0,"level":"GREEN","date":"2026-11-10","files":1017445}"#;
        let hist = "2026-11-09\t78.01\tx\t958424\n2026-11-10\t82.0\tx\t1017445\n";
        let plan = build_plan_with_history(tasks, disk, Some(hist)).unwrap();
        let f = plan.get("forecast").unwrap();
        assert_eq!(f.get("available").unwrap().as_bool(), Some(true));
        assert_eq!(f.get("files_per_day").unwrap().as_num(), Some(59021.0));
        assert_eq!(f.get("nearest").unwrap().as_str(), Some("file AMBER (1,100,000)"));
        let nd = f.get("nearest_days").unwrap().as_num().unwrap();
        assert!(nd > 1.3 && nd < 1.5, "nearest_days {} should be ~1.4", nd);
        let g = plan.get("disk_gate").unwrap();
        assert_eq!(g.get("state").unwrap().as_str(), Some("AMBER"));
        assert!(g.get("action").unwrap().as_str().unwrap().starts_with("FORECAST AMBER"));
    }

    #[test]
    fn forecast_does_not_touch_red_or_slow_growth() {
        let tasks = r#"{"tasks":[]}"#;
        // Crossing far away (> 3 days): gate stays GREEN.
        let hist = "2026-11-09\t70.0\tx\t800000\n2026-11-10\t70.5\tx\t801000\n";
        let plan = build_plan_with_history(tasks, r#"{"used_gb":70.5,"level":"GREEN","date":"2026-11-10","files":801000}"#, Some(hist)).unwrap();
        assert_eq!(plan.get("disk_gate").unwrap().get("state").unwrap().as_str(), Some("GREEN"));
        // Already RED: forecast never softens it.
        let plan = build_plan_with_history(tasks, r#"{"used_gb":103.0,"level":"GREEN","date":"2026-11-10","files":801000}"#, Some(hist)).unwrap();
        assert_eq!(plan.get("disk_gate").unwrap().get("state").unwrap().as_str(), Some("RED"));
        // Shrinking disk: no crossing at this rate -> null days.
        let hist = "2026-11-09\t80.0\tx\t900000\n2026-11-10\t79.0\tx\t890000\n";
        let plan = build_plan_with_history(tasks, r#"{"used_gb":79.0,"level":"GREEN","date":"2026-11-10","files":890000}"#, Some(hist)).unwrap();
        let f = plan.get("forecast").unwrap();
        assert_eq!(f.get("available").unwrap().as_bool(), Some(true));
        assert_eq!(f.get("nearest_days").unwrap(), &Json::Null);
    }
}
