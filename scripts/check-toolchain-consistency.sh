#!/usr/bin/env bash
# Checks that every place the workspace declares an MSRV agrees with
# rust-toolchain.toml's channel minor — the single source of truth under the
# pre-1.0 policy (MSRV tracks latest stable; see AGENTS.md / docs/PORT.md
# §Version policy). Run via `just gate` / `just toolchain-consistency-check`.
set -euo pipefail

# Version guard: this script is written against plain POSIX-ish bash (no
# mapfile/readarray, no associative arrays) specifically to sidestep the
# macOS /bin/bash 3.2 gotcha that bites scripts/port-check.sh. It only
# requires that it is actually invoked with bash, not sh/dash.
if [[ -z "${BASH_VERSINFO:-}" ]]; then
  echo "error: run this with bash, not sh (e.g. \`bash scripts/check-toolchain-consistency.sh\`)" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

toolchain_file="rust-toolchain.toml"
channel_line="$(grep -E '^channel = ' "$toolchain_file" || true)"
if [[ -z "$channel_line" ]]; then
  echo "toolchain-consistency: could not find 'channel = \"X.Y.Z\"' in $toolchain_file" >&2
  exit 1
fi
channel_minor="$(echo "$channel_line" | sed -E 's/^channel = "([0-9]+\.[0-9]+)\.[0-9]+"[[:space:]]*$/\1/')"
if [[ "$channel_minor" == "$channel_line" ]]; then
  echo "toolchain-consistency: could not parse a major.minor.patch version out of: $channel_line" >&2
  exit 1
fi

errors=0

check() {
  local label="$1" actual="$2"
  if [[ "$actual" != "$channel_minor" ]]; then
    echo "toolchain-consistency: $label declares \"$actual\", expected \"$channel_minor\" (from $toolchain_file's channel)" >&2
    errors=$((errors + 1))
  fi
}

cargo_rv="$(grep -E '^rust-version = "[0-9]+\.[0-9]+"' Cargo.toml | sed -E 's/^rust-version = "([0-9]+\.[0-9]+)"/\1/' || true)"
if [[ -z "$cargo_rv" ]]; then
  echo "toolchain-consistency: could not find 'rust-version = \"X.Y\"' in Cargo.toml" >&2
  errors=$((errors + 1))
else
  check "Cargo.toml [workspace.package].rust-version" "$cargo_rv"
fi

clippy_msrv="$(grep -E '^msrv = "[0-9]+\.[0-9]+"' clippy.toml | sed -E 's/^msrv = "([0-9]+\.[0-9]+)"/\1/' || true)"
if [[ -z "$clippy_msrv" ]]; then
  echo "toolchain-consistency: could not find 'msrv = \"X.Y\"' in clippy.toml" >&2
  errors=$((errors + 1))
else
  check "clippy.toml msrv" "$clippy_msrv"
fi

ci_toolchain="$(grep -E '^[[:space:]]*toolchain: "[0-9]+\.[0-9]+"' .github/workflows/ci.yml | head -1 | sed -E 's/^[[:space:]]*toolchain: "([0-9]+\.[0-9]+)"/\1/' || true)"
if [[ -z "$ci_toolchain" ]]; then
  echo "toolchain-consistency: could not find the msrv job's 'toolchain: \"X.Y\"' in .github/workflows/ci.yml" >&2
  errors=$((errors + 1))
else
  check ".github/workflows/ci.yml msrv job toolchain" "$ci_toolchain"
fi

template_count=0
for f in crates/flui-cli/src/templates/*.rs; do
  [[ -f "$f" ]] || continue
  tv="$(grep -E '^rust-version = "[0-9]+\.[0-9]+"' "$f" | sed -E 's/^rust-version = "([0-9]+\.[0-9]+)"/\1/' || true)"
  if [[ -n "$tv" ]]; then
    template_count=$((template_count + 1))
    check "$f rust-version" "$tv"
  fi
done
if [[ "$template_count" -eq 0 ]]; then
  echo "toolchain-consistency: found no 'rust-version = \"X.Y\"' line in any crates/flui-cli/src/templates/*.rs — template glob or format changed?" >&2
  errors=$((errors + 1))
fi

if [[ "$errors" -gt 0 ]]; then
  echo "toolchain-consistency: $errors mismatch(es) against $toolchain_file channel $channel_minor" >&2
  exit 1
fi

echo "toolchain-consistency: rust-toolchain.toml ($channel_minor) == Cargo.toml, clippy.toml, ci.yml msrv job, $template_count template(s)"
