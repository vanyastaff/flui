#!/usr/bin/env bash
# Prints the exact locked version of a package from Cargo.lock. Single
# source of truth for "what version of <tool> does this workspace's lockfile
# pin" -- previously copy-pasted identically in justfile's `wasm-test`
# recipe, .github/workflows/ci.yml's `wasm-check` job, and
# .github/workflows/weekly.yml's `cli-live-build` job, all reading
# wasm-bindgen's locked version so the installed CLI tool matches exactly
# (wasm-bindgen-test-runner refuses to start otherwise).
#
# Usage: bash scripts/locked-version.sh <package-name>
set -euo pipefail

if [[ -z "${BASH_VERSINFO:-}" ]]; then
  echo "error: run this with bash, not sh" >&2
  exit 1
fi

if [[ $# -ne 1 || -z "$1" ]]; then
  echo "usage: locked-version.sh <package-name>" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LOCK_PATH="$root/Cargo.lock" PACKAGE_NAME="$1" python3 -c '
import os, sys, tomllib
with open(os.environ["LOCK_PATH"], "rb") as f:
    lock = tomllib.load(f)
name = os.environ["PACKAGE_NAME"]
for pkg in lock["package"]:
    if pkg["name"] == name:
        print(pkg["version"])
        sys.exit(0)
sys.exit(f"error: package {name!r} not found in Cargo.lock")
'
