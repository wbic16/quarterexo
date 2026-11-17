//! doctor — the node's health, checked and (with --fix) repaired.
//!
//! Two layers. The FILESYSTEM facts: the state directory exists
//! and is writable, the state file exists and parses as phext,
//! the keyring directory (gnupg) is private (0700, on unix — modes
//! do not exist elsewhere and the finding says so), no stale
//! state lock is held by a dead process. The LATTICE facts (Layer 2):
//! the home index and every homeland index exist, and every
//! record (peer, persona, session) is named by its homeland's
//! index — records are truth, indexes are derived prose, so a
//! missing or stale index line is exactly the kind of damage
//! --fix repairs, by regenerating the indexes from the records.
//!
//! doctor never invents state: a missing state file is reported
//! as "not born — run qexo config", not silently created here.
//! (--fix creates directories and repairs permissions/locks; the
//! birth of state.phext itself is config's act.)

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub item: String,
    pub ok: bool,
    pub detail: String,
    pub fixable: bool,
}

pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.findings.iter().all(|f| f.ok)
    }
    pub fn problems(&self) -> usize {
        self.findings.iter().filter(|f| !f.ok).count()
    }
    /// One-line headline for `status`: "doctor ok" or
    /// "doctor: N problem(s)".
    pub fn headline(&self) -> String {
        if self.ok() {
            "doctor ok".to_string()
        } else {
            format!("doctor: {} problem(s)", self.problems())
        }
    }
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("qexo doctor\n");
        for f in &self.findings {
            out.push_str(&format!(
                "  [{}] {} — {}{}\n",
                if f.ok { "ok" } else { "!!" },
                f.item,
                f.detail,
                if !f.ok && f.fixable { " (fixable: qexo doctor --fix)" } else { "" }
            ));
        }
        out
    }
}

fn mode_of(p: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).ok().map(|m| m.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        None
    }
}

fn set_mode(p: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}: {}", p.display(), e))
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
        Ok(())
    }
}

/// The state lock file, if qexo keeps one: <state>.lock holding a
/// pid as text. A lock whose pid has no live process is stale.
///
/// Liveness is probed per platform, and the conservative
/// direction rules: a pid whose liveness cannot be judged is
/// treated as ALIVE (never stale) — doctor must never delete a
/// lock it cannot judge, because the lock it deletes might be
/// the one a running qexo is holding.
fn stale_lock(state_file: &Path) -> Option<PathBuf> {
    let lock = state_file.with_extension("lock");
    let pid_text = std::fs::read_to_string(&lock).ok()?;
    let pid: u32 = pid_text.trim().parse().ok()?;
    if pid_alive(pid) {
        None
    } else {
        Some(lock)
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    windows_pid_alive(pid)
}

#[cfg(not(any(unix, windows)))]
fn pid_alive(pid: u32) -> bool {
    let _ = pid;
    true // no probe on this platform: unjudgeable means alive
}

/// Windows liveness probe, std-only via kernel32: open the
/// process with query-limited rights and ask its exit code.
/// STILL_ACTIVE (259) means alive; a real exit code means the
/// process is gone. A null handle means "not alive" ONLY when
/// the error is ERROR_INVALID_PARAMETER (no such process); any
/// other failure (e.g. access denied on a protected process) is
/// ambiguity, and ambiguity reads as alive.
#[cfg(windows)]
fn windows_pid_alive(pid: u32) -> bool {
    use std::ffi::c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    const ERROR_INVALID_PARAMETER: u32 = 87;

    extern "system" {
        fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> *mut c_void;
        fn GetExitCodeProcess(h_process: *mut c_void, lp_exit_code: *mut u32) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        if ok == 0 {
            return true; // could not ask: ambiguity reads as alive
        }
        code == STILL_ACTIVE
    }
}

pub fn check(state_dir: &Path, state_file: &Path) -> Report {
    let mut findings = Vec::new();

    // state directory
    if state_dir.is_dir() {
        let writable = std::fs::metadata(state_dir).map(|m| !m.permissions().readonly()).unwrap_or(false);
        findings.push(Finding {
            item: "state directory".into(),
            ok: writable,
            detail: format!("{} {}", state_dir.display(), if writable { "writable" } else { "NOT writable" }),
            fixable: false,
        });
    } else {
        findings.push(Finding {
            item: "state directory".into(),
            ok: false,
            detail: format!("{} missing", state_dir.display()),
            fixable: true,
        });
    }

    // state file: exists + parses (parse = Phext::parse does not
    // fail; a state file is "parseable" iff it is valid UTF-8 text
    // we could read — read_to_string is the check).
    if state_file.exists() {
        match std::fs::read_to_string(state_file) {
            Ok(text) => {
                let px = crate::phext::Phext::parse(&text);
                findings.push(Finding {
                    item: "state file".into(),
                    ok: true,
                    detail: format!("{} ({} scrolls)", state_file.display(), px.scrolls.len()),
                    fixable: false,
                });
            }
            Err(e) => findings.push(Finding {
                item: "state file".into(),
                ok: false,
                detail: format!("{} unreadable: {}", state_file.display(), e),
                fixable: false,
            }),
        }
    } else {
        findings.push(Finding {
            item: "state file".into(),
            ok: false,
            detail: format!("{} not born — run qexo config", state_file.display()),
            fixable: false,
        });
    }

    // keyring privacy
    let gnupg = state_dir.join("gnupg");
    if gnupg.is_dir() {
        #[cfg(unix)]
        {
            let mode = mode_of(&gnupg).unwrap_or(0);
            findings.push(Finding {
                item: "gnupg keyring".into(),
                ok: mode == 0o700,
                detail: format!("{} mode {:o}", gnupg.display(), mode),
                fixable: mode != 0o700,
            });
        }
        #[cfg(not(unix))]
        findings.push(Finding {
            item: "gnupg keyring".into(),
            ok: true,
            detail: format!(
                "{} present — permission modes are a unix concept, not applicable on this platform",
                gnupg.display()
            ),
            fixable: false,
        });
    } else {
        findings.push(Finding {
            item: "gnupg keyring".into(),
            ok: false,
            detail: format!("{} missing (created by qexo config)", gnupg.display()),
            fixable: false,
        });
    }

    // stale lock
    match stale_lock(state_file) {
        Some(lock) => findings.push(Finding {
            item: "state lock".into(),
            ok: false,
            detail: format!("{} held by a dead pid", lock.display()),
            fixable: true,
        }),
        None => findings.push(Finding {
            item: "state lock".into(),
            ok: true,
            detail: "no stale lock".into(),
            fixable: false,
        }),
    }

    Report { findings }
}

/// Apply the filesystem repairs: create the state directory,
/// tighten the keyring mode, remove a stale lock. Returns the
/// human list of what was done. Never creates the state file.
pub fn fix(state_dir: &Path, state_file: &Path) -> Vec<String> {
    let mut done = Vec::new();
    if !state_dir.is_dir() {
        if std::fs::create_dir_all(state_dir).is_ok() {
            done.push(format!("created state directory {}", state_dir.display()));
        }
    }
    let gnupg = state_dir.join("gnupg");
    if gnupg.is_dir() {
        // Only where modes exist (unix) can there be anything to
        // repair; elsewhere mode_of is None — nothing to do, and
        // nothing to claim was done.
        if let Some(mode) = mode_of(&gnupg) {
            if mode != 0o700 && set_mode(&gnupg, 0o700).is_ok() {
                done.push(format!("set {} to mode 700", gnupg.display()));
            }
        }
    }
    if let Some(lock) = stale_lock(state_file) {
        if std::fs::remove_file(&lock).is_ok() {
            done.push(format!("removed stale lock {}", lock.display()));
        }
    }
    done
}

// ----------------------------------------------------------------
// The lattice half: neighborhood (index) checks and repair.
// ----------------------------------------------------------------

use crate::lattice::{self, Lattice};

/// Names listed by a family index scroll's record lines
/// ("- <name> — <coord> — ...", or "- hosted: <name> — ...").
fn index_names(index_text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in index_text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- ") {
            let rest = rest.strip_prefix("hosted: ").unwrap_or(rest);
            if let Some((name, _)) = rest.split_once(" — ") {
                out.push(name.trim().to_string());
            }
        }
    }
    out
}

/// Check the lattice layer of an EXISTING state document: home
/// index and homeland indexes exist; every record is named by
/// its homeland's index; no index line names a ghost.
pub fn check_lattice(lat: &Lattice) -> Report {
    let mut findings = Vec::new();
    let px = &lat.px;

    if px.get(&lattice::HOME).is_some() {
        findings.push(Finding {
            item: "home index".into(),
            ok: true,
            detail: format!("{} is the map", crate::phext::fmt_coord(&lattice::HOME)),
            fixable: false,
        });
    } else {
        findings.push(Finding {
            item: "home index".into(),
            ok: false,
            detail: format!("{} missing — the state has no map", crate::phext::fmt_coord(&lattice::HOME)),
            fixable: true,
        });
    }

    for (h, label) in [
        (lattice::HL_KEYS, "keys & trust index"),
        (lattice::HL_PEERS, "peers index"),
        (lattice::HL_PERSONAS, "personas index"),
        (lattice::HL_PLAN, "plan index"),
        (lattice::HL_LEDGERS, "ledgers index"),
        (lattice::HL_MAIL, "mailbox & sessions index"),
        (lattice::HL_TELEMETRY, "telemetry index"),
        (lattice::HL_CONFIG, "config & wiring index"),
        (lattice::HL_LEARN, "lessons & learning index"),
        (lattice::HL_DISPATCH, "dispatch & queue index"),
    ] {
        let addr = lattice::index_addr(h);
        findings.push(Finding {
            item: label.into(),
            ok: px.get(&addr).is_some(),
            detail: if px.get(&addr).is_some() {
                format!("{} present", crate::phext::fmt_coord(&addr))
            } else {
                format!("{} missing", crate::phext::fmt_coord(&addr))
            },
            fixable: px.get(&addr).is_none(),
        });
    }

    // records vs indexes: a record its index does not name is an
    // orphan — present in the landscape, absent from the map.
    let recs = lattice::records(px);
    for (kind, h) in [("peer", lattice::HL_PEERS), ("persona", lattice::HL_PERSONAS), ("session", lattice::HL_MAIL)] {
        let idx_text = px.get(&lattice::index_addr(h)).cloned().unwrap_or_default();
        let names = index_names(&idx_text);
        for r in recs.iter().filter(|r| r.kind == kind) {
            let named = if kind == "session" {
                r.status != "open" || names.iter().any(|n| n == &r.name)
            } else {
                names.iter().any(|n| n == &r.name)
            };
            findings.push(Finding {
                item: format!("{} index line", kind),
                ok: named,
                detail: if named {
                    format!("{} at {} is on the map", r.name, crate::phext::fmt_coord(&r.coord))
                } else {
                    format!("{} at {} has NO index line (orphan record)", r.name, crate::phext::fmt_coord(&r.coord))
                },
                fixable: !named,
            });
        }
        // ghosts: index lines naming no living record.
        for n in &names {
            let alive = recs.iter().any(|r| r.kind == kind && &r.name == n && (kind != "session" || r.status == "open"));
            if !alive {
                findings.push(Finding {
                    item: format!("{} index line", kind),
                    ok: false,
                    detail: format!("index names {}, but no {} record answers to it (ghost line)", n, kind),
                    fixable: true,
                });
            }
        }
    }

    Report { findings }
}

/// Repair the lattice layer: regenerate every index scroll from
/// the records. Returns the human list of what changed.
pub fn fix_lattice(lat: &mut Lattice) -> Vec<String> {
    let watched: Vec<(crate::phext::Coord, &str)> = vec![
        (lattice::HOME, "home index"),
        (lattice::index_addr(lattice::HL_KEYS), "keys & trust index"),
        (lattice::index_addr(lattice::HL_PEERS), "peers index"),
        (lattice::index_addr(lattice::HL_PERSONAS), "personas index"),
        (lattice::index_addr(lattice::HL_PLAN), "plan index"),
        (lattice::index_addr(lattice::HL_LEDGERS), "ledgers index"),
        (lattice::index_addr(lattice::HL_MAIL), "mailbox & sessions index"),
        (lattice::index_addr(lattice::HL_TELEMETRY), "telemetry index"),
        (lattice::index_addr(lattice::HL_CONFIG), "config & wiring index"),
        (lattice::index_addr(lattice::HL_LEARN), "lessons & learning index"),
        (lattice::index_addr(lattice::HL_DISPATCH), "dispatch & queue index"),
    ];
    let before: Vec<Option<String>> = watched.iter().map(|(a, _)| lat.px.get(a).cloned()).collect();
    lattice::regen_indexes(&mut lat.px);
    let mut done = Vec::new();
    for (i, (addr, label)) in watched.iter().enumerate() {
        if lat.px.get(addr).cloned() != before[i] {
            done.push(format!("rebuilt {} at {}", label, crate::phext::fmt_coord(addr)));
        }
    }
    done
}

// ----------------------------------------------------------------
// The queue reconciliation (dispatch core slice 2, spec §4.5):
// calculated from the records on every run, never stored.
// ----------------------------------------------------------------

use crate::dispatch::{self, ItemState};

/// Doctor's queue section. The §4.5 counts: items by state;
/// terminal items lacking a receipt (must be 0); receipts
/// citing no live item (must be 0); assignments whose
/// heartbeat is stale (counted here — the F5 path that acts
/// on them arrives in slice 5); an audit record for every
/// non-ping closure (a closure without one is a finding,
/// §7.2). The §4.6 illegal combinations are checked one by
/// one, each in its own words. No queue finding is fixable
/// by doctor: the repair is a transition or Will's hand,
/// never an invention — a nonzero must-be-0 count is a
/// doctor failure of the same class as a missing homeland
/// index (§4.5).
pub fn check_queue(lat: &Lattice) -> Report {
    let px = &lat.px;
    let mut findings = Vec::new();
    let items = dispatch::load_items(px);
    let receipts = dispatch::load_transition_receipts(px);
    let audits = dispatch::load_audits(px);
    let assignments = dispatch::load_assignments(px);

    // Items by state — calculated, never stored.
    let mut counts: Vec<String> = Vec::new();
    for s in dispatch::ALL_STATES {
        let n = items.iter().filter(|i| i.state == Some(s)).count();
        if n > 0 {
            counts.push(format!("{} {}", n, s.name()));
        }
    }
    let unknown = items.iter().filter(|i| i.state.is_none()).count();
    if unknown > 0 {
        counts.push(format!("{} in an unknown state", unknown));
    }
    findings.push(Finding {
        item: "queue reconciliation".into(),
        ok: true,
        detail: if items.is_empty() {
            "queue: 0 item(s)".to_string()
        } else {
            format!("queue: {} item(s) — {}", items.len(), counts.join(", "))
        },
        fixable: false,
    });

    let item_addrs: std::collections::HashSet<String> =
        items.iter().map(|i| crate::phext::fmt_coord(&i.addr)).collect();
    let local_node = px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .unwrap_or_default();
    let local_actor = dispatch::local_actor(px);

    for item in &items {
        let addr_s = crate::phext::fmt_coord(&item.addr);
        let bad = |detail: String| Finding { item: "queue integrity".into(), ok: false, detail, fixable: false };
        if item.state.is_none() {
            findings.push(bad(format!(
                "{}: state '{}' is not a queue state (§4.6)",
                addr_s, item.state_raw
            )));
            continue;
        }
        let state = item.state.unwrap();
        if !item.attempts_raw.is_empty() && item.attempts_raw.parse::<u32>().is_err() {
            findings.push(bad(format!("{}: attempts: '{}' is not a count", addr_s, item.attempts_raw)));
        }
        if !item.failures_raw.is_empty() && item.failures_raw.parse::<u32>().is_err() {
            findings.push(bad(format!("{}: failures: '{}' is not a count", addr_s, item.failures_raw)));
        }
        // Illegal combination 5: waiting with no register cite.
        if state == ItemState::Waiting
            && item.register.as_deref().map(|r| r.trim().is_empty()).unwrap_or(true)
        {
            findings.push(bad(format!(
                "{}: waiting with no register number cited (illegal combination 5, §4.6)",
                addr_s
            )));
        }
        // Illegal combination 3: the auto-pause counter rules.
        if state == ItemState::PausedAuto && item.failures < 3 {
            findings.push(bad(format!(
                "{}: paused-auto with failures: {} < 3 (illegal combination 3, §4.6)",
                addr_s, item.failures
            )));
        }
        if state == ItemState::Queued && item.failures >= 3 {
            findings.push(bad(format!(
                "{}: queued with failures: {} ≥ 3 and no intervening verified progress or manual resume (illegal combination 3, §4.6)",
                addr_s, item.failures
            )));
        }
        // Illegal combination 7: the cited attempt universe
        // does not exist. Attempts live in this node's chain
        // for items homed here; an item whose `home:` names
        // another node keeps its attempts there.
        if item.attempts >= 1 {
            let home_here = item.home.as_deref().map(|h| h == local_node).unwrap_or(true);
            if home_here && lat.universe_count() < (item.attempts as usize) + 1 {
                findings.push(bad(format!(
                    "{}: cites attempt universe ${} but the state chain holds {} universe(s) (illegal combination 7, §4.6)",
                    addr_s,
                    item.attempts + 1,
                    lat.universe_count()
                )));
            }
        }
        // Illegal combination 6: the bundle hash. The shape
        // is checkable today; a divergent attempt-universe
        // copy is checkable today; recomputation against
        // the canonical bundle lands with the bundle
        // builder in slice 4.
        if let Some(h) = &item.bundle_sha256 {
            if !dispatch::is_sha256_hex(h) {
                findings.push(bad(format!(
                    "{}: bundle-sha256 '{}' is not a SHA-256 digest (illegal combination 6, §4.6)",
                    addr_s, h
                )));
            }
            for n in 2..=(item.attempts as usize) + 1 {
                if let Some(doc) = lat.universe(n) {
                    if let Some(copy) = doc.get(&item.addr) {
                        if let Some(h2) = lattice::field(copy, "bundle-sha256") {
                            if h2 != *h {
                                findings.push(bad(format!(
                                    "{}: attempt universe ${}'s bundle-sha256 does not match the item record's (illegal combination 6, §4.6)",
                                    addr_s, n
                                )));
                            }
                        }
                    }
                }
            }
        }
        // Terminal items: combination 1 (no receipt, or a
        // receipt whose evidence fields are empty),
        // combination 4 (cancelled with no note, or by an
        // executor), and §7.2's twin (a done closure with
        // no audit record).
        if state.is_terminal() {
            let term = receipts
                .iter()
                .filter(|r| r.item.as_deref() == Some(addr_s.as_str()) && r.to.as_deref() == Some(state.name()))
                .last();
            match term {
                None => findings.push(bad(format!(
                    "{}: terminal ({}) with no receipt (illegal combination 1, §4.6)",
                    addr_s,
                    state.name()
                ))),
                Some(r) => {
                    let ev_empty = r.evidence.as_deref().map(|e| e.trim().is_empty()).unwrap_or(true);
                    if ev_empty {
                        if state == ItemState::Cancelled {
                            findings.push(bad(format!(
                                "{}: cancelled with no note — receipt {} carries no evidence (illegal combination 4, §4.6)",
                                addr_s,
                                crate::phext::fmt_coord(&r.addr)
                            )));
                        } else {
                            findings.push(bad(format!(
                                "{}: terminal receipt {} has empty evidence fields (illegal combination 1, §4.6)",
                                addr_s,
                                crate::phext::fmt_coord(&r.addr)
                            )));
                        }
                    }
                    if state == ItemState::Cancelled {
                        let actor = r.actor.clone().unwrap_or_default();
                        let actor_node = actor.split_whitespace().next().unwrap_or("");
                        let legit = actor == "will"
                            || actor == "emise"
                            || actor == local_actor
                            || (!local_node.is_empty() && actor_node == local_node);
                        if !legit {
                            findings.push(bad(format!(
                                "{}: cancelled by '{}' — not Will/Emise or this node (illegal combination 4, §4.6)",
                                addr_s, actor
                            )));
                        }
                    }
                }
            }
            if state == ItemState::Done {
                let has_audit = audits
                    .iter()
                    .any(|a| a.kind == "non-ping-close" && a.item.as_deref() == Some(addr_s.as_str()));
                if !has_audit {
                    findings.push(bad(format!(
                        "{}: done with no audit record — a non-ping closure without one is a finding (§4.5, §7.2)",
                        addr_s
                    )));
                }
            }
        }
        // Illegal combination 2: an item running under two
        // live assignments (the single-assignment invariant).
        if state == ItemState::Running {
            let live = assignments
                .iter()
                .filter(|a| a.live && a.item.as_deref() == Some(addr_s.as_str()))
                .count();
            if live >= 2 {
                findings.push(bad(format!(
                    "{}: running under {} live assignments — exactly one attempt universe is authoritative (illegal combination 2, §4.6)",
                    addr_s, live
                )));
            }
        }
    }

    // §4.5: receipts citing no live item (must be 0).
    for r in &receipts {
        if let Some(it) = &r.item {
            if !item_addrs.contains(it) {
                findings.push(Finding {
                    item: "queue integrity".into(),
                    ok: false,
                    detail: format!(
                        "receipt {} cites {}, which is not a live queue item (§4.5)",
                        crate::phext::fmt_coord(&r.addr),
                        it
                    ),
                    fixable: false,
                });
            }
        }
    }

    // Assignments: counted, with stale heartbeats named as
    // a count — the F5 path that acts on them is slice 5.
    let live = assignments.iter().filter(|a| a.live).count();
    let now = crate::core::now_minutes();
    let stale = assignments
        .iter()
        .filter(|a| {
            a.live
                && a.heartbeat
                    .as_deref()
                    .and_then(crate::core::stamp_minutes)
                    .map(|m| now - m > 6)
                    .unwrap_or(false)
        })
        .count();
    findings.push(Finding {
        item: "queue assignments".into(),
        ok: true,
        detail: format!(
            "{} assignment(s), {} live, {} with a stale heartbeat (3 missed 2-minute beats, §15 Q5) — the F5 path that acts on these arrives in slice 5",
            assignments.len(),
            live,
            stale
        ),
        fixable: false,
    });

    Report { findings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_repairs_dir_perms_and_stale_lock() {
        let dir = std::env::temp_dir().join(format!("qexo-doctor-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state_file = dir.join("state.phext");
        // damage: no state dir at all -> fix creates it
        // (platform-neutral: runs everywhere)
        let before = check(&dir, &state_file);
        assert!(!before.ok());
        let done = fix(&dir, &state_file);
        assert!(done.iter().any(|d| d.contains("created state directory")), "{:?}", done);
        // keyring present. On unix it is seeded with wrong perms
        // (0o755) and must be repaired to 0o700; elsewhere
        // permission modes do not exist, and the finding must be
        // present saying exactly that — never silently absent.
        let gnupg = dir.join("gnupg");
        std::fs::create_dir_all(&gnupg).unwrap();
        #[cfg(unix)]
        set_mode(&gnupg, 0o755).unwrap();
        // stale lock: pid 999999 cannot exist on any platform
        let lock = dir.join("state.lock");
        std::fs::write(&lock, "999999").unwrap();
        let mid = check(&dir, &state_file);
        #[cfg(unix)]
        assert!(mid.findings.iter().any(|f| f.item == "gnupg keyring" && !f.ok && f.fixable));
        #[cfg(not(unix))]
        assert!(
            mid.findings.iter().any(|f| f.item == "gnupg keyring" && f.ok && f.detail.contains("not applicable")),
            "non-unix keyring finding must be present as ok/not-applicable: {:?}",
            mid.findings.iter().map(|f| &f.detail).collect::<Vec<_>>()
        );
        assert!(mid.findings.iter().any(|f| f.item == "state lock" && !f.ok && f.fixable));
        let done = fix(&dir, &state_file);
        #[cfg(unix)]
        assert_eq!(mode_of(&gnupg), Some(0o700));
        assert!(!lock.exists());
        assert!(done.iter().any(|d| d.contains("removed stale lock")), "{:?}", done);
        #[cfg(unix)]
        assert!(done.len() >= 2, "{:?}", done);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lattice_check_finds_orphan_and_fix_restores_the_line() {
        use crate::lattice::{self, Lattice};
        use crate::phext::Phext;
        let dir = std::env::temp_dir().join(format!("qexo-doctor-lattice-{}", std::process::id()));
        let path = dir.join("state.phext");
        let mut px = Phext::new();
        // a peer record, indexed
        px.insert(
            lattice::at(lattice::HL_PEERS, [2, 1, 1, 1, 1, 1]),
            lattice::record(&[("kind", "peer"), ("name", "peer-ABCD"), ("fingerprint", "FF"), ("status", "registered")]),
        );
        lattice::regen_indexes(&mut px);
        let mut lat = Lattice::born(path);
        lat.px = px;
        assert!(check_lattice(&lat).ok(), "freshly indexed state must be healthy");
        // damage: strip the peer's line from the peers index
        let idx_addr = lattice::index_addr(lattice::HL_PEERS);
        let damaged: String = lat
            .px
            .get(&idx_addr)
            .unwrap()
            .lines()
            .filter(|l| !l.contains("peer-ABCD"))
            .collect::<Vec<_>>()
            .join("\n");
        lat.px.insert(idx_addr, damaged);
        let report = check_lattice(&lat);
        assert!(!report.ok());
        assert!(
            report.findings.iter().any(|f| !f.ok && f.detail.contains("peer-ABCD")),
            "{:?}",
            report.findings.iter().map(|f| &f.detail).collect::<Vec<_>>()
        );
        let done = fix_lattice(&mut lat);
        assert!(done.iter().any(|d| d.contains("peers index")), "{:?}", done);
        assert!(check_lattice(&lat).ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --------------------------------------------------------
    // Slice 2: the queue reconciliation (§4.5) and the
    // illegal combinations (§4.6), one fixture per rule.
    // --------------------------------------------------------

    fn queue_lat(px: crate::phext::Phext) -> Lattice {
        let dir = std::env::temp_dir().join(format!("qexo-doctor-queue-{}", std::process::id()));
        let mut lat = Lattice::born(dir.join("state.phext"));
        lat.px = px;
        lat
    }

    fn queue_px() -> crate::phext::Phext {
        let mut px = crate::phext::Phext::new();
        px.insert(
            lattice::IDENTITY,
            lattice::record(&[("name", "Test Node"), ("node", "test-node"), ("born", "now")]),
        );
        px.insert(
            lattice::NODE_KEY_RECORD,
            lattice::record(&[("kind", "node key"), ("fingerprint", "TESTFP"), ("status", "generated")]),
        );
        px
    }

    fn item_text(state: &str, failures: u32, extra: &[(&str, &str)]) -> String {
        let mut t = crate::dispatch::item_record(
            "T",
            None,
            None,
            "emise",
            "2026-11-16 07:52 UTC",
            "test-node TESTFP",
            "standing",
            Some(5),
            Some(1),
            None,
            None,
        );
        t = lattice::set_field(&t, "state", state);
        t = lattice::set_field(&t, "failures", &failures.to_string());
        for (k, v) in extra {
            t = lattice::set_field(&t, k, v);
        }
        t
    }

    fn transition_receipt(px: &mut crate::phext::Phext, item: &str, from: &str, to: &str, actor: &str, evidence: &str) {
        crate::core::append_receipt(
            px,
            "transition",
            &[("item", item), ("from", from), ("to", to), ("actor", actor), ("evidence", evidence)],
        );
    }

    fn flagged(report: &Report, needle: &str) -> bool {
        report.findings.iter().any(|f| !f.ok && f.detail.contains(needle))
    }

    #[test]
    fn queue_reconciliation_accepts_a_healthy_queue() {
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("queued", 0, &[]));
        px.insert(crate::dispatch::item_addr(2), item_text("paused", 1, &[]));
        transition_receipt(&mut px, "11.11.11/2.1.2/1.1.1", "queued", "paused", "test-node TESTFP", "later");
        let report = check_queue(&queue_lat(px));
        assert!(report.ok(), "{:?}", report.findings.iter().map(|f| &f.detail).collect::<Vec<_>>());
    }

    #[test]
    fn queue_reconciliation_flags_each_illegal_combination() {
        // 1 — terminal with no receipt; and done with a
        // receipt but no audit (the §7.2 twin).
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("done", 0, &[]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 1"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("done", 0, &[]));
        transition_receipt(&mut px, "11.11.11/2.1.1/1.1.1", "running", "done", "test-node TESTFP", "checked: x");
        let report = check_queue(&queue_lat(px.clone()));
        assert!(!flagged(&report, "illegal combination 1"), "receipted done must pass combination 1");
        assert!(flagged(&report, "no audit record"));
        crate::dispatch::append_audit(&mut px, "non-ping-close", "11.11.11/2.1.1/1.1.1", "c", "s", "v", "test-node TESTFP");
        assert!(check_queue(&queue_lat(px)).ok());

        // 2 — running under two live assignments.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("running", 0, &[]));
        for b in [1, 2] {
            px.insert(
                lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, b, 1, 1, 1]),
                lattice::record(&[("item", "11.11.11/2.1.1/1.1.1"), ("attempt", "1"), ("heartbeat", "2026-11-16 07:52 UTC")]),
            );
        }
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 2"));

        // 3 — paused-auto below the failure cap; queued at it.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("paused-auto", 1, &[]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 3"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("queued", 3, &[]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 3"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("paused-auto", 3, &[]));
        assert!(!flagged(&check_queue(&queue_lat(px)), "illegal combination 3"));

        // 4 — cancelled by another node (an executor), and
        // cancelled with no note.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("cancelled", 0, &[]));
        transition_receipt(&mut px, "11.11.11/2.1.1/1.1.1", "queued", "cancelled", "other-node ABCD", "moot");
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 4"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("cancelled", 0, &[]));
        transition_receipt(&mut px, "11.11.11/2.1.1/1.1.1", "queued", "cancelled", "test-node TESTFP", "");
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 4"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("cancelled", 0, &[]));
        transition_receipt(&mut px, "11.11.11/2.1.1/1.1.1", "queued", "cancelled", "test-node TESTFP", "moot");
        assert!(check_queue(&queue_lat(px)).ok());

        // 5 — waiting with no register cite.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("waiting", 0, &[]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 5"));
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("waiting", 0, &[("register", "181")]));
        assert!(check_queue(&queue_lat(px)).ok());

        // 6 — a bundle-sha256 that is not a digest.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("queued", 0, &[("bundle-sha256", "xyz")]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 6"));

        // 7 — attempts cited, attempt universe absent.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("queued", 0, &[("attempts", "2")]));
        assert!(flagged(&check_queue(&queue_lat(px)), "illegal combination 7"));

        // §4.5 — a receipt citing no live item.
        let mut px = queue_px();
        transition_receipt(&mut px, "11.11.11/2.1.99/1.1.1", "queued", "paused", "test-node TESTFP", "later");
        assert!(flagged(&check_queue(&queue_lat(px)), "not a live queue item"));

        // An unknown state is a finding in its own right.
        let mut px = queue_px();
        px.insert(crate::dispatch::item_addr(1), item_text("sideways", 0, &[]));
        assert!(flagged(&check_queue(&queue_lat(px)), "is not a queue state"));
    }

    #[test]
    fn queue_reconciliation_honors_an_existing_attempt_universe() {
        // Item with attempts: 1 whose attempt universe ($2)
        // exists in the chain: no combination-7 finding.
        let mut u1 = queue_px();
        u1.insert(crate::dispatch::item_addr(1), item_text("queued", 0, &[("attempts", "1")]));
        let mut u2 = crate::phext::Phext::new();
        u2.insert(crate::dispatch::item_addr(1), item_text("running", 0, &[("attempts", "1")]));
        let dir = std::env::temp_dir().join(format!("qexo-doctor-chain-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("state.phext");
        let bytes = crate::phext::Phext::serialize_universes(&[u1, u2], true);
        std::fs::write(&path, bytes).unwrap();
        let lat = Lattice::load_from(&path).unwrap().expect("chain state loads");
        let report = check_queue(&lat);
        assert!(!flagged(&report, "illegal combination 7"), "{:?}", report.findings.iter().map(|f| &f.detail).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
