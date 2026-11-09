//! MCP mode (section 15.1): a bridge that is a CLIENT of the API,
//! never a bypass. Tool definitions are generated from the operation
//! catalogue; every tool call is dispatched through the same core
//! with the calling client's own delegated key in the caller block.
//! Transport here is stdio JSON-RPC (the MCP shape): initialize,
//! tools/list, tools/call.

use crate::core::{Caller, Ctx};
use crate::json::Json;
use std::io::{BufRead, Write};

pub fn catalogue() -> Vec<(&'static str, &'static str, Vec<String>, &'static str)> {
    // (tool name, http method, api path segments, description)
    vec![
        ("qexo_plan", "POST", vec!["plan".into()], "emit the placement plan (four currencies)"),
        ("qexo_personas_list", "GET", vec!["personas".into()], "the persona index: 126 keys, 106 records"),
        ("qexo_personas_show", "GET", vec!["personas".into(), "{key}".into()], "resolve one query key (Orin returns both records)"),
        ("qexo_personas_swap", "POST", vec!["personas".into(), "swap".into()], "hot swap the worn persona"),
        ("qexo_personas_spawn", "POST", vec!["personas".into(), "{key}".into(), "spawn".into()], "spawn a task instance of a spawn-eligible record"),
        ("qexo_teleport", "POST", vec!["personas".into(), "{key}".into(), "teleport".into()], "issue a teleport token (steps follow under qexo_teleport_step)"),
        ("qexo_teleport_step", "POST", vec!["teleports".into(), "{token}".into(), "{step}".into()], "run one teleport protocol step: consent|transfer|verify|return|relinquish|activate"),
        ("qexo_keys_list", "GET", vec!["keys".into()], "the key band"),
        ("qexo_grants_list", "GET", vec!["grants".into()], "the grants catalogue with current toggles"),
        ("qexo_corpus_list", "GET", vec!["corpus".into()], "the bake manifest"),
        ("qexo_corpus_sbor", "GET", vec!["corpus".into(), "sbor".into()], "the SBOR, readable offline from the box"),
        ("qexo_meta_nodes", "GET", vec!["meta".into(), "nodes".into()], "fleet metadata"),
        ("qexo_route_ask", "POST", vec!["route".into(), "ask".into()], "make an inference request through the router"),
        ("qexo_doctor", "GET", vec!["doctor".into()], "node diagnostics against the pitfall corpus"),
        ("qexo_sync_manifest", "POST", vec!["sync".into(), "manifest".into()], "write a MANIFEST.sha256 for a bundle directory"),
        ("qexo_mdmap_compute", "POST", vec!["mdmap".into(), "compute".into()], "the Singular coordinate of a markdown file (provisional)"),
    ]
}

pub fn tools_list() -> Json {
    let tools: Vec<Json> = catalogue()
        .iter()
        .map(|(name, _, _, desc)| {
            Json::obj(vec![
                ("name", Json::s(name)),
                ("description", Json::s(desc)),
                (
                    "inputSchema",
                    Json::obj(vec![
                        ("type", Json::s("object")),
                        ("properties", Json::obj(vec![
                            ("key", Json::obj(vec![("type", Json::s("string"))])),
                            ("token", Json::obj(vec![("type", Json::s("string"))])),
                            ("step", Json::obj(vec![("type", Json::s("string"))])),
                            ("body", Json::obj(vec![("type", Json::s("object"))])),
                        ])),
                    ]),
                ),
            ])
        })
        .collect();
    Json::obj(vec![("tools", Json::Arr(tools))])
}

fn substitute(path: &[String], args: &Json) -> Vec<String> {
    path.iter()
        .map(|seg| {
            if seg.starts_with('{') {
                let name = seg.trim_matches(|c| c == '{' || c == '}');
                args.get(name).and_then(|v| v.as_str()).unwrap_or("").to_string()
            } else {
                seg.clone()
            }
        })
        .collect()
}

pub fn serve_stdio(ctx: &Ctx) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    // MCP client identity: each client holds its OWN delegated key
    // (section 8A.5) — from the environment the bridge was launched
    // with, never a shared bridge key.
    let client_id = std::env::var("QEXO_MCP_CLIENT").unwrap_or_else(|_| "unregistered".into());
    let client_key = std::env::var("QEXO_MCP_KEY").ok();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req = match crate::json::parse(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let id = req.get("id").cloned().unwrap_or(Json::Null);
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let result = match method {
            "initialize" => Json::obj(vec![
                ("protocolVersion", Json::s("2024-11-05")),
                ("serverInfo", Json::obj(vec![("name", Json::s("qexo")), ("version", Json::s(env!("CARGO_PKG_VERSION")))])),
                ("capabilities", Json::obj(vec![("tools", Json::obj(vec![]))])),
            ]),
            "tools/list" => tools_list(),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(Json::Null);
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Json::obj(vec![]));
                match catalogue().iter().find(|(n, _, _, _)| *n == name) {
                    Some((_, http_method, path, _)) => {
                        let op_path = substitute(path, &args);
                        let body = args.get("body").cloned().unwrap_or(Json::Obj(vec![]));
                        // the bridge caller: the client's own principal
                        // + delegated key. It grants nothing itself.
                        let caller = Caller {
                            principal: format!("mcp-client:{}", client_id),
                            key: client_key.clone(),
                            delegation: std::env::var("QEXO_MCP_DELEGATION").ok(),
                            tainted: false,
                            trigger: None,
                            epoch: None,
                            sequence: None,
                            timestamp: None,
                            recipient: None,
                            local: false,
                        };
                        let rid = ctx.state.request_id();
                        let (status, payload) = crate::core::dispatch(ctx, http_method, &op_path, &[], &body, &caller, &rid);
                        Json::obj(vec![
                            ("content", Json::Arr(vec![Json::obj(vec![
                                ("type", Json::s("text")),
                                ("text", Json::s(&payload.to_string())),
                            ])])),
                            ("isError", Json::b(status >= 400)),
                        ])
                    }
                    None => Json::obj(vec![("content", Json::Arr(vec![Json::obj(vec![("type", Json::s("text")), ("text", Json::s("no such tool"))])])), ("isError", Json::b(true))]),
                }
            }
            _ => Json::obj(vec![("error", Json::s("unknown method"))]),
        };
        let resp = Json::obj(vec![("jsonrpc", Json::s("2.0")), ("id", id), ("result", result)]);
        let _ = writeln!(stdout, "{}", resp.to_string());
        let _ = stdout.flush();
    }
}
