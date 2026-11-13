//! The plan: placement proposal in the four currencies, with the
//! disk gates carried from the old line (v2.1 byte gate, v2.2
//! file-count gate, v2.3 forecast gate — behavior preserved, tests
//! ported).
//!
//! From-scratch shape: this module is the pure core. It computes
//! from VALUES — a current disk observation, a history of
//! observations, task rows, a host snapshot — and renders. Where
//! the values come from (the lattice's telemetry series, the task
//! register) is the caller's business; the old line's file-format
//! inputs were its own baggage and are not reproduced here. The
//! plan itself becomes a record in the lattice (current + history);
//! `render` produces the human text that a plan record stores and
//! bare `qexo` prints.

use std::fs;

const VIDEO_TOKENS: &[&str] = &[
    "video", "film", "ffmpeg", "encode", "encoding", "render", "rendering",
    "wavefront", "frame", "frames", "mp4", "mkv", "v14", "v15", "v16", "transcode",
];
const BUILD_TOKENS: &[&str] = &[
    "build", "builds", "compile", "compiler", "cargo", "test", "tests", "testing",
    "benchmark", "bulk", "suite", "phcc",
];

pub const PLAN_VERSION: &str = "v1.0";

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
    // combined text for any token-word as a substring first; only on
    // a substring hit pay for tokenization. Semantics unchanged
    // from the old line — substring hits that are not whole tokens
    // fall through to the exact token pass.
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

/// (MemAvailable MB, loadavg 1min) of the host qexo runs on — a
/// live probe of /proc, as before.
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

/// One disk observation: the current reading, and (as history
/// entries) the series the forecast gate rates growth from. In the
/// lattice these are telemetry scrolls; here they are values.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskObs {
    /// Epoch day (days_from_civil) of the observation.
    pub day: i64,
    pub used_gb: f64,
    /// The watcher's own level word ("GREEN"/"AMBER"/"RED"/"?").
    pub level: String,
    pub files: u64,
}

/// A task row as the planner consumes it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskInput {
    pub coordinate: String,
    pub title: String,
    pub request: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    pub available: bool,
    pub files_per_day: f64,
    pub gb_per_day: f64,
    pub span_days: f64,
    pub nearest_label: String,
    pub nearest_days: Option<f64>,
    pub file_amber_days: Option<f64>,
    pub byte_amber_days: Option<f64>,
}

impl Forecast {
    pub fn unavailable() -> Forecast {
        Forecast {
            available: false,
            files_per_day: 0.0,
            gb_per_day: 0.0,
            span_days: 0.0,
            nearest_label: "none".to_string(),
            nearest_days: None,
            file_amber_days: None,
            byte_amber_days: None,
        }
    }
}

/// The forecast computation (v2.3 semantics): rate growth from the
/// history's last two observations, project days to each of the
/// four thresholds, name the nearest crossing. Fewer than two
/// usable observations degrades to unavailable — never invented.
pub fn forecast_gate(history: &[DiskObs]) -> Forecast {
    if history.len() < 2 {
        return Forecast::unavailable();
    }
    let a = &history[history.len() - 2];
    let b = &history[history.len() - 1];
    let span = (b.day - a.day).max(1) as f64;
    let gb_rate = (b.used_gb - a.used_gb) / span;
    let fc_rate = (b.files as f64 - a.files as f64) / span;
    let days_to = |target: f64, cur: f64, rate: f64| -> Option<f64> {
        if cur >= target {
            Some(0.0)
        } else if rate > 0.0 {
            Some((target - cur) / rate)
        } else {
            None
        }
    };
    let cands: [(&str, Option<f64>); 4] = [
        ("file AMBER (1,100,000)", days_to(1_100_000.0, b.files as f64, fc_rate)),
        ("file RED (1,300,000)", days_to(1_300_000.0, b.files as f64, fc_rate)),
        ("byte AMBER (95 GB)", days_to(95.0, b.used_gb, gb_rate)),
        ("byte RED (102 GB)", days_to(102.0, b.used_gb, gb_rate)),
    ];
    let (near_label, near_days) = cands
        .iter()
        .fold(("none", None), |acc, c| match (acc.1, c.1) {
            (None, Some(_)) => (c.0, c.1),
            (Some(a), Some(d)) if d < a => (c.0, c.1),
            _ => acc,
        });
    Forecast {
        available: true,
        files_per_day: fc_rate.round(),
        gb_per_day: (gb_rate * 100.0).round() / 100.0,
        span_days: span,
        nearest_label: near_label.to_string(),
        nearest_days: near_days.map(|d| (d * 100.0).round() / 100.0),
        file_amber_days: cands[0].1.map(|d| (d * 100.0).round() / 100.0),
        byte_amber_days: cands[2].1.map(|d| (d * 100.0).round() / 100.0),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanRow {
    pub coordinate: String,
    pub title: String,
    pub class: Class,
    pub execution: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanData {
    pub disk_date: String,
    pub used_gb: f64,
    pub disk_level: String,
    pub files: u64,
    pub byte_gate: String,
    pub file_gate: String,
    pub disk_gate: String,
    pub disk_gate_action: String,
    pub forecast: Forecast,
    pub ram_available_mb: f64,
    pub load1: f64,
    pub counts: (usize, usize, usize), // video, build, light
    pub rows: Vec<PlanRow>,
}

impl PlanData {
    pub fn open_tasks(&self) -> usize {
        self.rows.len()
    }
}

/// Compute the plan (old build_plan semantics):
/// - byte gate: RED at level RED or >= 102 GB; AMBER at level AMBER
///   or >= 95 GB (thresholds anchored to the 2026-11-08 EDQUOT
///   failure at ~106 GB).
/// - file gate (v2.2): AMBER at 1,100,000 files, RED at 1,300,000 —
///   the count is the faster-moving half of the disk currency and a
///   byte-only gate can sit GREEN while the count runs at the wall.
/// - disk gate = the worse of the two halves.
/// - forecast gate (v2.3): a GREEN disk gate escalates to AMBER when
///   the nearest projected crossing is within 3 days. A planner
///   that only watches crossings gives no lead time.
pub fn compute(tasks: &[TaskInput], current: &DiskObs, history: &[DiskObs], host: (f64, f64)) -> PlanData {
    let byte_gate = if current.level.eq_ignore_ascii_case("RED") || current.used_gb >= 102.0 {
        "RED"
    } else if current.level.eq_ignore_ascii_case("AMBER") || current.used_gb >= 95.0 {
        "AMBER"
    } else {
        "GREEN"
    };
    let file_gate = if current.files >= 1_300_000 {
        "RED"
    } else if current.files >= 1_100_000 {
        "AMBER"
    } else {
        "GREEN"
    };
    let sev = |g: &str| match g {
        "RED" => 2,
        "AMBER" => 1,
        _ => 0,
    };
    let mut disk_gate = if sev(file_gate) > sev(byte_gate) { file_gate } else { byte_gate };
    let mut disk_gate_action: String = match disk_gate {
        "RED" => "HOLD all new heavy placements — stage nothing on the VM until the disk gate clears (bytes back under 95 GB and files back under 1,100,000)".to_string(),
        "AMBER" => "no new VM staging — heavy execution ranch-only until the disk gate clears".to_string(),
        _ => "VM staging within quota headroom (bytes: AMBER at 95 GB, RED at 102 GB; files: AMBER at 1,100,000, RED at 1,300,000)".to_string(),
    };
    let forecast = forecast_gate(history);
    if let Some(near_days) = forecast.nearest_days {
        if near_days <= 3.0 && disk_gate == "GREEN" {
            disk_gate = "AMBER";
            disk_gate_action = format!(
                "FORECAST AMBER — {} projected in {:.1} days at the last interval's growth rate; no new VM staging, heavy execution ranch-only, move staging before the crossing",
                forecast.nearest_label, near_days
            );
        }
    }

    let (mut nv, mut nb, mut nl) = (0usize, 0usize, 0usize);
    let mut rows: Vec<PlanRow> = Vec::new();
    for t in tasks {
        let class = classify(&t.title, &t.request);
        match class {
            Class::Video => nv += 1,
            Class::Build => nb += 1,
            Class::Light => nl += 1,
        }
        let execution = if disk_gate == "RED" && class != Class::Light {
            "HELD (disk gate RED)"
        } else if disk_gate == "AMBER" && class == Class::Light {
            "vm:light-coordination (no staging)"
        } else {
            class.execution()
        };
        rows.push(PlanRow {
            coordinate: t.coordinate.clone(),
            title: t.title.clone(),
            class,
            execution: execution.to_string(),
        });
    }

    PlanData {
        disk_date: String::new(), // caller stamps the reading's date text
        used_gb: current.used_gb,
        disk_level: current.level.clone(),
        files: current.files,
        byte_gate: byte_gate.to_string(),
        file_gate: file_gate.to_string(),
        disk_gate: disk_gate.to_string(),
        disk_gate_action,
        forecast,
        ram_available_mb: host.0,
        load1: host.1,
        counts: (nv, nb, nl),
        rows,
    }
}

/// Days from civil date (Howard Hinnant's algorithm) — the v2.3
/// date arithmetic, carried verbatim in behavior.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let ya = if m <= 2 { y - 1 } else { y };
    let era = if ya >= 0 { ya } else { ya - 399 } / 400;
    let yoe = ya - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn forecast_line(plan: &PlanData) -> String {
    let f = &plan.forecast;
    if !f.available {
        return "forecast unavailable: disk-watch history missing or single-row".to_string();
    }
    let fmt_days = |v: Option<f64>| -> String {
        match v {
            Some(d) => format!("{:.1} d", d),
            None => "never at this rate".to_string(),
        }
    };
    format!(
        "growth {:+.0} files/day, {:+.2} GB/day (last interval, {} d) — nearest crossing: {} in {}; file AMBER {}, byte AMBER {}",
        f.files_per_day,
        f.gb_per_day,
        f.span_days as i64,
        f.nearest_label,
        fmt_days(f.nearest_days),
        fmt_days(f.file_amber_days),
        fmt_days(f.byte_amber_days)
    )
}

/// Human-readable rendering of a plan (the v1 shape, kept). In the
/// scratch line this renders from PlanData; the same text is what a
/// plan record stores as its rendered scroll.
pub fn render(plan: &PlanData) -> String {
    let mut out = String::new();
    out.push_str("qexo — placement plan\n");
    out.push_str("four currencies, at a glance:\n");
    out.push_str(&format!(
        "  disk:      {} GB used ({}), {} files\n",
        plan.used_gb, plan.disk_level, plan.files
    ));
    out.push_str(&format!(
        "  disk gate: {} — {}\n",
        plan.disk_gate, plan.disk_gate_action
    ));
    out.push_str(&format!(
        "  file gate: {} — {} files on the VM volume (AMBER at 1,100,000, RED at 1,300,000)\n",
        plan.file_gate, plan.files
    ));
    out.push_str(&format!("  forecast:  {}\n", forecast_line(plan)));
    out.push_str(&format!(
        "  cpu/ram:   host snapshot — {:.0} MB RAM available, load {:.2}\n",
        plan.ram_available_mb, plan.load1
    ));
    out.push_str(&format!("  inference: Emise carries all {} open tasks\n", plan.open_tasks()));
    out.push_str("\nplacements (inference → Emise for every task):\n");
    if plan.disk_gate == "RED" {
        out.push_str("  ** DISK GATE RED: every heavy placement below is HELD — do not dispatch until the gate clears. **\n");
    }
    for r in &plan.rows {
        let title: String = r.title.chars().take(78).collect();
        out.push_str(&format!(
            "  [{}] {} — inference: Emise · exec: {} · class: {}\n",
            r.coordinate,
            title,
            r.execution,
            r.class.name()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(day: i64, used_gb: f64, level: &str, files: u64) -> DiskObs {
        DiskObs { day, used_gb, level: level.to_string(), files }
    }
    fn task(coord: &str, title: &str) -> TaskInput {
        TaskInput { coordinate: coord.into(), title: title.into(), request: String::new() }
    }

    #[test]
    fn classify_precedence() {
        assert_eq!(classify("V16 film render", ""), Class::Video);
        assert_eq!(classify("cargo build the thing", ""), Class::Build);
        assert_eq!(classify("write a note", ""), Class::Light);
    }

    #[test]
    fn disk_gate_states() {
        // Ported from the old line's gate battery: same readings,
        // same outcomes, values instead of file text.
        let tasks = vec![task("1.1.1/1.1.1/1.1.1", "film encode"), task("2.2.2/2.2.2/2.2.2", "think")];
        let gate = |o: DiskObs| compute(&tasks, &o, &[], (0.0, 0.0));
        let p = gate(obs(1, 71.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "GREEN");
        assert_eq!(p.rows[0].execution, "ranch:aletheia-core (video — never on VM)");
        assert_eq!(p.rows[1].execution, "vm:light-coordination");
        let p = gate(obs(1, 96.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "AMBER");
        assert_eq!(p.rows[1].execution, "vm:light-coordination (no staging)");
        let p = gate(obs(1, 50.0, "RED", 1));
        assert_eq!(p.disk_gate, "RED");
        assert_eq!(p.rows[0].execution, "HELD (disk gate RED)");
        assert_eq!(p.rows[1].execution, "vm:light-coordination");
        let p = gate(obs(1, 103.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "RED");
        let p = gate(obs(1, 71.0, "GREEN", 1_150_000));
        assert_eq!(p.disk_gate, "AMBER");
        assert_eq!(p.file_gate, "AMBER");
        let p = gate(obs(1, 71.0, "GREEN", 1_310_000));
        assert_eq!(p.disk_gate, "RED");
        assert_eq!(p.rows[0].execution, "HELD (disk gate RED)");
        let p = gate(obs(1, 71.0, "GREEN", 1_099_999));
        assert_eq!(p.disk_gate, "GREEN");
    }

    #[test]
    fn forecast_unavailable_without_history() {
        let p = compute(&[], &obs(1, 71.0, "GREEN", 900_000), &[], (0.0, 0.0));
        assert!(!p.forecast.available);
        assert_eq!(p.disk_gate, "GREEN");
        let one = vec![obs(1, 88.0, "GREEN", 1_000_000)];
        let p = compute(&[], &obs(1, 88.0, "GREEN", 1_000_000), &one, (0.0, 0.0));
        assert!(!p.forecast.available);
        assert_eq!(p.disk_gate, "GREEN");
    }

    #[test]
    fn forecast_escalates_green_to_amber() {
        // The v2.3 scenario (2026-11-10 evaluation): files grew
        // 958,424 -> 1,017,445 in one day (+59,021/day), putting
        // file AMBER ~1.4 days out.
        let d0 = days_from_civil(2026, 11, 9);
        let d1 = days_from_civil(2026, 11, 10);
        let hist = vec![obs(d0, 78.01, "GREEN", 958_424), obs(d1, 82.0, "GREEN", 1_017_445)];
        let cur = obs(d1, 82.0, "GREEN", 1_017_445);
        let p = compute(&[task("1.1.1/1.1.1/1.1.1", "think")], &cur, &hist, (0.0, 0.0));
        assert!(p.forecast.available);
        assert_eq!(p.forecast.files_per_day, 59021.0);
        assert_eq!(p.forecast.nearest_label, "file AMBER (1,100,000)");
        let nd = p.forecast.nearest_days.unwrap();
        assert!(nd > 1.3 && nd < 1.5, "nearest_days {} should be ~1.4", nd);
        assert_eq!(p.disk_gate, "AMBER");
        assert!(p.disk_gate_action.starts_with("FORECAST AMBER"));
    }

    #[test]
    fn forecast_does_not_touch_red_or_slow_growth() {
        let hist = vec![obs(1, 70.0, "GREEN", 800_000), obs(2, 70.5, "GREEN", 801_000)];
        let p = compute(&[], &obs(2, 70.5, "GREEN", 801_000), &hist, (0.0, 0.0));
        assert_eq!(p.disk_gate, "GREEN");
        let p = compute(&[], &obs(2, 103.0, "GREEN", 801_000), &hist, (0.0, 0.0));
        assert_eq!(p.disk_gate, "RED");
        let hist = vec![obs(1, 80.0, "GREEN", 900_000), obs(2, 79.0, "GREEN", 890_000)];
        let p = compute(&[], &obs(2, 79.0, "GREEN", 890_000), &hist, (0.0, 0.0));
        assert!(p.forecast.available);
        assert_eq!(p.forecast.nearest_days, None);
    }

    #[test]
    fn render_header_and_task_line() {
        // The old line's plan_human regression, on PlanData: the
        // renderer takes the whole plan value, header + task line.
        let p = compute(
            &[task("1.1.1/1.1.1/1.1.1", "film encode")],
            &obs(1, 71.0, "GREEN", 900_000),
            &[],
            (4096.0, 0.5),
        );
        let out = render(&p);
        assert!(out.starts_with("qexo — placement plan\n"), "header missing");
        assert!(out.contains("[1.1.1/1.1.1/1.1.1] film encode"), "task line missing:\n{}", out);
        assert!(out.contains("disk gate: GREEN"), "gate line missing:\n{}", out);
    }
}
