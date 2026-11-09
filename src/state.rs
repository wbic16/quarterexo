//! Runtime state: the node's qexo state directory (~/.qexo or
//! $QEXO_HOME): persona index state, ownership/token band, grants and
//! policy, key directory records, delegations, receipts, replay
//! ledger, teleport + spawn state. Stored as JSON files, append-only
//! where the design says append-only (receipts, replay ledger,
//! delegation/revocation records, teleport ledger).

use crate::json::Json;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// RFC 3339 timestamp (UTC) for the current instant, plus offset form
/// kept simple: qexo timestamps are UTC with Z, named as such.
pub fn now_rfc3339() -> String {
    rfc3339(now_unix())
}

pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let tod = secs % 86400;
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yr, mo, d, h, m, s)
}

pub struct State {
    pub dir: PathBuf,
    pub node: String,
}

pub fn home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("QEXO_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".qexo")
}

pub fn node_name() -> String {
    if let Ok(n) = std::env::var("QEXO_NODE") {
        return n;
    }
    if let Ok(h) = fs::read_to_string("/etc/hostname") {
        let h = h.trim().to_string();
        if !h.is_empty() {
            return h;
        }
    }
    "unknown-node".to_string()
}

impl State {
    pub fn open() -> State {
        let dir = home_dir();
        let _ = fs::create_dir_all(&dir);
        State { dir, node: node_name() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn read_json(&self, name: &str) -> Option<Json> {
        let text = fs::read_to_string(self.path(name)).ok()?;
        crate::json::parse(&text).ok()
    }

    pub fn write_json(&self, name: &str, value: &Json) -> Result<(), String> {
        // Atomic temp+rename is a substrate capability, not an
        // assumption (docs/DESIGN-NOTES.md, 2026-11-09: the Muse VM
        // substrate denies fresh-temp-file writes with EPERM while
        // appends to existing files pass). Probe it; where the probe
        // fails, fall back to a direct in-place write and name the
        // fallback in the write-capability file.
        if atomic_rename_capable(self) {
            let tmp = self.path(&format!(".{}.tmp", name));
            match fs::write(&tmp, value.to_string())
                .and_then(|_| fs::rename(&tmp, self.path(name)))
            {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let _ = fs::remove_file(&tmp);
                    // fall through to the direct write
                }
            }
        }
        fs::write(self.path(name), value.to_string())
            .map_err(|e| format!("write {}: {}", name, e))
    }

    /// Append one JSON object per line to an append-only log.
    pub fn append_log(&self, name: &str, value: &Json) -> Result<(), String> {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(name))
            .map_err(|e| format!("open {}: {}", name, e))?;
        writeln!(f, "{}", value.to_string()).map_err(|e| e.to_string())
    }

    pub fn read_log(&self, name: &str) -> Vec<Json> {
        let text = fs::read_to_string(self.path(name)).unwrap_or_default();
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| crate::json::parse(l).ok())
            .collect()
    }

    // ---- request-id receipts (idempotency, section 8B) ------------------
    pub fn receipt_for(&self, request_id: &str) -> Option<Json> {
        self.read_log("receipts.jsonl")
            .into_iter()
            .find(|r| r.get("request_id").and_then(|v| v.as_str()) == Some(request_id))
            .and_then(|r| r.get("response").cloned())
    }

    pub fn store_receipt(&self, request_id: &str, response: &Json) {
        let _ = self.append_log(
            "receipts.jsonl",
            &Json::obj(vec![
                ("request_id", Json::s(request_id)),
                ("at", Json::s(&now_rfc3339())),
                ("response", response.clone()),
            ]),
        );
    }

    // ---- replay ledger (per-principal sequences, section 8B) -----------
    pub fn last_sequence(&self, principal: &str) -> u64 {
        self.read_log("replay.jsonl")
            .into_iter()
            .filter(|r| r.get("principal").and_then(|v| v.as_str()) == Some(principal))
            .filter_map(|r| r.get("sequence").and_then(|v| v.as_num()).map(|n| n as u64))
            .max()
            .unwrap_or(0)
    }

    pub fn record_sequence(&self, principal: &str, sequence: u64, request_id: &str) {
        let _ = self.append_log(
            "replay.jsonl",
            &Json::obj(vec![
                ("principal", Json::s(principal)),
                ("sequence", Json::n(sequence as f64)),
                ("request_id", Json::s(request_id)),
                ("at", Json::s(&now_rfc3339())),
            ]),
        );
    }

    // ---- counters -------------------------------------------------------
    pub fn next_counter(&self, name: &str) -> u64 {
        let cur = self
            .read_json("counters.json")
            .and_then(|j| j.get(name).and_then(|v| v.as_num()).map(|n| n as u64))
            .unwrap_or(0);
        let next = cur + 1;
        let mut obj = match self.read_json("counters.json") {
            Some(Json::Obj(p)) => p,
            _ => Vec::new(),
        };
        obj.retain(|(k, _)| k != name);
        obj.push((name.to_string(), Json::n(next as f64)));
        let _ = self.write_json("counters.json", &Json::Obj(obj));
        next
    }

    pub fn request_id(&self) -> String {
        format!("{}-{}-{}", self.node, now_ms(), self.next_counter("req"))
    }
}

/// The grants catalogue (section 8A.3): name -> governs. All default ON.
pub const GRANTS: &[(&str, &str)] = &[
    ("sync.push", "pushing state bundles to other nodes"),
    ("sync.restore", "restoring state from a bundle onto a node"),
    ("persona.swap", "hot swaps (section 2.3)"),
    ("persona.teleport", "teleport issue/accept (section 9)"),
    ("persona.seed", "creating persona phexts and index entries"),
    ("route.change", "editing qexo-routing.toml"),
    ("route.ask", "making inference requests through the router"),
    ("sq.manage", "SQ lifecycle under qexo's keeping"),
    ("voice.speak", "submitting voice turns"),
    ("voice.floor", "holding the voice floor"),
    ("meta.read", "metadata queries"),
    ("install.apply", "installing/upgrading tooling"),
    ("wake.send", "issuing wake requests"),
    ("mail.send", "dropping mailbox messages (/chat)"),
    ("keys.issue", "generating node/persona/agent keys"),
    ("keys.delegate", "issuing delegations"),
    ("grants.change", "changing grants or key policy — the meta-grant"),
    ("persona.spawn", "spawning a task instance of a spawn-eligible record"),
    ("persona.meld", "entering a mind meld — capability only; consent is separate and undelegatable"),
];

/// The TAINTED grant set (section 8A.6): what an externally-triggered
/// turn may do. Everything else is refused as tainted-scope.
pub const TAINTED_ALLOWED: &[&str] = &["meta.read", "route.ask"];

pub fn grant_known(name: &str) -> bool {
    GRANTS.iter().any(|(g, _)| *g == name)
}

/// Effective grant state: fleet toggles in grants.json override the
/// permissive default; per-persona toggles are applied by the caller.
pub fn grant_enabled(state: &State, grant: &str) -> bool {
    match state.read_json("grants.json") {
        Some(j) => j.get(grant).and_then(|v| v.as_bool()).unwrap_or(true),
        None => true,
    }
}

/// Self-modifying classes key-bound from bootstrap (section 8A.0).
pub fn is_bootstrap_class(action: &str) -> bool {
    action == "grants.change"
        || action.starts_with("keys.")
        || action == "route.change"
        || action == "persona.seed"
        || action == "persona.manifest"
}

/// The sensitive set with a default policy (section 8A.0/8A.1).
pub fn is_sensitive(action: &str) -> bool {
    matches!(
        action,
        "grants.change"
            | "keys.issue"
            | "keys.delegate"
            | "install.apply"
            | "sync.restore"
            | "persona.teleport"
    ) || is_bootstrap_class(action)
}

/// Minimal TOML reader for flat `key = "value"` / `key = true` config
/// (routing + bake config family; config is data, shallow by design).
pub fn read_toml_flat(path: &std::path::Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return out,
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let mut val = line[eq + 1..].trim().to_string();
            if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                val = val[1..val.len() - 1].to_string();
            }
            let full = if section.is_empty() { key } else { format!("{}.{}", section, key) };
            out.insert(full, val);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1762459200), "2025-11-06T12:00:00Z");
    }

    #[test]
    fn grants_catalogue_is_19() {
        assert_eq!(GRANTS.len(), 19);
        assert!(grant_known("persona.meld"));
        assert!(!grant_known("persona.fly"));
    }
}


/// Probe: can this directory do create-temp + write + rename?
/// The probe result is written (by direct write, which every
/// substrate here permits) to write-capability.txt in the state
/// dir, so the capability — and any fallback it forces — is a named
/// fact in the record, never a silent behavior change.
pub fn atomic_rename_capable(state: &State) -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        let probe_tmp = state.path(".capability-probe.tmp");
        let probe_dst = state.path(".capability-probe.dst");
        let ok = fs::write(&probe_tmp, b"probe")
            .and_then(|_| fs::rename(&probe_tmp, &probe_dst))
            .is_ok();
        let _ = fs::remove_file(&probe_tmp);
        let _ = fs::remove_file(&probe_dst);
        let note = if ok {
            "atomic-rename: capable (temp+rename verified at runtime)\n"
        } else {
            "atomic-rename: NOT capable — append/direct write patterns in force (docs/DESIGN-NOTES.md 2026-11-09)\n"
        };
        let _ = fs::write(state.path("write-capability.txt"), note);
        ok
    })
}
