#!/usr/bin/env bash
# scripts/port-check.sh
#
# Verifies the six refusal triggers documented in docs/PORT.md against
# the workspace. Exits non-zero on the first violation outside the
# whitelist; prints offending file:line and the trigger ID.
#
# Cross-platform note: this script is bash. On Windows, run via Git Bash
# or WSL. A PowerShell sibling is not provided in this iteration; see
# docs/PORT.md "## Verification" for usage and rationale.
#
# Usage:
#   bash scripts/port-check.sh        # check all six triggers
#   bash scripts/port-check.sh -v     # verbose (print each check's pass line)

set -euo pipefail

verbose=0
if [[ "${1:-}" == "-v" ]]; then verbose=1; fi

# Resolve repo root regardless of cwd.
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

if ! command -v rg >/dev/null 2>&1; then
  echo "port-check: ripgrep (rg) not found on PATH" >&2
  echo "install: https://github.com/BurntSushi/ripgrep#installation" >&2
  exit 2
fi

violations=0
trigger_doc="docs/PORT.md#refusal-triggers"

# Run a refusal-trigger check. Filters out doc-comment lines (`//!`, `///`,
# leading `//`) from rg output before evaluating the result.
check() {
  local trigger_id="$1"
  local description="$2"
  local pattern="$3"
  shift 3
  # Remaining args are rg path/glob arguments.

  local hits
  if hits=$(rg --line-number --column "${pattern}" "$@" 2>/dev/null \
    | grep -Ev ':\s*(//!|///|//)' \
    || true); then
    :
  fi

  if [[ -n "${hits}" ]]; then
    echo "VIOLATION ${trigger_id}: ${description}"
    echo "see ${trigger_doc} (trigger ${trigger_id})"
    echo "${hits}"
    echo ""
    violations=$((violations + 1))
  else
    if [[ "${verbose}" -eq 1 ]]; then
      echo "ok    ${trigger_id}: ${description}"
    fi
  fi
}

# -----------------------------------------------------------------------------
# Trigger 1 -- RwLock<Box<dyn RenderObject ...>> in render/view crates.
# This is the canonical exemplar violation at flui-rendering/src/storage/entry.rs.
# -----------------------------------------------------------------------------
check "1" \
  "RwLock<Box<dyn RenderObject<...>>> in render/view crates" \
  'RwLock<\s*Box<\s*dyn\s+RenderObject' \
  --type rust \
  crates/flui-rendering/src \
  crates/flui-view/src

# -----------------------------------------------------------------------------
# Trigger 2 -- Box<dyn RenderObject<...>> stored as a struct field.
# Multi-line match for `struct X { ... Box<dyn RenderObject ...> ... }`.
# Function-parameter uses inside `fn(...)` and `impl From<...>` blocks are
# legitimate transient borrows and are not matched (they live outside `struct
# ... {}` blocks).
# -----------------------------------------------------------------------------
check "2" \
  "Box<dyn RenderObject<...>> stored as a struct field in render storage" \
  '(?s)struct\s+\w+[^{}]*\{[^}]*Box<\s*dyn\s+RenderObject' \
  --multiline \
  --type rust \
  crates/flui-rendering/src/storage \
  crates/flui-view/src/element

# -----------------------------------------------------------------------------
# Trigger 3 -- async fn build/layout/paint/perform_layout in render hot path.
# Whitelist: route-notification handlers in flui-view/src/binding.rs are async
# per Flutter SystemChannels callback semantics -- they sit on the binding
# layer, not the render path. Excluded by file glob.
# -----------------------------------------------------------------------------
check "3" \
  "async fn build/layout/paint/perform_layout in render hot path" \
  'async\s+fn\s+(build|layout|paint|perform_layout)\b' \
  --type rust \
  --glob '!**/binding.rs' \
  crates/flui-rendering/src \
  crates/flui-view/src \
  crates/flui-painting/src

# -----------------------------------------------------------------------------
# Trigger 4 -- Mutex on dirty-list state mutated during build/layout/paint.
# Forward-looking; production code uses AtomicRenderFlags + OnceCell + atomics.
# state.rs has a #[cfg(test)] MockTree with Mutex<Vec<ElementId>>; that file
# is excluded so the mock does not register as a violation.
# -----------------------------------------------------------------------------
check "4" \
  "Mutex on dirty-list state in flui-rendering production code" \
  'Mutex<\s*(Vec|HashSet|HashMap|BTreeSet|BTreeMap)<\s*ElementId' \
  --type rust \
  --glob '!**/test*.rs' \
  --glob '!**/tests/**' \
  --glob '!**/state.rs' \
  crates/flui-rendering/src

# -----------------------------------------------------------------------------
# Trigger 5 -- Arc::clone inside per-frame paint loop.
# Forward-looking. Scope is render-object paint impls only.
# -----------------------------------------------------------------------------
check "5" \
  "Arc::clone in per-frame paint loop (crates/flui-rendering/src/objects/)" \
  'Arc::clone\(' \
  --type rust \
  --glob '!**/test*.rs' \
  --glob '!**/tests/**' \
  crates/flui-rendering/src/objects

# -----------------------------------------------------------------------------
# Trigger 6 -- recursive Box<dyn View> stored in element child collections.
# Scope: struct field declarations under flui-view/src/element/. Field-only
# pattern (trailing comma); funnel parameters in trait method signatures are
# acceptable transient borrows and are excluded by the trailing-comma anchor.
# -----------------------------------------------------------------------------
check "6" \
  "Box<dyn View> stored as a struct field in element child collections" \
  '^\s+(pub\s+)?\w+\s*:\s*(Vec<\s*)?Box<\s*dyn\s+View\b[^,]*,\s*$' \
  --type rust \
  crates/flui-view/src/element

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
if [[ "${violations}" -gt 0 ]]; then
  echo "port-check: ${violations} violation(s) found"
  echo "fix the violations or update docs/PORT.md if the rule itself needs to change"
  exit 1
fi

echo "port-check: all six refusal triggers clean"
exit 0
