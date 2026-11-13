# qexo changelog — the scratch line

## 1.0.1 (v1.0.1) — built and gated, 2026-11-12

A Windows correctness fix for doctor, in two parts. The doctor
test encoded unix permission semantics as if they were
universal: it seeded the keyring at mode 0755 and asserted a
repair to 0700 — on Windows there are no permission modes to
read or set, so the test could never pass there. And behind the
test sat a real bug: lock liveness was judged by /proc/{pid},
which never exists on Windows, so a lock held by a *live* qexo
process would have been judged stale and doctor --fix would
have deleted it.

Now: liveness is probed per platform (unix /proc; Windows
OpenProcess + GetExitCodeProcess via kernel32, std-only), and
a pid whose liveness cannot be judged reads as alive — doctor
never deletes a lock it cannot judge. The keyring check on
non-unix platforms reports itself present and ok, with the
detail that permission modes are a unix concept and do not
apply there; the repair path claims nothing it cannot do.
The test asserts the platform-neutral repairs (missing
directory, stale lock) everywhere and the mode repair on unix
only, and asserts the not-applicable keyring finding exists
off-unix. No behavior change on unix. Gates: cargo test on
the ranch (unix), release build, wiring check, and a clean
`cargo check --tests` for the x86_64-pc-windows-gnu target;
Windows runtime execution is verified on Will's machine, not
the ranch.

## 1.0.0 (v1.0) — built and gated, 2026-11-12

A from-scratch line. The old line (0.x, in git history) kept its
node state in JSON documents and JSONL ledgers; JSON was the wrong
substrate for a phext-native tool, so this line replaces it rather
than decorating it: all node state lives in one phext file,
/var/lib/qexo/state.phext, and there is no JSON anywhere in qexo's
own machinery — the one foreign edge that speaks it (Ollama) is a
translation module, isolated and named.

There are no deployments of the old line, so nothing migrates:
state is born at `qexo config`, empty, with the neighborhood map
laid down.

Built in two passes: the map-free parts first (format, seal,
hash, GPG mechanics, the Ollama edge, the plan's pure core,
doctor's filesystem layer, the CLI, the wiring gate) with the
state commands held, then the addressing layer and the state
commands under the corrected Layer 2 map (docs/LAYER2.md v2 —
the first map was withdrawn the day it was issued because it
treated coordinates as a key-space instead of places). One
format fix landed on the way: the phext serializer now walks
the dead-reckoning path in delimiter stacks and the parser
holds the scroll-existence rule — the single-delimiter form
could not represent the homeland addresses at all. Gates:
29/29 cargo tests, 59/59 CLI battery, wiring ok, json confined
to the inference edge and one rule-stating comment.
