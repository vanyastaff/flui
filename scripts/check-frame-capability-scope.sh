#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# Port-check trigger #22 — **lifecycle-only frame capabilities** must not be
# acquired from a build / layout / paint / composite body.
#
# A frame capability lets code reach into the *next* frame from outside one.
# Three exist:
#
#   rebuild_handle()    ADR-0018 U1 — `RebuildHandle::schedule()` marks an element
#                       dirty for the next frame.
#   post_frame_handle() ADR-0021 U2 — `PostFrameHandle::schedule()` queues work for
#                       the end of the current frame.
#   text_input_handle() ADR-0030 — `TextInputHandle::attach()`/`detach()` register
#                       a client with the binding's IME registry; acquiring it
#                       from `build`/`layout`/`paint` would attach on every
#                       rebuild instead of once per focus transition.
#
# All three must be acquired in `ViewState::init_state` / `did_change_dependencies`,
# stored, and fired later from a callback.
#
# Acquiring one inside `build` and scheduling from it is an unbounded rebuild loop
# (rebuild) or a callback that fires against the very frame that is still running
# (post-frame). Inside `perform_layout` / `paint` / compositing either would touch
# the tree mid-frame, after `build_scope` has already run. FOUNDATIONS.md permits
# an out-of-catalog `mark_needs_build` driver only when "gated by a refusal trigger
# barring signal subscriptions from `build`/`layout`/`paint`" — this is that gate.
#
# A grep cannot express "inside a function body", so this is a brace-depth
# scanner: it enters a guarded function at its opening `{` and leaves at the
# matching `}`, flagging any capability token seen in between. Line comments are
# stripped first, so prose mentioning the rule is not a violation.
#
# Usage:
#   scripts/check-frame-capability-scope.sh <path>...   # scan; exit 1 on violation
#   scripts/check-frame-capability-scope.sh --self-test # verify the scanner itself
# -----------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Functions whose bodies may never acquire a frame capability. `build` covers
# `StatelessView::build`, `ViewState::build`, and `build_into_views`.
guarded_fns='build|build_into_views|perform_layout|layout_node_with_children|paint|paint_raw|run_paint|run_layout|run_compositing|compose|composite'

# The capabilities themselves. Adding one here is the whole cost of guarding it.
capabilities='rebuild_handle|post_frame_handle|text_input_handle'

scan() {
  awk -v guarded="${guarded_fns}" -v caps="${capabilities}" '
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
        if (match(line, caps)) {
          printf "%s:%d: %s() acquired inside the function opened at line %d\n", FILENAME, FNR, substr(line, RSTART, RLENGTH), fn_line
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
  local fixtures="${repo_root}/scripts/fixtures/frame-capability"
  local status=0

  echo "self-test: rejected fixture (frame capability inside build/layout/paint)"
  if scan "${fixtures}/rejected.rs.fixture" >/dev/null 2>&1; then
    echo "  FAIL: scanner accepted a file it must reject"
    status=1
  else
    scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | sed 's/^/  /' || true
    local found
    found=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | wc -l || true)
    if [[ "${found}" -ne 5 ]]; then
      echo "  FAIL: expected 5 violations (rebuild_handle in build/perform_layout/paint, post_frame_handle in build/paint), got ${found}"
      status=1
    else
      echo "  ok: 5 violations reported"
    fi
    # Both capability tokens must actually be named — a scanner that only ever
    # matched `rebuild_handle` would still report 5 if the fixture were sloppy.
    local reported
    reported=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null || true)
    for cap in rebuild_handle post_frame_handle; do
      if ! grep -q "${cap}()" <<<"${reported}"; then
        echo "  FAIL: scanner never reported a ${cap}() violation"
        status=1
      fi
    done
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
