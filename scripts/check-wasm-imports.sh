#!/usr/bin/env bash
# Verifies that the linked wasm cdylibs import ONLY from the expected host
# modules. An undefined symbol on wasm32 does not fail the link — rust-lld
# turns it into an import — so "it linked" proves nothing about the import
# surface. This check makes the import list an intentional, reviewed artifact:
# a new module (most notably `env`, the catch-all for accidental native
# externs) fails CI until it is added to the committed allowlist.
#
# Requires: wasm-tools (https://github.com/bytecodealliance/wasm-tools)
# Usage: scripts/check-wasm-imports.sh <wasm-file>...
set -euo pipefail

allowlist="$(dirname "$0")/wasm-import-allowlist.txt"
if [[ ! -f "$allowlist" ]]; then
    echo "error: allowlist not found at $allowlist" >&2
    exit 2
fi

status=0
for wasm in "$@"; do
    if [[ ! -f "$wasm" ]]; then
        echo "error: $wasm does not exist (was the cdylib built?)" >&2
        status=1
        continue
    fi
    # Import entries look like: (import "module" "name" ...)
    modules=$(wasm-tools print "$wasm" \
        | grep -oE '\(import "[^"]+"' \
        | sed -E 's/\(import "([^"]+)"/\1/' \
        | sort -u)
    unexpected=$(comm -23 <(printf '%s\n' "$modules") <(sort -u "$allowlist"))
    if [[ -n "$unexpected" ]]; then
        echo "error: $wasm imports from modules not in $allowlist:" >&2
        echo "$unexpected" | sed 's/^/  /' >&2
        echo "If intentional, add the module to the allowlist with a comment in the PR." >&2
        status=1
    else
        echo "ok: $wasm imports only from allowed modules:"
        echo "$modules" | sed 's/^/  /'
    fi
done
exit "$status"
