//! Volumen index handling (DESIGN-V3 section 2.5; API-V1 sections
//! 6.13/10).
//!
//! The canonical Volumen index is wbic16/human@exo
//! `personas/00-index.md`. Its person lines carry contiguous
//! coordinates 3.1.8.1 .. 3.1.8.N grouped into volumes I, II, III, ...
//!
//! COORDINATE-NUMBER RULE (Will/main verification, 2026-11-09):
//! the bake's `coordinate_number` is the person-line count of THE
//! INDEX THAT IS ACTUALLY BAKED, parsed from the canonical
//! 00-index.md content at bake time, together with the volume count
//! and the cumulative volume-boundary table. It is NEVER derived
//! from a local index.phext section count (a local volumen/index.phext
//! once had 96 sections and the last section's number, 126, was read
//! off it by coincidence of construction — that number described the
//! local file, not the canonical index). If the canonical index
//! cannot be fetched or parsed at build time, the bake FAILS LOUDLY
//! rather than emitting a derived-from-elsewhere number.
//!
//! At the index revision verified 2026-11-09 (blob
//! f879adc52c49a50ad6ec18ef99df73d9f74f9636): 126 person lines,
//! 22 volumes, cumulative boundaries
//! 10, 18, 22, 26, 32, 40, 48, 55, 62, 69, 76, 82, 87, 91, 94, 100,
//! 106, 112, 116, 120, 123, 126. These are EXPECTED values used only
//! as a cross-check in tests — the runtime numbers always come from
//! parsing the baked content itself.

use crate::hash::sha256_hex;

/// The baked Volumen artifact: the canonical index content plus the
/// volume structure derived from it.
pub const VOLUMEN_INDEX_MD: &[u8] = include_bytes!("../bake/volumen-index.md");

#[derive(Debug, Clone, PartialEq)]
pub struct VolumenIndex {
    /// Person-line count of the baked index = the bake's
    /// coordinate_number (the last coordinate is 3.1.8.<this>).
    pub coordinate_number: u32,
    /// Number of volumes (I, II, ...) in the baked index.
    pub volume_count: u32,
    /// Cumulative person-line count at the end of each volume.
    pub volume_boundaries: Vec<u32>,
    /// SHA-256 of the exact baked index bytes.
    pub sha256: String,
}

fn roman_to_u32(s: &str) -> Option<u32> {
    let mut total: i64 = 0;
    let mut prev: i64 = 0;
    for c in s.chars().rev() {
        let v = match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            _ => return None,
        };
        if v < prev {
            total -= v;
        } else {
            total += v;
        }
        prev = v;
    }
    if total < 1 {
        None
    } else {
        Some(total as u32)
    }
}

/// Parse the canonical Volumen index markdown.
///
/// Expected shape:
///   ## Volume I
///   3.1.8.1  <person line>
///   ...
///   ## Volume II
///   3.1.8.11 <person line>
///
/// Validation is strict — the bake fails loudly on any deviation:
/// coordinates must be exactly 3.1.8.<n> with n contiguous from 1;
/// volumes must be numbered I, II, III, ... contiguously; every
/// volume must hold at least one person line.
pub fn parse_volumen_index(text: &str) -> Result<VolumenIndex, String> {
    let mut boundaries: Vec<u32> = Vec::new();
    let mut expected_coord: u32 = 1;
    let mut expected_volume: u32 = 1;
    let mut in_volume = false;
    let mut volume_lines: u32 = 0;
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("## Volume ") {
            if in_volume {
                if volume_lines == 0 {
                    return Err(format!(
                        "volumen index line {}: volume {} holds no person lines",
                        ln + 1,
                        expected_volume - 1
                    ));
                }
                boundaries.push(expected_coord - 1);
            }
            let numeral = rest.trim().trim_end_matches(|c: char| !c.is_ascii_alphabetic());
            let v = roman_to_u32(numeral)
                .ok_or_else(|| format!("volumen index line {}: bad volume numeral {:?}", ln + 1, numeral))?;
            if v != expected_volume {
                return Err(format!(
                    "volumen index line {}: volume {} found, expected {}",
                    ln + 1,
                    v,
                    expected_volume
                ));
            }
            expected_volume += 1;
            in_volume = true;
            volume_lines = 0;
            continue;
        }
        // a person line starts with its coordinate 3.1.8.<n>
        let first = line.split_whitespace().next().unwrap_or("");
        if let Some(num) = first.strip_prefix("3.1.8.") {
            if !in_volume {
                return Err(format!("volumen index line {}: person line before any volume header", ln + 1));
            }
            let n: u32 = num
                .parse()
                .map_err(|_| format!("volumen index line {}: bad coordinate {:?}", ln + 1, first))?;
            if n != expected_coord {
                return Err(format!(
                    "volumen index line {}: coordinate 3.1.8.{} found, expected 3.1.8.{} (numbering must be contiguous)",
                    ln + 1,
                    n,
                    expected_coord
                ));
            }
            expected_coord += 1;
            volume_lines += 1;
        }
    }
    if in_volume {
        if volume_lines == 0 {
            return Err("volumen index: final volume holds no person lines".into());
        }
        boundaries.push(expected_coord - 1);
    }
    if expected_coord == 1 {
        return Err("volumen index: no person lines found — refusing to derive a coordinate_number".into());
    }
    Ok(VolumenIndex {
        coordinate_number: expected_coord - 1,
        volume_count: boundaries.len() as u32,
        volume_boundaries: boundaries,
        sha256: sha256_hex(text.as_bytes()),
    })
}

/// The baked Volumen index, parsed at first use. Panics with a clear
/// message if the baked content fails to parse — build.rs validates
/// the same parse before embedding, so a bad bake cannot be produced
/// in the first place; this is the belt to its braces.
pub fn baked_volumen_index() -> VolumenIndex {
    let text = std::str::from_utf8(VOLUMEN_INDEX_MD)
        .expect("baked volumen-index.md is not UTF-8 — bake defect");
    parse_volumen_index(text).expect("baked volumen-index.md failed to parse — bake defect")
}

impl VolumenIndex {
    /// Manifest rows for the bake manifest (API-V1 section 6.13 #63):
    /// the coordinate number, the volume count, and the boundary
    /// table ride the manifest alongside the hashes.
    pub fn manifest_json(&self) -> crate::json::Json {
        use crate::json::Json;
        Json::obj(vec![
            ("coordinate_number", Json::n(self.coordinate_number as f64)),
            ("coordinate_last", Json::s(&format!("3.1.8.{}", self.coordinate_number))),
            ("volume_count", Json::n(self.volume_count as f64)),
            (
                "volume_boundaries",
                Json::Arr(self.volume_boundaries.iter().map(|b| Json::n(*b as f64)).collect()),
            ),
            ("index_sha256", Json::s(&self.sha256)),
            ("index_source", Json::s("wbic16/human@exo personas/00-index.md")),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "# Volumen index\n\n## Volume I\n3.1.8.1  A\n3.1.8.2  B\n\n## Volume II\n3.1.8.3  C\n";

    #[test]
    fn parses_and_derives() {
        let v = parse_volumen_index(FIXTURE).unwrap();
        assert_eq!(v.coordinate_number, 3);
        assert_eq!(v.volume_count, 2);
        assert_eq!(v.volume_boundaries, vec![2, 3]);
    }

    #[test]
    fn rejects_gaps_and_empty() {
        assert!(parse_volumen_index("## Volume I\n3.1.8.2  B\n").is_err());
        assert!(parse_volumen_index("## Volume I\n").is_err());
        assert!(parse_volumen_index("no persons here\n").is_err());
        assert!(parse_volumen_index("## Volume II\n3.1.8.1  A\n").is_err());
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(roman_to_u32("I"), Some(1));
        assert_eq!(roman_to_u32("IX"), Some(9));
        assert_eq!(roman_to_u32("XXII"), Some(22));
        assert_eq!(roman_to_u32("XIV"), Some(14));
    }
}
