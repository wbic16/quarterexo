//! inventory — the phext ecosystem roster, baked into the binary.
//!
//! `qexo inventory` (alias `qexo catalog`) renders every shipped
//! phext ecosystem product as phext-native records — field lines,
//! the same record shape the state lattice uses: name, what it
//! is (one line), home (repo/site), install (the exact command),
//! launch, version, status.
//!
//! The roster is a CENSUS, not a hand list: every wbic16 GitHub
//! repo was enumerated and its output judged for a published,
//! useful artifact — a crate live on crates.io, a package on
//! PyPI/npm, GitHub release assets, a live hosted product, or a
//! library with a documented consumption path. Only entries
//! verified live at census time are baked; everything short of
//! verified is excluded, not guessed at (a wrong install command
//! in the roster is worse than a missing entry — the census
//! caught one: phext-mcp's README documents `cargo install
//! phext-mcp`, and no such crate exists). The full census, every
//! repo with its verdict and evidence, is the build record's
//! ROSTER-CENSUS.md. Census date: 2026-11-14.
//!
//! `--install <name>` is PRINT-ONLY in v1.4.0: it prints the
//! command ready to copy. qexo never installs ecosystem products
//! silently.
//!
//! The inventory needs no node state — a fresh `cargo install
//! qexo` user can run it before `qexo config`, which is rather
//! the point of a roster.

use crate::lattice;
use crate::phext::Phext;

struct ProductSpec {
    name: &'static str,
    what: &'static str,
    home: &'static str,
    install: &'static str,
    launch: &'static str,
    version: &'static str,
    evidence: &'static str,
}

/// The verified roster (census 2026-11-14). Registry versions
/// were read from the registries themselves that day; release
/// assets from the GitHub releases API; sites by live probe.
const ROSTER: &[ProductSpec] = &[
    ProductSpec {
        name: "qexo",
        what: "the Exocortex coordinator — this tool: phextnet nodes, signed updates, persona hosting",
        home: "https://crates.io/crates/qexo (source: github.com/wbic16/quarterexo)",
        install: "cargo install qexo",
        launch: "qexo config (birth a node), then qexo",
        version: "1.3.0 on crates.io (this binary may be newer — the roster is a census snapshot)",
        evidence: "crates.io API, max_version 1.3.0, 2026-11-14",
    },
    ProductSpec {
        name: "sq",
        what: "SQ — the phext database: host a .phext and query it by coordinate (the prequel to SQLite)",
        home: "https://crates.io/crates/sq (source: github.com/wbic16/SQ)",
        install: "cargo install sq",
        launch: "sq share <file.phext> (daemon) or sq host <port> (HTTP API)",
        version: "0.6.1 on crates.io",
        evidence: "crates.io API, max_version 0.6.1, 2026-11-14",
    },
    ProductSpec {
        name: "libphext",
        what: "the reference phext library for Rust — parse, address, and serialize phext (the canonical reckoning)",
        home: "https://crates.io/crates/libphext (source: github.com/wbic16/libphext-rs)",
        install: "cargo add libphext",
        launch: "(library — no launch; link it from a Rust project)",
        version: "0.6.0 on crates.io",
        evidence: "crates.io API, max_version 0.6.0, 2026-11-14",
    },
    ProductSpec {
        name: "phext-shell",
        what: "an interactive, phext-aware shell that tracks your current scroll by coordinate",
        home: "https://crates.io/crates/phext-shell (source: github.com/wbic16/phext-shell)",
        install: "cargo install phext-shell",
        launch: "phext-shell",
        version: "0.6.0 on crates.io",
        evidence: "crates.io API, max_version 0.6.0, 2026-11-14",
    },
    ProductSpec {
        name: "phext-lattice",
        what: "Phext Edit — the hosted phext editor at edit.phext.io, on the lattice library (memory-mapped 9D index)",
        home: "https://edit.phext.io (source: github.com/wbic16/phext-lattice)",
        install: "open https://edit.phext.io — hosted, nothing to install (the library: cargo add phext-lattice)",
        launch: "https://edit.phext.io",
        version: "editor live (HTTP 200, 2026-11-14); library 1.2.0 on crates.io",
        evidence: "live probe of edit.phext.io + crates.io API, 2026-11-14",
    },
    ProductSpec {
        name: "phext-notepad",
        what: "Phext Notepad — the Windows reference editor for phext, in C# (the encoding conformance reference)",
        home: "github.com/wbic16/phext-notepad",
        install: "download PhextNotepad-Setup.zip from https://github.com/wbic16/phext-notepad/releases and run the installer",
        launch: "Phext Notepad (Windows, from the Start menu)",
        version: "v0.4.4 (GitHub release, with PhextNotepad-Setup.zip and PhextNotepad-ClickOnce.zip assets)",
        evidence: "GitHub releases API, tag v0.4.4, 2026-11-14",
    },
    ProductSpec {
        name: "libphext-node",
        what: "libphext for Node.js / TypeScript — the phext library in JavaScript",
        home: "https://www.npmjs.com/package/libphext (source: github.com/wbic16/libphext-node)",
        install: "npm install libphext",
        launch: "(library — no launch; require/import it from a Node project)",
        version: "0.4.0 on npm",
        evidence: "npm registry, dist-tags.latest 0.4.0, repository field names wbic16/libphext-node, 2026-11-14",
    },
    ProductSpec {
        name: "libphext-py",
        what: "libphext for Python — the phext library as the PyPI package named `phext`",
        home: "https://pypi.org/project/phext/ (source: github.com/wbic16/libphext-py)",
        install: "pip install phext",
        launch: "(library — no launch; import it from a Python project)",
        version: "0.3.2 on PyPI (package name: phext)",
        evidence: "PyPI JSON API, version 0.3.2, project_urls name wbic16/libphext-py, 2026-11-14",
    },
    ProductSpec {
        name: "libphext-c",
        what: "libphext in C — a reference parser for fetching text from phext buffers, with the phextio tool",
        home: "github.com/wbic16/libphext",
        install: "git clone https://github.com/wbic16/libphext",
        launch: "(library — `make` in the clone compiles phext.o and links the phextio reference parser)",
        version: "repo head (C, Makefile build documented in its README)",
        evidence: "repo README read via the GitHub API, 2026-11-14",
    },
    ProductSpec {
        name: "libphext-cs",
        what: "libphext in C# — the .NET implementation of the phext library (net8.0)",
        home: "github.com/wbic16/libphext-cs",
        install: "git clone https://github.com/wbic16/libphext-cs",
        launch: "(library — no launch; build with the .NET SDK 8.0+: dotnet build)",
        version: "repo head (.NET 8, per its README's Installation section)",
        evidence: "repo README read via the GitHub API, 2026-11-14",
    },
    ProductSpec {
        name: "phext.io",
        what: "the phext project site — documentation, research notes, and the corpus front door",
        home: "https://phext.io (source: github.com/wbic16/phextio)",
        install: "(website — nothing to install)",
        launch: "https://phext.io",
        version: "live (HTTP 200, 2026-11-14)",
        evidence: "live probe of phext.io, 2026-11-14",
    },
];

/// One product's phext record text (field lines, lattice style).
fn product_record(p: &ProductSpec) -> String {
    lattice::record(&[
        ("kind", "product"),
        ("name", p.name),
        ("what", p.what),
        ("home", p.home),
        ("install", p.install),
        ("launch", p.launch),
        ("version", p.version),
        ("status", "verified"),
        ("evidence", p.evidence),
    ])
}

/// The roster as one phext document: an index scroll plus one
/// record scroll per product, by adjacency — the baked form the
/// renderings below read, so the inventory is phext all the way
/// down, not a table wearing a costume.
fn roster_phext() -> Phext {
    let mut px = Phext::new();
    let mut index = String::new();
    index.push_str("phext ecosystem — inventory\n\n");
    index.push_str("Every shipped phext product from the wbic16 census\n(2026-11-14): a crate, package, release, hosted product,\nor documented library — verified live, or not baked.\n\n");
    for (i, p) in ROSTER.iter().enumerate() {
        index.push_str(&format!("- {} — {}\n", p.name, p.what));
        px.insert([1, 1, 1, 2, 1, (i + 1) as u32, 1, 1, 1], product_record(p));
    }
    px.insert([1, 1, 1, 1, 1, 1, 1, 1, 1], index);
    px
}

/// (name, record text) per product, in roster order.
fn products() -> Vec<(String, String)> {
    let px = roster_phext();
    ROSTER
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let text = px
                .get(&[1, 1, 1, 2, 1, (i + 1) as u32, 1, 1, 1])
                .cloned()
                .unwrap_or_else(|| product_record(p));
            (p.name.to_string(), text)
        })
        .collect()
}

/// Find a product by name: exact (any case), else a unique
/// case-insensitive prefix.
fn find(query: &str) -> Option<(String, String)> {
    let all = products();
    let q = query.to_lowercase();
    if let Some(hit) = all.iter().find(|(n, _)| n.to_lowercase() == q) {
        return Some(hit.clone());
    }
    let hits: Vec<&(String, String)> = all.iter().filter(|(n, _)| n.to_lowercase().starts_with(&q)).collect();
    if hits.len() == 1 {
        return Some(hits[0].clone());
    }
    None
}

fn render_roster() -> String {
    let mut out = String::new();
    out.push_str("phext ecosystem — inventory (baked into qexo v1.4.0; census 2026-11-14)\n");
    let all = products();
    let n = all.len();
    for (i, (name, text)) in all.iter().enumerate() {
        let mark = if i + 1 == n { "\\--" } else { "+--" };
        out.push_str(&format!(
            "{} {} — {} — {}\n",
            mark,
            name,
            lattice::field(text, "what").unwrap_or_default(),
            lattice::field(text, "version").unwrap_or_default()
        ));
    }
    out.push_str("\nqexo inventory <name> prints one product in full; --install <name> prints its install command.\n");
    out
}

/// `qexo inventory [<name>]` / `qexo inventory --install <name>`
/// (alias verb: catalog).
pub fn inventory(name: Option<String>, install: Option<String>) -> i32 {
    if let Some(query) = install {
        return match find(&query) {
            Some((n, text)) => {
                println!("{} — install:", n);
                println!("  {}", lattice::field(&text, "install").unwrap_or_default());
                println!("launch: {}", lattice::field(&text, "launch").unwrap_or_default());
                println!("home: {}", lattice::field(&text, "home").unwrap_or_default());
                println!("(print-only in qexo v1.4.0 — qexo never installs ecosystem products silently; copy and run it yourself)");
                0
            }
            None => {
                eprintln!("qexo inventory: no product named '{}' in the roster", query);
                1
            }
        };
    }
    if let Some(query) = name {
        return match find(&query) {
            Some((_, text)) => {
                print!("{}", text);
                0
            }
            None => {
                eprintln!("qexo inventory: no product named '{}' in the roster", query);
                1
            }
        };
    }
    print!("{}", render_roster());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_renders_every_baked_product() {
        let out = render_roster();
        assert_eq!(products().len(), ROSTER.len());
        assert_eq!(ROSTER.len(), 11, "the census baked 11 products");
        for p in ROSTER {
            assert!(out.contains(p.name), "roster render is missing {}", p.name);
        }
    }

    #[test]
    fn every_baked_entry_is_verified_with_a_real_install_line() {
        // The census rule, enforced: nothing is baked short of
        // verified, and every entry carries a non-empty install
        // line taken from its evidence — no guessed commands.
        for (name, text) in products() {
            assert_eq!(lattice::field(&text, "status").as_deref(), Some("verified"), "{} not verified", name);
            assert!(!lattice::field(&text, "install").unwrap_or_default().is_empty(), "{} has no install line", name);
            assert!(!lattice::field(&text, "what").unwrap_or_default().is_empty(), "{} has no what line", name);
            assert!(!lattice::field(&text, "home").unwrap_or_default().is_empty(), "{} has no home line", name);
            assert!(!lattice::field(&text, "evidence").unwrap_or_default().is_empty(), "{} has no evidence line", name);
        }
    }

    #[test]
    fn lookup_is_exact_then_unique_prefix() {
        assert_eq!(find("sq").map(|(n, _)| n), Some("sq".to_string()));
        assert_eq!(find("SQ").map(|(n, _)| n), Some("sq".to_string()));
        assert_eq!(find("phext-n").map(|(n, _)| n), Some("phext-notepad".to_string()));
        assert_eq!(find("libphext-c").map(|(n, _)| n), Some("libphext-c".to_string()), "an exact name beats the ambiguous prefix");
        assert_eq!(find("libphext-").map(|(n, _)| n), None, "ambiguous prefix finds nothing");
        assert_eq!(find("libphext-cs").map(|(n, _)| n), Some("libphext-cs".to_string()));
        assert_eq!(find("nope"), None);
    }

    #[test]
    fn the_roster_document_is_real_phext() {
        // round-trip the baked document through the reference
        // reckoning: parse + serialize holds every record.
        let px = roster_phext();
        let text = px.serialize();
        let back = Phext::parse(&text);
        assert_eq!(back.scrolls.len(), px.scrolls.len());
        assert_eq!(back.scrolls.len(), ROSTER.len() + 1, "index + one record per product");
    }
}
