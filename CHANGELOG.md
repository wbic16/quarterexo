# qexo changelog — the scratch line

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
