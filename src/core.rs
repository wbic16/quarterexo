//! core — the state commands, dispatched from main.
//!
//! Every command here works the same way: load the one state
//! file through the lattice, read and write RECORDS (one scroll,
//! field lines), regenerate the indexes from the records, save.
//! Identity lives in the records' text and in the index scrolls;
//! nothing is addressed by formula (Layer 2, v2).

use crate::crypto::Gpg;
use crate::doctor;
use crate::hash;
use crate::infer::{Infer, InferError};
use crate::lattice::{self, Lattice};
use crate::phext::{fmt_coord, Phext};
use crate::plan;
use std::io::Read as IoRead;
use std::io::Write as IoWrite;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

// ----------------------------------------------------------------
// small shared helpers
// ----------------------------------------------------------------

fn hostname() -> String {
    if let Ok(h) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let h = h.trim().to_string();
        if !h.is_empty() {
            return h;
        }
    }
    std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "qexo-node".to_string())
}

/// (epoch day number, "YYYY-MM-DD HH:MM UTC") for right now.
pub(crate) fn now_stamp() -> (i64, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let day = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(day);
    (
        day,
        format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m, d, sod / 3600, (sod % 3600) / 60),
    )
}

/// Inverse of plan::days_from_civil (Hinnant's civil_from_days).
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn require_born() -> Result<Lattice, i32> {
    match Lattice::load() {
        Ok(Some(lat)) => Ok(lat),
        Ok(None) => {
            eprintln!(
                "qexo: {} is not born yet — run `qexo config` to birth this node",
                lattice::state_path().display()
            );
            Err(1)
        }
        Err(e) => {
            eprintln!("qexo: cannot load state: {}", e);
            Err(1)
        }
    }
}

/// Append a receipt scroll to the receipts run (6.6.6/6.6.6/6.6.6 homeland, cluster 2)
/// and return its coordinate. Receipts cite what they record by
/// coordinate, in prose, per Layer 2.
fn append_receipt(px: &mut Phext, action: &str, extra: &[(&str, &str)]) -> crate::phext::Coord {
    let b = lattice::next_free_book(px, [6, 6, 6, lattice::LEDGER_RECEIPTS, 1], 1);
    let addr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RECEIPTS, 1, b, 1, 1, 1]);
    let (_, stamp) = now_stamp();
    let mut fields: Vec<(&str, &str)> = vec![("kind", "receipt"), ("at", &stamp), ("action", action)];
    fields.extend_from_slice(extra);
    px.insert(addr, lattice::record(&fields));
    addr
}

/// Split a document into adjacent-scroll chunks at line
/// boundaries (Layer 2 §2: documents cite-able in parts take
/// adjacent scrolls in sequence).
fn chunk_text(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if cur.len() + line.len() + 1 > max && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        if line.len() > max {
            for piece in line.as_bytes().chunks(max) {
                chunks.push(String::from_utf8_lossy(piece).to_string());
            }
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

// ----------------------------------------------------------------
// config — the node's birth
// ----------------------------------------------------------------

/// The disk gate's default config lines, seeded visibly into the
/// config record so a node operator can see and edit them in the
/// phext: the volume lens on (judge the volume by its own free
/// fraction), the quota lens off (no absolute ceiling until the
/// node writes one), the standard file counts. plan reads the
/// same keys via plan::GateConfig::from_lookup; absent = default,
/// so seeding is for the operator's eyes, not the gate's needs.
fn gate_config_defaults() -> Vec<(&'static str, String)> {
    vec![
        ("volume_lens", "on".to_string()),
        ("disk_gate_free_amber_pct", "25".to_string()),
        ("disk_gate_free_red_pct", "10".to_string()),
        ("quota_lens", "off".to_string()),
        ("quota_amber_gb", "95".to_string()),
        ("quota_red_gb", "102".to_string()),
        ("staging_root", lattice::state_dir().display().to_string()),
        ("file_amber", "1100000".to_string()),
        ("file_red", "1300000".to_string()),
    ]
}

/// Ensure the config record carries every gate key, adding only
/// the absent ones (an operator's edits are never overwritten).
/// Returns the keys that were added.
fn ensure_gate_config(lat: &mut Lattice) -> Vec<String> {
    let mut added = Vec::new();
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    for (k, v) in gate_config_defaults() {
        if lattice::field(&text, k).is_none() {
            text = lattice::set_field(&text, k, &v);
            added.push(k.to_string());
        }
    }
    if !added.is_empty() {
        lat.px.insert(lattice::CONFIG_RECORD, text);
    }
    added
}

/// The update machinery's config lines (v1.1.0), seeded visibly
/// like the gate keys: where serve binds (loopback until the
/// operator names the node's tailnet address — the phextnet is
/// reached over the tailnet, never the public wire), whether
/// automatic updates are on, how often the timer polls, and the
/// release signer this node trusts. `release_signer` is seeded
/// EMPTY on purpose: trust is an operator's act, never a default
/// — until a fingerprint stands there, `qexo update` refuses
/// plainly and changes nothing.
fn update_config_defaults() -> Vec<(&'static str, String)> {
    vec![
        ("serve_bind", "127.0.0.1".to_string()),
        ("auto_update", "off".to_string()),
        ("update_interval_hours", "24".to_string()),
        ("release_signer", "".to_string()),
    ]
}

/// Ensure the config record carries every update key, adding
/// only the absent ones. Returns the keys that were added.
fn ensure_update_config(lat: &mut Lattice) -> Vec<String> {
    let mut added = Vec::new();
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    for (k, v) in update_config_defaults() {
        if lattice::field(&text, k).is_none() {
            text = lattice::set_field(&text, k, &v);
            added.push(k.to_string());
        }
    }
    if !added.is_empty() {
        lat.px.insert(lattice::CONFIG_RECORD, text);
    }
    added
}

/// The evolve key (v1.2.0), seeded visibly like the others:
/// `evolve: on` tells the automatic timer to fire a full
/// `qexo evolve` convergence pass instead of `qexo update`
/// alone — update is one stage of evolve, so the timer's
/// trigger widens, its logic does not move. Seeded OFF: a
/// node converges on a schedule only by its operator's word.
/// Running `qexo evolve` by hand never consults this key.
fn evolve_config_defaults() -> Vec<(&'static str, String)> {
    vec![("evolve", "off".to_string())]
}

/// Ensure the config record carries the evolve key, adding it
/// only when absent. Returns the keys that were added.
fn ensure_evolve_config(lat: &mut Lattice) -> Vec<String> {
    let mut added = Vec::new();
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    for (k, v) in evolve_config_defaults() {
        if lattice::field(&text, k).is_none() {
            text = lattice::set_field(&text, k, &v);
            added.push(k.to_string());
        }
    }
    if !added.is_empty() {
        lat.px.insert(lattice::CONFIG_RECORD, text);
    }
    added
}

/// Whether the config record carries `evolve: on` — the timer
/// trigger reads this, nothing else does.
fn evolve_enabled(lat: &Lattice) -> bool {
    lat.px
        .get(&lattice::CONFIG_RECORD)
        .and_then(|t| lattice::field(t, "evolve"))
        .map(|v| v == "on")
        .unwrap_or(false)
}

/// `qexo config --release-signer <FP>`: the operator's act of
/// trust as a command, instead of a hand edit of the config
/// scroll (the gap Will hit on 2026-11-13, setting the signer
/// from Phext Notepad because no setter existed). Writes the
/// fingerprint into the config record's `release_signer` field
/// line, via the lattice's field editor; the record itself is
/// config's seeding, created before this runs when absent.
fn apply_release_signer(lat: &mut Lattice, signer: &str) {
    let mut text = lat
        .px
        .get(&lattice::CONFIG_RECORD)
        .cloned()
        .unwrap_or_else(|| "kind: qexo config\n".to_string());
    text = lattice::set_field(&text, "release_signer", signer);
    lat.px.insert(lattice::CONFIG_RECORD, text);
}

pub fn config(auto_update: bool, evolve: bool, release_signer: Option<String>) -> i32 {
    let node = hostname();
    // Resolution first (the order is stated in lattice.rs): an
    // existing state — system or per-user — IS the node. Only a
    // birth picks a fresh path, and the birth path falls back to
    // the per-user home when the system directory is out of this
    // user's reach, so a plain `cargo install qexo` user births a
    // node with no sudo anywhere in the loop.
    let (mut lat, fell_back) = match Lattice::load() {
        Ok(Some(l)) => (l, false),
        Ok(None) => {
            let (bp, fb) = lattice::birth_path();
            (Lattice::born(bp), fb)
        }
        Err(e) => {
            eprintln!("qexo config: cannot load state: {}", e);
            return 1;
        }
    };
    let path = lat.path().clone();
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/var/lib/qexo"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("qexo config: cannot create {}: {}", dir.display(), e);
        return 1;
    }
    let gpg = Gpg::open_in(&dir);

    // Idempotent: a node key record already in the lattice means
    // this node is born; say so and stop — but first backfill any
    // gate config keys this binary knows that the record predates
    // (defaults are seeded visibly; existing values never touched).
    if let Some(t) = lat.px.get(&lattice::NODE_KEY_RECORD) {
        let fp = lattice::field(t, "fingerprint").unwrap_or_default();
        let mut added = ensure_gate_config(&mut lat);
        added.extend(ensure_update_config(&mut lat));
        added.extend(ensure_evolve_config(&mut lat));
        let signer_set = match &release_signer {
            Some(signer) => {
                apply_release_signer(&mut lat, signer);
                true
            }
            None => false,
        };
        if !added.is_empty() || signer_set {
            lattice::regen_indexes(&mut lat.px);
            if let Err(e) = lat.save() {
                eprintln!("qexo config: cannot save state: {}", e);
                return 1;
            }
            if !added.is_empty() {
                println!("config defaults added: {}", added.join(", "));
            }
        }
        println!("already configured — this node was born {}", lattice::field(lat.px.get(&lattice::IDENTITY).map(|s| s.as_str()).unwrap_or(""), "born").unwrap_or_default());
        println!("state: {}", path.display());
        println!("node key fingerprint: {}", fp);
        if let Some(signer) = &release_signer {
            println!("release signer set: {}", signer);
        }
        println!("home: {}", fmt_coord(&lattice::HOME));
        if evolve {
            let code = finish_evolve_flag();
            if code != 0 {
                return code;
            }
        }
        if auto_update {
            return finish_auto_update();
        }
        return 0;
    }

    // The key itself: reuse the keyring's key if one is already
    // there (a birth interrupted between key and record), else
    // generate. The record of it lives in the lattice either way.
    let existing = gpg.list_fingerprints().unwrap_or_default();
    let fp = match existing.last() {
        Some(f) => f.clone(),
        None => {
            if !crate::crypto::gpg_available() {
                eprintln!("qexo config: gpg is not available on this node; the node key cannot be set up");
                return 1;
            }
            match gpg.generate(&format!("qexo node {}", node), "qexo node key") {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("qexo config: node key generation failed: {}", e);
                    return 1;
                }
            }
        }
    };
    let public = match gpg.export_public(&fp) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("qexo config: cannot export the node public key: {}", e);
            return 1;
        }
    };

    let (_, stamp) = now_stamp();
    let name = format!("qexo node {}", node);
    lat.px.insert(
        lattice::IDENTITY,
        lattice::record(&[
            ("kind", "node identity"),
            ("name", &name),
            ("node", &node),
            ("born", &stamp),
            ("qexo", "v1.4.0"),
            ("state", &path.display().to_string()),
        ]),
    );
    lat.px.insert(
        lattice::NODE_KEY_RECORD,
        lattice::record(&[
            ("kind", "node key"),
            ("name", &name),
            ("fingerprint", &fp),
            ("algorithm", "eddsa/ed25519, suite: gpg"),
            ("status", "active"),
            ("issued", &stamp),
            ("public key", &fmt_coord(&lattice::NODE_KEY_PUBLIC)),
        ]),
    );
    lat.px.insert(lattice::NODE_KEY_PUBLIC, public.trim_end().to_string() + "\n");

    // Config record (9.9.9/9.9.9/9.9.9 homeland): defaults from the environment valves;
    // existing values (a re-run) are never overwritten.
    if lat.px.get(&lattice::CONFIG_RECORD).is_none() {
        let inf = Infer::from_env();
        let home_dir = std::env::var("HOME").unwrap_or_default();
        let tasks_default = format!("{}/workspace/phext/tasks.phext", home_dir);
        let tasks_source = if Path::new(&tasks_default).exists() { tasks_default.as_str() } else { "" };
        let disk_root = {
            let ws = format!("{}/workspace", home_dir);
            if Path::new(&ws).is_dir() { ws } else { dir.display().to_string() }
        };
        let mut fields: Vec<(&str, &str)> = vec![
            ("kind", "qexo config"),
            ("ollama endpoint", &inf.endpoint),
            ("model", &inf.model),
            ("tasks source", tasks_source),
            ("disk root", &disk_root),
        ];
        let defaults = gate_config_defaults();
        for (k, v) in &defaults {
            fields.push((*k, v.as_str()));
        }
        let upd = update_config_defaults();
        for (k, v) in &upd {
            fields.push((*k, v.as_str()));
        }
        let evo = evolve_config_defaults();
        for (k, v) in &evo {
            fields.push((*k, v.as_str()));
        }
        let rec = lattice::record(&fields);
        lat.px.insert(lattice::CONFIG_RECORD, rec);
    }

    // Wiring registry (9.9.9, cluster 2): one scroll per module —
    // the registry the wiring gate's story is told in.
    let wiring_first8 = [9, 9, 9, 2, 1, 1, 1, 1];
    if lattice::prefix_scrolls(&lat.px, &wiring_first8).is_empty() {
        let mut modules = vec![
            "cli", "core", "crypto", "doctor", "hash", "infer", "inventory", "lattice", "learn", "phext", "plan", "wiring", "xz",
        ];
        modules.sort();
        for (i, m) in modules.iter().enumerate() {
            let addr = lattice::at(lattice::HL_CONFIG, [2, 1, 1, 1, 1, (i + 1) as u32]);
            lat.px.insert(addr, format!("module: {}\nfile: src/{}.rs\n", m, m));
        }
    }

    if let Some(signer) = &release_signer {
        apply_release_signer(&mut lat, signer);
    }
    let receipt = append_receipt(&mut lat.px, "config", &[("note", "node born; state.phext initialized with the homeland map")]);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo config: cannot save state: {}", e);
        return 1;
    }
    println!("node born: {}", name);
    if fell_back {
        println!(
            "note: /var/lib/qexo is not writable by this user, so this node lives per-user — state and keyring under {} (no sudo needed, ever)",
            dir.display()
        );
    }
    println!("state: {}", path.display());
    println!("node key fingerprint: {}", fp);
    println!("home index: {}", fmt_coord(&lattice::HOME));
    println!("birth receipt: {}", fmt_coord(&receipt));
    if let Some(signer) = &release_signer {
        println!("release signer set: {}", signer);
    }
    if evolve {
        let code = finish_evolve_flag();
        if code != 0 {
            return code;
        }
    }
    if auto_update {
        return finish_auto_update();
    }
    0
}

/// `qexo config --auto-update`'s second act: set `auto_update:
/// on` in the config record (the node's own word, in its own
/// state), then try to install the systemd timer that pokes
/// `qexo update` on the configured interval. The timer is a thin
/// trigger — every decision stays inside `qexo update` and its
/// trust rule. Where systemd, root, or systemctl is missing, the
/// report says exactly that and prints the units for manual
/// installation; the config flag stands either way.
fn finish_auto_update() -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    ensure_update_config(&mut lat);
    ensure_evolve_config(&mut lat);
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    text = lattice::set_field(&text, "auto_update", "on");
    lat.px.insert(lattice::CONFIG_RECORD, text.clone());
    if let Err(e) = lat.save() {
        eprintln!("qexo config: cannot save state: {}", e);
        return 1;
    }
    let hours: u64 = lattice::field(&text, "update_interval_hours").and_then(|v| v.parse().ok()).unwrap_or(24);
    let evolve_on = lattice::field(&text, "evolve").map(|v| v == "on").unwrap_or(false);
    println!(
        "auto_update: on (poll every {}h; the trust rule in the config record still governs every poll{})",
        hours,
        if evolve_on { "; trigger: qexo evolve" } else { "" }
    );
    print!("{}", install_update_timer(hours, evolve_on));
    0
}

/// `qexo config --evolve`'s act: set `evolve: on` in the config
/// record — the node's own word that its automatic trigger is
/// a full convergence pass, not an update alone (update ⊂
/// evolve: the self-update is evolve's third stage). An
/// installed systemd unit names its command in its own text,
/// so when auto_update is already on, the unit is reinstalled
/// here with the widened trigger; flipping the key alone is
/// never left pretending to have rewritten a unit on disk.
fn finish_evolve_flag() -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    ensure_evolve_config(&mut lat);
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    text = lattice::set_field(&text, "evolve", "on");
    let auto_on = lattice::field(&text, "auto_update").map(|v| v == "on").unwrap_or(false);
    let hours: u64 = lattice::field(&text, "update_interval_hours").and_then(|v| v.parse().ok()).unwrap_or(24);
    lat.px.insert(lattice::CONFIG_RECORD, text);
    if let Err(e) = lat.save() {
        eprintln!("qexo config: cannot save state: {}", e);
        return 1;
    }
    println!("evolve: on (the automatic trigger, when installed, runs a full evolve pass; update is its third stage)");
    if auto_on {
        print!("{}", install_update_timer(hours, true));
    } else {
        println!("auto_update is off, so no timer stands to refresh; the trigger takes effect when the timer is installed (qexo config --auto-update)");
    }
    0
}

/// The systemd units for the automatic timer: a oneshot service
/// running the node's trigger by this binary's own path —
/// `qexo update`, or `qexo evolve` when the config record's
/// evolve key is on — and the timer that fires it. Pure text —
/// tested, and printed verbatim when installation isn't
/// possible here.
fn update_unit_texts(exe: &Path, hours: u64, evolve_trigger: bool) -> (String, String) {
    let trigger = if evolve_trigger { "evolve" } else { "update" };
    let service = format!(
        "[Unit]\nDescription=qexo automatic {} (phextnet, signed-manifest trust rule)\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nExecStart={} {}\n",
        if evolve_trigger { "evolve pass" } else { "update" },
        exe.display(),
        trigger
    );
    let timer = format!(
        "[Unit]\nDescription=Run qexo {} every {}h\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec={}h\nUnit=qexo-update.service\n\n[Install]\nWantedBy=timers.target\n",
        trigger, hours, hours
    );
    (service, timer)
}

/// Attempt the timer installation; return the report either
/// way. Installed only where all three hold: running as root,
/// systemd is the running init, systemctl answers. Anything
/// less and nothing is written outside the config record.
fn install_update_timer(hours: u64, evolve_trigger: bool) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("qexo"));
    let (service, timer) = update_unit_texts(&exe, hours, evolve_trigger);
    let mut why_not: Vec<String> = Vec::new();
    let uid0 = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|v| v.to_string()))
        })
        .map(|u| u == "0")
        .unwrap_or(false);
    if !uid0 {
        why_not.push("not running as root (the timer lives in /etc/systemd/system)".to_string());
    }
    if !Path::new("/run/systemd/system").is_dir() {
        why_not.push("systemd is not the running init here (no /run/systemd/system)".to_string());
    }
    let systemctl_ok = std::process::Command::new("systemctl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !systemctl_ok {
        why_not.push("systemctl does not answer on this node".to_string());
    }
    if !why_not.is_empty() {
        return format!(
            "timer NOT installed: {}.\nauto_update is on in the config record; a node's operator can install the trigger by hand —\n/etc/systemd/system/qexo-update.service:\n{}\n/etc/systemd/system/qexo-update.timer:\n{}\nthen: systemctl daemon-reload && systemctl enable --now qexo-update.timer\n",
            why_not.join("; "),
            service,
            timer
        );
    }
    let dir = Path::new("/etc/systemd/system");
    let mut steps: Vec<String> = Vec::new();
    for (name, text) in [("qexo-update.service", &service), ("qexo-update.timer", &timer)] {
        match std::fs::write(dir.join(name), text) {
            Ok(()) => steps.push(format!("wrote {}", dir.join(name).display())),
            Err(e) => return format!("timer NOT installed: cannot write {}: {}\n", dir.join(name).display(), e),
        }
    }
    for args in [&["daemon-reload"][..], &["enable", "--now", "qexo-update.timer"][..]] {
        match std::process::Command::new("systemctl").args(args).output() {
            Ok(o) if o.status.success() => steps.push(format!("systemctl {} ok", args.join(" "))),
            Ok(o) => {
                return format!(
                    "timer NOT fully installed: systemctl {} failed: {}\n",
                    args.join(" "),
                    String::from_utf8_lossy(&o.stderr).trim()
                )
            }
            Err(e) => return format!("timer NOT installed: systemctl {}: {}\n", args.join(" "), e),
        }
    }
    format!("timer installed: {}\n", steps.join("; "))
}

// ----------------------------------------------------------------
// peers — register / list (homeland anchor 3.3.3/3.3.3/3.3.3)
// ----------------------------------------------------------------

fn peer_records(px: &Phext) -> Vec<(u32, crate::phext::Coord, String)> {
    let mut out = Vec::new();
    for c in lattice::component_values(px, &lattice::HL_PEERS, 3) {
        if c < 2 {
            continue;
        }
        let addr = lattice::at(lattice::HL_PEERS, [c, 1, 1, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            out.push((c, addr, t.clone()));
        }
    }
    out
}

/// The TRUSTED peers: records whose status is `registered` —
/// the operator's own act (`qexo register`) put them there.
/// Everything that polls, syncs, or takes a release offer
/// iterates THIS set and no other. A `discovered` record (v1.2.0
/// gossip) is knowledge about the phextnet, not trust in it:
/// it is never polled for releases, never synced from, and
/// never eligible as a release signer, until the operator
/// registers its key and the record is promoted.
fn trusted_peer_records(px: &Phext) -> Vec<(u32, crate::phext::Coord, String)> {
    peer_records(px)
        .into_iter()
        .filter(|(_, _, t)| lattice::field(t, "status").as_deref() == Some("registered"))
        .collect()
}

pub fn register(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: qexo register <pubkey> [--endpoint <url>] — armored text, a file path, or - for stdin");
        return 2;
    }
    // The peer's endpoint (its `qexo serve` base URL) rides
    // along as --endpoint <url>, or as a trailing bare URL. It is
    // recorded on the peer record: without it the peer is
    // registered but not pollable, and `qexo update` says so.
    let mut endpoint: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--endpoint" {
            match it.next() {
                Some(v) => endpoint = Some(v.trim_end_matches('/').to_string()),
                None => {
                    eprintln!("qexo register: --endpoint needs a URL");
                    return 2;
                }
            }
        } else {
            rest.push(a.clone());
        }
    }
    if endpoint.is_none() && rest.len() > 1 {
        if let Some(last) = rest.last() {
            if last.starts_with("http://") || last.starts_with("https://") {
                endpoint = Some(rest.pop().unwrap().trim_end_matches('/').to_string());
            }
        }
    }
    let args = &rest;
    let armored = if args[0] == "-" {
        let mut s = String::new();
        match std::io::stdin().read_to_string(&mut s) {
            Ok(_) => s,
            Err(e) => {
                eprintln!("qexo register: cannot read stdin: {}", e);
                return 1;
            }
        }
    } else if args.len() == 1 && Path::new(&args[0]).is_file() {
        match std::fs::read_to_string(&args[0]) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("qexo register: {}: {}", args[0], e);
                return 1;
            }
        }
    } else {
        args.join(" ")
    };
    if !armored.contains("BEGIN PGP PUBLIC KEY BLOCK") {
        eprintln!("qexo register: that does not look like an armored public key");
        return 1;
    }
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let gpg = Gpg::open_in(&lattice::state_dir());
    // Identify BEFORE importing: who a key claims to be is read
    // from the key itself, never from the caller's say-so.
    let fp = match gpg.fingerprint_of(&armored) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("qexo register: cannot read a fingerprint from that key: {}", e);
            return 1;
        }
    };
    // The peer's host name rides in its key's user id ("qexo node
    // <hostname> (qexo node key)") — read from the key itself,
    // like the fingerprint, never from the caller's say-so. It is
    // recorded as the record's host: field (backfilled on
    // re-register for records that predate it), so displays name
    // the machine instead of surfacing a bare address.
    let host = gpg.name_of(&armored).ok().and_then(|uid| host_from_uid(&uid));
    for (cluster, addr, text) in peer_records(&lat.px) {
        if lattice::field(&text, "fingerprint").as_deref() == Some(fp.as_str()) {
            if lattice::field(&text, "status").as_deref() == Some("discovered") {
                // Promotion: this key was learned by gossip —
                // knowledge, not trust. The operator registering
                // it is the act that makes it trust. The record
                // is promoted in place; its via/discovered lines
                // stay, as provenance of how it was first met.
                if let Err(e) = gpg.import_public(&armored) {
                    eprintln!("qexo register: gpg import failed: {}", e);
                    return 1;
                }
                let (_, stamp) = now_stamp();
                let name = lattice::field(&text, "name").unwrap_or_default();
                let via = lattice::field(&text, "via").unwrap_or_else(|| "unknown".to_string());
                let key_addr = lattice::at(lattice::HL_PEERS, [cluster, 1, 1, 1, 1, 2]);
                let mut rec = lattice::set_field(&text, "status", "registered");
                rec = lattice::set_field(&rec, "registered", &stamp);
                rec = lattice::set_field(&rec, "public key", &fmt_coord(&key_addr));
                if let Some(h) = &host {
                    rec = lattice::set_field(&rec, "host", h);
                }
                if let Some(ep) = &endpoint {
                    rec = lattice::set_field(&rec, "endpoint", ep);
                }
                lat.px.insert(addr, rec);
                lat.px.insert(key_addr, armored.trim_end().to_string() + "\n");
                let kb = lattice::next_free_book(&lat.px, [2, 2, 2, lattice::KEYS_PEER_C, 1], 1);
                let kaddr = lattice::at(lattice::HL_KEYS, [lattice::KEYS_PEER_C, 1, kb, 1, 1, 1]);
                lat.px.insert(
                    kaddr,
                    lattice::record(&[
                        ("kind", "peer key"),
                        ("name", &name),
                        ("fingerprint", &fp),
                        ("peer", &fmt_coord(&addr)),
                        ("status", "registered"),
                    ]),
                );
                let receipt = append_receipt(
                    &mut lat.px,
                    "register",
                    &[
                        ("peer", &fmt_coord(&addr)),
                        ("fingerprint", &fp),
                        ("promoted", &format!("from discovered (via {})", via)),
                    ],
                );
                lattice::regen_indexes(&mut lat.px);
                if let Err(e) = lat.save() {
                    eprintln!("qexo register: cannot save state: {}", e);
                    return 1;
                }
                println!("registered {} at {} — promoted from discovered (via {})", name, fmt_coord(&addr), via);
                println!("fingerprint: {}", fp);
                println!("receipt: {}", fmt_coord(&receipt));
                return 0;
            }
            // Already known — but an endpoint named now backfills
            // the record (the peer becomes pollable), and the
            // host name read from the key backfills beside it for
            // records that predate the host: field.
            let mut updated = text.clone();
            let mut changed = false;
            if let Some(ep) = &endpoint {
                if lattice::field(&updated, "endpoint").as_deref() != Some(ep.as_str()) {
                    updated = lattice::set_field(&updated, "endpoint", ep);
                    changed = true;
                    println!("endpoint recorded for {}: {}", lattice::field(&text, "name").unwrap_or_default(), ep);
                }
            }
            if let Some(h) = &host {
                if lattice::field(&updated, "host").as_deref() != Some(h.as_str()) {
                    updated = lattice::set_field(&updated, "host", h);
                    changed = true;
                    println!("host recorded for {}: {}", lattice::field(&text, "name").unwrap_or_default(), h);
                }
            }
            if changed {
                lat.px.insert(addr, updated);
                lattice::regen_indexes(&mut lat.px);
                if let Err(e) = lat.save() {
                    eprintln!("qexo register: cannot save state: {}", e);
                    return 1;
                }
            }
            println!("already registered at {}", fmt_coord(&addr));
            println!("fingerprint: {}", fp);
            return 0;
        }
    }
    if let Err(e) = gpg.import_public(&armored) {
        eprintln!("qexo register: gpg import failed: {}", e);
        return 1;
    }
    let (_, stamp) = now_stamp();
    let cluster = lattice::next_free_cluster(&lat.px, lattice::HL_PEERS, 2);
    let addr = lattice::at(lattice::HL_PEERS, [cluster, 1, 1, 1, 1, 1]);
    let key_addr = lattice::at(lattice::HL_PEERS, [cluster, 1, 1, 1, 1, 2]);
    let name = format!("peer-{}", &fp[fp.len().saturating_sub(8)..].to_lowercase());
    let key_coord = fmt_coord(&key_addr);
    let mut peer_fields: Vec<(&str, &str)> = vec![
        ("kind", "peer"),
        ("name", &name),
        ("fingerprint", &fp),
        ("status", "registered"),
        ("registered", &stamp),
        ("public key", &key_coord),
    ];
    if let Some(h) = &host {
        peer_fields.push(("host", h.as_str()));
    }
    if let Some(ep) = &endpoint {
        peer_fields.push(("endpoint", ep.as_str()));
    }
    lat.px.insert(addr, lattice::record(&peer_fields));
    lat.px.insert(key_addr, armored.trim_end().to_string() + "\n");
    // The keys homeland keeps its own record of the same key,
    // citing the peer scroll — cross-references are text.
    let kb = lattice::next_free_book(&lat.px, [2, 2, 2, lattice::KEYS_PEER_C, 1], 1);
    let kaddr = lattice::at(lattice::HL_KEYS, [lattice::KEYS_PEER_C, 1, kb, 1, 1, 1]);
    lat.px.insert(
        kaddr,
        lattice::record(&[
            ("kind", "peer key"),
            ("name", &name),
            ("fingerprint", &fp),
            ("peer", &fmt_coord(&addr)),
            ("status", "registered"),
        ]),
    );
    let receipt = append_receipt(&mut lat.px, "register", &[("peer", &fmt_coord(&addr)), ("fingerprint", &fp)]);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo register: cannot save state: {}", e);
        return 1;
    }
    println!("registered {} at {}", name, fmt_coord(&addr));
    if let Some(h) = &host {
        println!("host: {}", h);
    }
    println!("fingerprint: {}", fp);
    println!("receipt: {}", fmt_coord(&receipt));
    0
}

/// A host name from a node key's user id: "qexo node <host>
/// (qexo node key)" gives <host>; a bare uid gives itself minus
/// any trailing parenthetical comment. None when nothing usable
/// remains — the display fallback (the record name) then rules.
fn host_from_uid(uid: &str) -> Option<String> {
    let mut s = uid.trim();
    if let Some(rest) = s.strip_prefix("qexo node ") {
        s = rest;
    } else if s == "qexo node" {
        s = "";
    }
    if s.ends_with(')') {
        if let Some(i) = s.rfind(" (") {
            s = s[..i].trim_end();
        }
    }
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// The name a peer is shown by: its host: field when one is
/// recorded, else its record name. Records that predate host:
/// need no rewrite for display — the fleet's record names ARE
/// their tailnet hostnames, so the fallback is already a name.
pub(crate) fn peer_display_name(text: &str) -> String {
    match lattice::field(text, "host") {
        Some(h) if !h.is_empty() => h,
        _ => lattice::field(text, "name").unwrap_or_default(),
    }
}

/// The endpoint as shown to humans: when the recorded endpoint's
/// authority is a bare IPv4 address and the peer has a name, the
/// name stands in the address's place (name every tailnet
/// address; never surface a bare IP where a name exists). The
/// recorded endpoint is never rewritten by display — polling
/// keeps using exactly what the operator registered.
pub(crate) fn peer_display_endpoint(text: &str) -> Option<String> {
    let ep = lattice::field(text, "endpoint").filter(|s| !s.is_empty())?;
    Some(display_endpoint_for(&peer_display_name(text), &ep))
}

/// The display substitution itself, for callers holding a name
/// and an endpoint rather than a whole record (gossip's poll
/// list): a bare-IPv4 authority is shown by name; anything else
/// (a hostname endpoint, a name-less peer) stands as recorded.
fn display_endpoint_for(name: &str, ep: &str) -> String {
    if name.is_empty() {
        return ep.to_string();
    }
    let Some((scheme, rest)) = ep.split_once("://") else {
        return ep.to_string();
    };
    let (authority, tail) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let host_part = authority.split(':').next().unwrap_or("");
    if host_part.parse::<std::net::Ipv4Addr>().is_ok() {
        format!("{}://{}{}{}", scheme, name, &authority[host_part.len()..], tail)
    } else {
        ep.to_string()
    }
}

fn peers_text(lat: &Lattice) -> String {
    let recs = peer_records(&lat.px);
    if recs.is_empty() {
        return "no peers registered — qexo register <pubkey> adds one\n".to_string();
    }
    let mut out = String::new();
    for (_, addr, text) in &recs {
        out.push_str(&format!(
            "{}  {}  {}  {}{}\n",
            lattice::field(text, "name").unwrap_or_default(),
            lattice::fp_short(&lattice::field(text, "fingerprint").unwrap_or_default()),
            fmt_coord(addr),
            lattice::field(text, "status").unwrap_or_default(),
            lattice::field(text, "endpoint").map(|e| format!("  {}", e)).unwrap_or_default()
        ));
    }
    out
}

/// The phextnet as a navigation tree (v1.4.0): the peers
/// homeland anchor as root, this node first and marked, each
/// peer nested under its record address with its display name,
/// status, and endpoint shown by name where a name exists.
/// Plain ASCII-safe marks, the editor's style: indentation and
/// +-- / \-- branch marks, nothing else.
fn peers_tree(lat: &Lattice) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} — peers\n", fmt_coord(&lattice::index_addr(lattice::HL_PEERS))));
    let node = lat
        .px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "this node".to_string());
    let fp = lat
        .px
        .get(&lattice::NODE_KEY_RECORD)
        .and_then(|t| lattice::field(t, "fingerprint"))
        .map(|f| lattice::fp_short(&f))
        .unwrap_or_default();
    let recs = peer_records(&lat.px);
    out.push_str(&format!(
        "{} {} (this node) — {}\n",
        if recs.is_empty() { "\\--" } else { "+--" },
        node,
        fp
    ));
    let n = recs.len();
    for (i, (_, addr, text)) in recs.iter().enumerate() {
        let mark = if i + 1 == n { "\\--" } else { "+--" };
        let status = lattice::field(text, "status").unwrap_or_default();
        let mut line = format!(
            "{} {} — {} — {}",
            mark,
            peer_display_name(text),
            status,
            fmt_coord(addr)
        );
        if let Some(ep) = peer_display_endpoint(text) {
            line.push_str(&format!(" — {}", ep));
        }
        if status == "discovered" {
            if let Some(via) = lattice::field(text, "via") {
                line.push_str(&format!(" (via {})", via));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    if recs.is_empty() {
        out.push_str("(no peers yet — qexo register <pubkey> adds one)\n");
    }
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
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

pub fn list(json: bool) -> i32 {
    let lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    if json {
        // The flat machine shape: one object per peer, fields as
        // recorded (the endpoint here is the recorded one —
        // machines dial what the operator registered; the
        // by-name substitution is a human display).
        let mut items = Vec::new();
        for (_, addr, text) in peer_records(&lat.px) {
            let t = text.as_str();
            items.push(format!(
                "{{\"name\":\"{}\",\"host\":\"{}\",\"fingerprint\":\"{}\",\"status\":\"{}\",\"coord\":\"{}\",\"endpoint\":\"{}\"}}",
                json_escape(&lattice::field(t, "name").unwrap_or_default()),
                json_escape(&lattice::field(t, "host").unwrap_or_default()),
                json_escape(&lattice::field(t, "fingerprint").unwrap_or_default()),
                json_escape(&lattice::field(t, "status").unwrap_or_default()),
                fmt_coord(&addr),
                json_escape(&lattice::field(t, "endpoint").unwrap_or_default())
            ));
        }
        println!("[{}]", items.join(","));
    } else {
        print!("{}", peers_tree(&lat));
    }
    0
}

// ----------------------------------------------------------------
// persona lineage — revision / follows / history / stamp (v1.2.0)
//
// A persona copy carries its lineage in its record: `revision`
// (1 at birth, +1 per local edit), `follows` ("none" at birth,
// else "<revision> <stamp>" naming the copy it directly
// succeeds — the scroll names the revision it follows),
// `history` (the comma-joined stamp chain from the root to
// this copy), and `stamp` (this copy's own stamp). A stamp is
// a short content address — sha256 over name + revision +
// follows + boot text — so two nodes advancing the same
// ancestor the same way arrive at the same stamp, and two
// advancing it differently provably diverge. Sync (`qexo
// evolve`) judges copies by their histories: a history that
// extends yours names a descendant; anything that is neither
// your descendant nor your ancestor is another line, and
// both stand (the KSO divergence ruling: keep both attempts,
// never last-writer-wins).
// ----------------------------------------------------------------

/// A persona copy's stamp: first 16 hex of sha256 over the
/// copy's identity-bearing content.
fn persona_stamp(name: &str, revision: u64, follows: &str, boot: &str) -> String {
    let h = hash::sha256_hex(format!("{}\n{}\n{}\n{}", name, revision, follows, boot).as_bytes());
    h[..16].to_string()
}

/// One node's copy of a persona, as its record + boot present
/// it. Records born before lineage existed read as revision 1
/// with a computed stamp — the implicit-lineage rule: a copy
/// with no named parent is a root, and identical roots (same
/// name, same boot) share ancestry by construction.
#[derive(Debug, Clone, PartialEq)]
struct PersonaCopy {
    revision: u64,
    follows: String,
    history: Vec<String>,
    stamp: String,
    boot: String,
}

fn copy_view(record_text: &str, boot: &str) -> PersonaCopy {
    let name = lattice::field(record_text, "name").unwrap_or_default();
    let revision: u64 = lattice::field(record_text, "revision").and_then(|v| v.parse().ok()).unwrap_or(1);
    let follows = lattice::field(record_text, "follows").unwrap_or_else(|| "none".to_string());
    let history: Vec<String> = match lattice::field(record_text, "history") {
        Some(h) if !h.trim().is_empty() => h.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    };
    if history.is_empty() {
        let stamp = lattice::field(record_text, "stamp")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| persona_stamp(&name, revision, &follows, boot));
        PersonaCopy { revision, follows, history: vec![stamp.clone()], stamp, boot: boot.to_string() }
    } else {
        let stamp = history.last().cloned().unwrap_or_default();
        PersonaCopy { revision, follows, history, stamp, boot: boot.to_string() }
    }
}

/// How two copies of one persona stand to each other.
#[derive(Debug, PartialEq)]
enum SyncDecision {
    /// Same line, same head: nothing to do.
    Same,
    /// The remote history extends the local one: a descendant —
    /// adopt it whole.
    Adopt,
    /// The local history extends the remote one: we are ahead.
    Behind,
    /// Neither history extends the other: the lines forked (or
    /// never met). Both stand; the incoming copy is kept as an
    /// attempt artifact. Never last-writer-wins.
    Diverge,
}

fn sync_decision(local: &PersonaCopy, remote: &PersonaCopy) -> SyncDecision {
    fn is_prefix(a: &[String], b: &[String]) -> bool {
        a.len() <= b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
    }
    if local.history == remote.history {
        // Same head stamp. A different boot under the same stamp
        // could only be surgery or collision — a fork; keep both.
        return if local.boot == remote.boot { SyncDecision::Same } else { SyncDecision::Diverge };
    }
    if is_prefix(&local.history, &remote.history) {
        return SyncDecision::Adopt;
    }
    if is_prefix(&remote.history, &local.history) {
        return SyncDecision::Behind;
    }
    SyncDecision::Diverge
}

/// This node's own name for prose (the identity record's name,
/// e.g. "qexo node <host>").
fn node_name(lat: &Lattice) -> String {
    lat.px
        .get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "name"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("qexo node {}", hostname()))
}

// ----------------------------------------------------------------
// personas & sessions — host / unhost / message (anchors 4.4.4/4.4.4/4.4.4, 7.7.7/7.7.7/7.7.7)
// ----------------------------------------------------------------

/// A persona's cluster (Collection) in the 4.4.4/4.4.4/4.4.4 homeland, found by the name
/// written in its record — navigation via the index's own terms.
fn persona_cluster(px: &Phext, name: &str) -> Option<u32> {
    for c in lattice::component_values(px, &lattice::HL_PERSONAS, 3) {
        if c < 2 {
            continue;
        }
        let addr = lattice::at(lattice::HL_PERSONAS, [c, 1, 1, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "name").as_deref() == Some(name) {
                return Some(c);
            }
        }
    }
    // second pass, case-insensitive: names are spoken loosely
    for c in lattice::component_values(px, &lattice::HL_PERSONAS, 3) {
        if c < 2 {
            continue;
        }
        let addr = lattice::at(lattice::HL_PERSONAS, [c, 1, 1, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "name").map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false) {
                return Some(c);
            }
        }
    }
    None
}

fn persona_addr(cluster: u32, book: u32, scroll: u32) -> crate::phext::Coord {
    lattice::at(lattice::HL_PERSONAS, [cluster, 1, book, 1, 1, scroll])
}

/// The persona's boot text, reassembled from its adjacent scroll
/// run (Book 2 of its cluster).
fn boot_text_of(px: &Phext, cluster: u32) -> Option<String> {
    let parts = lattice::prefix_scrolls(px, &[4, 4, 4, cluster, 1, 2, 1, 1]);
    if parts.is_empty() {
        return None;
    }
    Some(parts.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(""))
}

/// The open session (Book in the sessions cluster) for a persona.
fn open_session(px: &Phext, name: &str) -> Option<(u32, String)> {
    for b in lattice::component_values(px, &[7, 7, 7, lattice::MAIL_SESSIONS_C, 1], 5) {
        let addr = lattice::at(lattice::HL_MAIL, [lattice::MAIL_SESSIONS_C, 1, b, 1, 1, 1]);
        if let Some(t) = px.get(&addr) {
            if lattice::field(t, "persona").as_deref() == Some(name) && lattice::field(t, "status").as_deref() == Some("open") {
                return Some((b, t.clone()));
            }
        }
    }
    None
}

/// Inference settings: the config record's fields, with the
/// environment valves (QEXO_OLLAMA_URL / QEXO_MODEL) overriding —
/// the valves are for development, the record is the node's word.
fn infer_for(lat: &Lattice) -> Infer {
    let mut inf = Infer::from_env();
    if let Some(t) = lat.px.get(&lattice::CONFIG_RECORD) {
        if std::env::var("QEXO_OLLAMA_URL").is_err() {
            if let Some(e) = lattice::field(t, "ollama endpoint") {
                if !e.is_empty() {
                    inf.endpoint = e.trim_end_matches('/').to_string();
                }
            }
        }
        if std::env::var("QEXO_MODEL").is_err() {
            if let Some(m) = lattice::field(t, "model") {
                if !m.is_empty() {
                    inf.model = m;
                }
            }
        }
    }
    inf
}

/// Read a boot text from a file: if the file is itself a phext,
/// its largest scroll is the boot text; otherwise the whole text.
fn boot_from_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if text.bytes().any(|b| (0x17..=0x1f).contains(&b) || b == 0x02) {
        let px = Phext::parse(&text);
        let largest = px.scrolls.values().max_by_key(|t| t.len()).cloned().unwrap_or_default();
        Ok(largest)
    } else {
        Ok(text)
    }
}

pub fn host(persona: Option<String>, boot: Option<String>) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let name = match persona {
        None => {
            // `qexo host` with no persona: who is hosted now.
            let mut any = false;
            for b in lattice::component_values(&lat.px, &[7, 7, 7, lattice::MAIL_SESSIONS_C, 1], 5) {
                let addr = lattice::at(lattice::HL_MAIL, [lattice::MAIL_SESSIONS_C, 1, b, 1, 1, 1]);
                if let Some(t) = lat.px.get(&addr) {
                    if lattice::field(t, "status").as_deref() == Some("open") {
                        println!(
                            "{} — hosted since {} — session {}",
                            lattice::field(t, "persona").unwrap_or_default(),
                            lattice::field(t, "since").unwrap_or_default(),
                            fmt_coord(&addr)
                        );
                        any = true;
                    }
                }
            }
            if !any {
                println!("no personas hosted — qexo host <persona> loads one");
            }
            return 0;
        }
        Some(n) => n,
    };

    if open_session(&lat.px, &name).is_some() {
        println!("{} is already hosted", name);
        return 0;
    }

    // Resolve (or create) the persona's home in the 4.4.4/4.4.4/4.4.4 homeland.
    let mut cluster = persona_cluster(&lat.px, &name);
    let mut boot_text = cluster.and_then(|c| boot_text_of(&lat.px, c));
    let mut boot_source = String::new();
    if boot_text.is_none() {
        let (text, source) = if let Some(bp) = &boot {
            match boot_from_file(Path::new(bp)) {
                Ok(t) => (t, format!("--boot {}", bp)),
                Err(e) => {
                    eprintln!("qexo host: cannot read boot text: {}", e);
                    return 1;
                }
            }
        } else {
            let sidecar = lattice::state_dir().join(format!("{}.phext", name));
            match boot_from_file(&sidecar) {
                Ok(t) => (t, format!("sidecar {}", sidecar.display())),
                Err(_) => {
                    eprintln!(
                        "qexo host: no boot text for {} — pass --boot <path>, or place {}.phext beside the state file ({})",
                        name,
                        name,
                        lattice::state_dir().display()
                    );
                    return 1;
                }
            }
        };
        boot_text = Some(text);
        boot_source = source;
    }
    // A --boot file against an existing home whose boot differs
    // is a LOCAL EDIT (v1.2.0): the replacement is stamped below
    // as the next revision of the persona's line. (host() has
    // already returned for a hosted persona, so an edit never
    // swaps a live session's boot under it.)
    let mut edit_of: Option<PersonaCopy> = None;
    if let (Some(c), Some(bp)) = (cluster, &boot) {
        if let Some(existing) = boot_text.clone() {
            match boot_from_file(Path::new(bp)) {
                Ok(t) if t != existing => {
                    let rec = lat.px.get(&persona_addr(c, 1, 1)).cloned().unwrap_or_default();
                    edit_of = Some(copy_view(&rec, &existing));
                    boot_text = Some(t);
                    boot_source = format!("--boot {} (local edit)", bp);
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("qexo host: cannot read boot text: {}", e);
                    return 1;
                }
            }
        }
    }
    let boot_text = boot_text.unwrap();
    let (_, stamp) = now_stamp();
    let c = match cluster {
        Some(c) => c,
        None => {
            let c = lattice::next_free_cluster(&lat.px, lattice::HL_PERSONAS, 2);
            let pstamp = persona_stamp(&name, 1, "none", &boot_text);
            let origin = node_name(&lat);
            lat.px.insert(
                persona_addr(c, 1, 1),
                lattice::record(&[
                    ("kind", "persona"),
                    ("name", &name),
                    ("status", "resident"),
                    ("born", &stamp),
                    ("boot source", &boot_source),
                    ("revision", "1"),
                    ("follows", "none"),
                    ("history", &pstamp),
                    ("stamp", &pstamp),
                    ("origin", &origin),
                ]),
            );
            cluster = Some(c);
            c
        }
    };
    // Store the boot text as an adjacent scroll run when this
    // home has none yet, or when this call is a local edit
    // replacing it (a persona's boot is cite-able in parts).
    if edit_of.is_some() || boot_text_of(&lat.px, c).is_none() {
        let old_chunks: Vec<crate::phext::Coord> = lattice::prefix_scrolls(&lat.px, &[4, 4, 4, c, 1, 2, 1, 1])
            .iter()
            .map(|(a, _)| *a)
            .collect();
        for a in old_chunks {
            lat.px.scrolls.remove(&a);
        }
        for (i, chunk) in chunk_text(&boot_text, 3800).iter().enumerate() {
            lat.px.insert(persona_addr(c, 2, (i + 1) as u32), chunk.clone());
        }
        if lat.px.get(&persona_addr(c, 3, 1)).is_none() {
            lat.px.insert(
                persona_addr(c, 3, 1),
                lattice::record(&[("kind", "persona state"), ("persona", &name), ("status", "resident")]),
            );
        }
    }
    // Lineage bookkeeping for the ways a boot arrives after
    // birth: a local edit bumps the revision and names the copy
    // it follows; a first lineage stamp lands on a record that
    // predates lineage (its root, computed like any root).
    if let Some(old) = &edit_of {
        let new_rev = old.revision + 1;
        let follows = format!("{} {}", old.revision, old.stamp);
        let pstamp = persona_stamp(&name, new_rev, &follows, &boot_text);
        let mut history = old.history.clone();
        history.push(pstamp.clone());
        let origin = node_name(&lat);
        if let Some(rec) = lat.px.get(&persona_addr(c, 1, 1)).cloned() {
            let mut rec = lattice::set_field(&rec, "revision", &new_rev.to_string());
            rec = lattice::set_field(&rec, "follows", &follows);
            rec = lattice::set_field(&rec, "history", &history.join(","));
            rec = lattice::set_field(&rec, "stamp", &pstamp);
            rec = lattice::set_field(&rec, "origin", &origin);
            rec = lattice::set_field(&rec, "boot source", &boot_source);
            lat.px.insert(persona_addr(c, 1, 1), rec);
        }
    } else if let Some(rec) = lat.px.get(&persona_addr(c, 1, 1)).cloned() {
        if lattice::field(&rec, "revision").is_none() && boot_text_of(&lat.px, c).is_some() {
            let pstamp = persona_stamp(&name, 1, "none", &boot_text);
            let mut rec = lattice::set_field(&rec, "revision", "1");
            rec = lattice::set_field(&rec, "follows", "none");
            rec = lattice::set_field(&rec, "history", &pstamp);
            rec = lattice::set_field(&rec, "stamp", &pstamp);
            lat.px.insert(persona_addr(c, 1, 1), rec);
        }
    }

    // The one foreign border: load into local inference.
    let inf = infer_for(&lat);
    match inf.prime(&boot_text) {
        Ok(reply) => {
            let sb = lattice::next_free_book(&lat.px, [7, 7, 7, lattice::MAIL_SESSIONS_C, 1], 1);
            let saddr = lattice::at(lattice::HL_MAIL, [lattice::MAIL_SESSIONS_C, 1, sb, 1, 1, 1]);
            lat.px.insert(
                saddr,
                lattice::record(&[
                    ("kind", "session"),
                    ("persona", &name),
                    ("model", &inf.model),
                    ("endpoint", &inf.endpoint),
                    ("since", &stamp),
                    ("status", "open"),
                    ("prime reply", reply.trim()),
                    ("persona home", &fmt_coord(&persona_addr(c, 1, 1))),
                ]),
            );
            if let Some(st) = lat.px.get(&persona_addr(c, 3, 1)).cloned() {
                lat.px.insert(persona_addr(c, 3, 1), lattice::set_field(&st, "status", "hosted"));
            }
            let receipt = append_receipt(&mut lat.px, "host", &[("persona", &name), ("session", &fmt_coord(&saddr))]);
            lattice::regen_indexes(&mut lat.px);
            if let Err(e) = lat.save() {
                eprintln!("qexo host: cannot save state: {}", e);
                return 1;
            }
            println!("hosted {} — session {}", name, fmt_coord(&saddr));
            println!("prime reply: {}", reply.trim());
            println!("receipt: {}", fmt_coord(&receipt));
            0
        }
        Err(e) => {
            // Honest refusal: record the attempt as a receipt,
            // host nothing, say exactly why.
            let why = match &e {
                InferError::Unreachable(m) => format!("unreachable: {}", m),
                InferError::ModelAbsent(m) => format!("model absent: {}", m),
                InferError::BadResponse(m) => format!("bad response: {}", m),
                InferError::Refused(m) => format!("refused: {}", m),
            };
            append_receipt(&mut lat.px, "host refused", &[("persona", &name), ("reason", &why)]);
            lattice::regen_indexes(&mut lat.px);
            let _ = lat.save();
            eprintln!("qexo host: cannot host {} — {}", name, e);
            1
        }
    }
}

pub fn unhost(name: &str) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let (sb, stext) = match open_session(&lat.px, name) {
        Some(s) => s,
        None => {
            eprintln!("qexo unhost: {} is not hosted", name);
            return 1;
        }
    };
    let inf = infer_for(&lat);
    let unload_note = match inf.unload() {
        Ok(()) => "model unloaded".to_string(),
        Err(e) => format!("endpoint unload not confirmed ({}) — a dead endpoint holds nothing anyway", e),
    };
    let (_, stamp) = now_stamp();
    let saddr = lattice::at(lattice::HL_MAIL, [lattice::MAIL_SESSIONS_C, 1, sb, 1, 1, 1]);
    let closed = lattice::set_field(&lattice::set_field(&stext, "status", "closed"), "closed", &stamp);
    lat.px.insert(saddr, closed);
    if let Some(c) = persona_cluster(&lat.px, name) {
        if let Some(st) = lat.px.get(&persona_addr(c, 3, 1)).cloned() {
            lat.px.insert(persona_addr(c, 3, 1), lattice::set_field(&st, "status", "resident"));
        }
    }
    let receipt = append_receipt(&mut lat.px, "unhost", &[("persona", name), ("session", &fmt_coord(&saddr))]);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo unhost: cannot save state: {}", e);
        return 1;
    }
    println!("unhosted {} — session {} closed ({})", name, fmt_coord(&saddr), unload_note);
    println!("receipt: {}", fmt_coord(&receipt));
    0
}

pub fn message(persona: &str, text: &str) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let mut text = text.to_string();
    if text.trim().is_empty() {
        let mut s = String::new();
        match std::io::stdin().read_to_string(&mut s) {
            Ok(_) => text = s.trim().to_string(),
            Err(e) => {
                eprintln!("qexo message: cannot read stdin: {}", e);
                return 1;
            }
        }
    }
    if text.is_empty() {
        eprintln!("usage: qexo message <persona> <text...>");
        return 2;
    }
    if open_session(&lat.px, persona).is_none() {
        eprintln!("qexo message: {} is not hosted — `qexo host {}` first", persona, persona);
        return 1;
    }
    let cluster = match persona_cluster(&lat.px, persona) {
        Some(c) => c,
        None => {
            eprintln!("qexo message: no persona named {} lives in state", persona);
            return 1;
        }
    };
    let boot = match boot_text_of(&lat.px, cluster) {
        Some(b) => b,
        None => {
            eprintln!("qexo message: {} has a session but no boot text in its home — inconsistent state; qexo doctor may say more", persona);
            return 1;
        }
    };
    let inf = infer_for(&lat);
    match inf.chat(&boot, &text, "30m") {
        Ok(reply) => {
            let (_, stamp) = now_stamp();
            let entry = lattice::next_free_scroll(&lat.px, [4, 4, 4, cluster, 1, 4, 1, 1], 1);
            lat.px.insert(
                entry,
                format!("at: {}\nuser: {}\n{}: {}\n", stamp, text, persona, reply.trim()),
            );
            append_receipt(&mut lat.px, "message", &[("persona", persona), ("transcript", &fmt_coord(&entry))]);
            lattice::regen_indexes(&mut lat.px);
            if let Err(e) = lat.save() {
                eprintln!("qexo message: cannot save state: {}", e);
                return 1;
            }
            println!("{}", reply.trim());
            0
        }
        Err(e) => {
            eprintln!("qexo message: the exchange failed — {}", e);
            1
        }
    }
}

// ----------------------------------------------------------------
// status
// ----------------------------------------------------------------

pub fn status() -> i32 {
    let path = lattice::state_path();
    println!("qexo v1.4.0 — node status");
    println!("state: {}", path.display());
    let lat = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => {
            println!("node: not born — run qexo config");
            return 1;
        }
        Err(e) => {
            println!("node: state unreadable: {}", e);
            return 1;
        }
    };
    if let Some(t) = lat.px.get(&lattice::IDENTITY) {
        println!(
            "node: {} ({})",
            lattice::field(t, "name").unwrap_or_default(),
            lattice::field(t, "node").unwrap_or_default()
        );
    }
    let fp = lat
        .px
        .get(&lattice::NODE_KEY_RECORD)
        .and_then(|t| lattice::field(t, "fingerprint"))
        .unwrap_or_else(|| "(no node key)".to_string());
    println!("node key: {}", fp);
    let peers = peer_records(&lat.px);
    println!(
        "peers: {}{}",
        peers.len(),
        if peers.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                peers
                    .iter()
                    .map(|(_, _, t)| peer_display_name(t))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    let mut hosted: Vec<String> = Vec::new();
    for b in lattice::component_values(&lat.px, &[7, 7, 7, lattice::MAIL_SESSIONS_C, 1], 5) {
        let addr = lattice::at(lattice::HL_MAIL, [lattice::MAIL_SESSIONS_C, 1, b, 1, 1, 1]);
        if let Some(t) = lat.px.get(&addr) {
            if lattice::field(t, "status").as_deref() == Some("open") {
                hosted.push(lattice::field(t, "persona").unwrap_or_default());
            }
        }
    }
    println!("hosted: {}", if hosted.is_empty() { "none".to_string() } else { hosted.join(", ") });
    let fs_report = doctor::check(&lattice::state_dir(), &path);
    let lat_report = doctor::check_lattice(&lat);
    let problems = fs_report.problems() + lat_report.problems();
    println!("doctor: {}", if problems == 0 { "ok".to_string() } else { format!("{} problem(s)", problems) });
    0
}

// ----------------------------------------------------------------
// plan — telemetry in, plan scroll out (anchor 5.5.5/5.5.5/5.5.5, fed by 8.8.8/8.8.8/8.8.8)
// ----------------------------------------------------------------

/// What the disk probe reads: the volume under one root (bytes,
/// from the platform's own volume call — see volume_at) and a
/// file count under another. The probe takes no position on what
/// the numbers MEAN — thresholds live in plan::evaluate_gate,
/// fed by the node's config record. (Before v1.0.2 this function
/// carried the VM quota's 95/102 GB verdicts inside itself; a
/// probe that judges is a gate no one configured.)
struct DiskProbe {
    volume: Option<plan::Volume>,
    files: u64,
}

fn probe_disk(volume_root: &Path, files_root: &Path, file_cap: u64) -> DiskProbe {
    let volume = volume_at(volume_root);
    // file count under the staging root, capped just past the RED
    // threshold — past it, the exact number stops mattering.
    let cap: u64 = file_cap + 1;
    let mut files: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![files_root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if files >= cap {
            break;
        }
        if let Ok(rd) = std::fs::read_dir(&d) {
            for entry in rd.flatten() {
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push(entry.path()),
                    Ok(t) if t.is_file() => {
                        files += 1;
                        if files >= cap {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    DiskProbe { volume, files }
}

/// The volume under a root, read by the platform's own call.
/// On any failure — and on platforms with no read here — the
/// probe honestly has no volume: plan already works from an
/// absent volume (every consumer reads it through Option).
#[cfg(unix)]
fn volume_at(root: &Path) -> Option<plan::Volume> {
    #[repr(C)]
    struct Statvfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
        f_spare: [i32; 6],
    }
    extern "C" {
        fn statvfs(path: *const std::os::raw::c_char, buf: *mut Statvfs) -> i32;
    }
    let mut volume: Option<plan::Volume> = None;
    if let Ok(cpath) = std::ffi::CString::new(root.display().to_string()) {
        let mut st = Statvfs {
            f_bsize: 0, f_frsize: 0, f_blocks: 0, f_bfree: 0, f_bavail: 0,
            f_files: 0, f_ffree: 0, f_favail: 0, f_fsid: 0, f_flag: 0, f_namemax: 0,
            f_spare: [0; 6],
        };
        let rc = unsafe { statvfs(cpath.as_ptr(), &mut st) };
        if rc == 0 && st.f_blocks > 0 {
            let fr = st.f_frsize;
            volume = Some(plan::Volume {
                root: root.display().to_string(),
                total_bytes: st.f_blocks * fr,
                used_bytes: (st.f_blocks - st.f_bfree) * fr,
                free_bytes: st.f_bavail * fr,
            });
        }
    }
    volume
}

/// Windows volume read, std-only via kernel32:
/// GetDiskFreeSpaceExW answers for the volume under a directory
/// path — total bytes, total free bytes, and the bytes available
/// to the caller (the quota-aware figure). (Through v1.0.3 the
/// probe declared POSIX statvfs on every platform; Windows has
/// no such symbol, so the Windows build failed at link time —
/// and `cargo check` gates could not see it, because check
/// never links. Fixed in v1.0.4.)
#[cfg(windows)]
fn volume_at(root: &Path) -> Option<plan::Volume> {
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = root
        .display()
        .to_string()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut avail: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut total_free) };
    if ok == 0 || total == 0 {
        return None;
    }
    Some(win32_volume(root.display().to_string(), total, total_free, avail))
}

/// Map GetDiskFreeSpaceExW's three numbers onto plan::Volume in
/// the same terms as the statvfs read, so the gate cannot tell
/// which platform measured: total is the whole volume; used is
/// every byte not free at all (the f_bfree analog, derived from
/// lpTotalNumberOfFreeBytes); free is what the caller may
/// actually write (the f_bavail analog, lpFreeBytesAvailableToCaller
/// — quota-aware). Deliberately not cfg-gated: the mapping is
/// pure math and is unit-tested on every platform; on
/// non-Windows builds the test is its only caller.
#[allow(dead_code)]
fn win32_volume(root: String, total_bytes: u64, total_free_bytes: u64, free_bytes: u64) -> plan::Volume {
    plan::Volume {
        root,
        total_bytes,
        used_bytes: total_bytes.saturating_sub(total_free_bytes),
        free_bytes,
    }
}

#[cfg(not(any(unix, windows)))]
fn volume_at(root: &Path) -> Option<plan::Volume> {
    let _ = root;
    None
}

/// Task rows from the task register phext named by the config
/// record: each scroll is a task (its `# title` line, its
/// `Status:` line); closed/completed tasks are not open work.
fn tasks_from_source(path: &Path) -> Vec<plan::TaskInput> {
    let mut out = Vec::new();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return out,
    };
    let px = Phext::parse(&text);
    for (coord, body) in &px.scrolls {
        if *coord == [1u32; 9] {
            continue; // the register's home/index scroll
        }
        let status = lattice::field(body, "Status").unwrap_or_default().to_lowercase();
        if status.starts_with("closed") || status.starts_with("complete") || status.starts_with("superseded") {
            continue;
        }
        let title = body
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l[2..].trim().to_string())
            .unwrap_or_else(|| {
                body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
            });
        if title.is_empty() {
            continue;
        }
        let mut request = String::new();
        let lines: Vec<&str> = body.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if l.starts_with("Request") {
                let after = l.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                request = if !after.is_empty() {
                    after
                } else {
                    lines.get(i + 1).map(|s| s.trim().to_string()).unwrap_or_default()
                };
                break;
            }
        }
        let coordinate = lattice::field(body, "Coordinate").unwrap_or_else(|| fmt_coord(coord));
        out.push(plan::TaskInput { coordinate, title, request });
    }
    out
}

pub fn plan_cmd() -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let cfg = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_default();
    let gate_cfg = plan::GateConfig::from_lookup(|k| lattice::field(&cfg, k));
    let disk_root_s = lattice::field(&cfg, "disk root").unwrap_or_else(|| lattice::state_dir().display().to_string());
    let staging_root_s = lattice::field(&cfg, "staging_root").unwrap_or_else(|| lattice::state_dir().display().to_string());
    let probe = probe_disk(Path::new(&disk_root_s), Path::new(&staging_root_s), gate_cfg.file_red);
    let used_gb = probe
        .volume
        .as_ref()
        .map(|v| (v.used_bytes as f64 / 1.0e9 * 100.0).round() / 100.0)
        .unwrap_or(0.0);
    let files = probe.files;
    let gate_inputs = plan::GateInputs {
        volume: probe.volume.clone(),
        files_root: staging_root_s.clone(),
        config: gate_cfg,
    };
    let (day, stamp) = now_stamp();
    let (y, m, d) = civil_from_days(day);

    // The level recorded with the observation is the gate's own
    // evaluation of this reading (pre-forecast): the telemetry
    // series carries qexo's verdicts, computed from the node's
    // configured thresholds — never a probe's private constants.
    let draft = plan::DiskObs { day, used_gb, level: String::new(), files };
    let level = plan::evaluate_gate(&draft, gate_inputs.volume.as_ref(), &gate_inputs.files_root, &gate_inputs.config).disk_gate;
    let (total_bytes, free_bytes) = probe
        .volume
        .as_ref()
        .map(|v| (v.total_bytes.to_string(), v.free_bytes.to_string()))
        .unwrap_or_else(|| ("".to_string(), "".to_string()));

    // Record the observation in the telemetry homeland: the
    // series is native now; the forecast reads its own lattice.
    let ob = lattice::next_free_book(&lat.px, [8, 8, 8, lattice::TELEM_DISK_C, 1], 1);
    let oaddr = lattice::at(lattice::HL_TELEMETRY, [lattice::TELEM_DISK_C, 1, ob, 1, 1, 1]);
    lat.px.insert(
        oaddr,
        lattice::record(&[
            ("kind", "disk observation"),
            ("day", &day.to_string()),
            ("date", &format!("{:04}-{:02}-{:02}", y, m, d)),
            ("used_gb", &format!("{:.2}", used_gb)),
            ("total_bytes", &total_bytes),
            ("free_bytes", &free_bytes),
            ("level", &level),
            ("files", &files.to_string()),
            ("files root", &staging_root_s),
            ("root", &disk_root_s),
        ]),
    );

    // History = every observation scroll, in address (arrival) order.
    let mut history: Vec<plan::DiskObs> = Vec::new();
    for b in lattice::component_values(&lat.px, &[8, 8, 8, lattice::TELEM_DISK_C, 1], 5) {
        let addr = lattice::at(lattice::HL_TELEMETRY, [lattice::TELEM_DISK_C, 1, b, 1, 1, 1]);
        if let Some(t) = lat.px.get(&addr) {
            history.push(plan::DiskObs {
                day: lattice::field(t, "day").and_then(|v| v.parse().ok()).unwrap_or(0),
                used_gb: lattice::field(t, "used_gb").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                level: lattice::field(t, "level").unwrap_or_else(|| "?".to_string()),
                files: lattice::field(t, "files").and_then(|v| v.parse().ok()).unwrap_or(0),
            });
        }
    }
    let current = history.last().cloned().unwrap_or(plan::DiskObs { day, used_gb, level: level.clone(), files });

    let tasks_source = lattice::field(&cfg, "tasks source").unwrap_or_default();
    let tasks = if tasks_source.is_empty() {
        Vec::new()
    } else {
        tasks_from_source(Path::new(&tasks_source))
    };

    let host = plan::host_snapshot();
    let mut data = plan::compute(&tasks, &current, &history, host, &gate_inputs);
    data.disk_date = format!("{:04}-{:02}-{:02}", y, m, d);
    let rendered = plan::render(&data);

    // The plan is a record: fields scroll + rendering beside it,
    // and the same pair appended to the history run.
    let fields = lattice::record(&[
        ("kind", "plan"),
        ("plan version", plan::PLAN_VERSION),
        ("emitted", &stamp),
        ("disk gate", &data.disk_gate),
        ("gate reason", &data.gate_reason),
        ("open tasks", &data.open_tasks().to_string()),
        ("tasks source", if tasks_source.is_empty() { "(none configured)" } else { &tasks_source }),
        ("observation", &fmt_coord(&oaddr)),
    ]);
    lat.px.insert(lattice::PLAN_CURRENT, fields.clone());
    lat.px.insert(lattice::PLAN_CURRENT_RENDER, rendered.clone());
    let hb = lattice::next_free_book(&lat.px, [5, 5, 5, lattice::PLAN_HISTORY_C, 1], 1);
    lat.px.insert(lattice::at(lattice::HL_PLAN, [lattice::PLAN_HISTORY_C, 1, hb, 1, 1, 1]), fields);
    lat.px.insert(lattice::at(lattice::HL_PLAN, [lattice::PLAN_HISTORY_C, 1, hb, 1, 1, 2]), rendered.clone());
    append_receipt(&mut lat.px, "plan", &[("plan", &fmt_coord(&lattice::PLAN_CURRENT)), ("disk gate", &data.disk_gate)]);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo plan: cannot save state: {}", e);
        return 1;
    }
    print!("{}", rendered);
    0
}

// ----------------------------------------------------------------
// serve — the phext HTTP API: payloads are scroll text
// ----------------------------------------------------------------

pub fn serve(port: u16) -> i32 {
    // The bind address is the config record's `serve_bind`
    // (default 127.0.0.1). Loopback serves the node itself; a
    // node that wants to be reached over the phextnet sets it
    // to its tailnet address — the operator's act, in the
    // operator's state. Integrity never depends on the
    // transport: release trust is the manifest's signature.
    let bind_s = match Lattice::load() {
        Ok(Some(lat)) => lat
            .px
            .get(&lattice::CONFIG_RECORD)
            .and_then(|t| lattice::field(t, "serve_bind"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string()),
        _ => "127.0.0.1".to_string(),
    };
    let bind_ip: std::net::IpAddr = match bind_s.parse() {
        Ok(ip) => ip,
        Err(_) => {
            eprintln!("qexo serve: serve_bind {:?} in the config record is not an IP address", bind_s);
            return 1;
        }
    };
    let listener = match TcpListener::bind((bind_ip, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("qexo serve: cannot bind {}:{}: {}", bind_ip, port, e);
            return 1;
        }
    };
    println!("qexo serve — http://{}:{} — payloads are scroll text", bind_ip, port);
    println!("routes: / (home index), /v1/doctor, /v1/plan, /v1/peers, /v1/scroll/<coord>, /v1/release, /v1/release/signature, /release/qexo-<version>");
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut buf = [0u8; 8192];
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_string();
        let (status, ctype, body) = route(&path);
        let mut resp = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            status,
            ctype,
            body.len()
        )
        .into_bytes();
        resp.extend_from_slice(&body);
        let _ = stream.write_all(&resp);
    }
    0
}

/// The newest release announcement's (manifest, signature)
/// scroll texts, if this node has announced one. The manifest
/// is the phextnet's trust object; serve only carries it.
fn latest_release(px: &Phext) -> Option<(String, String)> {
    let books = lattice::component_values(px, &[6, 6, 6, lattice::LEDGER_RELEASES, 1], 5);
    let book = books.last()?;
    let manifest = px.get(&lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RELEASES, 1, *book, 1, 1, 1]))?.clone();
    let signature = px.get(&lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RELEASES, 1, *book, 1, 1, 2]))?.clone();
    Some((manifest, signature))
}

fn text_route(status: &str, body: String) -> (String, &'static str, Vec<u8>) {
    (status.to_string(), "text/plain; charset=utf-8", body.into_bytes())
}

fn route(path: &str) -> (String, &'static str, Vec<u8>) {
    let lat = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => return text_route("200 OK", "qexo: state not born — run qexo config on the node\n".to_string()),
        Err(e) => return text_route("500 Internal Server Error", format!("qexo: state unreadable: {}\n", e)),
    };
    match path {
        "/" => text_route(
            "200 OK",
            lat.px.get(&lattice::HOME).cloned().unwrap_or_else(|| "(no home index)\n".to_string()),
        ),
        "/v1/doctor" => {
            let mut report = doctor::check(&lattice::state_dir(), &lattice::state_path());
            report.findings.extend(doctor::check_lattice(&lat).findings);
            text_route("200 OK", report.render())
        }
        "/v1/plan" => text_route(
            "200 OK",
            lat.px
                .get(&lattice::PLAN_CURRENT_RENDER)
                .cloned()
                .unwrap_or_else(|| "no plan emitted yet — bare qexo on the node computes the first\n".to_string()),
        ),
        "/v1/peers" => text_route("200 OK", peers_text(&lat)),
        _ if path.starts_with("/v1/scroll/") => {
            // One scroll by its full 9D coordinate — the read the
            // phextnet's convergence passes navigate by: a peer's
            // index scroll names the records, this route carries
            // them. Read-only, like everything serve does; state
            // changes only ever arrive as whole verified acts
            // (register, sync adoption), never as writes here.
            let coord_s = &path["/v1/scroll/".len()..];
            match crate::phext::parse_coord(coord_s) {
                Some(c) => match lat.px.get(&c) {
                    Some(t) => text_route("200 OK", t.clone()),
                    None => text_route("404 Not Found", format!("no scroll at {}\n", coord_s)),
                },
                None => text_route("400 Bad Request", format!("not a full 9D coordinate: {}\n", coord_s)),
            }
        }
        "/v1/release" => match latest_release(&lat.px) {
            Some((manifest, _)) => text_route("200 OK", manifest),
            None => text_route("404 Not Found", "no release announced on this node — `qexo release` on the signer announces one\n".to_string()),
        },
        "/v1/release/signature" => match latest_release(&lat.px) {
            Some((_, signature)) => text_route("200 OK", signature),
            None => text_route("404 Not Found", "no release announced on this node\n".to_string()),
        },
        _ if path.starts_with("/release/") => {
            // The binary itself: bulk bytes ride the wire from the
            // releases dir beside the state file — never out of
            // state.phext. The name is a single path component,
            // minted by `qexo release` as qexo-<version>.
            let name = &path["/release/".len()..];
            if name.is_empty() || name.contains('/') || name.contains("..") || !name.starts_with("qexo-") {
                return text_route("404 Not Found", "no such release file\n".to_string());
            }
            match std::fs::read(lattice::state_dir().join("releases").join(name)) {
                Ok(bytes) => ("200 OK".to_string(), "application/octet-stream", bytes),
                Err(_) => text_route("404 Not Found", format!("no release file {} in the releases dir\n", name)),
            }
        }
        _ => text_route(
            "404 Not Found",
            "no such route — qexo serves /, /v1/doctor, /v1/plan, /v1/peers, /v1/scroll/<coord>, /v1/release, /v1/release/signature, /release/qexo-<version> (payloads are scroll text; the release binary is the one byte route)\n".to_string(),
        ),
    }
}

// ----------------------------------------------------------------
// updates over the phextnet — release / update (v1.1.0)
//
// A release is announced as a SIGNED SCROLL: the manifest record
// in the ledgers homeland (cluster: releases) of the signer's
// state.phext. The manifest is the trust object — what travels
// the phextnet and what gets verified. The binary itself is
// bulk bytes beside the state file (releases/qexo-<version>),
// served over the wire; a multi-MB base64 scroll inside every
// node's state would be the wrong shape, and integrity never
// depends on the transport anyway: a spoofed endpoint cannot
// forge the signer's signature, and the manifest's sha256
// judges the bytes.
//
// THE TRUST RULE (config record, 9.9.9 homeland): `qexo update`
// applies a release only when (1) the manifest's detached
// signature verifies, and the fingerprint it verifies under IS
// the config record's `release_signer` AND that fingerprint's
// key stands in this node's peer registry; (2) the offered
// version is strictly newer than the running binary; (3) the
// downloaded bytes hash to the manifest's sha256. Anything else
// is a refusal with the reason printed. An offer that CLAIMS
// the trusted signer but fails verification is a hard stop with
// a ledger receipt — someone is impersonating the signer, and
// the node does not shop around. Offers signed by anyone else
// (a peer announcing its own release) are noted and ignored:
// every node's trust is its own config record's word.
// ----------------------------------------------------------------

/// A parsed release manifest (the record's field lines).
#[derive(Debug, Clone, PartialEq)]
struct Manifest {
    version: String,
    sha256: String,
    size_bytes: u64,
    signer: String,
    binary: String,
}

fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let version = lattice::field(text, "version").filter(|s| !s.is_empty()).ok_or("manifest has no version field")?;
    let sha256 = lattice::field(text, "sha256").filter(|s| !s.is_empty()).ok_or("manifest has no sha256 field")?;
    let size_bytes: u64 = lattice::field(text, "size_bytes")
        .and_then(|s| s.parse().ok())
        .ok_or("manifest has no usable size_bytes field")?;
    let signer = lattice::field(text, "signer").filter(|s| !s.is_empty()).ok_or("manifest has no signer field")?;
    let binary = lattice::field(text, "binary").unwrap_or_else(|| format!("qexo-{}", version));
    Ok(Manifest { version, sha256, size_bytes, signer, binary })
}

/// Compose the manifest record text for a release. This exact
/// text — every byte, trailing newline included — is what the
/// signer signs and what verifiers check the signature against:
/// the scroll IS the canonical form.
fn compose_manifest(version: &str, sha256: &str, size_bytes: u64, built_from: &str, signer: &str, stamp: &str, binary: &str, sig_coord: &str) -> String {
    lattice::record(&[
        ("kind", "release announcement"),
        ("version", version),
        ("sha256", sha256),
        ("size_bytes", &size_bytes.to_string()),
        ("built_from", built_from),
        ("signer", signer),
        ("announced", stamp),
        ("binary", binary),
        ("signature", sig_coord),
    ])
}

/// Dotted-numeric version triple ("1.1.0" or "v1.1.0").
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let mut parts = v.split('.');
    let a: u64 = parts.next()?.parse().ok()?;
    let b: u64 = parts.next()?.parse().ok()?;
    let c: u64 = parts.next().map(|p| p.parse().unwrap_or(0)).unwrap_or(0);
    if parts.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

/// What an evaluated offer amounts to. Pure decision logic —
/// the caller does the fetching and the signature mechanics,
/// this judges the facts.
#[derive(Debug, PartialEq)]
enum Assess {
    /// Trusted signer, strictly newer: applicable.
    Accept,
    /// Trusted signer, but not newer than what runs.
    AlreadyCurrent,
    /// The offer names some other signer: not this node's
    /// trust, noted and ignored (no receipt, no failure).
    NotOurSigner,
    /// The offer CLAIMS the trusted signer but the facts do
    /// not hold up. Reason attached. Hard stop + receipt.
    Refuse(String),
}

fn assess_offer(claimed_signer: &str, verified: Result<&str, &str>, trusted: &str, offered: &str, current: &str) -> Assess {
    if claimed_signer != trusted {
        return Assess::NotOurSigner;
    }
    let fp = match verified {
        Ok(fp) => fp,
        Err(e) => return Assess::Refuse(format!("signature does not verify ({})", e)),
    };
    if fp != trusted {
        return Assess::Refuse(format!(
            "manifest claims signer {} but its signature verifies under {}",
            lattice::fp_short(claimed_signer),
            lattice::fp_short(fp)
        ));
    }
    match (parse_version(offered), parse_version(current)) {
        (Some(o), Some(c)) => {
            if o > c {
                Assess::Accept
            } else {
                Assess::AlreadyCurrent
            }
        }
        _ => Assess::Refuse(format!("unusable version numbers (offered {:?}, running {:?})", offered, current)),
    }
}

/// Minimal HTTP/1.1 GET over TcpStream — the phextnet's wire.
/// http:// only: the net it rides is the tailnet, and integrity
/// is the signature's business, never the transport's.
fn http_get(url: &str) -> Result<(u16, Vec<u8>), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{}: only http:// endpoints are pollable (integrity is the signature's, not the transport's)", url))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let mut stream = TcpStream::connect(hostport).map_err(|e| format!("connect {}: {}", hostport, e))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, hostport);
    stream.write_all(req.as_bytes()).map_err(|e| format!("write {}: {}", hostport, e))?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| format!("read {}: {}", hostport, e))?;
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
    Ok((status, raw[split + 4..].to_vec()))
}

/// A receipt for an update attempt, in the receipts run —
/// update outcomes are ledger facts, applied or refused alike.
/// (Deliberately NOT written for: no signer configured, signer
/// unregistered, no offer, already current, or --check. A
/// timer polling an up-to-date node must not spam the ledger.)
fn update_receipt(lat: &mut Lattice, verdict: &str, extra: &[(&str, &str)]) {
    let mut fields: Vec<(&str, &str)> = vec![("verdict", verdict)];
    fields.extend_from_slice(extra);
    append_receipt(&mut lat.px, "update", &fields);
    lattice::regen_indexes(&mut lat.px);
    let _ = lat.save();
}

/// `qexo release` — on the signer: package the running binary
/// into the releases dir, write + sign the manifest scroll,
/// print what was announced.
pub fn release() -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let fp = match lat.px.get(&lattice::NODE_KEY_RECORD).and_then(|t| lattice::field(t, "fingerprint")) {
        Some(f) if !f.is_empty() => f,
        _ => {
            eprintln!("qexo release: this node has no node key — run `qexo config` first");
            return 1;
        }
    };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("qexo release: cannot locate the running binary: {}", e);
            return 1;
        }
    };
    let bytes = match std::fs::read(&exe) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("qexo release: cannot read {}: {}", exe.display(), e);
            return 1;
        }
    };
    let version = env!("CARGO_PKG_VERSION");
    let sha = hash::sha256_hex(&bytes);
    let size = bytes.len() as u64;
    let built_from = option_env!("QEXO_COMMIT").unwrap_or("unstamped");
    let binary_name = format!("qexo-{}", version);

    let dir = lattice::state_dir().join("releases");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("qexo release: cannot create {}: {}", dir.display(), e);
        return 1;
    }
    if let Err(e) = std::fs::write(dir.join(&binary_name), &bytes) {
        eprintln!("qexo release: cannot stage {}: {}", dir.join(&binary_name).display(), e);
        return 1;
    }

    let book = lattice::next_free_book(&lat.px, [6, 6, 6, lattice::LEDGER_RELEASES, 1], 1);
    let maddr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RELEASES, 1, book, 1, 1, 1]);
    let saddr = lattice::at(lattice::HL_LEDGERS, [lattice::LEDGER_RELEASES, 1, book, 1, 1, 2]);
    let (_, stamp) = now_stamp();
    let manifest = compose_manifest(version, &sha, size, built_from, &fp, &stamp, &binary_name, &fmt_coord(&saddr));
    let gpg = Gpg::open_in(&lattice::state_dir());
    let signature = match gpg.sign_detached(&fp, manifest.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("qexo release: signing the manifest failed: {}", e);
            return 1;
        }
    };
    lat.px.insert(maddr, manifest);
    lat.px.insert(saddr, signature.trim_end().to_string() + "\n");
    let receipt = append_receipt(
        &mut lat.px,
        "release",
        &[
            ("version", version),
            ("sha256", &sha),
            ("signer", &fp),
            ("manifest", &fmt_coord(&maddr)),
        ],
    );
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo release: cannot save state: {}", e);
        return 1;
    }
    println!("release announced: qexo {}", version);
    println!("manifest: {} (signature beside it, {})", fmt_coord(&maddr), fmt_coord(&saddr));
    println!("signer: {}", fp);
    println!("sha256: {}", sha);
    println!("binary: {} ({} bytes, served at /release/{})", dir.join(&binary_name).display(), size, binary_name);
    println!("receipt: {}", fmt_coord(&receipt));
    0
}

/// How an update pass is being run: Apply and Check are
/// `qexo update`'s own modes (its prints and ledger receipts
/// stand as they have since v1.1.0); JudgeOnly is evolve's
/// --check borrowing the same eyes — identical polling and
/// verification, but mute and writeless: no prints of its own,
/// no receipts, never an application, so a dry pass over the
/// node changes nothing on disk.
#[derive(Clone, Copy, PartialEq)]
enum UpdateMode {
    Apply,
    Check,
    JudgeOnly,
}

/// What an update pass concluded, in facts another caller
/// (evolve's receipt) can cite. `qexo update` maps these back
/// to the exit codes it has always returned.
#[derive(Debug, PartialEq)]
enum UpdateOutcome {
    Applied { from: String, to: String },
    RolledBack { from: String, to: String },
    AlreadyCurrent { offered: String },
    Available { from: String, to: String, via: String },
    NoOffer,
    IgnoredOffers,
    Refused(String),
    NoSigner,
    SignerUnregistered,
}

/// `qexo update [--check]` — poll the trusted peers, judge
/// every offer by the trust rule, and apply the trusted one.
pub fn update(check_only: bool) -> i32 {
    let mode = if check_only { UpdateMode::Check } else { UpdateMode::Apply };
    match update_inner(mode) {
        // The exit codes are v1.1.0's, exactly: a judged pass
        // that applied, found nothing newer, or found nothing to
        // apply is 0; refusals, rollbacks, and the two trust
        // preconditions are 1.
        Ok(outcome) => match outcome {
            UpdateOutcome::Applied { .. }
            | UpdateOutcome::AlreadyCurrent { .. }
            | UpdateOutcome::Available { .. }
            | UpdateOutcome::NoOffer
            | UpdateOutcome::IgnoredOffers => 0,
            _ => 1,
        },
        Err(code) => code,
    }
}

/// The update pass itself. Err(code) is a state-level failure
/// (unborn / unreadable state) whose message require_born
/// already printed; Ok(outcome) is a judged result.
fn update_inner(mode: UpdateMode) -> Result<UpdateOutcome, i32> {
    let loud = mode != UpdateMode::JudgeOnly;
    let receipts = mode != UpdateMode::JudgeOnly;
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return Err(c),
    };
    let cfg_text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_default();
    let trusted = match lattice::field(&cfg_text, "release_signer").filter(|s| !s.is_empty()) {
        Some(fp) => fp,
        None => {
            if loud {
                eprintln!(
                    "qexo update: no trusted release signer configured — set `release_signer: <fingerprint>` in the config record ({}) to the node whose signed releases this node trusts. Nothing was polled and nothing changed.",
                    fmt_coord(&lattice::CONFIG_RECORD)
                );
            }
            return Ok(UpdateOutcome::NoSigner);
        }
    };
    // Only TRUSTED peers are ever polled: a discovered record
    // (gossip's knowledge) is not in this set, and the signer
    // must stand in it as registered — a fingerprint learned
    // second-hand grants nothing.
    let peers = trusted_peer_records(&lat.px);
    if !peers.iter().any(|(_, _, t)| lattice::field(t, "fingerprint").as_deref() == Some(trusted.as_str())) {
        if loud {
            eprintln!(
                "qexo update: release signer {} is not in this node's peer registry — register their public key first (`qexo register <pubkey> --endpoint <url>`). No key, no trust: nothing was polled and nothing changed.",
                trusted
            );
        }
        return Ok(UpdateOutcome::SignerUnregistered);
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    let gpg = Gpg::open_in(&lattice::state_dir());
    let mut saw_offer = false;
    let mut current_offer: Option<String> = None;

    for (_, _, text) in &peers {
        // Human output names the peer and shows its endpoint by
        // name where a name exists; the recorded endpoint below
        // is what is actually dialed, untouched by display.
        let dname = peer_display_name(text);
        let endpoint = match lattice::field(text, "endpoint").filter(|s| !s.is_empty()) {
            Some(e) => e.trim_end_matches('/').to_string(),
            None => {
                if loud {
                    println!(
                        "peer {}: no endpoint recorded — not pollable (re-register with --endpoint <url> to add one)",
                        dname
                    );
                }
                continue;
            }
        };
        let shown = peer_display_endpoint(text).unwrap_or_else(|| endpoint.clone());
        let manifest_bytes = match http_get(&format!("{}/v1/release", endpoint)) {
            Ok((200, b)) => b,
            Ok((404, _)) => {
                if loud {
                    println!("peer {} ({}): no release announced", dname, shown);
                }
                continue;
            }
            Ok((s, _)) => {
                if loud {
                    println!("peer {} ({}): /v1/release answered HTTP {} — skipped", dname, shown, s);
                }
                continue;
            }
            Err(e) => {
                if loud {
                    println!("peer {} ({}): unreachable — {}", dname, shown, e);
                }
                continue;
            }
        };
        saw_offer = true;
        let manifest_text = String::from_utf8_lossy(&manifest_bytes).to_string();
        let manifest = match parse_manifest(&manifest_text) {
            Ok(m) => m,
            Err(e) => {
                if loud {
                    println!("peer {} ({}): unusable manifest — {} — skipped", dname, shown, e);
                }
                continue;
            }
        };
        // Whose release is this, by its own word? Another node's
        // own announcement is not this node's trust — note and
        // move on WITHOUT fetching its signature or binary.
        if manifest.signer != trusted {
            if loud {
                println!(
                    "peer {} offers qexo {} signed by {} — not this node's release signer ({}); ignored",
                    dname,
                    manifest.version,
                    lattice::fp_short(&manifest.signer),
                    lattice::fp_short(&trusted)
                );
            }
            continue;
        }
        // The offer CLAIMS the trusted signer. Verify before
        // anything is downloaded: a failure here is a hard stop.
        let sig_text = match http_get(&format!("{}/v1/release/signature", endpoint)) {
            Ok((200, b)) => String::from_utf8_lossy(&b).to_string(),
            Ok((s, _)) => {
                let why = format!("the manifest's signature could not be fetched (HTTP {})", s);
                if receipts {
                    update_receipt(
                        &mut lat,
                        "refused",
                        &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                    );
                }
                if loud {
                    eprintln!("qexo update: release from {} claims the trusted signer but {}", shown, why);
                }
                return Ok(UpdateOutcome::Refused(why));
            }
            Err(e) => {
                let why = format!("the manifest's signature could not be fetched ({})", e);
                if receipts {
                    update_receipt(
                        &mut lat,
                        "refused",
                        &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                    );
                }
                if loud {
                    eprintln!("qexo update: release from {} claims the trusted signer but {}", shown, why);
                }
                return Ok(UpdateOutcome::Refused(why));
            }
        };
        let verified = gpg.verify_detached(&sig_text, manifest_text.as_bytes());
        let verdict = assess_offer(
            &manifest.signer,
            verified.as_ref().map(|s| s.as_str()).map_err(|e| e.as_str()),
            &trusted,
            &manifest.version,
            &current,
        );
        match verdict {
            Assess::NotOurSigner => unreachable!("signer equality checked above"),
            Assess::Refuse(why) => {
                if receipts {
                    update_receipt(
                        &mut lat,
                        "refused",
                        &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                    );
                }
                if loud {
                    eprintln!(
                        "qexo update: REFUSED — the release offered by {} claims signer {} but {} — nothing was downloaded and the running binary is untouched",
                        shown, lattice::fp_short(&trusted), why
                    );
                }
                return Ok(UpdateOutcome::Refused(why));
            }
            Assess::AlreadyCurrent => {
                current_offer = Some(manifest.version.clone());
            }
            Assess::Accept => {
                if mode != UpdateMode::Apply {
                    if mode == UpdateMode::Check {
                        println!(
                            "update available: {} → {} (signer {}, via {}) — run `qexo update` to apply",
                            current, manifest.version, lattice::fp_short(&trusted), shown
                        );
                    }
                    return Ok(UpdateOutcome::Available {
                        from: current.clone(),
                        to: manifest.version.clone(),
                        via: endpoint.clone(),
                    });
                }
                let (_code, outcome) = apply_update(&mut lat, &endpoint, &manifest, &trusted, &current);
                return Ok(outcome);
            }
        }
    }

    if let Some(offered) = current_offer {
        if loud {
            println!("already current — this node runs qexo {}; the trusted signer offers {}", current, offered);
        }
        Ok(UpdateOutcome::AlreadyCurrent { offered })
    } else if saw_offer {
        if loud {
            println!("qexo update: offers were seen, none from this node's release signer — nothing applied");
        }
        Ok(UpdateOutcome::IgnoredOffers)
    } else {
        if loud {
            println!("qexo update: no peer offered a release manifest — nothing applied");
        }
        Ok(UpdateOutcome::NoOffer)
    }
}

/// Download, verify, swap, post-check, roll back on failure.
/// The caller has already established trust (signature verified
/// against the registry key named by `release_signer`) and
/// novelty (strictly newer). What remains is the bytes' own
/// verdict: size + sha256 against the manifest, then the swap.
/// Returns the exit code `qexo update` has always returned,
/// beside the outcome in facts.
fn apply_update(lat: &mut Lattice, endpoint: &str, manifest: &Manifest, trusted: &str, current: &str) -> (i32, UpdateOutcome) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("qexo update: cannot locate the running binary: {}", e);
            return (1, UpdateOutcome::Refused(format!("cannot locate the running binary: {}", e)));
        }
    };
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let bytes = match http_get(&format!("{}/release/{}", endpoint, manifest.binary)) {
        Ok((200, b)) => b,
        Ok((s, _)) => {
            let why = format!("binary download answered HTTP {}", s);
            update_receipt(
                lat,
                "refused",
                &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
            );
            eprintln!("qexo update: REFUSED — {}", why);
            return (1, UpdateOutcome::Refused(why));
        }
        Err(e) => {
            let why = format!("binary download failed: {}", e);
            update_receipt(
                lat,
                "refused",
                &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
            );
            eprintln!("qexo update: REFUSED — {}", why);
            return (1, UpdateOutcome::Refused(why));
        }
    };
    let tmp = dir.join("qexo.update-tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        eprintln!(
            "qexo update: cannot stage the download beside the running binary ({}): {} — the binary's directory must be writable for a self-update",
            tmp.display(),
            e
        );
        return (1, UpdateOutcome::Refused(format!("cannot stage the download beside the running binary: {}", e)));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    // The bytes give their own verdict against the signed words.
    let got_sha = hash::sha256_hex(&bytes);
    if bytes.len() as u64 != manifest.size_bytes || got_sha != manifest.sha256 {
        let why = format!(
            "the served bytes fail the manifest: size {} (manifest {}), sha256 {} (manifest {})",
            bytes.len(),
            manifest.size_bytes,
            got_sha,
            manifest.sha256
        );
        let _ = std::fs::remove_file(&tmp);
        update_receipt(
            lat,
            "refused",
            &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
        );
        eprintln!("qexo update: REFUSED — {} — the running binary is untouched", why);
        return (1, UpdateOutcome::Refused(why));
    }
    // The swap: current aside as qexo.prev, the verified bytes
    // into place — renames within one directory, atomic each.
    let prev = dir.join("qexo.prev");
    if prev.exists() {
        let _ = std::fs::remove_file(&prev);
    }
    if let Err(e) = std::fs::rename(&exe, &prev) {
        let _ = std::fs::remove_file(&tmp);
        eprintln!("qexo update: cannot set the running binary aside ({}): {}", prev.display(), e);
        return (1, UpdateOutcome::Refused(format!("cannot set the running binary aside: {}", e)));
    }
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::rename(&prev, &exe);
        let why = format!("swap failed ({}); previous binary restored", e);
        update_receipt(
            lat,
            "rolled back",
            &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
        );
        eprintln!("qexo update: {}", why);
        return (
            1,
            UpdateOutcome::RolledBack { from: current.to_string(), to: manifest.version.clone() },
        );
    }
    // Post-check: the new binary must speak its own version and
    // pass doctor against this node's state. Anything less and
    // qexo.prev comes back — a bad update never stands.
    let post = post_update_check(&exe, &manifest.version);
    match post {
        Ok(()) => {
            update_receipt(
                lat,
                "applied",
                &[("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
            );
            println!("updated: qexo {} → {} (signer {})", current, manifest.version, lattice::fp_short(trusted));
            println!("sha256 verified: {}", manifest.sha256);
            println!("previous binary kept at {}", prev.display());
            (
                0,
                UpdateOutcome::Applied { from: current.to_string(), to: manifest.version.clone() },
            )
        }
        Err(why) => {
            let _ = std::fs::remove_file(&exe);
            let restored = std::fs::rename(&prev, &exe).is_ok();
            update_receipt(
                lat,
                "rolled back",
                &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
            );
            eprintln!(
                "qexo update: the new binary failed its post-check ({}) — {}",
                why,
                if restored { "qexo.prev restored; the node still runs the previous binary" } else { "RESTORE FAILED — qexo.prev is still beside the binary; move it back by hand" }
            );
            (
                1,
                UpdateOutcome::RolledBack { from: current.to_string(), to: manifest.version.clone() },
            )
        }
    }
}

// ----------------------------------------------------------------
// evolve — one receipted convergence pass over the phextnet (v1.2.0)
//
// `update` moves the binary; `evolve` is the node's full
// convergence pass — everything a node can learn from its
// peers, applied in one receipted cycle:
//   1. peer gossip — what trusted peers know about the
//      network, recorded as knowledge, never as trust;
//   2. persona sync — copies of this node's personas held by
//      trusted peers: adopted when they descend from ours,
//      kept beside ours when the lines fork;
//   3. self-update — the v1.1.0 mechanism, invoked in-process;
//   4. one evolution receipt in the ledgers homeland naming
//      every stage's outcome.
// Each stage fails independently — a stage's failure never
// blocks the next. --check performs every read and prints the
// plan, and writes nothing at all: the knowledge stages run
// against a throwaway copy of the lattice and the binary
// stage only judges.
// ----------------------------------------------------------------

/// This node's own key fingerprint, from its node-key record
/// ("" when the node has no key — a born node always has one).
fn own_fingerprint(px: &Phext) -> String {
    px.get(&lattice::NODE_KEY_RECORD)
        .and_then(|t| lattice::field(t, "fingerprint"))
        .unwrap_or_default()
}

/// A peer as a reporting peer's index + records describe it.
#[derive(Debug, PartialEq)]
struct ReportedPeer {
    name: String,
    fingerprint: Option<String>,
    endpoint: Option<String>,
    /// The reported record's host: field, when it carries one —
    /// carried into a discovered record so the new knowledge is
    /// displayable by name from the moment it lands.
    host: Option<String>,
}

/// What gossip makes of one reported peer.
#[derive(Debug, PartialEq)]
enum GossipVerdict {
    /// The report is about this node itself.
    SkipSelf,
    /// The report confirms a trusted peer (fingerprint agrees).
    Confirm,
    /// Already recorded as discovered; nothing to add.
    Known,
    /// A known NAME with a different fingerprint, or a known
    /// FINGERPRINT under a different name: a conflict — warn,
    /// never overwrite.
    Conflict(String),
    /// New to this node: record as discovered.
    Discover,
}

/// The gossip trust rule, in one pure function: discovery is
/// knowledge, not trust. A report can confirm what the
/// operator already registered, or add an untrusted record —
/// it can never promote, demote, or overwrite anything, and a
/// fingerprint conflict for a known name is a warning, never
/// a silent overwrite.
fn gossip_verdict(
    via: &str,
    own_fp: &str,
    trusted: &[(String, String)],
    discovered: &[(String, String)],
    rep: &ReportedPeer,
) -> GossipVerdict {
    let rep_fp = rep.fingerprint.clone().unwrap_or_default();
    if !rep_fp.is_empty() && rep_fp == own_fp {
        return GossipVerdict::SkipSelf;
    }
    for (tname, tfp) in trusted {
        if *tname == rep.name {
            return if !rep_fp.is_empty() && rep_fp == *tfp {
                GossipVerdict::Confirm
            } else {
                GossipVerdict::Conflict(format!(
                    "gossip via {}: {} reported with fingerprint {} — this node's trusted record says {} — report not adopted, record untouched",
                    via,
                    rep.name,
                    lattice::fp_short(&rep_fp),
                    lattice::fp_short(tfp)
                ))
            };
        }
        if !rep_fp.is_empty() && rep_fp == *tfp {
            return GossipVerdict::Conflict(format!(
                "gossip via {}: fingerprint {} reported under the name {} — this node trusts that fingerprint as {} — report not adopted, record untouched",
                via,
                lattice::fp_short(&rep_fp),
                rep.name,
                tname
            ));
        }
    }
    for (dname, dfp) in discovered {
        if *dname == rep.name || (!rep_fp.is_empty() && rep_fp == *dfp) {
            return if rep_fp == *dfp && *dname == rep.name {
                GossipVerdict::Known
            } else {
                GossipVerdict::Conflict(format!(
                    "gossip via {}: {} reported with fingerprint {} — the discovered record on file says {} ({}) — report not adopted, record untouched",
                    via,
                    rep.name,
                    lattice::fp_short(&rep_fp),
                    lattice::fp_short(dfp),
                    dname
                ))
            };
        }
    }
    GossipVerdict::Discover
}

/// One pass's facts, accumulated stage by stage and written
/// into the evolution receipt whole.
#[derive(Default)]
struct EvolveReport {
    peers_polled: usize,
    peers_learned: Vec<String>,
    peers_confirmed: usize,
    personas_considered: usize,
    personas_adopted: Vec<String>,
    divergences_kept: Vec<String>,
    personas_ahead: usize,
    binary: String,
    binary_bad: bool,
    warnings: Vec<String>,
}

/// Stage 1 — peer gossip. For each TRUSTED peer with an
/// endpoint: read its peers index, fetch each record it names,
/// and judge the reports. Mutates the given lattice (the real
/// one in a live pass, a throwaway copy under --check).
fn gossip_stage(px: &mut Phext, report: &mut EvolveReport) {
    let own_fp = own_fingerprint(px);
    let trusted_list: Vec<(String, String)> = trusted_peer_records(px)
        .iter()
        .filter_map(|(_, _, t)| Some((lattice::field(t, "name")?, lattice::field(t, "fingerprint").filter(|s| !s.is_empty())?)))
        .collect();
    let poll: Vec<(String, String)> = trusted_peer_records(px)
        .iter()
        .filter_map(|(_, _, t)| {
            Some((
                lattice::field(t, "name")?,
                lattice::field(t, "endpoint").filter(|s| !s.is_empty())?.trim_end_matches('/').to_string(),
            ))
        })
        .collect();
    for (pname, endpoint) in &poll {
        report.peers_polled += 1;
        let index_url = format!("{}/v1/scroll/{}", endpoint, fmt_coord(&lattice::index_addr(lattice::HL_PEERS)));
        let index_text = match http_get(&index_url) {
            Ok((200, b)) => String::from_utf8_lossy(&b).to_string(),
            Ok((s, _)) => {
                report.warnings.push(format!(
                    "gossip: peer {} ({}) answered its peers index with HTTP {} — skipped",
                    pname,
                    display_endpoint_for(pname, endpoint),
                    s
                ));
                continue;
            }
            Err(e) => {
                report.warnings.push(format!(
                    "gossip: peer {} ({}) unreachable — {}",
                    pname,
                    display_endpoint_for(pname, endpoint),
                    e
                ));
                continue;
            }
        };
        // The peers index names its records, one per line:
        // "- <name> — <coord> — status <s>". Fetch each record
        // by coordinate from the same peer and judge it.
        let mut reported: Vec<ReportedPeer> = Vec::new();
        for line in index_text.lines() {
            let line = line.trim();
            if !line.starts_with("- ") {
                continue;
            }
            let parts: Vec<&str> = line.split(" — ").collect();
            if parts.len() < 2 {
                continue;
            }
            let rname = parts[0].trim_start_matches("- ").trim().to_string();
            let rcoord = match crate::phext::parse_coord(parts[1].trim()) {
                Some(c) => c,
                None => continue,
            };
            let rec_url = format!("{}/v1/scroll/{}", endpoint, fmt_coord(&rcoord));
            match http_get(&rec_url) {
                Ok((200, b)) => {
                    let t = String::from_utf8_lossy(&b).to_string();
                    reported.push(ReportedPeer {
                        name: lattice::field(&t, "name").unwrap_or(rname),
                        fingerprint: lattice::field(&t, "fingerprint").filter(|s| !s.is_empty()),
                        endpoint: lattice::field(&t, "endpoint").filter(|s| !s.is_empty()),
                        host: lattice::field(&t, "host").filter(|s| !s.is_empty()),
                    });
                }
                Ok((s, _)) => report.warnings.push(format!(
                    "gossip: peer {} lists {} but its record answered HTTP {} — skipped",
                    pname, rname, s
                )),
                Err(e) => report.warnings.push(format!(
                    "gossip: peer {} lists {} but its record is unreachable — {}",
                    pname, rname, e
                )),
            }
        }
        for rep in &reported {
            // Judged against the registry as it stands NOW —
            // records learned earlier in this same pass count.
            let discovered_list: Vec<(String, String)> = peer_records(px)
                .iter()
                .filter(|(_, _, t)| lattice::field(t, "status").as_deref() == Some("discovered"))
                .filter_map(|(_, _, t)| Some((lattice::field(t, "name")?, lattice::field(t, "fingerprint")?)))
                .collect();
            match gossip_verdict(pname, &own_fp, &trusted_list, &discovered_list, rep) {
                GossipVerdict::SkipSelf | GossipVerdict::Known => {}
                GossipVerdict::Confirm => report.peers_confirmed += 1,
                GossipVerdict::Conflict(msg) => report.warnings.push(msg),
                GossipVerdict::Discover => {
                    let c = lattice::next_free_cluster(px, lattice::HL_PEERS, 2);
                    let addr = lattice::at(lattice::HL_PEERS, [c, 1, 1, 1, 1, 1]);
                    let (_, stamp) = now_stamp();
                    let mut fields: Vec<(&str, &str)> = vec![
                        ("kind", "peer"),
                        ("name", &rep.name),
                        ("fingerprint", rep.fingerprint.as_deref().unwrap_or("")),
                        ("status", "discovered"),
                        ("via", pname),
                        ("discovered", &stamp),
                    ];
                    if let Some(h) = &rep.host {
                        fields.push(("host", h));
                    }
                    if let Some(ep) = &rep.endpoint {
                        fields.push(("endpoint", ep));
                    }
                    px.insert(addr, lattice::record(&fields));
                    report.peers_learned.push(format!("{} via {} at {}", rep.name, pname, fmt_coord(&addr)));
                }
            }
        }
    }
}

/// Stage 2 — persona sync. For each persona this node holds,
/// poll each TRUSTED peer for its copy (found through the
/// peer's own personas index, never by guessing coordinates)
/// and judge the two copies by their lineages.
fn persona_stage(px: &mut Phext, report: &mut EvolveReport) {
    let mut mine: Vec<(u32, String)> = Vec::new();
    for r in lattice::records(px) {
        if r.kind == "persona" {
            mine.push((r.coord[3], r.name.clone()));
        }
    }
    if mine.is_empty() {
        return;
    }
    let poll: Vec<(String, String)> = trusted_peer_records(px)
        .iter()
        .filter_map(|(_, _, t)| {
            Some((
                lattice::field(t, "name")?,
                lattice::field(t, "endpoint").filter(|s| !s.is_empty())?.trim_end_matches('/').to_string(),
            ))
        })
        .collect();
    for (cluster, name) in &mine {
        report.personas_considered += 1;
        for (pname, endpoint) in &poll {
            // Re-read the local copy each time: an adoption
            // earlier in this pass changes what "local" means
            // for the next peer.
            let local_rec = match px.get(&persona_addr(*cluster, 1, 1)) {
                Some(t) => t.clone(),
                None => continue,
            };
            let local_boot = boot_text_of(px, *cluster).unwrap_or_default();
            let local = copy_view(&local_rec, &local_boot);
            let idx_url = format!("{}/v1/scroll/{}", endpoint, fmt_coord(&lattice::index_addr(lattice::HL_PERSONAS)));
            let idx_text = match http_get(&idx_url) {
                Ok((200, b)) => String::from_utf8_lossy(&b).to_string(),
                Ok((s, _)) => {
                    report.warnings.push(format!(
                        "persona sync: peer {} answered its personas index with HTTP {} — skipped for {}",
                        pname, s, name
                    ));
                    continue;
                }
                Err(e) => {
                    report.warnings.push(format!(
                        "persona sync: peer {} unreachable — {} — skipped for {}",
                        pname, e, name
                    ));
                    continue;
                }
            };
            let remote_cluster = idx_text.lines().find_map(|line| {
                let parts: Vec<&str> = line.split(" — ").collect();
                if parts.len() >= 2 && parts[0].trim_start_matches("- ").trim() == name.as_str() {
                    crate::phext::parse_coord(parts[1].trim()).map(|c| c[3])
                } else {
                    None
                }
            });
            let rc = match remote_cluster {
                Some(c) => c,
                None => continue,
            };
            let rec_url = format!("{}/v1/scroll/{}", endpoint, fmt_coord(&persona_addr(rc, 1, 1)));
            let remote_rec = match http_get(&rec_url) {
                Ok((200, b)) => String::from_utf8_lossy(&b).to_string(),
                Ok((s, _)) => {
                    report.warnings.push(format!(
                        "persona sync: peer {} lists {} but its record answered HTTP {} — skipped",
                        pname, name, s
                    ));
                    continue;
                }
                Err(e) => {
                    report.warnings.push(format!(
                        "persona sync: peer {} lists {} but its record is unreachable — {}",
                        pname, name, e
                    ));
                    continue;
                }
            };
            if lattice::field(&remote_rec, "kind").as_deref() != Some("persona") {
                continue;
            }
            // The remote boot run: chunk scrolls until a gap.
            let mut chunks: Vec<String> = Vec::new();
            let mut s = 1u32;
            loop {
                let u = format!("{}/v1/scroll/{}", endpoint, fmt_coord(&persona_addr(rc, 2, s)));
                match http_get(&u) {
                    Ok((200, b)) => {
                        chunks.push(String::from_utf8_lossy(&b).to_string());
                        s += 1;
                    }
                    _ => break,
                }
                if s > 4096 {
                    break;
                }
            }
            if chunks.is_empty() {
                continue;
            }
            let remote_boot = chunks.concat();
            let remote = copy_view(&remote_rec, &remote_boot);
            match sync_decision(&local, &remote) {
                SyncDecision::Same => {}
                SyncDecision::Behind => report.personas_ahead += 1,
                SyncDecision::Adopt => {
                    // Adopt whole: the remote record verbatim
                    // (its lineage IS the adoption's proof) and
                    // its boot run in its own chunking.
                    px.insert(persona_addr(*cluster, 1, 1), remote_rec.clone());
                    let old: Vec<crate::phext::Coord> = lattice::prefix_scrolls(px, &[4, 4, 4, *cluster, 1, 2, 1, 1])
                        .iter()
                        .map(|(a, _)| *a)
                        .collect();
                    for a in old {
                        px.scrolls.remove(&a);
                    }
                    for (i, ch) in chunks.iter().enumerate() {
                        px.insert(persona_addr(*cluster, 2, (i + 1) as u32), ch.clone());
                    }
                    report.personas_adopted.push(format!(
                        "{} revision {} → {} from {} (it follows {})",
                        name, local.revision, remote.revision, pname, remote.follows
                    ));
                }
                SyncDecision::Diverge => {
                    // KEEP BOTH — the KSO divergence ruling:
                    // the incoming copy lands beside ours as an
                    // attempt artifact named by its origin node.
                    // Ours is untouched; nothing is dropped
                    // silently; a copy already kept (same stamp)
                    // is not kept twice.
                    let mut already = false;
                    for b in lattice::component_values(px, &[4, 4, 4, *cluster, 1], 5) {
                        if let Some(t) = px.get(&persona_addr(*cluster, b, 1)) {
                            if lattice::field(t, "kind").as_deref() == Some("persona attempt")
                                && lattice::field(t, "stamp").as_deref() == Some(remote.stamp.as_str())
                            {
                                already = true;
                            }
                        }
                    }
                    if !already {
                        let b = lattice::next_free_book(px, [4, 4, 4, *cluster, 1], 5);
                        let (_, tstamp) = now_stamp();
                        px.insert(
                            persona_addr(*cluster, b, 1),
                            lattice::record(&[
                                ("kind", "persona attempt"),
                                ("persona", name),
                                ("origin", pname),
                                ("revision", &remote.revision.to_string()),
                                ("follows", &remote.follows),
                                ("history", &remote.history.join(",")),
                                ("stamp", &remote.stamp),
                                ("received", &tstamp),
                                ("note", "divergent copy kept under the KSO ruling — both lines stand; the resident copy is unchanged"),
                            ]),
                        );
                        for (i, ch) in chunks.iter().enumerate() {
                            px.insert(persona_addr(*cluster, b, (i + 2) as u32), ch.clone());
                        }
                        report.divergences_kept.push(format!(
                            "{}: {}'s revision {} (stamp {}) kept as attempt at {} — local revision {} (stamp {}) unchanged",
                            name,
                            pname,
                            remote.revision,
                            remote.stamp,
                            fmt_coord(&persona_addr(*cluster, b, 1)),
                            local.revision,
                            local.stamp
                        ));
                    }
                }
            }
        }
    }
}

/// Fold the binary stage's outcome into the report, in words
/// the receipt can carry.
fn set_binary_outcome(report: &mut EvolveReport, outcome: &UpdateOutcome) {
    report.binary = match outcome {
        UpdateOutcome::Applied { from, to } => format!("updated {} → {}", from, to),
        UpdateOutcome::RolledBack { from, to } => format!("update to {} FAILED and rolled back — still {}", to, from),
        UpdateOutcome::AlreadyCurrent { offered } => format!("already current (the signer offers {})", offered),
        UpdateOutcome::Available { from, to, via } => format!("update available: {} → {} via {} (not applied — check only)", from, to, via),
        UpdateOutcome::NoOffer => "no release offered by any trusted peer".to_string(),
        UpdateOutcome::IgnoredOffers => "offers seen, none from the trusted signer — nothing applied".to_string(),
        UpdateOutcome::Refused(why) => format!("REFUSED — {}", why),
        UpdateOutcome::NoSigner => "no release signer configured — binary stage had nothing to trust".to_string(),
        UpdateOutcome::SignerUnregistered => "release signer not in the peer registry — binary stage had nothing to trust".to_string(),
    };
    report.binary_bad = matches!(outcome, UpdateOutcome::Refused(_) | UpdateOutcome::RolledBack { .. });
}

/// The evolution receipt's fields: every stage, every count,
/// every named outcome, every warning. The pass is not done
/// until its outcomes are on the ledger.
fn evolve_receipt_fields(report: &EvolveReport) -> Vec<(String, String)> {
    let mut f: Vec<(String, String)> = vec![
        ("peers polled".into(), report.peers_polled.to_string()),
        ("peers learned".into(), report.peers_learned.len().to_string()),
        ("peers confirmed".into(), report.peers_confirmed.to_string()),
        ("personas considered".into(), report.personas_considered.to_string()),
        ("personas adopted".into(), report.personas_adopted.len().to_string()),
        ("divergences kept".into(), report.divergences_kept.len().to_string()),
        ("binary".into(), report.binary.clone()),
        ("warnings".into(), report.warnings.len().to_string()),
    ];
    for l in &report.peers_learned {
        f.push(("learned".into(), l.clone()));
    }
    for a in &report.personas_adopted {
        f.push(("adopted".into(), a.clone()));
    }
    for d in &report.divergences_kept {
        f.push(("divergence kept".into(), d.clone()));
    }
    for w in &report.warnings {
        f.push(("warning".into(), w.clone()));
    }
    f
}

/// The pass's summary lines — printed by a live pass above its
/// receipt coordinate, and by --check as the plan.
fn evolve_plan_lines(report: &EvolveReport, check: bool) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "  gossip: {} trusted peer(s) polled, {} learned, {} confirmed\n",
        report.peers_polled,
        report.peers_learned.len(),
        report.peers_confirmed
    ));
    for l in &report.peers_learned {
        s.push_str(&format!("    {} {}\n", if check { "would learn" } else { "learned" }, l));
    }
    s.push_str(&format!(
        "  personas: {} considered, {} adopted, {} divergence(s) kept, {} peer(s) behind us\n",
        report.personas_considered,
        report.personas_adopted.len(),
        report.divergences_kept.len(),
        report.personas_ahead
    ));
    for a in &report.personas_adopted {
        s.push_str(&format!("    adopted {}\n", a));
    }
    for d in &report.divergences_kept {
        s.push_str(&format!("    kept {}\n", d));
    }
    s.push_str(&format!("  binary: {}\n", report.binary));
    for w in &report.warnings {
        s.push_str(&format!("  warning: {}\n", w));
    }
    s
}

/// `qexo evolve [--check]` — one convergence pass over the
/// phextnet. Manual runs always converge, whatever the
/// `evolve` config key says — the key only steers the auto
/// timer's trigger (see `qexo config --evolve`).
pub fn evolve(check_only: bool) -> i32 {
    let lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };

    if check_only {
        // Every read, no writes: the knowledge stages run
        // against a throwaway copy of the lattice and the
        // binary stage only judges. Nothing in this branch
        // touches the disk.
        let mut report = EvolveReport::default();
        let mut px = lat.px.clone();
        gossip_stage(&mut px, &mut report);
        persona_stage(&mut px, &mut report);
        match update_inner(UpdateMode::JudgeOnly) {
            Ok(o) => set_binary_outcome(&mut report, &o),
            Err(_) => report.binary = "state unreadable".to_string(),
        }
        println!("evolve --check — what a pass would do (nothing was written):");
        print!("{}", evolve_plan_lines(&report, true));
        return if report.binary_bad { 1 } else { 0 };
    }

    let mut report = EvolveReport::default();
    let mut work = lat;
    gossip_stage(&mut work.px, &mut report);
    persona_stage(&mut work.px, &mut report);
    lattice::regen_indexes(&mut work.px);
    if let Err(e) = work.save() {
        eprintln!("qexo evolve: cannot save state after the knowledge stages: {}", e);
        return 1;
    }
    // Stage 3 runs the v1.1.0 mechanism in-process; it opens
    // its own view of the (just-saved) state and writes its
    // own receipts, exactly as `qexo update` does.
    match update_inner(UpdateMode::Apply) {
        Ok(o) => set_binary_outcome(&mut report, &o),
        Err(_) => {
            report.binary = "state unreadable".to_string();
            report.binary_bad = true;
        }
    }
    // Reload before receipting: the binary stage's receipts
    // landed in the on-disk state; the evolution receipt joins
    // them there, in the same ledgers family sequence.
    let mut lat2 = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => {
            eprintln!("qexo evolve: state vanished before the evolution receipt could be written");
            return 1;
        }
        Err(e) => {
            eprintln!("qexo evolve: cannot reload state for the evolution receipt: {}", e);
            return 1;
        }
    };
    let fields = evolve_receipt_fields(&report);
    let refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let receipt = append_receipt(&mut lat2.px, "evolve", &refs);
    lattice::regen_indexes(&mut lat2.px);
    if let Err(e) = lat2.save() {
        eprintln!("qexo evolve: cannot save the evolution receipt: {}", e);
        return 1;
    }
    println!("evolve — one convergence pass over the phextnet:");
    print!("{}", evolve_plan_lines(&report, false));
    println!("receipt: {}", fmt_coord(&receipt));
    if report.binary_bad { 1 } else { 0 }
}

/// Post-swap check: `--version` must name the new version and
/// `doctor` must pass against this node's (inherited) state.
fn post_update_check(exe: &Path, version: &str) -> Result<(), String> {
    let out = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .map_err(|e| format!("cannot run the new binary: {}", e))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() || !text.contains(&format!("v{}", version)) {
        return Err(format!("version stamp reads {:?}, expected qexo v{}", text.trim(), version));
    }
    let doc = std::process::Command::new(exe)
        .arg("doctor")
        .output()
        .map_err(|e| format!("cannot run the new binary's doctor: {}", e))?;
    if !doc.status.success() {
        return Err("the new binary's doctor reports problems against this node's state".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win32_volume_maps_in_statvfs_terms() {
        // GetDiskFreeSpaceExW reports total / total-free /
        // available-to-caller. The mapping must land in the same
        // terms as the statvfs read: used = total − total_free
        // (the f_bfree analog); free = available-to-caller (the
        // f_bavail analog). The two free figures differ exactly
        // when a quota reserves space the caller may not write —
        // transposing them would misstate used by the reserve.
        let v = win32_volume("C:\\".to_string(), 1_000, 400, 350);
        assert_eq!(v.total_bytes, 1_000);
        assert_eq!(v.used_bytes, 600);
        assert_eq!(v.free_bytes, 350);
        // a degenerate report (total_free > total) must not underflow
        let v = win32_volume("C:\\".to_string(), 100, 140, 0);
        assert_eq!(v.used_bytes, 0);
        assert_eq!(v.free_bytes, 0);
    }

    #[test]
    fn version_triples_compare() {
        assert!(parse_version("1.1.0") > parse_version("1.0.3"));
        assert!(parse_version("v1.0.3") == parse_version("1.0.3"));
        assert!(parse_version("1.0.3") < parse_version("1.0.10"));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("soon"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn assess_offer_enforces_the_trust_rule() {
        let trusted = "AAAA1111BBBB2222CCCC3333DDDD4444EEEE5555";
        let other = "FFFF6666EEEE7777DDDD8888CCCC9999BBBB0000";
        // another signer's offer: not our trust, whatever its signature
        assert_eq!(assess_offer(other, Ok(other), trusted, "9.9.9", "1.0.3"), Assess::NotOurSigner);
        assert_eq!(assess_offer(other, Err("no key"), trusted, "9.9.9", "1.0.3"), Assess::NotOurSigner);
        // claimed signer, signature fails outright
        match assess_offer(trusted, Err("bad signature"), trusted, "1.1.0", "1.0.3") {
            Assess::Refuse(w) => assert!(w.contains("does not verify"), "{}", w),
            v => panic!("expected Refuse, got {:?}", v),
        }
        // claimed signer, but the signature is someone else's
        match assess_offer(trusted, Ok(other), trusted, "1.1.0", "1.0.3") {
            Assess::Refuse(w) => assert!(w.contains("verifies under"), "{}", w),
            v => panic!("expected Refuse, got {:?}", v),
        }
        // the real thing: trusted, verified, strictly newer
        assert_eq!(assess_offer(trusted, Ok(trusted), trusted, "1.1.0", "1.0.3"), Assess::Accept);
        // trusted and verified, but equal or older: never a downgrade
        assert_eq!(assess_offer(trusted, Ok(trusted), trusted, "1.0.3", "1.0.3"), Assess::AlreadyCurrent);
        assert_eq!(assess_offer(trusted, Ok(trusted), trusted, "1.0.2", "1.0.3"), Assess::AlreadyCurrent);
    }

    #[test]
    fn manifest_composes_parses_and_detects_tampering() {
        let dir = std::env::temp_dir().join(format!("qexo-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let gpg = Gpg::open_in(&dir);
        let fp = gpg.generate("qexo test signer", "qexo test").expect("test key generation");
        let text = compose_manifest("1.1.0", &hash::sha256_hex(b"fake-binary"), 11, "deadbeef", &fp, "2026-11-12 00:00 UTC", "qexo-1.1.0", "6.6.6/8.1.1/1.1.2");
        let m = parse_manifest(&text).expect("manifest parses");
        assert_eq!(m.version, "1.1.0");
        assert_eq!(m.signer, fp);
        assert_eq!(m.size_bytes, 11);
        assert_eq!(m.binary, "qexo-1.1.0");
        assert_eq!(m.sha256, hash::sha256_hex(b"fake-binary"));
        // the scroll text is the signed text: sign it, verify it,
        // then change one byte and watch verification fail
        let sig = gpg.sign_detached(&fp, text.as_bytes()).expect("sign");
        let verified = gpg.verify_detached(&sig, text.as_bytes()).expect("verify");
        assert_eq!(verified, fp);
        let tampered = text.replace("1.1.0", "1.1.1");
        assert!(gpg.verify_detached(&sig, tampered.as_bytes()).is_err());
        // a stranger's key verifies nothing here
        let gpg2_dir = dir.join("other");
        let gpg2 = Gpg::open_in(&gpg2_dir);
        assert!(gpg2.verify_detached(&sig, text.as_bytes()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timer_units_carry_the_trigger_not_the_logic() {
        let (service, timer) = update_unit_texts(Path::new("/usr/local/bin/qexo"), 12, false);
        assert!(service.contains("ExecStart=/usr/local/bin/qexo update"), "{}", service);
        assert!(service.contains("Type=oneshot"), "{}", service);
        assert!(timer.contains("OnUnitActiveSec=12h"), "{}", timer);
        assert!(timer.contains("WantedBy=timers.target"), "{}", timer);
        // with evolve: on, the same thin trigger names the wider
        // pass — the decision logic still lives in the command,
        // never in the unit.
        let (service, timer) = update_unit_texts(Path::new("/usr/local/bin/qexo"), 12, true);
        assert!(service.contains("ExecStart=/usr/local/bin/qexo evolve"), "{}", service);
        assert!(timer.contains("Run qexo evolve every 12h"), "{}", timer);
    }

    // ---- v1.2.0: evolve — gossip trust, persona lineage ----

    fn rep(name: &str, fp: &str) -> ReportedPeer {
        ReportedPeer { name: name.to_string(), fingerprint: Some(fp.to_string()), endpoint: None, host: None }
    }

    #[test]
    fn gossip_discovers_but_never_promotes() {
        let own = "AAAA".repeat(10);
        let trusted = vec![("peer-bbbb".to_string(), "BBBB".repeat(10))];
        // a new name+fingerprint is discovered — and discovery
        // is the ONLY thing that can happen to it here: the
        // verdict vocabulary has no "trust" outcome at all.
        assert_eq!(
            gossip_verdict("peer-bbbb", &own, &trusted, &[], &rep("peer-cccc", &"CCCC".repeat(10))),
            GossipVerdict::Discover
        );
        // a report about ourselves is skipped
        assert_eq!(
            gossip_verdict("peer-bbbb", &own, &trusted, &[], &rep("peer-aaaa", &own)),
            GossipVerdict::SkipSelf
        );
        // a faithful report about a trusted peer confirms it
        assert_eq!(
            gossip_verdict("peer-cccc", &own, &trusted, &[], &rep("peer-bbbb", &"BBBB".repeat(10))),
            GossipVerdict::Confirm
        );
        // an already-discovered peer is simply known
        let disc = vec![("peer-cccc".to_string(), "CCCC".repeat(10))];
        assert_eq!(
            gossip_verdict("peer-bbbb", &own, &trusted, &disc, &rep("peer-cccc", &"CCCC".repeat(10))),
            GossipVerdict::Known
        );
    }

    #[test]
    fn gossip_conflicts_warn_and_never_overwrite() {
        let own = "AAAA".repeat(10);
        let trusted = vec![("peer-bbbb".to_string(), "BBBB".repeat(10))];
        // a known NAME with a foreign fingerprint: conflict
        match gossip_verdict("peer-cccc", &own, &trusted, &[], &rep("peer-bbbb", &"EEEE".repeat(10))) {
            GossipVerdict::Conflict(m) => {
                assert!(m.contains("record untouched"), "{}", m);
                assert!(m.contains("peer-cccc"), "{}", m);
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
        // a known FINGERPRINT under a foreign name: conflict
        match gossip_verdict("peer-cccc", &own, &trusted, &[], &rep("peer-zzzz", &"BBBB".repeat(10))) {
            GossipVerdict::Conflict(m) => assert!(m.contains("record untouched"), "{}", m),
            other => panic!("expected Conflict, got {:?}", other),
        }
    }

    #[test]
    fn discovered_peers_are_never_in_the_trusted_set() {
        let mut px = Phext::new();
        // a registered peer and a discovered peer, side by side
        px.insert(
            lattice::at(lattice::HL_PEERS, [2, 1, 1, 1, 1, 1]),
            lattice::record(&[("kind", "peer"), ("name", "peer-reg"), ("fingerprint", "F1"), ("status", "registered")]),
        );
        px.insert(
            lattice::at(lattice::HL_PEERS, [3, 1, 1, 1, 1, 1]),
            lattice::record(&[
                ("kind", "peer"),
                ("name", "peer-disc"),
                ("fingerprint", "F2"),
                ("status", "discovered"),
                ("via", "peer-reg"),
            ]),
        );
        let all = peer_records(&px);
        assert_eq!(all.len(), 2, "both records exist as knowledge");
        let trusted = trusted_peer_records(&px);
        assert_eq!(trusted.len(), 1, "only the registered peer is trusted");
        assert!(trusted[0].2.contains("peer-reg"));
    }

    fn copy(rev: u64, follows: &str, history: &[&str], boot: &str) -> PersonaCopy {
        PersonaCopy {
            revision: rev,
            follows: follows.to_string(),
            history: history.iter().map(|s| s.to_string()).collect(),
            stamp: history.last().copied().unwrap_or("").to_string(),
            boot: boot.to_string(),
        }
    }

    #[test]
    fn sync_decision_judges_by_lineage() {
        let root = copy(1, "none", &["s1"], "boot one");
        let child = copy(2, "1 s1", &["s1", "s2"], "boot two");
        let grandchild = copy(3, "2 s2", &["s1", "s2", "s3"], "boot three");
        assert_eq!(sync_decision(&root, &root), SyncDecision::Same);
        assert_eq!(sync_decision(&root, &child), SyncDecision::Adopt);
        assert_eq!(sync_decision(&child, &grandchild), SyncDecision::Adopt);
        assert_eq!(sync_decision(&grandchild, &child), SyncDecision::Behind);
        // a fork off the same root is a divergence, both ways
        let fork = copy(2, "1 s1", &["s1", "s2x"], "boot two, other words");
        assert_eq!(sync_decision(&child, &fork), SyncDecision::Diverge);
        assert_eq!(sync_decision(&fork, &child), SyncDecision::Diverge);
        // unrelated roots diverge even at the same revision
        let alien = copy(1, "none", &["s9"], "someone else's boot");
        assert_eq!(sync_decision(&root, &alien), SyncDecision::Diverge);
    }

    #[test]
    fn persona_stamps_are_deterministic_and_parented() {
        // same content, same stamp — two nodes editing alike agree
        let a = persona_stamp("Pell", 1, "none", "boot text");
        let b = persona_stamp("Pell", 1, "none", "boot text");
        assert_eq!(a, b);
        // a copy_view over a record written by host reads back
        // the lineage the record carries
        let rec = lattice::record(&[
            ("kind", "persona"),
            ("name", "Pell"),
            ("revision", "2"),
            ("follows", &format!("1 {}", a)),
            ("history", &format!("{},{}", a, persona_stamp("Pell", 2, &format!("1 {}", a), "boot two"))),
            ("stamp", &persona_stamp("Pell", 2, &format!("1 {}", a), "boot two")),
        ]);
        let v = copy_view(&rec, "boot two");
        assert_eq!(v.revision, 2);
        assert_eq!(v.history.len(), 2);
        assert_eq!(v.history[0], a);
        assert_eq!(v.follows, format!("1 {}", a));
        // a legacy record with no lineage fields reads as a root
        let legacy = lattice::record(&[("kind", "persona"), ("name", "Pell")]);
        let v = copy_view(&legacy, "boot text");
        assert_eq!(v.revision, 1);
        assert_eq!(v.history, vec![a]);
    }

    #[test]
    fn divergence_keeps_both_copies_in_state() {
        // B holds Pell at revision 2 on one line; a divergent
        // revision 2 arrives from a peer. The stage's Diverge
        // arm is exercised at the state level here: local copy
        // untouched, attempt preserved with its origin named.
        let mut px = Phext::new();
        let s1 = persona_stamp("Pell", 1, "none", "boot one");
        let s2 = persona_stamp("Pell", 2, &format!("1 {}", s1), "boot two");
        let rec = lattice::record(&[
            ("kind", "persona"),
            ("name", "Pell"),
            ("revision", "2"),
            ("follows", &format!("1 {}", s1)),
            ("history", &format!("{},{}", s1, s2)),
            ("stamp", &s2),
        ]);
        px.insert(persona_addr(2, 1, 1), rec.clone());
        for (i, ch) in chunk_text("boot two", 3800).iter().enumerate() {
            px.insert(persona_addr(2, 2, (i + 1) as u32), ch.clone());
        }
        // boots are read back in their stored form: chunk_text
        // newline-terminates every line, and that normalized
        // form is the canonical text sync compares
        let local = copy_view(&rec, "boot two\n");
        // the divergent incoming copy: same root, other child
        let s2x = persona_stamp("Pell", 2, &format!("1 {}", s1), "boot two, other words");
        let remote = PersonaCopy {
            revision: 2,
            follows: format!("1 {}", s1),
            history: vec![s1.clone(), s2x.clone()],
            stamp: s2x.clone(),
            boot: "boot two, other words\n".to_string(),
        };
        assert_eq!(sync_decision(&local, &remote), SyncDecision::Diverge);
        // keep it the way persona_stage does
        let b = lattice::next_free_book(&px, [4, 4, 4, 2, 1], 5);
        assert!(b >= 5);
        px.insert(
            persona_addr(2, b, 1),
            lattice::record(&[
                ("kind", "persona attempt"),
                ("persona", "Pell"),
                ("origin", "peer-cee"),
                ("stamp", &remote.stamp),
            ]),
        );
        px.insert(persona_addr(2, b, 2), remote.boot.clone());
        // both stand: the resident record and boot are the
        // originals, and the attempt is beside them
        assert_eq!(px.get(&persona_addr(2, 1, 1)), Some(&rec));
        assert_eq!(boot_text_of(&px, 2).as_deref(), Some("boot two\n"));
        let attempt = px.get(&persona_addr(2, b, 1)).expect("attempt record");
        assert!(attempt.contains("origin: peer-cee"), "{}", attempt);
        assert_eq!(px.get(&persona_addr(2, b, 2)).map(|s| s.as_str()), Some("boot two, other words\n"));
    }

    #[test]
    fn evolve_receipt_names_every_stage() {
        let mut r = EvolveReport::default();
        r.peers_polled = 2;
        r.peers_learned.push("peer-dee via peer-cee at 3.3.3/3.3.3/4.1.1".to_string());
        r.peers_confirmed = 1;
        r.personas_considered = 1;
        r.personas_adopted.push("Pell revision 2 → 3 from peer-aye".to_string());
        r.divergences_kept.push("Pell: peer-cee's revision 3 kept as attempt".to_string());
        r.binary = "updated 1.1.0 → 1.2.0".to_string();
        r.warnings.push("gossip via peer-cee: a lie".to_string());
        let fields = evolve_receipt_fields(&r);
        let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        for want in [
            "peers polled",
            "peers learned",
            "peers confirmed",
            "personas considered",
            "personas adopted",
            "divergences kept",
            "binary",
            "warnings",
            "learned",
            "adopted",
            "divergence kept",
            "warning",
        ] {
            assert!(keys.contains(&want), "receipt is missing the {} field: {:?}", want, keys);
        }
        let bin = fields.iter().find(|(k, _)| k == "binary").map(|(_, v)| v.as_str());
        assert_eq!(bin, Some("updated 1.1.0 → 1.2.0"));
    }

    // ---- v1.4.0: hostnames, display, the nav tree ----

    #[test]
    fn host_comes_out_of_the_key_uid() {
        assert_eq!(host_from_uid("qexo node aletheia-core (qexo node key)"), Some("aletheia-core".to_string()));
        assert_eq!(host_from_uid("qexo node rock-mountain"), Some("rock-mountain".to_string()));
        assert_eq!(host_from_uid("someone (a comment)"), Some("someone".to_string()));
        assert_eq!(host_from_uid("  "), None);
        assert_eq!(host_from_uid("qexo node "), None);
    }

    fn peer_rec(fields: &[(&str, &str)]) -> String {
        lattice::record(fields)
    }

    #[test]
    fn display_name_prefers_host_then_falls_back_to_name() {
        let with = peer_rec(&[("name", "peer-a4e7277f"), ("host", "aletheia-core")]);
        assert_eq!(peer_display_name(&with), "aletheia-core");
        // a record that predates host: shows its name — for the
        // fleet those names ARE the tailnet hostnames already
        let legacy = peer_rec(&[("name", "rock-mountain")]);
        assert_eq!(peer_display_name(&legacy), "rock-mountain");
        let empty_host = peer_rec(&[("name", "peer-a4e7277f"), ("host", "")]);
        assert_eq!(peer_display_name(&empty_host), "peer-a4e7277f");
    }

    #[test]
    fn display_endpoint_substitutes_the_name_for_a_bare_ip() {
        let t = peer_rec(&[("name", "peer-a4e7277f"), ("host", "aletheia-core"), ("endpoint", "http://100.88.154.1:8484")]);
        assert_eq!(peer_display_endpoint(&t).as_deref(), Some("http://aletheia-core:8484"));
        // a hostname endpoint stands as recorded
        let t = peer_rec(&[("name", "rock-mountain"), ("endpoint", "http://rock-mountain:8484")]);
        assert_eq!(peer_display_endpoint(&t).as_deref(), Some("http://rock-mountain:8484"));
        // no endpoint, no display endpoint
        let t = peer_rec(&[("name", "rock-mountain")]);
        assert_eq!(peer_display_endpoint(&t), None);
    }

    #[test]
    fn nav_tree_renders_the_net_by_name_with_a_discovered_peer() {
        let mut px = Phext::new();
        px.insert(
            lattice::IDENTITY,
            lattice::record(&[("kind", "node identity"), ("name", "qexo node testbox"), ("node", "testbox")]),
        );
        px.insert(
            lattice::NODE_KEY_RECORD,
            lattice::record(&[("kind", "node key"), ("fingerprint", &"A".repeat(40)), ("status", "active")]),
        );
        px.insert(
            lattice::at(lattice::HL_PEERS, [2, 1, 1, 1, 1, 1]),
            lattice::record(&[
                ("kind", "peer"),
                ("name", "peer-a4e7277f"),
                ("host", "aletheia-core"),
                ("fingerprint", &"B".repeat(40)),
                ("status", "registered"),
                ("endpoint", "http://100.88.154.1:8484"),
            ]),
        );
        px.insert(
            lattice::at(lattice::HL_PEERS, [3, 1, 1, 1, 1, 1]),
            lattice::record(&[
                ("kind", "peer"),
                ("name", "peer-8cf39bf8"),
                ("fingerprint", &"C".repeat(40)),
                ("status", "discovered"),
                ("via", "aletheia-core"),
            ]),
        );
        let mut lat = Lattice::born(std::path::PathBuf::from("/tmp/qexo-tree-test/state.phext"));
        lat.px = px;
        let tree = peers_tree(&lat);
        // the snapshot, byte for byte: root anchor, this node
        // marked, peers nested under their record addresses, the
        // endpoint shown by name, the discovered peer with its via
        let fp_short_local = lattice::fp_short(&"A".repeat(40));
        let expected = format!(
            "3.3.3/3.3.3/3.3.3 — peers\n\
             +-- testbox (this node) — {}\n\
             +-- aletheia-core — registered — 3.3.3/2.1.1/1.1.1 — http://aletheia-core:8484\n\
             \\-- peer-8cf39bf8 — discovered — 3.3.3/3.1.1/1.1.1 (via aletheia-core)\n",
            fp_short_local
        );
        assert_eq!(tree, expected, "\n{}", tree);
        // no bare IP anywhere in the human rendering
        assert!(!tree.contains("100.88.154.1"), "{}", tree);
    }
}

