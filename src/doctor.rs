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
}
