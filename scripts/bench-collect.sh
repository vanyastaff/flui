#!/usr/bin/env bash
# Runs every criterion bench target in the workspace and saves the numbers
# under a named baseline (target/criterion/<bench>/<baseline>).
#
# Why not plain `cargo bench --workspace -- --save-baseline X`: cargo's bench
# target selection includes every lib/test target whose default `bench = true`
# flag is set, and those run under libtest's harness, which rejects criterion
# CLI flags ("Unrecognized option: 'save-baseline'"). Enumerating the real
# `[[bench]]` targets (harness = false, criterion) sidesteps that without
# sprinkling `bench = false` across two dozen manifests.
#
# Targets with `required-features` (the GPU readback benches) are skipped:
# explicitly selecting such a target without its features is a hard cargo
# error, and this script runs in GPU-less environments.
#
# Usage: scripts/bench-collect.sh <baseline-name>
set -euo pipefail

baseline="${1:?usage: bench-collect.sh <baseline-name>}"

targets=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | .name as $p | .targets[]
             | select((.kind | index("bench")) and ((.["required-features"] // []) | length == 0))
             | "\($p) \(.name)"')

if [[ -z "$targets" ]]; then
    echo "error: no bench targets found" >&2
    exit 1
fi

skipped=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | .name as $p | .targets[]
             | select((.kind | index("bench")) and ((.["required-features"] // []) | length > 0))
             | "\($p) \(.name) (required-features)"')
if [[ -n "$skipped" ]]; then
    echo "skipping feature-gated bench targets:"
    echo "$skipped" | sed 's/^/  /'
fi

while read -r pkg bench; do
    echo "== $pkg / $bench"
    cargo bench -p "$pkg" --bench "$bench" --locked -- --noplot --save-baseline "$baseline"
done <<< "$targets"

echo "baseline '$baseline' saved under target/criterion/"
