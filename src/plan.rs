//! The plan: placement proposal in the four currencies, with the
//! disk gates carried from the old line (v2.1 byte gate, v2.2
//! file-count gate, v2.3 forecast gate — behavior preserved, tests
//! ported) and, from v1.0.2, re-based on the volume the gate
//! actually measures: the old absolute byte thresholds were the
//! Muse VM's staging quota wearing a universal costume. The gate
//! now reads through two lenses — a volume lens (free fraction of
//! the measured volume) and a quota lens (absolute used-bytes
//! ceiling, on only where a node configures it) — plus the file
//! lens, with every threshold read from the node's own config
//! record in state.phext. From v1.0.3 the forecast gate projects
//! the same lenses the gate itself reads: v1.0.2 moved the gate
//! but left the forecast projecting the old absolute ceilings,
//! so on any node whose volume already sat past 95 GB used —
//! every ranch node — the second plan read "FORECAST AMBER,
//! byte AMBER projected in 0.0 days" over a GREEN gate.
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

pub const PLAN_VERSION: &str = "v1.6.2";

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

/// A volume as the probe measures it: the filesystem under a
/// named root, in bytes. The gate judges THIS volume — not a
/// quota remembered from another machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    /// The measured root (its volume is what was measured).
    pub root: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    /// Bytes available to an unprivileged writer (statvfs f_bavail;
    /// on Windows, GetDiskFreeSpaceExW's caller-available figure).
    pub free_bytes: u64,
}

impl Volume {
    /// Free space as a percentage of the whole volume.
    pub fn free_pct(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        self.free_bytes as f64 * 100.0 / self.total_bytes as f64
    }
}

/// The gate's thresholds, read from the node's config record in
/// state.phext (homeland 9.9.9/9.9.9/9.9.9). Absent keys mean the
/// defaults — the defaults are the volume lens only: a ranch
/// volume is judged by its own free fraction, and no absolute
/// ceiling applies until a node writes one into its config.
#[derive(Debug, Clone, PartialEq)]
pub struct GateConfig {
    pub volume_on: bool,
    /// Volume lens: AMBER when free < this % of total.
    pub free_amber_pct: f64,
    /// Volume lens: RED when free < this % of total.
    pub free_red_pct: f64,
    pub quota_on: bool,
    /// Quota lens: AMBER at this many GB used (decimal, telemetry GB).
    pub quota_amber_gb: f64,
    /// Quota lens: RED at this many GB used.
    pub quota_red_gb: f64,
    /// File lens: AMBER at this many files under the staging root.
    pub file_amber: u64,
    /// File lens: RED at this many files.
    pub file_red: u64,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            volume_on: true,
            free_amber_pct: 25.0,
            free_red_pct: 10.0,
            quota_on: false,
            quota_amber_gb: 95.0,
            quota_red_gb: 102.0,
            file_amber: 1_100_000,
            file_red: 1_300_000,
        }
    }
}

impl GateConfig {
    /// Build from a config record via a field lookup (the record's
    /// `key: value` lines; the caller supplies the reader so this
    /// module stays free of lattice mechanics). Keys:
    /// volume_lens, disk_gate_free_amber_pct, disk_gate_free_red_pct,
    /// quota_lens, quota_amber_gb, quota_red_gb, file_amber, file_red.
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> GateConfig {
        let mut cfg = GateConfig::default();
        let on = |v: &str| v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true");
        if let Some(v) = get("volume_lens") {
            cfg.volume_on = on(&v);
        }
        if let Some(v) = get("disk_gate_free_amber_pct").and_then(|s| s.parse().ok()) {
            cfg.free_amber_pct = v;
        }
        if let Some(v) = get("disk_gate_free_red_pct").and_then(|s| s.parse().ok()) {
            cfg.free_red_pct = v;
        }
        if let Some(v) = get("quota_lens") {
            cfg.quota_on = on(&v);
        }
        if let Some(v) = get("quota_amber_gb").and_then(|s| s.parse().ok()) {
            cfg.quota_amber_gb = v;
        }
        if let Some(v) = get("quota_red_gb").and_then(|s| s.parse().ok()) {
            cfg.quota_red_gb = v;
        }
        if let Some(v) = get("file_amber").and_then(|s| s.parse().ok()) {
            cfg.file_amber = v;
        }
        if let Some(v) = get("file_red").and_then(|s| s.parse().ok()) {
            cfg.file_red = v;
        }
        cfg
    }
}

/// Everything the gate needs beyond the observations: the volume
/// the probe measured (None when the probe could not read one),
/// the root the file count was taken under, and the thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct GateInputs {
    pub volume: Option<Volume>,
    pub files_root: String,
    pub config: GateConfig,
}

/// The gate's verdict, with its reasons kept as text: which lens
/// fired and the numbers it fired on. The plan renders these; a
/// gate that cannot say why it fired is a gate no one can argue
/// with — or trust.
#[derive(Debug, Clone, PartialEq)]
pub struct GateEval {
    pub byte_gate: String,
    pub file_gate: String,
    pub disk_gate: String,
    /// Why the byte half reads as it does, lens by lens.
    pub byte_detail: String,
    /// The file half, with the counted root named.
    pub file_detail: String,
    /// The firing lens's reason (the worse half's detail).
    pub reason: String,
    /// The directive line for the combined gate (pre-forecast).
    pub action: String,
}

fn sev(g: &str) -> i32 {
    match g {
        "RED" => 2,
        "AMBER" => 1,
        _ => 0,
    }
}

fn gate_word(s: i32) -> &'static str {
    match s {
        2 => "RED",
        1 => "AMBER",
        _ => "GREEN",
    }
}

fn separated(digits: &str) -> String {
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Format bytes as GiB (2^30) with thousands separators — the
/// units `df -h` speaks, labeled so no one has to guess.
pub fn fmt_gib(bytes: u64) -> String {
    let gib = bytes as f64 / 1_073_741_824.0;
    let whole = gib.round() as u64;
    format!("{} GiB", separated(&whole.to_string()))
}

/// Format a count with thousands separators (1,100,000).
pub fn fmt_count(n: u64) -> String {
    separated(&n.to_string())
}

/// Evaluate the disk gate from values: the current observation,
/// the measured volume, the file count's root, the thresholds.
/// Byte half = the worse of the lenses that are ON; disk gate =
/// the worse of byte and file halves. The observation's own level
/// word is NOT consulted: since the telemetry series is qexo's
/// own, the level recorded with an observation is this very
/// evaluation's output (the caller stamps it), not an input.
pub fn evaluate_gate(current: &DiskObs, vol: Option<&Volume>, files_root: &str, cfg: &GateConfig) -> GateEval {
    let mut byte_sev = 0;
    let mut byte_parts: Vec<String> = Vec::new();
    if cfg.volume_on {
        if let Some(v) = vol {
            let free_pct = v.free_pct();
            let s = if free_pct < cfg.free_red_pct {
                2
            } else if free_pct < cfg.free_amber_pct {
                1
            } else {
                0
            };
            byte_sev = byte_sev.max(s);
            byte_parts.push(format!(
                "volume lens: {:.0}% free of {} (AMBER under {:.0}% free, RED under {:.0}%)",
                free_pct,
                fmt_gib(v.total_bytes),
                cfg.free_amber_pct,
                cfg.free_red_pct
            ));
        } else {
            byte_parts.push("volume lens: no volume reading available".to_string());
        }
    }
    if cfg.quota_on {
        let s = if current.used_gb >= cfg.quota_red_gb {
            2
        } else if current.used_gb >= cfg.quota_amber_gb {
            1
        } else {
            0
        };
        byte_sev = byte_sev.max(s);
        byte_parts.push(format!(
            "quota lens: {:.2} GB used (AMBER at {:.0} GB, RED at {:.0} GB ceiling)",
            current.used_gb, cfg.quota_amber_gb, cfg.quota_red_gb
        ));
    }
    if byte_parts.is_empty() {
        byte_parts.push("no byte lens configured".to_string());
    }
    let byte_gate = gate_word(byte_sev).to_string();
    let byte_detail = byte_parts.join("; ");

    let file_sev = if current.files >= cfg.file_red {
        2
    } else if current.files >= cfg.file_amber {
        1
    } else {
        0
    };
    let file_gate = gate_word(file_sev).to_string();
    let file_detail = format!(
        "{} files under {} (AMBER at {}, RED at {})",
        current.files, files_root, cfg.file_amber, cfg.file_red
    );

    let (disk_gate, reason) = if file_sev > byte_sev {
        (file_gate.clone(), file_detail.clone())
    } else {
        (byte_gate.clone(), byte_detail.clone())
    };
    let root = vol.map(|v| v.root.as_str()).unwrap_or(files_root);
    let action = match disk_gate.as_str() {
        "RED" => format!(
            "HOLD all new heavy placements — stage nothing under {} until the disk gate clears ({})",
            root, reason
        ),
        "AMBER" => format!(
            "no new staging under {} — heavy execution ranch-only until the disk gate clears ({})",
            root, reason
        ),
        _ => format!("staging within headroom ({})", byte_detail),
    };
    GateEval { byte_gate, file_gate, disk_gate, byte_detail, file_detail, reason, action }
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
    /// How many per-interval rates the reported rates are the
    /// median of (v1.2.1): 1 means the single-interval rate, the
    /// pre-v1.2.1 behavior, kept for a two-observation history.
    pub intervals: usize,
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
            intervals: 0,
            nearest_label: "none".to_string(),
            nearest_days: None,
            file_amber_days: None,
            byte_amber_days: None,
        }
    }
}

/// How far ahead a crossing projection is reported, in days. A
/// linear rate read off the last interval is a short-horizon
/// instrument: a crossing farther out than this will be
/// re-projected hundreds of times before it can matter, and
/// naming a day count for it lends it a precision it does not
/// have. Crossings beyond the horizon are reported as no
/// crossing.
pub const FORECAST_HORIZON_DAYS: f64 = 365.0;

/// How many of the most recent intervals the forecast rates
/// (v1.2.1). The window bounds the median's memory: older
/// intervals fall out instead of anchoring the rate forever,
/// and a history shorter than the window simply contributes
/// every interval it has.
pub const FORECAST_WINDOW_INTERVALS: usize = 7;

/// The median of a set of per-interval rates: the middle rate
/// (the mean of the two middle rates for an even count). A
/// single-interval spike or stall moves the median not at all
/// until it is half the window — that is the point (v1.2.1).
fn median(rates: &mut [f64]) -> f64 {
    if rates.is_empty() {
        return 0.0;
    }
    rates.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let n = rates.len();
    if n % 2 == 1 {
        rates[n / 2]
    } else {
        (rates[n / 2 - 1] + rates[n / 2]) / 2.0
    }
}

/// The forecast computation (v2.3 semantics, re-based on the
/// gate's lenses in v1.0.3; rates re-based on the median
/// interval in v1.2.1): rate growth from the history — the
/// median of the per-interval rates over the most recent
/// FORECAST_WINDOW_INTERVALS intervals, falling back to the
/// single interval when the history holds only two observations
/// — then project days to the crossings of the
/// SAME thresholds evaluate_gate reads — the volume lens's free
/// fractions of this volume's total, the quota lens's ceilings
/// (only when the node has that lens on), and the file lens's
/// configured counts. Until v1.0.3 the byte half projected the
/// legacy absolute levels (95/102 GB) whatever the node's
/// configuration, so the forecast contradicted the gate it
/// forecasts on every ranch volume.
///
/// A crossing the current reading has already reached is the
/// gate's verdict, not a forecast: projecting it would report
/// 0.0 days for a level the lenses already own — exactly the
/// v1.0.2 fleet failure, where the second plan on a healthy
/// ranch node flipped to FORECAST AMBER because its volume had
/// long since passed a ceiling that no longer applies to it.
/// Flat or falling growth projects nothing, and nothing is
/// projected beyond FORECAST_HORIZON_DAYS. Fewer than two usable
/// observations degrades to unavailable — never invented.
///
/// Why the median (v1.2.1, from the 2026-11-12 evaluation of
/// 2026-11-11 and re-proven by 2026-11-12's interval): rating
/// from the last interval alone made the nearest-crossing
/// estimate whipsaw with whatever the last day happened to do —
/// file AMBER read ~1.4 days out on the 2026-11-10 interval,
/// ~14 days out on 2026-11-11's, ~9.9 days out on 2026-11-12's,
/// while the underlying series never changed character. The
/// median of the recent intervals reports the series' typical
/// rate: a planner whose warnings hold still long enough to
/// act on.
pub fn forecast_gate(history: &[DiskObs], cfg: &GateConfig, total_bytes: Option<u64>) -> Forecast {
    if history.len() < 2 {
        return Forecast::unavailable();
    }
    let b = &history[history.len() - 1];
    let start = history.len().saturating_sub(FORECAST_WINDOW_INTERVALS + 1);
    let window = &history[start..];
    let mut gb_rates: Vec<f64> = Vec::new();
    let mut fc_rates: Vec<f64> = Vec::new();
    for pair in window.windows(2) {
        let ispan = (pair[1].day - pair[0].day).max(1) as f64;
        gb_rates.push((pair[1].used_gb - pair[0].used_gb) / ispan);
        fc_rates.push((pair[1].files as f64 - pair[0].files as f64) / ispan);
    }
    let intervals = gb_rates.len();
    let gb_rate = median(&mut gb_rates);
    let fc_rate = median(&mut fc_rates);
    let span = (b.day - window[0].day).max(1) as f64;
    let days_to = |target: f64, cur: f64, rate: f64| -> Option<f64> {
        if cur >= target || rate <= 0.0 {
            None
        } else {
            let d = (target - cur) / rate;
            if d <= FORECAST_HORIZON_DAYS { Some(d) } else { None }
        }
    };
    let mut cands: Vec<(String, Option<f64>)> = vec![
        (
            format!("file AMBER ({})", fmt_count(cfg.file_amber)),
            days_to(cfg.file_amber as f64, b.files as f64, fc_rate),
        ),
        (
            format!("file RED ({})", fmt_count(cfg.file_red)),
            days_to(cfg.file_red as f64, b.files as f64, fc_rate),
        ),
    ];
    // The nearest byte-lens AMBER crossing across the lenses that
    // are on, for the rendered summary line.
    let mut byte_amber: Option<f64> = None;
    let mut note_amber = |d: Option<f64>| {
        if let Some(d) = d {
            byte_amber = Some(byte_amber.map_or(d, |x: f64| x.min(d)));
        }
    };
    if cfg.volume_on {
        if let Some(total) = total_bytes {
            // used_gb is decimal GB (bytes / 1e9, as the probe
            // records it); the lens targets are used levels at
            // which the free fraction of THIS volume crosses the
            // configured percentages.
            let total_gb = total as f64 / 1.0e9;
            let amber_used = total_gb * (1.0 - cfg.free_amber_pct / 100.0);
            let red_used = total_gb * (1.0 - cfg.free_red_pct / 100.0);
            let da = days_to(amber_used, b.used_gb, gb_rate);
            note_amber(da);
            cands.push((format!("volume AMBER ({:.0}% free)", cfg.free_amber_pct), da));
            cands.push((
                format!("volume RED ({:.0}% free)", cfg.free_red_pct),
                days_to(red_used, b.used_gb, gb_rate),
            ));
        }
    }
    if cfg.quota_on {
        let da = days_to(cfg.quota_amber_gb, b.used_gb, gb_rate);
        note_amber(da);
        cands.push((format!("quota AMBER ({:.0} GB)", cfg.quota_amber_gb), da));
        cands.push((
            format!("quota RED ({:.0} GB)", cfg.quota_red_gb),
            days_to(cfg.quota_red_gb, b.used_gb, gb_rate),
        ));
    }
    let file_amber_days = cands[0].1;
    let (near_label, near_days) = cands
        .into_iter()
        .fold(("none".to_string(), None), |acc, c| match (acc.1, c.1) {
            (None, Some(_)) => (c.0, c.1),
            (Some(a), Some(d)) if d < a => (c.0, c.1),
            _ => acc,
        });
    let round2 = |d: f64| (d * 100.0).round() / 100.0;
    Forecast {
        available: true,
        files_per_day: fc_rate.round(),
        gb_per_day: round2(gb_rate),
        span_days: span,
        intervals,
        nearest_label: near_label,
        nearest_days: near_days.map(round2),
        file_amber_days: file_amber_days.map(round2),
        byte_amber_days: byte_amber.map(round2),
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
    /// The measured volume (root + bytes), for rendering: the
    /// plan names the volume it judged, in GiB.
    pub vol_root: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    /// The root the file count was taken under, named in the plan.
    pub files_root: String,
    pub byte_gate: String,
    pub file_gate: String,
    /// Which lens fired and the numbers it fired on.
    pub gate_reason: String,
    pub byte_detail: String,
    pub file_detail: String,
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

/// Compute the plan:
/// - byte gate: the worse of the lenses that are ON (see
///   evaluate_gate) — the volume lens judges the measured volume's
///   free fraction; the quota lens, only where a node configures
///   it, reproduces the old absolute ceiling (AMBER 95 GB / RED
///   102 GB, thresholds anchored to the 2026-11-08 EDQUOT failure
///   at ~106 GB on the Muse VM's staging quota — a real ceiling
///   on that one machine, a false verdict everywhere else).
/// - file gate (v2.2): AMBER/RED at the configured counts under
///   the named staging root — the count is the faster-moving half
///   of the disk currency, and a byte-only gate can sit GREEN
///   while the count runs at the wall.
/// - disk gate = the worse of the two halves.
/// - forecast gate (v2.3, re-based on the lenses in v1.0.3): a
///   GREEN disk gate escalates to AMBER when the nearest crossing
///   projected under the node's configured lenses is within 3
///   days. A planner that only watches crossings gives no lead
///   time — and a forecast that projects levels the gate no
///   longer reads gives false alarms instead.
pub fn compute(tasks: &[TaskInput], current: &DiskObs, history: &[DiskObs], host: (f64, f64), gate: &GateInputs) -> PlanData {
    let eval = evaluate_gate(current, gate.volume.as_ref(), &gate.files_root, &gate.config);
    let byte_gate = eval.byte_gate.as_str();
    let file_gate = eval.file_gate.as_str();
    let mut disk_gate = eval.disk_gate.as_str();
    let mut disk_gate_action: String = eval.action.clone();
    let forecast = forecast_gate(history, &gate.config, gate.volume.as_ref().map(|v| v.total_bytes));
    if let Some(near_days) = forecast.nearest_days {
        if near_days <= 3.0 && disk_gate == "GREEN" {
            disk_gate = "AMBER";
            disk_gate_action = format!(
                "FORECAST AMBER — {} projected in {:.1} days at the last interval's growth rate; no new staging, heavy execution ranch-only, move staging before the crossing",
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
        vol_root: gate.volume.as_ref().map(|v| v.root.clone()).unwrap_or_default(),
        total_bytes: gate.volume.as_ref().map(|v| v.total_bytes).unwrap_or(0),
        used_bytes: gate.volume.as_ref().map(|v| v.used_bytes).unwrap_or(0),
        free_bytes: gate.volume.as_ref().map(|v| v.free_bytes).unwrap_or(0),
        files_root: gate.files_root.clone(),
        byte_gate: byte_gate.to_string(),
        file_gate: file_gate.to_string(),
        gate_reason: eval.reason.clone(),
        byte_detail: eval.byte_detail.clone(),
        file_detail: eval.file_detail.clone(),
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
    let basis = if f.intervals <= 1 {
        format!("last interval, {} d", f.span_days as i64)
    } else {
        format!("median of {} intervals, {} d", f.intervals, f.span_days as i64)
    };
    format!(
        "growth {:+.0} files/day, {:+.2} GB/day ({}) — nearest crossing: {} in {}; file AMBER {}, byte AMBER {}",
        f.files_per_day,
        f.gb_per_day,
        basis,
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
    if plan.total_bytes > 0 {
        out.push_str(&format!(
            "  disk:      {} used of {} ({} free) on the volume at {}, {} files\n",
            fmt_gib(plan.used_bytes),
            fmt_gib(plan.total_bytes),
            fmt_gib(plan.free_bytes),
            plan.vol_root,
            plan.files
        ));
    } else {
        out.push_str(&format!(
            "  disk:      {:.2} GB used ({}), {} files — no volume reading available\n",
            plan.used_gb, plan.disk_level, plan.files
        ));
    }
    out.push_str(&format!(
        "  disk gate: {} — {}\n",
        plan.disk_gate, plan.disk_gate_action
    ));
    out.push_str(&format!(
        "  file gate: {} — {}\n",
        plan.file_gate, plan.file_detail
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
    /// Default gate inputs: the from-scratch defaults — volume
    /// lens only (no volume supplied in most unit fixtures), file
    /// lens at the standard counts.
    fn gin() -> GateInputs {
        GateInputs { volume: None, files_root: "/staging".to_string(), config: GateConfig::default() }
    }
    /// The old world's configuration: the quota lens on at the
    /// VM's 95/102 GB ceiling. Every pre-v1.0.2 gate expectation is
    /// expressed through this — the old behavior, now by config.
    fn gin_quota() -> GateInputs {
        let mut config = GateConfig::default();
        config.quota_on = true;
        GateInputs { volume: None, files_root: "/staging".to_string(), config }
    }
    fn vol(root: &str, total: u64, used: u64, free: u64) -> Volume {
        Volume { root: root.to_string(), total_bytes: total, used_bytes: used, free_bytes: free }
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
        // same outcomes, values instead of file text — expressed
        // through the quota lens (gin_quota), which is what the old
        // absolute thresholds now are. One old case is retired with
        // its mechanism: an external watcher's level word forcing
        // RED. The telemetry series is qexo's own now; the level
        // recorded with an observation is this gate's own output,
        // not an input, so there is no watcher word to honor.
        let tasks = vec![task("1.1.1/1.1.1/1.1.1", "film encode"), task("2.2.2/2.2.2/2.2.2", "think")];
        let gate = |o: DiskObs| compute(&tasks, &o, &[], (0.0, 0.0), &gin_quota());
        let p = gate(obs(1, 71.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "GREEN");
        assert_eq!(p.rows[0].execution, "ranch:aletheia-core (video — never on VM)");
        assert_eq!(p.rows[1].execution, "vm:light-coordination");
        let p = gate(obs(1, 96.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "AMBER");
        assert_eq!(p.rows[1].execution, "vm:light-coordination (no staging)");
        let p = gate(obs(1, 103.0, "GREEN", 1));
        assert_eq!(p.disk_gate, "RED");
        assert_eq!(p.rows[0].execution, "HELD (disk gate RED)");
        assert_eq!(p.rows[1].execution, "vm:light-coordination");
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
    fn gate_ranch_volume_is_green() {
        // The aletheia-core incident (2026-11-12): 808.96 GB used
        // on a 3.6 TB volume with ~2.7 TB free read RED under the
        // VM's absolute thresholds. Through the volume lens, the
        // same numbers are what they are: a healthy volume.
        let gib = 1_073_741_824u64;
        let v = vol("/", 3_586 * gib, 754 * gib, 2_700 * gib);
        let g = GateInputs { volume: Some(v), files_root: "/var/lib/qexo".to_string(), config: GateConfig::default() };
        let p = compute(&[task("1.1.1/1.1.1/1.1.1", "think")], &obs(1, 808.96, "GREEN", 6), &[], (0.0, 0.0), &g);
        assert_eq!(p.byte_gate, "GREEN");
        assert_eq!(p.disk_gate, "GREEN");
        assert!(p.gate_reason.contains("volume lens"), "reason: {}", p.gate_reason);
        assert!(p.file_detail.contains("6 files under /var/lib/qexo"), "file detail: {}", p.file_detail);
        let out = render(&p);
        assert!(out.contains("754 GiB used of 3,586 GiB (2,700 GiB free)"), "disk line:\n{}", out);
    }

    #[test]
    fn gate_vm_quota_by_configuration() {
        // The VM case: quota lens on at 95/102 GB — the old world's
        // behavior, reproduced only where a node configures it.
        let gate = |used: f64| {
            let o = obs(1, used, "GREEN", 1);
            evaluate_gate(&o, None, "/staging", &gin_quota().config)
        };
        let e = gate(97.0);
        assert_eq!(e.byte_gate, "AMBER");
        assert_eq!(e.disk_gate, "AMBER");
        assert!(e.reason.contains("quota lens"), "reason: {}", e.reason);
        let e = gate(103.0);
        assert_eq!(e.byte_gate, "RED");
        assert_eq!(e.disk_gate, "RED");
        // And with the quota lens off (the default), the same
        // absolute numbers alone fire nothing.
        let o = obs(1, 103.0, "GREEN", 1);
        let e = evaluate_gate(&o, None, "/staging", &GateConfig::default());
        assert_eq!(e.disk_gate, "GREEN");
    }

    #[test]
    fn gate_dying_volume_is_red() {
        // A volume at 5% free is RED by the volume lens regardless
        // of absolute numbers: 94 GB used would have read GREEN
        // under the old absolute byte gate.
        let gib = 1_073_741_824u64;
        let v = vol("/data", 100 * gib, 94 * gib, 5 * gib);
        let o = obs(1, 94.0, "GREEN", 10);
        let e = evaluate_gate(&o, Some(&v), "/data", &GateConfig::default());
        assert_eq!(e.byte_gate, "RED");
        assert_eq!(e.disk_gate, "RED");
        assert!(e.reason.contains("volume lens: 5% free"), "reason: {}", e.reason);
        assert!(e.action.contains("stage nothing under /data"), "action: {}", e.action);
        assert!(!e.action.contains("VM"), "action must not name the VM: {}", e.action);
    }

    #[test]
    fn gate_config_from_lookup() {
        // Absent keys are the defaults; written keys win.
        let empty = GateConfig::from_lookup(|_| None);
        assert_eq!(empty, GateConfig::default());
        let rec = "kind: qexo config\nquota_lens: on\nquota_red_gb: 120\ndisk_gate_free_red_pct: 5\nfile_red: 42\n";
        let cfg = GateConfig::from_lookup(|k| {
            let prefix = format!("{}:", k);
            rec.lines().find_map(|l| l.strip_prefix(&prefix)).map(|v| v.trim().to_string())
        });
        assert!(cfg.quota_on);
        assert_eq!(cfg.quota_red_gb, 120.0);
        assert_eq!(cfg.free_red_pct, 5.0);
        assert_eq!(cfg.file_red, 42);
        assert_eq!(cfg.quota_amber_gb, 95.0); // untouched default
        assert!(cfg.volume_on); // untouched default
    }

    #[test]
    fn forecast_unavailable_without_history() {
        let p = compute(&[], &obs(1, 71.0, "GREEN", 900_000), &[], (0.0, 0.0), &gin());
        assert!(!p.forecast.available);
        assert_eq!(p.disk_gate, "GREEN");
        let one = vec![obs(1, 88.0, "GREEN", 1_000_000)];
        let p = compute(&[], &obs(1, 88.0, "GREEN", 1_000_000), &one, (0.0, 0.0), &gin());
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
        let p = compute(&[task("1.1.1/1.1.1/1.1.1", "think")], &cur, &hist, (0.0, 0.0), &gin());
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
        // The RED case is the quota lens's (gin_quota): the
        // forecast must not de-escalate a gate a lens has fired.
        let hist = vec![obs(1, 70.0, "GREEN", 800_000), obs(2, 70.5, "GREEN", 801_000)];
        let p = compute(&[], &obs(2, 70.5, "GREEN", 801_000), &hist, (0.0, 0.0), &gin_quota());
        assert_eq!(p.disk_gate, "GREEN");
        let p = compute(&[], &obs(2, 103.0, "GREEN", 801_000), &hist, (0.0, 0.0), &gin_quota());
        assert_eq!(p.disk_gate, "RED");
        let hist = vec![obs(1, 80.0, "GREEN", 900_000), obs(2, 79.0, "GREEN", 890_000)];
        let p = compute(&[], &obs(2, 79.0, "GREEN", 890_000), &hist, (0.0, 0.0), &gin_quota());
        assert!(p.forecast.available);
        assert_eq!(p.forecast.nearest_days, None);
    }

    #[test]
    fn forecast_ranch_volume_projects_no_crossing() {
        // The fleet regression (2026-11-12 rollout): a ranch
        // volume at ~809 GB used (754 GiB of 3,586 GiB, ~74%
        // free) growing +0.06 GB/day. Under the legacy levels the
        // second plan flipped to "FORECAST AMBER — byte AMBER
        // (95 GB) projected in 0.0 days" because the volume had
        // long since passed 95 GB — a ceiling its gate no longer
        // reads. Under the node's own lenses there is no crossing
        // to project: the volume AMBER level sits ~2,900 GB away,
        // tens of thousands of days out at this rate — beyond the
        // horizon, so no crossing at all.
        let gib = 1_073_741_824u64;
        let v = vol("/", 3_586 * gib, 754 * gib, 2_700 * gib);
        let g = GateInputs { volume: Some(v), files_root: "/var/lib/qexo".to_string(), config: GateConfig::default() };
        let hist = vec![obs(1, 809.07, "GREEN", 6), obs(2, 809.13, "GREEN", 6)];
        let cur = obs(2, 809.13, "GREEN", 6);
        let p = compute(&[task("1.1.1/1.1.1/1.1.1", "think")], &cur, &hist, (0.0, 0.0), &g);
        assert!(p.forecast.available);
        assert_eq!(p.forecast.nearest_days, None, "nearest: {} in {:?}", p.forecast.nearest_label, p.forecast.nearest_days);
        assert_eq!(p.forecast.byte_amber_days, None);
        assert_eq!(p.disk_gate, "GREEN");
        assert!(!p.disk_gate_action.contains("FORECAST"), "action: {}", p.disk_gate_action);
        let out = render(&p);
        assert!(!out.contains("FORECAST AMBER"), "render:\n{}", out);
    }

    #[test]
    fn forecast_filling_volume_projects_volume_red() {
        // A 100 GiB volume at 12% free (gate AMBER by the volume
        // lens), used growing +5 GB/day: the RED level (10% free)
        // is a real, near crossing and the forecast names it as
        // the volume lens's. The AMBER level is already behind
        // the current reading, so it is the gate's verdict, not
        // a projection — it must not appear as a 0.0-day crossing.
        let gib = 1_073_741_824u64;
        let total = 100 * gib;
        let v = vol("/data", total, 88 * gib, 12 * gib);
        let g = GateInputs { volume: Some(v), files_root: "/data".to_string(), config: GateConfig::default() };
        let hist = vec![obs(1, 85.0, "AMBER", 10), obs(2, 90.0, "AMBER", 10)];
        let f = forecast_gate(&hist, &g.config, Some(total));
        assert!(f.available);
        assert_eq!(f.nearest_label, "volume RED (10% free)");
        let nd = f.nearest_days.unwrap();
        assert!(nd > 1.2 && nd < 1.5, "red crossing {} should be ~1.3 d", nd);
        // Via compute: the gate is already AMBER (the lens owns
        // the present); the forecast neither escalates it to RED
        // nor reports the stale AMBER crossing.
        let p = compute(&[], &obs(2, 90.0, "AMBER", 10), &hist, (0.0, 0.0), &g);
        assert_eq!(p.disk_gate, "AMBER");
        assert_eq!(p.forecast.nearest_label, "volume RED (10% free)");
    }

    #[test]
    fn forecast_quota_crossing_named_quota() {
        // Quota lens on (the VM's configuration), used growing
        // +10 GB/day toward the configured ceilings: the crossing
        // is named as the quota lens's, in the config's numbers —
        // the words "byte AMBER" and any hardcoded level are gone
        // from the forecast path.
        let cfg = gin_quota().config;
        let hist = vec![obs(1, 80.0, "GREEN", 10), obs(2, 90.0, "GREEN", 10)];
        let f = forecast_gate(&hist, &cfg, None);
        assert!(f.available);
        assert_eq!(f.nearest_label, "quota AMBER (95 GB)");
        let nd = f.nearest_days.unwrap();
        assert!(nd > 0.4 && nd < 0.6, "quota amber {} should be ~0.5 d", nd);
        assert_eq!(f.byte_amber_days.map(|d| (d - 0.5).abs() < 0.01), Some(true));
        // And with the quota lens off, the same series projects
        // no byte crossing at all — the ceiling does not exist
        // for a node that has not configured it.
        let f = forecast_gate(&hist, &GateConfig::default(), None);
        assert_eq!(f.byte_amber_days, None);
        assert_eq!(f.nearest_days, None);
    }

    #[test]
    fn forecast_median_damps_single_interval_whipsaw() {
        // The real disk-watch series, 2026-11-08 → 2026-11-12
        // (the 2026-11-13 nightly evaluation). Interval file
        // rates: +124,074, +59,021, +5,372, +7,064 per day. The
        // pre-v1.2.1 forecaster read only the last interval:
        // 7,064 files/day, file AMBER ~9.9 days out — after
        // reading ~1.4 days out on the 2026-11-10 interval and
        // ~14 days out on 2026-11-11's. The median of the four
        // intervals is 33,042.5 files/day: file AMBER ~2.1 days
        // out, a warning that reflects the series, not the last
        // day's mood.
        let d = |day: i64| days_from_civil(2026, 11, day);
        let hist = vec![
            obs(d(8), 71.01, "GREEN", 834_350),
            obs(d(9), 77.46, "GREEN", 958_424),
            obs(d(10), 81.45, "GREEN", 1_017_445),
            obs(d(11), 82.47, "GREEN", 1_022_817),
            obs(d(12), 84.16, "GREEN", 1_029_881),
        ];
        let f = forecast_gate(&hist, &GateConfig::default(), None);
        assert!(f.available);
        assert_eq!(f.intervals, 4);
        assert_eq!(f.span_days, 4.0);
        assert_eq!(f.files_per_day, 33043.0);
        assert_eq!(f.gb_per_day, 2.84);
        assert_eq!(f.nearest_label, "file AMBER (1,100,000)");
        let nd = f.nearest_days.unwrap();
        assert!(nd > 2.1 && nd < 2.14, "median crossing {} should be ~2.12 d", nd);
        // The contrast, stated in the same test: the last
        // interval alone still rates 7,064 files/day when it is
        // the only interval there is (two-observation history).
        let f1 = forecast_gate(&hist[3..], &GateConfig::default(), None);
        assert_eq!(f1.intervals, 1);
        assert_eq!(f1.files_per_day, 7064.0);
        let nd1 = f1.nearest_days.unwrap();
        assert!(nd1 > 9.8 && nd1 < 10.0, "single-interval crossing {} should be ~9.9 d", nd1);
    }

    #[test]
    fn forecast_window_excludes_old_spike() {
        // Nine observations: one +400,000-file interval, then
        // +1,000/day. The window keeps only the most recent
        // FORECAST_WINDOW_INTERVALS intervals, so the spike —
        // eight intervals back — is out of the rate entirely,
        // not merely damped by the median.
        let mut hist = vec![obs(1, 10.0, "GREEN", 100_000), obs(2, 10.0, "GREEN", 500_000)];
        for day in 3..=9_i64 {
            hist.push(obs(day, 10.0, "GREEN", 500_000 + (day as u64 - 2) * 1_000));
        }
        let f = forecast_gate(&hist, &GateConfig::default(), None);
        assert!(f.available);
        assert_eq!(f.intervals, FORECAST_WINDOW_INTERVALS);
        assert_eq!(f.files_per_day, 1000.0);
        assert_eq!(f.gb_per_day, 0.0);
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
            &gin(),
        );
        let out = render(&p);
        assert!(out.starts_with("qexo — placement plan\n"), "header missing");
        assert!(out.contains("[1.1.1/1.1.1/1.1.1] film encode"), "task line missing:\n{}", out);
        assert!(out.contains("disk gate: GREEN"), "gate line missing:\n{}", out);
        assert!(out.contains("900000 files under /staging"), "file line must name the counted root:\n{}", out);
    }
}
