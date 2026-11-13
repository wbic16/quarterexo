//! The wiring gate (v3.2): every module file under `src/` must be
//! declared in `src/main.rs`, and every declaration must have a file.
//!
//! Motivating failure (2026-11-09/10 nightly ships, repaired by the
//! v3.1 rewire on 2026-11-11): the v2.2/v2.3 builds were assembled
//! from a staging tree that predated the v3 merge, and their
//! `src/main.rs` — a standalone root with almost no `mod`
//! declarations — overwrote the modular root on exo. All sixteen
//! v3 module files survived in the tree, orphaned: present,
//! uncompiled, untested. Nothing in the ship process compared the
//! tree's files against the root's declarations, so the loss was
//! silent until an audit found it two days later.
//!
//! This module is that comparison, as a pure function over the
//! root's text plus the file stems, so the exact failure shape is
//! testable: a root that declares only a few modules against a
//! full file set reports every other file as orphaned.
//!
//! CLI form (main.rs, following the `bake --check` precedent for
//! file-level local checks): `qexo wiring-check [src-dir]`
//! (default `src`). Exit 0 when the wiring is exact, 1 when any
//! file is orphaned or any declaration is missing its file.

use std::path::Path;

/// Module names declared by `mod <name>;` (or `pub mod <name>;`)
/// lines in a crate root's text, in declaration order.
pub fn declared_modules(main_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in main_text.lines() {
        let t = line.trim();
        let rest = t.strip_prefix("pub mod ").or_else(|| t.strip_prefix("mod "));
        if let Some(rest) = rest {
            if let Some(name) = rest.strip_suffix(';') {
                let name = name.trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// Compare a root's declarations against the module files present.
/// Returns (orphaned, missing): files with no declaration, and
/// declarations with no file. Both empty means the wiring is exact.
/// `main` is the root itself and is never expected as a declaration.
pub fn check(main_text: &str, file_stems: &[String]) -> (Vec<String>, Vec<String>) {
    let declared = declared_modules(main_text);
    let mut orphaned: Vec<String> = file_stems
        .iter()
        .filter(|s| s.as_str() != "main" && !declared.contains(s))
        .cloned()
        .collect();
    let mut missing: Vec<String> = declared
        .iter()
        .filter(|d| !file_stems.contains(d))
        .cloned()
        .collect();
    orphaned.sort();
    missing.sort();
    (orphaned, missing)
}

/// Run the gate against a source directory: read its `main.rs`,
/// list its `*.rs` stems, and apply `check`.
pub fn check_dir(src_dir: &Path) -> Result<(Vec<String>, Vec<String>, usize), String> {
    let main_path = src_dir.join("main.rs");
    let main_text = std::fs::read_to_string(&main_path)
        .map_err(|e| format!("{}: {}", main_path.display(), e))?;
    let mut stems: Vec<String> = Vec::new();
    let rd = std::fs::read_dir(src_dir).map_err(|e| format!("{}: {}", src_dir.display(), e))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("{}: {}", src_dir.display(), e))?;
        let p = ent.path();
        if p.extension().and_then(|x| x.to_str()) == Some("rs") {
            if let Some(stem) = p.file_stem().and_then(|x| x.to_str()) {
                stems.push(stem.to_string());
            }
        }
    }
    let n = stems.len();
    let (orphaned, missing) = check(&main_text, &stems);
    Ok((orphaned, missing, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exact_wiring_is_clean() {
        let main = "mod cli;\nmod core;\nmod plan;\nfn main() {}\n";
        let (o, m) = check(main, &stems(&["main", "cli", "core", "plan"]));
        assert!(o.is_empty() && m.is_empty());
    }

    #[test]
    fn standalone_root_orphans_the_v3_modules() {
        // The v2.2/v2.3 failure shape: a standalone root declaring
        // almost nothing, pushed over a tree carrying all the v3
        // module files. Every undeclared file must be named.
        // (Fixture adapted for the scratch line: the old fixture's
        // declared set named a module this crate no longer has;
        // the shape — one declaration against the full set — and
        // the orphan count are unchanged.)
        let main = "mod plan;\nfn main() {}\n";
        let files = stems(&[
            "main", "cli", "coord", "core", "crypto", "hash", "http", "mcp", "personas",
            "phext", "plan", "state", "store", "tools", "vol", "xz",
        ]);
        let (o, m) = check(main, &files);
        assert_eq!(o.len(), 14);
        assert!(o.contains(&"core".to_string()));
        assert!(o.contains(&"tools".to_string()));
        assert!(!o.contains(&"plan".to_string()));
        assert!(m.is_empty());
    }

    #[test]
    fn declaration_without_file_is_missing() {
        let main = "mod cli;\nmod ghost;\n";
        let (o, m) = check(main, &stems(&["main", "cli"]));
        assert!(o.is_empty());
        assert_eq!(m, vec!["ghost".to_string()]);
    }

    #[test]
    fn declared_modules_parsing() {
        let main = "// mod commented;\nmod cli;\npub mod core;\n  mod plan;  \nmod not_a_decl\nlet x = \"mod fake;\";\n";
        assert_eq!(declared_modules(main), vec!["cli".to_string(), "core".to_string(), "plan".to_string()]);
    }
}
