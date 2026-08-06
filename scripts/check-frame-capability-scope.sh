#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# Port-check trigger #22 — **lifecycle-only presentation capabilities** must
# not be acquired from a build / layout / paint / composite body.
#
# A lifecycle-only capability lets code affect presentation state outside the
# build/layout/paint transaction. Seven are guarded:
#
#   rebuild_handle()    ADR-0018 — `RebuildHandle::schedule()` marks an element
#                       dirty for the next frame.
#   post_frame_handle() ADR-0021 — `PostFrameHandle::schedule()` queues work for
#                       the end of the current frame.
#   local_post_frame_handle() ADR-0021/#556 — `LocalPostFrameHandle::schedule_local()`
#                       is the same hazard as `post_frame_handle()` (queues work
#                       against the frame still building/laying out/painting),
#                       just through the `!Send` handle that addresses its lane
#                       directly instead of the `Send` cross-thread queue. A
#                       second token, not a variant of the first: the scanner is
#                       a textual match, and "post_frame_handle" is already a
#                       substring of this name, but the token is named
#                       explicitly here rather than relying on that coincidence.
#   text_input_handle() ADR-0030 — `TextInputHandle::attach()`/`detach()` register
#                       a client with the presentation's text-input owner;
#                       acquiring it from `build`/`layout`/`paint` would attach
#                       on every rebuild instead of once per focus transition.
#   focus_manager()     ADR-0037 — returns the presentation's concrete focus
#                       owner; imperative focus changes synchronously notify
#                       listeners and may schedule rebuilds.
#   owner_platform()    ADR-0039 §6 — `with_owner_platform` in flui-app's
#                       runner reaches the loop-scoped `!Send` owner-thread
#                       platform capability; acquiring it from `build`/
#                       `layout`/`paint` would let presentation code enqueue
#                       owner-lane platform work (e.g. `open_window`) mid-frame
#                       transaction, ahead of trigger #22's other six via the
#                       same ambient-authority hazard.
#   pipeline_owner()    PipelineCell port (docs/runtime-contract.toml's
#                       `semantics-two-phase-borrow` contract and `PipelineCell`
#                       surface entry) — `BuildContext::pipeline_owner()` returns
#                       a live `PipelineCell` handle to the whole render tree;
#                       calling `.with_mut()` on it from inside `build`/`layout`/
#                       `paint` would reenter the pipeline mid-transaction, which
#                       `PipelineCell::with_mut`'s own reentrancy guard turns into
#                       an immediate panic rather than the silent corruption an
#                       `Arc<RwLock<_>>` shape would have risked. Production call
#                       sites acquire it in `init_state` and clone it into a
#                       callback fired later, never touching it synchronously
#                       inside a guarded body — see `flui-widgets`'s
#                       `Focus::init_state` (`install_rect_provider`) and
#                       `InteractiveViewerState::init_state`.
#
# Seven tokens, six `BuildContext` methods: `rebuild_handle`,
# `local_post_frame_handle`, `post_frame_handle`, `text_input_handle`,
# `focus_manager`, and `pipeline_owner` are all `BuildContext` methods;
# `owner_platform` is the odd one out — a free function in flui-app, not a
# `BuildContext` method at all — and the scanner is a textual token match,
# not a method-call parse, so both shapes are caught the same way. All seven
# must be acquired in `ViewState::init_state` / `did_change_dependencies` (the
# six `BuildContext` methods) or outside any guarded body entirely
# (`owner_platform`, which is composition-root-only, `pub(crate)` to
# `flui-app`'s app module), stored, and fired later from a callback. A stored
# capability's OWN variable/field name must avoid the literal guarded token
# too (e.g. `pipeline_cell`, not `pipeline_owner`) — this scanner does not
# parse method calls, so a mere re-clone of an already-acquired value under
# the capability's own name inside a guarded body reads identically to a
# fresh acquisition.
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
capabilities='rebuild_handle|local_post_frame_handle|post_frame_handle|text_input_handle|focus_manager|owner_platform|pipeline_owner'

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

  echo "self-test: rejected fixture (presentation capability inside build/layout/paint)"
  if scan "${fixtures}/rejected.rs.fixture" >/dev/null 2>&1; then
    echo "  FAIL: scanner accepted a file it must reject"
    status=1
  else
    scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | sed 's/^/  /' || true
    local found
    found=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | wc -l || true)
    if [[ "${found}" -ne 10 ]]; then
      echo "  FAIL: expected 10 violations across all seven lifecycle-only capability tokens, got ${found}"
      status=1
    else
      echo "  ok: 10 violations reported"
    fi
    # Every capability token must actually be named — a scanner can otherwise
    # report the expected count while silently leaving a newer capability open.
    local reported
    reported=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null || true)
    for cap in rebuild_handle local_post_frame_handle post_frame_handle text_input_handle focus_manager owner_platform pipeline_owner; do
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
