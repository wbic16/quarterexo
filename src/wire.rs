//! Wire dispatch — the dispatch core's slice 4 (spec
//! §13.3 slice 4): the bundle format and `POST
//! /v1/dispatch` (§3.4), the acceptance rule and dedup
//! (§§3.4, 4.3), placement — the fair draw over the
//! eligible set (§6) — heartbeat and checkpoints (§6),
//! and the executor's failure receipt (§4.6).
//!
//! The bundle is a chain on the wire. $1: is the spec
//! universe: the manifest scroll at the bundle's own
//! anchor (11.11.11/11.11.11/11.11.11), the detached
//! signature over the manifest's exact bytes beside it
//! at Scroll 2, and the item record copy + brief at
//! their home addresses. $2:…$k: are context universes
//! — scroll selections with provenance, read-only
//! evidence. Bundle identity is (item address,
//! bundle-sha256), where the hash is over the canonical
//! chain bytes of the UNSIGNED bundle (the signature
//! scroll excluded): the manifest cannot carry its own
//! hash, a signature is not reproducible, and §4.3's
//! rule — re-dispatching an unchanged item reproduces
//! the same identity — requires a hash every honest
//! builder recomputes identically. The manifest's
//! per-universe hash list covers the context universes
//! ($2:…$k:); $1:'s payload rides under the bundle hash
//! itself and the signature. The doctor's
//! combination-6 recomputation (§4.6) rebuilds exactly
//! this hash from the attempt universe an acceptance
//! filed — see `recompute_bundle_sha256`.
//!
//! Placement follows §6: eligibility is judged from
//! facts already in state — peer records (3.3.3), each
//! candidate's own config scroll and latest telemetry
//! observation read over the shipped scroll route —
//! and the draw is a pure function of the eligible set
//! and a seed; the seed is receipted on the assignment
//! record (the placement receipt), so a round is
//! reproducible and fairness is auditable after the
//! fact. The origin never defaults: an empty eligible
//! set is a named, receipted outcome and the item
//! stays queued.
//!
//! Heartbeat is one verb with two sides, decided by
//! the caller's own state. On the executing node it
//! beats: the `heartbeat:` line lands on the
//! assignment record and the attempt universe's record
//! scroll (§6), and the first beat moves the item
//! dispatched → running through the §4.6 oracle. On
//! the item's home node it reads the executor's
//! assignment record (and its attempt scroll) over
//! /v1/scroll/<coord> — no new transport — and applies
//! what it finds to the home record through the same
//! oracle: the first-beat transition, a failure
//! transition carrying the executor's evidence,
//! checkpoint cites.
//!
//! Slice 5 (§13.3 slice 5) completes F5 (§8) on this
//! foundation: **failure detection** — the origin's
//! heartbeat pass is the coordinator act that declares
//! an executor lost on either evidence: its serve
//! unreachable with 3 consecutive beats missed, or
//! its serve answering while its beats have stopped
//! (silent). Detection files a `failure detected`
//! receipt, closes the attempt through the oracle
//! (the third consecutive failure lands in
//! paused-auto, §8), retires the origin's assignment,
//! and — when the item lands queued — teleports it:
//! a fresh `dispatch` in a fresh fair-draw round whose
//! bundle carries `resume-from:` citing the last
//! checkpoint (§6). **Checkpoint custody:** a
//! reconcile that meets a new checkpoint cite reads
//! the checkpoint scroll back and files it into the
//! origin's own stratum for the attempt, so a
//! teleport's bundle can carry the checkpoint itself
//! as a resume universe — the resume currency (§6)
//! survives the node that minted it. **Verified
//! progress:** a checkpoint may cite outputs
//! (`--output <coord>`, hashed at checkpoint time);
//! the origin reads each cited output back over the
//! scroll route and compares hashes — only a fully
//! verified checkpoint resets the item's consecutive-
//! failure counter, with a `progress verified`
//! receipt in the same save (§8: never the executor's
//! say-so). And the acceptance fencing is completed:
//! a bundle below the highest attempt this node has
//! filed — live or already closed — is refused as
//! stale, so a dead executor's late bundle cannot
//! move state (§4.6 combination 2).

use crate::core::{append_receipt, now_stamp, require_born};
use crate::crypto::Gpg;
use crate::dispatch::{self, Actor, Item, ItemState, Transition};
use crate::hash::sha256_hex;
use crate::lattice::{self, Lattice};
use crate::phext::{self, Coord, Phext};
use std::io::{Read, Write};
use std::net::TcpStream;

/// The bundle manifest's address inside $1: — the
/// bundle's own 11.11.11/11.11.11/11.11.11 (§3.4).
pub const BUNDLE_ANCHOR: Coord = [11, 11, 11, 11, 11, 11, 11, 11, 11];
/// The detached signature beside the manifest — the
/// next scroll of the anchor's Book (§3.4: "beside it
/// at Scroll 2").
pub const BUNDLE_SIG: Coord = [11, 11, 11, 11, 11, 11, 11, 11, 12];

/// Where an attempt's paperwork lives in its stratum:
/// the assignment's own Book family, mirrored into the
/// attempt universe — the manifest at Scroll 1, the
/// provenance for context universe $k at Scroll k. An
/// attempt universe is shared by every item's attempt
/// N, so the bundle anchor itself cannot hold them
/// (two items' manifests would overwrite each other);
/// the assignment Book is unique per acceptance, so
/// its family in the stratum is unique per attempt.
fn stratum_manifest_addr(assignment_book: u32) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, assignment_book, 1, 1, 1])
}

fn stratum_provenance_addr(assignment_book: u32, k: usize) -> Coord {
    lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, assignment_book, 1, 1, k as u32])
}

/// The thermal gate (§6): a node at or above 75 °C
/// fails eligibility (b).
pub const THERMAL_LIMIT_C: f64 = 75.0;
/// The heavy-work cap (§6): at most 4 heavy items
/// running fleet-wide, FIFO among heavy items.
pub const HEAVY_CAP: usize = 4;

/// Every line in `text` carrying `key:`, in order —
/// `lattice::field` reads the first; capability lists,
/// exclusion lists, and hash lists are plural.
pub fn field_all(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("{}:", key);
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix(&prefix) {
            out.push(rest.trim().to_string());
        }
    }
    out
}

/// Remove every line carrying `key:` from a record
/// scroll — the inverse of the appends `set_field`
/// makes, used to reconstruct an as-bundled record
/// from its working copy (the doctor's recomputation).
fn remove_field(text: &str, key: &str) -> String {
    let prefix = format!("{}:", key);
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        if !line.trim_start().starts_with(&prefix) {
            out.push(line);
        }
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// This node's name (the identity record's `node:`
/// field) — placement and receipts name nodes by it.
fn local_node(px: &Phext) -> String {
    px.get(&lattice::IDENTITY)
        .and_then(|t| lattice::field(t, "node"))
        .unwrap_or_else(|| "unknown-node".to_string())
}

// ----------------------------------------------------------------
// The brief's dispatch facts (§6): capability
// requirements, the heavy flag, context citations.
// ----------------------------------------------------------------

/// What placement and the bundle builder read from an
/// item's brief scroll: `capability:` lines
/// (repeatable), `heavy: yes`, and `context:`
/// citations naming scroll selections in the origin's
/// state (one coordinate, or a range `coord..coord`,
/// inclusive, in coordinate order).
#[derive(Debug, Clone, Default)]
pub struct BriefFacts {
    pub capabilities: Vec<String>,
    pub heavy: bool,
    pub contexts: Vec<(Coord, Coord)>,
    /// The item's acceptable bridges in preference
    /// order (§9.2): `bridge:` lines in the brief.
    pub bridges: Vec<String>,
}

pub fn brief_facts(brief: &str) -> BriefFacts {
    let mut out = BriefFacts::default();
    for cap in field_all(brief, "capability") {
        if !cap.is_empty() && !out.capabilities.contains(&cap) {
            out.capabilities.push(cap);
        }
    }
    out.bridges = crate::bridge::brief_prefs(brief);
    out.heavy = lattice::field(brief, "heavy").as_deref() == Some("yes");
    for cite in field_all(brief, "context") {
        if let Some((lo, hi)) = cite.split_once("..") {
            if let (Some(a), Some(b)) = (phext::parse_coord(lo.trim()), phext::parse_coord(hi.trim())) {
                out.contexts.push((a, b));
            }
        } else if let Some(a) = phext::parse_coord(cite.trim()) {
            out.contexts.push((a, a));
        }
    }
    out
}

/// Context citations ride in the brief and in the item
/// Book's Scroll 3+ citation scrolls alike (§3.1:
/// "Scroll 3+ = dependency and context citations").
fn item_contexts(px: &Phext, item: &Item) -> Vec<(Coord, Coord)> {
    let mut out = Vec::new();
    let brief = px.get(&dispatch::brief_addr(item.book)).cloned().unwrap_or_default();
    out.extend(brief_facts(&brief).contexts);
    for (addr, text) in lattice::prefix_scrolls(px, &[11, 11, 11, lattice::DISPATCH_ITEMS_C, 1, item.book]) {
        if addr[8] >= 3 {
            out.extend(brief_facts(text).contexts);
        }
    }
    out
}

// ----------------------------------------------------------------
// The bundle (§3.4) — one builder path: the origin
// builds and signs; the doctor rebuilds and compares.
// ----------------------------------------------------------------

/// The canonical bytes a bundle's hash is taken over:
/// the chain of its universes with the signature
/// scroll removed. Deterministic in the item record,
/// the brief, the context selections, the origin
/// identity, the attempt, and the routed bridge
/// (§9.2 — the choice is cited in the manifest, so a
/// different route is a different bundle) — nothing
/// else (§4.3).
pub fn unsigned_chain_bytes(universes: &[Phext]) -> String {
    let mut us: Vec<Phext> = universes.to_vec();
    if let Some(first) = us.first_mut() {
        let mut clean = Phext::new();
        for (c, _) in first.toc() {
            if c != BUNDLE_SIG {
                if let Some(t) = first.get(&c) {
                    clean.insert(c, t.clone());
                }
            }
        }
        *first = clean;
    }
    Phext::serialize_universes(&us, true)
}

/// Build a bundle's universes (unsigned) and manifest
/// for `item` as it stands in `px` — the record
/// pre-transition (state queued, attempts = attempt−1).
/// `resume_text` is the checkpoint scroll the origin
/// holds in custody for `resume_from`'s cite (§8): when
/// both are present, the checkpoint rides as the
/// bundle's last universe, provenanced like any context
/// universe, so the replacement executor receives the
/// resume currency itself — not just its address.
pub fn build_bundle_parts(
    px: &Phext,
    item: &Item,
    attempt: u32,
    resume_from: Option<&str>,
    resume_text: Option<&str>,
    bridge: Option<(&str, &str)>,
) -> Result<(Vec<Phext>, String), String> {
    let raw_record = px
        .get(&item.addr)
        .cloned()
        .ok_or_else(|| format!("no item record at {}", phext::fmt_coord(&item.addr)))?;
    // The bundle carries the PRISTINE record copy: the
    // fields wire dispatch itself writes over a
    // record's life (a previous attempt's bundle hash,
    // home, heartbeat, checkpoint cites) are stripped,
    // so the copy is the same on attempt 1 and attempt
    // N — and the doctor's recomputation, which strips
    // exactly these fields from the working copy,
    // reproduces the bundle hash at every attempt.
    let mut record_text = remove_field(&raw_record, "bundle-sha256");
    record_text = remove_field(&record_text, "home");
    record_text = remove_field(&record_text, "heartbeat");
    record_text = remove_field(&record_text, "checkpoint");
    let brief = px.get(&dispatch::brief_addr(item.book)).cloned().unwrap_or_default();
    let facts = brief_facts(&brief);
    let origin = dispatch::local_actor(px);

    // Context universes: each citation becomes one
    // universe — a provenance scroll at its anchor
    // (source range, the selection's SHA-256, its
    // scroll list) plus the selected scrolls at their
    // own addresses. Read-only evidence (§3.4).
    let mut contexts: Vec<Phext> = Vec::new();
    for (lo, hi) in item_contexts(px, item) {
        let mut sel = Phext::new();
        for (c, _) in px.toc() {
            if c >= lo && c <= hi {
                if let Some(t) = px.get(&c) {
                    sel.insert(c, t.clone());
                }
            }
        }
        let members: Vec<String> = sel.toc().iter().map(|(c, _)| phext::fmt_coord(c)).collect();
        let prov = format!(
            "bundle provenance\n{}",
            lattice::record(&[
                ("kind", "provenance"),
                ("source", &format!("{}..{}", phext::fmt_coord(&lo), phext::fmt_coord(&hi))),
                ("source-sha256", &sha256_hex(sel.serialize().as_bytes())),
                ("scrolls", &members.join(", ")),
            ])
        );
        let mut u = Phext::new();
        u.insert(BUNDLE_ANCHOR, prov);
        for (c, _) in sel.toc() {
            if let Some(t) = sel.get(&c) {
                u.insert(c, t.clone());
            }
        }
        contexts.push(u);
    }

    // The resume universe (§6 teleport, §8): when this
    // dispatch resumes and the origin holds the cited
    // checkpoint's scroll in custody, the checkpoint
    // rides as the last universe — its provenance names
    // the cite and the scroll's hash, and the scroll
    // itself rides at its own coordinate, so the
    // receiving node's stratum files it where the new
    // attempt's own checkpoints will follow. A cite
    // whose content the origin does not hold still
    // names the resume point in the manifest; the
    // bundle simply carries no resume universe, and
    // the gap is visible in the manifest's hash list.
    if let (Some(cite), Some(text)) = (resume_from, resume_text) {
        if let Some((_, coord)) = phext::parse_universe_addr(cite) {
            let prov = format!(
                "bundle provenance\n{}",
                lattice::record(&[
                    ("kind", "resume-checkpoint"),
                    ("source", cite),
                    ("source-sha256", &sha256_hex(text.as_bytes())),
                    ("scrolls", &phext::fmt_coord(&coord)),
                ])
            );
            let mut u = Phext::new();
            u.insert(BUNDLE_ANCHOR, prov);
            u.insert(coord, text.to_string());
            contexts.push(u);
        }
    }

    // The manifest's per-universe hash list, in
    // universe order (§3.4) — calculated from the
    // assembled universes, so a resume universe takes
    // its place in the list like any other.
    let mut hash_lines: Vec<String> = Vec::new();
    for (i, u) in contexts.iter().enumerate() {
        hash_lines.push(format!(
            "universe: {} sha256: {}",
            i + 2,
            sha256_hex(u.serialize().as_bytes())
        ));
    }

    // The manifest ($1:'s anchor scroll, §3.4): the
    // item's address and its coordinate (or unminted),
    // the origin (node name + fingerprint), the
    // per-universe hash list, the capability lines,
    // value/cost/goal, resume-from when resuming (§8).
    let mut mf: Vec<(&str, String)> = Vec::new();
    mf.push(("item", phext::fmt_coord(&item.addr)));
    mf.push(("coordinate", item.coordinate.clone().unwrap_or_else(|| "unminted".into())));
    mf.push(("origin", origin));
    mf.push(("attempt", attempt.to_string()));
    for cap in &facts.capabilities {
        mf.push(("capability", cap.clone()));
    }
    // The routed bridge (§9.2): the choice the dispatch
    // walk verified live for this dispatch, cited in the
    // manifest so the executor routes its work over it —
    // and so the doctor's recomputation, which reads the
    // filed manifest verbatim, reproduces this bundle.
    if let Some((bname, bendpoint)) = bridge {
        mf.push(("bridge", bname.to_string()));
        mf.push(("bridge-endpoint", bendpoint.to_string()));
    }
    if let Some(v) = item.value {
        mf.push(("value", v.to_string()));
    }
    if let Some(c) = item.cost {
        mf.push(("cost", c.to_string()));
    }
    mf.push(("goal", item.goal.clone()));
    if let Some(r) = resume_from {
        mf.push(("resume-from", r.to_string()));
    }
    // §11: until the encrypt fix lands, bundles travel
    // signed and hash-verified but unsealed — stated on
    // the manifest, never implied.
    mf.push(("sealed", "no".into()));
    let refs: Vec<(&str, &str)> = mf.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut manifest = String::from("bundle manifest\n");
    manifest.push_str(&lattice::record(&refs));
    for h in &hash_lines {
        manifest.push_str(h);
        manifest.push('\n');
    }

    // $1:: the manifest, the item record copy, and the
    // brief — the copy and the brief at their home
    // addresses (§3.4).
    let mut u1 = Phext::new();
    u1.insert(BUNDLE_ANCHOR, manifest.clone());
    u1.insert(item.addr, record_text);
    if !brief.is_empty() {
        u1.insert(dispatch::brief_addr(item.book), brief);
    }
    let mut all = vec![u1];
    all.extend(contexts);
    Ok((all, manifest))
}

/// A built bundle: its identity hash (§4.3), its
/// manifest, and its unsigned universes.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub item_addr: String,
    pub attempt: u32,
    pub sha256: String,
    pub manifest: String,
    pub universes: Vec<Phext>,
}

pub fn build_bundle(
    px: &Phext,
    item: &Item,
    attempt: u32,
    resume_from: Option<&str>,
    resume_text: Option<&str>,
    bridge: Option<(&str, &str)>,
) -> Result<Bundle, String> {
    let (universes, manifest) = build_bundle_parts(px, item, attempt, resume_from, resume_text, bridge)?;
    let sha256 = sha256_hex(unsigned_chain_bytes(&universes).as_bytes());
    Ok(Bundle {
        item_addr: phext::fmt_coord(&item.addr),
        attempt,
        sha256,
        manifest,
        universes,
    })
}

/// Sign a built bundle into its wire bytes: the
/// detached signature over the manifest's exact bytes
/// at the anchor's Scroll 2 (§3.4 — the releases
/// cluster's manifest+signature pattern).
pub fn sign_bundle(gpg: &Gpg, bundle: &Bundle) -> Result<String, String> {
    let fp = lattice::field(&bundle.manifest, "origin")
        .and_then(|o| o.split_whitespace().last().map(|s| s.to_string()))
        .unwrap_or_default();
    if fp.is_empty() {
        return Err("the manifest names no origin fingerprint — cannot sign".into());
    }
    let sig = gpg.sign_detached(&fp, bundle.manifest.as_bytes())?;
    let mut us: Vec<Phext> = bundle.universes.clone();
    if let Some(first) = us.first_mut() {
        first.insert(BUNDLE_SIG, sig);
    }
    Ok(Phext::serialize_universes(&us, true))
}

// ----------------------------------------------------------------
// The receiving side (§3.4): parse a wire bundle,
// verify it end to end, and file the attempt — or
// refuse, with a receipt. `accept_wire_bundle` mutates
// the in-memory chain and never saves; the caller
// (the serve handler, or the dispatch verb's own
// self-acceptance) owns the save, so a dispatch writes
// once (§4.2).
// ----------------------------------------------------------------

/// Every peer record in state: (cluster, address,
/// text) — the register verb's own enumeration
/// (clusters under 3.3.3 from 2 up).
fn peer_records(px: &Phext) -> Vec<(u32, Coord, String)> {
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

/// A parsed wire bundle: the manifest's claims, the
/// universes as received, and the manifest's exact
/// bytes (the signature's payload).
pub struct ParsedBundle {
    pub manifest: String,
    pub item_addr: Coord,
    pub item_book: u32,
    pub origin_node: String,
    pub origin_fp: String,
    pub attempt: u32,
    pub capabilities: Vec<String>,
    pub universes: Vec<Phext>,
    pub sha256: String,
    /// The manifest's `resume-from:` cite, when this
    /// bundle resumes a teleported attempt (§6).
    pub resume_from: Option<String>,
    /// The manifest's routed bridge (§9.2), when the
    /// item named bridges and the dispatch walk chose
    /// one: the bridge's name and the endpoint the
    /// executor routes its work over.
    pub bridge: Option<String>,
    pub bridge_endpoint: Option<String>,
}

pub fn parse_wire_bundle(bytes: &[u8]) -> Result<ParsedBundle, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("bundle is not text: {}", e))?;
    let (universes, _closed) = Phext::parse_universes(text);
    let u1 = universes.first().ok_or("bundle has no $1: universe")?;
    let manifest = u1
        .get(&BUNDLE_ANCHOR)
        .cloned()
        .ok_or("no manifest at the bundle anchor (11.11.11/11.11.11/11.11.11)")?;
    if !manifest.starts_with("bundle manifest") {
        return Err("the anchor scroll is not a bundle manifest".into());
    }
    let item_s = lattice::field(&manifest, "item").ok_or("manifest names no item")?;
    let item_addr = phext::parse_coord(&item_s).ok_or("manifest item is not a coordinate")?;
    let origin = lattice::field(&manifest, "origin").unwrap_or_default();
    let (origin_node, origin_fp) = match origin.rsplit_once(' ') {
        Some((n, f)) => (n.to_string(), f.to_string()),
        None => (origin.clone(), String::new()),
    };
    let attempt: u32 = lattice::field(&manifest, "attempt")
        .and_then(|a| a.parse().ok())
        .ok_or("manifest attempt is not a number")?;
    let sha256 = sha256_hex(unsigned_chain_bytes(&universes).as_bytes());
    let capabilities = field_all(&manifest, "capability");
    let resume_from = lattice::field(&manifest, "resume-from");
    let bridge = lattice::field(&manifest, "bridge");
    let bridge_endpoint = lattice::field(&manifest, "bridge-endpoint");
    Ok(ParsedBundle {
        manifest,
        item_addr,
        item_book: item_addr[5],
        origin_node,
        origin_fp,
        attempt,
        capabilities,
        universes,
        sha256,
        resume_from,
        bridge,
        bridge_endpoint,
    })
}

/// Verify a parsed bundle (§3.4, in the acceptance
/// rule's order): the claimed signer must be a
/// registered peer in this node's state and the
/// detached signature must verify under that identity
/// (registering a peer imported its key — verification
/// is the shipped path); every context universe's hash
/// must match the manifest's list. The bundle hash is
/// carried by the identity rule (§4.3), not verified
/// against the manifest — the manifest cannot contain
/// it; it is checked against the item's record by the
/// callers that hold one.
pub fn verify_bundle(lat: &Lattice, gpg: &Gpg, pb: &ParsedBundle) -> Result<(), String> {
    // The claimed signer is trusted when it is a
    // registered peer — or this node itself: a node
    // never registers its own key as a peer record, and
    // its own fingerprint is the root its registry
    // hangs from (self-acceptance runs this same rule).
    let mut registered = false;
    if !pb.origin_fp.is_empty() {
        let own_fp = lat
            .px
            .get(&lattice::NODE_KEY_RECORD)
            .and_then(|t| lattice::field(t, "fingerprint"))
            .unwrap_or_default();
        if own_fp == pb.origin_fp {
            registered = true;
        }
        for (_, _, t) in peer_records(&lat.px) {
            if lattice::field(&t, "status").as_deref() == Some("registered")
                && lattice::field(&t, "fingerprint").as_deref() == Some(pb.origin_fp.as_str())
            {
                registered = true;
            }
        }
    }
    if !registered {
        return Err(format!(
            "origin {} ({}) is not a registered peer on this node",
            pb.origin_node, pb.origin_fp
        ));
    }
    let u1 = pb.universes.first().ok_or("bundle has no $1: universe")?;
    let sig = u1
        .get(&BUNDLE_SIG)
        .cloned()
        .ok_or("no signature scroll at the bundle anchor's Scroll 2")?;
    let fp = gpg
        .verify_detached(&sig, pb.manifest.as_bytes())
        .map_err(|e| format!("signature does not verify: {}", e))?;
    if fp != pb.origin_fp {
        return Err(format!(
            "signature is by {}, but the manifest claims {}",
            fp, pb.origin_fp
        ));
    }
    for line in field_all(&pb.manifest, "universe") {
        let n: usize = line.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let want = line
            .split("sha256:")
            .nth(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let u = pb
            .universes
            .get(n.wrapping_sub(1))
            .ok_or_else(|| format!("manifest lists universe {} but the bundle carries {}", n, pb.universes.len()))?;
        let got = sha256_hex(u.serialize().as_bytes());
        if got != want {
            return Err(format!("universe {} hashes to {}, manifest says {}", n, got, want));
        }
    }
    Ok(())
}

/// File the bundle's paperwork (manifest +
/// provenances + context scrolls) into a stratum
/// under the assignment's Book family — shared by the
/// executor's filing and the origin's snapshot.
fn file_paperwork(stratum: &mut Phext, pb: &ParsedBundle, assignment_book: u32) {
    stratum.insert(stratum_manifest_addr(assignment_book), pb.manifest.clone());
    for (i, u) in pb.universes.iter().enumerate().skip(1) {
        let k = i + 1;
        if let Some(prov) = u.get(&BUNDLE_ANCHOR) {
            stratum.insert(stratum_provenance_addr(assignment_book, k), prov.clone());
        }
        for (c, _) in u.toc() {
            if c != BUNDLE_ANCHOR {
                if let Some(t) = u.get(&c) {
                    stratum.insert(c, t.clone());
                }
            }
        }
    }
}

/// File an accepted bundle's attempt (§3.3, §3.4): the
/// attempt universe (attempt N → universe N+1) holds
/// the executor's working copy of the item — the
/// record copy with `home:` set to the origin node,
/// `bundle-sha256:` set, state dispatched — the brief,
/// and the bundle's paperwork under the assignment's
/// Book family. Universe 1 holds a second copy at the
/// item's home address — the record the oracle and the
/// local verbs read; the attempt itself (its heartbeat
/// lines, its checkpoint scrolls) lives in the
/// stratum, never merged into universe 1 (§3.3). An
/// address already holding a DIFFERENT bundle's copy
/// is a named refusal — two bundles never share a home
/// address in a stratum.
pub fn file_attempt(lat: &mut Lattice, pb: &ParsedBundle, assignment_book: u32) -> Result<(), String> {
    let target_u = (pb.attempt + 1) as usize;
    let stratum = lat.universe(target_u).cloned().unwrap_or_default();
    if let Some(existing) = stratum.get(&pb.item_addr) {
        let same = lattice::field(existing, "bundle-sha256").as_deref() == Some(pb.sha256.as_str());
        if !same {
            return Err(format!(
                "the attempt universe {} already holds a different bundle at {}",
                target_u,
                phext::fmt_coord(&pb.item_addr)
            ));
        }
    }
    let u1 = pb.universes.first().ok_or("bundle has no $1: universe")?;
    let record_copy = u1
        .get(&pb.item_addr)
        .cloned()
        .ok_or("the bundle carries no item record copy")?;
    let mut working = record_copy.clone();
    working = lattice::set_field(&working, "bundle-sha256", &pb.sha256);
    working = lattice::set_field(&working, "home", &pb.origin_node);
    working = lattice::set_field(&working, "state", "dispatched");

    let mut stratum = stratum;
    stratum.insert(pb.item_addr, working.clone());
    if let Some(brief) = u1.get(&dispatch::brief_addr(pb.item_book)) {
        stratum.insert(dispatch::brief_addr(pb.item_book), brief.clone());
    }
    file_paperwork(&mut stratum, pb, assignment_book);
    lat.set_universe(target_u, stratum);
    // The universe-1 copy: an executor's copy carries
    // `home:` and is replaced by the new attempt's copy
    // (a re-dispatch after a failure lands here); the
    // origin's own record carries no `home:` and is
    // left standing — a self-acceptance's dispatch verb
    // drives that record through the oracle itself.
    let replace = match lat.px.get(&pb.item_addr) {
        None => true,
        Some(existing) => lattice::field(existing, "home").is_some(),
    };
    if replace {
        lat.px.insert(pb.item_addr, working);
    }
    Ok(())
}

/// File the ORIGIN's as-dispatched snapshot of an
/// attempt (§4.6 combination 7's conservative reading:
/// attempts homed here need chain universes ≥
/// attempts+1 in the origin's own state). The snapshot
/// is the bundle's contents verbatim — the pristine
/// record copy, the brief, the manifest, the context
/// provenances — never the executor's working copy;
/// the doctor's combination 6 recomputes from either
/// form (the recomputation strips exactly the fields
/// acceptance adds).
pub fn file_origin_snapshot(lat: &mut Lattice, pb: &ParsedBundle, assignment_book: u32) {
    let target_u = (pb.attempt + 1) as usize;
    let mut stratum = lat.universe(target_u).cloned().unwrap_or_default();
    let u1 = match pb.universes.first() {
        Some(u) => u,
        None => return,
    };
    if let Some(rec) = u1.get(&pb.item_addr) {
        stratum.insert(pb.item_addr, rec.clone());
    }
    if let Some(brief) = u1.get(&dispatch::brief_addr(pb.item_book)) {
        stratum.insert(dispatch::brief_addr(pb.item_book), brief.clone());
    }
    file_paperwork(&mut stratum, pb, assignment_book);
    lat.set_universe(target_u, stratum);
}

/// The outcome of an acceptance attempt — the serve
/// handler renders it, the dispatch verb cites it.
pub enum AcceptOutcome {
    /// Accepted (or a dedup replay of an acceptance):
    /// the response body — its first line cites the
    /// acceptance receipt's coordinate.
    Accepted(String),
    /// Refused, with the reason; a refusal receipt was
    /// filed on this node.
    Refused(String),
}

/// An assignment record's full text by its parsed
/// record (load_assignments reads the lenient shape;
/// writers and reconciliation need the whole scroll).
fn assignment_text(lat: &Lattice, rec: &dispatch::AssignmentRec) -> Option<String> {
    lat.px.get(&rec.addr).cloned()
}

/// A live assignment for (item, attempt) in this
/// node's Collection 3 — (its record, its text).
fn live_assignment_for(lat: &Lattice, item: &Coord, attempt: u32) -> Option<(dispatch::AssignmentRec, String)> {
    let item_s = phext::fmt_coord(item);
    for rec in dispatch::load_assignments(&lat.px) {
        if !rec.live {
            continue;
        }
        if rec.item.as_deref() == Some(item_s.as_str())
            && rec.attempt.as_deref().and_then(|a| a.parse::<u32>().ok()) == Some(attempt)
        {
            if let Some(t) = assignment_text(lat, &rec) {
                return Some((rec, t));
            }
        }
    }
    None
}

/// Retire an assignment record in place: set its
/// `status:` line — the one write that ends liveness
/// (§3.1). Callers own the save.
fn retire_assignment(lat: &mut Lattice, addr: &Coord, status: &str) {
    if let Some(t) = lat.px.get(addr).cloned() {
        let t = lattice::set_field(&t, "status", status);
        lat.px.insert(*addr, t);
    }
}

/// The next free assignment Book (Collection 3).
pub fn next_assignment_book(lat: &Lattice) -> u32 {
    lattice::next_free_book(&lat.px, [11, 11, 11, lattice::DISPATCH_ASSIGN_C, 1], 1)
}

/// Write an assignment record (C3) at a named Book —
/// returns its address. The record is LIVE by
/// construction: no `status:` line (§3.1).
fn write_assignment_at(lat: &mut Lattice, book: u32, fields: &[(&str, &str)]) -> Coord {
    let addr = lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, book, 1, 1, 1]);
    let text = format!("assignment\n{}", lattice::record(fields));
    lat.px.insert(addr, text);
    addr
}

/// Allocate and write an assignment record (C3) at
/// the next free Book — returns its address.
fn write_assignment(lat: &mut Lattice, fields: &[(&str, &str)]) -> Coord {
    let book = next_assignment_book(lat);
    write_assignment_at(lat, book, fields)
}

/// The fencing screen (§4.6 combination 2, §8): is
/// this bundle's attempt already behind the story this
/// node has filed for the item? Exactly one attempt
/// universe is authoritative — the highest. A bundle
/// is fenced out when a LIVE assignment carries a
/// higher attempt, when any CLOSED assignment carries
/// a higher attempt, or when a closed assignment
/// carries this very attempt (a closed attempt is
/// never re-run; the origin dispatches the next one).
/// An equal LIVE attempt is not fenced here — dedup
/// and the equal-attempt stale rule, below, own it.
/// Returns the refusal reason when fenced.
fn fencing_block(lat: &Lattice, item: &Coord, attempt: u32) -> Option<String> {
    let item_s = phext::fmt_coord(item);
    let mut highest_live: Option<u32> = None;
    let mut highest_closed: Option<u32> = None;
    let mut closed_same = false;
    for rec in dispatch::load_assignments(&lat.px) {
        if rec.item.as_deref() != Some(item_s.as_str()) {
            continue;
        }
        let a = rec.attempt.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
        if rec.live {
            if a > attempt {
                highest_live = Some(highest_live.map_or(a, |h: u32| h.max(a)));
            }
        } else {
            if a > attempt {
                highest_closed = Some(highest_closed.map_or(a, |h: u32| h.max(a)));
            }
            if a == attempt {
                closed_same = true;
            }
        }
    }
    if let Some(h) = highest_live {
        return Some(format!(
            "stale: attempt {} is below the live attempt {} for this item on this node (fencing, §4.6)",
            attempt, h
        ));
    }
    if let Some(h) = highest_closed {
        return Some(format!(
            "stale: attempt {} is below attempt {}, already closed on this node (fencing, §4.6)",
            attempt, h
        ));
    }
    if closed_same {
        return Some(format!(
            "stale: attempt {} already closed on this node — a closed attempt is never re-run; the origin dispatches the next attempt (fencing, §4.6)",
            attempt
        ));
    }
    None
}

/// Accept a wire bundle on this node (§3.4), in the
/// acceptance rule's order: parse → the signer is a
/// registered peer and the signature verifies → the
/// per-universe hashes match → capability fit → dedup
/// and fencing (§4.3, §4.6 combination 2). On
/// acceptance the attempt is filed and the assignment
/// record written — LIVE (no `status:` line). A
/// replay of the same (item, sha256, attempt) returns
/// the original acceptance receipt and writes nothing.
/// A lower attempt, or an equal attempt carrying a
/// different hash, is refused as stale (the fencing
/// rule); a higher attempt supersedes live older
/// assignments, which are retired in the same pass.
/// Refusals file a refusal receipt in this node's
/// ledgers (§4.6 combination 5's shape) and never file
/// an attempt. Mutates in memory; the caller saves.
pub fn accept_wire_bundle(lat: &mut Lattice, gpg: &Gpg, bytes: &[u8]) -> AcceptOutcome {
    let refuse = |lat: &mut Lattice, pb: Option<&ParsedBundle>, why: String| {
        let mut extras: Vec<(&str, &str)> = vec![("refused", "dispatch")];
        let item_s;
        let attempt_s;
        if let Some(pb) = pb {
            item_s = phext::fmt_coord(&pb.item_addr);
            attempt_s = pb.attempt.to_string();
            extras.push(("item", &item_s));
            extras.push(("attempt", &attempt_s));
            extras.push(("origin", &pb.origin_node));
        }
        extras.push(("reason", &why));
        let coord = append_receipt(&mut lat.px, "dispatch refused", &extras);
        AcceptOutcome::Refused(format!("refused: {} (receipt {})", why, phext::fmt_coord(&coord)))
    };

    let pb = match parse_wire_bundle(bytes) {
        Ok(pb) => pb,
        Err(e) => return refuse(lat, None, e),
    };
    if let Err(e) = verify_bundle(lat, gpg, &pb) {
        return refuse(lat, Some(&pb), e);
    }
    // Capability fit: the manifest's required
    // capabilities must be a subset of this node's
    // declared ones (its identity record, 1.1.1).
    let my_caps = lat
        .px
        .get(&lattice::IDENTITY)
        .map(|t| field_all(t, "capability"))
        .unwrap_or_default();
    if let Some(missing) = pb.capabilities.iter().find(|c| !my_caps.contains(c)) {
        return refuse(
            lat,
            Some(&pb),
            format!("this node does not declare capability {}", missing),
        );
    }
    // Fencing (§4.6 combination 2, completed in
    // slice 5): a bundle below the highest attempt
    // this node has filed — live or already closed —
    // is stale, and a closed attempt is never re-run.
    // The refusal receipt below is its evidence; it
    // cannot move state.
    if let Some(why) = fencing_block(lat, &pb.item_addr, pb.attempt) {
        return refuse(lat, Some(&pb), why);
    }
    // Dedup and fencing (§4.3, §4.6 combination 2).
    if let Some((rec, text)) = live_assignment_for(lat, &pb.item_addr, pb.attempt) {
        if lattice::field(&text, "bundle-sha256").as_deref() == Some(pb.sha256.as_str()) {
            // The same bundle again: return the
            // original acceptance receipt, write
            // nothing (§4.3's replay rule).
            let cite = lattice::field(&text, "receipt").unwrap_or_default();
            let body = phext::parse_coord(&cite)
                .and_then(|c| lat.px.get(&c).cloned())
                .unwrap_or_default();
            return AcceptOutcome::Accepted(format!("receipt: {}\n{}", cite, body));
        }
        return refuse(
            lat,
            Some(&pb),
            format!(
                "stale: attempt {} at {} is already live under a different bundle hash (fencing, §4.6)",
                pb.attempt,
                phext::fmt_coord(&rec.addr)
            ),
        );
    }
    // A higher attempt supersedes live older ones.
    let mut superseded: Vec<Coord> = Vec::new();
    for older in 1..pb.attempt {
        if let Some((rec, _)) = live_assignment_for(lat, &pb.item_addr, older) {
            superseded.push(rec.addr);
        }
    }
    // The assignment Book is allocated first: the
    // attempt's paperwork files under its family in
    // the stratum.
    let asg_book = next_assignment_book(lat);
    if let Err(e) = file_attempt(lat, &pb, asg_book) {
        return refuse(lat, Some(&pb), e);
    }
    for addr in &superseded {
        retire_assignment(lat, addr, &format!("superseded by attempt {}", pb.attempt));
    }
    // The acceptance receipt and the assignment
    // record — the assignment is the receipt's home
    // (§6's placement-receipt rule, executor side).
    let self_name = local_node(&lat.px);
    let receipt = append_receipt(
        &mut lat.px,
        "dispatch accepted",
        &[
            ("item", &phext::fmt_coord(&pb.item_addr)),
            ("attempt", &pb.attempt.to_string()),
            ("bundle-sha256", &pb.sha256),
            ("origin", &pb.origin_node),
            ("node", &self_name),
        ],
    );
    write_assignment_at(
        lat,
        asg_book,
        &[
            ("item", &phext::fmt_coord(&pb.item_addr)),
            ("node", &self_name),
            ("attempt", &pb.attempt.to_string()),
            ("bundle-sha256", &pb.sha256),
            ("home", &pb.origin_node),
            ("receipt", &phext::fmt_coord(&receipt)),
        ],
    );
    AcceptOutcome::Accepted(format!(
        "receipt: {}\n{}",
        phext::fmt_coord(&receipt),
        lat.px.get(&receipt).cloned().unwrap_or_default()
    ))
}

// ----------------------------------------------------------------
// Placement (§6): the eligible set and the fair draw.
// Facts are read from state the system already keeps —
// peer records, each candidate's own config scroll and
// latest telemetry observation over the scroll route —
// and the draw is pure given the facts.
// ----------------------------------------------------------------

/// One candidate's facts for one placement round.
/// `None` on a measured field means unmeasured, which
/// §6 treats as ineligible — never as zero ("load
/// measured from telemetry, never assumed").
#[derive(Debug, Clone)]
pub struct NodeFacts {
    pub name: String,
    pub endpoint: Option<String>,
    pub capabilities: Vec<String>,
    pub temp_c: Option<f64>,
    pub load1: Option<f64>,
    pub cores: Option<u32>,
    pub free_bytes: Option<u64>,
    pub resident_working: bool,
    pub reachable: bool,
}

/// The eligibility verdict (§6, in the spec's own
/// order): (a) reachability, (b) load and temperature,
/// (c) a resident agent at work, (d) data locality —
/// then capability fit. `Ok(())` is eligible; the
/// `Err` is the exclusion's reason, tagged with its
/// criterion, which the placement receipt carries
/// verbatim (§14: exclusions are recorded, never
/// silent).
pub fn eligibility(f: &NodeFacts, required_caps: &[String], bundle_bytes: u64) -> Result<(), String> {
    if !f.reachable {
        return Err("(a) unreachable".into());
    }
    match f.temp_c {
        None => return Err("(b) temperature unmeasured".into()),
        Some(t) if t >= THERMAL_LIMIT_C => {
            return Err(format!(
                "(b) temperature {:.1} °C at or above the {:.0} °C gate",
                t, THERMAL_LIMIT_C
            ))
        }
        _ => {}
    }
    match (f.load1, f.cores) {
        (None, _) | (_, None) => return Err("(b) load unmeasured".into()),
        (Some(l), Some(c)) if l >= c as f64 => {
            return Err(format!("(b) load {:.2} at or above {} cores", l, c))
        }
        _ => {}
    }
    if f.resident_working {
        return Err("(c) resident agent at work".into());
    }
    match f.free_bytes {
        None => return Err("(d) free space unverifiable — inputs not stageable".into()),
        Some(free) if free < bundle_bytes => {
            return Err(format!(
                "(d) inputs not stageable: {} free bytes under the {}-byte bundle",
                free, bundle_bytes
            ))
        }
        _ => {}
    }
    for cap in required_caps {
        if !f.capabilities.contains(cap) {
            return Err(format!("capability {} not declared", cap));
        }
    }
    Ok(())
}

/// The draw: splitmix64 over the round's seed, modulo
/// the eligible set — uniform over the set as listed,
/// and the eligible set is always listed in a stable
/// order (self first, then peers by cluster), so a
/// round replays exactly from its receipted seed (§6).
pub fn draw(eligible: &[String], seed: u64) -> Option<String> {
    if eligible.is_empty() {
        return None;
    }
    let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    let r = z ^ (z >> 31);
    Some(eligible[(r % eligible.len() as u64) as usize].clone())
}

/// A fresh round seed from the wall clock — receipted
/// on the assignment, so the round is reproducible
/// after the fact even though it is not predictable
/// before it (§6's receipt rule).
fn round_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Split an endpoint base URL (`http://host[:port]`)
/// into (host header, connect address). The port
/// defaults to qexo's serve port — the operator
/// records endpoints in the form core's sync pass
/// already dials (`{endpoint}/v1/scroll/<coord>`).
fn endpoint_parts(endpoint: &str) -> (String, String) {
    let rest = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.contains(':') {
        (authority.to_string(), authority.to_string())
    } else {
        (authority.to_string(), format!("{}:8484", authority))
    }
}

/// Minimal HTTP over TcpStream — the house client
/// pattern (register.rs's tcp_request): a short read
/// timeout, the body after the blank line.
fn http_exchange(addr: &str, request: &[u8], timeout_secs: u64) -> Result<(u32, String), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {}: {}", addr, e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)))
        .map_err(|e| e.to_string())?;
    stream.write_all(request).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u32 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((status, body.to_string()))
}

/// GET one scroll from a peer's endpoint over the
/// shipped route (§6: no new transport for reads). The
/// coordinate may carry a `$N:` universe prefix.
fn fetch_scroll(endpoint: &str, coord: &str) -> Result<String, String> {
    let (host, addr) = endpoint_parts(endpoint);
    let req = format!("GET /v1/scroll/{} HTTP/1.0\r\nHost: {}\r\n\r\n", coord, host);
    let (status, body) = http_exchange(&addr, req.as_bytes(), 5)?;
    if status == 200 {
        Ok(body)
    } else {
        Err(format!("HTTP {} — {}", status, body.trim()))
    }
}

/// POST a bundle to a peer's /v1/dispatch (§3.4).
fn post_bundle(endpoint: &str, bytes: &[u8]) -> Result<String, String> {
    let (host, addr) = endpoint_parts(endpoint);
    let head = format!(
        "POST /v1/dispatch HTTP/1.0\r\nHost: {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
        host,
        bytes.len()
    );
    let mut req = head.into_bytes();
    req.extend_from_slice(bytes);
    let (status, body) = http_exchange(&addr, &req, 10)?;
    if status == 200 {
        Ok(body)
    } else {
        Err(format!("HTTP {} — {}", status, body.trim()))
    }
}

/// Read a telemetry observation's probe fields (§6:
/// temperature joins the disk observation as a probe
/// field in slice 4, with load and cores beside it).
fn observe(f: &mut NodeFacts, obs: &str) {
    f.temp_c = lattice::field(obs, "temp").and_then(|s| s.parse().ok());
    f.load1 = lattice::field(obs, "load").and_then(|s| s.parse().ok());
    f.cores = lattice::field(obs, "cores").and_then(|s| s.parse().ok());
    f.free_bytes = lattice::field(obs, "free_bytes").and_then(|s| s.parse().ok());
    f.resident_working = lattice::field(obs, "resident").as_deref() == Some("working");
}

/// The newest observation coordinate an index names
/// (`latest: <coord> — …`, the telemetry index's own
/// line, as regen_indexes writes it).
fn latest_observation(index: &str) -> Option<String> {
    index
        .lines()
        .find_map(|l| l.trim().strip_prefix("latest:"))
        .and_then(|rest| rest.split_whitespace().next())
        .map(|s| s.to_string())
}

/// Read a candidate's facts: its identity scroll
/// (capabilities) and its latest telemetry observation
/// over the scroll route. Any failed read makes the
/// node unreachable for this round — eligibility
/// judges what it can see (§6).
fn peer_facts(name: &str, endpoint: &str) -> NodeFacts {
    let mut f = NodeFacts {
        name: name.to_string(),
        endpoint: Some(endpoint.to_string()),
        capabilities: Vec::new(),
        temp_c: None,
        load1: None,
        cores: None,
        free_bytes: None,
        resident_working: false,
        reachable: false,
    };
    let config = match fetch_scroll(endpoint, &phext::fmt_coord(&lattice::IDENTITY)) {
        Ok(t) => t,
        Err(_) => return f,
    };
    f.capabilities = field_all(&config, "capability");
    f.reachable = true;
    let index_addr = lattice::index_addr(lattice::HL_TELEMETRY);
    if let Ok(index) = fetch_scroll(endpoint, &phext::fmt_coord(&index_addr)) {
        if let Some(coord) = latest_observation(&index) {
            if let Ok(obs) = fetch_scroll(endpoint, &coord) {
                observe(&mut f, &obs);
            }
        }
    }
    f
}

/// This node's own facts, read from its own state —
/// the coordinator is a candidate like any other, and
/// is drawn only when the draw says so (§6).
fn self_facts(px: &Phext) -> NodeFacts {
    let mut f = NodeFacts {
        name: local_node(px),
        endpoint: None,
        capabilities: px
            .get(&lattice::IDENTITY)
            .map(|t| field_all(t, "capability"))
            .unwrap_or_default(),
        temp_c: None,
        load1: None,
        cores: None,
        free_bytes: None,
        resident_working: false,
        reachable: true,
    };
    let index_addr = lattice::index_addr(lattice::HL_TELEMETRY);
    if let Some(index) = px.get(&index_addr) {
        if let Some(coord) = latest_observation(index) {
            if let Some(c) = phext::parse_coord(&coord) {
                if let Some(obs) = px.get(&c) {
                    observe(&mut f, obs);
                }
            }
        }
    }
    f
}

/// Gather every candidate's facts: self, then the
/// registered peer records (3.3.3) that carry
/// endpoints, in cluster order — the stable order the
/// draw indexes.
fn gather_facts(px: &Phext) -> Vec<NodeFacts> {
    let mut out = vec![self_facts(px)];
    for (_, _, t) in peer_records(px) {
        if lattice::field(&t, "status").as_deref() != Some("registered") {
            continue;
        }
        let name = lattice::field(&t, "name").unwrap_or_default();
        if let Some(ep) = lattice::field(&t, "endpoint") {
            if !name.is_empty() {
                out.push(peer_facts(&name, &ep));
            }
        }
    }
    out
}

/// A placement round's full account: the eligible set
/// in draw order, every exclusion with its reason, the
/// draw (or the named target), and the seed.
pub struct Placement {
    pub eligible: Vec<String>,
    pub excluded: Vec<(String, String)>,
    pub chosen: Option<String>,
    pub seed: u64,
    pub named: bool,
}

/// Run one placement round over gathered facts. With
/// `--to`, the named node is the target — but its
/// eligibility is still computed and recorded, and an
/// ineligible named node places nothing (§6: naming
/// overrides the draw, never the gates).
pub fn place(facts: &[NodeFacts], required_caps: &[String], bundle_bytes: u64, to: Option<&str>) -> Placement {
    let mut eligible = Vec::new();
    let mut excluded = Vec::new();
    for f in facts {
        match eligibility(f, required_caps, bundle_bytes) {
            Ok(()) => eligible.push(f.name.clone()),
            Err(why) => excluded.push((f.name.clone(), why)),
        }
    }
    let seed = round_seed();
    let (chosen, named) = match to {
        Some(name) => (
            if eligible.iter().any(|n| n == name) { Some(name.to_string()) } else { None },
            true,
        ),
        None => (draw(&eligible, seed), false),
    };
    Placement { eligible, excluded, chosen, seed, named }
}

/// The placement receipt's lines — what the origin's
/// assignment record carries (§6: the eligible set as
/// drawn from, every exclusion with its reason, the
/// staging cost, the draw seed).
fn placement_lines(p: &Placement, bundle_bytes: u64) -> Vec<String> {
    let mut out = vec![
        format!(
            "drawn-from: {}",
            if p.eligible.is_empty() { "none".into() } else { p.eligible.join(", ") }
        ),
        format!("staging: {} bytes", bundle_bytes),
    ];
    if p.named {
        out.push("draw-seed: named (--to)".into());
    } else {
        out.push(format!("draw-seed: {}", p.seed));
    }
    for (name, why) in &p.excluded {
        out.push(format!("excluded: {} — {}", name, why));
    }
    out
}

// ----------------------------------------------------------------
// The verbs (§2.1: `qexo dispatch [<item-address>]
// [--to <node>] [--check]`; heartbeat, checkpoint, and
// fail are slice 4's working verbs — §2.1's table is
// silent on them, flagged in the build record).
// ----------------------------------------------------------------

/// §2.1's `after:` dependency: an item dispatches only
/// after the item it names is done. Returns the reason
/// when the dependency is unmet.
fn after_unmet(px: &Phext, item: &Item) -> Option<String> {
    let dep = item.after.as_ref()?;
    let dep_addr = phext::parse_coord(dep)?;
    let dep_book = dep_addr[5];
    let text = px.get(&dispatch::item_addr(dep_book))?;
    let dep_item = dispatch::parse_item(dep_book, text);
    if dep_item.state == Some(ItemState::Done) {
        None
    } else {
        Some(format!(
            "after: {} is {} — the dependency is not done",
            dep,
            dep_item.state.map(|s| s.name()).unwrap_or("in an unknown state")
        ))
    }
}

/// Count this origin's heavy items currently out: live
/// assignments whose items are homed here (the item
/// record carries no `home:`), whose briefs say
/// `heavy: yes`, and whose records stand dispatched or
/// running (§6's cap, from this node's own books —
/// the only fleet view one node can prove).
fn heavy_active(lat: &Lattice) -> usize {
    let mut n = 0;
    for rec in dispatch::load_assignments(&lat.px) {
        if !rec.live {
            continue;
        }
        if let Some(item_s) = &rec.item {
            if let Some(ic) = phext::parse_coord(item_s) {
                if let Some(text) = lat.px.get(&dispatch::item_addr(ic[5])) {
                    let item = dispatch::parse_item(ic[5], text);
                    if item.home.is_none()
                        && matches!(item.state, Some(ItemState::Dispatched) | Some(ItemState::Running))
                    {
                        let brief = lat.px.get(&dispatch::brief_addr(item.book)).cloned().unwrap_or_default();
                        if brief_facts(&brief).heavy {
                            n += 1;
                        }
                    }
                }
            }
        }
    }
    n
}

/// `qexo dispatch [<item-address>] [--to <node>]
/// [--check]` — §2.1, §5, §6. One save, at the end, on
/// success; --check writes nothing at all. The CLI driver:
/// the run below, printed live, byte for byte as always.
pub fn cmd_dispatch(item_arg: Option<&str>, to: Option<&str>, check: bool) -> i32 {
    dispatch_run(item_arg, to, check, true).0
}

/// The same dispatch as a captured run (slice 7, §2.3):
/// the `--app` driver's road — one engine, its facts
/// rendered in the protocol's shape by the caller.
pub fn dispatch_app_run(item_arg: Option<&str>, to: Option<&str>, check: bool) -> (i32, dispatch::RunLines) {
    dispatch_run(item_arg, to, check, false)
}

fn dispatch_run(item_arg: Option<&str>, to: Option<&str>, check: bool, live: bool) -> (i32, dispatch::RunLines) {
    let mut sink = if live { dispatch::RunLines::live() } else { dispatch::RunLines::new() };
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return (code, sink),
    };
    // Item selection (§5): a named item, or the head of
    // the calculated order. Non-dispatchable items are
    // skipped with their reason named, never silently
    // passed over.
    let items = dispatch::load_items(&lat.px);
    let mut chosen: Option<Item> = None;
    if let Some(arg) = item_arg {
        let addr = match phext::parse_coord(arg) {
            Some(a) => a,
            None => {
                sink.err(format!("qexo dispatch: `{}` is not an item address", arg));
                return (2, sink);
            }
        };
        match items.iter().find(|i| i.addr == addr) {
            Some(i) if i.state == Some(ItemState::Queued) => {
                if let Some(why) = after_unmet(&lat.px, i) {
                    sink.err(format!("qexo dispatch: {} is not dispatchable — {}", arg, why));
                    return (1, sink);
                }
                chosen = Some(i.clone());
            }
            Some(i) => {
                sink.err(format!(
                    "qexo dispatch: {} is {} — only a queued item dispatches",
                    arg,
                    i.state.map(|s| s.name()).unwrap_or("in an unknown state")
                ));
                return (1, sink);
            }
            None => {
                sink.err(format!("qexo dispatch: no item at {}", arg));
                return (1, sink);
            }
        }
    } else {
        let daily = dispatch::daily_goal(&lat.px);
        let order = dispatch::order_items(items, daily.as_deref());
        for cand in &order {
            if cand.state != Some(ItemState::Queued) {
                continue;
            }
            if let Some(why) = after_unmet(&lat.px, cand) {
                sink.out(format!("skip {} — {}", phext::fmt_coord(&cand.addr), why));
                sink.fact("skip", &format!("{} — {}", phext::fmt_coord(&cand.addr), why));
                continue;
            }
            let brief = lat.px.get(&dispatch::brief_addr(cand.book)).cloned().unwrap_or_default();
            if brief_facts(&brief).heavy && heavy_active(&lat) >= HEAVY_CAP {
                sink.out(format!(
                    "skip {} — heavy: the heavy cap ({}) is reached; heavy items dispatch FIFO as slots free",
                    phext::fmt_coord(&cand.addr),
                    HEAVY_CAP
                ));
                sink.fact("skip", &format!("{} — heavy cap reached", phext::fmt_coord(&cand.addr)));
                continue;
            }
            chosen = Some(cand.clone());
            break;
        }
        if chosen.is_none() {
            sink.out(format!("qexo dispatch: nothing dispatchable — the queue's head is not ready (see the skips above)"));
            sink.fact("outcome", "nothing dispatchable");
            return (0, sink);
        }
    }
    let item = chosen.unwrap();
    let attempt = item.attempts + 1;
    let brief = lat.px.get(&dispatch::brief_addr(item.book)).cloned().unwrap_or_default();
    let facts_brief = brief_facts(&brief);
    if facts_brief.heavy && heavy_active(&lat) >= HEAVY_CAP {
        sink.err(format!(
            "qexo dispatch: {} is heavy and the heavy cap ({}) is reached — heavy items dispatch FIFO as slots free (§6)",
            phext::fmt_coord(&item.addr),
            HEAVY_CAP
        ));
        return (1, sink);
    }

    // Bridge routing (§9.2): an item whose brief names
    // bridges routes only over a bridge verified live
    // for this dispatch. The walk skips the bridge the
    // last attempt's mid-execution failover receipt
    // advances past, probes stale or failed evidence on
    // the spot (a --check reads the evidence as it
    // stands and writes nothing), and files a failover
    // receipt for every candidate it walks past. No
    // live bridge: the item waits queued, the reason
    // on its record, the wait receipted — it never
    // falls back to an unlisted source.
    let item_s = phext::fmt_coord(&item.addr);
    let mut route_choice: Option<(String, String, String)> = None;
    let mut route_notes: Vec<String> = Vec::new();
    if !facts_brief.bridges.is_empty() {
        let skip = crate::bridge::mid_execution_skips(&lat.px, &item_s, item.attempts);
        if check {
            match crate::bridge::route(&mut lat.px, &item_s, attempt, &facts_brief.bridges, &skip, false) {
                crate::bridge::Route::Chosen(c) => {
                    route_notes.push(format!(
                        "bridge: {} — verified live for this dispatch (probe {})",
                        c.name,
                        phext::fmt_coord(&c.probe)
                    ));
                    route_choice = Some((c.name, c.endpoint, phext::fmt_coord(&c.probe)));
                }
                crate::bridge::Route::WouldProbe(n) => {
                    route_notes.push(format!(
                        "bridge: {}'s evidence is stale — a live dispatch probes it before routing; the bundle hash below excludes routing",
                        n
                    ));
                }
                crate::bridge::Route::NoRoute(why) => {
                    route_notes.push(format!("bridge: {} — a live dispatch waits queued", why));
                }
                crate::bridge::Route::Wait(_) => {
                    route_notes.push("bridge: no live bridge on current evidence".to_string());
                }
            }
        } else {
            match crate::bridge::route(&mut lat.px, &item_s, attempt, &facts_brief.bridges, &skip, true) {
                crate::bridge::Route::Chosen(c) => {
                    route_choice = Some((c.name, c.endpoint, phext::fmt_coord(&c.probe)));
                }
                crate::bridge::Route::Wait(reason) => {
                    append_receipt(
                        &mut lat.px,
                        "bridge-wait",
                        &[
                            ("item", &item_s),
                            ("attempt", &attempt.to_string()),
                            ("reason", &reason),
                        ],
                    );
                    let rec = lat.px.get(&item.addr).cloned().unwrap_or_default();
                    let rec = lattice::set_field(&rec, "bridge-wait", &reason);
                    lat.px.insert(item.addr, rec);
                    if let Err(e) = lat.save() {
                        sink.err(format!("qexo dispatch: cannot save state: {}", e));
                        return (1, sink);
                    }
                    sink.out(format!(
                        "{} waits queued — {} (the wait and every failover are receipted; no unlisted source is ever substituted)",
                        item_s, reason
                    ));
                    sink.fact("item", &item_s);
                    sink.fact("attempt", &attempt.to_string());
                    sink.fact("outcome", "bridge-wait");
                    sink.fact("reason", &reason);
                    return (0, sink);
                }
                _ => {
                    sink.err(format!("qexo dispatch: internal — a live routing walk returned a check-mode outcome"));
                    return (1, sink);
                }
            }
        }
    }

    // Teleport (§6, §8): a re-dispatch of an item whose
    // earlier attempt checkpointed resumes from the
    // last checkpoint — the cite the origin's
    // reconciles kept on the assignment record, and
    // the checkpoint scroll itself when the origin
    // holds it in custody (filed into the origin's
    // stratum for that attempt when the cite was
    // read). The brief is never widened: the resumed
    // bundle is built from the same record and brief
    // as every attempt (§4.3).
    let (resume_from, resume_text) = if attempt >= 2 {
        match last_checkpoint_cite(&lat, &item) {
            Some(cite) => {
                let text = phext::parse_universe_addr(&cite)
                    .and_then(|(u, c)| lat.universe(u).and_then(|s| s.get(&c)).cloned());
                (Some(cite), text)
            }
            None => (None, None),
        }
    } else {
        (None, None)
    };
    let resume_line: Option<String> = if attempt >= 2 {
        Some(match &resume_from {
            Some(cite) => cite.clone(),
            None => "none — no checkpoint held; the attempt starts from the brief".to_string(),
        })
    } else {
        None
    };

    // Build and sign the bundle (§3.4, §4.3).
    let gpg = Gpg::open_in(&lattice::state_dir());
    let bundle = match build_bundle(
        &lat.px,
        &item,
        attempt,
        resume_from.as_deref(),
        resume_text.as_deref(),
        route_choice.as_ref().map(|(n, e, _)| (n.as_str(), e.as_str())),
    ) {
        Ok(b) => b,
        Err(e) => {
            // The routing walk's evidence (probe scrolls,
            // failover receipts) is already in the state
            // in memory; it is evidence even when the
            // bundle cannot be built (§9.2).
            if !facts_brief.bridges.is_empty() && !check {
                let _ = lat.save();
            }
            sink.err(format!("qexo dispatch: cannot build the bundle: {}", e));
            return (1, sink);
        }
    };
    let wire_bytes = match sign_bundle(&gpg, &bundle) {
        Ok(w) => w.into_bytes(),
        Err(e) => {
            if !facts_brief.bridges.is_empty() && !check {
                let _ = lat.save();
            }
            sink.err(format!("qexo dispatch: cannot sign the bundle: {}", e));
            return (1, sink);
        }
    };

    // Placement (§6): facts, eligibility, the draw.
    let facts = gather_facts(&lat.px);
    let placement = place(&facts, &facts_brief.capabilities, wire_bytes.len() as u64, to);
    let self_name = local_node(&lat.px);

    if check {
        sink.out(format!("dispatch check — {} attempt {}", phext::fmt_coord(&item.addr), attempt));
        sink.out(format!("bundle-sha256: {}", bundle.sha256));
        for note in &route_notes {
            sink.out(format!("{}", note));
        }
        sink.out(format!("bundle: {} bytes over {} universe(s)", wire_bytes.len(), bundle.universes.len()));
        for line in placement_lines(&placement, wire_bytes.len() as u64) {
            sink.out(format!("{}", line));
        }
        match &placement.chosen {
            Some(n) => sink.out(format!("would place on: {}", n)),
            None => sink.out(format!("would place: nowhere — the eligible set is empty; the item stays queued")),
        }
        sink.fact("item", &phext::fmt_coord(&item.addr));
        sink.fact("attempt", &attempt.to_string());
        sink.fact("check", "yes");
        sink.fact("bundle-sha256", &bundle.sha256);
        sink.fact("bundle-bytes", &wire_bytes.len().to_string());
        for note in &route_notes {
            if let Some((k, v)) = note.split_once(": ") {
                sink.fact(k, v);
            }
        }
        for line in placement_lines(&placement, wire_bytes.len() as u64) {
            if let Some((k, v)) = line.split_once(": ") {
                sink.fact(k, v);
            }
        }
        match &placement.chosen {
            Some(n) => sink.fact("would-place", n),
            None => sink.fact("would-place", "nowhere — the eligible set is empty; the item stays queued"),
        }
        return (0, sink);
    }

    // No target: two distinct outcomes, both receipted.
    // A NAMED node that failed its gates is an error —
    // naming overrides the draw, never eligibility
    // (§6). An empty eligible set is a named, receipted
    // outcome: the item stays queued, nothing is
    // placed — least of all on the coordinator by
    // default.
    if placement.chosen.is_none() {
        let mut owned: Vec<(String, String)> = vec![
            ("item".into(), bundle.item_addr.clone()),
            ("attempt".into(), attempt.to_string()),
            ("bundle-sha256".into(), bundle.sha256.clone()),
        ];
        for l in placement_lines(&placement, wire_bytes.len() as u64) {
            if let Some((k, v)) = l.split_once(": ") {
                owned.push((k.to_string(), v.to_string()));
            }
        }
        if placement.named {
            let named = to.unwrap_or_default();
            let why = placement
                .excluded
                .iter()
                .find(|(n, _)| n == named)
                .map(|(_, w)| w.clone())
                .unwrap_or_else(|| "not a known node".into());
            owned.push(("outcome".into(), format!("named node {} is ineligible: {}", named, why)));
            let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let coord = append_receipt(&mut lat.px, "placement", &refs);
            if let Err(e) = lat.save() {
                sink.err(format!("qexo dispatch: cannot save state: {}", e));
                return (1, sink);
            }
            sink.err(format!(
                "qexo dispatch: named node {} is ineligible for {}: {} (placement receipt {})",
                named,
                bundle.item_addr,
                why,
                phext::fmt_coord(&coord)
            ));
            sink.fact("item", &bundle.item_addr);
            sink.fact("attempt", &attempt.to_string());
            sink.fact("bundle-sha256", &bundle.sha256);
            sink.fact("outcome", &format!("named node {} is ineligible: {}", named, why));
            sink.fact("receipt", &phext::fmt_coord(&coord));
            return (1, sink);
        }
        owned.push(("outcome".into(), "no eligible node — item stays queued".into()));
        let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let coord = append_receipt(&mut lat.px, "placement", &refs);
        if let Err(e) = lat.save() {
            sink.err(format!("qexo dispatch: cannot save state: {}", e));
            return (1, sink);
        }
        sink.out(format!(
            "no eligible node for {} — the item stays queued (placement receipt {})",
            bundle.item_addr,
            phext::fmt_coord(&coord)
        ));
        for (name, why) in &placement.excluded {
            sink.out(format!("excluded: {} — {}", name, why));
        }
        sink.fact("item", &bundle.item_addr);
        sink.fact("attempt", &attempt.to_string());
        sink.fact("bundle-sha256", &bundle.sha256);
        sink.fact("outcome", "no eligible node — item stays queued");
        sink.fact("receipt", &phext::fmt_coord(&coord));
        for (name, why) in &placement.excluded {
            sink.fact("excluded", &format!("{} — {}", name, why));
        }
        return (0, sink);
    }
    let target_name = placement.chosen.clone().unwrap();

    // Acceptance: self-acceptance runs the identical
    // acceptance function in-process; a peer is POSTed
    // the wire bytes. Either way the outcome is the
    // target's own acceptance (or refusal) receipt.
    let outcome = if target_name == self_name {
        accept_wire_bundle(&mut lat, &gpg, &wire_bytes)
    } else {
        let endpoint = facts
            .iter()
            .find(|f| f.name == target_name)
            .and_then(|f| f.endpoint.clone());
        match endpoint {
            Some(ep) => match post_bundle(&ep, &wire_bytes) {
                Ok(body) => AcceptOutcome::Accepted(body),
                Err(e) => AcceptOutcome::Refused(format!("the target refused or could not be reached: {}", e)),
            },
            None => AcceptOutcome::Refused("the drawn node has no endpoint on record".into()),
        }
    };
    let accept_body = match outcome {
        AcceptOutcome::Accepted(body) => body,
        AcceptOutcome::Refused(why) => {
            // The item stays queued; the refusal is
            // receipted here as well as on the target.
            let coord = append_receipt(
                &mut lat.px,
                "dispatch refused",
                &[
                    ("item", &bundle.item_addr),
                    ("attempt", &attempt.to_string()),
                    ("node", &target_name),
                    ("reason", &why),
                ],
            );
            if let Err(e) = lat.save() {
                sink.err(format!("qexo dispatch: cannot save state: {}", e));
                return (1, sink);
            }
            sink.err(format!(
                "qexo dispatch: {} refused {} attempt {}: {} (receipt {})",
                target_name,
                bundle.item_addr,
                attempt,
                why,
                phext::fmt_coord(&coord)
            ));
            sink.fact("item", &bundle.item_addr);
            sink.fact("attempt", &attempt.to_string());
            sink.fact("node", &target_name);
            sink.fact("outcome", &format!("refused: {}", why));
            sink.fact("receipt", &phext::fmt_coord(&coord));
            return (1, sink);
        }
    };
    // The origin verifies the acceptance it was handed:
    // the cited bundle hash must be the hash it built
    // (§3.4 — acceptance is of THIS bundle).
    let cited_sha = accept_body
        .lines()
        .find_map(|l| l.trim().strip_prefix("bundle-sha256:"))
        .map(|s| s.trim().to_string());
    if cited_sha.as_deref() != Some(bundle.sha256.as_str()) {
        sink.err(format!(
            "qexo dispatch: {} accepted a different bundle (cited {:?}, built {}) — not recording the dispatch",
            target_name, cited_sha, bundle.sha256
        ));
        return (1, sink);
    }
    let accept_receipt = accept_body
        .lines()
        .next()
        .and_then(|l| l.trim().strip_prefix("receipt:"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // One save (§4.2): the item record gains its bundle
    // hash, the §4.6 transition runs through the
    // oracle, the origin's assignment (the placement
    // receipt, §6) is written, and the as-dispatched
    // snapshot is filed (remote targets only — a
    // self-acceptance already filed the attempt).
    let rec = lat.px.get(&item.addr).cloned().unwrap_or_default();
    let rec = lattice::set_field(&rec, "bundle-sha256", &bundle.sha256);
    // A routed dispatch clears any bridge wait the
    // record was carrying (§9.2).
    let rec = remove_field(&rec, "bridge-wait");
    lat.px.insert(item.addr, rec);
    let actor_name = dispatch::local_actor(&lat.px);
    let mut t = Transition::new(ItemState::Dispatched, Actor::Dispatch, &actor_name);
    t.evidence = format!(
        "bundle {} accepted by {} — acceptance receipt {}",
        bundle.sha256, target_name, accept_receipt
    );
    if let Some((bname, _, bprobe)) = &route_choice {
        // The probe and the choice are cited in the
        // dispatch receipt (§9.2).
        t.evidence = format!("{} — bridge {} (probe {})", t.evidence, bname, bprobe);
    }
    if let Err(e) = dispatch::apply_transition(&mut lat, item.book, &t) {
        sink.err(format!("qexo dispatch: the transition was refused by the oracle: {}", e));
        return (1, sink);
    }
    if target_name == self_name {
        // Self-acceptance: the executor assignment the
        // acceptance wrote doubles as the placement
        // receipt — the placement lines join it, so the
        // item stands under exactly one live assignment
        // (§4.6 combination 2).
        if let Some((rec2, text)) = live_assignment_for(&lat, &item.addr, attempt) {
            let mut text = text;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            for l in placement_lines(&placement, wire_bytes.len() as u64) {
                text.push_str(&l);
                text.push('\n');
            }
            if let Some(r) = &resume_line {
                text.push_str(&format!("resume-from: {}\n", r));
            }
            if let Some((bname, _, bprobe)) = &route_choice {
                text.push_str(&format!("bridge: {}\nbridge-probe: {}\n", bname, bprobe));
            }
            lat.px.insert(rec2.addr, text);
        }
    } else {
        let mut owned: Vec<(String, String)> = vec![
            ("item".into(), bundle.item_addr.clone()),
            ("node".into(), target_name.clone()),
            ("attempt".into(), attempt.to_string()),
            ("bundle-sha256".into(), bundle.sha256.clone()),
            ("receipt".into(), accept_receipt.clone()),
        ];
        for l in placement_lines(&placement, wire_bytes.len() as u64) {
            if let Some((k, v)) = l.split_once(": ") {
                owned.push((k.to_string(), v.to_string()));
            }
        }
        if let Some(r) = &resume_line {
            owned.push(("resume-from".into(), r.clone()));
        }
        if let Some((bname, _, bprobe)) = &route_choice {
            owned.push(("bridge".into(), bname.clone()));
            owned.push(("bridge-probe".into(), bprobe.clone()));
        }
        let refs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let origin_book = next_assignment_book(&lat);
        write_assignment_at(&mut lat, origin_book, &refs);
        let pb = match parse_wire_bundle(&wire_bytes) {
            Ok(pb) => pb,
            Err(e) => {
                sink.err(format!("qexo dispatch: internal — the built bundle does not re-parse: {}", e));
                return (1, sink);
            }
        };
        file_origin_snapshot(&mut lat, &pb, origin_book);
    }
    if let Err(e) = lat.save() {
        sink.err(format!("qexo dispatch: cannot save state: {}", e));
        return (1, sink);
    }
    sink.out(format!(
        "dispatched {} attempt {} to {} — bundle {} (acceptance receipt {}){}{}",
        bundle.item_addr,
        attempt,
        target_name,
        bundle.sha256,
        accept_receipt,
        match &resume_from {
            Some(cite) => format!(" — resuming from {}", cite),
            None => String::new(),
        },
        match &route_choice {
            Some((bname, _, _)) => format!(" — bridge {}", bname),
            None => String::new(),
        }
    ));
    sink.fact("item", &bundle.item_addr);
    sink.fact("attempt", &attempt.to_string());
    sink.fact("node", &target_name);
    sink.fact("bundle-sha256", &bundle.sha256);
    sink.fact("receipt", &accept_receipt);
    sink.fact("outcome", "dispatched");
    if let Some(cite) = &resume_from {
        sink.fact("resume-from", cite);
    }
    if let Some((bname, _, bprobe)) = &route_choice {
        sink.fact("bridge", bname);
        sink.fact("bridge-probe", bprobe);
    }
    (0, sink)
}

// ----------------------------------------------------------------
// Heartbeat, checkpoint, fail (§6, §4.6). The executor
// side works on the attempt it holds; the origin side
// reconciles from the executor's assignment record,
// read over the scroll route.
// ----------------------------------------------------------------

/// This node's live assignment for an item where THIS
/// node is the executor (the record carries `home:` —
/// origin-side records name the target under `node:`
/// instead). Returns (record, text, attempt).
fn my_execution_assignment(lat: &Lattice, item: &Coord) -> Option<(dispatch::AssignmentRec, String, u32)> {
    let item_s = phext::fmt_coord(item);
    for rec in dispatch::load_assignments(&lat.px) {
        if !rec.live || rec.item.as_deref() != Some(item_s.as_str()) {
            continue;
        }
        if let Some(t) = assignment_text(lat, &rec) {
            if lattice::field(&t, "home").is_some() {
                let attempt = rec.attempt.as_deref().and_then(|a| a.parse().ok()).unwrap_or(0);
                return Some((rec, t, attempt));
            }
        }
    }
    None
}

/// This node's origin-side live assignment for an item
/// (names the executor under `node:`, carries no
/// `home:`). Returns (record, text, attempt, node).
fn my_origin_assignment(lat: &Lattice, item: &Coord) -> Option<(dispatch::AssignmentRec, String, u32, String)> {
    let item_s = phext::fmt_coord(item);
    for rec in dispatch::load_assignments(&lat.px) {
        if !rec.live || rec.item.as_deref() != Some(item_s.as_str()) {
            continue;
        }
        if let Some(t) = assignment_text(lat, &rec) {
            if lattice::field(&t, "home").is_none() {
                let attempt = rec.attempt.as_deref().and_then(|a| a.parse().ok()).unwrap_or(0);
                let node = lattice::field(&t, "node").unwrap_or_default();
                return Some((rec, t, attempt, node));
            }
        }
    }
    None
}

/// The endpoint of a node by name, from the peer
/// records (3.3.3) — the origin reads the executor's
/// assignment through it.
fn endpoint_of(lat: &Lattice, name: &str) -> Option<String> {
    for (_, _, t) in peer_records(&lat.px) {
        if lattice::field(&t, "name").as_deref() == Some(name) {
            return lattice::field(&t, "endpoint");
        }
    }
    None
}

/// The item record this node holds for an assignment
/// (its universe-1 copy), parsed.
fn held_item(lat: &Lattice, addr: &Coord) -> Option<Item> {
    let text = lat.px.get(addr)?;
    Some(dispatch::parse_item(addr[5], text))
}

/// The last checkpoint cite the origin holds for an
/// item (§3.1: the assignment record carries the last
/// checkpoint cite): the `checkpoint:` field of the
/// item's highest-attempt assignment, live or retired
/// — retirement ends an assignment's liveness, never
/// its record. This is the cite a teleport's manifest
/// names in `resume-from:` (§6).
fn last_checkpoint_cite(lat: &Lattice, item: &Item) -> Option<String> {
    let item_s = phext::fmt_coord(&item.addr);
    let mut best: Option<(u32, String)> = None;
    for rec in dispatch::load_assignments(&lat.px) {
        if rec.item.as_deref() != Some(item_s.as_str()) {
            continue;
        }
        if let Some(t) = assignment_text(lat, &rec) {
            if let Some(cp) = lattice::field(&t, "checkpoint") {
                let a = rec.attempt.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                if best.as_ref().map(|(ba, _)| a >= *ba).unwrap_or(true) {
                    best = Some((a, cp));
                }
            }
        }
    }
    best.map(|(_, cp)| cp)
}

/// Verify a checkpoint's cited outputs (§8): every
/// `output:` line names a scroll, every
/// `output-sha256:` line the hash the executor took
/// of it at checkpoint time; `read` fetches a cited
/// scroll's current text (the origin reads over the
/// scroll route). Ok(n) = n outputs read back and
/// hash-matched. A checkpoint citing no outputs
/// returns Ok(0) — it verifies nothing and proves no
/// progress; the caller treats only n ≥ 1 as a
/// verified progress event. Err names the first
/// failure: an unreadable output, a hash that does
/// not match, or a cite/hash count mismatch.
pub fn check_checkpoint_outputs(text: &str, read: &dyn Fn(&str) -> Option<String>) -> Result<usize, String> {
    let outs = field_all(text, "output");
    let hashes = field_all(text, "output-sha256");
    if outs.len() != hashes.len() {
        return Err(format!(
            "the checkpoint cites {} output(s) but {} output hash(es)",
            outs.len(),
            hashes.len()
        ));
    }
    for (o, h) in outs.iter().zip(hashes.iter()) {
        match read(o) {
            None => return Err(format!("output {} cannot be read back", o)),
            Some(body) => {
                let got = sha256_hex(body.as_bytes());
                if got != *h {
                    return Err(format!(
                        "output {} hashes to {}, the checkpoint cites {}",
                        o, got, h
                    ));
                }
            }
        }
    }
    Ok(outs.len())
}

/// The stamp of an item's latest dispatched
/// transition on this node's ledger — when no beat
/// was ever reconciled, the executor's beats were due
/// from here (§8: the 3-missed-beats reckoning needs
/// a start line, and the dispatch is it).
fn dispatch_stamp(lat: &Lattice, item_s: &str) -> Option<String> {
    let mut stamp = None;
    for r in dispatch::load_transition_receipts(&lat.px) {
        if r.item.as_deref() == Some(item_s) && r.to.as_deref() == Some("dispatched") {
            stamp = lat.px.get(&r.addr).and_then(|t| lattice::field(t, "at"));
        }
    }
    stamp
}

/// §8's coordinator act, shared by both detection
/// evidences (serve unreachable, executor silent):
/// file the `failure detected` receipt, close the
/// attempt through the oracle — the third consecutive
/// failure lands in paused-auto, with its audit —
/// retire the origin's assignment, and save. When the
/// item lands queued, teleport it: the caller
/// re-enters through the dispatch verb, so a teleport
/// is an ordinary dispatch of a re-queued item in a
/// fresh fair-draw round, its bundle carrying
/// `resume-from:` from the held checkpoint cite (§6).
/// When it lands paused-auto, the surfacing is the
/// queue pin and the audit view (§8) — no teleport.
fn finish_detection(
    lat: &mut Lattice,
    addr: &Coord,
    asg: &dispatch::AssignmentRec,
    attempt: u32,
    node: &str,
    evidence: &str,
    actor_name: &str,
) -> i32 {
    let item_s = phext::fmt_coord(addr);
    let det = append_receipt(
        &mut lat.px,
        "failure detected",
        &[
            ("item", &item_s),
            ("attempt", &attempt.to_string()),
            ("node", node),
            ("evidence", evidence),
        ],
    );
    let det_s = phext::fmt_coord(&det);
    let mut landed = String::new();
    if let Some(it) = held_item(lat, addr) {
        if matches!(it.state, Some(ItemState::Dispatched) | Some(ItemState::Running)) {
            let mut t = Transition::new(ItemState::Queued, Actor::Coordinator, actor_name);
            t.evidence = format!("failure detected — {} (detection receipt {})", evidence, det_s);
            if let Err(e) = dispatch::apply_transition(lat, it.book, &t) {
                eprintln!("qexo heartbeat: the detected failure transition was refused by the oracle: {}", e);
                return 1;
            }
        }
        landed = held_item(lat, addr)
            .and_then(|i| i.state)
            .map(|s| s.name().to_string())
            .unwrap_or_default();
    }
    retire_assignment(lat, &asg.addr, &format!("failed — executor lost (detection receipt {})", det_s));
    if let Err(e) = lat.save() {
        eprintln!("qexo heartbeat: cannot save state: {}", e);
        return 1;
    }
    println!(
        "failure detected for {} attempt {} — {} (detection receipt {})",
        item_s, attempt, evidence, det_s
    );
    if landed == "paused-auto" {
        println!(
            "{} auto-paused at 3 consecutive failures — pinned at the head of every queue rendering; the auto-pause audit stands in the audit view (§8)",
            item_s
        );
        return 0;
    }
    if landed == "queued" {
        println!(
            "teleporting {} — a fresh fair-draw round places the next attempt (§6, §8)",
            item_s
        );
        return cmd_dispatch(Some(&item_s), None, false);
    }
    0
}

/// `qexo heartbeat <item-address>` — one verb, two
/// sides (§6). Executor side: beat — the `heartbeat:`
/// line lands on the assignment record and the attempt
/// universe's record scroll; the first beat moves the
/// item dispatched → running through the oracle.
/// Origin side: read the executor's assignment record
/// over the scroll route and reconcile the home record
/// with what it says — or, on §8's evidences, detect
/// the executor's loss and teleport the item.
pub fn cmd_heartbeat(item_arg: &str) -> i32 {
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let addr = match phext::parse_coord(item_arg) {
        Some(a) => a,
        None => {
            eprintln!("qexo heartbeat: `{}` is not an item address", item_arg);
            return 2;
        }
    };
    let (_, stamp) = now_stamp();
    let actor_name = dispatch::local_actor(&lat.px);

    // Executor side.
    if let Some((asg, _t, attempt)) = my_execution_assignment(&lat, &addr) {
        let item = match held_item(&lat, &addr) {
            Some(i) => i,
            None => {
                eprintln!("qexo heartbeat: no attempt copy held for {}", item_arg);
                return 1;
            }
        };
        let stratum_u = (attempt + 1) as usize;
        match item.state {
            Some(ItemState::Dispatched) => {
                // The first heartbeat: dispatched →
                // running, through the oracle.
                let mut t = Transition::new(ItemState::Running, Actor::Executor, &actor_name);
                t.evidence = format!("first heartbeat {}", stamp);
                if let Err(e) = dispatch::apply_transition(&mut lat, item.book, &t) {
                    eprintln!("qexo heartbeat: the transition was refused by the oracle: {}", e);
                    return 1;
                }
                if let Some(mut stratum) = lat.universe(stratum_u).cloned() {
                    if let Some(copy) = stratum.get(&addr).cloned() {
                        let copy = lattice::set_field(&copy, "state", "running");
                        let copy = lattice::set_field(&copy, "heartbeat", &stamp);
                        stratum.insert(addr, copy);
                        lat.set_universe(stratum_u, stratum);
                    }
                }
            }
            Some(ItemState::Running) => {
                if let Some(mut stratum) = lat.universe(stratum_u).cloned() {
                    if let Some(copy) = stratum.get(&addr).cloned() {
                        let copy = lattice::set_field(&copy, "heartbeat", &stamp);
                        stratum.insert(addr, copy);
                        lat.set_universe(stratum_u, stratum);
                    }
                }
            }
            other => {
                eprintln!(
                    "qexo heartbeat: the attempt copy for {} is {} — only dispatched or running beats",
                    item_arg,
                    other.map(|s| s.name()).unwrap_or("in an unknown state")
                );
                return 1;
            }
        }
        let asg_text = lat.px.get(&asg.addr).cloned().unwrap_or_default();
        let asg_text = lattice::set_field(&asg_text, "heartbeat", &stamp);
        lat.px.insert(asg.addr, asg_text);
        if let Err(e) = lat.save() {
            eprintln!("qexo heartbeat: cannot save state: {}", e);
            return 1;
        }
        println!("heartbeat {} — attempt {} ({})", item_arg, attempt, stamp);
        return 0;
    }

    // Origin side: reconcile from the executor's
    // assignment record.
    let (asg, _t, attempt, node) = match my_origin_assignment(&lat, &addr) {
        Some(x) => x,
        None => {
            eprintln!(
                "qexo heartbeat: no live assignment for {} on this node — nothing to beat, nothing to reconcile",
                item_arg
            );
            return 1;
        }
    };
    let self_name = local_node(&lat.px);
    if node == self_name {
        eprintln!(
            "qexo heartbeat: {} executes here, but no executor assignment is live — state is inconsistent; run qexo doctor",
            item_arg
        );
        return 1;
    }
    let endpoint = match endpoint_of(&lat, &node) {
        Some(e) => e,
        None => {
            eprintln!("qexo heartbeat: no endpoint on record for executor {}", node);
            return 1;
        }
    };
    // Find the executor's assignment scroll FOR THIS
    // ATTEMPT: its Collection 3 Books over the scroll
    // route (dense from 1 — next_free_book's
    // allocation) until a record names this item, this
    // attempt, with `home:` = us. The attempt match is
    // load-bearing (slice 5): a retired record from an
    // earlier attempt names the same item and home —
    // reconciling against it would re-apply a closed
    // attempt's outcome to the live one.
    let attempt_s = attempt.to_string();
    let mut remote: Option<String> = None;
    for b in 1..=64u32 {
        let c = lattice::at(lattice::HL_DISPATCH, [lattice::DISPATCH_ASSIGN_C, 1, b, 1, 1, 1]);
        match fetch_scroll(&endpoint, &phext::fmt_coord(&c)) {
            Ok(t) => {
                if lattice::field(&t, "item").as_deref() == Some(item_arg)
                    && lattice::field(&t, "home").as_deref() == Some(self_name.as_str())
                    && lattice::field(&t, "attempt").as_deref() == Some(attempt_s.as_str())
                {
                    remote = Some(t);
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let remote = match remote {
        Some(r) => r,
        None => {
            // The executor's record cannot be read.
            // §8's detection (a) needs both halves: the
            // node's serve unreachable or silent AND 3
            // consecutive beats missed. A serve that
            // answers but holds no record for the item
            // is neither — the home record is untouched,
            // as it is when fewer than 3 beats are due.
            let serve_answers = fetch_scroll(&endpoint, &phext::fmt_coord(&lattice::IDENTITY)).is_ok();
            if serve_answers {
                eprintln!(
                    "qexo heartbeat: {} holds no assignment record for {} — the serve answers but the record is absent; the home record is untouched",
                    node, item_arg
                );
                return 1;
            }
            let asg_text = lat.px.get(&asg.addr).cloned().unwrap_or_default();
            let last_beat = lattice::field(&asg_text, "heartbeat").or_else(|| dispatch_stamp(&lat, item_arg));
            let missed = last_beat
                .as_deref()
                .and_then(crate::core::stamp_minutes)
                .map(|m| crate::core::now_minutes() - m)
                .unwrap_or(0);
            if missed <= 6 {
                eprintln!(
                    "qexo heartbeat: {} is unreachable, but the last beat ({}) is only {} minute(s) old — 3 consecutive missed 2-minute beats (6 minutes) make a detection (§8); the home record is untouched",
                    node,
                    last_beat.as_deref().unwrap_or("none"),
                    missed
                );
                return 1;
            }
            let evidence = format!(
                "heartbeat miss — the serve at {} is unreachable and the last beat ({}) is {} minutes old: 3 consecutive 2-minute beats missed",
                endpoint,
                last_beat.as_deref().unwrap_or("none"),
                missed
            );
            return finish_detection(&mut lat, &addr, &asg, attempt, &node, &evidence, &actor_name);
        }
    };
    // The attempt scroll itself, for its checkpoint
    // cite (§6: the origin reads them over the scroll
    // route).
    let attempt_scroll = fetch_scroll(&endpoint, &format!("${}:{}", attempt + 1, item_arg)).ok();
    let item = held_item(&lat, &addr);
    let mut changed = false;
    if let Some(status) = lattice::field(&remote, "status") {
        // The executor retired its assignment — a
        // failure (or a supersede). A failure moves the
        // home record through the oracle's failure
        // transition with the executor's evidence.
        if status.starts_with("failed") {
            if let Some(it) = &item {
                if matches!(it.state, Some(ItemState::Dispatched) | Some(ItemState::Running)) {
                    let mut t = Transition::new(ItemState::Queued, Actor::Executor, &actor_name);
                    t.evidence = format!("executor {} reports: {}", node, status);
                    if let Err(e) = dispatch::apply_transition(&mut lat, it.book, &t) {
                        eprintln!("qexo heartbeat: the failure transition was refused by the oracle: {}", e);
                        return 1;
                    }
                }
            }
            // Bridge failover (§9.2): the attempt routed
            // over a bridge, and the executor's evidence
            // names that bridge as what failed — the
            // failover is receipted in this same save,
            // and the next dispatch's walk advances past
            // the bridge (bridge::mid_execution_skips).
            if let Some(bname) = &asg.bridge {
                if status.contains(bname.as_str()) {
                    let brief = lat.px.get(&dispatch::brief_addr(addr[5])).cloned().unwrap_or_default();
                    let prefs = crate::bridge::brief_prefs(&brief);
                    let to = prefs
                        .iter()
                        .skip_while(|n| *n != bname)
                        .nth(1)
                        .cloned()
                        .unwrap_or_else(|| "none".to_string());
                    let coord = append_receipt(
                        &mut lat.px,
                        "bridge-failover",
                        &[
                            ("item", item_arg),
                            ("attempt", &attempt.to_string()),
                            ("from", bname.as_str()),
                            ("to", to.as_str()),
                            ("evidence", &format!("executor {} reports: {}", node, status)),
                        ],
                    );
                    println!(
                        "bridge failover for {} — {} failed mid-execution; the next dispatch advances to {} (receipt {})",
                        item_arg,
                        bname,
                        to,
                        phext::fmt_coord(&coord)
                    );
                }
            }
        }
        retire_assignment(&mut lat, &asg.addr, &format!("executor reports: {}", status));
        changed = true;
    } else {
        // No retirement on the executor: its beats are
        // the evidence. Stopped beats with an answering
        // serve are §8's silent executor — the same
        // 3-missed-beats reckoning as the unreachable
        // case, the start line being the last beat (or
        // the dispatch itself, when none ever landed).
        let beat = lattice::field(&remote, "heartbeat");
        let beat_stamp = beat.clone().or_else(|| dispatch_stamp(&lat, item_arg));
        let missed = beat_stamp
            .as_deref()
            .and_then(crate::core::stamp_minutes)
            .map(|m| crate::core::now_minutes() - m)
            .unwrap_or(0);
        if missed > 6 {
            let evidence = format!(
                "executor silent — {}'s serve answers, but its last beat ({}) is {} minutes old: 3 consecutive 2-minute beats missed",
                node,
                beat_stamp.as_deref().unwrap_or("none"),
                missed
            );
            return finish_detection(&mut lat, &addr, &asg, attempt, &node, &evidence, &actor_name);
        }
        if let Some(beat) = beat {
        // Refresh the origin's cites; the first
        // executor heartbeat moves the home record
        // dispatched → running through the oracle.
        let asg_text = lat.px.get(&asg.addr).cloned().unwrap_or_default();
        let mut asg_text = lattice::set_field(&asg_text, "heartbeat", &beat);
        let cp = lattice::field(&remote, "checkpoint").or_else(|| {
            attempt_scroll
                .as_ref()
                .and_then(|s| lattice::field(s, "checkpoint"))
        });
        if let Some(cp) = cp {
            let prev_cp = lattice::field(&asg_text, "checkpoint");
            asg_text = lattice::set_field(&asg_text, "checkpoint", &cp);
            if prev_cp.as_deref() != Some(cp.as_str()) {
                // A checkpoint cite this origin has not
                // seen: take custody of the checkpoint
                // scroll — the resume currency must
                // survive the executor that wrote it
                // (§6) — and verify its cited outputs
                // against the executor's own scrolls.
                // Only a checkpoint whose outputs all
                // read back and hash-match is verified
                // progress (§8): the counter resets on
                // the coordinator's READ, never on the
                // executor's say-so.
                match fetch_scroll(&endpoint, &cp) {
                    Ok(cp_text) => {
                        if let Some((u, coord)) = phext::parse_universe_addr(&cp) {
                            let mut stratum = lat.universe(u).cloned().unwrap_or_default();
                            stratum.insert(coord, cp_text.clone());
                            lat.set_universe(u, stratum);
                        }
                        let read = |o: &str| fetch_scroll(&endpoint, o).ok();
                        match check_checkpoint_outputs(&cp_text, &read) {
                            Ok(0) => {}
                            Ok(n) => {
                                asg_text = lattice::set_field(&asg_text, "verified-checkpoint", &cp);
                                if let Some(it) = &item {
                                    if it.state == Some(ItemState::Running) && it.failures > 0 {
                                        let old = it.failures;
                                        append_receipt(
                                            &mut lat.px,
                                            "progress verified",
                                            &[
                                                ("item", item_arg),
                                                ("attempt", &attempt.to_string()),
                                                ("checkpoint", &cp),
                                                ("outputs", &format!("{} cited output(s) read back from {} and hash-matched", n, node)),
                                                ("failures", &format!("{} -> 0", old)),
                                            ],
                                        );
                                        let rec = lat.px.get(&it.addr).cloned().unwrap_or_default();
                                        let rec = lattice::set_field(&rec, "failures", "0");
                                        lat.px.insert(it.addr, rec);
                                        println!(
                                            "verified progress for {} — {} output(s) read back from {}; failures reset {} -> 0 (§8)",
                                            item_arg, n, node, old
                                        );
                                    }
                                }
                            }
                            Err(why) => {
                                append_receipt(
                                    &mut lat.px,
                                    "progress unverified",
                                    &[
                                        ("item", item_arg),
                                        ("attempt", &attempt.to_string()),
                                        ("checkpoint", &cp),
                                        ("reason", &why),
                                    ],
                                );
                                println!(
                                    "checkpoint {} for {} does not verify: {} — the failure counter stands (§8)",
                                    cp, item_arg, why
                                );
                            }
                        }
                    }
                    Err(e) => {
                        println!(
                            "checkpoint {} for {} could not be read back for custody ({}): the cite stands, its content is not held here",
                            cp, item_arg, e
                        );
                    }
                }
            }
        }
        lat.px.insert(asg.addr, asg_text);
        changed = true;
        if let Some(it) = &item {
            if it.state == Some(ItemState::Dispatched) {
                let mut t = Transition::new(ItemState::Running, Actor::Executor, &actor_name);
                t.evidence = format!("executor {} heartbeat {}", node, beat);
                if let Err(e) = dispatch::apply_transition(&mut lat, it.book, &t) {
                    eprintln!("qexo heartbeat: the transition was refused by the oracle: {}", e);
                    return 1;
                }
            }
        }
        }
    }
    if changed {
        if let Err(e) = lat.save() {
            eprintln!("qexo heartbeat: cannot save state: {}", e);
            return 1;
        }
    }
    println!(
        "reconciled {} from {} — heartbeat {} checkpoint {}",
        item_arg,
        node,
        lattice::field(&remote, "heartbeat").unwrap_or_else(|| "none".into()),
        lattice::field(&remote, "checkpoint").unwrap_or_else(|| "none".into())
    );
    0
}

/// `qexo checkpoint <item-address> --note <text>
/// [--output <coordinate>]…` — executor-local (§6):
/// a checkpoint scroll in the attempt universe's item
/// Book (the next free scroll at Scroll 4+), a
/// running → running oracle transition, and the cite
/// recorded on the attempt copy and the assignment.
/// Each `--output` names a scroll this attempt
/// produced (a bare coordinate, or `$<n>:<coordinate>`
/// to name its universe); the executor resolves it
/// from its own chain and records the scroll's
/// SHA-256 beside the cite — the pair the origin later
/// reads back to verify progress (§8). An output that
/// cannot be read here is refused before anything is
/// written: a checkpoint never cites what its own
/// node cannot produce.
pub fn cmd_checkpoint(args: &[String]) -> i32 {
    let mut item_arg: Option<&str> = None;
    let mut note: Option<String> = None;
    let mut outputs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--note" => {
                i += 1;
                note = args.get(i).cloned();
            }
            "--output" => {
                i += 1;
                match args.get(i) {
                    Some(o) => outputs.push(o.clone()),
                    None => {
                        eprintln!("qexo checkpoint: --output needs a coordinate");
                        return 2;
                    }
                }
            }
            other if !other.starts_with("--") && item_arg.is_none() => item_arg = Some(other),
            other => {
                eprintln!("qexo checkpoint: unexpected argument `{}`", other);
                return 2;
            }
        }
        i += 1;
    }
    let item_arg = match item_arg {
        Some(a) => a,
        None => {
            eprintln!("usage: qexo checkpoint <item-address> --note <text> [--output <coordinate>]…");
            return 2;
        }
    };
    let note = note.unwrap_or_else(|| "checkpoint".to_string());
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let addr = match phext::parse_coord(item_arg) {
        Some(a) => a,
        None => {
            eprintln!("qexo checkpoint: `{}` is not an item address", item_arg);
            return 2;
        }
    };
    let (asg, _t, attempt) = match my_execution_assignment(&lat, &addr) {
        Some(x) => x,
        None => {
            eprintln!("qexo checkpoint: this node holds no live execution assignment for {}", item_arg);
            return 1;
        }
    };
    let item = match held_item(&lat, &addr) {
        Some(i) => i,
        None => {
            eprintln!("qexo checkpoint: no attempt copy held for {}", item_arg);
            return 1;
        }
    };
    if item.state != Some(ItemState::Running) {
        eprintln!(
            "qexo checkpoint: {} is {} — checkpoints ride on running work (heartbeat first)",
            item_arg,
            item.state.map(|s| s.name()).unwrap_or("in an unknown state")
        );
        return 1;
    }
    // Resolve every cited output from this node's own
    // chain before any write (§8): the cite and the
    // hash the origin will verify against.
    let mut output_pairs: Vec<(String, String)> = Vec::new();
    for o in &outputs {
        let body = match phext::parse_universe_addr(o) {
            Some((u, c)) => lat.universe(u).and_then(|s| s.get(&c)).cloned(),
            None => match phext::parse_coord(o) {
                Some(c) => lat.px.get(&c).cloned(),
                None => None,
            },
        };
        match body {
            Some(b) => {
                let cite = match phext::parse_universe_addr(o) {
                    Some(_) => o.clone(),
                    None => format!("$1:{}", o),
                };
                output_pairs.push((cite, sha256_hex(b.as_bytes())));
            }
            None => {
                eprintln!(
                    "qexo checkpoint: output {} cannot be read on this node — a checkpoint cites only what its own node can produce (§8)",
                    o
                );
                return 1;
            }
        }
    }
    let (_, stamp) = now_stamp();
    let actor_name = dispatch::local_actor(&lat.px);
    let stratum_u = (attempt + 1) as usize;
    // The running → running transition through the
    // oracle first (its receipt lands in universe 1);
    // a refusal leaves every scroll untouched.
    let cite_coord = {
        let stratum = lat.universe(stratum_u).cloned().unwrap_or_default();
        let first8: [u32; 8] = addr[..8].try_into().unwrap();
        lattice::next_free_scroll(&stratum, first8, 4)
    };
    let cite = format!("${}:{}", stratum_u, phext::fmt_coord(&cite_coord));
    let mut t = Transition::new(ItemState::Running, Actor::Executor, &actor_name);
    t.evidence = format!("checkpoint {} — {}", cite, note);
    if let Err(e) = dispatch::apply_transition(&mut lat, item.book, &t) {
        eprintln!("qexo checkpoint: the transition was refused by the oracle: {}", e);
        return 1;
    }
    let mut stratum = lat.universe(stratum_u).cloned().unwrap_or_default();
    let mut fields: Vec<(&str, &str)> = vec![("at", &stamp), ("note", &note)];
    for (cite, hash) in &output_pairs {
        fields.push(("output", cite));
        fields.push(("output-sha256", hash));
    }
    let text = format!("checkpoint\n{}", lattice::record(&fields));
    stratum.insert(cite_coord, text);
    if let Some(copy) = stratum.get(&addr).cloned() {
        let copy = lattice::set_field(&copy, "checkpoint", &cite);
        let copy = lattice::set_field(&copy, "heartbeat", &stamp);
        stratum.insert(addr, copy);
    }
    lat.set_universe(stratum_u, stratum);
    let asg_text = lat.px.get(&asg.addr).cloned().unwrap_or_default();
    let asg_text = lattice::set_field(&asg_text, "checkpoint", &cite);
    let asg_text = lattice::set_field(&asg_text, "heartbeat", &stamp);
    lat.px.insert(asg.addr, asg_text);
    if let Err(e) = lat.save() {
        eprintln!("qexo checkpoint: cannot save state: {}", e);
        return 1;
    }
    println!("checkpoint {} at {} — {}", item_arg, cite, note);
    0
}

/// `qexo fail <item-address> --note <evidence>` —
/// executor-local (§4.6): the oracle's failure
/// transition on the attempt copy with the evidence
/// the spec requires (the plan lands the third
/// consecutive failure in paused-auto, §8), and the
/// assignment retired with the same evidence as its
/// status — the origin reads it on its next heartbeat
/// reconciliation.
pub fn cmd_fail(args: &[String]) -> i32 {
    let mut item_arg: Option<&str> = None;
    let mut note: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--note" => {
                i += 1;
                note = args.get(i).cloned();
            }
            other if !other.starts_with("--") && item_arg.is_none() => item_arg = Some(other),
            other => {
                eprintln!("qexo fail: unexpected argument `{}`", other);
                return 2;
            }
        }
        i += 1;
    }
    let (item_arg, note) = match (item_arg, note) {
        (Some(a), Some(n)) if !n.trim().is_empty() => (a, n),
        _ => {
            eprintln!("usage: qexo fail <item-address> --note <evidence>");
            return 2;
        }
    };
    let mut lat = match require_born() {
        Ok(l) => l,
        Err(code) => return code,
    };
    let addr = match phext::parse_coord(item_arg) {
        Some(a) => a,
        None => {
            eprintln!("qexo fail: `{}` is not an item address", item_arg);
            return 2;
        }
    };
    let (asg, _t, attempt) = match my_execution_assignment(&lat, &addr) {
        Some(x) => x,
        None => {
            eprintln!("qexo fail: this node holds no live execution assignment for {}", item_arg);
            return 1;
        }
    };
    let item = match held_item(&lat, &addr) {
        Some(i) => i,
        None => {
            eprintln!("qexo fail: no attempt copy held for {}", item_arg);
            return 1;
        }
    };
    let actor_name = dispatch::local_actor(&lat.px);
    let mut t = Transition::new(ItemState::Queued, Actor::Executor, &actor_name);
    t.evidence = note.clone();
    if let Err(e) = dispatch::apply_transition(&mut lat, item.book, &t) {
        eprintln!("qexo fail: the transition was refused by the oracle: {}", e);
        return 1;
    }
    // The stratum copy lands where the oracle landed
    // the item (queued, or paused-auto at the third
    // consecutive failure).
    let landed = held_item(&lat, &addr).and_then(|i| i.state).map(|s| s.name()).unwrap_or("queued");
    let stratum_u = (attempt + 1) as usize;
    if let Some(mut stratum) = lat.universe(stratum_u).cloned() {
        if let Some(copy) = stratum.get(&addr).cloned() {
            let copy = lattice::set_field(&copy, "state", landed);
            stratum.insert(addr, copy);
            lat.set_universe(stratum_u, stratum);
        }
    }
    retire_assignment(&mut lat, &asg.addr, &format!("failed — {}", note));
    if let Err(e) = lat.save() {
        eprintln!("qexo fail: cannot save state: {}", e);
        return 1;
    }
    println!("failed {} — {} (attempt copy now {})", item_arg, note, landed);
    0
}

// ----------------------------------------------------------------
// The doctor's combination-6 recomputation (§4.6):
// rebuild the unsigned bundle from an attempt
// universe and compare its hash to the record's.
// ----------------------------------------------------------------

/// Recompute an accepted attempt's bundle hash from
/// its stratum: the manifest verbatim (filed under
/// the assignment's Book family), the pristine record
/// copy reconstructed from the working copy (the
/// acceptance's own writes stripped, the
/// pre-transition state restored), the brief, and the
/// context universes regrouped by their provenance
/// scrolls. Returns None when the stratum lacks the
/// pieces (combination 7 owns that finding) and
/// Some(hash) otherwise. The origin's as-dispatched
/// snapshot recomputes identically — its record copy
/// is already pristine, so the strips are no-ops.
pub fn recompute_bundle_sha256(lat: &Lattice, item_addr: &Coord, attempt: u32) -> Option<String> {
    let stratum = lat.universe((attempt + 1) as usize)?;
    let item_s = phext::fmt_coord(item_addr);
    let mut asg_book = None;
    for rec in dispatch::load_assignments(&lat.px) {
        if rec.item.as_deref() == Some(item_s.as_str())
            && rec.attempt.as_deref().and_then(|a| a.parse::<u32>().ok()) == Some(attempt)
        {
            asg_book = Some(rec.addr[5]);
            break;
        }
    }
    let asg_book = asg_book?;
    let manifest = stratum.get(&stratum_manifest_addr(asg_book))?.clone();
    if lattice::field(&manifest, "item").as_deref() != Some(item_s.as_str()) {
        return None;
    }
    let item_addr = *item_addr;
    let book = item_addr[5];
    let working = stratum.get(&item_addr)?.clone();
    let mut pristine = remove_field(&working, "bundle-sha256");
    pristine = remove_field(&pristine, "home");
    pristine = remove_field(&pristine, "heartbeat");
    pristine = remove_field(&pristine, "checkpoint");
    pristine = lattice::set_field(&pristine, "state", "queued");
    pristine = lattice::set_field(&pristine, "attempts", &attempt.saturating_sub(1).to_string());
    let mut u1 = Phext::new();
    u1.insert(BUNDLE_ANCHOR, manifest);
    u1.insert(item_addr, pristine);
    if let Some(brief) = stratum.get(&dispatch::brief_addr(book)) {
        u1.insert(dispatch::brief_addr(book), brief.clone());
    }
    let mut universes = vec![u1];
    // Context universes, in manifest order: the
    // provenance at Scroll k of the assignment's
    // stratum family names its members.
    let mut k = 2usize;
    loop {
        let cite = stratum_provenance_addr(asg_book, k);
        let prov = match stratum.get(&cite) {
            Some(p) => p.clone(),
            None => break,
        };
        let mut u = Phext::new();
        u.insert(BUNDLE_ANCHOR, prov.clone());
        if let Some(list) = lattice::field(&prov, "scrolls") {
            for part in list.split(',') {
                let part = part.trim();
                if let Some(c) = phext::parse_coord(part) {
                    if let Some(t) = stratum.get(&c) {
                        u.insert(c, t.clone());
                    }
                }
            }
        }
        universes.push(u);
        k += 1;
        if k > 64 {
            break;
        }
    }
    Some(sha256_hex(unsigned_chain_bytes(&universes).as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(name: &str) -> NodeFacts {
        NodeFacts {
            name: name.into(),
            endpoint: Some("http://127.0.0.1:8484".into()),
            capabilities: vec!["rust".into()],
            temp_c: Some(41.0),
            load1: Some(0.5),
            cores: Some(8),
            free_bytes: Some(1_000_000_000),
            resident_working: false,
            reachable: true,
        }
    }

    #[test]
    fn eligibility_names_each_gate() {
        let caps = vec!["rust".to_string()];
        assert!(eligibility(&facts("a"), &caps, 100).is_ok());
        let mut f = facts("a");
        f.reachable = false;
        assert_eq!(eligibility(&f, &caps, 100).unwrap_err(), "(a) unreachable");
        let mut f = facts("a");
        f.temp_c = None;
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("(b) temperature unmeasured"));
        let mut f = facts("a");
        f.temp_c = Some(75.0);
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("75"));
        let mut f = facts("a");
        f.temp_c = Some(74.9);
        assert!(eligibility(&f, &caps, 100).is_ok());
        let mut f = facts("a");
        f.load1 = Some(8.0);
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("(b) load"));
        let mut f = facts("a");
        f.resident_working = true;
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("(c) resident"));
        let mut f = facts("a");
        f.free_bytes = None;
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("(d)"));
        let mut f = facts("a");
        f.free_bytes = Some(50);
        assert!(eligibility(&f, &caps, 100).unwrap_err().contains("not stageable"));
        assert!(eligibility(&facts("a"), &["gpu".to_string()], 100)
            .unwrap_err()
            .contains("capability"));
    }

    #[test]
    fn draw_is_reproducible_uniform_and_never_invents_a_node() {
        let set: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(draw(&set, 42), draw(&set, 42));
        assert!(draw(&[], 42).is_none());
        let mut counts = [0usize; 4];
        for seed in 0..4000u64 {
            let pick = draw(&set, seed).unwrap();
            counts[set.iter().position(|n| n == &pick).unwrap()] += 1;
        }
        for c in counts {
            assert!((850..=1150).contains(&c), "count {} outside the fairness tolerance", c);
        }
        for seed in 0..100u64 {
            assert!(set.contains(&draw(&set, seed).unwrap()));
        }
    }

    #[test]
    fn placement_records_exclusions_and_the_empty_set_places_nothing() {
        let mut hot = facts("hot");
        hot.temp_c = Some(81.0);
        let cool = facts("cool");
        let p = place(&[hot.clone(), cool], &["rust".into()], 100, None);
        assert_eq!(p.eligible, vec!["cool".to_string()]);
        assert_eq!(p.excluded.len(), 1);
        assert!(p.excluded[0].1.contains("81"));
        assert_eq!(p.chosen.as_deref(), Some("cool"));
        let p = place(&[hot], &["rust".into()], 100, None);
        assert!(p.eligible.is_empty());
        assert!(p.chosen.is_none());
        // A named ineligible node is never chosen.
        let cold = facts("cold");
        let p = place(&[cold], &["rust".into()], 100, Some("hot"));
        assert!(p.chosen.is_none());
        assert!(p.named);
    }

    fn fixture_px() -> Phext {
        let mut px = Phext::new();
        px.insert(
            lattice::IDENTITY,
            "qexo node config\nnode: alpha\ncapability: rust\n".into(),
        );
        px.insert(
            lattice::NODE_KEY_RECORD,
            "node key\nfingerprint: AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n".into(),
        );
        let record = dispatch::item_record(
            "Test item",
            Some("3.3.3/3.3.3/3.3.3"),
            None,
            "test",
            "2026-11-17 00:00 UTC",
            "alpha AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "test goal",
            Some(5),
            Some(2),
            None,
            None,
        );
        px.insert(dispatch::item_addr(1), record);
        px.insert(
            dispatch::brief_addr(1),
            "brief\ncapability: rust\ncontext: 5.5.5/5.5.5/5.5.5\ndo the thing\n".into(),
        );
        px.insert([5, 5, 5, 5, 5, 5, 5, 5, 5], "context evidence\n".into());
        px
    }

    fn fixture_item(px: &Phext) -> Item {
        dispatch::parse_item(1, px.get(&dispatch::item_addr(1)).unwrap())
    }

    fn born_lat(px: Phext) -> Lattice {
        let mut lat = Lattice::born(std::env::temp_dir().join("qexo-wire-test-state.phext"));
        lat.px = px;
        lat
    }

    #[test]
    fn bundle_carries_the_routed_bridge_in_its_identity() {
        let px = fixture_px();
        let item = fixture_item(&px);
        let plain = build_bundle(&px, &item, 1, None, None, None).unwrap();
        let routed = build_bundle(
            &px,
            &item,
            1,
            None,
            None,
            Some(("claude", "http://127.0.0.1:8791/")),
        )
        .unwrap();
        // The choice is cited in the manifest (§9.2) and
        // is part of the bundle's identity: a different
        // route is a different bundle (§4.3).
        assert!(routed.manifest.contains("bridge: claude"));
        assert!(routed.manifest.contains("bridge-endpoint: http://127.0.0.1:8791/"));
        assert_ne!(plain.sha256, routed.sha256);
        // Deterministic: the same route rebuilds to the
        // same identity — the property the doctor's
        // recomputation (which reads the filed manifest
        // verbatim) relies on.
        let again = build_bundle(
            &px,
            &item,
            1,
            None,
            None,
            Some(("claude", "http://127.0.0.1:8791/")),
        )
        .unwrap();
        assert_eq!(routed.sha256, again.sha256);
        // The brief's preference lines parse in order.
        let facts = brief_facts("brief\nbridge: grok\nbridge: claude\nbridge: grok\n");
        assert_eq!(facts.bridges, vec!["grok".to_string(), "claude".to_string()]);
    }

    #[test]
    fn bundle_canonical_form_is_deterministic_and_self_describing() {
        let px = fixture_px();
        let item = fixture_item(&px);
        let b1 = build_bundle(&px, &item, 1, None, None, None).unwrap();
        let b2 = build_bundle(&px, &item, 1, None, None, None).unwrap();
        assert_eq!(b1.sha256, b2.sha256, "an unchanged item re-dispatches to the same identity (§4.3)");
        assert_eq!(b1.universes.len(), 2, "spec universe + one context universe");
        assert!(b1.manifest.contains("item: 11.11.11/2.1.1/1.1.1"));
        assert!(b1.manifest.contains("coordinate: 3.3.3/3.3.3/3.3.3"));
        assert!(b1.manifest.contains("origin: alpha AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(b1.manifest.contains("attempt: 1"));
        assert!(b1.manifest.contains("capability: rust"));
        assert!(b1.manifest.contains("sealed: no"));
        // The context universe's hash line matches its bytes.
        let ctx_hash = sha256_hex(b1.universes[1].serialize().as_bytes());
        assert!(b1.manifest.contains(&format!("universe: 2 sha256: {}", ctx_hash)));
        // The signature scroll never enters the hash.
        let mut signed = b1.universes.clone();
        signed[0].insert(BUNDLE_SIG, "fake signature".into());
        assert_eq!(
            sha256_hex(unsigned_chain_bytes(&signed).as_bytes()),
            b1.sha256,
            "the hash is over the unsigned bundle"
        );
        // A changed brief is a different bundle.
        let mut px2 = fixture_px();
        px2.insert(dispatch::brief_addr(1), "brief\ncapability: rust\ndo another thing\n".into());
        let item2 = fixture_item(&px2);
        let b3 = build_bundle(&px2, &item2, 1, None, None, None).unwrap();
        assert_ne!(b1.sha256, b3.sha256);
    }

    #[test]
    fn doctor_recomputation_round_trips_and_detects_divergence() {
        let px = fixture_px();
        let item = fixture_item(&px);
        let bundle = build_bundle(&px, &item, 1, None, None, None).unwrap();
        // Parse the unsigned universes as a wire bundle
        // (parse does not need the signature; verify does).
        let wire = Phext::serialize_universes(&bundle.universes, true);
        let pb = parse_wire_bundle(wire.as_bytes()).unwrap();
        assert_eq!(pb.sha256, bundle.sha256);
        assert_eq!(pb.item_addr, dispatch::item_addr(1));
        let mut lat = born_lat(Phext::new());
        // The assignment record comes first — the
        // attempt's paperwork files under its Book.
        write_assignment(
            &mut lat,
            &[
                ("item", &phext::fmt_coord(&pb.item_addr)),
                ("node", "beta"),
                ("attempt", "1"),
                ("bundle-sha256", &pb.sha256),
                ("home", "alpha"),
            ],
        );
        file_attempt(&mut lat, &pb, 1).unwrap();
        // The working copy carries home + hash + state.
        let copy = lat.px.get(&pb.item_addr).unwrap();
        assert!(copy.contains("home: alpha"));
        assert!(copy.contains(&format!("bundle-sha256: {}", bundle.sha256)));
        assert!(copy.contains("state: dispatched"));
        // Combination 6: the recomputation reproduces
        // the accepted hash exactly.
        assert_eq!(recompute_bundle_sha256(&lat, &pb.item_addr, 1).as_deref(), Some(bundle.sha256.as_str()));
        // Tamper with the filed brief: the recomputation
        // diverges — the digest check is real.
        let mut stratum = lat.universe(2).unwrap().clone();
        stratum.insert(dispatch::brief_addr(1), "brief\ncapability: rust\ntampered\n".into());
        lat.set_universe(2, stratum);
        let recomputed = recompute_bundle_sha256(&lat, &pb.item_addr, 1).unwrap();
        assert_ne!(recomputed, bundle.sha256);
        // Attempt 2: the origin's record now carries
        // attempt 1's bundle hash (the dispatch verb
        // sets it) — the builder strips it, so the
        // attempt-2 bundle still recomputes exactly.
        let mut px2 = fixture_px();
        let rec2 = lattice::set_field(px2.get(&item.addr).unwrap(), "bundle-sha256", &bundle.sha256);
        let rec2 = lattice::set_field(&rec2, "attempts", "1");
        px2.insert(item.addr, rec2);
        let item2 = fixture_item(&px2);
        let bundle2 = build_bundle(&px2, &item2, 2, None, None, None).unwrap();
        let wire2 = Phext::serialize_universes(&bundle2.universes, true);
        let pb2 = parse_wire_bundle(wire2.as_bytes()).unwrap();
        let mut lat2 = born_lat(Phext::new());
        write_assignment(
            &mut lat2,
            &[
                ("item", &phext::fmt_coord(&pb2.item_addr)),
                ("node", "beta"),
                ("attempt", "2"),
                ("bundle-sha256", &pb2.sha256),
                ("home", "alpha"),
            ],
        );
        file_attempt(&mut lat2, &pb2, 1).unwrap();
        assert_eq!(recompute_bundle_sha256(&lat2, &pb2.item_addr, 2).as_deref(), Some(bundle2.sha256.as_str()));
    }

    #[test]
    fn file_attempt_refuses_a_foreign_bundle_at_the_same_address() {
        let px = fixture_px();
        let item = fixture_item(&px);
        let bundle = build_bundle(&px, &item, 1, None, None, None).unwrap();
        let wire = Phext::serialize_universes(&bundle.universes, true);
        let pb = parse_wire_bundle(wire.as_bytes()).unwrap();
        let mut lat = born_lat(Phext::new());
        // Occupy the stratum address with a different
        // bundle's copy.
        let mut stratum = Phext::new();
        stratum.insert(
            pb.item_addr,
            "queue item\nstate: dispatched\nbundle-sha256: deadbeef\n".into(),
        );
        lat.set_universe(2, stratum);
        let err = file_attempt(&mut lat, &pb, 1).unwrap_err();
        assert!(err.contains("different bundle"), "{}", err);
    }

    #[test]
    fn assignment_liveness_is_the_status_line() {
        // §3.1's contract, slice 2's reading: an
        // assignment is LIVE while it carries no
        // `status:` line — slice 4 is the writer.
        let mut lat = born_lat(Phext::new());
        let item = dispatch::item_addr(1);
        let addr = write_assignment(
            &mut lat,
            &[
                ("item", &phext::fmt_coord(&item)),
                ("node", "beta"),
                ("attempt", "1"),
                ("bundle-sha256", "ab12"),
                ("home", "alpha"),
            ],
        );
        let (rec, _t) = live_assignment_for(&lat, &item, 1).expect("live after write");
        assert_eq!(rec.addr, addr);
        retire_assignment(&mut lat, &addr, "failed — boom");
        assert!(live_assignment_for(&lat, &item, 1).is_none(), "a status line retires it");
        let recs = dispatch::load_assignments(&lat.px);
        assert_eq!(recs.len(), 1);
        assert!(!recs[0].live);
    }

    #[test]
    fn resume_bundle_carries_the_checkpoint_and_recomputes() {
        // §6 teleport, §8: a re-dispatch's bundle names
        // resume-from: in the manifest and carries the
        // checkpoint scroll itself as its last universe;
        // the receiving side's recompute reproduces the
        // identity from the filed paperwork (§4.3).
        // Attempt 2 is built from the record as it
        // stands after attempt 1 (attempts: 1) — the
        // same fixture discipline as the recompute test
        // above.
        let mut px = fixture_px();
        let rec = lattice::set_field(px.get(&dispatch::item_addr(1)).unwrap(), "attempts", "1");
        px.insert(dispatch::item_addr(1), rec);
        let item = fixture_item(&px);
        let cp_text = "checkpoint\nat: 2026-11-17 10:00 CST\nnote: half the welds done\n";
        let bundle = build_bundle(&px, &item, 2, Some("$2:11.11.11/2.1.1/1.1.4"), Some(cp_text), None).unwrap();
        assert!(bundle.manifest.contains("attempt: 2"));
        assert!(bundle.manifest.contains("resume-from: $2:11.11.11/2.1.1/1.1.4"));
        assert_eq!(bundle.universes.len(), 3, "spec + context + resume");
        let resume_u = &bundle.universes[2];
        let prov = resume_u.get(&BUNDLE_ANCHOR).unwrap();
        assert!(prov.contains("kind: resume-checkpoint"), "{}", prov);
        assert!(prov.contains("source: $2:11.11.11/2.1.1/1.1.4"));
        let cp_coord = [11, 11, 11, 2, 1, 1, 1, 1, 4];
        assert_eq!(resume_u.get(&cp_coord).map(|s| s.as_str()), Some(cp_text));
        // The manifest's hash list covers the resume
        // universe like any other.
        let h = sha256_hex(resume_u.serialize().as_bytes());
        assert!(bundle.manifest.contains(&format!("universe: 3 sha256: {}", h)));
        // A cite without held content still names the
        // resume point — and carries no resume universe.
        let bare = build_bundle(&px, &item, 2, Some("$2:11.11.11/2.1.1/1.1.4"), None, None).unwrap();
        assert!(bare.manifest.contains("resume-from: $2:11.11.11/2.1.1/1.1.4"));
        assert_eq!(bare.universes.len(), 2);
        // File the resume bundle on a receiving lattice:
        // the checkpoint lands in the attempt stratum,
        // and the doctor's recomputation reproduces the
        // bundle identity exactly (combination 6).
        let wire = Phext::serialize_universes(&bundle.universes, true);
        let pb = parse_wire_bundle(wire.as_bytes()).unwrap();
        assert_eq!(pb.resume_from.as_deref(), Some("$2:11.11.11/2.1.1/1.1.4"));
        let mut lat = born_lat(Phext::new());
        write_assignment(
            &mut lat,
            &[
                ("item", &phext::fmt_coord(&pb.item_addr)),
                ("node", "beta"),
                ("attempt", "2"),
                ("bundle-sha256", &pb.sha256),
                ("home", "alpha"),
            ],
        );
        file_attempt(&mut lat, &pb, 1).unwrap();
        let stratum = lat.universe(3).unwrap();
        assert_eq!(stratum.get(&cp_coord).map(|s| s.as_str()), Some(cp_text), "the checkpoint scroll is filed in the new attempt's stratum");
        assert_eq!(recompute_bundle_sha256(&lat, &pb.item_addr, 2).as_deref(), Some(bundle.sha256.as_str()));
    }

    #[test]
    fn fencing_refuses_stale_and_closed_attempts() {
        // §4.6 combination 2, completed in slice 5: a
        // bundle below the highest filed attempt —
        // live or already closed — is stale, and a
        // closed attempt is never re-run. The next
        // attempt is never fenced.
        let mut lat = born_lat(Phext::new());
        let item = dispatch::item_addr(1);
        let item_s = phext::fmt_coord(&item);
        let a2 = write_assignment(
            &mut lat,
            &[
                ("item", &item_s),
                ("node", "beta"),
                ("attempt", "2"),
                ("bundle-sha256", "ab12"),
                ("home", "alpha"),
            ],
        );
        assert!(fencing_block(&lat, &item, 1).unwrap().contains("below the live attempt 2"));
        assert!(fencing_block(&lat, &item, 2).is_none(), "an equal live attempt is dedup's business, not fencing's");
        assert!(fencing_block(&lat, &item, 3).is_none());
        retire_assignment(&mut lat, &a2, "failed — boom");
        assert!(fencing_block(&lat, &item, 2).unwrap().contains("already closed"), "a closed attempt is never re-run");
        assert!(fencing_block(&lat, &item, 1).unwrap().contains("already closed"));
        assert!(fencing_block(&lat, &item, 3).is_none(), "the next attempt is never fenced");
        // Another item's history fences nothing here.
        let other = dispatch::item_addr(2);
        assert!(fencing_block(&lat, &other, 1).is_none());
    }

    #[test]
    fn checkpoint_outputs_verify_by_readback() {
        // §8: verified progress is the coordinator's
        // READ of the cited outputs, hash-matched —
        // never the executor's say-so.
        let out_text = "the welded frame\n";
        let hash = sha256_hex(out_text.as_bytes());
        let cp = format!(
            "checkpoint\nat: 2026-11-17 10:00 CST\nnote: frame done\noutput: $2:5.5.5/5.5.5/5.5.5\noutput-sha256: {}\n",
            hash
        );
        let read = |o: &str| {
            if o == "$2:5.5.5/5.5.5/5.5.5" {
                Some(out_text.to_string())
            } else {
                None
            }
        };
        assert_eq!(check_checkpoint_outputs(&cp, &read), Ok(1));
        // Tampered content: the hash no longer matches.
        let read_bad = |_: &str| Some("forged\n".to_string());
        assert!(check_checkpoint_outputs(&cp, &read_bad).unwrap_err().contains("hashes to"));
        // An output that cannot be read back.
        let read_none = |_: &str| None;
        assert!(check_checkpoint_outputs(&cp, &read_none).unwrap_err().contains("cannot be read back"));
        // A checkpoint citing nothing verifies nothing —
        // Ok(0), which the caller never counts as
        // progress.
        let bare = "checkpoint\nat: 2026-11-17 10:00 CST\nnote: thinking\n";
        assert_eq!(check_checkpoint_outputs(bare, &read_none), Ok(0));
        // A cite without its hash is malformed.
        let lopsided = "checkpoint\noutput: $2:5.5.5/5.5.5/5.5.5\n";
        assert!(check_checkpoint_outputs(lopsided, &read).is_err());
    }
}
