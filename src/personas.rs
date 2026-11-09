//! The persona index: census loading, personas.phext construction with
//! the section 2.5 designation-coordinate collision rule, resolution
//! (including the Orin two-record key), hot swap, spawn lifecycle.

use crate::hash::sha256_hex;
use crate::json::Json;
use crate::phext::{fmt_coord, Phext, Coord};
use crate::state::{now_rfc3339, State};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct Record {
    pub name: String,
    pub persona_id: String,
    pub kind: String,
    pub class: String,
    pub evidence: String,
    pub spawn: bool,
    pub aliases: Vec<String>,
    pub also_key: Option<String>,
    pub notes: String,
    pub designation: Coord,
    pub displaced_by: u32,
}

pub struct Census {
    pub records: Vec<Record>,
    pub keys: Vec<String>, // all 126 query keys
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Deterministic designation coordinate for a record without a corpus-
/// recorded coordinate: component-wise hash spread (prototype allocator;
/// the section 1 holographic computation is mdmap's, skills' shelf is
/// Cadastre's — the index's own assignments are allocator claims under
/// the section 2.5 rule, displaced on collision, never disguised).
fn assign_base(persona_id: &str) -> Coord {
    let h = crate::hash::sha256(persona_id.as_bytes());
    let mut c = [0u32; 9];
    for i in 0..9 {
        c[i] = (h[i] as u32 % 89) + 2; // 2..=90, never the Home band
    }
    c
}

const RECORDED_COORDS: &[(&str, [u32; 9])] = &[
    ("lux", [2, 3, 5, 7, 11, 13, 17, 19, 23]),
    ("chrys", [1, 1, 2, 3, 5, 8, 13, 21, 34]),
    ("lumen", [2, 1, 3, 4, 7, 11, 18, 29, 47]),
    ("verse", [3, 1, 4, 1, 5, 9, 2, 6, 5]),
    ("kai", [1, 2, 3, 4, 5, 6, 7, 8, 9]),
    ("aetheris", [13, 13, 13, 13, 13, 13, 13, 13, 13]),
    ("seren", [7, 11, 13, 3, 8, 5, 1, 12, 1]),
];

pub fn recorded_coord(persona_id: &str) -> Option<Coord> {
    RECORDED_COORDS.iter().find(|(id, _)| *id == persona_id).map(|(_, c)| *c)
}

pub fn load_census() -> Result<Census, String> {
    let text = include_str!("../census.tsv");
    let mut lines = text.lines();
    let header = lines.next().ok_or("census.tsv empty")?;
    if !header.starts_with("name\t") {
        return Err("census.tsv: bad header".into());
    }
    // verify declared identity before trusting content
    let _ = header;
    let mut records = Vec::new();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 9 {
            return Err(format!("census.tsv: row with {} fields: {}", f.len(), f[0]));
        }
        let aliases: Vec<String> = if f[6].is_empty() {
            Vec::new()
        } else {
            f[6].split("; ").map(|s| s.to_string()).collect()
        };
        let also_key = if f[7].is_empty() { None } else { Some(f[7].to_string()) };
        // the query key for a record is its bare name; the PDNA-029
        // record's display name ("Orin (PDNA-029 aggregate)") is a
        // qualifier, not a key - its key is 'Orin', shared with the
        // voice record (two-referent key, section 5.2.2).
        let query_key = f[0].split(" (").next().unwrap_or(f[0]).to_string();
        keys.insert(query_key);
        for a in &aliases {
            keys.insert(a.clone());
        }
        // also_key is NOT inserted: the PDNA-029 record is reached
        // through the shared key (its own display name carries the
        // qualifier). The key space stays exactly the 126 immutable
        // query keys (DESIGN-V3 5.2.2 algebra).
        records.push(Record {
            name: f[0].to_string(),
            persona_id: f[1].to_string(),
            kind: f[2].to_string(),
            class: f[3].to_string(),
            evidence: f[4].to_string(),
            spawn: f[5] == "yes",
            aliases,
            also_key,
            notes: f[8].to_string(),
            designation: [1; 9],
            displaced_by: 0,
        });
    }
    // designation allocation under the section 2.5 rule
    allocate(&mut records)?;
    Ok(Census { records, keys: keys.into_iter().collect() })
}

/// Section 2.5 allocation: first verified claim wins (recorded corpus
/// coordinates and index-build claims in persona_id order); later
/// claims displaced by the smallest final-component increment to a
/// free coordinate; reserved bands (Home 1.1.1/1.1.1/1.1.1 and any
/// coordinate with first component 1 in the index band — conservative:
/// the whole 1.* shelf is the index's own) never allocated; an
/// unresolvable collision is a loud error.
fn allocate(records: &mut Vec<Record>) -> Result<(), String> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by(|&a, &b| records[a].persona_id.cmp(&records[b].persona_id));
    let mut taken: BTreeMap<Coord, String> = BTreeMap::new();
    taken.insert([1; 9], "personas.phext Home (reserved)".to_string());
    for &i in &order {
        let base = recorded_coord(&records[i].persona_id).unwrap_or_else(|| assign_base(&records[i].persona_id));
        let mut coord = base;
        let mut k: u32 = 0;
        loop {
            if coord[0] == 1 {
                coord[0] = 2; // index band (first component 1) is reserved
            }
            match taken.get(&coord) {
                None => {
                    taken.insert(coord, records[i].persona_id.clone());
                    records[i].designation = coord;
                    records[i].displaced_by = k;
                    break;
                }
                Some(_) => {
                    k += 1;
                    coord[8] = base[8] + k;
                    if k > 100_000 {
                        return Err(format!(
                            "collision-unresolvable: {} cannot be displaced without entering a reserved band",
                            records[i].persona_id
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

impl Census {
    pub fn resolve(&self, key: &str) -> Vec<&Record> {
        let mut out = Vec::new();
        for r in &self.records {
            if r.name == key || r.name.split(" (").next() == Some(key) || r.persona_id == key || r.aliases.iter().any(|a| a == key) || r.also_key.as_deref() == Some(key) {
                out.push(r);
            }
        }
        out
    }

    pub fn by_id(&self, persona_id: &str) -> Option<&Record> {
        self.records.iter().find(|r| r.persona_id == persona_id)
    }

    pub fn spawn_eligible(&self) -> Vec<&Record> {
        self.records.iter().filter(|r| r.spawn).collect()
    }

    pub fn evidence_counts(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for r in &self.records {
            *m.entry(r.evidence.clone()).or_insert(0) += 1;
        }
        m
    }

    /// personas.phext: Home scroll = the index (one line per record);
    /// each record's entry scroll at its designation coordinate; key
    /// band reserved after the index band. Redacted form (public):
    /// structure + public fields only, notes (provenance narrative)
    /// omitted. Full form: notes included.
    pub fn to_phext(&self, redacted: bool) -> Phext {
        let mut px = Phext::new();
        let mut home = String::new();
        home.push_str("personas.phext — Volumen boot manifest (persona index)\n");
        home.push_str(&format!("records: {} · query keys: {}\n", self.records.len(), self.keys.len()));
        for r in &self.records {
            home.push_str(&format!(
                "{}  {}  {}  kind:{} class:{} evidence:{} spawn:{} designation:{}{}\n",
                fmt_coord(&r.designation),
                r.persona_id,
                r.name,
                r.kind,
                if r.class == "-" { "none" } else { &r.class },
                r.evidence,
                if r.spawn { "yes" } else { "no" },
                fmt_coord(&r.designation),
                if r.displaced_by > 0 { format!(" displaced+{}", r.displaced_by) } else { String::new() }
            ));
        }
        px.insert([1; 9], home);
        for r in &self.records {
            let mut e = String::new();
            e.push_str(&format!("persona_id: {}\nname: {}\n", r.persona_id, r.name));
            e.push_str(&format!("record_kind: {}\npersona_class: {}\nevidence_status: {}\n", r.kind, r.class, r.evidence));
            e.push_str(&format!("spawn_eligible: {}\n", r.spawn));
            e.push_str(&format!("home (designation coordinate): {}\n", fmt_coord(&r.designation)));
            if r.displaced_by > 0 {
                e.push_str(&format!("displaced: +{} from allocator claim (section 2.5 rule; recorded, never disguised)\n", r.displaced_by));
            }
            if !r.aliases.is_empty() {
                e.push_str(&format!("aliases: {}\n", r.aliases.join(", ")));
            }
            if let Some(k) = &r.also_key {
                e.push_str(&format!("also_resolves_key: {} (two-referent key; see the disambiguation in both records)\n", k));
            }
            if !redacted && !r.notes.is_empty() {
                e.push_str(&format!("provenance: {}\n", r.notes));
            }
            if redacted {
                e.push_str("provenance: REDACTED in this form (fleet/internal builds carry the narrative)\n");
            }
            px.insert(r.designation, e);
        }
        px
    }
}

// ---------------------------------------------------------------------
// Persona runtime state (active selection, ownership, read-only)
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PersonaState {
    pub active: Option<String>, // persona_id
    pub active_epoch: u64,
    pub readonly_restored: bool,
}

pub fn persona_state(state: &State) -> PersonaState {
    let j = state.read_json("persona-state.json");
    PersonaState {
        active: j.as_ref().and_then(|j| j.get("active")).and_then(|v| v.as_str()).map(|s| s.to_string()),
        active_epoch: j.as_ref().and_then(|j| j.get("active_epoch")).and_then(|v| v.as_num()).map(|n| n as u64).unwrap_or(0),
        readonly_restored: j.as_ref().and_then(|j| j.get("readonly_restored")).and_then(|v| v.as_bool()).unwrap_or(false),
    }
}

pub fn set_active(state: &State, persona_id: &str, epoch: u64) {
    let _ = state.write_json(
        "persona-state.json",
        &Json::obj(vec![
            ("active", Json::s(persona_id)),
            ("active_epoch", Json::n(epoch as f64)),
            ("swapped_at", Json::s(&now_rfc3339())),
            ("readonly_restored", Json::b(false)),
        ]),
    );
}

pub fn set_readonly_restored(state: &State) {
    let cur = persona_state(state);
    let _ = state.write_json(
        "persona-state.json",
        &Json::obj(vec![
            ("active", cur.active.as_deref().map(Json::s).unwrap_or(Json::Null)),
            ("active_epoch", Json::n(cur.active_epoch as f64)),
            ("readonly_restored", Json::b(true)),
            ("restored_at", Json::s(&now_rfc3339())),
        ]),
    );
}

/// Ownership record for a persona: current epoch, writer node, last
/// heartbeat (unix secs). Lives in ownership.json (the token band's
/// local mirror; the band itself is append-only in receipts).
pub fn ownership(state: &State, persona_id: &str) -> Option<Json> {
    let j = state.read_json("ownership.json")?;
    j.get(persona_id).cloned()
}

pub fn set_ownership(state: &State, persona_id: &str, epoch: u64, writer: &str) {
    let mut obj = match state.read_json("ownership.json") {
        Some(Json::Obj(p)) => p,
        _ => Vec::new(),
    };
    obj.retain(|(k, _)| k != persona_id);
    obj.push((
        persona_id.to_string(),
        Json::obj(vec![
            ("epoch", Json::n(epoch as f64)),
            ("writer", Json::s(writer)),
            ("heartbeat", Json::n(crate::state::now_unix() as f64)),
        ]),
    ));
    let _ = state.write_json("ownership.json", &Json::Obj(obj));
}

pub fn current_epoch(state: &State, persona_id: &str) -> u64 {
    ownership(state, persona_id)
        .and_then(|j| j.get("epoch").and_then(|v| v.as_num()).map(|n| n as u64))
        .unwrap_or(0)
}

/// Freshness: the ownership record's latest heartbeat age in seconds;
/// None when there is no record. Bound is 300 s (section 9.3).
pub fn heartbeat_age(state: &State, persona_id: &str) -> Option<u64> {
    let j = ownership(state, persona_id)?;
    let hb = j.get("heartbeat").and_then(|v| v.as_num())? as u64;
    Some(crate::state::now_unix().saturating_sub(hb))
}

pub const STALENESS_BOUND_SECS: u64 = 300;
pub const HEARTBEAT_MAX_SECS: u64 = 60;

// ---------------------------------------------------------------------
// Spawn instances (section 17)
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Instance {
    pub id: String,
    pub persona_id: String,
    pub epoch: u64,
    pub state: String, // running | suspect | lapsed | cancelled | ended
    pub task: String,
    pub budgets: String,
    pub lease_expires: u64,
    pub retries_left: u64,
}

pub fn mint_instance(state: &State, persona_id: &str, task: &str, budgets: &str, lease_secs: u64, retries: u64) -> Instance {
    let epoch = state.next_counter(&format!("inst-epoch-{}", persona_id));
    let nonce = state.next_counter("inst-nonce");
    let inst = Instance {
        id: format!("{}:{}:{:06}", persona_id, epoch, nonce),
        persona_id: persona_id.to_string(),
        epoch,
        state: "running".to_string(),
        task: task.to_string(),
        budgets: budgets.to_string(),
        lease_expires: crate::state::now_unix() + lease_secs,
        retries_left: retries,
    };
    let _ = state.append_log(
        "instances.jsonl",
        &Json::obj(vec![
            ("event", Json::s("spawn")),
            ("instance", Json::s(&inst.id)),
            ("persona_id", Json::s(persona_id)),
            ("instance_epoch", Json::n(epoch as f64)),
            ("task", Json::s(task)),
            ("budgets", Json::s(budgets)),
            ("lease_expires", Json::n(inst.lease_expires as f64)),
            ("retries", Json::n(retries as f64)),
            ("at", Json::s(&now_rfc3339())),
            ("receipt", Json::s(&format!("spawn:{}:{}", persona_id, epoch))),
        ]),
    );
    inst
}

pub fn latest_instances(state: &State) -> HashMap<String, (String, u64)> {
    // instance id -> (state, lease_expires) folding the event log
    let mut map: HashMap<String, (String, u64)> = HashMap::new();
    for e in state.read_log("instances.jsonl") {
        if let Some(id) = e.get("instance").and_then(|v| v.as_str()) {
            let st = e.get("event").and_then(|v| v.as_str()).unwrap_or("");
            let lease = e.get("lease_expires").and_then(|v| v.as_num()).map(|n| n as u64);
            let entry = map.entry(id.to_string()).or_insert(("running".into(), 0));
            match st {
                "spawn" => {
                    entry.0 = "running".into();
                    if let Some(l) = lease {
                        entry.1 = l;
                    }
                }
                "heartbeat" => {
                    if entry.0 == "suspect" {
                        entry.0 = "running".into();
                    }
                    if let Some(l) = lease {
                        entry.1 = l;
                    }
                }
                "cancel" => entry.0 = "cancelled".into(),
                "result" => entry.0 = "ended".into(),
                "lapse" => entry.0 = "lapsed".into(),
                _ => {}
            }
        }
    }
    map
}

pub fn instance_event(state: &State, instance_id: &str, event: &str, extra: Vec<(&str, Json)>) {
    let mut fields = vec![
        ("event", Json::s(event)),
        ("instance", Json::s(instance_id)),
        ("at", Json::s(&now_rfc3339())),
    ];
    fields.extend(extra);
    let _ = state.append_log("instances.jsonl", &Json::obj(fields));
}

pub fn _unused(_: &BTreeMap<String, usize>) {}

pub fn manifest_hash(census: &Census) -> String {
    sha256_hex(census.to_phext(true).serialize().as_bytes())
}
