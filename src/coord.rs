//! Singular coordinate computation (DESIGN-V3 section 1.1) and the
//! mdmap builder (sections 1.2-1.6). The atlas is the Singular atlas
//! built from Will's archive + CYOA (tools/atlas.tsv): a word's
//! position is the average of the coordinates of every valid CYOA
//! scroll holding it; archive-but-not-CYOA words rest at
//! 1.1.1/1.1.1/1.1.1; words in neither rest at 9.9.9/9.9.9/9.9.9 —
//! the atlas's rest position, a convention, not a corner or bound.
//!
//! The tokenizer matches the Singular checker exactly: Unicode
//! letters with internal apostrophes, lowercased (the semantics of
//! check_singular.py's WORD_RE [^\W\d_]+(?:'[^\W\d_]+)?).

use crate::hash::sha256_hex;
use crate::json::Json;
use crate::phext::{fmt_coord, Coord};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

struct Atlas {
    positions: HashMap<String, Coord>,
    in_archive: BTreeSet<String>,
}

fn atlas() -> &'static Atlas {
    static A: OnceLock<Atlas> = OnceLock::new();
    A.get_or_init(|| {
        let text = include_str!("../tools/atlas.tsv");
        let mut positions = HashMap::new();
        let mut in_archive = BTreeSet::new();
        for (i, line) in text.lines().enumerate() {
            if i == 0 {
                continue; // header
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 6 {
                continue;
            }
            in_archive.insert(f[0].to_string());
            if f[4] == "cyoa" {
                if let Some(c) = crate::phext::parse_coord(f[1]) {
                    positions.insert(f[0].to_string(), c);
                }
            }
        }
        Atlas { positions, in_archive }
    })
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphabetic() {
            cur.push(c.to_lowercase().next().unwrap_or(c));
            i += 1;
        } else if c == '\'' && !cur.is_empty() && i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
            cur.push('\'');
            i += 1;
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            i += 1;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

const FAR: Coord = [9; 9];
const HOME: Coord = [1; 9];

fn round_half_up(total: u64, n: u64) -> u32 {
    let v = (2 * total + n) / (2 * n);
    if v < 1 {
        1
    } else {
        v as u32
    }
}

/// The holographic merge: component-wise average over distinct words,
/// rounded half up, never below 1.
pub fn coordinate_of(text: &str) -> Coord {
    let a = atlas();
    let tokens = tokenize(text);
    let distinct: BTreeSet<String> = tokens.into_iter().collect();
    if distinct.is_empty() {
        return FAR;
    }
    let mut totals = [0u64; 9];
    for w in &distinct {
        let pos = if let Some(p) = a.positions.get(w) {
            *p
        } else if a.in_archive.contains(w) {
            HOME
        } else {
            FAR
        };
        for k in 0..9 {
            totals[k] += pos[k] as u64;
        }
    }
    let n = distinct.len() as u64;
    let mut out = [0u32; 9];
    for k in 0..9 {
        out[k] = round_half_up(totals[k], n);
    }
    out
}

pub fn coordinate_of_text(text: &str) -> String {
    fmt_coord(&coordinate_of(text))
}

// ---------------------------------------------------------------- mdmap
/// Build mdmap for the .md files under `dir` (flat): pin register in
/// the state dir (mdmap-pins.json), index lines (mdmap-index.json),
/// and the phext itself (mdmap.phext in the state dir).
///
/// Rules (sections 1.2-1.4): first computation pins; reservation is
/// atomic across the whole scroll set — content at C, provenance at
/// C+1, redirect slot at the drifted coordinate — displacement (+k on
/// the final component) searches only coordinates outside every
/// reserved set; the Home coordinate and key band are never
/// allocatable; drifted entries are NAMED, never papered over.
pub fn mdmap_build(dir: &str) -> Json {
    let st = crate::state::State::open();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "md").unwrap_or(false) {
                if let Ok(bytes) = std::fs::read(&p) {
                    files.push((e.file_name().to_string_lossy().to_string(), bytes));
                }
            }
        }
    }
    files.sort();
    // pin register: path -> {coord, sha256, pin_date, displaced}
    let mut pins: BTreeMap<String, Json> = match st.read_json("mdmap-pins.json") {
        Some(Json::Obj(pairs)) => pairs.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    // reserved set: every pinned set's coordinates
    let reserved_of = |coord: &Coord, drifted: &Option<Coord>| -> Vec<Coord> {
        let mut v = vec![*coord];
        let mut prov = *coord;
        prov[8] += 1;
        v.push(prov);
        if let Some(d) = drifted {
            v.push(*d);
        }
        v
    };
    let mut reserved: BTreeSet<Coord> = BTreeSet::new();
    reserved.insert([1; 9]); // Home/index — never allocatable
    for pin in pins.values() {
        if let Some(c) = pin.get("coord").and_then(|v| v.as_str()).and_then(crate::phext::parse_coord) {
            let drift = pin.get("drifted").and_then(|v| v.as_str()).and_then(crate::phext::parse_coord);
            for r in reserved_of(&c, &drift) {
                reserved.insert(r);
            }
        }
    }
    let mut px = crate::phext::Phext::new();
    let mut index_rows: Vec<Json> = Vec::new();
    for (name, bytes) in &files {
        let text = String::from_utf8_lossy(bytes).to_string();
        let computed = coordinate_of(&text);
        let sha = sha256_hex(bytes);
        let existing = pins.get(name).cloned();
        let (coord, displaced, status, drifted_coord) = match &existing {
            Some(pin) => {
                let pinned = pin.get("coord").and_then(|v| v.as_str()).and_then(crate::phext::parse_coord).unwrap_or(computed);
                let disp = pin.get("displaced").and_then(|v| v.as_num()).unwrap_or(0.0) as u32;
                if computed != pinned {
                    (pinned, disp, "drifted".to_string(), Some(computed))
                } else {
                    (pinned, disp, "current".to_string(), None)
                }
            }
            None => {
                // allocate the whole set atomically
                let mut c = computed;
                let mut k: u32 = 0;
                loop {
                    let set = reserved_of(&c, &None);
                    if set.iter().all(|r| !reserved.contains(r)) {
                        for r in &set {
                            reserved.insert(*r);
                        }
                        break;
                    }
                    k += 1;
                    c[8] = computed[8] + k;
                    if k > 100_000 {
                        return Json::obj(vec![("error", Json::s(&format!("collision-unresolvable for {} — the build fails loudly rather than improvising an address", name)))]);
                    }
                }
                pins.insert(name.clone(), Json::obj(vec![
                    ("coord", Json::s(&fmt_coord(&c))),
                    ("sha256", Json::s(&sha)),
                    ("pin_date", Json::s(&crate::state::now_rfc3339())),
                    ("displaced", Json::n(k as f64)),
                ]));
                (c, k, "current".to_string(), None)
            }
        };
        let _ = drifted_coord;
        // content scroll at pinned C; provenance at C+1
        px.insert(coord, text);
        let mut prov = coord;
        prov[8] += 1;
        px.insert(prov, format!(
            "source path: {}\npin date: {}\nsha256 at pin: {}\ngenerator: qexo mdmap (section 1)\nstatus: {}\n{}",
            name,
            pins.get(name).and_then(|p| p.get("pin_date")).and_then(|v| v.as_str()).unwrap_or(""),
            sha,
            status,
            if displaced > 0 { format!("displaced +{} from computed {} — recorded, never disguised\n", displaced, fmt_coord(&computed)) } else { String::new() }
        ));
        index_rows.push(Json::obj(vec![
            ("coord", Json::s(&fmt_coord(&coord))),
            ("path", Json::s(name)),
            ("bytes", Json::n(bytes.len() as f64)),
            ("sha256", Json::s(&sha)),
            ("status", Json::s(&if displaced > 0 && status == "current" { format!("displaced+{}", displaced) } else { status })),
        ]));
    }
    // Home index scroll
    let mut home = String::from("mdmap.phext — index (Home)\n");
    for r in &index_rows {
        home.push_str(&format!(
            "{}  {}  {}  sha256:{}  status: {}\n",
            r.get("coord").and_then(|v| v.as_str()).unwrap_or(""),
            r.get("path").and_then(|v| v.as_str()).unwrap_or(""),
            r.get("bytes").and_then(|v| v.as_num()).unwrap_or(0.0) as u64,
            r.get("sha256").and_then(|v| v.as_str()).unwrap_or(""),
            r.get("status").and_then(|v| v.as_str()).unwrap_or("")
        ));
    }
    px.insert([1; 9], home);
    let out_path = st.dir.join("mdmap.phext");
    let _ = std::fs::write(&out_path, px.serialize());
    let _ = st.write_json("mdmap-pins.json", &Json::Obj(pins.into_iter().collect()));
    let _ = st.write_json("mdmap-index.json", &Json::Arr(index_rows.clone()));
    Json::obj(vec![
        ("files", Json::n(index_rows.len() as f64)),
        ("mdmap_phext", Json::s(&out_path.to_string_lossy())),
        ("phext_sha256", Json::s(&sha256_hex(px.serialize().as_bytes()))),
        ("index", Json::Arr(index_rows)),
        ("mdmap_status", Json::s("provisional")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_matches_checker_shape() {
        assert_eq!(tokenize("Don't stop, BELIEVING!"), vec!["don't", "stop", "believing"]);
        assert_eq!(tokenize("abc123def"), vec!["abc", "def"]);
        assert_eq!(tokenize("'quoted'"), vec!["quoted"]);
    }

    #[test]
    fn coordinate_never_below_one() {
        let c = coordinate_of("the the the");
        assert!(c.iter().all(|v| *v >= 1));
        assert_eq!(coordinate_of(""), [9; 9]);
    }
}
