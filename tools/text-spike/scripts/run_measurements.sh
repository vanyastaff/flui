#!/bin/bash
# Runs the full 6-corpus x 2-backend x 2-size x 2-cache x 5-rep measurement
# series behind docs/research/text-stack-2026.md's main tables, and writes
# one JSON object per line to the output file given as $1.
#
# Usage (from the repo root, after `cargo build --release` in this crate):
#   tools/text-spike/scripts/run_measurements.sh tools/text-spike/results/measurements.jsonl
#
# Run on an otherwise-idle machine, single process at a time -- see this
# crate's docs and .rust-studio/specs/b7-text-stack-spike/plan.md for why
# that matters for the timing numbers. `parley` runs in its default
# `per-paragraph` mode; the report's separate single-`Layout` numbers (Risk
# section) come from ad hoc `--parley-mode single` invocations, not this
# script.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$SCRIPT_DIR/../target/release/text-spike"
OUT="${1:?usage: run_measurements.sh <output.jsonl>}"

if [ ! -x "$BIN" ]; then
  echo "error: $BIN not found -- run 'cargo build --release' in tools/text-spike first" >&2
  exit 1
fi

: > "$OUT"
CORPORA="latin arabic arabic_mixed cjk emoji_zwj devanagari"
BACKENDS="cosmic-text parley"
SIZES="paragraph large"
CACHES="cold warm"

for backend in $BACKENDS; do
  for corpus in $CORPORA; do
    for size in $SIZES; do
      for cache in $CACHES; do
        for rep in 1 2 3 4 5; do
          RESULT=$("$BIN" --backend "$backend" --corpus "$corpus" --size "$size" --cache "$cache")
          echo "$RESULT" | python3 -c "
import json, sys
d = json.load(sys.stdin)
d['rep'] = $rep
print(json.dumps(d))
" >>"$OUT"
        done
        echo "done: $backend $corpus $size $cache" >&2
      done
    done
  done
done
echo "SERIES_COMPLETE" >&2
