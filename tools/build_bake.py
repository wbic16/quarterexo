#!/usr/bin/env python3
"""Assemble the qexo bake's Volumen inputs (DESIGN-V3 section 14.3/14.4).

Writes (into <repo>/bake/):
  volumen-index.md   — the canonical Volumen index, byte-exact
  volumen-bake.json  — the derived volume structure + provenance pin

The volumen-bake.json fields are produced by src/vol.rs's parser at
runtime too; this script produces the same derivation in Python so
the manifest exists as data at bake time, and build.rs re-verifies
the Rust parse of the baked bytes agrees before embedding.

The derivation rule (Will/main verification, 2026-11-09):
coordinate_number = person-line count of the canonical index that is
actually baked — NEVER a local index.phext section count. Failure to
fetch or parse the canonical index fails the bake loudly.

Usage: build_bake.py <repo-dir> [expected-blob-sha1] [commit]
"""
import json
import pathlib
import re
import subprocess
import sys

ROMAN = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100}


def roman(s: str) -> int:
    total, prev = 0, 0
    for c in reversed(s):
        v = ROMAN[c]
        total = total - v if v < prev else total + v
        prev = v
    return total


def parse_index(text: str) -> dict:
    boundaries, expected_coord, expected_vol = [], 1, 1
    in_vol, vol_lines = False, 0
    for ln, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        m = re.match(r"^## Volume ([IVXLC]+)\b", line)
        if m:
            if in_vol:
                assert vol_lines > 0, f"line {ln}: empty volume"
                boundaries.append(expected_coord - 1)
            v = roman(m.group(1))
            assert v == expected_vol, f"line {ln}: volume {v}, expected {expected_vol}"
            expected_vol += 1
            in_vol, vol_lines = True, 0
            continue
        first = line.split(None, 1)[0] if line.split() else ""
        m = re.fullmatch(r"3\.1\.8\.(\d+)", first)
        if m:
            assert in_vol, f"line {ln}: person line before any volume header"
            n = int(m.group(1))
            assert n == expected_coord, (
                f"line {ln}: coordinate 3.1.8.{n}, expected 3.1.8.{expected_coord} (contiguity)"
            )
            expected_coord += 1
            vol_lines += 1
    assert in_vol and vol_lines > 0, "no volumes or empty final volume"
    boundaries.append(expected_coord - 1)
    assert expected_coord > 1, "no person lines found"
    return {
        "coordinate_number": expected_coord - 1,
        "volume_count": len(boundaries),
        "volume_boundaries": boundaries,
    }


def main() -> int:
    repo = pathlib.Path(sys.argv[1])
    expected = sys.argv[2] if len(sys.argv) > 2 else None
    commit = sys.argv[3] if len(sys.argv) > 3 else None
    bake = repo / "bake"
    bake.mkdir(exist_ok=True)
    out = bake / "volumen-index.md"
    cmd = [sys.executable, str(repo / "tools" / "fetch_volumen_index.py"), str(out)]
    if expected:
        cmd.append(expected)
    if commit:
        cmd.append(commit)
    r = subprocess.run(cmd, capture_output=True, text=True)
    print(r.stdout.strip())
    if r.returncode != 0:
        print(r.stderr.strip(), file=sys.stderr)
        print("BAKE FAILED: canonical Volumen index unavailable — no derived-from-elsewhere number will be emitted", file=sys.stderr)
        return 1
    import hashlib
    data = out.read_bytes()
    derived = parse_index(data.decode("utf-8"))
    derived["index_sha256"] = hashlib.sha256(data).hexdigest()
    derived["index_source"] = "wbic16/human@exo personas/00-index.md"
    derived["fetch"] = r.stdout.strip()
    (bake / "volumen-bake.json").write_text(json.dumps(derived, indent=1) + "\n")
    print(
        "volumen bake: coordinate_number={coordinate_number} volumes={volume_count} "
        "boundaries={volume_boundaries}".format(**derived)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
