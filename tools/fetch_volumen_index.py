#!/usr/bin/env python3
"""Fetch the canonical Volumen index for the qexo bake.

The canonical Volumen index is wbic16/human@exo personas/00-index.md.
This script fetches that exact file at an explicit commit (or the
current exo HEAD), verifies the git blob SHA-1 when a pin is given,
and writes the raw bytes to the bake output path.

Usage: fetch_volumen_index.py <out-path> [expected-blob-sha1] [commit]

With no pin, it fetches exo's current HEAD and prints the blob id it
landed (the caller records that pin in the bake manifest — fresh HEAD
is the default ref rule per DESIGN-V3 section 14.4, and the landed
pin is what makes the bake auditable).

Exit codes are loud on purpose: any failure — network, missing path,
blob mismatch — exits non-zero and the bake must not proceed. There
is deliberately NO fallback to any local index file: a number
derived from anywhere else is not the canonical index's number.
"""
import subprocess
import sys

REPO = "git@github.com:wbic16/human.git"
PATH = "personas/00-index.md"


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: fetch_volumen_index.py <out-path> [blob-sha1] [commit]", file=sys.stderr)
        return 2
    out_path = sys.argv[1]
    expected_blob = sys.argv[2] if len(sys.argv) > 2 else None
    commit = sys.argv[3] if len(sys.argv) > 3 else None

    clone_dir = "/tmp/qexo-volumen-fetch"
    subprocess.run(["rm", "-rf", clone_dir], check=True)
    r = subprocess.run(
        ["git", "clone", "-q", "--depth", "50", "--branch", "exo", REPO, clone_dir],
    )
    if r.returncode != 0:
        print("FETCH FAILED: could not clone wbic16/human@exo", file=sys.stderr)
        return 1
    if commit:
        r = subprocess.run(["git", "-C", clone_dir, "checkout", "-q", commit])
        if r.returncode != 0:
            print(f"FETCH FAILED: commit {commit} not reachable", file=sys.stderr)
            return 1
    blob = subprocess.run(
        ["git", "-C", clone_dir, "rev-parse", f"HEAD:{PATH}"],
        capture_output=True, text=True,
    )
    if blob.returncode != 0:
        print(f"FETCH FAILED: {PATH} not present at HEAD of exo", file=sys.stderr)
        return 1
    blob = blob.stdout.strip()
    if expected_blob and blob != expected_blob:
        print(
            f"BLOB MISMATCH: {PATH} is blob {blob}, expected {expected_blob} — bake aborted",
            file=sys.stderr,
        )
        return 1
    head = subprocess.run(
        ["git", "-C", clone_dir, "rev-parse", "HEAD"], capture_output=True, text=True
    ).stdout.strip()
    with open(f"{clone_dir}/{PATH}", "rb") as fh:
        data = fh.read()
    with open(out_path, "wb") as fh:
        fh.write(data)
    print(f"volumen index: {PATH} @ {head} blob {blob}, {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
