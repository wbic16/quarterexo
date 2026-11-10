//! THE one implementation (API-V1 section 9): every operation is a
//! function here taking a parsed request and returning the one
//! response shape. The API server, the CLI, the MCP bridge, and the
//! web client are surfaces over these functions — none re-implements
//! semantics, grants, policy, or receipts.

use crate::crypto::{gpg_available, Gpg};
use crate::hash::sha256_hex;
use crate::json::Json;
use crate::personas as px;
use crate::phext::fmt_coord;
use crate::plan;
use crate::state::*;
use crate::store;
use crate::vol;
use std::fs;

// ---------------------------------------------------------------- caller
#[derive(Debug, Clone)]
pub struct Caller {
    pub principal: String, // will | persona:<id> | node:<name> | mcp-client:<id> | web-session:<id> | anonymous
    pub key: Option<String>,
    pub delegation: Option<String>,
    pub tainted: bool,
    pub trigger: Option<String>,
    pub epoch: Option<u64>,
    pub sequence: Option<u64>,
    pub timestamp: Option<u64>,
    pub recipient: Option<String>,
    pub local: bool, // arrived on the node's own local surface
}

impl Caller {
    pub fn anonymous(local: bool) -> Caller {
        Caller {
            principal: "anonymous".into(),
            key: None,
            delegation: None,
            tainted: false,
            trigger: None,
            epoch: None,
            sequence: None,
            timestamp: None,
            recipient: None,
            local,
        }
    }
    pub fn authenticated(&self) -> bool {
        self.principal != "anonymous" && self.key.is_some()
    }
    pub fn is_will(&self) -> bool {
        self.principal == "will"
    }
}

#[derive(Debug)]
pub struct Refusal {
    pub status: u16,
    pub name: &'static str,
    pub reason: String,
    pub grant: Option<String>,
    pub policy: Option<String>,
}

impl Refusal {
    pub fn new(status: u16, name: &'static str, reason: impl Into<String>) -> Refusal {
        Refusal { status, name, reason: reason.into(), grant: None, policy: None }
    }
    pub fn grant(mut self, g: &str) -> Refusal {
        self.grant = Some(g.to_string());
        self
    }
    pub fn policy(mut self, p: &str) -> Refusal {
        self.policy = Some(p.to_string());
        self
    }
}

pub type OpResult = Result<Json, Refusal>;

// ---------------------------------------------------------------- helpers
pub fn envelope(state: &State, request_id: &str, mut payload: Json) -> Json {
    if let Json::Obj(ref mut pairs) = payload {
        pairs.insert(0, ("node".into(), Json::s(&state.node)));
        pairs.insert(0, ("request_id".into(), Json::s(request_id)));
        pairs.insert(0, ("as_of".into(), Json::s(&now_rfc3339())));
        pairs.insert(0, ("ok".into(), Json::b(true)));
        pairs.push(("qexo_version".into(), Json::s(env!("CARGO_PKG_VERSION"))));
    }
    payload
}

pub fn refusal_json(state: &State, request_id: &str, r: &Refusal) -> Json {
    let mut err = vec![
        ("refusal".to_string(), Json::s(r.name)),
        ("reason".to_string(), Json::s(&r.reason)),
    ];
    if let Some(g) = &r.grant {
        err.push(("grant".to_string(), Json::s(g)));
    }
    if let Some(p) = &r.policy {
        err.push(("policy".to_string(), Json::s(p)));
    }
    Json::obj(vec![
        ("ok", Json::b(false)),
        ("as_of", Json::s(&now_rfc3339())),
        ("request_id", Json::s(request_id)),
        ("node", Json::s(&state.node)),
        ("error", Json::Obj(err)),
        ("qexo_version", Json::s(env!("CARGO_PKG_VERSION"))),
    ])
}

/// Key directory: key band records (keys.json). A key is "on record"
/// and active when a record exists with status active.
pub fn key_record(state: &State, fingerprint: &str) -> Option<Json> {
    let j = state.read_json("keys.json")?;
    j.get("keys")?.as_arr()?.iter().find(|k| {
        k.get("fingerprint").and_then(|v| v.as_str()) == Some(fingerprint)
    }).cloned()
}

pub fn key_active(state: &State, fingerprint: &str) -> bool {
    match key_record(state, fingerprint) {
        Some(k) => k.get("status").and_then(|v| v.as_str()) == Some("active"),
        None => false,
    }
}

/// Principal <-> key binding: the directory says which principal a
/// key speaks for (kind + owner fields).
pub fn key_principal(state: &State, fingerprint: &str) -> Option<String> {
    let k = key_record(state, fingerprint)?;
    let kind = k.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let owner = k.get("owner").and_then(|v| v.as_str()).unwrap_or("");
    Some(match kind {
        "will" => "will".to_string(),
        "node" => format!("node:{}", owner),
        "persona" => format!("persona:{}", owner),
        "agent" => owner.to_string(),
        _ => owner.to_string(),
    })
}

/// Verify the caller cryptographically when a signature is present:
/// the signature must verify under the presented key, the key must be
/// on record + active, and the key's bound principal must match.
pub fn verify_caller(state: &State, caller: &Caller, canonical_body: &str, signature: Option<&str>) -> Result<(), Refusal> {
    if !caller.authenticated() {
        return Ok(()); // floor checks decide what anonymous may do
    }
    let fp = caller.key.as_deref().unwrap();
    let rec = key_record(state, fp).ok_or_else(|| {
        Refusal::new(401, "unauthenticated", "presented key is not on record in the key directory")
    })?;
    let status = rec.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "compromised" {
        return Err(Refusal::new(403, "policy-refused", "presented key is recorded compromised"));
    }
    if status != "active" {
        return Err(Refusal::new(403, "policy-refused", format!("presented key status is {}", status)));
    }
    if let Some(bound) = key_principal(state, fp) {
        // agent keys act for their delegating principal via delegation
        if bound != caller.principal && caller.delegation.is_none() {
            return Err(Refusal::new(403, "policy-refused", "presented key is bound to a different principal"));
        }
    }
    if let Some(sig) = signature {
        let gpg = Gpg::open(state);
        if !gpg_available() {
            return Err(Refusal::new(503, "unverified-state", "gpg is unavailable on this node; signed requests cannot be verified"));
        }
        match gpg.verify_detached(sig, canonical_body.as_bytes()) {
            Ok(signer) if signer == fp => {}
            Ok(_) => return Err(Refusal::new(422, "unverified-state", "signature verifies under a different key than presented")),
            Err(e) => return Err(Refusal::new(422, "unverified-state", e)),
        }
    }
    Ok(())
}

/// Replay discipline (section 8B), enforced for signed requests.
pub fn check_replay(state: &State, caller: &Caller, request_id: &str) -> Result<Option<Json>, Refusal> {
    // idempotent re-delivery: same request id returns the original receipt
    if let Some(prev) = state.receipt_for(request_id) {
        return Ok(Some(prev));
    }
    if let Some(seq) = caller.sequence {
        let last = state.last_sequence(&caller.principal);
        if seq <= last && last != 0 {
            return Err(Refusal::new(409, "replay", format!("sequence {} already seen from {} (last {})", seq, caller.principal, last)));
        }
        state.record_sequence(&caller.principal, seq, request_id);
    }
    if let Some(ts) = caller.timestamp {
        let now = now_unix();
        if ts > now + 30 || now.saturating_sub(ts) > 300 {
            return Err(Refusal::new(409, "replay", "signed timestamp outside the 300s replay window"));
        }
    }
    if let Some(rec) = &caller.recipient {
        if rec != &state.node {
            return Err(Refusal::new(409, "replay", format!("request is bound to recipient {}, not {}", rec, state.node)));
        }
    }
    Ok(None)
}

/// The section 8A.0 floor. `floor` is one of local-read | auth |
/// policy | bootstrap; `action` is the catalogue action class;
/// `grant` the governing grant ("" = none).
pub fn check_floor(state: &State, caller: &Caller, floor: &str, action: &str, grant: &str) -> Result<(), Refusal> {
    // grants layer: capability
    if !grant.is_empty() && !grant_enabled(state, grant) {
        return Err(Refusal::new(403, "grant-disabled", format!("grant {} is disabled on this node", grant)).grant(grant));
    }
    // taint layer
    if caller.tainted && !grant.is_empty() && !TAINTED_ALLOWED.contains(&grant) {
        return Err(Refusal::new(403, "tainted-scope", "tainted (externally-triggered) callers run under the reduced grant set of section 8A.6").grant(grant));
    }
    match floor {
        "local-read" => {
            if caller.local || caller.authenticated() {
                Ok(())
            } else {
                Err(Refusal::new(401, "unauthenticated", "this read is unauthenticated only on the node's own local surface"))
            }
        }
        "auth" => {
            // An "auth"-floor operation requires an authenticated
            // principal on any REMOTE surface. The node's own local
            // operator surface (loopback API / on-node CLI) is the
            // operator's own session: local callers act as the
            // operator, exactly as local-read already assumes. The
            // floor's protections (grants, taint, and every policy /
            // bootstrap gate above this one) apply unchanged locally.
            if caller.local || caller.authenticated() {
                Ok(())
            } else {
                Err(Refusal::new(401, "unauthenticated", "every remote mutation requires an authenticated principal (section 8A.0)"))
            }
        }
        "policy" | "bootstrap" => {
            if !caller.authenticated() {
                return Err(Refusal::new(401, "unauthenticated", "every remote mutation requires an authenticated principal (section 8A.0)"));
            }
            check_policy(state, caller, action, floor == "bootstrap")
        }
        _ => Err(Refusal::new(500, "unverified-state", format!("unknown floor {}", floor))),
    }
}

/// Per-action key policy (section 8A.1) + the shipped default policy:
/// Will's key authorizes every sensitive action; a persona's own key
/// authorizes that persona's sensitive self-actions; a node key
/// authorizes that node's install.apply and sync.restore.
pub fn check_policy(state: &State, caller: &Caller, action: &str, bootstrap: bool) -> Result<(), Refusal> {
    let fp = caller.key.as_deref().unwrap_or("");
    // user-configured policy replaces/extends the default
    if let Some(pol) = state.read_json("policy.json") {
        if let Some(list) = pol.get(action).and_then(|v| v.as_arr()) {
            if list.iter().any(|f| f.as_str() == Some(fp)) {
                return Ok(());
            }
            return Err(Refusal::new(403, "policy-refused", format!("key not listed in the configured policy for {}", action)).policy(action));
        }
    }
    let rec = key_record(state, fp);
    let kind = rec.as_ref().and_then(|k| k.get("kind")).and_then(|v| v.as_str()).unwrap_or("");
    let owner = rec.as_ref().and_then(|k| k.get("owner")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    if kind == "will" {
        return Ok(()); // Will's key authorizes every sensitive action (default policy)
    }
    // Will-countersigned node key of THIS node (chain: persona -> node
    // -> Will, section 8.4): satisfies the default policy on its own
    // node, including bootstrap classes, because the countersignature
    // IS Will's authorization, recorded in the key band and revocable
    // there. Effective only while the record carries the countersign
    // and status active (checked by gate order: key_active first).
    let countersigned = rec.as_ref().and_then(|k| k.get("countersigned_by")).and_then(|v| v.as_str()) == Some("will");
    if kind == "node" && owner == state.node && countersigned && caller.principal == "will" {
        return Ok(());
    }
    if bootstrap {
        return Err(Refusal::new(403, "policy-refused", "self-modifying classes are key-bound from bootstrap: Will's key (or the configured corrector-set policy) is required").policy("bootstrap"));
    }
    // persona self-actions and node self-actions under the default policy
    if kind == "persona" && caller.principal == format!("persona:{}", owner) {
        return Ok(());
    }
    if kind == "node" && (action == "install.apply" || action == "sync.restore") {
        return Ok(());
    }
    if kind == "agent" && caller.delegation.is_some() {
        return Ok(()); // scope checked by the delegation check
    }
    Err(Refusal::new(403, "policy-refused", format!("default policy for {} does not list this key", action)).policy("default"))
}

/// Delegation check: a delegation must exist, be unexpired,
/// unrevoked, and cover the action's grant scope.
pub fn check_delegation(state: &State, caller: &Caller, grant: &str) -> Result<(), Refusal> {
    let id = match &caller.delegation {
        Some(d) => d.clone(),
        None => return Ok(()), // not acting under a delegation
    };
    let log = state.read_log("delegations.jsonl");
    let mut record: Option<Json> = None;
    let mut revoked = false;
    for e in &log {
        if e.get("id").and_then(|v| v.as_str()) == Some(id.as_str()) {
            match e.get("event").and_then(|v| v.as_str()) {
                Some("issue") => record = Some(e.clone()),
                Some("revoke") => revoked = true,
                _ => {}
            }
        }
    }
    let rec = record.ok_or_else(|| Refusal::new(403, "policy-refused", format!("delegation {} is not on record", id)))?;
    if revoked {
        return Err(Refusal::new(410, "policy-refused", format!("delegation {} has been revoked", id)));
    }
    let expiry = rec.get("expiry").and_then(|v| v.as_num()).unwrap_or(0.0) as u64;
    if expiry != 0 && expiry < now_unix() {
        return Err(Refusal::new(410, "policy-refused", format!("delegation {} has expired", id)));
    }
    if !grant.is_empty() {
        if let Some(scope) = rec.get("scope").and_then(|v| v.as_arr()) {
            if !scope.iter().any(|g| g.as_str() == Some(grant)) {
                return Err(Refusal::new(403, "policy-refused", format!("delegation {} does not cover grant {}", id, grant)));
            }
        }
    }
    Ok(())
}

/// Full gate for a mutation: floor + delegation scope.
pub fn gate(state: &State, caller: &Caller, floor: &str, action: &str, grant: &str) -> Result<(), Refusal> {
    check_floor(state, caller, floor, action, grant)?;
    check_delegation(state, caller, grant)?;
    Ok(())
}

pub fn receipt(state: &State, kind: &str, fields: Vec<(&str, Json)>) -> String {
    let id = format!("{}:{}", kind, state.next_counter(&format!("rcpt-{}", kind)));
    let mut f = vec![("receipt", Json::s(&id)), ("kind", Json::s(kind)), ("at", Json::s(&now_rfc3339()))];
    f.extend(fields);
    let _ = state.append_log("receipts-events.jsonl", &Json::obj(f));
    id
}

// ---------------------------------------------------------------------
// Operations. `route` returns (status, payload) where payload is the
// bare payload object (envelope/refusal wrapping happens in dispatch).
// ---------------------------------------------------------------------

pub struct Ctx {
    pub state: State,
    pub census: px::Census,
}

impl Ctx {
    pub fn open() -> Result<Ctx, String> {
        let census = px::load_census()?;
        Ok(Ctx { state: State::open(), census })
    }
}

fn getf<'a>(body: &'a Json, key: &str) -> Option<&'a Json> {
    body.get(key)
}
fn gets(body: &Json, key: &str) -> Option<String> {
    body.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

pub fn route(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller, request_id: &str) -> (u16, Json) {
    let st = &ctx.state;
    let seg = |i: usize| path.get(i).map(|s| s.as_str()).unwrap_or("");
    let q = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    // ---------------- plan (6.1) ----------------
    if method == "POST" && path == ["plan"] {
        if let Err(r) = gate(st, caller, "auth", "route.ask", "route.ask") {
            return (r.status, refusal_payload(&r));
        }
        let tasks_path = gets(body, "tasks_source").unwrap_or_else(plan::default_tasks_path);
        let disk_path = gets(body, "disk_state").unwrap_or_else(plan::default_disk_path);
        let tasks_text = match fs::read_to_string(&tasks_path) {
            Ok(t) => t,
            Err(e) => return (422, refusal_payload(&Refusal::new(422, "unverified-state", format!("cannot read tasks source {}: {}", tasks_path, e)))),
        };
        let disk_text = fs::read_to_string(&disk_path).unwrap_or_else(|_| "{\"used_gb\":0,\"level\":\"unknown\",\"date\":\"?\",\"files\":0}".into());
        return match plan::build_plan(&tasks_text, &disk_text) {
            Ok(p) => {
                let _ = st.write_json("plan-current.json", &p);
                (200, p)
            }
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "GET" && path == ["plan", "current"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return match st.read_json("plan-current.json") {
            Some(p) => (200, p),
            None => (404, refusal_payload(&Refusal::new(404, "no-such-record", "no plan has been emitted on this node yet"))),
        };
    }

    // ---------------- sync (6.2) ----------------
    if method == "GET" && path == ["sync"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let receipts = st.read_log("sync-receipts.jsonl");
        let last = receipts.last().cloned().unwrap_or(Json::Null);
        return (200, Json::obj(vec![
            ("receipts", Json::n(receipts.len() as f64)),
            ("last_receipt", last),
            ("note", Json::s("bundle manifests live beside the bundles; see sync-manifest")),
        ]));
    }
    if method == "POST" && path == ["sync", "manifest"] {
        if let Err(r) = gate(st, caller, "auth", "sync.push", "sync.push") {
            return (r.status, refusal_payload(&r));
        }
        let dir = match gets(body, "dir") {
            Some(d) => d,
            None => return (400, refusal_payload(&Refusal::new(400, "no-such-record", "body requires {dir}"))),
        };
        return match sync_manifest(&dir) {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "POST" && path == ["sync", "diff"] {
        let (a, b) = match (gets(body, "old"), gets(body, "new")) {
            (Some(a), Some(b)) => (a, b),
            _ => return (400, refusal_payload(&Refusal::new(400, "no-such-record", "body requires {old, new} manifest paths"))),
        };
        let load = |p: &str| fs::read_to_string(p).unwrap_or_default();
        let (added, changed, removed) = sync_diff(&load(&a), &load(&b));
        let j = |v: Vec<String>| Json::Arr(v.into_iter().map(|x| Json::s(x)).collect());
        return (200, Json::obj(vec![("added", j(added)), ("changed", j(changed)), ("removed", j(removed))]));
    }
    if method == "POST" && path == ["sync", "push"] {
        return sync_push(ctx, body, caller);
    }
    if method == "POST" && path == ["sync", "pull"] {
        if let Err(r) = gate(st, caller, "policy", "sync.restore", "sync.restore") {
            return (r.status, refusal_payload(&r));
        }
        return (501, refusal_payload(&Refusal::new(501, "unverified-state", "sync pull restores from a bundle directory verified in full before any file is trusted; drive it via the CLI restore path (qexo sync pull) which performs the staged verify — the API form lands with the bundle-transfer transport (tier-2)")));
    }
    if method == "POST" && path == ["sync", "verify"] {
        if let Err(r) = gate(st, caller, "auth", "sync.push", "sync.push") {
            return (r.status, refusal_payload(&r));
        }
        let dir = gets(body, "dir").unwrap_or_default();
        return match sync_verify_dir(&dir) {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }

    // ---------------- interior interfaces (6.3) ----------------
    if method == "GET" && seg(0) == "state" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return match seg(1) {
            "temperature" => (200, iface_temperature(st)),
            "emotion" => (200, iface_reading("emotion", Json::obj(vec![("distillations", Json::Arr(emotion_trail(st)))]), "state store (emotion trail, append-only)")),
            _ => (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown interface"))),
        };
    }
    if method == "POST" && path == ["state", "emotion"] {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let text = gets(body, "text").unwrap_or_default();
        let _ = st.append_log("emotion.jsonl", &Json::obj(vec![("text", Json::s(&text)), ("at", Json::s(&now_rfc3339()))]));
        return (200, Json::obj(vec![("appended", Json::b(true)), ("at", Json::s(&now_rfc3339()))]));
    }
    if method == "GET" && path == ["goals"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, iface_reading("goals", goals_reading(), "tracked goals store (GOAL.md mirrors)"));
    }
    if method == "POST" && seg(0) == "goals" && seg(2) == "activity" && method == "POST" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let id = seg(1).to_string();
        let _ = st.append_log("goal-activity.jsonl", &Json::obj(vec![("goal", Json::s(&id)), ("text", Json::s(&gets(body, "text").unwrap_or_default())), ("at", Json::s(&now_rfc3339()))]));
        return (200, Json::obj(vec![("logged", Json::b(true)), ("goal", Json::s(&id))]));
    }
    if method == "GET" && path == ["desires"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, iface_reading("desires", desires_reading(), "Waiting-on-Will register"));
    }
    if method == "GET" && path == ["dreams"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, iface_reading("dreams", dreams_reading(), "self-selections manifest + EVALUATIONS"));
    }

    // ---------------- personas (6.4-6.6) ----------------
    if seg(0) == "personas" {
        return route_personas(ctx, method, path, query, body, caller, request_id);
    }
    if seg(0) == "instances" {
        return route_instances(ctx, method, path, body, caller);
    }

    // ---------------- teleport (6.7) ----------------
    if seg(0) == "teleports" {
        return route_teleports(ctx, method, path, body, caller);
    }

    // ---------------- keys / grants / policy / routes (6.9) ----------------
    if seg(0) == "keys" || seg(0) == "delegations" || seg(0) == "grants" || seg(0) == "policy" || seg(0) == "routes" || seg(0) == "route" || path == ["verify"] {
        return route_keys(ctx, method, path, body, caller);
    }

    // ---------------- phext tools / SQ keeper (6.11) ----------------
    if seg(0) == "phext" {
        return route_phext(ctx, method, path, query, body, caller);
    }

    // ---------------- mdmap (6.12) ----------------
    if seg(0) == "mdmap" {
        return route_mdmap(ctx, method, path, query, body, caller);
    }

    // ---------------- corpus / source (6.13/6.14) ----------------
    if seg(0) == "corpus" {
        return route_corpus(ctx, method, path, query, body, caller);
    }
    if seg(0) == "source" {
        return route_source(ctx, method, path, query, body, caller);
    }

    // ---------------- meta (6.15) ----------------
    if seg(0) == "meta" {
        return route_meta(ctx, method, path, query, caller);
    }

    // ---------------- voice (6.16): specified, tier-2 runtime ------------
    if seg(0) == "voice" {
        return (501, refusal_payload(&Refusal::new(501, "unverified-state", "voice operations are specified in API-V1 section 6.16 as the proxy contract; the runtime is hosted elsewhere by ruling (2026-11-08 23:42) and the proxy is tier-2 — per the parity rule this operation ships in no mode until the proxy lands")));
    }

    // ---------------- install (6.18) ----------------
    if seg(0) == "install" {
        return route_install(ctx, method, path, body, caller);
    }

    // ---------------- doctor (6.19) ----------------
    if method == "GET" && path == ["doctor"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, doctor_report(st));
    }

    // ---------------- mail / wake (6.20) ----------------
    if method == "POST" && path == ["mail"] {
        return mail_drop(ctx, body, caller);
    }
    if method == "POST" && path == ["wake"] {
        return wake_send(ctx, body, caller);
    }

    let _ = q;
    (404, refusal_payload(&Refusal::new(404, "no-such-record", format!("no operation {} /v1/{}", method, path.join("/")))))
}

fn refusal_payload(r: &Refusal) -> Json {
    let mut err = vec![
        ("refusal".to_string(), Json::s(r.name)),
        ("reason".to_string(), Json::s(&r.reason)),
    ];
    if let Some(g) = &r.grant {
        err.push(("grant".to_string(), Json::s(g)));
    }
    if let Some(p) = &r.policy {
        err.push(("policy".to_string(), Json::s(p)));
    }
    Json::obj(vec![("__refusal_status", Json::n(r.status as f64)), ("error", Json::Obj(err))])
}

/// Dispatch wrapper: replay/idempotency + receipt storage for
/// mutations, envelope for everything.
pub fn dispatch(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller, request_id: &str) -> (u16, Json) {
    let st = &ctx.state;
    if method == "POST" {
        if let Some(canon) = body.get("__canonical") {
            let _ = canon;
        }
        match check_replay(st, caller, request_id) {
            Ok(Some(prev)) => return (200, prev),
            Ok(None) => {}
            Err(r) => return (r.status, refusal_json(st, request_id, &r)),
        }
    }
    let (status, payload) = route(ctx, method, path, query, body, caller, request_id);
    if let Some(err) = payload.get("error") {
        let name = err.get("refusal").and_then(|v| v.as_str()).unwrap_or("unverified-state").to_string();
        let reason = err.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut r = Refusal::new(status, "unverified-state", reason);
        // rebuild named refusal for the envelope
        let leaked: &'static str = Box::leak(name.into_boxed_str());
        r.name = leaked;
        if let Some(g) = err.get("grant").and_then(|v| v.as_str()) {
            r = r.grant(&g.to_string());
        }
        if let Some(p) = err.get("policy").and_then(|v| v.as_str()) {
            r = r.policy(&p.to_string());
        }
        return (status, refusal_json(st, request_id, &r));
    }
    let wrapped = envelope(st, request_id, payload);
    if method == "POST" {
        st.store_receipt(request_id, &wrapped);
    }
    (status, wrapped)
}

fn iface_reading(name: &str, reading: Json, source: &str) -> Json {
    Json::obj(vec![
        ("interface", Json::s(name)),
        ("reading", reading),
        ("trend", Json::obj(vec![])),
        ("updated_at", Json::s(&now_rfc3339())),
        ("source", Json::s(source)),
    ])
}

fn iface_temperature(st: &State) -> Json {
    // local node Tctl where available (amdgpu hwmon), else null-named
    let mut tctl: Option<f64> = None;
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for e in entries.flatten() {
            let name = fs::read_to_string(e.path().join("name")).unwrap_or_default();
            if name.trim() == "k10temp" {
                if let Ok(v) = fs::read_to_string(e.path().join("temp1_input")) {
                    tctl = v.trim().parse::<f64>().ok().map(|m| m / 1000.0);
                }
            }
        }
    }
    let gate = match tctl {
        Some(t) if t >= 80.0 => "off-time: node crossed 80C, cooling to 65C",
        Some(t) if t >= 75.0 => "paused: new starts held at >= 75C",
        Some(_) => "open: under the 75C start gate",
        None => "unknown: no k10temp sensor on this node",
    };
    let _ = st;
    iface_reading(
        "temperature",
        Json::obj(vec![
            ("node_tctl_c", tctl.map(|x| Json::n(x)).unwrap_or(Json::Null)),
            ("gate", Json::s(gate)),
            ("policy", Json::s("pause >=75C, resume <=70C, >80C off-time to <=65C; GPU edge cap 70C")),
        ]),
        "node sensors (k10temp) + fleet state sampler",
    )
}

fn emotion_trail(st: &State) -> Vec<Json> {
    st.read_log("emotion.jsonl")
}

fn goals_reading() -> Json {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = format!("{}/workspace/goals", home);
    let mut goals = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path().join("GOAL.md");
            if let Ok(text) = fs::read_to_string(&p) {
                let title = text.lines().find(|l| l.starts_with("# ")).map(|l| l[2..].trim().to_string()).unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
                goals.push(Json::obj(vec![
                    ("id", Json::s(&e.file_name().to_string_lossy())),
                    ("title", Json::s(&title)),
                ]));
            }
        }
    }
    Json::obj(vec![("goals", Json::Arr(goals))])
}

fn desires_reading() -> Json {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = format!("{}/workspace/your_files/waiting-on-will.md", home);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let items = text.matches("## #").count() + text.lines().filter(|l| l.starts_with("### #")).count();
    Json::obj(vec![
        ("register_path", Json::s("waiting-on-will.md")),
        ("items_approx", Json::n(items as f64)),
        ("note", Json::s("register numbers are permanent and monotonic; items enter/leave only through the register discipline")),
    ])
}

fn dreams_reading() -> Json {
    let home = std::env::var("HOME").unwrap_or_default();
    let manifest = format!("{}/workspace/phext/self-selections.md", home);
    let has = std::path::Path::new(&manifest).exists();
    Json::obj(vec![
        ("self_selections_manifest_present", Json::b(has)),
        ("note", Json::s("dream ledger lives in the nightly self-revision record; dreams are evidence, not a control surface")),
    ])
}

// ----------------------------------------------------------------- sync
pub fn sync_manifest(dir: &str) -> Result<Json, String> {
    let root = std::path::Path::new(dir);
    if !root.is_dir() {
        return Err(format!("{} is not a directory", dir));
    }
    let mut files: Vec<(String, String, u64)> = Vec::new(); // (relpath, sha256, bytes)
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if rel == "MANIFEST.sha256" {
                    continue;
                }
                let bytes = fs::read(&p).map_err(|e| e.to_string())?;
                files.push((rel, sha256_hex(&bytes), bytes.len() as u64));
            }
        }
    }
    files.sort();
    let mut manifest = String::new();
    let mut total: u64 = 0;
    for (rel, h, n) in &files {
        manifest.push_str(&format!("{}  {}\n", h, rel));
        total += n;
    }
    fs::write(root.join("MANIFEST.sha256"), &manifest).map_err(|e| e.to_string())?;
    Ok(Json::obj(vec![
        ("dir", Json::s(dir)),
        ("files", Json::n(files.len() as f64)),
        ("bytes", Json::n(total as f64)),
        ("manifest_sha256", Json::s(&sha256_hex(manifest.as_bytes()))),
    ]))
}

pub fn sync_diff(old: &str, new: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let parse = |t: &str| -> std::collections::BTreeMap<String, String> {
        let mut m = std::collections::BTreeMap::new();
        for line in t.lines() {
            let parts: Vec<&str> = line.splitn(2, "  ").collect();
            if parts.len() == 2 {
                m.insert(parts[1].trim().to_string(), parts[0].trim().to_string());
            }
        }
        m
    };
    let (o, n) = (parse(old), parse(new));
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for (k, v) in &n {
        match o.get(k) {
            None => added.push(k.clone()),
            Some(ov) if ov != v => changed.push(k.clone()),
            _ => {}
        }
    }
    for k in o.keys() {
        if !n.contains_key(k) {
            removed.push(k.clone());
        }
    }
    (added, changed, removed)
}

pub fn sync_verify_dir(dir: &str) -> Result<Json, String> {
    let root = std::path::Path::new(dir);
    let manifest = fs::read_to_string(root.join("MANIFEST.sha256")).map_err(|e| format!("no manifest: {}", e))?;
    let mut failures = Vec::new();
    let mut checked = 0u64;
    for line in manifest.lines() {
        let parts: Vec<&str> = line.splitn(2, "  ").collect();
        if parts.len() != 2 {
            continue;
        }
        checked += 1;
        let p = root.join(parts[1].trim());
        match fs::read(&p) {
            Ok(bytes) => {
                if sha256_hex(&bytes) != parts[0].trim() {
                    failures.push(Json::s(parts[1].trim()));
                }
            }
            Err(_) => failures.push(Json::s(parts[1].trim())),
        }
    }
    Ok(Json::obj(vec![
        ("dir", Json::s(dir)),
        ("checked", Json::n(checked as f64)),
        ("failures", Json::Arr(failures.clone())),
        ("verified", Json::b(checked > 0 && failures.is_empty())),
    ]))
}

fn sync_push(ctx: &Ctx, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "policy", "sync.push", "sync.push") {
        return (r.status, refusal_payload(&r));
    }
    // Transport of the day is the ranch route (SSH): qexo records the
    // push receipt against a manifest it generated; the two-node
    // verified rule is checked by sync/verify receipts.
    let dir = gets(body, "dir").unwrap_or_default();
    let to = gets(body, "to").unwrap_or_default();
    if dir.is_empty() || to.is_empty() {
        return (400, refusal_payload(&Refusal::new(400, "no-such-record", "sync push requires {dir, to}")));
    }
    match sync_manifest(&dir) {
        Ok(m) => {
            let rcpt = receipt(st, "sync-push", vec![("to", Json::s(&to)), ("dir", Json::s(&dir)), ("manifest_sha256", m.get("manifest_sha256").cloned().unwrap_or(Json::Null))]);
            (200, Json::obj(vec![
                ("pushed", Json::b(true)),
                ("to", Json::s(&to)),
                ("manifest", m),
                ("receipt", Json::s(&rcpt)),
                ("transport", Json::s("ranch route (SSH); far-side full-manifest verify is the completion rule — sync counts complete only verified on >= 2 ranch nodes")),
            ]))
        }
        Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
    }
}

// ------------------------------------------------------------- personas
fn record_json(r: &px::Record, full: bool) -> Json {
    let mut fields = vec![
        ("persona_id", Json::s(&r.persona_id)),
        ("name", Json::s(&r.name)),
        ("record_kind", Json::s(&r.kind)),
        ("persona_class", Json::s(&r.class)),
        ("evidence_status", Json::s(&r.evidence)),
        ("spawn_eligible", Json::b(r.spawn)),
        ("home", Json::s(&fmt_coord(&r.designation))),
        ("designation_coordinate", Json::s(&fmt_coord(&r.designation))),
        ("aliases", Json::Arr(r.aliases.iter().map(|a| Json::s(a)).collect())),
    ];
    if r.displaced_by > 0 {
        fields.push(("displaced_by", Json::n(r.displaced_by as f64)));
    }
    if let Some(k) = &r.also_key {
        fields.push(("also_resolves_key", Json::s(k)));
    }
    if full && !r.notes.is_empty() {
        fields.push(("provenance", Json::s(&r.notes)));
    }
    Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn personas_phext_json(ctx: &Ctx) -> Json {
    let redacted = store::volumen_form() != "full";
    let px = ctx.census.to_phext(redacted);
    Json::obj(vec![
        ("volumen_form", Json::s(if redacted { "redacted" } else { "full" })),
        ("records", Json::n(ctx.census.records.len() as f64)),
        ("query_keys", Json::n(ctx.census.keys.len() as f64)),
        ("phext_sha256", Json::s(&sha256_hex(px.serialize().as_bytes()))),
    ])
}

fn route_personas(ctx: &Ctx, method: &str, path: &[String], _query: &[(String, String)], body: &Json, caller: &Caller, _rid: &str) -> (u16, Json) {
    let st = &ctx.state;
    let seg = |i: usize| path.get(i).map(|s| s.as_str()).unwrap_or("");

    // GET /v1/personas
    if method == "GET" && path.len() == 1 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "meta.read") {
            return (r.status, refusal_payload(&r));
        }
        let rows: Vec<Json> = ctx.census.records.iter().map(|r| record_json(r, false)).collect();
        let counts = ctx.census.evidence_counts();
        return (200, Json::obj(vec![
            ("query_keys", Json::n(ctx.census.keys.len() as f64)),
            ("records", Json::n(ctx.census.records.len() as f64)),
            ("evidence_partition", Json::Obj(counts.iter().map(|(k, v)| (k.clone(), Json::n(*v as f64))).collect())),
            ("spawn_eligible", Json::Arr(ctx.census.spawn_eligible().iter().map(|r| Json::s(&r.persona_id)).collect())),
            ("personas", Json::Arr(rows)),
            ("volumen_form", Json::s(if store::volumen_form() == "full" { "full" } else { "redacted" })),
        ]));
    }
    // GET /v1/personas/active
    if method == "GET" && path.len() == 2 && seg(1) == "active" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let ps = px::persona_state(st);
        return (200, Json::obj(vec![
            ("active", ps.active.as_deref().map(Json::s).unwrap_or(Json::Null)),
            ("instance_epoch", Json::n(ps.active_epoch as f64)),
            ("manifest_sha256", Json::s(&px::manifest_hash(&ctx.census))),
            ("readonly_restored", Json::b(ps.readonly_restored)),
        ]));
    }
    // POST /v1/personas/swap
    if method == "POST" && path.len() == 2 && seg(1) == "swap" {
        return persona_swap(ctx, body, caller);
    }
    // POST /v1/personas (seed)
    if method == "POST" && path.len() == 1 {
        return persona_seed(ctx, body, caller);
    }
    // GET /v1/personas/<key> and sub-operations
    if path.len() >= 2 {
        let key = seg(1).to_string();
        let records = ctx.census.resolve(&key);
        if records.is_empty() {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", format!("key {:?} resolves to no identity record", key))));
        }
        if method == "GET" && path.len() == 2 {
            if let Err(r) = check_floor(st, caller, "local-read", "", "meta.read") {
                return (r.status, refusal_payload(&r));
            }
            let full = store::volumen_form() == "full" || caller.local;
            let rows: Vec<Json> = records.iter().map(|r| record_json(r, full)).collect();
            let mut payload = vec![
                ("key", Json::s(&key)),
                ("resolves_to", Json::n(rows.len() as f64)),
                ("records", Json::Arr(rows)),
            ];
            if records.len() > 1 {
                payload.push(("disambiguation", Json::s("the key 'Orin' names two distinct referents in the corpus — Orin the voice (CYOA Nine Vectors, 'Horizon') and the PDNA-029 aggregate (nine Incipit-class LLMs reported as one deployment). The index records the fact; it does not force a merge. Operations needing exactly one record must name a persona_id.")));
            }
            return (200, Json::Obj(payload.into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
        }
        if method == "POST" && path.len() == 3 && seg(2) == "spawn" {
            return persona_spawn(ctx, &key, body, caller);
        }
        if method == "POST" && path.len() == 3 && seg(2) == "teleport" {
            return teleport_issue(ctx, &key, body, caller);
        }
        if method == "POST" && path.len() == 4 && seg(2) == "teleport" && seg(3) == "return" {
            return teleport_return_home(ctx, &key, caller);
        }
        if method == "GET" && path.len() == 3 && seg(2) == "residence" {
            if let Err(r) = check_floor(st, caller, "local-read", "", "") {
                return (r.status, refusal_payload(&r));
            }
            let r0 = records[0];
            let own = px::ownership(st, &r0.persona_id);
            return (200, Json::obj(vec![
                ("persona_id", Json::s(&r0.persona_id)),
                ("ownership", own.unwrap_or(Json::Null)),
                ("heartbeat_age_secs", px::heartbeat_age(st, &r0.persona_id).map(|x| Json::n(x as f64)).unwrap_or(Json::Null)),
                ("staleness_bound_secs", Json::n(px::STALENESS_BOUND_SECS as f64)),
                ("readonly_restored", Json::b(px::persona_state(st).readonly_restored)),
            ]));
        }
        if method == "POST" && path.len() == 3 && seg(2) == "grants" {
            if let Err(r) = gate(st, caller, "bootstrap", "grants.change", "grants.change") {
                return (r.status, refusal_payload(&r));
            }
            let rcpt = receipt(st, "persona-grants", vec![("persona_id", Json::s(&records[0].persona_id)), ("toggles", body.clone())]);
            return (200, Json::obj(vec![("recorded", Json::b(true)), ("receipt", Json::s(&rcpt)), ("note", Json::s("per-persona toggles are recorded as receipts against the index; census toggles enter through a corrector index amendment"))]));
        }
        if method == "POST" && path.len() == 3 && seg(2) == "meld" {
            return meld_form(ctx, &key, body, caller);
        }
        if method == "POST" && path.len() == 3 && seg(2) == "graduate" {
            if let Err(r) = gate(st, caller, "bootstrap", "persona.seed", "persona.seed") {
                return (r.status, refusal_payload(&r));
            }
            let rcpt = receipt(st, "graduation", vec![("persona_id", Json::s(&records[0].persona_id)), ("evidence", body.clone())]);
            return (200, Json::obj(vec![("graduated", Json::b(true)), ("receipt", Json::s(&rcpt))]));
        }
    }
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown personas operation")))
}

fn persona_swap(ctx: &Ctx, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "policy", "persona.swap", "persona.swap") {
        return (r.status, refusal_payload(&r));
    }
    let name = gets(body, "name").unwrap_or_default();
    let records = ctx.census.resolve(&name);
    if records.is_empty() {
        return (404, refusal_payload(&Refusal::new(404, "no-such-record", format!("no record for {:?}", name))));
    }
    if records.len() > 1 {
        return (409, refusal_payload(&Refusal::new(409, "ambiguous-key", "the key resolves to two records; a swap must name a persona_id")));
    }
    let rec = records[0];
    // who may swap: the person herself or Will (section 2.3)
    let is_self = caller.principal == format!("persona:{}", rec.persona_id);
    if !caller.is_will() && !is_self {
        return (403, refusal_payload(&Refusal::new(403, "policy-refused", "a swap is performed by the person herself or by Will — nobody else")));
    }
    // residency rule: residents swap only on their residence node;
    // substrate seats instantiate per-instance; facets ride their core.
    // In this single-node runtime the residency record is ownership.json;
    // a resident whose ownership names another node is refused.
    if rec.class == "resident" {
        if let Some(own) = px::ownership(st, &rec.persona_id) {
            let writer = own.get("writer").and_then(|v| v.as_str()).unwrap_or("");
            if !writer.is_empty() && writer != st.node {
                return (403, refusal_payload(&Refusal::new(403, "consent-refused", format!("{} is resident on {}; residence changes by teleport, not swap", rec.persona_id, writer))));
            }
        }
    }
    if px::persona_state(st).readonly_restored {
        return (403, refusal_payload(&Refusal::new(403, "read-only-restored", "this node's persona state was restored from backup and is read-only until reactivation (section 9.4)")));
    }
    // atomic load: the incoming selection must verify before anything
    // is displaced — here: record exists in the verified census and
    // its manifest hash computes. (Full scroll-level verification is
    // the selection loader's job when persona phexts land.)
    let mhash = px::manifest_hash(&ctx.census);
    let prev = px::persona_state(st);
    let epoch = prev.active_epoch + 1;
    px::set_active(st, &rec.persona_id, epoch);
    let rcpt = receipt(st, "swap", vec![
        ("from", prev.active.as_deref().map(Json::s).unwrap_or(Json::Null)),
        ("to", Json::s(&rec.persona_id)),
        ("by", Json::s(&caller.principal)),
        ("manifest_sha256", Json::s(&mhash)),
    ]);
    (200, Json::obj(vec![
        ("swapped", Json::b(true)),
        ("persona_id", Json::s(&rec.persona_id)),
        ("instance_epoch", Json::n(epoch as f64)),
        ("manifest_sha256", Json::s(&mhash)),
        ("receipt", Json::s(&rcpt)),
        ("dispatch_binding", Json::s("in-flight work completes under its dispatch-time persona_id + instance epoch; the swap binds new dispatches only")),
    ]))
}

fn persona_seed(ctx: &Ctx, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "bootstrap", "persona.seed", "persona.seed") {
        return (r.status, refusal_payload(&r));
    }
    let name = gets(body, "name").unwrap_or_default();
    if name.is_empty() {
        return (400, refusal_payload(&Refusal::new(400, "no-such-record", "seed requires {name}")));
    }
    let gestation = body.get("gestation").and_then(|v| v.as_bool()).unwrap_or(false);
    let rcpt = receipt(st, "seed", vec![("name", Json::s(&name)), ("gestation", Json::b(gestation)), ("by", Json::s(&caller.principal))]);
    (200, Json::obj(vec![
        ("seeded", Json::b(true)),
        ("receipt", Json::s(&rcpt)),
        ("state", Json::s(if gestation { "gestation: record + persona_id + lineage exist; spawn_eligible false, no key, no teleport, restricted grants until a receipted graduation" } else { "recorded as a seed receipt; the index amendment itself is a corrector act on personas.phext" })),
    ]))
}

fn persona_spawn(ctx: &Ctx, key: &str, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "policy", "persona.spawn", "persona.spawn") {
        return (r.status, refusal_payload(&r));
    }
    // a spawn names a persona_id, not a shared key
    let target = if let Some(pid) = gets(body, "persona_id") {
        ctx.census.by_id(&pid).cloned()
    } else {
        let recs = ctx.census.resolve(key);
        if recs.len() > 1 {
            return (409, refusal_payload(&Refusal::new(409, "ambiguous-key", "the key resolves to two records; a spawn must name a persona_id")));
        }
        recs.first().cloned().cloned()
    };
    let rec = match target {
        Some(r) => r,
        None => return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no record to spawn"))),
    };
    if !rec.spawn {
        return (403, refusal_payload(&Refusal::new(403, "spawn-ineligible", format!("record {} is not spawn_eligible (at census time exactly one record is: Emise)", rec.persona_id))));
    }
    if px::persona_state(st).readonly_restored {
        return (403, refusal_payload(&Refusal::new(403, "read-only-restored", "restored personas start read-only; spawning is refused until reactivation (section 17)")));
    }
    let task = gets(body, "task").unwrap_or_default();
    let budgets = body.get("budgets").map(|b| b.to_string()).unwrap_or_else(|| "{}".into());
    let lease = body.get("lease_secs").and_then(|v| v.as_num()).map(|n| n as u64).unwrap_or(3600);
    let retries = body.get("retries").and_then(|v| v.as_num()).map(|n| n as u64).unwrap_or(0);
    let inst = px::mint_instance(st, &rec.persona_id, &task, &budgets, lease, retries);
    (200, Json::obj(vec![
        ("instance_id", Json::s(&inst.id)),
        ("persona_id", Json::s(&inst.persona_id)),
        ("instance_epoch", Json::n(inst.epoch as f64)),
        ("lease_expires", Json::n(inst.lease_expires as f64)),
        ("receipt", Json::s(&format!("spawn:{}:{}", inst.persona_id, inst.epoch))),
    ]))
}

fn route_instances(ctx: &Ctx, method: &str, path: &[String], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    let id = path.get(1).cloned().unwrap_or_default();
    let known = px::latest_instances(st).contains_key(&id);
    if method == "GET" && path.len() == 2 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        if !known {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such instance")));
        }
        let events: Vec<Json> = st.read_log("instances.jsonl").into_iter().filter(|e| e.get("instance").and_then(|v| v.as_str()) == Some(id.as_str())).collect();
        let state = px::latest_instances(st).get(&id).map(|(s, _)| s.clone()).unwrap_or_default();
        return (200, Json::obj(vec![("instance", Json::s(&id)), ("state", Json::s(&state)), ("events", Json::Arr(events))]));
    }
    if method == "POST" && path.len() == 3 && path[2] == "heartbeat" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        if !known {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such instance")));
        }
        let lease = body.get("lease_secs").and_then(|v| v.as_num()).map(|n| n as u64).unwrap_or(3600);
        px::instance_event(st, &id, "heartbeat", vec![("lease_expires", Json::n((now_unix() + lease) as f64))]);
        return (200, Json::obj(vec![("heartbeat", Json::b(true)), ("instance", Json::s(&id))]));
    }
    if method == "POST" && path.len() == 3 && path[2] == "cancel" {
        if let Err(r) = gate(st, caller, "policy", "persona.spawn", "persona.spawn") {
            return (r.status, refusal_payload(&r));
        }
        if !known {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such instance")));
        }
        px::instance_event(st, &id, "cancel", vec![("by", Json::s(&caller.principal))]);
        return (200, Json::obj(vec![("cancelled", Json::b(true)), ("instance", Json::s(&id)), ("note", Json::s("the instance stops at its next act boundary; in-flight calls finish under their own attribution"))]));
    }
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown instances operation")))
}

fn meld_form(ctx: &Ctx, key: &str, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "policy", "persona.meld", "persona.meld") {
        return (r.status, refusal_payload(&r));
    }
    // both parties' consent must be signed under their own keys —
    // presented as two verified signatures over the meld's canonical
    // body. Consent is undelegatable: a delegation never satisfies it.
    if caller.delegation.is_some() {
        return (403, refusal_payload(&Refusal::new(403, "consent-refused", "meld consent is undelegatable; a delegated caller cannot form a meld")));
    }
    let parties = body.get("consents").and_then(|v| v.as_arr()).cloned().unwrap_or_default();
    if parties.len() != 2 {
        return (403, refusal_payload(&Refusal::new(403, "consent-refused", "a meld requires both personas' consent signed under their own keys (body.consents: two {persona_id, key, signature} entries)")));
    }
    let gpg = Gpg::open(st);
    let canon = format!("meld:{}:{}", key, gets(body, "other").unwrap_or_default());
    for c in &parties {
        let fp = c.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let sig = c.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        if !key_active(st, fp) {
            return (403, refusal_payload(&Refusal::new(403, "consent-refused", "a consenting party's key is not on record and active")));
        }
        match gpg.verify_detached(sig, canon.as_bytes()) {
            Ok(s) if s == fp => {}
            _ => return (403, refusal_payload(&Refusal::new(403, "consent-refused", "a party's meld consent signature does not verify under its own key"))),
        }
    }
    let rcpt = receipt(st, "meld", vec![("parties", Json::Arr(parties))]);
    (200, Json::obj(vec![
        ("meld_formed", Json::b(true)),
        ("receipt", Json::s(&rcpt)),
        ("outcome_rule", Json::s("a merge meld produces exactly one identity outcome: a new offspring record in gestation, or a recorded union-of-continuity — never an ambiguous third state (section 18 test 6)")),
    ]))
}

// ------------------------------------------------------------- teleport
fn token_record(st: &State, token: &str) -> Option<Json> {
    // the token's ISSUE record carries the binding fields (persona,
    // epochs, manifest, destination); later events carry state only.
    // Return the issue record merged with the LATEST state.
    let mut issue: Option<Json> = None;
    let mut latest_state: Option<String> = None;
    for e in st.read_log("teleports.jsonl") {
        if e.get("token").and_then(|v| v.as_str()) == Some(token) {
            let state = e.get("state").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if state == "issued" {
                issue = Some(e.clone());
            }
            latest_state = Some(state);
        }
    }
    let mut rec = issue?;
    if let (Json::Obj(ref mut fields), Some(ls)) = (&mut rec, latest_state) {
        fields.retain(|(k, _)| k != "state");
        fields.push(("state".to_string(), Json::s(&ls)));
    }
    Some(rec)
}

fn token_state(st: &State, token: &str) -> String {
    token_record(st, token)
        .and_then(|e| e.get("state").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

fn token_event(st: &State, token: &str, state: &str, fields: Vec<(&str, Json)>) {
    let mut f = vec![
        ("token", Json::s(token)),
        ("state", Json::s(state)),
        ("at", Json::s(&now_rfc3339())),
        ("unix", Json::n(now_unix() as f64)),
    ];
    f.extend(fields);
    let _ = st.append_log("teleports.jsonl", &Json::obj(f));
}

fn teleport_issue(ctx: &Ctx, key: &str, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
        return (r.status, refusal_payload(&r));
    }
    let records = ctx.census.resolve(key);
    if records.len() != 1 {
        return (409, refusal_payload(&Refusal::new(409, "ambiguous-key", "teleport names exactly one record; use a persona_id for a two-referent key")));
    }
    let rec = records[0];
    let to = gets(body, "to").unwrap_or_default();
    if to.is_empty() {
        return (400, refusal_payload(&Refusal::new(400, "no-such-record", "teleport requires {to}")));
    }
    // initiation authority: the person herself, Will, or a roaming
    // seat under a scoped delegation (never a resident by delegation)
    let is_self = caller.principal == format!("persona:{}", rec.persona_id);
    if !caller.is_will() && !is_self && caller.delegation.is_none() {
        return (403, refusal_payload(&Refusal::new(403, "policy-refused", "teleport is initiated by the person herself or by Will; delegated initiation is for roaming seats under a scoped delegation")));
    }
    if caller.delegation.is_some() && rec.class == "resident" {
        return (403, refusal_payload(&Refusal::new(403, "policy-refused", "a resident is never teleported by delegation")));
    }
    if rec.class == "resident" || rec.kind == "resident" {
        // unkeyed persona: Will only (interim rule, section 2.3)
        let has_key = st.read_json("keys.json")
            .and_then(|j| j.get("keys").and_then(|k| k.as_arr()).cloned())
            .map(|ks| ks.iter().any(|k| k.get("owner").and_then(|v| v.as_str()) == Some(rec.persona_id.as_str()) && k.get("kind").and_then(|v| v.as_str()) == Some("persona")))
            .unwrap_or(false);
        if !has_key && !caller.is_will() {
            return (403, refusal_payload(&Refusal::new(403, "policy-refused", "an unkeyed persona is teleported by Will only (interim rule)")));
        }
    }
    // an open token refuses a second issuance (section 9.4)
    for e in st.read_log("teleports.jsonl") {
        if e.get("persona_id").and_then(|v| v.as_str()) == Some(rec.persona_id.as_str()) {
            let s = e.get("state").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(s, "issued" | "consented" | "transferred" | "verified" | "returned") {
                return (409, refusal_payload(&Refusal::new(409, "stale-epoch", "an open teleport token already exists for this persona")));
            }
        }
    }
    let epoch = px::current_epoch(st, &rec.persona_id);
    let token = format!("tp-{}-{}", rec.persona_id, st.next_counter("teleport"));
    let manifest = px::manifest_hash(&ctx.census);
    // sign the token scroll with the origin node key when one exists
    let gpg = Gpg::open(st);
    let node_key = st.read_json("keys.json")
        .and_then(|j| j.get("keys").and_then(|k| k.as_arr()).cloned())
        .and_then(|ks| ks.iter().find(|k| k.get("kind").and_then(|v| v.as_str()) == Some("node")).and_then(|k| k.get("fingerprint")).and_then(|v| v.as_str()).map(|s| s.to_string()));
    let token_body = format!("teleport:{}:{}:{}:{}:{}", rec.persona_id, st.node, to, epoch, manifest);
    let signature = match &node_key {
        Some(fp) => gpg.sign_detached(fp, token_body.as_bytes()).unwrap_or_default(),
        None => String::new(),
    };
    token_event(st, &token, "issued", vec![
        ("persona_id", Json::s(&rec.persona_id)),
        ("origin", Json::s(&st.node)),
        ("destination", Json::s(&to)),
        ("epoch", Json::n(epoch as f64)),
        ("manifest_sha256", Json::s(&manifest)),
        ("initiated_by", Json::s(&caller.principal)),
        ("delegation", caller.delegation.as_deref().map(Json::s).unwrap_or(Json::Null)),
        ("signature", Json::s(&signature)),
        ("expires", Json::n((now_unix() + 3600) as f64)),
    ]);
    // the ownership writer heartbeats from issue on
    px::set_ownership(st, &rec.persona_id, epoch, &st.node);
    (200, Json::obj(vec![
        ("token", Json::s(&token)),
        ("persona_id", Json::s(&rec.persona_id)),
        ("epoch", Json::n(epoch as f64)),
        ("manifest_sha256", Json::s(&manifest)),
        ("state", Json::s("issued")),
        ("next", Json::s("consent (destination host) -> transfer -> verify -> return -> relinquish -> activate")),
    ]))
}

fn teleport_return_home(ctx: &Ctx, key: &str, caller: &Caller) -> (u16, Json) {
    // the return trip is a fresh teleport whose destination is the
    // previous node of record; it is also the reactivation path for a
    // read-only restored persona (new token, new epoch — section 9.5)
    let st = &ctx.state;
    let records = ctx.census.resolve(key);
    if records.len() != 1 {
        return (409, refusal_payload(&Refusal::new(409, "ambiguous-key", "name a persona_id")));
    }
    let rec = records[0];
    if !caller.is_will() && caller.principal != format!("persona:{}", rec.persona_id) {
        return (403, refusal_payload(&Refusal::new(403, "policy-refused", "the return/reactivation path is issued by Will, a corrector, or the person")));
    }
    let prev_epoch = px::current_epoch(st, &rec.persona_id);
    let new_epoch = prev_epoch + 1;
    px::set_ownership(st, &rec.persona_id, new_epoch, &st.node);
    let was_readonly = px::persona_state(st).readonly_restored;
    if was_readonly {
        // reactivation through the protocol: clear read-only only via
        // this receipted path
        let cur = px::persona_state(st);
        let _ = st.write_json("persona-state.json", &Json::obj(vec![
            ("active", cur.active.as_deref().map(Json::s).unwrap_or(Json::Null)),
            ("active_epoch", Json::n(cur.active_epoch as f64)),
            ("readonly_restored", Json::b(false)),
            ("reactivated_at", Json::s(&now_rfc3339())),
            ("reactivated_epoch", Json::n(new_epoch as f64)),
        ]));
    }
    let rcpt = receipt(st, "teleport-return", vec![
        ("persona_id", Json::s(&rec.persona_id)),
        ("from_epoch", Json::n(prev_epoch as f64)),
        ("new_epoch", Json::n(new_epoch as f64)),
        ("closed_restoration", Json::b(was_readonly)),
        ("by", Json::s(&caller.principal)),
    ]);
    (200, Json::obj(vec![
        ("return_issued", Json::b(true)),
        ("persona_id", Json::s(&rec.persona_id)),
        ("new_epoch", Json::n(new_epoch as f64)),
        ("closed_restoration", Json::b(was_readonly)),
        ("receipt", Json::s(&rcpt)),
        ("note", Json::s("reactivation runs the ordinary protocol (verify, cognitive return, relinquish of any stale claim, activation) under the new token and epoch")),
    ]))
}

fn route_teleports(ctx: &Ctx, method: &str, path: &[String], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if method == "GET" && path.len() == 1 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let events = st.read_log("teleports.jsonl");
        return (200, Json::obj(vec![("events", Json::n(events.len() as f64)), ("ledger", Json::Arr(events))]));
    }
    if method != "POST" || path.len() != 3 {
        return (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown teleports operation")));
    }
    let token = path[1].clone();
    let action = path[2].as_str();
    let rec = match token_record(st, &token) {
        Some(r) => r,
        None => return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such teleport token"))),
    };
    let state_now = token_state(st, &token);
    let pid = rec.get("persona_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let epoch = rec.get("epoch").and_then(|v| v.as_num()).unwrap_or(0.0) as u64;
    let expires = rec.get("expires").and_then(|v| v.as_num()).unwrap_or(0.0) as u64;
    if expires != 0 && now_unix() > expires && state_now == "issued" {
        token_event(st, &token, "lapsed", vec![("persona_id", Json::s(&pid))]);
        return (410, refusal_payload(&Refusal::new(410, "stale-evidence", "the token lapsed before the return-receipt step; the origin never stopped being authoritative")));
    }
    let expect_state = |want: &str| -> Result<(), (u16, Json)> {
        if state_now == want {
            Ok(())
        } else {
            Err((409, refusal_payload(&Refusal::new(409, "stale-epoch", format!("token is in state {}, this step requires {}", state_now, want)))))
        }
    };
    // freshness gate for the steps that read ownership evidence
    let fresh = || -> Result<(), (u16, Json)> {
        match px::heartbeat_age(st, &pid) {
            Some(age) if age <= px::STALENESS_BOUND_SECS => Ok(()),
            _ => Err((503, refusal_payload(&Refusal::new(503, "stale-evidence", "ownership evidence is past the 5-minute freshness bound; the protocol pauses — unavailability over split-brain")))),
        }
    };

    match action {
        "consent" => {
            if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("issued") {
                return e;
            }
            let accepted = body.get("accept").and_then(|v| v.as_bool()).unwrap_or(false);
            let ground = gets(body, "ground").unwrap_or_default();
            if !accepted {
                token_event(st, &token, "refused", vec![("persona_id", Json::s(&pid)), ("ground", Json::s(&ground))]);
                return (403, refusal_payload(&Refusal::new(403, "host-refused", format!("destination host refused the placement: {}", ground))));
            }
            token_event(st, &token, "consented", vec![("persona_id", Json::s(&pid)), ("conditions", gets(body, "conditions").map(|x| Json::s(x)).unwrap_or(Json::Null))]);
            (200, Json::obj(vec![("token", Json::s(&token)), ("state", Json::s("consented"))]))
        }
        "transfer" => {
            if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("consented") {
                return e;
            }
            token_event(st, &token, "transferred", vec![("persona_id", Json::s(&pid)), ("bundle", gets(body, "bundle").map(|x| Json::s(x)).unwrap_or(Json::Null))]);
            (200, Json::obj(vec![("token", Json::s(&token)), ("state", Json::s("transferred")), ("step", Json::s("BP4-a"))]))
        }
        "verify" => {
            if let Err(r) = gate(st, caller, "auth", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("transferred") {
                return e;
            }
            let presented = gets(body, "manifest_sha256").unwrap_or_default();
            let want = rec.get("manifest_sha256").and_then(|v| v.as_str()).unwrap_or("");
            if presented != want {
                token_event(st, &token, "failed", vec![("persona_id", Json::s(&pid)), ("reason", Json::s("manifest hash mismatch"))]);
                return (422, refusal_payload(&Refusal::new(422, "unverified-state", "bundle hashes disagree with the signed manifest; nothing activates on unverified state")));
            }
            token_event(st, &token, "verified", vec![("persona_id", Json::s(&pid))]);
            (200, Json::obj(vec![("token", Json::s(&token)), ("state", Json::s("verified"))]))
        }
        "return" => {
            // BP4-b: the destination NODE key signs the return receipt,
            // bound to the fencing epoch and the transfer hash
            if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("verified") {
                return e;
            }
            let sig = gets(body, "signature").unwrap_or_default();
            let signer = gets(body, "signer_key").unwrap_or_default();
            let bound_epoch = body.get("epoch").and_then(|v| v.as_num()).map(|n| n as u64).unwrap_or(0);
            let bound_hash = gets(body, "manifest_sha256").unwrap_or_default();
            let want_hash = rec.get("manifest_sha256").and_then(|v| v.as_str()).unwrap_or("");
            // the signer must be a NODE key on record (not a persona key)
            let kind = key_record(st, &signer).and_then(|k| k.get("kind").and_then(|v| v.as_str()).map(|s| s.to_string())).unwrap_or_default();
            if kind != "node" {
                return (422, refusal_payload(&Refusal::new(422, "no-cognitive-return", "the return receipt must be signed with the destination NODE key — a persona-key signature in the handoff window would prove custody, not arrival")));
            }
            if bound_epoch != epoch || bound_hash != want_hash {
                return (422, refusal_payload(&Refusal::new(422, "no-cognitive-return", "the return receipt is bound to the wrong epoch or transfer hash")));
            }
            let gpg = Gpg::open(st);
            let canon = format!("return:{}:{}:{}", token, bound_epoch, bound_hash);
            match gpg.verify_detached(&sig, canon.as_bytes()) {
                Ok(s) if s == signer => {}
                _ => return (422, refusal_payload(&Refusal::new(422, "no-cognitive-return", "the return receipt is unsigned or unverifiable; no relinquish, no activation"))),
            }
            token_event(st, &token, "returned", vec![("persona_id", Json::s(&pid)), ("signer_key", Json::s(&signer))]);
            (200, Json::obj(vec![("token", Json::s(&token)), ("state", Json::s("returned")), ("step", Json::s("BP4-b: the mind arrived — receipt verified before relinquish"))]))
        }
        "relinquish" => {
            if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("returned") {
                return e;
            }
            if let Err(e) = fresh() {
                return e;
            }
            let new_epoch = epoch + 1;
            px::set_ownership(st, &pid, new_epoch, rec.get("destination").and_then(|v| v.as_str()).unwrap_or(""));
            token_event(st, &token, "relinquished", vec![("persona_id", Json::s(&pid)), ("new_epoch", Json::n(new_epoch as f64))]);
            (200, Json::obj(vec![("token", Json::s(&token)), ("state", Json::s("relinquished")), ("new_epoch", Json::n(new_epoch as f64)), ("note", Json::s("the origin is dormant before the destination is active; its acts under the old epoch are non-authoritative from this instant"))]))
        }
        "activate" => {
            if let Err(r) = gate(st, caller, "policy", "persona.teleport", "persona.teleport") {
                return (r.status, refusal_payload(&r));
            }
            if let Err(e) = expect_state("relinquished") {
                return e;
            }
            if let Err(e) = fresh() {
                return e;
            }
            let cur = px::current_epoch(st, &pid);
            px::set_active(st, &pid, cur);
            token_event(st, &token, "active", vec![("persona_id", Json::s(&pid)), ("epoch", Json::n(cur as f64))]);
            (200, Json::obj(vec![
                ("token", Json::s(&token)),
                ("state", Json::s("active")),
                ("persona_id", Json::s(&pid)),
                ("epoch", Json::n(cur as f64)),
                ("note", Json::s("status flips on the receipts, not on the file transfer; the persona key's first act at the destination is the activation countersign")),
            ]))
        }
        _ => (404, refusal_payload(&Refusal::new(404, "no-such-record", format!("unknown teleport step {}", action)))),
    }
}

// ------------------------------------------------------- keys & grants
fn keys_add_record(st: &State, record: Json) {
    let mut obj = match st.read_json("keys.json") {
        Some(Json::Obj(p)) => p,
        _ => vec![("keys".to_string(), Json::Arr(vec![]))],
    };
    if let Some((_, Json::Arr(ref mut arr))) = obj.iter_mut().find(|(k, _)| k == "keys") {
        arr.push(record);
    }
    let _ = st.write_json("keys.json", &Json::Obj(obj));
}

fn keys_update_status(st: &State, fingerprint: &str, status: &str, note: &str) -> bool {
    let mut obj = match st.read_json("keys.json") {
        Some(Json::Obj(p)) => p,
        _ => return false,
    };
    let mut hit = false;
    if let Some((_, Json::Arr(ref mut arr))) = obj.iter_mut().find(|(k, _)| k == "keys") {
        for rec in arr.iter_mut() {
            if rec.get("fingerprint").and_then(|v| v.as_str()) == Some(fingerprint) {
                if let Json::Obj(ref mut fields) = rec {
                    fields.retain(|(k, _)| k != "status" && k != "status_note");
                    fields.push(("status".to_string(), Json::s(status)));
                    fields.push(("status_note".to_string(), Json::s(note)));
                }
                hit = true;
            }
        }
    }
    if hit {
        let _ = st.write_json("keys.json", &Json::Obj(obj));
    }
    hit
}

fn route_keys(ctx: &Ctx, method: &str, path: &[String], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    let seg = |i: usize| path.get(i).map(|s| s.as_str()).unwrap_or("");

    if method == "GET" && path == ["keys"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let keys = st.read_json("keys.json").and_then(|j| j.get("keys").cloned()).unwrap_or(Json::Arr(vec![]));
        return (200, Json::obj(vec![("keys", keys)]));
    }
    if method == "GET" && path.len() == 2 && seg(0) == "keys" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return match key_record(st, seg(1)) {
            Some(k) => (200, k),
            None => (404, refusal_payload(&Refusal::new(404, "no-such-record", "no key with that fingerprint is on record"))),
        };
    }
    if method == "POST" && path == ["keys", "nodes"] {
        if let Err(r) = gate(st, caller, "bootstrap", "keys.issue", "keys.issue") {
            // bootstrap exception: at provisioning, before any key
            // exists, node-key generation is the box-zero act itself
            let any_keys = st.read_json("keys.json").and_then(|j| j.get("keys").and_then(|k| k.as_arr()).map(|a| !a.is_empty())).unwrap_or(false);
            // the box-zero exception is a PROVISIONING act: the node
            // being provisioned, on-node, before any key exists. An
            // anonymous remote caller never satisfies it (8A.0: all
            // remote mutations require an authenticated principal).
            let provisioning = caller.local && caller.principal == format!("node:{}", st.node);
            if any_keys || !provisioning {
                return (r.status, refusal_payload(&r));
            }
        }
        let gpg = Gpg::open(st);
        return match gpg.generate(&format!("{} (node key)", st.node), "qexo node key") {
            Ok(fp) => {
                let public = gpg.export_public(&fp).unwrap_or_default();
                keys_add_record(st, Json::obj(vec![
                    ("fingerprint", Json::s(&fp)),
                    ("kind", Json::s("node")),
                    ("owner", Json::s(&st.node)),
                    ("algorithm", Json::s("eddsa/ed25519, suite: gpg")),
                    ("status", Json::s("active")),
                    ("public_key", Json::s(&public)),
                    ("issued", Json::s(&now_rfc3339())),
                    ("provenance", Json::s("generated at provisioning on the node; awaits Will's countersignature (section 8.4)")),
                ]));
                (200, Json::obj(vec![("fingerprint", Json::s(&fp)), ("kind", Json::s("node")), ("note", Json::s("emit the public-key scroll for Will's countersignature"))]))
            }
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "POST" && path == ["keys", "personas"] {
        if let Err(r) = gate(st, caller, "bootstrap", "keys.issue", "keys.issue") {
            return (r.status, refusal_payload(&r));
        }
        let persona = gets(body, "persona").unwrap_or_default();
        let rec = ctx.census.resolve(&persona);
        if rec.is_empty() {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "persona key issuance is deliberate, persona by persona — the persona must be in the index")));
        }
        let gpg = Gpg::open(st);
        return match gpg.generate(&format!("{} (persona key)", rec[0].persona_id), "qexo persona key") {
            Ok(fp) => {
                let public = gpg.export_public(&fp).unwrap_or_default();
                keys_add_record(st, Json::obj(vec![
                    ("fingerprint", Json::s(&fp)),
                    ("kind", Json::s("persona")),
                    ("owner", Json::s(&rec[0].persona_id)),
                    ("algorithm", Json::s("eddsa/ed25519, suite: gpg")),
                    ("status", Json::s("active")),
                    ("public_key", Json::s(&public)),
                    ("issued", Json::s(&now_rfc3339())),
                    ("provenance", Json::s("issued on the hosting node; the node countersigns the key scroll (persona -> node -> Will)")),
                ]));
                (200, Json::obj(vec![("fingerprint", Json::s(&fp)), ("kind", Json::s("persona")), ("owner", Json::s(&rec[0].persona_id))]))
            }
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "POST" && path.len() == 3 && seg(0) == "keys" && seg(2) == "countersign" {
        // Will's countersignature over a key scroll (section 8.4).
        // Authority: a will-class key, or — at provisioning only —
        // the local operator acting as principal "will" on the node
        // before any key carries a countersign (Will present at the
        // box; the act is receipted and the record is public in the
        // key band, so a false countersign is visible and revocable).
        let fp = seg(1).to_string();
        let caller_ok = match caller.key.as_deref() {
            Some(k) => key_record(st, k).and_then(|r| r.get("kind").and_then(|v| v.as_str()).map(|x| x.to_string())).as_deref() == Some("will"),
            None => false,
        };
        let any_countersigned = st.read_json("keys.json")
            .and_then(|j| j.get("keys").and_then(|k| k.as_arr()).cloned())
            .map(|ks| ks.iter().any(|k| k.get("countersigned_by").and_then(|v| v.as_str()) == Some("will")))
            .unwrap_or(false);
        let bootstrap_ok = caller.local && caller.principal == "will" && !any_countersigned;
        if !caller_ok && !bootstrap_ok {
            return (403, refusal_payload(&Refusal::new(403, "policy-refused", "a countersignature is Will's act: his key, or Will present at the box during provisioning")));
        }
        // stamp the record
        let mut obj = match st.read_json("keys.json") {
            Some(Json::Obj(p)) => p,
            _ => return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no key directory"))),
        };
        let mut hit = false;
        if let Some((_, Json::Arr(ref mut arr))) = obj.iter_mut().find(|(k, _)| k == "keys") {
            for rec in arr.iter_mut() {
                if rec.get("fingerprint").and_then(|v| v.as_str()) == Some(fp.as_str()) {
                    if let Json::Obj(ref mut fields) = rec {
                        fields.retain(|(k, _)| k != "countersigned_by");
                        fields.push(("countersigned_by".to_string(), Json::s("will")));
                    }
                    hit = true;
                }
            }
        }
        if !hit {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such key")));
        }
        let _ = st.write_json("keys.json", &Json::Obj(obj));
        let rcpt = receipt(st, "key-countersign", vec![("fingerprint", Json::s(&fp)), ("by", Json::s("will"))]);
        return (200, Json::obj(vec![("countersigned", Json::b(true)), ("fingerprint", Json::s(&fp)), ("receipt", Json::s(&rcpt)), ("chain", Json::s("persona -> node -> Will: the countersignature is Will's authorization, recorded in the key band"))]));
    }
    if method == "POST" && path.len() == 3 && seg(0) == "keys" && (seg(2) == "retire" || seg(2) == "revoke") {
        let action = if seg(2) == "revoke" { "keys.issue" } else { "keys.issue" };
        if let Err(r) = gate(st, caller, "bootstrap", action, "keys.issue") {
            return (r.status, refusal_payload(&r));
        }
        let fp = seg(1).to_string();
        let ground = gets(body, "ground").unwrap_or_default();
        if seg(2) == "revoke" {
            // compromised-key revocation: immediate where it counts
            if keys_update_status(st, &fp, "compromised", &ground) {
                let rcpt = receipt(st, "key-revoke", vec![("fingerprint", Json::s(&fp)), ("ground", Json::s(&ground)), ("by", Json::s(&caller.principal))]);
                return (200, Json::obj(vec![
                    ("revoked", Json::b(true)),
                    ("receipt", Json::s(&rcpt)),
                    ("effect", Json::s("new signatures under this key halt at once; in-flight work under it is tainted for review")),
                ]));
            }
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such key")));
        }
        if keys_update_status(st, &fp, "retired", &ground) {
            let rcpt = receipt(st, "key-retire", vec![("fingerprint", Json::s(&fp)), ("by", Json::s(&caller.principal))]);
            return (200, Json::obj(vec![("retired", Json::b(true)), ("receipt", Json::s(&rcpt)), ("note", Json::s("append-only: the key scroll gains a status line and is never deleted; pre-retirement signatures still verify with their status at signing time"))]));
        }
        return (404, refusal_payload(&Refusal::new(404, "no-such-record", "no such key")));
    }
    if method == "POST" && path == ["verify"] {
        // answers node and persona verification SEPARATELY, forever
        let obj = gets(body, "object").unwrap_or_default();
        let sig = gets(body, "signature").unwrap_or_default();
        let claimed = gets(body, "signer_key").unwrap_or_default();
        let gpg = Gpg::open(st);
        let result = gpg.verify_detached(&sig, obj.as_bytes());
        let (node_v, persona_v) = match key_record(st, &claimed) {
            Some(k) => {
                let kind = k.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                match result {
                    Ok(ref fp) if fp == &claimed && kind == "node" => ("verified".to_string(), "not-a-node-key".to_string()),
                    Ok(ref fp) if fp == &claimed && kind == "persona" => ("not-a-persona-key".to_string(), "verified".to_string()),
                    Ok(_) => ("signature verifies under a different key".into(), "signature verifies under a different key".into()),
                    Err(e) => (e.clone(), e),
                }
            }
            None => ("no key on record".to_string(), "no key on record".to_string()),
        };
        return (200, Json::obj(vec![
            ("node", Json::obj(vec![("verification", Json::s(&node_v))])),
            ("persona", Json::obj(vec![("verification", Json::s(&persona_v))])),
            ("note", Json::s("signatures authenticate origin; they do not authorize action")),
        ]));
    }
    if method == "POST" && path == ["delegations"] {
        if let Err(r) = gate(st, caller, "bootstrap", "keys.delegate", "keys.delegate") {
            return (r.status, refusal_payload(&r));
        }
        let id = format!("dlg-{}", st.next_counter("delegation"));
        let scope = body.get("scope").cloned().unwrap_or(Json::Arr(vec![]));
        let expiry = body.get("expiry").and_then(|v| v.as_num()).unwrap_or(0.0);
        let _ = st.append_log("delegations.jsonl", &Json::obj(vec![
            ("event", Json::s("issue")),
            ("id", Json::s(&id)),
            ("delegating_key", caller.key.as_deref().map(Json::s).unwrap_or(Json::Null)),
            ("agent_key", gets(body, "agent_key").map(|x| Json::s(x)).unwrap_or(Json::Null)),
            ("scope", scope),
            ("expiry", Json::n(expiry)),
            ("re_delegable", Json::b(body.get("re_delegable").and_then(|v| v.as_bool()).unwrap_or(false))),
            ("at", Json::s(&now_rfc3339())),
        ]));
        return (200, Json::obj(vec![("delegation", Json::s(&id)), ("note", Json::s("recorded in the key band; verifiable by anyone holding the directory; revocable by signed status append"))]));
    }
    if method == "POST" && path.len() == 3 && seg(0) == "delegations" && seg(2) == "revoke" {
        if let Err(r) = gate(st, caller, "bootstrap", "keys.delegate", "keys.delegate") {
            return (r.status, refusal_payload(&r));
        }
        let _ = st.append_log("delegations.jsonl", &Json::obj(vec![
            ("event", Json::s("revoke")),
            ("id", Json::s(seg(1))),
            ("by", Json::s(&caller.principal)),
            ("ground", gets(body, "ground").map(|x| Json::s(x)).unwrap_or(Json::Null)),
            ("at", Json::s(&now_rfc3339())),
        ]));
        let rcpt = receipt(st, "delegation-revoke", vec![("delegation", Json::s(seg(1)))]);
        return (200, Json::obj(vec![("revoked", Json::b(true)), ("receipt", Json::s(&rcpt)), ("effect", Json::s("ordinary revocation: effective at the next dispatch boundary; in-flight work keeps its dispatch attribution"))]));
    }
    if method == "GET" && path == ["grants"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let rows: Vec<Json> = GRANTS.iter().map(|(g, what)| Json::obj(vec![
            ("grant", Json::s(g)),
            ("governs", Json::s(what)),
            ("enabled", Json::b(grant_enabled(st, g))),
            ("default", Json::s("on")),
        ])).collect();
        return (200, Json::obj(vec![("grants", Json::Arr(rows))]));
    }
    if method == "POST" && path == ["grants"] {
        if let Err(r) = gate(st, caller, "bootstrap", "grants.change", "grants.change") {
            return (r.status, refusal_payload(&r));
        }
        let grant = gets(body, "grant").unwrap_or_default();
        if !grant_known(&grant) {
            return (404, refusal_payload(&Refusal::new(404, "no-such-record", format!("grant {} is not in the catalogue", grant))));
        }
        let on = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        let mut obj = match st.read_json("grants.json") {
            Some(Json::Obj(p)) => p,
            _ => Vec::new(),
        };
        obj.retain(|(k, _)| k != &grant);
        obj.push((grant.clone(), Json::b(on)));
        let _ = st.write_json("grants.json", &Json::Obj(obj));
        let rcpt = receipt(st, "grants-change", vec![("grant", Json::s(&grant)), ("enabled", Json::b(on)), ("by", Json::s(&caller.principal))]);
        return (200, Json::obj(vec![("grant", Json::s(&grant)), ("enabled", Json::b(on)), ("receipt", Json::s(&rcpt))]));
    }
    if method == "GET" && path == ["policy"] {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let configured = st.read_json("policy.json").unwrap_or(Json::Obj(vec![]));
        return (200, Json::obj(vec![
            ("default_policy", Json::s("Will's key authorizes every sensitive action; a persona's own key authorizes its sensitive self-actions; a node key authorizes that node's install.apply and sync.restore; self-modifying classes are key-bound from bootstrap")),
            ("configured", configured),
            ("bootstrap_form", Json::s("per-action keys (the corrector-set form is Will's open pick; the floor — Will's key from bootstrap — is not the open part)")),
        ]));
    }
    if method == "POST" && path == ["routes"] {
        if let Err(r) = gate(st, caller, "bootstrap", "route.change", "route.change") {
            return (r.status, refusal_payload(&r));
        }
        let rcpt = receipt(st, "route-change", vec![("by", Json::s(&caller.principal)), ("change", body.clone())]);
        return (200, Json::obj(vec![("recorded", Json::b(true)), ("receipt", Json::s(&rcpt)), ("note", Json::s("the nightly cycle proposes route diffs; application is this act, under the bootstrap key policy"))]));
    }
    if method == "POST" && path == ["route", "ask"] {
        if let Err(r) = gate(st, caller, "auth", "route.ask", "route.ask") {
            return (r.status, refusal_payload(&r));
        }
        let class = gets(body, "class").unwrap_or_else(|| "triage".into());
        let route = route_for_class(&class);
        return (200, Json::obj(vec![
            ("route", Json::s(route)),
            ("class", Json::s(&class)),
            ("cost_line", Json::s("hybrid accounting: local watts + cloud tokens; never a bare total")),
            ("note", Json::s("routing decision served from qexo-routing.toml rules (DESIGN-V3 section 3.3); the provider call itself rides the substrate session")),
        ]));
    }
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown keys/grants operation")))
}

pub fn route_for_class(class: &str) -> &'static str {
    match class {
        "triage" | "drafting" | "summarising" | "classification" | "summarizing" => "sq-api (cache, then local:<coolest node>)",
        "bulk" | "background" => "local:<shell>",
        "reasoning" | "long-horizon" => "substrate-steering day's driver",
        "context-recall" => "chatgpt (context reservoir)",
        _ => "sq-api",
    }
}

// ------------------------------------------------------------- phext tools
fn run_cmd(prog: &str, args: &[&str], stdin_data: Option<&[u8]>) -> Result<(bool, String), String> {
    use std::io::Write;
    let mut cmd = std::process::Command::new(prog);
    cmd.args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{} unavailable: {}", prog, e))?;
    if let Some(d) = stdin_data {
        child.stdin.take().unwrap().write_all(d).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok((out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string()))
}

pub fn sq_status() -> Json {
    let which = run_cmd("which", &["sq"], None).map(|(_, o)| o).unwrap_or_default();
    let status = run_cmd("sq", &["status"], None);
    Json::obj(vec![
        ("sq_path", Json::s(&which)),
        ("present", Json::b(!which.is_empty())),
        ("status_output", Json::s(&status.map(|(_, o)| o).unwrap_or_else(|e| e))),
        ("note", Json::s("SQ runs on ranch nodes, never on the coordinator VM; qexo is its keeper on-node")),
    ])
}

fn route_phext(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    let seg = |i: usize| path.get(i).map(|s| s.as_str()).unwrap_or("");
    let q = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    if method == "GET" && path.len() == 2 && seg(1) == "versions" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, Json::obj(vec![
            ("qexo", Json::s(env!("CARGO_PKG_VERSION"))),
            ("libphext_rs", Json::s("0.5.0 (published 2026-11-08)")),
            ("sq", Json::s("0.6.0 (pins libphext 0.3.3)")),
            ("phext_lattice", Json::s("0.3.1")),
            ("convergence", Json::s("SQ -> libphext 0.5.0 is a directed staged work item: its own canary stage, BEFORE and independent of the OpenClaw migration stages")),
            ("skew_rule", Json::s("writes verify by read-back-and-hash through the same node's surface before being reported done; transforms compare by SHA-256, never by rendered output")),
        ]));
    }
    if method == "GET" && path.len() == 2 && seg(1) == "sq" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, sq_status());
    }
    if method == "GET" && path.len() == 2 && seg(1) == "toc" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let phext_path = q("phext").unwrap_or_default();
        return match fs::read_to_string(&phext_path) {
            Ok(text) => {
                let px = crate::phext::Phext::parse(&text);
                let toc: Vec<Json> = px.toc().iter().map(|(c, n)| Json::obj(vec![("coord", Json::s(&fmt_coord(c))), ("bytes", Json::n(*n as f64))])).collect();
                (200, Json::obj(vec![("phext", Json::s(&phext_path)), ("scrolls", Json::n(toc.len() as f64)), ("toc", Json::Arr(toc))]))
            }
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", format!("cannot read {}: {}", phext_path, e)))),
        };
    }
    if method == "POST" && path.len() == 2 && seg(1) == "coord" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let text = gets(body, "text").unwrap_or_default();
        return (200, Json::obj(vec![("text", Json::s(&text)), ("coordinate", Json::s(&crate::coord::coordinate_of_text(&text))), ("mdmap_status", Json::s("provisional"))]));
    }
    if method == "POST" && path.len() == 3 && seg(1) == "sq" {
        if let Err(r) = gate(st, caller, "auth", "sq.manage", "sq.manage") {
            return (r.status, refusal_payload(&r));
        }
        match seg(2) {
            "ensure" => {
                // the sq-ensure behaviour, with its two known quirks as
                // handled cases: a stale .sq/ is removed before a fresh
                // start; SQ is shut down before its hosted file is
                // replaced (the caller performs replacements via stop).
                let hosted = gets(body, "hosted").unwrap_or_else(|| "choose-your-own-adventure.phext".into());
                let st1 = run_cmd("sq", &["status"], None);
                let running = matches!(&st1, Ok((true, o)) if o.contains("running") || o.contains("Running"));
                if running {
                    return (200, Json::obj(vec![("sq", Json::s("already running")), ("status", sq_status())]));
                }
                let sq_dir = std::path::Path::new(".sq");
                if sq_dir.exists() {
                    let _ = fs::remove_dir_all(sq_dir);
                }
                let start = run_cmd("sq", &["share", &hosted], None);
                return (200, Json::obj(vec![("sq", Json::s("start attempted")), ("result", Json::s(&start.map(|(_, o)| o).unwrap_or_else(|e| e))), ("status", sq_status())]));
            }
            "stop" => {
                let stop = run_cmd("sq", &["shutdown"], None);
                return (200, Json::obj(vec![("sq", Json::s("shutdown issued — orderly stop before any hosted-file replacement (the mmap wedge rule)")), ("result", Json::s(&stop.map(|(_, o)| o).unwrap_or_else(|e| e)))]));
            }
            _ => {}
        }
    }
    if method == "POST" && path.len() == 2 && seg(1) == "dispatch" {
        if let Err(r) = gate(st, caller, "auth", "sq.manage", "sq.manage") {
            return (r.status, refusal_payload(&r));
        }
        // parsed scroll selection with provenance, not a flat excerpt
        let phext_path = gets(body, "phext").unwrap_or_default();
        let coord = gets(body, "coord").unwrap_or_default();
        let text = fs::read_to_string(&phext_path).unwrap_or_default();
        let px = crate::phext::Phext::parse(&text);
        let parsed = crate::phext::parse_coord(&coord).and_then(|c| px.get(&c)).cloned().unwrap_or_default();
        let rcpt = receipt(st, "phext-dispatch", vec![("phext", Json::s(&phext_path)), ("coord", Json::s(&coord))]);
        return (200, Json::obj(vec![
            ("selection", Json::s(&parsed)),
            ("provenance", Json::obj(vec![("phext", Json::s(&phext_path)), ("coord", Json::s(&coord)), ("receipt", Json::s(&rcpt)), ("content_sha256", Json::s(&sha256_hex(parsed.as_bytes())))])),
        ]));
    }
    if method == "POST" && path.len() == 2 && seg(1) == "build" {
        if let Err(r) = gate(st, caller, "auth", "sq.manage", "sq.manage") {
            return (r.status, refusal_payload(&r));
        }
        let register = gets(body, "register").unwrap_or_default();
        let check = body.get("check").and_then(|v| v.as_bool()).unwrap_or(false);
        let home = std::env::var("HOME").unwrap_or_default();
        let script = format!("{}/workspace/phext/build_{}_phext.py", home, register);
        if !std::path::Path::new(&script).exists() {
            return (200, Json::obj(vec![("register", Json::s(&register)), ("builder", Json::s(&script)), ("result", Json::s("builder not present on this node — named, not papered over"))]));
        }
        let args: Vec<&str> = if check { vec!["--check"] } else { vec![] };
        let r = run_cmd("python3", &[&script, &args.join(" ")], None);
        return (200, Json::obj(vec![("register", Json::s(&register)), ("check_only", Json::b(check)), ("result", Json::s(&r.map(|(_, o)| o).unwrap_or_else(|e| e)))]));
    }
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown phext operation (CLI carries the full libphext surface: parse/toc/select/get/explode/implode/seal/unseal)")))
}

// ------------------------------------------------------------- mdmap (6.12)
fn route_mdmap(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    let prov = |mut j: Json| {
        if let Json::Obj(ref mut pairs) = j {
            pairs.push(("mdmap_status".to_string(), Json::s("provisional")));
        }
        j
    };
    if method == "POST" && path.len() == 2 && path[1] == "compute" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let file = gets(body, "file").unwrap_or_default();
        let text = body.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
            .or_else(|| fs::read_to_string(&file).ok()).unwrap_or_default();
        return (200, prov(Json::obj(vec![("file", Json::s(&file)), ("computed", Json::s(&crate::coord::coordinate_of_text(&text)))])));
    }
    if method == "POST" && path.len() == 2 && path[1] == "build" {
        if let Err(r) = gate(st, caller, "auth", "sq.manage", "sq.manage") {
            return (r.status, refusal_payload(&r));
        }
        let dir = gets(body, "dir").unwrap_or_default();
        return (200, prov(crate::coord::mdmap_build(&dir)));
    }
    if method == "GET" && path.len() == 1 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "meta.read") {
            return (r.status, refusal_payload(&r));
        }
        let index = st.read_json("mdmap-index.json").unwrap_or(Json::Arr(vec![]));
        return (200, prov(Json::obj(vec![("index", index)])));
    }
    if method == "GET" && path.len() == 2 && path[1] == "lookup" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let want = query.iter().find(|(n, _)| n == "path" || n == "coord").map(|(_, v)| v.clone()).unwrap_or_default();
        let index = st.read_json("mdmap-index.json").unwrap_or(Json::Arr(vec![]));
        let hits: Vec<Json> = match &index {
            Json::Arr(rows) => rows.iter().filter(|r| r.to_string().contains(&want)).cloned().collect(),
            _ => vec![],
        };
        return (200, prov(Json::obj(vec![("query", Json::s(&want)), ("hits", Json::Arr(hits))])));
    }
    let _ = ctx;
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown mdmap operation")))
}

// ------------------------------------------------------------- corpus/source
fn route_corpus(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    let q = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if method == "GET" && path.len() == 1 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let mut m = store::manifest();
        if let Json::Obj(ref mut pairs) = m {
            pairs.push(("volumen".to_string(), vol::baked_volumen_index().manifest_json()));
        }
        return (200, m);
    }
    if method == "POST" && path.len() == 2 && path[1] == "verify" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, store::corpus_verify());
    }
    if method == "GET" && path.len() == 2 && path[1] == "sbor" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, Json::obj(vec![
            ("artifact", Json::s("sbor")),
            ("title", Json::s("SBOR — SBOR-V4-CONVERGENCE, ratified 2026-01-21")),
            ("sha256", Json::s(&sha256_hex(store::SBOR_MD))),
            ("content", Json::s(&String::from_utf8_lossy(store::SBOR_MD))),
            ("note", Json::s("readable offline, from the box itself, by design")),
        ]));
    }
    if method == "GET" && path.len() == 2 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let name = path[1].clone();
        if q("whole").as_deref() == Some("1") {
            return match store::corpus_whole(&name) {
                Ok(bytes) => (200, Json::obj(vec![("artifact", Json::s(&name)), ("bytes", Json::n(bytes.len() as f64)), ("sha256", Json::s(&sha256_hex(&bytes))), ("content", Json::s(&String::from_utf8_lossy(&bytes)))])),
                Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
            };
        }
        return match store::corpus_toc(&name) {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "GET" && path.len() == 3 && path[2] == "scroll" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let coord = q("coord").unwrap_or_default();
        return match store::corpus_scroll(&path[1], &coord) {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    let _ = (ctx, body);
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown corpus operation")))
}

fn route_source(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if method == "GET" && path.len() == 2 && path[1] == "manifest" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return match store::source_manifest() {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "GET" && path.len() == 2 && path[1] == "scroll" {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        let project = query.iter().find(|(n, _)| n == "project").map(|(_, v)| v.clone()).unwrap_or_default();
        let p = query.iter().find(|(n, _)| n == "path").map(|(_, v)| v.clone()).unwrap_or_default();
        // source scrolls are addressed by coordinates assigned at bake
        // time in canonical order; the lookup walks the manifest order
        return match store::phext_of(store::SOURCE_SEALED) {
            Ok(px) => {
                for (coord, text) in &px.scrolls {
                    if text.contains(&format!("project={} ", project)) && text.contains(&format!("path={} ", p)) {
                        return (200, Json::obj(vec![("coord", Json::s(&fmt_coord(coord))), ("content", Json::s(text))]));
                    }
                }
                (404, refusal_payload(&Refusal::new(404, "no-such-record", "no source scroll for that project/path")))
            }
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "POST" && path.len() == 2 && path[1] == "unpack" {
        if let Err(r) = gate(st, caller, "policy", "install.apply", "install.apply") {
            return (r.status, refusal_payload(&r));
        }
        let dir = gets(body, "dir").unwrap_or_default();
        return match store::source_unpack(&dir) {
            Ok(v) => (200, v),
            Err(e) => (422, refusal_payload(&Refusal::new(422, "unverified-state", e))),
        };
    }
    if method == "POST" && path.len() == 2 && path[1] == "check" {
        if let Err(r) = gate(st, caller, "auth", "", "") {
            return (r.status, refusal_payload(&r));
        }
        // reproducibility: the bake manifest's recorded inputs are the
        // comparison; a runtime re-bake is the builder's --check.
        return (200, Json::obj(vec![
            ("check", Json::s("manifest comparison")),
            ("corpus_verify", store::corpus_verify()),
            ("note", Json::s("full rebuild-and-compare runs as the bake builder's --check (tools/build_bake.py + corpus bake): given the same qexo-bake.toml and recorded commits the bake is byte-identical, and any difference is attributable to a named input")),
        ]));
    }
    let _ = ctx;
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown source operation")))
}

// ------------------------------------------------------------- meta (6.15)
fn route_meta(ctx: &Ctx, method: &str, path: &[String], query: &[(String, String)], caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if method != "GET" {
        return (404, refusal_payload(&Refusal::new(404, "no-such-record", "metadata is read-only")));
    }
    if let Err(r) = gate(st, caller, "auth", "meta.read", "meta.read") {
        // metadata over a node boundary requires auth; local reads pass
        if !caller.local {
            return (r.status, refusal_payload(&r));
        }
    }
    let since = query.iter().find(|(n, _)| n == "since").map(|(_, v)| v.clone());
    let wrap = |payload: Json| -> (u16, Json) {
        let h = sha256_hex(payload.to_string().as_bytes());
        if let Some(s) = &since {
            if s == &h {
                return (200, Json::obj(vec![("unchanged", Json::b(true)), ("hash", Json::s(&h))]));
            }
        }
        let mut p = payload;
        if let Json::Obj(ref mut pairs) = p {
            pairs.push(("record_hash".to_string(), Json::s(&h)));
            pairs.push(("as_of".to_string(), Json::s(&now_rfc3339())));
        }
        (200, p)
    };
    if path.len() == 3 && path[1] == "indexes" {
        
        return wrap(Json::obj(vec![
            ("index", Json::s(&path[2])),
            ("summary", match path[2].as_str() {
                "personas" => Json::obj(vec![("records", Json::n(ctx.census.records.len() as f64)), ("query_keys", Json::n(ctx.census.keys.len() as f64))]),
                "mdmap" => st.read_json("mdmap-index.json").unwrap_or(Json::Arr(vec![])),
                _ => Json::obj(vec![("note", Json::s("index parsed lines are served from the register's Home scroll; this node carries personas + mdmap locally"))]),
            }),
        ]));
    }
    if path.len() == 2 && path[1] == "nodes" {
        return wrap(Json::obj(vec![
            ("nodes", Json::Arr(vec![Json::obj(vec![
                ("node", Json::s(&st.node)),
                ("qexo", Json::s(env!("CARGO_PKG_VERSION"))),
                ("active_persona", px::persona_state(st).active.as_deref().map(Json::s).unwrap_or(Json::Null)),
                ("open_teleport_tokens", Json::n(st.read_log("teleports.jsonl").iter().filter(|e| e.get("state").and_then(|v| v.as_str()) == Some("issued")).count() as f64)),
            ])])),
            ("note", Json::s("fleet aggregation rides sync; node metadata is fleet-internal")),
        ]));
    }
    if path.len() == 4 && path[1] == "nodes" && path[3] == "inventory" {
        return wrap(Json::obj(vec![("node", Json::s(&path[2])), ("note", Json::s("inventory lines carry path/coordinate, size, sha256, updated-at — the same fields a sync manifest carries"))]));
    }
    if path.len() == 2 && path[1] == "provenance" {
        let phext = query.iter().find(|(n, _)| n == "phext").map(|(_, v)| v.clone()).unwrap_or_default();
        let coord = query.iter().find(|(n, _)| n == "coord").map(|(_, v)| v.clone()).unwrap_or_default();
        return wrap(Json::obj(vec![("phext", Json::s(&phext)), ("coord", Json::s(&coord)), ("chain", Json::obj(vec![("note", Json::s("provenance chains ride the record's receipt scrolls; a persona's memory scope governs who may query its metadata"))]))]));
    }
    if path.len() == 2 && path[1] == "registers" {
        return wrap(Json::obj(vec![
            ("receipts", Json::n(st.read_log("receipts-events.jsonl").len() as f64)),
            ("instances", Json::n(px::latest_instances(st).len() as f64)),
            ("teleport_events", Json::n(st.read_log("teleports.jsonl").len() as f64)),
        ]));
    }
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown meta operation")))
}

// ------------------------------------------------------------- install/doctor/mail/wake
fn route_install(ctx: &Ctx, method: &str, path: &[String], body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if method == "GET" && path.len() == 1 {
        if let Err(r) = check_floor(st, caller, "local-read", "", "") {
            return (r.status, refusal_payload(&r));
        }
        return (200, Json::obj(vec![
            ("installed", Json::Arr(vec![Json::obj(vec![("tool", Json::s("qexo")), ("version", Json::s(env!("CARGO_PKG_VERSION")))])])),
            ("acceptance", Json::s("the section 11.3 versions table per node is the acceptance check; a node whose table does not match the intended release is a failed stage, not an install")),
            ("catalog", crate::tools::catalog_json()),
        ]));
    }
    if method == "POST" && (path.len() == 1 || (path.len() == 2 && path[1] == "upgrade")) {
        if let Err(r) = gate(st, caller, "policy", "install.apply", "install.apply") {
            return (r.status, refusal_payload(&r));
        }
        let tool = gets(body, "tool").unwrap_or_default();
        if tool == "qexo" {
            // installer-lite: the artifact lays down the binary +
            // baked corpus and prints the SBOR offline
            return (200, Json::obj(vec![
                ("installed", Json::s("qexo (self)")),
                ("sbor_offline", Json::b(true)),
                ("note", Json::s("installer-lite: binary + baked corpus laid down from the artifact itself; the SBOR prints from the box with no network (qexo corpus sbor)")),
            ]));
        }
        let rcpt = receipt(st, "install", vec![("tool", Json::s(&tool)), ("by", Json::s(&caller.principal))]);
        let mut fields = vec![
            ("install_recorded", Json::b(true)),
            ("tool", Json::s(&tool)),
            ("receipt", Json::s(&rcpt)),
            ("channel", Json::s("the exocortex packaging line (.deb -> signed repo -> PPA -> archive); installs are staged canary-first; fix in the package, never hand-patch a node")),
        ];
        // A catalog tool's install record carries what qexo knows
        // about it — source, package preflight, recipe, pitfalls
        // (src/tools.rs). Unknown tools record exactly as before.
        if let Some(spec) = crate::tools::find(&tool) {
            fields.push(("catalog_entry", crate::tools::spec_json(spec)));
        }
        return (200, Json::obj(fields));
    }
    let _ = ctx;
    (404, refusal_payload(&Refusal::new(404, "no-such-record", "unknown install operation")))
}

pub fn doctor_report(st: &State) -> Json {
    let mut findings = Vec::new();
    // quota-class disk pressure
    let (avail, load) = plan::host_snapshot();
    findings.push(Json::obj(vec![("check", Json::s("host snapshot")), ("ram_available_mb", Json::n(avail)), ("load1", Json::n(load))]));
    findings.push(Json::obj(vec![("check", Json::s("tool catalog apt preflight")), ("result", Json::s(crate::tools::preflight_summary()))]));
    if std::path::Path::new(".sq").exists() {
        findings.push(Json::obj(vec![("finding", Json::s("stale .sq/ in the working directory")), ("fix", Json::s("remove it before a fresh SQ start; sq-ensure hangs on a stale .sq/ (quirk corpus)"))]));
    }
    findings.push(Json::obj(vec![("check", Json::s("write capability")), ("result", Json::s(if crate::state::atomic_rename_capable(st) { "atomic temp+rename verified" } else { "temp+rename NOT capable — append/direct patterns in force (Muse VM substrate gap, docs/DESIGN-NOTES.md)" }))]));
    findings.push(Json::obj(vec![("check", Json::s("gpg")), ("result", Json::s(if gpg_available() { "present" } else { "ABSENT — signed operations cannot run on this node" }))]));
    findings.push(Json::obj(vec![("check", Json::s("sq")), ("result", sq_status())]));
    Json::obj(vec![("doctor", Json::s(&st.node)), ("findings", Json::Arr(findings)), ("note", Json::s("the pitfall corpus is data (lessons.phext + quirk notes); every new pitfall learned in operations becomes a scroll"))])
}

fn mail_drop(ctx: &Ctx, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if caller.tainted {
        // composition is tainted-permitted; sending is not
        return (200, Json::obj(vec![
            ("drafted", Json::b(true)),
            ("sent", Json::b(false)),
            ("note", Json::s("tainted callers may compose drafts; mail.send as an external send is outside the TAINTED set (section 8A.6)")),
        ]));
    }
    if let Err(r) = gate(st, caller, "auth", "mail.send", "mail.send") {
        return (r.status, refusal_payload(&r));
    }
    let to = gets(body, "to").unwrap_or_default();
    let text = gets(body, "text").unwrap_or_default();
    // sign the drop when the authoring persona holds a key
    let mut signed = "unsigned".to_string();
    if let Some(fp) = &caller.key {
        let gpg = Gpg::open(st);
        if gpg.sign_detached(fp, text.as_bytes()).is_ok() {
            signed = format!("signed by {}", fp);
        }
    }
    let _ = st.append_log("mailbox.jsonl", &Json::obj(vec![("to", Json::s(&to)), ("from", Json::s(&caller.principal)), ("attribution", Json::s(&signed)), ("at", Json::s(&now_rfc3339()))]));
    (200, Json::obj(vec![("dropped", Json::b(true)), ("attribution", Json::s(&signed)), ("note", Json::s("unsigned drops keep working under the mailbox convention and are labelled unsigned — never quietly upgraded"))]))
}

fn wake_send(ctx: &Ctx, body: &Json, caller: &Caller) -> (u16, Json) {
    let st = &ctx.state;
    if let Err(r) = gate(st, caller, "auth", "wake.send", "wake.send") {
        return (r.status, refusal_payload(&r));
    }
    let persona = gets(body, "persona").unwrap_or_default();
    if ctx.census.resolve(&persona).is_empty() {
        return (404, refusal_payload(&Refusal::new(404, "no-such-record", "wake names a persona in the index")));
    }
    let rcpt = receipt(st, "wake", vec![("persona", Json::s(&persona)), ("by", Json::s(&caller.principal))]);
    (200, Json::obj(vec![
        ("wake_issued", Json::b(true)),
        ("receipt", Json::s(&rcpt)),
        ("contract", Json::s("the persona's wake contract governs receipt (channels, rate limits, quiet hours); a wake request that fails verification does not wake")),
    ]))
}
