//! CLI mode: the reference client. Every subcommand is a thin skin
//! over the same core dispatch the API serves — same code path,
//! --json emitting the endpoint's response shape unchanged, identical
//! authorization. Identity comes from the environment:
//!   QEXO_PRINCIPAL, QEXO_KEY, QEXO_DELEGATION, QEXO_TAINT,
//!   QEXO_TRIGGER, QEXO_SEQUENCE, QEXO_TIMESTAMP
//! A local invocation marks the caller local (loopback-equivalent):
//! local-read surfaces open; mutations still require a principal.

use crate::core::{Caller, Ctx};
use crate::json::Json;
use std::sync::Arc;

pub struct CliOut {
    pub status: u16,
    pub payload: Json,
}

fn env_caller() -> Caller {
    Caller {
        principal: std::env::var("QEXO_PRINCIPAL").unwrap_or_else(|_| "anonymous".into()),
        key: std::env::var("QEXO_KEY").ok(),
        delegation: std::env::var("QEXO_DELEGATION").ok(),
        tainted: std::env::var("QEXO_TAINT").map(|t| t == "tainted").unwrap_or(false),
        trigger: std::env::var("QEXO_TRIGGER").ok(),
        epoch: std::env::var("QEXO_EPOCH").ok().and_then(|v| v.parse().ok()),
        sequence: std::env::var("QEXO_SEQUENCE").ok().and_then(|v| v.parse().ok()),
        timestamp: std::env::var("QEXO_TIMESTAMP").ok().and_then(|v| v.parse().ok()),
        recipient: std::env::var("QEXO_RECIPIENT").ok(),
        local: true,
    }
}

fn parse_flags(args: &[String]) -> (Vec<String>, Vec<(String, String)>, bool) {
    let mut pos = Vec::new();
    let mut flags = Vec::new();
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--json" {
            json = true;
            i += 1;
        } else if let Some(v) = a.strip_prefix("--") {
            if let Some((k, val)) = v.split_once('=') {
                flags.push((k.to_string(), val.to_string()));
                i += 1;
            } else if i + 1 < args.len() {
                flags.push((v.to_string(), args[i + 1].clone()));
                i += 2;
            } else {
                flags.push((v.to_string(), "true".into()));
                i += 1;
            }
        } else {
            pos.push(a.clone());
            i += 1;
        }
    }
    (pos, flags, json)
}

fn body_from_flags(flags: &[(String, String)]) -> Json {
    Json::Obj(
        flags
            .iter()
            .map(|(k, v)| {
                let val = if v == "true" {
                    Json::Bool(true)
                } else if v == "false" {
                    Json::Bool(false)
                } else if let Ok(n) = v.parse::<f64>() {
                    Json::Num(n)
                } else {
                    Json::s(v)
                };
                (k.clone(), val)
            })
            .collect(),
    )
}

pub fn run(ctx: &Arc<Ctx>, args: &[String]) -> CliOut {
    let caller = env_caller();
    // bare `qexo` (no words): the v1 invariant — emit the plan.
    let args: &[String] = if args.is_empty() { &["plan".to_string()] } else { args };
    let (method, path, body, _human): (&str, Vec<String>, Json, bool) = translate(args);
    let rid = ctx.state.request_id();
    let (status, payload) = crate::core::dispatch(ctx, method, &path, &[], &body, &caller, &rid);
    CliOut { status, payload }
}

/// Map CLI words to (method, api path, body). The default: the words
/// ARE the path (qexo personas Emise -> GET /v1/personas/Emise).
fn translate(args: &[String]) -> (&'static str, Vec<String>, Json, bool) {
    let (pos, flags, _json) = parse_flags(args);
    let w = |i: usize| pos.get(i).map(|s| s.as_str()).unwrap_or("");
    let mut body = body_from_flags(&flags);
    let set = |b: &mut Json, k: &str, v: &str| {
        if let Json::Obj(ref mut pairs) = b {
            if !pairs.iter().any(|(pk, _)| pk == k) {
                pairs.push((k.to_string(), Json::s(v)));
            }
        }
    };
    match (w(0), w(1)) {
        ("plan", _) if w(1) == "current" => ("GET", vec!["plan".into(), "current".into()], body, false),
        ("plan", _) => ("POST", vec!["plan".into()], body, false),
        ("swap", _) => {
            set(&mut body, "name", w(1));
            ("POST", vec!["personas".into(), "swap".into()], body, false)
        }
        ("spawn", _) => ("POST", vec!["personas".into(), w(1).into(), "spawn".into()], body, false),
        ("teleport", "consent") | ("teleport", "transfer") | ("teleport", "verify") | ("teleport", "return") | ("teleport", "relinquish") | ("teleport", "activate") => {
            ("POST", vec!["teleports".into(), w(2).into(), w(1).into()], body, false)
        }
        ("teleport", _) => ("POST", vec!["personas".into(), w(1).into(), "teleport".into()], body, false),
        ("teleport-return", _) => ("POST", vec!["personas".into(), w(1).into(), "teleport".into(), "return".into()], body, false),
        ("teleports", _) => ("GET", vec!["teleports".into()], body, false),
        ("residence", _) => ("GET", vec!["personas".into(), w(1).into(), "residence".into()], body, false),
        ("keys", "list") => ("GET", vec!["keys".into()], body, false),
        ("keys", "show") => ("GET", vec!["keys".into(), w(2).into()], body, false),
        ("keys", "gen-node") => ("POST", vec!["keys".into(), "nodes".into()], body, false),
        ("keys", "gen-persona") => {
            set(&mut body, "persona", w(2));
            ("POST", vec!["keys".into(), "personas".into()], body, false)
        }
        ("keys", "countersign") => ("POST", vec!["keys".into(), w(2).into(), "countersign".into()], body, false),
        ("keys", "retire") => ("POST", vec!["keys".into(), w(2).into(), "retire".into()], body, false),
        ("keys", "revoke") => ("POST", vec!["keys".into(), w(2).into(), "revoke".into()], body, false),
        ("keys", "delegate") => ("POST", vec!["delegations".into()], body, false),
        ("keys", "revoke-delegation") => ("POST", vec!["delegations".into(), w(2).into(), "revoke".into()], body, false),
        ("verify", _) => ("POST", vec!["verify".into()], body, false),
        ("grants", "list") => ("GET", vec!["grants".into()], body, false),
        ("grants", "set") => {
            set(&mut body, "grant", w(2));
            if w(3) == "off" {
                set(&mut body, "enabled", "false");
                if let Json::Obj(ref mut pairs) = body {
                    pairs.retain(|(k, _)| k != "enabled");
                    pairs.push(("enabled".into(), Json::Bool(false)));
                }
            }
            ("POST", vec!["grants".into()], body, false)
        }
        ("policy", _) => ("GET", vec!["policy".into()], body, false),
        ("route", "set") => ("POST", vec!["routes".into()], body, false),
        ("route", "ask") => ("POST", vec!["route".into(), "ask".into()], body, false),
        ("sync-manifest", _) => {
            set(&mut body, "dir", w(1));
            ("POST", vec!["sync".into(), "manifest".into()], body, false)
        }
        ("sync-diff", _) => {
            // diff is computed locally in main for file args; routed here
            // for --json symmetry when bodies are used
            ("POST", vec!["sync".into(), "diff".into()], body, false)
        }
        ("sync", "status") => ("GET", vec!["sync".into()], body, false),
        ("sync", "push") => ("POST", vec!["sync".into(), "push".into()], body, false),
        ("sync", "verify") => ("POST", vec!["sync".into(), "verify".into()], body, false),
        ("sync", "pull") => ("POST", vec!["sync".into(), "pull".into()], body, false),
        ("interfaces", "temperature") => ("GET", vec!["state".into(), "temperature".into()], body, false),
        ("interfaces", "emotion") if w(2) == "append" => ("POST", vec!["state".into(), "emotion".into()], body, false),
        ("interfaces", "emotion") => ("GET", vec!["state".into(), "emotion".into()], body, false),
        ("interfaces", "goals") if w(2) == "log" => ("POST", vec!["goals".into(), w(3).into(), "activity".into()], body, false),
        ("interfaces", "goals") => ("GET", vec!["goals".into()], body, false),
        ("interfaces", "desires") => ("GET", vec!["desires".into()], body, false),
        ("interfaces", "dreams") => ("GET", vec!["dreams".into()], body, false),
        ("interfaces", _) => ("GET", vec!["state".into(), "temperature".into()], body, false),
        ("personas", "active") => ("GET", vec!["personas".into(), "active".into()], body, false),
        ("personas", "show") => ("GET", vec!["personas".into(), w(2).into()], body, false),
        ("personas", "seed") => ("POST", vec!["personas".into()], body, false),
        ("personas", "grants") => ("POST", vec!["personas".into(), w(2).into(), "grants".into()], body, false),
        ("personas", "graduate") => ("POST", vec!["personas".into(), w(2).into(), "graduate".into()], body, false),
        ("personas", _) if !w(1).is_empty() => ("GET", vec!["personas".into(), w(1).into()], body, false),
        ("personas", _) => ("GET", vec!["personas".into()], body, false),
        ("phext", "versions") => ("GET", vec!["phext".into(), "versions".into()], body, false),
        ("phext", "sq") if w(2) == "status" => ("GET", vec!["phext".into(), "sq".into()], body, false),
        ("phext", "sq") => ("POST", vec!["phext".into(), "sq".into(), w(2).into()], body, false),
        ("phext", "toc") => ("GET", vec!["phext".into(), "toc".into()], body, false),
        ("phext", "coord") => {
            set(&mut body, "text", w(2));
            ("POST", vec!["phext".into(), "coord".into()], body, false)
        }
        ("phext", "dispatch") => ("POST", vec!["phext".into(), "dispatch".into()], body, false),
        ("phext", "build") => {
            set(&mut body, "register", w(2));
            ("POST", vec!["phext".into(), "build".into()], body, false)
        }
        ("mdmap", "compute") => {
            set(&mut body, "file", w(2));
            ("POST", vec!["mdmap".into(), "compute".into()], body, false)
        }
        ("mdmap", "build") => {
            set(&mut body, "dir", w(2));
            ("POST", vec!["mdmap".into(), "build".into()], body, false)
        }
        ("mdmap", "index") => ("GET", vec!["mdmap".into()], body, false),
        ("mdmap", "lookup") => ("GET", vec!["mdmap".into(), "lookup".into()], body, false),
        ("corpus", "list") => ("GET", vec!["corpus".into()], body, false),
        ("corpus", "sbor") => ("GET", vec!["corpus".into(), "sbor".into()], body, false),
        ("corpus", "verify") => ("POST", vec!["corpus".into(), "verify".into()], body, false),
        ("corpus", "show") => ("GET", vec!["corpus".into(), w(2).into()], body, false),
        ("corpus", "scroll") => ("GET", vec!["corpus".into(), w(2).into(), "scroll".into()], body, false),
        ("source", "manifest") => ("GET", vec!["source".into(), "manifest".into()], body, false),
        ("source", "show") => ("GET", vec!["source".into(), "scroll".into()], body, false),
        ("source", "unpack") => {
            set(&mut body, "dir", w(2));
            ("POST", vec!["source".into(), "unpack".into()], body, false)
        }
        ("source", "check") => ("POST", vec!["source".into(), "check".into()], body, false),
        ("meta", "index") => ("GET", vec!["meta".into(), "indexes".into(), w(2).into()], body, false),
        ("meta", "nodes") => ("GET", vec!["meta".into(), "nodes".into()], body, false),
        ("meta", "inventory") => ("GET", vec!["meta".into(), "nodes".into(), w(2).into(), "inventory".into()], body, false),
        ("meta", "provenance") => ("GET", vec!["meta".into(), "provenance".into()], body, false),
        ("meta", "registers") => ("GET", vec!["meta".into(), "registers".into()], body, false),
        ("install", "list") => ("GET", vec!["install".into()], body, false),
        ("install", "self") => {
            set(&mut body, "tool", "qexo");
            ("POST", vec!["install".into()], body, false)
        }
        ("install", _) => {
            set(&mut body, "tool", w(1));
            ("POST", vec!["install".into()], body, false)
        }
        ("upgrade", _) => {
            set(&mut body, "tool", w(1));
            ("POST", vec!["install".into(), "upgrade".into()], body, false)
        }
        ("doctor", _) => ("GET", vec!["doctor".into()], body, false),
        ("mail", "drop") => ("POST", vec!["mail".into()], body, false),
        ("wake", _) => {
            set(&mut body, "persona", w(1));
            ("POST", vec!["wake".into()], body, false)
        }
        ("meld", _) => ("POST", vec!["personas".into(), w(1).into(), "meld".into()], body, false),
        _ => ("GET", pos.clone(), body, false),
    }
}
