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
fn now_stamp() -> (i64, String) {
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
fn civil_from_days(z: i64) -> (i64, i64, i64) {
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

fn require_born() -> Result<Lattice, i32> {
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

pub fn config(auto_update: bool) -> i32 {
    let dir = lattice::state_dir();
    let path = lattice::state_path();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("qexo config: cannot create {}: {}", dir.display(), e);
        return 1;
    }
    let node = hostname();
    let gpg = Gpg::open_in(&dir);

    let mut lat = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => Lattice::born(path.clone()),
        Err(e) => {
            eprintln!("qexo config: cannot load state: {}", e);
            return 1;
        }
    };

    // Idempotent: a node key record already in the lattice means
    // this node is born; say so and stop — but first backfill any
    // gate config keys this binary knows that the record predates
    // (defaults are seeded visibly; existing values never touched).
    if let Some(t) = lat.px.get(&lattice::NODE_KEY_RECORD) {
        let fp = lattice::field(t, "fingerprint").unwrap_or_default();
        let mut added = ensure_gate_config(&mut lat);
        added.extend(ensure_update_config(&mut lat));
        if !added.is_empty() {
            lattice::regen_indexes(&mut lat.px);
            if let Err(e) = lat.save() {
                eprintln!("qexo config: cannot save state: {}", e);
                return 1;
            }
            println!("config defaults added: {}", added.join(", "));
        }
        println!("already configured — this node was born {}", lattice::field(lat.px.get(&lattice::IDENTITY).map(|s| s.as_str()).unwrap_or(""), "born").unwrap_or_default());
        println!("state: {}", path.display());
        println!("node key fingerprint: {}", fp);
        println!("home: {}", fmt_coord(&lattice::HOME));
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
            ("qexo", "v1.1.0"),
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
        let rec = lattice::record(&fields);
        lat.px.insert(lattice::CONFIG_RECORD, rec);
    }

    // Wiring registry (9.9.9, cluster 2): one scroll per module —
    // the registry the wiring gate's story is told in.
    let wiring_first8 = [9, 9, 9, 2, 1, 1, 1, 1];
    if lattice::prefix_scrolls(&lat.px, &wiring_first8).is_empty() {
        let mut modules = vec![
            "cli", "core", "crypto", "doctor", "hash", "infer", "lattice", "phext", "plan", "wiring", "xz",
        ];
        modules.sort();
        for (i, m) in modules.iter().enumerate() {
            let addr = lattice::at(lattice::HL_CONFIG, [2, 1, 1, 1, 1, (i + 1) as u32]);
            lat.px.insert(addr, format!("module: {}\nfile: src/{}.rs\n", m, m));
        }
    }

    let receipt = append_receipt(&mut lat.px, "config", &[("note", "node born; state.phext initialized with the homeland map")]);
    lattice::regen_indexes(&mut lat.px);
    if let Err(e) = lat.save() {
        eprintln!("qexo config: cannot save state: {}", e);
        return 1;
    }
    println!("node born: {}", name);
    println!("state: {}", path.display());
    println!("node key fingerprint: {}", fp);
    println!("home index: {}", fmt_coord(&lattice::HOME));
    println!("birth receipt: {}", fmt_coord(&receipt));
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
    let mut text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_else(|| "kind: qexo config\n".to_string());
    text = lattice::set_field(&text, "auto_update", "on");
    lat.px.insert(lattice::CONFIG_RECORD, text.clone());
    if let Err(e) = lat.save() {
        eprintln!("qexo config: cannot save state: {}", e);
        return 1;
    }
    let hours: u64 = lattice::field(&text, "update_interval_hours").and_then(|v| v.parse().ok()).unwrap_or(24);
    println!("auto_update: on (poll every {}h; the trust rule in the config record still governs every poll)", hours);
    print!("{}", install_update_timer(hours));
    0
}

/// The systemd units for the update timer: a oneshot service
/// running `qexo update` by this binary's own path, and the
/// timer that fires it. Pure text — tested, and printed verbatim
/// when installation isn't possible here.
fn update_unit_texts(exe: &Path, hours: u64) -> (String, String) {
    let service = format!(
        "[Unit]\nDescription=qexo automatic update (phextnet, signed-manifest trust rule)\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nExecStart={} update\n",
        exe.display()
    );
    let timer = format!(
        "[Unit]\nDescription=Run qexo update every {}h\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec={}h\nUnit=qexo-update.service\n\n[Install]\nWantedBy=timers.target\n",
        hours, hours
    );
    (service, timer)
}

/// Attempt the timer installation; return the report either
/// way. Installed only where all three hold: running as root,
/// systemd is the running init, systemctl answers. Anything
/// less and nothing is written outside the config record.
fn install_update_timer(hours: u64) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("qexo"));
    let (service, timer) = update_unit_texts(&exe, hours);
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
    for (_, addr, text) in peer_records(&lat.px) {
        if lattice::field(&text, "fingerprint").as_deref() == Some(fp.as_str()) {
            // Already known — but an endpoint named now backfills
            // the record (the peer becomes pollable).
            if let Some(ep) = &endpoint {
                if lattice::field(&text, "endpoint").as_deref() != Some(ep.as_str()) {
                    let updated = lattice::set_field(&text, "endpoint", ep);
                    lat.px.insert(addr, updated);
                    lattice::regen_indexes(&mut lat.px);
                    if let Err(e) = lat.save() {
                        eprintln!("qexo register: cannot save state: {}", e);
                        return 1;
                    }
                    println!("endpoint recorded for {}: {}", lattice::field(&text, "name").unwrap_or_default(), ep);
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
    println!("fingerprint: {}", fp);
    println!("receipt: {}", fmt_coord(&receipt));
    0
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

pub fn list() -> i32 {
    let lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    print!("{}", peers_text(&lat));
    0
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
    let boot_text = boot_text.unwrap();
    let (_, stamp) = now_stamp();
    let c = match cluster {
        Some(c) => c,
        None => {
            let c = lattice::next_free_cluster(&lat.px, lattice::HL_PERSONAS, 2);
            lat.px.insert(
                persona_addr(c, 1, 1),
                lattice::record(&[
                    ("kind", "persona"),
                    ("name", &name),
                    ("status", "resident"),
                    ("born", &stamp),
                    ("boot source", &boot_source),
                ]),
            );
            cluster = Some(c);
            c
        }
    };
    // Store the boot text as an adjacent scroll run if this home
    // has none yet (a persona's boot is cite-able in parts).
    if boot_text_of(&lat.px, c).is_none() {
        for (i, chunk) in chunk_text(&boot_text, 3800).iter().enumerate() {
            lat.px.insert(persona_addr(c, 2, (i + 1) as u32), chunk.clone());
        }
        lat.px.insert(
            persona_addr(c, 3, 1),
            lattice::record(&[("kind", "persona state"), ("persona", &name), ("status", "resident")]),
        );
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
    println!("qexo v1.1.0 — node status");
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
                    .map(|(_, _, t)| lattice::field(t, "name").unwrap_or_default())
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
/// from statvfs) and a file count under another. The probe takes
/// no position on what the numbers MEAN — thresholds live in
/// plan::evaluate_gate, fed by the node's config record. (Before
/// v1.0.2 this function carried the VM quota's 95/102 GB verdicts
/// inside itself; a probe that judges is a gate no one configured.)
struct DiskProbe {
    volume: Option<plan::Volume>,
    files: u64,
}

fn probe_disk(volume_root: &Path, files_root: &Path, file_cap: u64) -> DiskProbe {
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
    if let Ok(cpath) = std::ffi::CString::new(volume_root.display().to_string()) {
        let mut st = Statvfs {
            f_bsize: 0, f_frsize: 0, f_blocks: 0, f_bfree: 0, f_bavail: 0,
            f_files: 0, f_ffree: 0, f_favail: 0, f_fsid: 0, f_flag: 0, f_namemax: 0,
            f_spare: [0; 6],
        };
        let rc = unsafe { statvfs(cpath.as_ptr(), &mut st) };
        if rc == 0 && st.f_blocks > 0 {
            let fr = st.f_frsize;
            volume = Some(plan::Volume {
                root: volume_root.display().to_string(),
                total_bytes: st.f_blocks * fr,
                used_bytes: (st.f_blocks - st.f_bfree) * fr,
                free_bytes: st.f_bavail * fr,
            });
        }
    }
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
    println!("routes: / (home index), /v1/doctor, /v1/plan, /v1/peers, /v1/release, /v1/release/signature, /release/qexo-<version>");
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
            "no such route — qexo serves /, /v1/doctor, /v1/plan, /v1/peers, /v1/release, /v1/release/signature, /release/qexo-<version> (payloads are scroll text; the release binary is the one byte route)\n".to_string(),
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

/// `qexo update [--check]` — poll the registered peers, judge
/// every offer by the trust rule, and apply the trusted one.
pub fn update(check_only: bool) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(c) => return c,
    };
    let cfg_text = lat.px.get(&lattice::CONFIG_RECORD).cloned().unwrap_or_default();
    let trusted = match lattice::field(&cfg_text, "release_signer").filter(|s| !s.is_empty()) {
        Some(fp) => fp,
        None => {
            eprintln!(
                "qexo update: no trusted release signer configured — set `release_signer: <fingerprint>` in the config record ({}) to the node whose signed releases this node trusts. Nothing was polled and nothing changed.",
                fmt_coord(&lattice::CONFIG_RECORD)
            );
            return 1;
        }
    };
    let peers = peer_records(&lat.px);
    if !peers.iter().any(|(_, _, t)| lattice::field(t, "fingerprint").as_deref() == Some(trusted.as_str())) {
        eprintln!(
            "qexo update: release signer {} is not in this node's peer registry — register their public key first (`qexo register <pubkey> --endpoint <url>`). No key, no trust: nothing was polled and nothing changed.",
            trusted
        );
        return 1;
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    let gpg = Gpg::open_in(&lattice::state_dir());
    let mut saw_offer = false;
    let mut current_offer: Option<String> = None;

    for (_, _, text) in &peers {
        let name = lattice::field(text, "name").unwrap_or_default();
        let endpoint = match lattice::field(text, "endpoint").filter(|s| !s.is_empty()) {
            Some(e) => e.trim_end_matches('/').to_string(),
            None => {
                println!(
                    "peer {}: no endpoint recorded — not pollable (re-register with --endpoint <url> to add one)",
                    name
                );
                continue;
            }
        };
        let manifest_bytes = match http_get(&format!("{}/v1/release", endpoint)) {
            Ok((200, b)) => b,
            Ok((404, _)) => {
                println!("peer {} ({}): no release announced", name, endpoint);
                continue;
            }
            Ok((s, _)) => {
                println!("peer {} ({}): /v1/release answered HTTP {} — skipped", name, endpoint, s);
                continue;
            }
            Err(e) => {
                println!("peer {} ({}): unreachable — {}", name, endpoint, e);
                continue;
            }
        };
        saw_offer = true;
        let manifest_text = String::from_utf8_lossy(&manifest_bytes).to_string();
        let manifest = match parse_manifest(&manifest_text) {
            Ok(m) => m,
            Err(e) => {
                println!("peer {} ({}): unusable manifest — {} — skipped", name, endpoint, e);
                continue;
            }
        };
        // Whose release is this, by its own word? Another node's
        // own announcement is not this node's trust — note and
        // move on WITHOUT fetching its signature or binary.
        if manifest.signer != trusted {
            println!(
                "peer {} offers qexo {} signed by {} — not this node's release signer ({}); ignored",
                name,
                manifest.version,
                lattice::fp_short(&manifest.signer),
                lattice::fp_short(&trusted)
            );
            continue;
        }
        // The offer CLAIMS the trusted signer. Verify before
        // anything is downloaded: a failure here is a hard stop.
        let sig_text = match http_get(&format!("{}/v1/release/signature", endpoint)) {
            Ok((200, b)) => String::from_utf8_lossy(&b).to_string(),
            Ok((s, _)) => {
                let why = format!("the manifest's signature could not be fetched (HTTP {})", s);
                update_receipt(
                    &mut lat,
                    "refused",
                    &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                );
                eprintln!("qexo update: release from {} claims the trusted signer but {}", endpoint, why);
                return 1;
            }
            Err(e) => {
                let why = format!("the manifest's signature could not be fetched ({})", e);
                update_receipt(
                    &mut lat,
                    "refused",
                    &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                );
                eprintln!("qexo update: release from {} claims the trusted signer but {}", endpoint, why);
                return 1;
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
                update_receipt(
                    &mut lat,
                    "refused",
                    &[("reason", &why), ("from", &current), ("offered", &manifest.version), ("signer", &trusted)],
                );
                eprintln!(
                    "qexo update: REFUSED — the release offered by {} claims signer {} but {} — nothing was downloaded and the running binary is untouched",
                    endpoint, lattice::fp_short(&trusted), why
                );
                return 1;
            }
            Assess::AlreadyCurrent => {
                current_offer = Some(manifest.version.clone());
            }
            Assess::Accept => {
                if check_only {
                    println!(
                        "update available: {} → {} (signer {}, via {}) — run `qexo update` to apply",
                        current, manifest.version, lattice::fp_short(&trusted), endpoint
                    );
                    return 0;
                }
                return apply_update(&mut lat, &endpoint, &manifest, &trusted, &current);
            }
        }
    }

    if let Some(offered) = current_offer {
        println!("already current — this node runs qexo {}; the trusted signer offers {}", current, offered);
        0
    } else if saw_offer {
        println!("qexo update: offers were seen, none from this node's release signer — nothing applied");
        1
    } else {
        println!("qexo update: no peer offered a release manifest — nothing applied");
        1
    }
}

/// Download, verify, swap, post-check, roll back on failure.
/// The caller has already established trust (signature verified
/// against the registry key named by `release_signer`) and
/// novelty (strictly newer). What remains is the bytes' own
/// verdict: size + sha256 against the manifest, then the swap.
fn apply_update(lat: &mut Lattice, endpoint: &str, manifest: &Manifest, trusted: &str, current: &str) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("qexo update: cannot locate the running binary: {}", e);
            return 1;
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
            return 1;
        }
        Err(e) => {
            let why = format!("binary download failed: {}", e);
            update_receipt(
                lat,
                "refused",
                &[("reason", &why), ("from", current), ("to", &manifest.version), ("sha256", &manifest.sha256), ("signer", trusted)],
            );
            eprintln!("qexo update: REFUSED — {}", why);
            return 1;
        }
    };
    let tmp = dir.join("qexo.update-tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes) {
        eprintln!(
            "qexo update: cannot stage the download beside the running binary ({}): {} — the binary's directory must be writable for a self-update",
            tmp.display(),
            e
        );
        return 1;
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
        return 1;
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
        return 1;
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
        return 1;
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
            0
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
            1
        }
    }
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
        let (service, timer) = update_unit_texts(Path::new("/usr/local/bin/qexo"), 12);
        assert!(service.contains("ExecStart=/usr/local/bin/qexo update"), "{}", service);
        assert!(service.contains("Type=oneshot"), "{}", service);
        assert!(timer.contains("OnUnitActiveSec=12h"), "{}", timer);
        assert!(timer.contains("WantedBy=timers.target"), "{}", timer);
    }
}

