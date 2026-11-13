//! The phext format — libphext-rs's reckoning, not a local one.
//!
//! Through v1.2.0 this module carried a hand-rolled parser and
//! serializer. It diverged from libphext-rs in exactly one
//! delimiter and read its own files happily for seven versions,
//! because qexo was the only reader: its library break was
//! \x02 (STX), where libphext — the reference implementation,
//! the parser SQ and Phext Notepad share — uses \x01 (SOH).
//! Every other break byte agreed, which is why the divergence
//! stayed invisible until a file crossed tools: on 2026-11-13 a
//! Phext Notepad edit appended to a node's state.phext landed
//! at a different address under each reader, and the node's
//! `qexo update` kept refusing a signer its operator had just
//! written. (Diagnosis and measurements: the v1.3.0 changelog.)
//!
//! As of v1.3.0 the reckoning is the library's, full stop. This
//! module is only the adapter between qexo's document model (a
//! BTreeMap from [u32; 9] coordinates to scroll texts — the
//! shape lattice.rs and the Layer 2 addressing are written
//! against) and libphext::phext's string primitives (explode /
//! implode). Address computation, delimiter emission, and
//! duplicate-address behavior are whatever libphext does; the
//! old model's delimiter table and walk are gone. Nothing else
//! was genuinely qexo-specific in the old file — its tests were
//! the only other content, and they are carried below, plus the
//! regression that pins tonight's file shape.
//!
//! One qexo rule survives, because it is the format's own
//! (incipit: a scroll exists when it holds at least a byte):
//! explode also names the empty stops on a delimiter climb;
//! qexo's model keeps only scrolls that hold text. The
//! addresses and texts of real scrolls are the library's,
//! unchanged.

use std::collections::BTreeMap;
use std::fmt;

use libphext::phext as lp;

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

fn to_lib(c: &Coord) -> lp::Coordinate {
    let mut out = lp::Coordinate::default();
    out.z.library = c[0] as usize;
    out.z.shelf = c[1] as usize;
    out.z.series = c[2] as usize;
    out.y.collection = c[3] as usize;
    out.y.volume = c[4] as usize;
    out.y.book = c[5] as usize;
    out.x.chapter = c[6] as usize;
    out.x.section = c[7] as usize;
    out.x.scroll = c[8] as usize;
    out
}

fn from_lib(c: &lp::Coordinate) -> Coord {
    let conv = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    [
        conv(c.z.library),
        conv(c.z.shelf),
        conv(c.z.series),
        conv(c.y.collection),
        conv(c.y.volume),
        conv(c.y.book),
        conv(c.x.chapter),
        conv(c.x.section),
        conv(c.x.scroll),
    ]
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

    /// Serialize with the library: implode sorts the scrolls
    /// and walks the dead-reckoning path emitting libphext's
    /// delimiter ladder (library break \x01 at the top).
    pub fn serialize(&self) -> String {
        let mut map = std::collections::HashMap::new();
        for (coord, text) in &self.scrolls {
            map.insert(to_lib(coord), text.clone());
        }
        lp::implode(map)
    }

    /// Parse with the library: explode reckons every scroll's
    /// address libphext's way; this adapter then drops the
    /// empty stops (see the module note — a scroll exists when
    /// it holds at least a byte).
    pub fn parse(text: &str) -> Phext {
        let mut px = Phext::new();
        for (coord, body) in lp::explode(text) {
            if !body.is_empty() {
                px.insert(from_lib(&coord), body);
            }
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
        // reached by delimiter stacks. (Carried from the
        // hand-rolled model, where it guarded the serializer
        // against single-delimiter jumps; it still guards the
        // adapter + library pair against the same class.)
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

    #[test]
    fn library_break_is_soh_not_stx() {
        // The one-byte divergence that cost seven versions
        // their interop: libphext's library break is \x01.
        // The hand-rolled model used \x02 there and read \x01
        // as scroll content.
        let px = Phext::parse("alpha\u{1}beta");
        assert_eq!(px.get(&[1, 1, 1, 1, 1, 1, 1, 1, 1]).map(|s| s.as_str()), Some("alpha"));
        assert_eq!(px.get(&[2, 1, 1, 1, 1, 1, 1, 1, 1]).map(|s| s.as_str()), Some("beta"));
        // ...and \x02 is ordinary content to the library.
        let px2 = Phext::parse("alpha\u{2}beta");
        assert_eq!(px2.scrolls.len(), 1);
        assert_eq!(px2.get(&[1, 1, 1, 1, 1, 1, 1, 1, 1]).map(|s| s.as_str()), Some("alpha\u{2}beta"));
    }

    /// Tonight's file shape, byte for byte: a state born by
    /// the hand-rolled v1.2.0 serializer (a sandbox birth on
    /// the hub, 2026-11-13) with Phext Notepad's append at the
    /// end — SOH×10, US×8, RS×8, GS×1, then the filled signer
    /// scroll — exactly as it landed in Will's laptop state.
    const EDITOR_APPEND_FIXTURE: &str = concat!(
    "qexo state — one address space\n\nThis file is the node's whole state: every scrap of it —\nmetadata",
    ", keys, grants, personas, plan, ledgers, sessions —\nas scrolls in one phext, addressed by meaning (",
    "Layer 2).\nAddresses are places; this scroll is the map, and the code\nkeeps it current. Records are",
    " truth; indexes are derived\nprose, rebuilt from the records by `qexo doctor --fix`.\n\nnode: qexo n",
    "ode aletheia-core (aletheia-core)\nborn: 2026-11-13 07:53 UTC\nidentity: 1.1.1/1.1.1/1.1.2\n\nHomela",
    "nds:\n1.1.1/1.1.1/1.1.1 — this index; node identity beside it at 1.1.1/1.1.1/1.1.2\n2.2.2/2.2.2/2.2.",
    "2 — keys & trust: node key CFA0ED35…142C0BEB, 0 record(s) in the neighboring runs\n3.3.3/3.3.3/3.3.3",
    " — peers: 0 registered\n4.4.4/4.4.4/4.4.4 — personas: none yet\n5.5.5/5.5.5/5.5.5 — plan: no plan ye",
    "t\n6.6.6/6.6.6/6.6.6 — ledgers & receipts: 1 entr(ies) across all kinds\n7.7.7/7.7.7/7.7.7 — mailbox",
    " & sessions: 0 hosted now\n8.8.8/8.8.8/8.8.8 — telemetry & forecasts: 0 disk observation(s)\n9.9.9/9",
    ".9.9/9.9.9 — config & wiring: the machine room\n\u{17}kind: node identity\nname: qexo node aletheia-",
    "core\nnode: aletheia-core\nborn: 2026-11-13 07:53 UTC\nqexo: v1.2.0\nstate: /home/wbic16/interop-dia",
    "g/state.phext\n\u{2}\u{1f}\u{1e}\u{1d}kind: node key\nname: qexo node aletheia-core\nfingerprint: CF",
    "A0ED35186497687125F818C4547C14142C0BEB\nalgorithm: eddsa/ed25519, suite: gpg\nstatus: active\nissued",
    ": 2026-11-13 07:53 UTC\npublic key: 2.2.2/2.1.1/1.1.2\n\u{17}-----BEGIN PGP PUBLIC KEY BLOCK-----\n",
    "\nmDMEavbB4xYJKwYBBAHaRw8BAQdAW/lfDRwsybYXT5VEEsjFLWjqzZJtd7lSZwu+\npA9G8K20J3FleG8gbm9kZSBhbGV0aGVp",
    "YS1jb3JlIChxZXhvIG5vZGUga2V5KYiT\nBBMWCgA7FiEEz6DtNRhkl2hxJfgYxFR8FBQsC+sFAmr2weMCGwMFCwkIBwICIgIG\n",
    "FQoJCAsCBBYCAwECHgcCF4AACgkQxFR8FBQsC+vYGwD9Fv8xFqbcTCGPBty6+mqi\nQRwSk0IdBHojfIH0JwEcYFoA/17s+itdTO",
    "SFcvhe8/tBwQYpuTTkoZ9B+IpeW4Z3\nyOkG\n=HiwD\n-----END PGP PUBLIC KEY BLOCK-----\n\u{1c}\u{1a}\u{19}",
    "\u{18}\u{17}keys & trust — homeland anchor 2.2.2/2.2.2/2.2.2\n\nTwo parties, a key between them: the",
    " node's own key first,\nthen persona keys, peer keys, grants, delegations, and policy,\nas neighbori",
    "ng runs.\n\n- node key 2.2.2/2.1.1/1.1.1 — fingerprint CFA0ED35186497687125F818C4547C14142C0BEB — st",
    "atus active\n- policy: 0 rule scroll(s) in the run from 2.2.2/7.1.1/1.1.1\n\u{2}\u{1f}\u{1f}\u{1e}",
    "\u{1e}\u{1d}\u{1d}\u{1c}\u{1c}\u{1a}\u{1a}\u{19}\u{19}\u{18}\u{18}\u{17}\u{17}peers — homeland ancho",
    "r 3.3.3/3.3.3/3.3.3\n\nThree-way handshake country: one cluster per registered peer,\nits record fir",
    "st and its armored public key beside it.\n\nno peers registered yet — qexo register <pubkey> adds on",
    "e.\n\u{2}\u{1f}\u{1f}\u{1f}\u{1e}\u{1e}\u{1e}\u{1d}\u{1d}\u{1d}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}",
    "\u{19}\u{19}\u{19}\u{18}\u{18}\u{18}\u{17}\u{17}\u{17}personas — homeland anchor 4.4.4/4.4.4/4.4.4\n",
    "\nOne cluster per persona: record, boot text as an adjacent\nscroll run, live state, and transcript.",
    "\n\nno personas yet — qexo host <persona> brings one home.\n\u{2}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}",
    "\u{1e}\u{1e}\u{1e}\u{1d}\u{1d}\u{1d}\u{1d}\u{1c}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}\u{1a}\u{19}",
    "\u{19}\u{19}\u{19}\u{18}\u{18}\u{18}\u{18}\u{17}\u{17}\u{17}\u{17}plan — homeland anchor 5.5.5/5.5.5",
    "/5.5.5\n\nno plan emitted yet — bare qexo computes the first.\nhistory: 0 emitted plan(s) in the run",
    " from 5.5.5/3.1.1/1.1.1\n\u{2}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1d}kind",
    ": receipt\nat: 2026-11-13 07:53 UTC\naction: config\nnote: node born; state.phext initialized with t",
    "he homeland map\n\u{1d}\u{1d}\u{1d}\u{1d}\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}\u{1a}",
    "\u{1a}\u{19}\u{19}\u{19}\u{19}\u{19}\u{18}\u{18}\u{18}\u{18}\u{18}\u{17}\u{17}\u{17}\u{17}\u{17}ledg",
    "ers & receipts — homeland anchor 6.6.6/6.6.6/6.6.6\n\nThe beast keeps the books: each kind its own r",
    "un of entry\nscrolls, appended beside its kin.\n\n- receipts: 1 entr(ies) from 6.6.6/2.1.1/1.1.1\n- ",
    "teleports: 0 entr(ies) from 6.6.6/3.1.1/1.1.1\n- instances: 0 entr(ies) from 6.6.6/4.1.1/1.1.1\n- em",
    "otion: 0 entr(ies) from 6.6.6/5.1.1/1.1.1\n- goal-activity: 0 entr(ies) from 6.6.6/6.1.1/1.1.1\n- sy",
    "nc-receipts: 0 entr(ies) from 6.6.6/7.1.1/1.1.1\n- releases: 0 entr(ies) from 6.6.6/8.1.1/1.1.1\n",
    "\u{2}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1d}\u{1d}\u{1d}",
    "\u{1d}\u{1d}\u{1d}\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{19}",
    "\u{19}\u{19}\u{19}\u{19}\u{19}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{17}\u{17}\u{17}\u{17}\u{17}",
    "\u{17}mailbox & sessions — homeland anchor 7.7.7/7.7.7/7.7.7\n\nhosted now: none.\nmailbox: 0 messag",
    "e scroll(s) in the run from 7.7.7/3.1.1/1.1.1\n\u{2}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}",
    "\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1c}\u{1c}\u{1c}",
    "\u{1c}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{19}\u{19}\u{19}\u{19}\u{19}",
    "\u{19}\u{19}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{17}\u{17}\u{17}\u{17}\u{17}\u{17}\u{17}tele",
    "metry & forecasts — homeland anchor 8.8.8/8.8.8/8.8.8\n\nSignals country: disk observations as a run",
    " of scrolls; the\nforecast gate reads the last two, as it always has.\n\ndisk observations: 0\n\u{2}",
    "\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}",
    "\u{1d}kind: qexo config\nollama endpoint: http://127.0.0.1:11434\nmodel: gemma4:e4b\ntasks source: ",
    "\ndisk root: /home/wbic16/workspace\nvolume_lens: on\ndisk_gate_free_amber_pct: 25\ndisk_gate_free_r",
    "ed_pct: 10\nquota_lens: off\nquota_amber_gb: 95\nquota_red_gb: 102\nstaging_root: /home/wbic16/inter",
    "op-diag\nfile_amber: 1100000\nfile_red: 1300000\nserve_bind: 127.0.0.1\nauto_update: off\nupdate_int",
    "erval_hours: 24\nrelease_signer: \nevolve: off\n\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1d}\u{1c}",
    "\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1c}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{1a}\u{19}",
    "\u{19}\u{19}\u{19}\u{19}\u{19}\u{19}\u{19}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{18}\u{17}",
    "\u{17}\u{17}\u{17}\u{17}\u{17}\u{17}\u{17}config & wiring — homeland anchor 9.9.9/9.9.9/9.9.9\n\nThe",
    " far corner is the machine room: qexo's own settings live\nin the config record here, as field lines",
    " — there is no config\nfile anywhere else. The wiring registry (one scroll per module)\nfollows it.",
    "\n\nconfig record: 9.9.9/2.1.1/1.1.1\n\u{1}\u{1}\u{1}\u{1}\u{1}\u{1}\u{1}\u{1}\u{1}\u{1}\u{1f}\u{1f}",
    "\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1f}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1e}\u{1d}release_si",
    "gner: A4E7277FB3EB0E589604BE5B506DBE3FC209E755",
    );

    #[test]
    fn editor_append_lands_where_libphext_puts_it() {
        let px = Phext::parse(EDITOR_APPEND_FIXTURE);
        // Where the reference reckoning puts the appended
        // signer scroll in this document — measured identically
        // by SQ (libphext 0.3.3, `sq select`) and by libphext
        // 0.5.0 explode/fetch before this test was written.
        assert_eq!(
            px.get(&[11, 9, 9, 2, 1, 1, 1, 1, 1]).map(|s| s.trim()),
            Some("release_signer: A4E7277FB3EB0E589604BE5B506DBE3FC209E755")
        );
        // Where the hand-rolled parser had put it (its SOH
        // bytes were content; the shelf breaks overshot).
        assert!(px.get(&[9, 17, 9, 2, 1, 1, 1, 1, 1]).is_none());
        // The append does not shadow the config record: nothing
        // at the config address carries the fingerprint, which
        // is why the node's update refused even though its
        // operator had "written" the signer. (In this document
        // the record itself sits at 1.37.9/2.1.1/1.1.1 under
        // the library's walk — the \x02 breaks the old
        // serializer climbed libraries with are content bytes
        // to libphext, so its shelves overshoot by every reset
        // they skipped.)
        let at_config = px.get(&[9, 9, 9, 2, 1, 1, 1, 1, 1]);
        assert!(at_config.map(|t| !t.contains("A4E7277F")).unwrap_or(true));
        let moved = px
            .get(&[1, 37, 9, 2, 1, 1, 1, 1, 1])
            .expect("config record under the library's walk");
        assert!(moved.contains("kind: qexo config"));
        assert!(moved.contains("release_signer: \n") || moved.contains("release_signer:\n"));
    }
}
