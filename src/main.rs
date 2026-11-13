//! qexo — the Exocortex coordinator, phext-native from scratch.
//!
//! One binary; modules by concern: the format (phext), the seal
//! (xz), the hash, GPG mechanics (crypto), the inference edge
//! (infer), the plan's pure core (plan), doctor's two layers
//! (doctor), the state lattice and its Layer 2 addressing
//! (lattice), the state commands (core), the wiring gate
//! (wiring), and this CLI.

mod cli;
mod core;
mod crypto;
mod doctor;
#[allow(dead_code)]
mod hash;
mod infer;
mod lattice;
mod phext;
mod plan;
mod wiring;
mod xz;

use cli::Command;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(cli::parse(&args), &args);
    std::process::exit(code);
}

fn run(cmd: Command, args: &[String]) -> i32 {
    match cmd {
        Command::Help => {
            print!("{}", cli::help_text());
            0
        }
        Command::Version => {
            println!("qexo v1.0.2 (Cargo 1.0.2)");
            0
        }
        Command::WiringCheck(dir) => {
            let dir = dir.unwrap_or_else(|| "src".to_string());
            match wiring::check_dir(std::path::Path::new(&dir)) {
                Ok((orphaned, missing, n)) => {
                    if orphaned.is_empty() && missing.is_empty() {
                        println!("wiring ok: {} module files, every one declared, every declaration filed", n);
                        0
                    } else {
                        for o in &orphaned {
                            eprintln!("orphaned (file without declaration): src/{}.rs", o);
                        }
                        for m in &missing {
                            eprintln!("missing (declaration without file): {}", m);
                        }
                        1
                    }
                }
                Err(e) => {
                    eprintln!("wiring-check: {}", e);
                    2
                }
            }
        }
        Command::FileOp(_) => phext_file_ops(args),
        Command::Doctor { fix } => {
            // Both layers: filesystem facts, then — when the
            // state is born — the lattice neighborhoods.
            let dir = lattice::state_dir();
            let file = lattice::state_path();
            if fix {
                let mut done = doctor::fix(&dir, &file);
                if let Ok(Some(mut lat)) = lattice::Lattice::load() {
                    done.extend(doctor::fix_lattice(&mut lat));
                    if let Err(e) = lat.save() {
                        eprintln!("doctor: cannot save repaired state: {}", e);
                        return 1;
                    }
                }
                for d in &done {
                    println!("fixed: {}", d);
                }
                if done.is_empty() {
                    println!("nothing to fix");
                }
            }
            let mut report = doctor::check(&dir, &file);
            if let Ok(Some(lat)) = lattice::Lattice::load() {
                report.findings.extend(doctor::check_lattice(&lat).findings);
            }
            print!("{}", report.render());
            if report.ok() { 0 } else { 1 }
        }
        Command::Unknown(word) => {
            eprintln!("qexo: unknown command '{}' — qexo --help lists the surface", word);
            2
        }
        Command::Bare | Command::Plan => core::plan_cmd(),
        Command::Config => core::config(),
        Command::List => core::list(),
        Command::Register(args) => core::register(&args),
        Command::Host { persona, boot } => core::host(persona, boot),
        Command::Unhost(name) => core::unhost(&name),
        Command::Message { persona, text } => core::message(&persona, &text),
        Command::Status => core::status(),
        Command::Serve(port) => core::serve(port.unwrap_or(8484)),
    }
}

/// File-level phext operations (the libphext surface in the CLI):
/// parse/explode/implode/seal/unseal/select/get operate on files,
/// not node state. Carried from the old line's implementation
/// (the from-scratch brief's named reuse), verbatim in behavior.
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
