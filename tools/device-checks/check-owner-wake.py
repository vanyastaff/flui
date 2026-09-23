#!/usr/bin/env python3
"""Bound each native owner-wake case and require post-return external oracles."""
import argparse
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary")
parser.add_argument("cases", nargs="*", default=["worker", "live-worker", "reentrant", "quit-priority", "bootstrap-error"])
args = parser.parse_args()
if sys.platform != "darwin":
    parser.error("native owner-wake probe requires macOS and a GUI session")
for case in args.cases:
    result = subprocess.run([args.binary, case], capture_output=True, text=True, timeout=12, check=False)
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    required = ("OWNER_WAKE_RETURNED", f"OWNER_WAKE_CASE={case}", "OWNER_WAKE_PASS")
    if result.returncode or any(marker not in result.stdout for marker in required):
        raise SystemExit(f"FAIL {case}: native status {result.returncode}")
    print(f"PASS {case}")
