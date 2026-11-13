//! Minimal phext model: an ordered set of scrolls at 9-component
//! coordinates, with the canonical delimiter serialization.
//!
//! Delimiters (ascending dimension): line \x0A inside a scroll's text is
//! content; the scroll structure uses: \x17 (scroll break within a
//! section), \x18 (section), \x19 (chapter), then volume-level breaks
//! \x1A (book) \x1C (volume) \x1D (collection), shelf-level breaks
//! \x1E (shelf) \x1F (library) and \x01 (final scroll separator class).
//! The canonical libphext ordering used here: coordinate components are
//! (library, shelf, scroll_in_shelf | collection, volume, book |
//! chapter, section, scroll) — written L.S.S/C.V.B/C.S.S.
//!
//! Serialization walks coordinates in ascending lexicographic order and
//! emits the delimiter for the highest component that changed. This is
//! the standard dead-reckoning layout: parse splits on the delimiter
//! ladder the same way.

use std::collections::BTreeMap;
use std::fmt;

pub type Coord = [u32; 9];

pub fn parse_coord(s: &str) -> Option<Coord> {
    let parts: Vec<&str> = s.split(|c| c == '.' || c == '/').collect();
    if parts.len() != 9 {
        return None;
    }
    let mut c = [0u32; 9];
    for (i, p) in parts.iter().enumerate() {
        c[i] = p.parse().ok()?;
        if c[i] < 1 {
            return None;
        }
    }
    Some(c)
}

pub fn fmt_coord(c: &Coord) -> String {
    format!(
        "{}.{}.{}/{}.{}.{}/{}.{}.{}",
        c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8]
    )
}

impl fmt::Display for CoordFmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", fmt_coord(&self.0))
    }
}
pub struct CoordFmt(pub Coord);

/// Delimiter for the boundary *above* component index k (0-based from the
/// most-significant component). When walking ascending coordinates, the
/// separator emitted between two scrolls is the delimiter of the highest
/// (most significant) component that differs.
fn delim_for(changed_from: usize) -> u8 {
    // components: 0 library, 1 shelf, 2 scroll(shelf)
    //             3 collection, 4 volume, 5 book
    //             6 chapter, 7 section, 8 scroll
    match changed_from {
        0 => 0x02, // library break (above \x1F class; SOH-family sentinel)
        1 => 0x1F, // shelf break
        2 => 0x1E, // shelf-scroll break
        3 => 0x1D, // collection break
        4 => 0x1C, // volume break
        5 => 0x1A, // book break
        6 => 0x19, // chapter break
        7 => 0x18, // section break
        _ => 0x17, // scroll break
    }
}

fn delim_rank(b: u8) -> Option<usize> {
    // returns the component index whose boundary this delimiter closes
    // (all components >= that index reset/increment semantics handled by
    // the parser's position tracking).
    match b {
        0x02 => Some(0),
        0x1F => Some(1),
        0x1E => Some(2),
        0x1D => Some(3),
        0x1C => Some(4),
        0x1A => Some(5),
        0x19 => Some(6),
        0x18 => Some(7),
        0x17 => Some(8),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Phext {
    pub scrolls: BTreeMap<Coord, String>,
}

impl Phext {
    pub fn new() -> Phext {
        Phext { scrolls: BTreeMap::new() }
    }

    pub fn insert(&mut self, coord: Coord, text: String) {
        self.scrolls.insert(coord, text);
    }

    pub fn get(&self, coord: &Coord) -> Option<&String> {
        self.scrolls.get(coord)
    }

    pub fn toc(&self) -> Vec<(Coord, usize)> {
        self.scrolls.iter().map(|(c, t)| (*c, t.len())).collect()
    }

    /// Serialize canonically: ascending coordinates, walking the
    /// dead-reckoning path from each scroll to the next. A single
    /// delimiter advances ONE component by ONE — so reaching a
    /// distant address takes a STACK of delimiters (component by
    /// component, most significant first, each emission resetting
    /// the less-significant components to 1), with no text between
    /// them. That is how CYOA's sparse landscape is actually laid
    /// down (a scroll at 13.13.13 is reached by climbing, not by
    /// one jump), and it is the only serialization parse can read
    /// back faithfully: an earlier form of this function emitted
    /// just the highest changed component's delimiter, which made
    /// every jump of more than 1 in any component land at the
    /// wrong address on re-read — patterned homeland addresses
    /// (2.2.2/2.2.2/2.2.2 and kin) were unrepresentable. Found by
    /// the v1.0 battery, 2026-11-12: records a command had just
    /// written could not be found by the next process.
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        let mut cur: Coord = [1; 9];
        for (coord, text) in &self.scrolls {
            // Climb from wherever the walk stands to this scroll's
            // address (for a scroll at the origin — a document's
            // home — the climb emits nothing).
            for k in 0..9 {
                while cur[k] < coord[k] {
                    out.push(delim_for(k) as char);
                    cur[k] = cur[k].saturating_add(1);
                    for j in (k + 1)..9 {
                        cur[j] = 1;
                    }
                }
            }
            out.push_str(text);
            cur = *coord;
        }
        out
    }

    /// Parse the canonical serialized form. Position starts at
    /// 1.1.1/1.1.1/1.1.1; each delimiter increments its component
    /// and resets all less-significant components to 1. A scroll
    /// EXISTS only when it holds at least a byte: delimiter stacks
    /// (the climb to a distant address) pass through intermediate
    /// positions without minting empty scrolls there.
    pub fn parse(text: &str) -> Phext {
        let mut px = Phext::new();
        let mut pos: Coord = [1; 9];
        let mut cur = String::new();
        for ch in text.chars() {
            let b = ch as u32;
            if b < 0x20 && b != 0x0A && b != 0x09 && b != 0x0D {
                if let Some(idx) = delim_rank(b as u8) {
                    if !cur.is_empty() {
                        px.insert(pos, std::mem::take(&mut cur));
                    }
                    pos[idx] = pos[idx].saturating_add(1);
                    for k in (idx + 1)..9 {
                        pos[k] = 1;
                    }
                    continue;
                }
            }
            cur.push(ch);
        }
        if !cur.is_empty() {
            px.insert(pos, cur);
        }
        px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coord_parse_format_roundtrip() {
        let c = parse_coord("2.3.5/7.11.13/17.19.23").unwrap();
        assert_eq!(fmt_coord(&c), "2.3.5/7.11.13/17.19.23");
        assert!(parse_coord("1.1.1/1.1.1/0.1.1").is_none()); // below floor
        assert!(parse_coord("1.1.1/1.1.1").is_none());
    }

    #[test]
    fn sparse_patterned_addresses_roundtrip() {
        // The homeland pattern: distant, patterned addresses
        // reached by delimiter stacks. Regression test for the
        // single-delimiter serializer, which landed every jump
        // of >1 at the wrong address on re-read.
        let mut px = Phext::new();
        px.insert([1, 1, 1, 1, 1, 1, 1, 1, 1], "home".into());
        px.insert([1, 1, 1, 1, 1, 1, 1, 1, 2], "identity".into());
        px.insert([2, 2, 2, 2, 1, 1, 1, 1, 1], "node key".into());
        px.insert([2, 2, 2, 2, 2, 2, 2, 2, 2], "keys anchor".into());
        px.insert([3, 3, 3, 2, 1, 1, 1, 1, 1], "peer record".into());
        px.insert([3, 3, 3, 3, 3, 3, 3, 3, 3], "peers anchor".into());
        px.insert([9, 9, 9, 9, 9, 9, 9, 9, 9], "far corner".into());
        px.insert([13, 13, 13, 13, 13, 13, 13, 13, 15], "the test sits at .15".into());
        let back = Phext::parse(&px.serialize());
        assert_eq!(back.scrolls, px.scrolls);
        // the climb mints no empty scrolls at intermediate stops
        assert!(back.scrolls.values().all(|t| !t.is_empty()));
        assert_eq!(back.scrolls.len(), 8);
    }

    #[test]
    fn serialize_parse_roundtrip() {
        let mut px = Phext::new();
        px.insert([1, 1, 1, 1, 1, 1, 1, 1, 1], "home".into());
        px.insert([1, 1, 1, 1, 1, 1, 1, 1, 2], "second scroll".into());
        px.insert([1, 1, 1, 1, 1, 1, 1, 2, 1], "new section".into());
        px.insert([1, 1, 1, 1, 1, 2, 1, 1, 1], "new book".into());
        px.insert([2, 1, 1, 1, 1, 1, 1, 1, 1], "new library".into());
        let s = px.serialize();
        let back = Phext::parse(&s);
        assert_eq!(back.scrolls, px.scrolls);
    }
}
