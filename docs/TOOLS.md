# The tool catalog — what qexo can install, and what it knows about each tool

`src/tools.rs` is qexo's catalog of installable exocortex tools — the
set named in docs/API-V1.md row 90: **phext-lattice, Rust, phcc,
phcpp, phst, and exovim**. It exists because of how qexo is meant to
work: *qexo is your exocortical buddy — it knows all the pitfalls
we've gone through.* A buddy who can record that you installed a
tool but knows nothing about the tool is not a buddy.

The catalog was introduced 2026-11-09 with the exovim entry. Before
it, the install route recorded any tool name it was handed and qexo
held no per-tool knowledge at all. Each entry now carries:

| field | what it is |
|---|---|
| `repo` | where the tool's source lives (the catalog is recipes, not vendored copies) |
| `about` | what the tool is, in one breath |
| `binary` | the command the user runs once installed (`""` = not yet codified) |
| `apt` | the Ubuntu package set the build needs on the node |
| `build` | the recipe, one shell step per line (empty = not yet codified, and `pitfalls` says so) |
| `pitfalls` | the hard-won knowledge: variants, workarounds, honest limits |

## Where it surfaces

- `GET /v1/install` · `qexo install list` — the response now carries
  a `catalog` array: every entry, with an `apt_missing` per entry
  computed live on the node (dpkg-query). That is the preflight: you
  see what a build still needs *before* it dies twenty minutes in.
- `POST /v1/install` · `qexo install <tool>` — for a catalog tool, the
  recorded-install response now also carries the full catalog entry
  (repo, apt set + what's missing, recipe, pitfalls). Unknown tools
  are recorded exactly as before.
- `qexo doctor` — one finding summarises the whole catalog's apt
  preflight for the node.

## The exovim entry (the first complete one)

**exovim** is Will's name for the phext-support Neovim fork
(wbic16/neovim, master; the phext plugin merged 2026-11-09, merge
commit 1c2b15dc). Scroll-as-buffer phext navigation in a modal
editor; design and limits in that repo's `PHEXT.md`, divergence from
upstream recorded in its `PATCHES.md`.

**Ubuntu package set** (the upstream BUILD.md prerequisites plus the
configure prerequisites proven in the phcc ladder):

```
ninja-build gettext cmake curl build-essential git unzip
pkg-config lua5.1 liblua5.1-0-dev
```

**System-compiler recipe** (in the catalog entry): clone the fork,
`make CMAKE_BUILD_TYPE=RelWithDebInfo`, `cmake --install build
--prefix <prefix>`, then `ln -sf nvim <prefix>/bin/exovim`. The
standard Neovim tree stays intact — Neovim finds its runtime relative
to the executable, so the renamed entry point keeps the full runtime
(phext plugin included) and the user just runs `exovim`.

**The phcc variant, and the pitfall qexo exists to remember**
(verified end-to-end 2026-11-08 on aletheia-core, phcc tip bdf12df2):

1. Build the bundled deps with the *system* compiler first:
   `cmake -S cmake.deps -B .deps -G Ninja -DCMAKE_BUILD_TYPE=Release -DUSE_BUNDLED_LUA=ON && cmake --build .deps`
2. Configure nvim with `CC='<phcc>/tcc -B<phcc>'` **plus**
   `-DPREFER_LUA=ON -DLUA_MATH_LIBRARY=/usr/lib/x86_64-linux-gnu/libm.so`.

- `PREFER_LUA=ON` selects the bundled Lua 5.1.5; LuaJIT is
  hand-written assembly and is not a phcc target.
- The `LUA_MATH_LIBRARY` pre-seed is the pitfall: CMake's FindLua
  resolves libm only via the multiarch directory, which it finds
  through `CMAKE_LIBRARY_ARCHITECTURE` — a variable the TinyCC
  compiler-ABI probe leaves empty. Without the pre-seed, the literal
  token `LUA_MATH_LIBRARY-NOTFOUND` leaks into the final link line
  and the link fails at the last step. A CMake artefact, not a phcc
  defect; the value mirrors what the same probe resolves under GCC.
- Result of the verified rung: full compile + link, zero source
  changes to either codebase, binary sha256
  `47f583b0eb9120b89027c3e4580800b7cc66c8fb40dddee0cbab608b8c4259ce`,
  and the phext integration suite passes 37/37 inside the phcc-built
  editor.
- **Honest caveat (ladder rung L2b, still open):** at that rung the
  bundled deps — libuv included — are system-compiler-built and only
  *linked* by phcc. libuv itself uses `__atomic_*` intrinsics phcc
  does not yet provide. Nobody should claim a whole-tree phcc build
  until L2b lands.

## Adding a tool

One static `ToolSpec` in `src/tools.rs`, plus a test if the entry
carries a claim worth pinning (an exact package set, a pitfall flag).
If the recipe isn't codified yet, ship the entry anyway with an
empty `build` and say so in `pitfalls` — a named gap in the catalog
beats a tool qexo pretends not to know.
