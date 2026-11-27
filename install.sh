#!/usr/bin/env bash
# qexo install — the one idempotent installer (v1.9.0).
#
# What it does, in order:
#   1. Resolve the binary: $QEXO_BIN, else $1, else the ranch
#      mirror (/mnt/mirrorborn/shared/qexo/qexo-<version>),
#      else ./target/release/qexo in a checkout, else a vendored
#      on-node build (cargo --offline against the fleet vendor
#      tree — ranch nodes cannot fetch crates.io directly).
#   2. Version gate: a pre-1.9.0 binary self-reports NOT-READY
#      and the install stops — a daemon-less binary is never
#      installed as the agent build.
#   3. Install the binary to /usr/local/bin/qexo.
#   4. The `qexo` group + state permissions that end the
#      "state not born" diversion: state dir 2775, state file
#      0664, gnupg 0750/0640, all group qexo; the invoking
#      operator joins the group. (Zero resolution-code
#      changes: the probe reports the system state writable,
#      and operator verbs work against the system node.)
#   5. State birth via `qexo config` (idempotent).
#   6. Units via `qexo config --agent` — the binary writes
#      qexo-serve.service + qexo-agent.service itself and
#      enables them; a restart follows so a re-install
#      actually swaps a running older binary.
#   7. `qexo doctor --fix`, then a HARD doctor-clean gate:
#      the install fails if doctor isn't clean.
#
# Upgrades after this install ride the signed phextnet
# release channel (`qexo update` / `qexo evolve`) as always.
# Re-running this script is safe: every step is idempotent.
#
# Run as root, or as a user with sudo. No secrets are read
# or written by this script.

set -euo pipefail

VERSION_WANT="1.9.0"
MIRROR_BIN="${QEXO_MIRROR:-/mnt/mirrorborn/shared/qexo/qexo-${VERSION_WANT}}"
VENDOR_DIR="${QEXO_VENDOR:-/mnt/mirrorborn/shared/qexo-vendor-2026-11-22/vendor}"
STATE_DIR="/var/lib/qexo"
BIN_DEST="/usr/local/bin/qexo"

say() { printf 'install: %s\n' "$*"; }
die() { printf 'install: FAILED — %s\n' "$*" >&2; exit 1; }

# --- 1. resolve the binary -------------------------------------

BIN=""
if [ -n "${QEXO_BIN:-}" ]; then
    BIN="$QEXO_BIN"
elif [ -n "${1:-}" ]; then
    BIN="$1"
elif [ -f "$MIRROR_BIN" ]; then
    BIN="$MIRROR_BIN"
elif [ -f "./target/release/qexo" ]; then
    BIN="./target/release/qexo"
elif [ -f "Cargo.toml" ] && [ -d "$VENDOR_DIR" ]; then
    say "no binary found — vendored on-node build (cargo --offline, vendor: $VENDOR_DIR)"
    cargo build --release --offline \
        --config "source.crates-io.replace-with=\"vendored-sources\"" \
        --config "source.vendored-sources.directory=\"$VENDOR_DIR\""
    BIN="./target/release/qexo"
else
    die "no binary: set QEXO_BIN, pass a path, place one at $MIRROR_BIN, or run from a checkout with the vendor tree at $VENDOR_DIR"
fi
[ -x "$BIN" ] || die "binary $BIN is not executable"
say "binary: $BIN"

# --- 2. version gate --------------------------------------------

VER_LINE="$("$BIN" --version)"
VER="$(printf '%s' "$VER_LINE" | sed -n 's/^qexo v\([0-9][0-9.]*\).*/\1/p')"
[ -n "$VER" ] || die "cannot parse a version from: $VER_LINE"
MAJOR="${VER%%.*}"
REST="${VER#*.}"
MINOR="${REST%%.*}"
if [ "$MAJOR" -lt 1 ] || { [ "$MAJOR" -eq 1 ] && [ "$MINOR" -lt 9 ]; }; then
    printf 'install: NOT-READY — %s is pre-1.9.0; it has no agent daemon. Refusing to install it as the agent build.\n' "$VER_LINE" >&2
    exit 3
fi
say "version: $VER_LINE (>= $VERSION_WANT)"

# --- sudo --------------------------------------------------------

if [ "$(id -u)" -eq 0 ]; then
    SUDO=""
else
    SUDO="sudo"
fi

# --- 3. install the binary --------------------------------------

if [ "$(readlink -f "$BIN")" != "$(readlink -f "$BIN_DEST" 2>/dev/null || echo none)" ]; then
    $SUDO install -m 0755 "$BIN" "$BIN_DEST"
    say "installed $BIN_DEST"
else
    say "$BIN_DEST already the resolved binary"
fi

# --- 4. group + permissions -------------------------------------

if ! getent group qexo >/dev/null; then
    $SUDO groupadd qexo
    say "group qexo created"
fi
OPERATOR="${SUDO_USER:-${USER:-}}"
if [ -n "$OPERATOR" ] && [ "$OPERATOR" != "root" ]; then
    $SUDO usermod -aG qexo "$OPERATOR"
    say "operator $OPERATOR added to group qexo (applies at next login)"
fi
$SUDO mkdir -p "$STATE_DIR"
$SUDO chgrp qexo "$STATE_DIR"
$SUDO chmod 2775 "$STATE_DIR"

# --- 5. birth ----------------------------------------------------

$SUDO "$BIN_DEST" config

# Permissions land after birth too: birth may have just created
# the state file and keyring root-owned.
for f in "$STATE_DIR"/state.phext*; do
    [ -e "$f" ] || continue
    $SUDO chgrp qexo "$f"
    $SUDO chmod 0664 "$f"
done
if [ -d "$STATE_DIR/gnupg" ]; then
    $SUDO chgrp -R qexo "$STATE_DIR/gnupg"
    $SUDO find "$STATE_DIR/gnupg" -type d -exec chmod 0750 {} +
    $SUDO find "$STATE_DIR/gnupg" -type f -exec chmod 0640 {} +
fi
say "state permissions: dir 2775, state 0664, gnupg 0750/0640, group qexo"

# --- 6. agent units ----------------------------------------------

$SUDO "$BIN_DEST" config --agent
if [ -d /run/systemd/system ]; then
    $SUDO systemctl restart qexo-serve.service qexo-agent.service
    say "services restarted on the new binary"
fi

# --- 7. doctor gate ----------------------------------------------

$SUDO "$BIN_DEST" doctor --fix
if ! $SUDO "$BIN_DEST" doctor; then
    die "qexo doctor is not clean after --fix; the install does not pass its own gate"
fi
say "doctor: clean"

# --- summary -------------------------------------------------------

"$BIN_DEST" --version
if [ -d /run/systemd/system ]; then
    systemctl is-active qexo-serve.service qexo-agent.service || true
fi
say "done — qexo $VER installed; serve + agent units enabled"
