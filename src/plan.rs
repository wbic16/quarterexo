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
/// `rows_out` receives the same rows for streaming emission.
pub fn build_plan(tasks_text: &str, disk_text: &str) -> Result<Json, String> {
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

    // ---- disk gate (v2.1, carried into v3 by the exo merge) ----
    // The plan used to only *report* the disk picture. On 2026-11-08 the
    // home volume hit its quota wall (EDQUOT at ~106 GB used; writes
    // failed while df still showed free space) and ~35 GB of WIP had to
    // be cleared under pressure. The planner acts on the picture, using
    // the disk-watch thresholds anchored to that failure
    // (AMBER >= 95 GB, RED >= 102 GB; disk_usage_report.py):
    //   RED   -> hold every new heavy placement; stage nothing on the VM
    //   AMBER -> no new VM staging; heavy execution ranch-only
    //   GREEN -> VM staging within quota headroom
    let disk_gate = if disk_level.eq_ignore_ascii_case("RED") || used_gb >= 102.0 {
        "RED"
    } else if disk_level.eq_ignore_ascii_case("AMBER") || used_gb >= 95.0 {
        "AMBER"
    } else {
        "GREEN"
    };
    let disk_gate_action = match disk_gate {
        "RED" => "HOLD all new heavy placements — stage nothing on the VM until usage is back under 95 GB",
        "AMBER" => "no new VM staging — heavy execution ranch-only until usage drops",
        _ => "VM staging within quota headroom (AMBER at 95 GB, RED at 102 GB)",
    };

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
        ("plan_version", Json::s("v3")),
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
    }
}
