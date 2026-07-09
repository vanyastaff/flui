#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# Port-check trigger #22 — `rebuild_handle()` must not be acquired from a
# build / layout / paint / composite body (ADR-0018 U1).
#
# `RebuildHandle::schedule()` marks an element dirty for the next frame. Taking
# a handle inside `build` and scheduling from it is an unbounded rebuild loop;
# taking one inside `perform_layout` / `paint` / compositing would dirty the
# tree mid-frame, after `build_scope` has already run. FOUNDATIONS.md permits an
# out-of-catalog `mark_needs_build` driver only when "gated by a refusal trigger
# barring signal subscriptions from `build`/`layout`/`paint`" — this is that
# gate.
#
# The capability is acquired in `ViewState::init_state` /
# `did_change_dependencies`, stored, and fired later from a callback.
#
# A grep cannot express "inside a function body", so this is a brace-depth
# scanner: it enters a guarded function at its opening `{` and leaves at the
# matching `}`, flagging any `rebuild_handle` token seen in between. Line
# comments are stripped first, so prose mentioning the rule is not a violation.
#
# Usage:
#   scripts/check-rebuild-handle-scope.sh <path>...   # scan; exit 1 on violation
#   scripts/check-rebuild-handle-scope.sh --self-test # verify the scanner itself
# -----------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Functions whose bodies may never acquire a rebuild handle. `build` covers
# `StatelessView::build`, `ViewState::build`, and `build_into_views`.
guarded_fns='build|build_into_views|perform_layout|layout_node_with_children|paint|paint_raw|run_paint|run_layout|run_compositing|compose|composite'

scan() {
  awk -v guarded="${guarded_fns}" '
    FNR == 1 { inside = 0; depth = 0; seen_brace = 0 }

    {
      line = $0
      sub(/\/\/.*$/, "", line)          # strip line comments (and /// , //!)

      if (!inside && line ~ ("(^|[^a-zA-Z0-9_])fn[ \t]+(" guarded ")[ \t]*[(<]")) {
        inside = 1
        depth = 0
        seen_brace = 0
        fn_line = FNR
      }

      if (inside) {
        if (line ~ /rebuild_handle/) {
          printf "%s:%d: rebuild_handle() acquired inside the function opened at line %d\n", FILENAME, FNR, fn_line
          violations++
        }
        opens = gsub(/\{/, "{", line)
        closes = gsub(/\}/, "}", line)
        if (opens > 0) { seen_brace = 1 }
        depth += opens - closes
        if (seen_brace && depth <= 0) { inside = 0 }
      }
    }

    END { exit (violations > 0) }
  ' "$@"
}

self_test() {
  local fixtures="${repo_root}/scripts/fixtures/rebuild-handle"
  local status=0

  echo "self-test: rejected fixture (rebuild_handle inside build/layout/paint)"
  if scan "${fixtures}/rejected.rs.fixture" >/dev/null 2>&1; then
    echo "  FAIL: scanner accepted a file it must reject"
    status=1
  else
    scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | sed 's/^/  /' || true
    local found
    found=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | wc -l || true)
    if [[ "${found}" -ne 3 ]]; then
      echo "  FAIL: expected 3 violations (build, perform_layout, paint), got ${found}"
      status=1
    else
      echo "  ok: 3 violations reported"
    fi
  fi

  echo "self-test: accepted fixture (init_state / did_change_dependencies / stored handle)"
  if scan "${fixtures}/accepted.rs.fixture" >/dev/null 2>&1; then
    echo "  ok: no violations"
  else
    echo "  FAIL: scanner rejected legal production usage:"
    scan "${fixtures}/accepted.rs.fixture" 2>/dev/null | sed 's/^/  /' || true
    status=1
  fi

  return "${status}"
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit $?
fi

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <path>... | --self-test" >&2
  exit 2
fi

mapfile -t files < <(find "$@" -name '*.rs' -type f | sort)
if [[ "${#files[@]}" -eq 0 ]]; then
  exit 0
fi

scan "${files[@]}"
