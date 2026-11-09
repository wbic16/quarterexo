//! The HTTP transports: API mode (the reference semantics) and Web
//! mode (the baked-in mobile-first client, served from the node root)
//! share this server. One implementation, two surfaces (API-V1
//! section 9): the web client is a client of /v1 rendered as static
//! HTML+JS baked into the binary; it calls the same endpoints under
//! the same floor.

use crate::core::{dispatch, Caller, Ctx};
use crate::json::Json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

const WEB_CLIENT: &str = include_str!("../web/index.html");

pub fn parse_query(qs: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in qs.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k.to_string(), percent_decode(v)),
            None => (pair.to_string(), String::new()),
        };
        out.push((percent_decode(&k), v));
    }
    out
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let h = (bytes[i + 1] as char).to_digit(16);
                let l = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (h, l) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn caller_from_headers(headers: &[(String, String)], local: bool) -> Caller {
    let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    let principal = get("x-qexo-principal");
    let key = get("x-qexo-key");
    match (principal, key) {
        (Some(p), Some(k)) => Caller {
            principal: p,
            key: Some(k),
            delegation: get("x-qexo-delegation"),
            tainted: get("x-qexo-taint").map(|t| t == "tainted").unwrap_or(false),
            trigger: get("x-qexo-trigger"),
            epoch: get("x-qexo-epoch").and_then(|v| v.parse().ok()),
            sequence: get("x-qexo-sequence").and_then(|v| v.parse().ok()),
            timestamp: get("x-qexo-timestamp").and_then(|v| v.parse().ok()),
            recipient: get("x-qexo-recipient"),
            local,
        },
        _ => {
            // web sessions authenticate as a principal too (section
            // 14.2): the session header carries principal+key the same
            // way; its absence is anonymous.
            Caller::anonymous(local)
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<(String, String)>, String)> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end;
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if buf.len() > 1_048_576 {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim().to_string(), v.trim().to_string());
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Some((method, target, headers, String::from_utf8_lossy(&body).to_string()))
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, reason, content_type, body.len(), body
    );
    let _ = stream.write_all(resp.as_bytes());
}

pub fn serve(ctx: Arc<Ctx>, addr: &str) -> Result<(), String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("bind {}: {}", addr, e))?;
    eprintln!("qexo serve: listening on {} (node {}) — API at /v1, web client at /", addr, ctx.state.node);
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let peer_local = stream.peer_addr().map(|a| a.ip().is_loopback()).unwrap_or(false);
            let (method, target, headers, body_text) = match read_request(&mut stream) {
                Some(r) => r,
                None => return,
            };
            let (raw_path, qs) = match target.split_once('?') {
                Some((p, q)) => (p.to_string(), q.to_string()),
                None => (target.clone(), String::new()),
            };
            // Web mode: the baked-in client from the node root.
            if method == "GET" && (raw_path == "/" || raw_path == "/index.html") {
                respond(&mut stream, 200, "text/html; charset=utf-8", WEB_CLIENT);
                return;
            }
            let path_str = percent_decode(&raw_path);
            let segments: Vec<String> = path_str
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if segments.first().map(|s| s.as_str()) != Some("v1") {
                respond(&mut stream, 404, "application/json", "{\"ok\":false,\"error\":{\"refusal\":\"no-such-record\",\"reason\":\"the API lives under /v1; the web client lives at /\"}}");
                return;
            }
            let op_path: Vec<String> = segments[1..].to_vec();
            let query = parse_query(&qs);
            let body = crate::json::parse(&body_text).unwrap_or(Json::Obj(vec![]));
            let caller = caller_from_headers(&headers, peer_local);
            // cryptographic verification of signed callers happens at
            // dispatch level via the caller block in the body when a
            // signature is carried; header-only callers are verified
            // against the key directory inside the core's gates.
            let request_id = get_header(&headers, "x-qexo-request-id").unwrap_or_else(|| ctx.state.request_id());
            let (status, payload) = dispatch(&ctx, &method, &op_path, &query, &body, &caller, &request_id);
            respond(&mut stream, status, "application/json", &payload.to_string());
        });
    }
    Ok(())
}

fn get_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}
