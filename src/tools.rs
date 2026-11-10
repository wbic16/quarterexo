//! Tool catalog — the installable exocortex tool set (docs/API-V1.md
//! row 90: phext-lattice, the exocortical tools, Rust, phcc, phcpp,
//! phst, …).
//!
//! Introduced 2026-11-09 with the exovim entry, at Will's direction
//! ("qexo is your exocortical buddy — it knows all the pitfalls we've
//! gone through"). Before this module the install route recorded any
//! tool name it was handed, but qexo held no per-tool knowledge: no
//! source repo, no system-package set, no build recipe, no pitfalls.
//! This module is that knowledge, in code, one static entry per tool.
//!
//! An entry is a *recipe*, not a vendored copy: the tool's source
//! stays in its own repo and qexo installs it from there, the same
//! way it has always treated phext-lattice and phcc. Entries whose
//! recipe has not been codified yet carry an empty `build` and say so
//! in `pitfalls` — the install act is still recorded by route_install
//! and delivery rides the packaging channel, exactly as before.
//!
//! The exovim entry is the first complete one: source repo, the exact
//! Ubuntu package set its build needs, the system-compiler recipe
//! that leaves the user an `exovim` command, and the phcc variant
//! with the CMake pitfall recorded in full (see `pitfalls`).

use crate::json::Json;

pub struct ToolSpec {
    pub name: &'static str,
    pub repo: &'static str,
    pub about: &'static str,
    /// The command the user runs once installed; "" = not yet codified.
    pub binary: &'static str,
    /// Ubuntu (apt) packages the build needs on the node.
    pub apt: &'static [&'static str],
    /// Build/install recipe, one shell step per line; empty = the
    /// recipe has not been codified yet (see `pitfalls`).
    pub build: &'static [&'static str],
    /// Hard-won operational knowledge: variants, workarounds, limits.
    pub pitfalls: &'static [&'static str],
}

/// exovim's Ubuntu package set: the upstream BUILD.md prerequisites
/// (ninja-build gettext cmake curl build-essential git unzip) plus
/// the configure prerequisites proven in the 2026-11-08 phcc ladder
/// (pkg-config, lua5.1, liblua5.1-0-dev — CMake's configure is a hard
/// failure without a Lua interpreter, and PREFER_LUA needs the 5.1
/// headers when the system Lua is used).
pub const EXOVIM_APT: &[&str] = &[
    "ninja-build",
    "gettext",
    "cmake",
    "curl",
    "build-essential",
    "git",
    "unzip",
    "pkg-config",
    "lua5.1",
    "liblua5.1-0-dev",
];

const CATALOG: &[ToolSpec] = &[
    ToolSpec {
        name: "phext-lattice",
        repo: "https://github.com/wbic16/phext-lattice",
        about: "the phext lattice: the editor/hosting layer behind edit.phext.io; SQ hosts its phexts",
        binary: "",
        apt: &["build-essential", "git"],
        build: &[],
        pitfalls: &[
            "build recipe not yet codified in qexo — the install is recorded and delivery rides the packaging channel",
            "needs the Rust toolchain (see the rust entry) and SQ on the node; SQ runs on ranch nodes, not on the coordinating VM",
        ],
    },
    ToolSpec {
        name: "rust",
        repo: "https://rustup.rs",
        about: "the Rust toolchain (rustup) — precondition for every cargo-built tool in this catalog, qexo included",
        binary: "cargo",
        apt: &["build-essential", "curl", "git"],
        build: &["curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"],
        pitfalls: &[
            "ranch nodes carry cargo/rustc 1.94 via rustup already; a node without it cannot build qexo, phext-lattice, or phst",
        ],
    },
    ToolSpec {
        name: "phcc",
        repo: "https://github.com/wbic16/phcc",
        about: "phcc — the phext-carried TinyCC modernisation: a provably-correct C compiler whose sources live as phext scrolls (PATCHES.md is the divergence record)",
        binary: "tcc",
        apt: &["build-essential", "git"],
        build: &[
            "git clone https://github.com/wbic16/phcc.git",
            "cd phcc && ./configure && make",
        ],
        pitfalls: &[
            "self-host receipt: phcc builds itself, and the phcc-built phcc passes make test",
            "an uninstalled build-tree phcc needs -B<tree> when invoked as a compiler (the exovim phcc variant relies on this)",
        ],
    },
    ToolSpec {
        name: "phcpp",
        repo: "https://github.com/wbic16/phcpp",
        about: "phcpp — phext support for C++23 on the phcc baseline: C++ lowers to phcc scrolls, handed to phcc with the C++ riding as commentary",
        binary: "",
        apt: &["build-essential", "git"],
        build: &[],
        pitfalls: &[
            "depends on phcc being installed first — it hands its lowered scrolls to phcc",
            "build recipe not yet codified in qexo — the install is recorded and delivery rides the packaging channel",
        ],
    },
    ToolSpec {
        name: "phst",
        repo: "https://github.com/wbic16/phst",
        about: "phst — the phext fork of Rust: phext-native Rust",
        binary: "",
        apt: &["build-essential", "git"],
        build: &[],
        pitfalls: &[
            "depends on the Rust toolchain (see the rust entry)",
            "build recipe not yet codified in qexo — the install is recorded and delivery rides the packaging channel",
        ],
    },
    ToolSpec {
        name: "exovim",
        repo: "https://github.com/wbic16/neovim",
        about: "exovim — Will's phext-support Neovim fork (wbic16/neovim, master). The phext plugin merged 2026-11-09 (merge 1c2b15dc): scroll-as-buffer phext navigation in a modal editor — coordinate as address, dimensional + sparse-populated motion, :PhextToc outline, byte-identical round-trip. Design and honest limits live in the repo's PHEXT.md; the fork's divergence from upstream neovim is one additive Lua patch set (PATCHES.md).",
        binary: "exovim",
        apt: EXOVIM_APT,
        build: &[
            "git clone https://github.com/wbic16/neovim.git exovim && cd exovim",
            "make CMAKE_BUILD_TYPE=RelWithDebInfo",
            "cmake --install build --prefix <prefix>",
            "ln -sf nvim <prefix>/bin/exovim",
        ],
        pitfalls: &[
            "the install lays the standard Neovim tree down intact (bin/nvim + share/nvim runtime) and adds bin/exovim as a symlink to the same binary: Neovim locates its runtime relative to the executable, so the renamed entry point keeps the full runtime and the user simply runs `exovim`. Do not rename the tree or strip the runtime — the phext plugin lives in runtime/ and loads from it",
            "phcc build variant (verified end-to-end 2026-11-08 on aletheia-core, phcc tip bdf12df2, this fork's base c868e1ab): build the bundled deps with the SYSTEM compiler first — cmake -S cmake.deps -B .deps -G Ninja -DCMAKE_BUILD_TYPE=Release -DUSE_BUNDLED_LUA=ON && cmake --build .deps — then configure nvim itself with CC='<phcc>/tcc -B<phcc>' and -DPREFER_LUA=ON -DLUA_MATH_LIBRARY=/usr/lib/x86_64-linux-gnu/libm.so. Result: full compile + link with zero source changes to either codebase; binary NVIM v0.13.0-dev, sha256 47f583b0eb9120b89027c3e4580800b7cc66c8fb40dddee0cbab608b8c4259ce; the phext integration suite passes 37/37 inside the phcc-built editor",
            "why the LUA_MATH_LIBRARY pre-seed: CMake's FindLua resolves libm only via the multiarch directory, which it finds through CMAKE_LIBRARY_ARCHITECTURE — a variable the TinyCC compiler-ABI probe leaves empty. Without the pre-seed the literal token LUA_MATH_LIBRARY-NOTFOUND leaks into the final link line and the link fails at the last step. It is a CMake artefact, not a phcc defect; the value mirrors exactly what the same probe resolves under GCC on Ubuntu",
            "why PREFER_LUA=ON: it selects the bundled Lua 5.1.5 instead of LuaJIT. LuaJIT is hand-written assembly and is not a phcc target. This is also why lua5.1 + liblua5.1-0-dev + pkg-config are in the apt set: the configure is a hard failure without a Lua interpreter present",
            "deps caveat (ladder rung L2b, still open): at the verified rung the bundled deps (.deps, including libuv) are system-compiler-built and only LINKED by phcc. Compiling libuv itself under phcc is not yet done — libuv uses __atomic_* intrinsics phcc does not currently provide. Do not claim a whole-tree phcc build",
            "editing model, so nobody is surprised: a .phext opens as ONE scroll in a buffer and delimiter bytes never enter a buffer; emptying a scroll and saving deletes it; undo does not cross scroll visits. These are v1 semantics from the repo's PHEXT.md, not bugs to patch around",
        ],
    },
];

pub fn catalog() -> &'static [ToolSpec] {
    CATALOG
}

pub fn find(name: &str) -> Option<&'static ToolSpec> {
    CATALOG.iter().find(|t| t.name == name)
}

/// Is this apt package installed on this node? Shells to dpkg-query,
/// the same way doctor_report shells to its probes. When dpkg-query
/// cannot run at all (non-Debian node), the answer is `true` —
/// unknown is not "missing", and a preflight that cries wolf on a
/// non-apt node is worse than none.
fn apt_installed(pkg: &str) -> bool {
    match std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", pkg])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("install ok installed"),
        Err(_) => true,
    }
}

/// The subset of a tool's apt set that this node is missing — the
/// preflight a buddy owes you before a build dies twenty minutes in.
pub fn apt_missing(spec: &ToolSpec) -> Vec<&'static str> {
    spec.apt.iter().copied().filter(|p| !apt_installed(p)).collect()
}

fn str_arr(items: &[&str]) -> Json {
    Json::Arr(items.iter().map(|s| Json::s(s)).collect())
}

pub fn spec_json(spec: &ToolSpec) -> Json {
    Json::obj(vec![
        ("tool", Json::s(spec.name)),
        ("repo", Json::s(spec.repo)),
        ("about", Json::s(spec.about)),
        ("binary", Json::s(spec.binary)),
        ("apt", str_arr(spec.apt)),
        (
            "apt_missing",
            Json::Arr(apt_missing(spec).iter().map(|s| Json::s(s)).collect()),
        ),
        ("build", str_arr(spec.build)),
        ("pitfalls", str_arr(spec.pitfalls)),
    ])
}

pub fn catalog_json() -> Json {
    Json::Arr(catalog().iter().map(|t| spec_json(t)).collect())
}

/// One-line doctor summary across the whole catalog.
pub fn preflight_summary() -> String {
    let mut parts = Vec::new();
    for t in catalog() {
        let missing = apt_missing(t);
        if !missing.is_empty() {
            parts.push(format!("{} missing: {}", t.name, missing.join(", ")));
        }
    }
    if parts.is_empty() {
        "every catalog tool's apt set is present on this node".to_string()
    } else {
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_names_are_unique() {
        let mut names: Vec<&str> = CATALOG.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), CATALOG.len());
    }

    #[test]
    fn catalog_covers_the_api_row_90_tools() {
        // docs/API-V1.md row 90 names the install set; every one of
        // them must be a catalog entry or the row is a lie.
        for name in ["phext-lattice", "rust", "phcc", "phcpp", "phst", "exovim"] {
            assert!(find(name).is_some(), "catalog is missing {name}");
        }
    }

    #[test]
    fn exovim_entry_carries_the_exact_apt_set() {
        let spec = find("exovim").expect("exovim must be in the catalog");
        // BUILD.md prerequisites + the phcc ladder's configure
        // prerequisites, in the order Will's direction listed them.
        assert_eq!(
            spec.apt,
            &[
                "ninja-build",
                "gettext",
                "cmake",
                "curl",
                "build-essential",
                "git",
                "unzip",
                "pkg-config",
                "lua5.1",
                "liblua5.1-0-dev",
            ]
        );
        assert_eq!(spec.repo, "https://github.com/wbic16/neovim");
        assert_eq!(spec.binary, "exovim");
    }

    #[test]
    fn exovim_phcc_pitfall_is_recorded_in_full() {
        let spec = find("exovim").expect("exovim must be in the catalog");
        let joined = spec.pitfalls.join("\n");
        // The two configure requirements and the reason for the
        // pre-seed — a bare flag without its why is not the pitfall
        // knowledge qexo exists to carry.
        assert!(joined.contains("-DPREFER_LUA=ON"), "PREFER_LUA pitfall missing");
        assert!(
            joined.contains("-DLUA_MATH_LIBRARY=/usr/lib/x86_64-linux-gnu/libm.so"),
            "LUA_MATH_LIBRARY pre-seed missing"
        );
        assert!(
            joined.contains("CMAKE_LIBRARY_ARCHITECTURE"),
            "the why (CMAKE_LIBRARY_ARCHITECTURE) is missing"
        );
        assert!(
            joined.contains("LUA_MATH_LIBRARY-NOTFOUND"),
            "the failure token is missing"
        );
        // The honest deps caveat rides along.
        assert!(joined.contains("libuv"), "L2b libuv caveat missing");
    }

    #[test]
    fn exovim_build_recipe_ends_in_an_exovim_command() {
        let spec = find("exovim").expect("exovim must be in the catalog");
        let joined = spec.build.join("\n");
        assert!(joined.contains("wbic16/neovim"), "recipe must clone the fork");
        assert!(
            joined.contains("bin/exovim"),
            "recipe must wire the exovim command name"
        );
    }
}
