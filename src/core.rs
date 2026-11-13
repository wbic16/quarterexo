//! core — the state commands, dispatched from main.
//!
//! Every command here works the same way: load the one state
//! file through the lattice, read and write RECORDS (one scroll,
//! field lines), regenerate the indexes from the records, save.
//! Identity lives in the records' text and in the index scrolls;
//! nothing is addressed by formula (Layer 2, v2).

use crate::crypto::Gpg;
use crate::doctor;
use crate::infer::{Infer, InferError};
use crate::lattice::{self, Lattice};
use crate::phext::{fmt_coord, Phext};
use crate::plan;
use std::io::Read as IoRead;
use std::io::Write as IoWrite;
use std::net::TcpListener;
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

pub fn config() -> i32 {
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
        let added = ensure_gate_config(&mut lat);
        if !added.is_empty() {
            lattice::regen_indexes(&mut lat.px);
            if let Err(e) = lat.save() {
                eprintln!("qexo config: cannot save state: {}", e);
                return 1;
            }
            println!("gate config defaults added: {}", added.join(", "));
        }
        println!("already configured — this node was born {}", lattice::field(lat.px.get(&lattice::IDENTITY).map(|s| s.as_str()).unwrap_or(""), "born").unwrap_or_default());
        println!("state: {}", path.display());
        println!("node key fingerprint: {}", fp);
        println!("home: {}", fmt_coord(&lattice::HOME));
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
            ("qexo", "v1.0.2"),
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
    0
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
        eprintln!("usage: qexo register <pubkey> — armored text, a file path, or - for stdin");
        return 2;
    }
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
    lat.px.insert(
        addr,
        lattice::record(&[
            ("kind", "peer"),
            ("name", &name),
            ("fingerprint", &fp),
            ("status", "registered"),
            ("registered", &stamp),
            ("public key", &fmt_coord(&key_addr)),
        ]),
    );
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
            "{}  {}  {}  {}\n",
            lattice::field(text, "name").unwrap_or_default(),
            lattice::fp_short(&lattice::field(text, "fingerprint").unwrap_or_default()),
            fmt_coord(addr),
            lattice::field(text, "status").unwrap_or_default()
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
    println!("qexo v1.0.2 — node status");
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
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("qexo serve: cannot bind 127.0.0.1:{}: {}", port, e);
            return 1;
        }
    };
    println!("qexo serve — http://127.0.0.1:{} — payloads are scroll text", port);
    println!("routes: / (home index), /v1/doctor, /v1/plan, /v1/peers");
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
        let (status, body) = route(&path);
        let resp = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
    }
    0
}

fn route(path: &str) -> (String, String) {
    let lat = match Lattice::load() {
        Ok(Some(l)) => l,
        Ok(None) => return ("200 OK".to_string(), "qexo: state not born — run qexo config on the node\n".to_string()),
        Err(e) => return ("500 Internal Server Error".to_string(), format!("qexo: state unreadable: {}\n", e)),
    };
    match path {
        "/" => (
            "200 OK".to_string(),
            lat.px.get(&lattice::HOME).cloned().unwrap_or_else(|| "(no home index)\n".to_string()),
        ),
        "/v1/doctor" => {
            let mut report = doctor::check(&lattice::state_dir(), &lattice::state_path());
            report.findings.extend(doctor::check_lattice(&lat).findings);
            ("200 OK".to_string(), report.render())
        }
        "/v1/plan" => (
            "200 OK".to_string(),
            lat.px
                .get(&lattice::PLAN_CURRENT_RENDER)
                .cloned()
                .unwrap_or_else(|| "no plan emitted yet — bare qexo on the node computes the first\n".to_string()),
        ),
        "/v1/peers" => ("200 OK".to_string(), peers_text(&lat)),
        _ => (
            "404 Not Found".to_string(),
            "no such route — qexo serves /, /v1/doctor, /v1/plan, /v1/peers (payloads are scroll text)\n".to_string(),
        ),
    }
}

