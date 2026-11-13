//! infer — the Ollama edge. THE foreign border of this crate.
//!
//! Ollama speaks JSON over HTTP; qexo does not. This module is the
//! translation skin, isolated and named per the scratch design:
//! requests are built here from plain values, responses are reduced
//! here to plain values, and no JSON value ever crosses into core
//! types. It is the ONLY module in the crate whose text contains
//! the protocol's name; the build record's audit grep proves the
//! rest of the tree is clean.
//!
//! Failure honesty: an unreachable endpoint, a missing model, and a
//! malformed reply are three different errors and are reported as
//! such — never collapsed into a fake success or a panic.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum InferError {
    /// The endpoint could not be reached at all.
    Unreachable(String),
    /// The endpoint answered, but not with the model we need.
    ModelAbsent(String),
    /// The endpoint answered something we cannot use.
    BadResponse(String),
    /// The endpoint returned an error body.
    Refused(String),
}

impl std::fmt::Display for InferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferError::Unreachable(m) => write!(f, "inference endpoint unreachable: {}", m),
            InferError::ModelAbsent(m) => write!(f, "model absent at inference endpoint: {}", m),
            InferError::BadResponse(m) => write!(f, "unusable inference response: {}", m),
            InferError::Refused(m) => write!(f, "inference endpoint refused: {}", m),
        }
    }
}

pub struct Infer {
    /// Base URL, e.g. http://127.0.0.1:11434 (no trailing slash).
    pub endpoint: String,
    /// Model name, e.g. gemma4:e4b.
    pub model: String,
}

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "gemma4:e4b";

impl Infer {
    /// From the environment: QEXO_OLLAMA_URL, QEXO_MODEL. (The
    /// lattice Config family will hold these as field scrolls when
    /// the Layer 2 map lands; env is the dev valve, as with
    /// QEXO_STATE.)
    pub fn from_env() -> Infer {
        let endpoint = std::env::var("QEXO_OLLAMA_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
        let model = std::env::var("QEXO_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Infer { endpoint: endpoint.trim_end_matches('/').to_string(), model }
    }

    pub fn new(endpoint: &str, model: &str) -> Infer {
        Infer {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    /// Model names the endpoint currently serves (GET /api/tags).
    pub fn tags(&self) -> Result<Vec<String>, InferError> {
        let body = self.http("GET", "/api/tags", None)?;
        Ok(all_string_fields(&body, "name"))
    }

    /// Confirm the endpoint is up AND carries our model.
    pub fn require_model(&self) -> Result<(), InferError> {
        let names = self.tags()?;
        if names.iter().any(|n| n == &self.model) {
            Ok(())
        } else {
            Err(InferError::ModelAbsent(format!(
                "{} not in /api/tags (endpoint serves: {})",
                self.model,
                if names.is_empty() { "nothing".to_string() } else { names.join(", ") }
            )))
        }
    }

    /// One chat exchange. `system` is the persona's boot text (a
    /// scroll's content, handed across the border as a plain string);
    /// `keep_alive` follows Ollama's convention ("30m" to hold the
    /// model resident, "0" to unload).
    pub fn chat(&self, system: &str, prompt: &str, keep_alive: &str) -> Result<String, InferError> {
        let body = format!(
            "{{\"model\":\"{}\",\"stream\":false,\"keep_alive\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}]}}",
            esc(&self.model),
            esc(keep_alive),
            esc(system),
            esc(prompt)
        );
        let resp = self.http("POST", "/api/chat", Some(&body))?;
        if let Some(err) = string_field(&resp, "error") {
            return Err(InferError::Refused(err));
        }
        // The reply text is message.content — the "content" string
        // that follows the "message" object; other content fields
        // (none in this API today) must not be mistaken for it.
        let after = match resp.find("\"message\"") {
            Some(i) => &resp[i..],
            None => &resp[..],
        };
        string_field(after, "content")
            .ok_or_else(|| InferError::BadResponse("no message.content in reply".to_string()))
    }

    /// Load the persona into local inference: a priming exchange
    /// with a long keep-alive, so the model stays resident with the
    /// persona's boot text as its system context.
    pub fn prime(&self, boot_text: &str) -> Result<String, InferError> {
        self.require_model()?;
        self.chat(boot_text, "State your name and readiness in one line.", "30m")
    }

    /// Unload the model (keep_alive 0). Best-effort by design: the
    /// caller records the session closed regardless; a dead endpoint
    /// holds nothing anyway.
    pub fn unload(&self) -> Result<(), InferError> {
        let body = format!(
            "{{\"model\":\"{}\",\"keep_alive\":\"0\",\"messages\":[]}}",
            esc(&self.model)
        );
        self.http("POST", "/api/chat", Some(&body)).map(|_| ())
    }

    // ---------------- the wire itself ----------------

    fn http(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, InferError> {
        let (host, port) = parse_host_port(&self.endpoint)?;
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| InferError::Unreachable(format!("{}: {}", addr, e)))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let mut req = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nConnection: close\r\n",
            method, path, addr
        );
        if let Some(b) = body {
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        req.push_str("\r\n");
        if let Some(b) = body {
            req.push_str(b);
        }
        stream
            .write_all(req.as_bytes())
            .map_err(|e| InferError::Unreachable(format!("write: {}", e)))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| InferError::Unreachable(format!("read: {}", e)))?;
        let text = String::from_utf8_lossy(&raw).to_string();
        let split = text
            .find("\r\n\r\n")
            .ok_or_else(|| InferError::BadResponse("no header/body split".to_string()))?;
        let head = &text[..split];
        let mut status = 0u32;
        if let Some(first) = head.lines().next() {
            status = first.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
        let body_bytes = &raw[(split + 4)..];
        let decoded;
        let body_text: &str = if head.to_lowercase().contains("transfer-encoding: chunked") {
            decoded = dechunk(body_bytes);
            match std::str::from_utf8(&decoded) {
                Ok(s) => s,
                Err(_) => return Err(InferError::BadResponse("chunked body not UTF-8".into())),
            }
        } else {
            match std::str::from_utf8(body_bytes) {
                Ok(s) => s,
                Err(_) => return Err(InferError::BadResponse("body not UTF-8".into())),
            }
        };
        if status == 200 {
            Ok(body_text.to_string())
        } else if status == 0 {
            Err(InferError::BadResponse("no HTTP status line".into()))
        } else {
            let msg = string_field(body_text, "error").unwrap_or_else(|| body_text.trim().chars().take(200).collect());
            Err(InferError::Refused(format!("HTTP {}: {}", status, msg)))
        }
    }
}

fn parse_host_port(endpoint: &str) -> Result<(String, u16), InferError> {
    let rest = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| InferError::BadResponse(format!("endpoint must be http://, got {}", endpoint)))?;
    let hostport = rest.split('/').next().unwrap_or("");
    match hostport.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| InferError::BadResponse(format!("bad port in {}", endpoint)))?;
            Ok((h.to_string(), port))
        }
        None => Ok((hostport.to_string(), 80)),
    }
}

fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        // chunk-size line
        let mut j = i;
        while j + 1 < data.len() && !(data[j] == b'\r' && data[j + 1] == b'\n') {
            j += 1;
        }
        if j + 1 >= data.len() {
            break;
        }
        let size_str = match std::str::from_utf8(&data[i..j]) {
            Ok(s) => s.trim().split(';').next().unwrap_or("0").to_string(),
            Err(_) => break,
        };
        let size = match usize::from_str_radix(&size_str, 16) {
            Ok(n) => n,
            Err(_) => break,
        };
        if size == 0 {
            break;
        }
        let start = j + 2;
        let end = (start + size).min(data.len());
        out.extend_from_slice(&data[start..end]);
        i = end + 2; // skip trailing CRLF
    }
    out
}

/// Escape a string for inclusion in a JSON string literal — the
/// write side of the border.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Read one string value out of a response body: the string after
/// `"key":`. Deliberately a scanner, not a parser — this module
/// reduces foreign responses to the few fields qexo consumes.
fn string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let mut rest = body;
    loop {
        let i = rest.find(&needle)?;
        let after = &rest[i + needle.len()..];
        let after = after.trim_start();
        if let Some(s) = after.strip_prefix('"') {
            return Some(unesc_until_quote(s));
        }
        rest = &rest[i + needle.len()..];
    }
}

fn all_string_fields(body: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{}\":", key);
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find(&needle) {
        let after = rest[i + needle.len()..].trim_start();
        if let Some(s) = after.strip_prefix('"') {
            out.push(unesc_until_quote(s));
            rest = &after[1 + unesc_len(s)..];
        } else {
            rest = &rest[i + needle.len()..];
        }
    }
    out
}

/// Length in bytes of the escaped string starting at `s` (which
/// begins just after the opening quote), up to its closing quote.
fn unesc_len(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    s.len()
}

fn unesc_until_quote(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(n) {
                            out.push(ch);
                        }
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as IoRead, Write as IoWrite};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A stub Ollama: canned /api/tags and /api/chat over a real
    /// socket, recording the last request body it received. This
    /// is how the edge is tested — never a real model (Will's
    /// standing rule for ranch nodes).
    fn stub_ollama() -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(String::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // read headers, then content-length body
                let mut header_end = None;
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                                header_end = Some(pos + 4);
                                let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                                let clen: usize = head
                                    .lines()
                                    .find_map(|l| {
                                        let l = l.to_lowercase();
                                        l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))
                                    })
                                    .unwrap_or(0);
                                if buf.len() >= pos + 4 + clen {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                *seen2.lock().unwrap() = req.clone();
                let body = if req.starts_with("GET /api/tags") {
                    "{\"models\":[{\"name\":\"gemma4:e4b\"},{\"name\":\"other:1b\"}]}"
                } else if req.starts_with("POST /api/chat") {
                    "{\"model\":\"gemma4:e4b\",\"message\":{\"role\":\"assistant\",\"content\":\"I am Fixture, ready.\"},\"done\":true}"
                } else {
                    "{\"error\":\"unknown route\"}"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                if header_end.is_none() {
                    break;
                }
            }
        });
        (format!("http://127.0.0.1:{}", port), seen)
    }

    fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn tags_and_chat_against_stub() {
        let (endpoint, seen) = stub_ollama();
        let inf = Infer::new(&endpoint, "gemma4:e4b");
        let names = inf.tags().unwrap();
        assert_eq!(names, vec!["gemma4:e4b".to_string(), "other:1b".to_string()]);
        inf.require_model().unwrap();
        let reply = inf.chat("BOOT TEXT for Fixture", "ping", "30m").unwrap();
        assert_eq!(reply, "I am Fixture, ready.");
        let req = seen.lock().unwrap().clone();
        assert!(req.contains("BOOT TEXT for Fixture"), "system text crossed the border: {}", req);
        assert!(req.contains("\\\"keep_alive\\\":\\\"30m\\\"" ) || req.contains("\"keep_alive\":\"30m\""), "keep_alive present: {}", req);
    }

    #[test]
    fn model_absent_is_its_own_error() {
        let (endpoint, _seen) = stub_ollama();
        let inf = Infer::new(&endpoint, "nosuch:99b");
        match inf.require_model() {
            Err(InferError::ModelAbsent(_)) => {}
            other => panic!("expected ModelAbsent, got {:?}", other),
        }
    }

    #[test]
    fn unreachable_is_its_own_error() {
        // Bind then drop a listener to get a certainly-closed port.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let inf = Infer::new(&format!("http://127.0.0.1:{}", port), "gemma4:e4b");
        match inf.tags() {
            Err(InferError::Unreachable(_)) => {}
            other => panic!("expected Unreachable, got {:?}", other),
        }
    }

    #[test]
    fn escaping_roundtrip() {
        let s = "a \"quoted\" \\ path\nnewline\ttab";
        let e = esc(s);
        assert_eq!(unesc_until_quote(&format!("{}\"", e)), s);
    }
}
