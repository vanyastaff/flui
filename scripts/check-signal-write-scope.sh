#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# Refusal trigger 24 (ADR-0074 §5.2): a realm-scoped signal is never WRITTEN
# or CREATED inside a build / layout / paint body.
#
# Reading a signal in `build` is the sanctioned subscription path (the same
# class as `depend_on`), so `Signal::get(cx)` / `with(cx)` / `try_get` /
# `try_with` / `peek(..)` are fine anywhere. What this scanner refuses is the
# write side — `set(&r, ..)`, `update(&r, ..)`, `set_if_changed(&r, ..)` — and
# slot creation — `signal(..)`, `signal_owned_by(..)`, `try_signal(..)`,
# `try_signal_owned_by(..)` — inside the frame phases: a write from `build`
# re-marks readers of the frame that is still building (the unbounded-loop
# hazard trigger 22 exists for), and a slot created per build leaks one slot
# per rebuild.
#
# This scanner is ADVISORY: a textual scan cannot tell a closure the build
# defines for later (`on_tap(move |cx| sig.set(..))`, legal) from one it invokes
# synchronously (`items.iter().for_each(|i| sig.set(..))`, a write during
# build), and it cannot see a write behind a helper function. The binding
# gate is the run-time guard in `flui-view::reactive`: while an element's
# `build` runs (armed by `build_or_recover`, every element kind's one choke
# point), a write is `SignalError::WrittenDuringBuild` and a creation is
# `SignalError::CreatedDuringBuild`, each with a `tracing::warn!`. This
# scanner catches the obvious cases at review time, before a test has to.
#
# Tokens are chosen so the common one-argument `Cell::set(x)` never matches:
# a signal write always passes the graph first, so it is `.set(<graph>, ..)` —
# two arguments, or `.set(` at a line end when rustfmt wrapped the arguments.
# `.signal(`/`.signal_owned_by(` (and their `try_` forms) are the creation
# names.
#
# Same brace-depth scanner as scripts/check-frame-capability-scope.sh (a line
# grep cannot express "inside a function body"). Usage:
#   scripts/check-signal-write-scope.sh <path>...   # scan; exit 1 on violation
#   scripts/check-signal-write-scope.sh --self-test # verify the scanner itself
# -----------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

guarded_fns='build|build_into_views|perform_layout|layout_node_with_children|paint|paint_raw|run_paint|run_layout|run_compositing|compose|composite'
# `.set(`/`.update(` only when the FIRST argument reads like a graph handle —
# `&r`, `r`, `&self.r`, `cx.reactive()` — followed by a comma: a signal write
# always passes the graph first. One-argument `Cell::set(x)` has no comma, and
# `WidgetStatesController::update(WidgetState::Pressed, ..)` starts with a
# `Type::` path, so neither matches. The remaining names no other API in this
# workspace uses on a value. Backslashes are
# doubled because `awk -v` interprets escape sequences once.
writes='\\.set\\(&?\\*?[a-z_][a-z0-9_.()]*,|\\.set\\($|\\.update\\(&?\\*?[a-z_][a-z0-9_.()]*,|\\.update\\($|\\.set_if_changed\\(|\\.signal\\(|\\.signal_owned_by\\(|\\.try_signal\\(|\\.try_signal_owned_by\\('

scan() {
  awk -v guarded="${guarded_fns}" -v toks="${writes}" '
    FNR == 1 { inside = 0; depth = 0; seen_brace = 0; in_closure = 0; closure_base = 0; skip_next = 0 }
    {
      line = $0
      sub(/\/\/.*$/, "", line)
      if (!inside && line ~ ("(^|[^a-zA-Z0-9_])fn[ \t]+(" guarded ")[ \t]*[(<]")) {
        inside = 1
        depth = 0
        seen_brace = 0
        fn_line = FNR
      }
      if (inside) {
        # `PORT-CHECK-OK-24: <reason>` on the line, or on the comment line right
        # above it, marks a deliberate violation — the runtime-guard tests write
        # from build ON PURPOSE to prove the refusal.
        skip_this = skip_next
        skip_next = 0
        if ($0 ~ /PORT-CHECK-OK-24/) { skip_this = 1; skip_next = 1 }
        # A callback closure defined in `build` runs later, from an event, so a
        # write inside it is legal. Single-line closures: the `move |..|` marker
        # precedes the token on the same line. Multi-line closures: the marker
        # line opens a brace; everything until that brace closes is skipped.
        closure_pos = 0
        if (match(line, /(move[ \t]*\|[^|]*\||\|[A-Za-z_,&: ]*\|[ \t]*\{)/)) { closure_pos = RSTART }
        if (!skip_this && match(line, toks)) {
          if (!(in_closure || (closure_pos > 0 && closure_pos < RSTART))) {
            printf "%s:%d: signal write/creation `%s` inside the function opened at line %d\n", FILENAME, FNR, substr(line, RSTART, RLENGTH), fn_line
            violations++
          }
        }
        depth_before = depth
        opens = gsub(/\{/, "{", line)
        closes = gsub(/\}/, "}", line)
        if (opens > 0) { seen_brace = 1 }
        depth += opens - closes
        if (!in_closure && closure_pos > 0 && depth > depth_before) { in_closure = 1; closure_base = depth_before }
        if (in_closure && depth <= closure_base) { in_closure = 0 }
        if (seen_brace && depth <= 0) { inside = 0; in_closure = 0 }
      }
    }
    END { exit (violations > 0) }
  ' "$@"
}

self_test() {
  local fixtures="${repo_root}/scripts/fixtures/signal-write"
  local status=0
  echo "self-test: rejected fixture (signal write / creation inside build, layout, paint)"
  if scan "${fixtures}/rejected.rs.fixture" >/dev/null 2>&1; then
    echo "  FAIL: scanner accepted a file it must reject"
    status=1
  else
    local found
    found=$( (scan "${fixtures}/rejected.rs.fixture" 2>/dev/null || true) | wc -l | tr -d ' ')
    if [[ "${found}" -ne 9 ]]; then
      echo "  FAIL: expected 9 violations across every write/creation token, got ${found}"
      scan "${fixtures}/rejected.rs.fixture" 2>/dev/null | sed 's/^/  /' || true
      status=1
    else
      echo "  ok: 9 violations reported"
    fi
    local reported
    reported=$(scan "${fixtures}/rejected.rs.fixture" 2>/dev/null || true)
    for tok in '.set(' '.update(' '.set_if_changed(' '.signal(' '.signal_owned_by(' '.try_signal(' '.try_signal_owned_by('; do
      if ! grep -qF -- "${tok}" <<<"${reported}"; then
        echo "  FAIL: scanner never reported a ${tok} violation"
        status=1
      fi
    done
  fi
  # The accepted fixture is REAL code: a test module compiled under the
  # `signals` feature, so every legal shape it pins also type-checks.
  local accepted="${repo_root}/crates/flui-widgets/tests/signals_scanner_accepted.rs"
  echo "self-test: accepted fixture (compiled: reads in build, Cell::set, writes from init_state / callbacks)"
  if scan "${accepted}" >/dev/null 2>&1; then
    echo "  ok: no violations"
  else
    echo "  FAIL: scanner rejected legal usage:"
    scan "${accepted}" 2>/dev/null | sed 's/^/  /' || true
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

files=()
while IFS= read -r -d '' file; do
  files+=("${file}")
done < <(find "$@" -type f -name '*.rs' -not -path '*/target/*' -print0)

if [[ ${#files[@]} -eq 0 ]]; then
  exit 0
fi

scan "${files[@]}"
