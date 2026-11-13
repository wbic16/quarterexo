# qexo changelog — the scratch line

## 1.0.4 (v1.0.4) — built and gated, 2026-11-12

The disk probe declared POSIX on every platform. probe_disk
read its volume through an unconditional `extern "C" statvfs`
— a symbol Windows does not have — so the Windows build of
v1.0.3 failed at link time with `unresolved external symbol
statvfs`, reported by Will from exo 1b819c2. The unix build
linked fine, and the v1.0.1 gate set could not have caught
it: those gates ran `cargo check` for the Windows targets,
and check never links, so an unsatisfiable extern sails
through. The gate set now includes a full Windows link
(`cargo build --target x86_64-pc-windows-gnu`, producing
qexo.exe), not just checks.

The volume read is now one function per platform, behind
`volume_at(root) -> Option<Volume>`: unix keeps the statvfs
read with identical semantics (total = f_blocks, used =
f_blocks − f_bfree, free = f_bavail — df's Used/Avail);
Windows reads the volume through kernel32
GetDiskFreeSpaceExW, mapped into the same terms (used =
total − total-free-bytes; free = bytes available to the
caller, the quota-aware f_bavail analog), so the gate cannot
tell which platform measured; any other platform — and any
failure — yields no volume, which plan already tolerates
(every consumer reads the volume through Option). The
mapping is a pure function, unit-tested on every platform.
The file-count half of the probe is unchanged, as is all
unix behavior. An audit of the rest of src/ found no other
unconditional unix extern: doctor's kernel32 block is
cfg(windows) (v1.0.1), the PermissionsExt uses sit inside
cfg(unix) blocks, and the /proc reads (hostname, meminfo,
loadavg) are ordinary file reads with graceful fallbacks —
on Windows they simply find nothing, which is a snapshot
that reads empty, not a build that fails.

## 1.0.3 (v1.0.3) — built and gated, 2026-11-12

The forecast kept the old VM ceilings after the gate moved.
v1.0.2 re-based the disk gate on the volume a node stands on —
a volume lens, a quota lens only where configured — but the
forecast gate was left untouched, still projecting crossings of
the legacy absolute levels: byte AMBER at 95 GB, byte RED at
102 GB. Every ranch volume already sits past 95 GB used, so on
the 2026-11-12 fleet rollout the second plan read on any node
flipped its headline to "AMBER — FORECAST AMBER — byte AMBER
(95 GB) projected in 0.0 days" over a gate that was GREEN with
66–92% of the volume free. A forecast that contradicts the gate
it forecasts is a bug, and this corrects it.

The forecast now projects the crossings of the same lenses the
gate reads, from the same config record: the volume lens's
levels are the used amounts at which this volume's free
fraction crosses the configured AMBER/RED percentages; the
quota lens contributes crossings only on nodes that have it on,
and they are named as quota crossings in the node's own
numbers; the file lens reads its thresholds from the config
record as before. The hardcoded 95/102 levels are deleted from
the forecast path — the only place those numbers remain is as
the quota lens's config defaults, off by default.

Two projection rules changed with the levels, both forced by
the same evidence. A crossing the current reading has already
reached is the gate's verdict, not a forecast, and is no longer
projected at 0.0 days. And projections are reported only
within a one-year horizon: a linear rate is a short-horizon
instrument, and a crossing tens of thousands of days out (the
ranch case: ~2,900 GB away at +0.06 GB/day) is reported as no
crossing rather than lent a day count it has not earned. The
old forecast tests that asserted the legacy levels were
rewritten to the lens semantics; the ported file-growth
scenario (+59,021 files/day, file AMBER ~1.4 days out, forecast
escalation) passes unchanged, because the file lens never moved.

## 1.0.2 (v1.0.2) — built and gated, 2026-11-12

The disk gate judged every node by the Muse VM's quota. Its
absolute thresholds (AMBER at 95 GB used, RED at 102 GB) were
calibrated to that one machine's staging quota — the 2026-11-08
EDQUOT failure — and ported value-for-value through every line
since, while the probe measured used bytes on whatever volume
it happened to stand on. Run on aletheia-core, the gate read
RED at 809 GB used on a 3.6 TB volume with 2.7 TB free and held
every placement on a healthy machine.

Now the gate measures the volume it stands on. The probe returns
the volume's total/used/free bytes (statvfs, as before, but the
probe no longer carries verdicts); the gate reads through two
lenses — a volume lens (AMBER under 25% free, RED under 10%
free) and a quota lens (the old absolute ceiling, on only where
a node configures it, because the VM's quota is a real ceiling
on that machine and nowhere else) — plus the file lens, which
now counts files under a named staging root and says so. Every
threshold lives in the node's config record in state.phext,
seeded visibly by `qexo config`: volume_lens,
disk_gate_free_amber_pct, disk_gate_free_red_pct, quota_lens,
quota_amber_gb, quota_red_gb, staging_root, file_amber,
file_red. Absent keys mean the defaults (volume lens only).

The plan renders the volume in GiB — the units df speaks,
labeled — names the measured root and the counted root, and
states which lens fired and the numbers it fired on. The HOLD
copy names the measured root instead of "the VM" wherever the
binary runs. The level recorded with each telemetry observation
is the gate's own evaluation under the node's configuration;
the old mechanism of an external watcher's level word forcing
the gate is retired (the series is qexo's own now). Forecast
gate behavior is unchanged. Gates: the decisive trio as unit
tests (ranch volume GREEN at the incident's own numbers; VM
ceiling AMBER/RED by configuration alone; a 5%-free volume RED
regardless of absolute size), the ported battery, cargo test,
release build, wiring check — all on aletheia-core. Stacked on
the v1.0.1 doctor slice (PR #9); the base retargets to exo when
that merges.

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
