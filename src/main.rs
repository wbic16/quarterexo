//! qexo — the Exocortex coordinator (DESIGN-V3).
//!
//! Bare `qexo` (no subcommand) emits the placement plan, exactly as
//! qexo v1/v2 did — the v1 invariant, the regression guard.
//! Modes (section 14.1): CLI (this file + cli.rs), API (`serve`),
//! MCP (`mcp`), Web (served by `serve` at /). One implementation.

mod cli;
mod coord;
mod core;
mod crypto;
mod hash;
mod http;
mod json;
mod mcp;
mod personas;
mod phext;
mod plan;
mod state;
mod store;
mod tools;
mod vol;
mod wiring;
mod xz;

use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(|s| s.as_str()).unwrap_or("");
    match first {
        "serve" => {
            let addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:8471".into());
            let ctx = match core::Ctx::open() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    eprintln!("qexo serve: {}", e);
                    std::process::exit(2);
                }
            };
            if let Err(e) = http::serve(ctx, &addr) {
                eprintln!("qexo serve: {}", e);
                std::process::exit(2);
            }
        }
        "mcp" => {
            let ctx = match core::Ctx::open() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("qexo mcp: {}", e);
                    std::process::exit(2);
                }
            };
            mcp::serve_stdio(&ctx);
        }
        "phext" if args.get(1).map(|s| s.as_str()) == Some("parse") || args.get(1).map(|s| s.as_str()) == Some("explode") || args.get(1).map(|s| s.as_str()) == Some("implode") || args.get(1).map(|s| s.as_str()) == Some("seal") || args.get(1).map(|s| s.as_str()) == Some("unseal") || args.get(1).map(|s| s.as_str()) == Some("select") || args.get(1).map(|s| s.as_str()) == Some("get") => {
            std::process::exit(phext_file_ops(&args[1..]));
        }
        "wiring-check" => {
            // `qexo wiring-check [src-dir]`: the v3.2 gate — every
            // src/*.rs module file must be declared in src/main.rs
            // and every declaration must have its file. Exit 1 names
            // the orphans. File-level local check, like `bake`.
            let dir = args.get(1).cloned().unwrap_or_else(|| "src".into());
            match wiring::check_dir(std::path::Path::new(&dir)) {
                Ok((orphaned, missing, n)) => {
                    for o in &orphaned {
                        println!("ORPHANED: src/{}.rs is present but not declared in main.rs — it does not compile", o);
                    }
                    for m in &missing {
                        println!("MISSING: mod {}; is declared in main.rs but src/{}.rs is absent", m, m);
                    }
                    if orphaned.is_empty() && missing.is_empty() {
                        println!("wiring ok: {} module files, all declared in main.rs, no missing files", n.saturating_sub(1));
                        std::process::exit(0);
                    }
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("qexo wiring-check: {}", e);
                    std::process::exit(2);
                }
            }
        }
        "bake" => {
            // `qexo bake --check`: verify the baked corpus hashes
            // against the manifest and the source.phext integrity.
            let v = store::corpus_verify();
            println!("{}", serde_pretty(&v));
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            std::process::exit(if ok { 0 } else { 1 });
        }
        "sync-diff" if args.len() >= 3 => {
            let old = std::fs::read_to_string(&args[1]).unwrap_or_default();
            let new = std::fs::read_to_string(&args[2]).unwrap_or_default();
            let (added, changed, removed) = core::sync_diff(&old, &new);
            for p in &added {
                println!("+ {}", p);
            }
            for p in &changed {
                println!("~ {}", p);
            }
            for p in &removed {
                println!("- {}", p);
            }
            println!("({} added, {} changed, {} removed)", added.len(), changed.len(), removed.len());
        }
        _ => {
            let ctx = match core::Ctx::open() {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    eprintln!("qexo: {}", e);
                    std::process::exit(2);
                }
            };
            let out = cli::run(&ctx, &args);
            let want_json = args.iter().any(|a| a == "--json");
            if want_json || !args.is_empty() {
                println!("{}", serde_pretty(&out.payload));
            } else {
                // bare qexo: the plan in its human (v1) rendering
                if let Some(p) = out.payload.get("payload") {
                    if let Some(tasks) = p.get("tasks") {
                        print!("{}", plan::plan_human(tasks));
                        return;
                    }
                }
                println!("{}", serde_pretty(&out.payload));
            }
            std::process::exit(if out.status < 400 { 0 } else { 1 });
        }
    }
}

fn serde_pretty(j: &json::Json) -> String {
    crate::json::pretty(j, 0)
}

/// File-level phext operations (the libphext surface in the CLI,
/// DESIGN-V3 section 14.1 API-note): parse/explode/implode/seal/
/// unseal/select/get operate on files, not node state, and so do not
/// take a caller block.
fn phext_file_ops(args: &[String]) -> i32 {
    let op = args[0].as_str();
    let path = args.get(1).cloned().unwrap_or_default();
    match op {
        "parse" | "implode" => {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            };
            let px = phext::Phext::parse(&text);
            println!("{}: {} scrolls, {} bytes serialized", path, px.scrolls.len(), px.serialize().len());
            for (c, body) in &px.scrolls {
                println!("  {} ({} bytes)", phext::fmt_coord(c), body.len());
            }
            0
        }
        "explode" => {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            };
            let px = phext::Phext::parse(&text);
            let dir = args.get(2).cloned().unwrap_or_else(|| "exploded".into());
            for (c, body) in &px.scrolls {
                let f = format!("{}/{}.scroll", dir, phext::fmt_coord(c).replace('/', "-"));
                let _ = std::fs::create_dir_all(&dir);
                if std::fs::write(&f, body).is_err() {
                    eprintln!("cannot write {}", f);
                    return 2;
                }
            }
            println!("exploded {} scrolls into {}/", px.scrolls.len(), dir);
            0
        }
        "seal" => {
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            };
            let sealed = xz::encode_stored(&bytes);
            let out = format!("{}.xz", path);
            match std::fs::write(&out, &sealed) {
                Ok(()) => {
                    println!("sealed {} -> {} ({} bytes; stored-form seal, compressible form interchangeable)", path, out, sealed.len());
                    0
                }
                Err(e) => {
                    eprintln!("{}", e);
                    2
                }
            }
        }
        "unseal" => {
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            };
            match xz::unseal(&bytes) {
                Ok(raw) => {
                    print!("{}", String::from_utf8_lossy(&raw));
                    0
                }
                Err(e) => {
                    eprintln!("unseal: {}", e);
                    1
                }
            }
        }
        "select" | "get" => {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            };
            let coord_s = args.get(2).cloned().unwrap_or_default();
            let px = phext::Phext::parse(&text);
            match phext::parse_coord(&coord_s).and_then(|c| px.get(&c)) {
                Some(body) => {
                    print!("{}", body);
                    0
                }
                None => {
                    eprintln!("no scroll at {}", coord_s);
                    1
                }
            }
        }
        _ => 2,
    }
}
