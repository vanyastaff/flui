#!/usr/bin/env bash
# scripts/port-check.sh
#
# Verifies the 20 refusal triggers (1-20, with #9 numbered for FR-036)
# documented in docs/PORT.md against the workspace, plus the FR-033
# sanctioned-dyn-boundary check. Exits non-zero on the first violation
# outside the whitelist; prints the offending file:line and the trigger
# ID. Triggers #8/#10/#11/#12/#13 added in D-block PR-C-3 §U41-U45
# (architecture-correction-plan SP-1/SP-3/SP-4/SP-6/SP-8). Trigger #14
# added by the N-geom polish pass §U12 (unit-barrier escape-hatch guard).
# Triggers #15/#16/#17/#18 added in core-0a adversarial-reaudit PR-4 §U5
# (println!/eprintln!/dbg! ban, module-level allow(unsafe_code) ban,
# reinvented debug_assert_* ban, key.rs new_unchecked ban). Trigger #19
# added in engine overhaul T9f (C4: Matrix4 must not appear on the
# record/pipeline side; convert at the Backend trait boundary). Trigger #20
# added in advanced-blend PR-5 (gradient/image producers must not regress
# to SrcOver warn-fallback; deleted strings must not reappear).
#
# Additionally reports the inline port-marker budget (TODO(port),
# PERF(port), PORT NOTE) — markers are deliberate Phase B deferrals, NOT
# violations; the script never fails on marker count.
#
# Cross-platform note: this script is bash. On Windows, run via Git Bash
# or WSL. A PowerShell sibling is not provided in this iteration; see
# docs/PORT.md "## Verification" for usage and rationale.
#
# Usage:
#   bash scripts/port-check.sh             # check all 20 triggers; silent on pass
#   bash scripts/port-check.sh -v          # verbose: per-trigger pass + marker totals
#   bash scripts/port-check.sh -b          # marker-budget mode (per-file breakdown)
#   bash scripts/port-check.sh --verbose   # alias for -v
#   bash scripts/port-check.sh --budget    # alias for -b

set -euo pipefail

verbose=0
budget=0
# Accept at most one flag — `-v` and `-b` are mutually exclusive (one is a
# trigger-check run with marker summary tail; the other is a marker-only
# scan that skips trigger checks). Extra args are a usage error so typos
# like `port-check -v -b` or `port-check -vfoo` fail loud instead of
# silently using only $1. Copilot review on PR #150.
# Print usage to stdout (used by --help) or stderr (used by error paths).
print_usage() {
  cat <<USAGE
usage: $0 [-v|--verbose|-b|--budget|-h|--help]

  (no flag)     Run all refusal triggers; silent on pass, list violations on fail.
  -v --verbose  Run triggers with per-trigger pass lines + marker-budget summary tail.
  -b --budget   Skip triggers; print per-file TODO(port) / PERF(port) / PORT NOTE breakdown. Exits 0 unconditionally.
  -h --help     Print this usage and exit 0.

See docs/PORT.md ## Verification for the full contract.
USAGE
}

if [[ $# -gt 1 ]]; then
  echo "port-check: at most one argument accepted; got $#: $*" >&2
  print_usage >&2
  exit 2
fi
case "${1:-}" in
  -v|--verbose) verbose=1 ;;
  -b|--budget)  budget=1  ;;
  -h|--help)    print_usage; exit 0 ;;
  "")           ;;
  *) echo "port-check: unknown arg: $1" >&2
     print_usage >&2
     exit 2 ;;
esac

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

# -----------------------------------------------------------------------------
# Marker scanning helpers (used by both -b mode and the -v summary tail).
#
# Three markers are tracked per docs/PORT.md ## Inline port markers tier:
#   - // TODO(port): <reason>        — Phase B re-read needed
#   - // PERF(port): <reason>        — Dart perf idiom elided; profile candidate
#   - // PORT NOTE: <reshape reason> — Rust shape diverged intentionally
#
# SAFETY: comments are standard Rust practice and NOT counted here; the unsafe
# audit is a separate concern from port-translation marker discipline.
# -----------------------------------------------------------------------------

# Regex set scanned across crates/. Slashes escaped (`\/\/`) because MSYS2
# bash on Windows path-mangles unescaped `//<x>` args as UNC paths and breaks
# the match silently. No `\b` anchor after `\)` because both `)` and `:` are
# non-word characters — `\b` requires a word/non-word transition, which is
# absent here, so the boundary silently never matches.
marker_pattern='\/\/\s+(TODO\(port\)|PERF\(port\)|PORT NOTE)'

# Count markers of one kind ("TODO(port)" | "PERF(port)" | "PORT NOTE") in a
# crate path. Echoes the count. Tolerates rg exit-1 (no matches) under
# `set -e pipefail` via `|| true`.
count_markers() {
  local kind="$1"
  local crate_path="$2"
  # Escape parens for grep -E; literal kind otherwise.
  local esc_kind="${kind//(/\\(}"
  esc_kind="${esc_kind//)/\\)}"
  local raw
  raw=$(rg --count-matches --type rust "\/\/\s+${esc_kind}" "${crate_path}" 2>/dev/null || true)
  if [[ -z "${raw}" ]]; then
    echo 0
  else
    echo "${raw}" | awk -F: '{s+=$NF} END {print s+0}'
  fi
}

# -----------------------------------------------------------------------------
# Marker-budget mode: skip all refusal-trigger checks and print a per-file
# breakdown of every marker hit under crates/. Exits 0 unconditionally —
# markers are deferred work, not violations.
# -----------------------------------------------------------------------------
if [[ "${budget}" -eq 1 ]]; then
  echo "port-check: marker-budget report (crates/)"
  echo ""
  hits=$(rg --line-number --no-heading --type rust "${marker_pattern}" crates/ 2>/dev/null || true)
  if [[ -z "${hits}" ]]; then
    echo "  (no TODO(port) / PERF(port) / PORT NOTE markers found)"
    echo ""
    echo "marker-budget: 0 markers across crates/"
    exit 0
  fi
  echo "${hits}"
  echo ""
  # Unified counting: reuse the same `count_markers` helper the verbose
  # summary tail uses. This was previously three inline `grep -E -c` pipes
  # that diverged from `count_markers` on tab-indented markers (the inline
  # form anchored on `\s+`, the helper passed the kind string directly to
  # rg). Maintainability finding on PR #150 — single source of truth for
  # the count semantics.
  total_todo=$(count_markers "TODO(port)" crates/)
  total_perf=$(count_markers "PERF(port)" crates/)
  total_note=$(count_markers "PORT NOTE"  crates/)
  total_all=$((total_todo + total_perf + total_note))
  echo "marker-budget: ${total_all} markers (${total_todo} TODO(port), ${total_perf} PERF(port), ${total_note} PORT NOTE)"
  exit 0
fi

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
# Trigger 1 -- RwLock<Box<dyn RenderObject ...>> in render/view/layer/painting/
# engine crates. This is the canonical exemplar violation at
# flui-rendering/src/storage/entry.rs. Mythos Step 13 of the flui-layer chain
# added `crates/flui-layer/src` to the scope; Mythos Step 13 of the
# flui-painting chain added `crates/flui-painting/src` as a forward-looking
# guard (today's flui-painting has no RenderObject/Layer/ContainerLayer trait
# objects -- the crate is #[forbid(unsafe_code)] and uses closed enums -- but
# the scope extension catches any reintroduction post-split). Mythos Step 9 of
# the flui-engine chain added `crates/flui-engine/src` plus the `CommandRenderer`
# trait-name to the regex so engine-storage types (`Backend`, etc.) are caught
# if wrapped in `RwLock<Box<dyn CommandRenderer>>`.
# -----------------------------------------------------------------------------
check "1" \
  "RwLock<Box<dyn ...>> in render/view/layer/painting/engine crates" \
  'RwLock<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer|CommandRenderer)' \
  --type rust \
  crates/flui-rendering/src \
  crates/flui-view/src \
  crates/flui-layer/src \
  crates/flui-painting/src \
  crates/flui-engine/src

# -----------------------------------------------------------------------------
# Trigger 2 -- Box<dyn RenderObject<...>> wrapped in an interior-mutability
# primitive in render storage.
#
# Owned `Box<dyn RenderObject<_>>` as a plain field is the chosen post-U2
# baseline (preserves the open-set trait, delegates mutation discipline to
# the borrow checker via &mut RenderTree). The hazard is *wrapping* the
# trait object in RwLock/Mutex/RefCell/Cell/UnsafeCell on the storage type,
# which would smuggle the lock-or-interior-mutability problem back in under
# a different primitive. Trigger 1 catches RwLock specifically; this trigger
# generalises to the others.
# -----------------------------------------------------------------------------
check "2" \
  "Box<dyn ...> wrapped in interior-mutability primitive in render/view/layer/painting/engine storage" \
  '(RwLock|Mutex|RefCell|Cell|UnsafeCell)<\s*Box<\s*dyn\s+(RenderObject|Layer\b|ContainerLayer|CommandRenderer)' \
  --type rust \
  crates/flui-rendering/src/storage \
  crates/flui-view/src/element \
  crates/flui-layer/src \
  crates/flui-painting/src \
  crates/flui-engine/src

# -----------------------------------------------------------------------------
# Trigger 3 -- async fn build/layout/paint/perform_layout/composite/render in
# render/layer/engine hot path.
# Whitelist: route-notification handlers in flui-view/src/binding.rs are async
# per Flutter SystemChannels callback semantics -- they sit on the binding
# layer, not the render path. Excluded by file glob.
# Mythos Step 13 of the flui-layer chain extended the verb set to include
# `composite`, `render`, and `fire_composition_callbacks` so layer-level
# async violations are caught at the same trigger.
# Mythos Step 9 of the flui-engine chain extended the verb set to include
# `submit`, `present`, `render_scene`, `render_layer_recursive`, and
# `handle_backdrop_filter` so engine-level async violations are caught.
# `new` and `new_offscreen` are NOT in the verb set because they are async
# at the wgpu boundary (setup-phase; acceptable per the strategy clause).
# -----------------------------------------------------------------------------
check "3" \
  "async fn build/layout/paint/perform_layout/composite/render/submit/present/render_scene/render_layer_recursive/handle_backdrop_filter/fire_composition_callbacks in render/layer/engine hot path" \
  'async\s+fn\s+(build|layout|paint|perform_layout|composite|render|fire_composition_callbacks|submit|present|render_scene|render_layer_recursive|handle_backdrop_filter)\b' \
  --type rust \
  --glob '!**/binding.rs' \
  crates/flui-rendering/src \
  crates/flui-view/src \
  crates/flui-painting/src \
  crates/flui-layer/src \
  crates/flui-engine/src

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
# Trigger 5 -- Arc::clone inside per-frame paint/composite loop.
# Forward-looking. Scope:
#   - flui-objects/src (per-render-object paint impls; moved from
#     flui-rendering/src/objects per ADR-0008)
#   - flui-engine/src/wgpu/layer_render.rs (per-layer wgpu walk; extended in
#     Mythos Step 13 of the flui-layer chain)
#
# *** SCOPE EXCLUSIONS BELOW ARE TRACKED-OUTSTANDING-REFACTOR WHITELISTS ***
#
# `flui-engine/src/wgpu/backend.rs` is NOT in the scope yet because it has
# known per-frame `Arc::clone` sites at lines 121-122 (offscreen-painter
# cache initialisation) and lines 408-409 (`render_shader_mask` accessor
# pattern). Both are documented as Friction log entries in
# `crates/flui-engine/ARCHITECTURE.md` and tracked as Outstanding refactor #1
# (`Arc<Mutex<OffscreenRenderer>>` -> direct ownership + `Backend<'a>`). When
# the refactor lands, `backend.rs` MUST be added to this trigger's scope in
# the same PR so regressions are caught against the post-refactor shape.
#
# `flui-engine/src/wgpu/renderer.rs` is NOT in the scope because:
# - `Renderer::new` and `new_offscreen` perform setup-phase `Arc::clone(&device)`
#   / `Arc::clone(&queue)` calls that amortise across the renderer's lifetime
#   (acceptable per the strategy clause).
# - The canonical per-frame clones at lines 656-657 (RenderContext
#   construction) are documented as Friction log entries and tracked as
#   Outstanding refactor #3 (Per-frame Arc::clone -> borrowed references;
#   depends on Outstanding refactor #1). When that refactor lands, `renderer.rs`
#   should be added to this trigger's scope with a function-level exclusion
#   for `Renderer::new` / `new_offscreen` (setup-phase) only.
# -----------------------------------------------------------------------------
check "5" \
  "Arc::clone in per-frame paint/composite loop" \
  'Arc::clone\(' \
  --type rust \
  --glob '!**/test*.rs' \
  --glob '!**/tests/**' \
  crates/flui-objects/src \
  crates/flui-engine/src/wgpu/layer_render.rs

# -----------------------------------------------------------------------------
# Trigger 6 -- recursive Box<dyn View> stored in element child collections.
# Scope: struct field declarations under flui-view/src/element/. Field-only
# pattern anchored to EXACTLY 4-space indent (top-level struct fields). Funnel
# parameters in multi-line function signatures sit at 8+ spaces and are excluded
# by the indent anchor (the trailing-comma alone was not enough — multi-line
# parameters also end in comma — so the indent depth distinguishes).
# Nested struct fields would also live at 8+ spaces; per PORT.md trigger 6
# "Box<dyn View> ... as a struct field in element child collections", the
# concern is the top-level element-tree storage shape, not nested helpers.
# -----------------------------------------------------------------------------
check "6" \
  "Box<dyn View> stored as a struct field in element child collections" \
  '^    (pub\s+)?\w+\s*:\s*(Vec<\s*)?Box<\s*dyn\s+View\b[^,]*,\s*$' \
  --type rust \
  crates/flui-view/src/element

# -----------------------------------------------------------------------------
# Trigger 7 -- Arc<Mutex<*>> or Arc<RwLock<*>> on a *Renderer / *Pool / wgpu::*
# field inside crates/flui-engine/src/wgpu/.
# Forward-looking. Added in Mythos Step 9 of the flui-engine chain. Catches
# regressions of the Arc<parking_lot::Mutex<OffscreenRenderer>> and
# Arc<Mutex<TexturePoolInner>> shapes documented as Outstanding refactors in
# crates/flui-engine/ARCHITECTURE.md.
#
# Today's known sites at crate root, intentionally surfaced as Friction log
# entries in ARCHITECTURE.md, do match this trigger and will be expected to
# be reported once the corresponding Outstanding refactor lands. Until then,
# the trigger is INFORMATIONAL on Friction-log-tracked sites; the regex is
# narrow enough that any NEW Arc<Mutex<>>/Arc<RwLock<>> on a *Renderer /
# *Pool / wgpu::* field is a regression that should be addressed.
#
# Scope excludes test files (`!**/test*.rs`, `!**/tests/**`) so test fixtures
# are not flagged.
#
# *** FILE-GLOB EXCLUSIONS BELOW ARE TRACKED-OUTSTANDING-REFACTOR WHITELISTS ***
#
# Three files contain the EXACT patterns this trigger is designed to catch:
#   - `texture_pool.rs:71,224`  -- `Arc<Mutex<TexturePoolInner>>` (R10; tracked
#                                  as Outstanding refactor #2 in
#                                  `crates/flui-engine/ARCHITECTURE.md`).
#   - `renderer.rs:147`         -- `Arc<parking_lot::Mutex<OffscreenRenderer>>`
#                                  (R9; tracked as Outstanding refactor #1).
#   - `backend.rs:26,45,57`     -- same `Arc<Mutex<OffscreenRenderer>>` shape,
#                                  symmetric with renderer.rs (R9).
#
# The Mythos chain (PR feat/flui-engine-mythos-redesign) DEFERRED these three
# refactors to follow-up work per ARCHITECTURE.md `## Outstanding refactors`.
# To avoid port-check fire-on-known-violation, the three files are whitelisted
# below. **When the corresponding Outstanding refactor lands (i.e., the
# Arc<Mutex<>> shape is removed from a file), the matching `--glob !**/<file>`
# exclusion below MUST be removed in the same PR** so this trigger then catches
# regressions against the post-refactor shape.
#
# Cross-reference: see `crates/flui-engine/ARCHITECTURE.md` ## Friction log
# entry "Arc<parking_lot::Mutex<OffscreenRenderer>>" and "Arc<Mutex<
# TexturePoolInner>>" for the deferral rationale.
# -----------------------------------------------------------------------------
#
# Regex shape (anchored + grouped per Copilot review on PR #79):
#   ^\s+(pub\s+)?\w+\s*:\s*(Option<\s*)?Arc<\s*(parking_lot::)?(Mutex|RwLock)<\s*((super::)?(\w+::)*\w*(Renderer|Pool)\w*|wgpu::\w+)
# Anchors to struct-field syntax: leading whitespace + optional `pub` + ident
# + `:`. Inner alternation `((super::)?(\w+::)*\w*(Renderer|Pool)\w*|wgpu::\w+)`
# is grouped so `wgpu::*` matches only at the outer-type position, not as a
# bleed-through into the `Renderer|Pool` arm. Path segments (`super::`,
# `\w+::`) allow `super::offscreen::OffscreenRenderer` and similar. Trailing
# `\w*` on the Renderer/Pool arm catches names like `TexturePoolInner` where
# `Pool` is not at the end of the identifier. `(Option<\s*)?` catches both
# `Arc<...>` direct fields and `Option<Arc<...>>` fields (the shape used by
# `Renderer::offscreen`).
# -----------------------------------------------------------------------------
check "7" \
  "Arc<(Mutex|RwLock)<*Renderer|*Pool|wgpu::*>> struct field in flui-engine wgpu module" \
  '^\s+(pub\s+)?\w+\s*:\s*(Option<\s*)?Arc<\s*(parking_lot::)?(Mutex|RwLock)<\s*((super::)?(\w+::)*\w*(Renderer|Pool)\w*|wgpu::\w+)' \
  --type rust \
  --glob '!**/test*.rs' \
  --glob '!**/tests/**' \
  --glob '!**/texture_pool.rs' \
  --glob '!**/renderer.rs' \
  --glob '!**/backend.rs' \
  crates/flui-engine/src/wgpu

# -----------------------------------------------------------------------------
# FR-033 (Phase 3 §U29): downcast_ref::<…> in the View-type update dispatch
# path. Scoped to `crates/flui-view/src/element/{generic.rs, dispatch.rs}` —
# the body of `ElementCore::update_view` and its dispatch helper. The grep
# matches **any** `downcast_ref::<` inside the scoped files, not just the
# `<…View…>` shape: the historical regression form is `downcast_ref::<V>()`
# where `V` is a generic parameter, and a regex that requires the literal
# substring `View` inside the type argument is a no-op for the exact defect
# FR-033 closes. Legitimate non-View-type `downcast_ref` uses (slot
# attachment in `unified.rs`) live OUTSIDE this scope and are not flagged
# here; per-line whitelist via `// PORT-CHECK-OK-DOWNCAST: <reason>` markers
# is reserved for sites that enter the scope but should be sanctioned
# individually.
#
# This is a SPEC requirement (FR-033, SC-004) but NOT a numbered refusal
# trigger — refusal trigger #9 (FR-036, plan §U30) is the broader
# sanctioned-`dyn` enforcement; this grep targets a single defect class on
# a tighter scope.
# -----------------------------------------------------------------------------
fr033_hits=$(rg --line-number --column 'downcast_ref::<' \
  crates/flui-view/src/element/generic.rs \
  crates/flui-view/src/element/dispatch.rs 2>/dev/null \
  | grep -Ev '//\s*PORT-CHECK-OK-DOWNCAST:' \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)
if [[ -n "${fr033_hits}" ]]; then
  echo "VIOLATION FR-033: downcast_ref::<…> in update-dispatch path"
  echo "see docs/PORT.md (FR-033 enforcement, spec § specs/004-view-element-core/spec.md FR-033)"
  echo "${fr033_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    FR-033: downcast_ref::<…> in update-dispatch path"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 8 (D-block PR-C-3 §U41, architecture-correction-plan SP-1) —
# stubbed-but-called functions.
#
# Greps for `unimplemented!(` / `todo!(` in production code (non-test).
# These are SP-1 violations: a `fn` body that panics on entry is a
# "stubbed-but-called" surface — it appears in the API but has no
# implementation. Common shapes:
#   - `fn foo() { unimplemented!() }`
#   - `fn foo() -> T { todo!() }`
#
# Exclusions:
#   - doc comments (`///`, `//!`, `//`)
#   - `// PORT-CHECK-OK-STUB: <reason>` markers on the same line
#   - `crates/flui-platform/src/platforms/{linux,ios,android}/`
#     (platform-init stubs deferred to native-platform implementation
#     work; tracked outside SP-1 — see `crates/flui-platform/ARCHITECTURE.md`
#     and `docs/ROADMAP.md` Core.0 → Core.1 platform-impl track)
#   - `#[cfg(test)]` / `tests/` files
#   - example crates (each example exists to demonstrate, not ship as
#     framework code; an example's `todo!()` is an EXAMPLE concern,
#     not an SP-1 violation)
#
# Allowlist marker grammar (same convention as triggers 7 / 9):
#   <something>  // PORT-CHECK-OK-STUB: <one-line justification + tracking issue>
#
# Tracking-issue requirement: every marker should reference a tracking
# issue or follow-up doc so the stub doesn't become permanent. Per-site
# audit during PR review enforces this.
# -----------------------------------------------------------------------------
trigger8_raw=$(rg --line-number --column \
    -e 'unimplemented!\s*\(' \
    -e 'todo!\s*\(' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!crates/flui-platform/src/platforms/linux/**' \
    --glob '!crates/flui-platform/src/platforms/ios/**' \
    --glob '!crates/flui-platform/src/platforms/android/**' \
    crates/ 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  | grep -Ev '//\s*PORT-CHECK-OK-STUB:' \
  || true)

# **PR #151 Codex review #3295220689:** post-filter out matches inside
# in-file `#[cfg(test)]` blocks (Rust convention: `#[cfg(test)] mod
# tests { ... }` at end of source file). The path-glob exclusions
# above only drop dedicated test files; in-file test modules slip
# through and false-positive on test-only `todo!()` / `unimplemented!()`
# scaffolding.
#
# Heuristic: scan the file from the top to the matched line for a
# `#[cfg(test)]` attribute on its own line. If found, assume the match
# is inside a test mod and drop. This accepts the false-negative of
# a `cfg(test)` ancestor that doesn't actually enclose the match
# (rare in practice; tests/ + test*.rs path-exclusion handles the
# dedicated-test-file case).
trigger8_hits=""
while IFS= read -r match_line; do
  [[ -z "${match_line}" ]] && continue
  # Bash parameter expansion — zero subprocesses (replaces echo|awk|tr, echo|awk).
  match_file="${match_line%%:*}"
  match_file="${match_file//\\//}"
  _rest="${match_line#*:}"
  match_lineno="${_rest%%:*}"
  [[ -z "${match_file}" || -z "${match_lineno}" ]] && continue
  # Single awk call (replaces head|grep pipeline).
  if awk -v maxn="${match_lineno}" \
      'NR>maxn{exit} /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/{found=1; exit} END{exit !found}' \
      "${match_file}" 2>/dev/null; then
    continue
  fi
  trigger8_hits="${trigger8_hits}${match_line}
"
done <<< "${trigger8_raw}"

if [[ -n "${trigger8_hits// }" ]]; then
  echo 'VIOLATION 8: SP-1 stubbed-but-called (unimplemented!()/todo!() in production fn body)'
  echo "see ${trigger_doc} (trigger 8)"
  echo "${trigger8_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    8: SP-1 stubbed-but-called (unimplemented!()/todo!() in production)"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 10 (D-block PR-C-3 §U42, architecture-correction-plan SP-3) —
# parallel cross-crate type definitions.
#
# Collects every `pub struct` / `pub enum` / `pub trait` identifier
# across the `crates/flui-*/src/` tree and flags any identifier defined
# (not re-exported) in 2+ DISTINCT crates. Parallel definitions are an
# SP-3 smell: either the same concept is implemented twice (consolidate)
# or two unrelated concepts collide on a single name (rename one).
#
# Re-exports (`pub use foo::Bar`) do not match because the pattern
# requires the `struct`/`enum`/`trait` keyword.
#
# Exclusions:
#   - doc comments (`///`, `//!`, `//`)
#   - tests (`tests/`, `test*.rs`, `#[cfg(test)]` files)
#   - example crates (`examples/`)
#   - `// PORT-CHECK-OK-SP3: <reason>` markers on the same line as
#     the `pub <kind> Name` declaration sanction the duplicate.
#
# Allowlist marker grammar:
#   pub struct Foo { ... }  // PORT-CHECK-OK-SP3: <reason + tracking-issue>
#
# Pre-existing parallel definitions in the current codebase are marked
# individually so future ADDITIONS are caught; the marker reason should
# point to a consolidation tracking issue so the duplicate doesn't
# become permanent.
# -----------------------------------------------------------------------------
trigger10_defs_raw=$(rg --line-number --no-heading \
    'pub +(struct|enum|trait) +[A-Z][a-zA-Z0-9_]*' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!examples/**' \
    crates/ 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

# Marker scan: a PORT-CHECK-OK-SP3 marker is sanctioning if it appears
# on the same line OR on the preceding line OR on either of the next
# 2 lines (rustfmt moves trailing same-line markers on block-opening
# decls like `pub enum Foo {` into the block body as the first
# non-blank line).
# Optimised: one rg call collects all SP3 markers; one awk pass filters
# defs via a single pipe (no process substitution — <() is ~7s each on
# Windows Git Bash; {} | awk costs one fork total instead of two
# <() forks). Window −1..+2 expanded at marker-load time → O(1) lookup.
trigger10_sp3_markers=$(rg --line-number --no-heading 'PORT-CHECK-OK-SP3:' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!examples/**' \
    crates/ 2>/dev/null || true)
trigger10_defs=$(
  { printf '%s\n' "${trigger10_sp3_markers}"; printf '%s\n' '---T10SPLIT---'; printf '%s\n' "${trigger10_defs_raw}"; } | \
  awk -F':' '
  /^---T10SPLIT---$/ { past_split = 1; next }
  !past_split {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    # Marker at M sanctions def at lines [M-2, M+1] (window def-1..def+2).
    for (d = -2; d <= 1; d++) covered[fp SUBSEP (ln + d)] = 1
    next
  }
  {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    if (!((fp SUBSEP ln) in covered)) print
  }'
)

# Build a tab-separated index: crate \t kind \t name \t full-line.
# **PR #151 Copilot review #3295220014:** Windows rg output uses `\`
# in paths; the awk `split($1, parts, "/")` then yields `n < 2` and
# silently drops every entry, suppressing all SP-3 duplicate detection.
# Normalize backslash → forward slash before splitting.
trigger10_index=$(echo "${trigger10_defs}" | awk -F':' '
BEGIN { OFS = "\t" }
{
  fp = $1
  gsub(/\\/, "/", fp)
  n = split(fp, parts, "/")
  if (n < 2) next
  crate = parts[2]
  line = $0
  pos = match(line, /pub +(struct|enum|trait) +[A-Z][a-zA-Z0-9_]*/)
  if (pos == 0) next
  matched = substr(line, pos, RLENGTH)
  split(matched, w, /[ \t]+/)
  print crate, w[2], w[3], line
}')

# Find (kind, name) pairs defined in 2+ distinct crates.
trigger10_dupes=$(echo "${trigger10_index}" \
  | awk -F'\t' 'NF>=3 {print $2 "\t" $3 "\t" $1}' \
  | sort -u \
  | awk -F'\t' '{print $1 "\t" $2}' \
  | sort \
  | uniq -d)

if [[ -n "${trigger10_dupes}" ]]; then
  trigger10_report=""
  while IFS=$'\t' read -r kind name; do
    [[ -z "${kind}" ]] && continue
    trigger10_report="${trigger10_report}  ${kind} ${name}:
$(echo "${trigger10_index}" | awk -F'\t' -v k="${kind}" -v n="${name}" '$2==k && $3==n {print "    " $4}')
"
  done <<< "${trigger10_dupes}"
  echo 'VIOLATION 10: SP-3 parallel cross-crate type definitions'
  echo "see ${trigger_doc} (trigger 10)"
  echo "${trigger10_report}"
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    10: SP-3 parallel cross-crate type definitions"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 11 (D-block PR-C-3 §U43, architecture-correction-plan SP-4) —
# speculative scaffolding: `pub mod` family with zero production
# consumers and not behind `cfg(feature = "unstable-*")`.
#
# Scans each crate's `lib.rs` for `pub mod <name>;` declarations.
# For each declaration the trigger:
#   1. Skips if the preceding non-blank line is `#[cfg(feature =
#      "unstable-...")]` (intentional speculation behind a feature
#      gate — sanctioned by SP-4 verdict).
#   2. Skips if the same `lib.rs` has `pub use <name>::` re-exporting
#      the module's items (explicit external API surface).
#   3. Otherwise searches the rest of the workspace for `<crate>::<name>`
#      or `use <crate>::<name>` references. If zero references outside
#      the defining crate exist, flags the declaration.
#
# Allowlist marker grammar:
#   pub mod foo;  // PORT-CHECK-OK-SP4: <reason + tracking-issue>
#
# Limitations: this is a mechanical scan and trades precision for
# implementability. It catches the common "lib.rs declares pub mod with
# no use sites" shape; it does NOT catch sub-module speculation
# (`mod foo { pub mod bar; }`). For deeper SP-4 audits, see the manual
# verdicts in architecture-correction-plan §SP-4 (table at line 451).
# -----------------------------------------------------------------------------
trigger11_lib_files=$(rg --files --type rust --glob '**/lib.rs' --glob '!**/tests/**' --glob '!examples/**' crates/ 2>/dev/null || true)
trigger11_violations=""
# cfg-feature pattern used in backward scan below (stored once; bash =~ avoids echo|grep).
_t11_cfg_feat_pat='#\[cfg\([^)]*feature[[:space:]]*=[[:space:]]*"(unstable-|testing)"'
for libfile in ${trigger11_lib_files}; do
  # Extract crate name from path: crates/flui-X/src/lib.rs → flui-X → flui_X
  # Bash parameter expansion — zero subprocesses (replaces echo|tr, echo|awk, echo|tr).
  libfile_norm="${libfile//\\//}"
  _t11_tmp="${libfile_norm#*/}"        # strip leading component ("crates/")
  crate_dir="${_t11_tmp%%/*}"          # keep only the crate directory name
  crate_underscore="${crate_dir//-/_}"

  # Preload lib.rs into an array for zero-fork backward scan.
  mapfile -t _t11_lines < "${libfile}" 2>/dev/null || true

  # Find every `pub mod NAME;` line in lib.rs (declaration form, not block form).
  mod_lines=$(grep -nE '^[[:space:]]*pub[[:space:]]+mod[[:space:]]+[a-z_][a-z0-9_]*[[:space:]]*;' "${libfile}" 2>/dev/null || true)
  while IFS= read -r mod_line; do
    [[ -z "${mod_line}" ]] && continue
    # Bash parameter expansion replaces echo|awk and echo|cut (zero forks).
    lineno="${mod_line%%:*}"
    content="${mod_line#*:}"

    # Skip if marker present on the same line (zero-fork string test).
    if [[ "${content}" == *'PORT-CHECK-OK-SP4:'* ]]; then
      continue
    fi

    # Extract mod name via bash regex (zero forks; replaces echo|sed).
    if [[ "${content}" =~ ^[[:space:]]*pub[[:space:]]+mod[[:space:]]+([a-z_][a-z0-9_]*) ]]; then
      modname="${BASH_REMATCH[1]}"
    else
      continue
    fi

    # Skip if previous non-blank line is `#[cfg(feature = "unstable-...")]`.
    # **PR #151 Copilot review #3295220020 + Codex #3295220690:** scan backward
    # until a non-blank line is found (a blank separator between the attribute
    # and `pub mod` must not cause a false-positive).
    # Preloaded array eliminates per-line sed forks.
    prev_lineno=$(( lineno - 1 ))
    prev_content=""
    while [[ "${prev_lineno}" -gt 0 ]]; do
      prev_content="${_t11_lines[$((prev_lineno - 1))]}"
      if [[ -n "${prev_content// }" ]]; then
        break
      fi
      prev_lineno=$(( prev_lineno - 1 ))
    done
    # Bash =~ replaces echo|grep (zero forks).
    if [[ "${prev_content}" =~ $_t11_cfg_feat_pat ]]; then
      continue
    fi

    # Skip if the same lib.rs re-exports items from the module
    # (`pub use <modname>::...` or `pub use crate::<modname>::...` —
    # explicit external API surface).
    if grep -qE "^[[:space:]]*pub[[:space:]]+use[[:space:]]+(crate::)?${modname}::" "${libfile}"; then
      continue
    fi

    # Workspace consumer search: look for `<crate>::<modname>` (qualified
    # path) or `use <crate>::<modname>` (import) anywhere in the
    # workspace OUTSIDE the defining crate. If zero matches → flag.
    consumer_matches=$(rg --type rust --no-heading -l \
        -e "${crate_underscore}::${modname}\\b" \
        --glob "!crates/${crate_dir}/**" \
        --glob '!**/tests/**' \
        crates/ examples/ 2>/dev/null | head -1 || true)
    if [[ -z "${consumer_matches}" ]]; then
      trigger11_violations="${trigger11_violations}${libfile}:${lineno}:${content}
"
    fi
  done <<< "${mod_lines}"
done

if [[ -n "${trigger11_violations}" ]]; then
  echo 'VIOLATION 11: SP-4 speculative scaffolding (pub mod with zero workspace consumers, not feature-gated)'
  echo "see ${trigger_doc} (trigger 11)"
  echo "${trigger11_violations}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    11: SP-4 speculative scaffolding (pub mod surfaces)"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 12 (D-block PR-C-3 §U44, architecture-correction-plan SP-6) —
# lock placement in public API.
#
# Lock types leak the framework's concurrency model across module
# boundaries. A `pub fn -> RwLockReadGuard<...>` or `pub field:
# Mutex<...>` forces every caller to reason about lock ordering /
# poisoning / re-entrancy. SP-6's verdict is that locks should live
# behind private fields; public APIs should expose immutable
# snapshots or scoped callbacks.
#
# Patterns flagged:
#   pub fn foo() -> RwLockReadGuard<...> | RwLockWriteGuard<...> |
#                   MutexGuard<...> | RwLock<...> | Mutex<...>
#   pub field: (Arc<)?(parking_lot::)?(RwLock|Mutex)<...>
#
# Allowlist marker grammar:
#   <decl>  // PORT-CHECK-OK-SP6: <reason + tracking-issue>
# -----------------------------------------------------------------------------
trigger12_raw=$(rg --line-number --no-heading \
    -e '^\s*pub fn .*-> .*(RwLock|Mutex|RwLockReadGuard|RwLockWriteGuard|MutexGuard)\b' \
    -e '^\s*pub \w+ *: *(Arc<\s*)?(parking_lot::)?(RwLock|Mutex)<' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!examples/**' \
    crates/ 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

# Same ±2 line marker-scan window as trigger #10 — rustfmt may move
# trailing same-line markers on `pub fn ... -> ... {` block-openings
# into the function body.
# Optimised: one rg call collects all SP6 markers; one awk pass via
# single pipe (no process substitution; window −1..+2 at load time).
trigger12_sp6_markers=$(rg --line-number --no-heading 'PORT-CHECK-OK-SP6:' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!examples/**' \
    crates/ 2>/dev/null || true)
trigger12_hits=$(
  { printf '%s\n' "${trigger12_sp6_markers}"; printf '%s\n' '---T12SPLIT---'; printf '%s\n' "${trigger12_raw}"; } | \
  awk -F':' '
  /^---T12SPLIT---$/ { past_split = 1; next }
  !past_split {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    for (d = -2; d <= 1; d++) covered[fp SUBSEP (ln + d)] = 1
    next
  }
  {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    if (!((fp SUBSEP ln) in covered)) print
  }'
)

if [[ -n "${trigger12_hits}" ]]; then
  echo 'VIOLATION 12: SP-6 lock placement in public API'
  echo "see ${trigger_doc} (trigger 12)"
  echo "${trigger12_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    12: SP-6 lock placement in public API"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 13 (D-block PR-C-3 §U45, architecture-correction-plan SP-8) —
# constructor-time panics.
#
# `unwrap()` / `expect()` / `panic!(...)` / `assert!(...)` inside a
# public CONSTRUCTOR (`pub fn new` / `pub fn from_*` / `pub fn try_*`)
# turns argument-validation bugs into process aborts at the public
# API surface. The SP-8 verdict is that public constructors should
# return `Result` or take pre-validated types; `debug_assert!` is
# allowed (compiled out in release).
#
# Mechanical scope (heuristic, deliberately narrow — catches the
# clearest shapes, accepts false-negatives over false-positives):
#   - single-line constructor bodies: `pub fn new(...) -> Self { ...
#     .unwrap()/.expect()/panic!/assert! ... }`
#   - inline body with one of the panic forms on the SAME line as the
#     `pub fn (new|from_*|try_*)` signature.
#
# Multi-line constructor bodies are NOT inspected — that requires
# real-AST traversal; rustc + clippy lints (`clippy::expect_used`,
# `clippy::unwrap_used`) cover that surface where opted in.
#
# Allowlist marker grammar:
#   pub fn new() -> Self { foo.unwrap() }  // PORT-CHECK-OK-SP8: <reason>
# -----------------------------------------------------------------------------
trigger13_hits=$(rg --line-number --no-heading \
    'pub fn (new|from_[a-z_]+|try_[a-z_]+)\b[^{]*\{[^}]*(\.unwrap\(\)|\.expect\(|panic!\s*\(|assert!\s*\()' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    --glob '!examples/**' \
    --glob '!crates/flui-platform/src/platforms/**' \
    crates/ 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  | grep -Ev 'debug_assert' \
  | grep -Ev '//\s*PORT-CHECK-OK-SP8:' \
  || true)

if [[ -n "${trigger13_hits}" ]]; then
  echo 'VIOLATION 13: SP-8 constructor-time panics (unwrap/expect/panic!/assert! in pub constructor body)'
  echo "see ${trigger_doc} (trigger 13)"
  echo "${trigger13_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    13: SP-8 constructor-time panics"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 14 (N-geom polish pass §U12) — unit-barrier escape hatches in
# flui-geometry.
#
# The `flui-geometry` polish pass (U1/U2/U4/U6) removed the implicit
# conversions and cross-type operators that let an untyped scalar leak across
# the unit boundary. This trigger keeps them gone — the next contributor who
# adds "just one quick conversion" re-opens the bug class the pass closed.
#
# Patterns flagged (in crates/flui-geometry/src/ only):
#   impl From<f32|f64> for <UnitWrapper>     (use px(..) / ::new(..) instead)
#   impl PartialEq<f32>  for <UnitWrapper>   (compare against px(..))
#   impl PartialOrd<f32> for <UnitWrapper>
#   impl Add<f32> for <UnitWrapper>          (Mul/Div<f32> stay — scaling is ok)
#   impl Sub<f32> for <UnitWrapper>
#   pub type Float(Point|Vec2|Size|Offset)   (dead GPU-ready aliases)
#
# Allowlist marker grammar (±2 line window, as trigger #12):
#   <decl>  // PORT-CHECK-OK-UNIT: <reason>
# -----------------------------------------------------------------------------
trigger14_raw=$(rg --line-number --no-heading \
    -e '^\s*impl From<(f32|f64)> for ' \
    -e '^\s*impl (PartialEq|PartialOrd|Add|Sub)<f32> for ' \
    -e '^\s*pub type Float(Point|Vec2|Size|Offset)\b' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    crates/flui-geometry/src/ 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

# Optimised: one rg call collects all UNIT markers; one awk pass via
# single pipe (trigger 14 window −2..+2, no process substitution).
trigger14_unit_markers=$(rg --line-number --no-heading 'PORT-CHECK-OK-UNIT:' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    crates/flui-geometry/src/ 2>/dev/null || true)
trigger14_hits=$(
  { printf '%s\n' "${trigger14_unit_markers}"; printf '%s\n' '---T14SPLIT---'; printf '%s\n' "${trigger14_raw}"; } | \
  awk -F':' '
  /^---T14SPLIT---$/ { past_split = 1; next }
  !past_split {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    # Trigger 14 window is −2..+2: marker at M sanctions def at M−2..M+2.
    for (d = -2; d <= 2; d++) covered[fp SUBSEP (ln + d)] = 1
    next
  }
  {
    if (NF < 2) next
    fp = $1; gsub(/\\/, "/", fp)
    ln = int($2)
    if (!((fp SUBSEP ln) in covered)) print
  }'
)

if [[ -n "${trigger14_hits// /}" && -n "$(echo "${trigger14_hits}" | tr -d '[:space:]')" ]]; then
  echo 'VIOLATION 14: U12 unit-barrier escape hatch in flui-geometry (From<scalar>/cross-type f32 op/Float* alias)'
  echo "see ${trigger_doc} (trigger 14)"
  echo "${trigger14_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    14: U12 unit-barrier (flui-geometry)"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 9 (FR-036, Phase 3.1 §U30) — sanctioned `dyn`-boundary registry.
#
# Greps every `Box<dyn …>`, reference `dyn …` (in any of the four reference
# forms — `&dyn`, `&mut dyn`, `&'a dyn`, `&'a mut dyn`), `Arc<dyn …>`,
# `Rc<dyn …>` introduction across the framework crates (`flui-view`,
# `flui-foundation`, `flui-tree`, `flui-engine`, `flui-rendering`,
# `flui-interaction`) AND every type alias of the same shape (`type X =
# Box<dyn …>`, including generic aliases `type X<T> = …` and reference
# aliases `type X = &'a dyn …`).
#
# Two filter layers gate what reaches the marker check:
#
# 1. **Language-runtime exempts** (FR-029 categorical exempt list —
#    universal patterns, not framework `dyn` introductions):
#    - `Pin<Box<dyn Future<…>>>` and `Box<dyn Future<…>>` (async runtime).
#    - `Box<dyn Iterator<…>>` (lazy enumeration).
#    - reference-form `dyn Fn*` callback-parameter binds (`&dyn Fn(…)`,
#      `&mut dyn Fn(…)`, `&'a dyn FnMut(…)`, etc.) — distinct from OWNED
#      callback storage (`Box<dyn Fn(…) + Send + Sync>`), which is
#      FR-029 #5 sanctioned via the allowlist below.
#
# 2. **Sanctioned trait allowlist** (FR-029 1–5 + the pre-existing
#    `View` / `ViewKey` / `BuildContext` surfaces, plus `Fn` / `FnMut` /
#    `FnOnce` for FR-029 #5 owned callback storage). These are widely-used
#    sanctioned trait surfaces in the framework; per-site marker discipline
#    would explode to ~500+ markers (mostly `&dyn View` function parameters
#    + ~96 `Arc<dyn Fn(…)>` callback-storage type aliases). The allowlist
#    captures the categories sanctioned by FR-029 by name; any NEW `dyn
#    Trait` outside the list either gets a marker or refactors. The
#    allowlist is intentionally narrow — `Trait` matches must be EXACT
#    names from this list, not regex prefixes.
#
# Hits that survive both filters must carry `// PORT-CHECK-OK-DYN: <reason>`
# on the SAME line as the matched pattern.
#
# Marker grammar:
#   <something with Box<dyn Foo>>  // PORT-CHECK-OK-DYN: <one-line justification>
#
# **Multi-line declaration handling**: this script intentionally does NOT
# use `rg -U` multiline mode for the trigger. Mixing `rg -U`'s multi-line
# output blocks with line-oriented `grep -Ev` filters partial-filters
# multi-line matches — a marker on the trait-name line gets dropped while
# the `Box<` line slips through, producing false positives, and the
# converse silently bypasses enforcement. The single-line scan instead
# catches rustfmt-formatted code (which prefers `Box<dyn Trait>` on one
# line whenever possible) and lets `cargo fmt` collapse multi-line splits
# back to single-line before this trigger runs in CI. Authors who
# deliberately split a declaration across lines for width can either
# keep the marker on the `Box<` line (matched here) or refactor to a
# `type` alias that fits one line + carries its own marker.
#
# Type aliases use the same marker convention; alias declarations carry
# their own marker, and downstream uses inherit the sanctioning (the alias
# name does not contain `dyn`, so the trigger does not see it again).
# -----------------------------------------------------------------------------

# Sanctioned trait allowlist — `|`-joined alternation read inline by the
# subsequent `grep -E`. The expression allows an optional path prefix
# (`crate::`, `std::`, `flui_foundation::`, etc.) between `dyn` and the
# trait name so `dyn crate::ElementBase` and `dyn ElementBase` both match.
# Add a trait here when its `dyn` usage is widespread enough that
# per-site markers become noise; remove only after auditing that the
# trait's `dyn` surface is genuinely gone.
#
# Categories (FR-029 sanctioning):
#   #1 element-storage sub-traits: ElementBase, ElementBehavior,
#      StatelessElementBase, StatefulElementBase, ProxyElementBase,
#      InheritedElementBase, RenderElementBase
#   #2 BoxedView dynamic-children: View, BoxedView, ViewObject
#   #4 pipeline-owner type-erasure: Any
#   #5 error chains + observer/animation + owned callback storage:
#      Error, std::error::Error, core::error::Error, Listenable,
#      Animation, WidgetsBindingObserver, Fn, FnMut, FnOnce
#      (Fn/FnMut/FnOnce included via allowlist rather than per-site
#      markers — see commit body of Phase 3.1 §U30 for the plan-time-
#      vs-reality rationale; 96 owned-callback storage sites would have
#      required per-site markers under the original plan §U30.4 sweep.)
#   #6 protocol-layout-erasure at the RenderObject<P>::perform_layout_raw
#      trait seam (D-block PR-A1b U19 / companion memo D5):
#      BoxLayoutCtxErased, SliverLayoutCtxErased. The trait-object form
#      lets the pipeline / RenderEntry hand a typed layout context to
#      the erased perform_layout_raw method without per-protocol dispatch
#      in the caller; the blanket impl reconstructs the typed
#      `BoxLayoutCtx<T::Arity, T::ParentData>` via the Proxy storage
#      ctor (`BoxLayoutCtx::from_erased`). This is a sanctioned shape
#      analogous to FR-029 #4 (pipeline-owner type-erasure) — without
#      `dyn`, RenderObject<P> would either ripple the typed context
#      into every impl (16+ user-widget call sites) or fragment the
#      perform_layout_raw API per-protocol.
#   Pre-existing surfaces: ViewKey, BuildContext, Notification,
#                          NotifiableElement, RenderObject, RenderObjectTrait
#   Framework trait surfaces (gesture / focus / delegate / parent-data /
#   clipper / binding patterns — widely-used reference shapes; their
#   owned-storage uses sit on sanctioned FR-029 categories):
#   GestureArenaMember, FocusTraversalPolicy, SliverGridDelegate,
#   SingleChildLayoutDelegate, MultiChildLayoutDelegate, FlowDelegate,
#   CustomPainter, ParentData, CustomClipper, RendererBinding, Debug
#   #6-adjacent: LogicalIndexParentData — the pub(crate) ParentData sub-trait the
#   re-entrant build contract (ADR-0003 U3c) uses to stamp the logical item index
#   through at deferred-insert apply, keeping the generic insert path parent-data-
#   agnostic. Sanctioned by the same FR-029 #6 rationale as the *LayoutCtxErased
#   erasure traits below.
fr036_allowed='dyn\s+(\$crate::|[a-zA-Z_][a-zA-Z0-9_]*::)*(View|ViewKey|BuildContext|ElementBase|ElementBehavior|StatelessElementBase|StatefulElementBase|ProxyElementBase|InheritedElementBase|RenderElementBase|InheritedElementAccess|RenderObjectTrait|RenderObject|Listenable|Notification|NotifiableElement|WidgetsBindingObserver|Animation|BoxedView|ViewObject|Any|Error|GestureArenaMember|MonotonicClock|FocusTraversalPolicy|SliverGridDelegate|SingleChildLayoutDelegate|MultiChildLayoutDelegate|MultiChildLayoutContext|FlowDelegate|CustomPainter|ParentData|LogicalIndexParentData|CustomClipper|RendererBinding|HitTestable|Debug|Fn|FnMut|FnOnce|BoxLayoutCtxErased|SliverLayoutCtxErased|ChildManager)\b'

# Framework crates under enforcement.
fr036_scope=(
  crates/flui-view/src
  crates/flui-foundation/src
  crates/flui-tree/src
  crates/flui-engine/src
  crates/flui-rendering/src
  crates/flui-interaction/src
)

# Reference-form prefix covering all four `&`/`&mut`/`&'a`/`&'a mut` shapes
# the borrow-checker recognizes. Embedded inside the `-e` patterns below.
fr036_ref_prefix="&\\s*('[a-zA-Z_][a-zA-Z0-9_]*\\s+)?(mut\\s+)?dyn\\s+"

# Pre-filter language-runtime exempts before the marker / allowlist check.
# Single-line `rg` (no -U) so the line-oriented `grep -Ev` filters below
# act on each matched line in isolation — multi-line declarations are
# explicitly out of scope per the header comment (rustfmt collapses them).
fr036_hits=$(rg --line-number --column \
    -e 'Box<\s*dyn\s+' \
    -e "${fr036_ref_prefix}" \
    -e 'Arc<\s*dyn\s+' \
    -e 'Rc<\s*dyn\s+' \
    "${fr036_scope[@]}" 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  | grep -Ev '//\s*PORT-CHECK-OK-DYN:' \
  | grep -Ev 'Pin<\s*Box<\s*dyn\s+([a-zA-Z_][a-zA-Z0-9_]*::)*Future|Box<\s*dyn\s+([a-zA-Z_][a-zA-Z0-9_]*::)*Future|Box<\s*dyn\s+([a-zA-Z_][a-zA-Z0-9_]*::)*Iterator' \
  | grep -Ev "${fr036_ref_prefix}Fn[A-Za-z]*\\s*[(<]|${fr036_ref_prefix}FnMut|${fr036_ref_prefix}FnOnce" \
  | grep -Ev "${fr036_allowed}" \
  || true)

# Type-alias closure: catch `type X = Box<dyn Y>` / `type X<T> = Arc<dyn Y>`
# / `type X = &'a dyn Y` etc. The LHS accepts an optional generic-parameter
# list (`<T>`, `<'a, T>`); the RHS accepts all four reference-form prefixes
# alongside `Box`, `Arc`, `Rc`. Alias declarations get their own marker;
# downstream uses of the alias name don't trip the trigger.
fr036_alias_hits=$(rg --line-number --column \
    "type\\s+\\w+(\\s*<[^>]*>)?\\s*=\\s*(Box|${fr036_ref_prefix%dyn\\\\s+}|Arc|Rc)<?\\s*dyn\\s+" \
    "${fr036_scope[@]}" 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  | grep -Ev '//\s*PORT-CHECK-OK-DYN:' \
  | grep -Ev "${fr036_allowed}" \
  || true)

fr036_combined=""
if [[ -n "${fr036_hits}" ]]; then
  fr036_combined="${fr036_hits}"
fi
if [[ -n "${fr036_alias_hits}" ]]; then
  if [[ -n "${fr036_combined}" ]]; then
    fr036_combined="${fr036_combined}
${fr036_alias_hits}"
  else
    fr036_combined="${fr036_alias_hits}"
  fi
fi

if [[ -n "${fr036_combined}" ]]; then
  echo 'VIOLATION 9: sanctioned dyn-boundary registry (FR-036)'
  echo "see ${trigger_doc} (trigger 9) and specs/004-view-element-core/spec.md FR-036"
  echo "${fr036_combined}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    9: sanctioned dyn-boundary registry (FR-036)"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 15 (core-0a adversarial-reaudit PR-4 §U5) — println!/eprintln!/dbg!
# in foundation/tree/macros production source.
#
# Foundation, tree, and macros are the framework's low-level substrate;
# they must route diagnostics through `tracing::{error,warn,info,debug,
# trace}!`, never stdout/stderr macros. A stray `println!`/`eprintln!`/
# `dbg!` in this layer leaks unstructured output into every downstream
# binary and is invisible to the tracing subscriber.
#
# Exclusions:
#   - doc comments (`//!`, `///`, `//`) — example code in docs is fine.
#   - tests (`tests/`, `test*.rs`, in-file `#[cfg(test)]` is acceptable;
#     the path globs drop dedicated test files — see note below).
#
# NOTE: in-file `#[cfg(test)]` modules are NOT post-filtered here (unlike
# trigger 8). The three crates in scope keep their test output via
# `assert!`/`tracing`, not `println!`, so the path-glob exclusion of
# dedicated test files is sufficient; if a future `#[cfg(test)]` block
# legitimately needs `println!`, append a `test*.rs`-style split or a
# per-line allowlist marker in the same PR.
# -----------------------------------------------------------------------------
trigger15_hits=$(rg --line-number --column \
    -e '\bprintln!\s*\(' \
    -e '\beprintln!\s*\(' \
    -e '\bdbg!\s*\(' \
    --type rust \
    --glob '!**/tests/**' \
    --glob '!**/test*.rs' \
    crates/flui-foundation/src \
    crates/flui-tree/src \
    crates/flui-macros/src 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

if [[ -n "${trigger15_hits}" ]]; then
  echo 'VIOLATION 15: println!/eprintln!/dbg! in foundation/tree/macros production source'
  echo "see ${trigger_doc} (trigger 15)"
  echo "${trigger15_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    15: no println!/eprintln!/dbg! in foundation/tree/macros source"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 16 (core-0a adversarial-reaudit PR-4 §U5) — module-level
# `#![allow(unsafe_code)]` in foundation/tree source.
#
# Edition-2024 idiom (F9): a module that genuinely needs `unsafe` must use
# `#![expect(unsafe_code, reason = "...")]` so the lint fires the day the
# last `unsafe` block is removed; a module with no `unsafe` must carry
# neither attribute. A blanket `#![allow(unsafe_code)]` silently permits
# any future unsafe and never self-cleans — it is forbidden in these two
# crates. (`#![expect(unsafe_code, ...)]` is the sanctioned form and does
# NOT match this pattern.)
# -----------------------------------------------------------------------------
trigger16_hits=$(rg --line-number --column \
    '^\s*#!\[allow\(unsafe_code' \
    --type rust \
    crates/flui-foundation/src \
    crates/flui-tree/src 2>/dev/null \
  || true)

if [[ -n "${trigger16_hits}" ]]; then
  echo 'VIOLATION 16: module-level #![allow(unsafe_code)] in foundation/tree source (use #![expect(...)] or delete)'
  echo "see ${trigger_doc} (trigger 16)"
  echo "${trigger16_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    16: no module-level #![allow(unsafe_code)] in foundation/tree source"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 17 (core-0a adversarial-reaudit PR-4 §U5) — reinvented
# `debug_assert_*` macros in foundation source.
#
# F29 deleted `debug_assert_valid!` / `debug_assert_range!` /
# `debug_assert_finite!` / `debug_assert_not_nan!` — they reinvented
# stdlib `debug_assert!` with no added value. This trigger prevents their
# reintroduction: any `macro_rules!` defining one of these four names in
# foundation source is a regression. Stdlib `debug_assert!` is the
# canonical form.
# -----------------------------------------------------------------------------
trigger17_hits=$(rg --line-number --column \
    'macro_rules!\s+(debug_assert_valid|debug_assert_range|debug_assert_finite|debug_assert_not_nan)\b' \
    --type rust \
    crates/flui-foundation/src 2>/dev/null \
  || true)

if [[ -n "${trigger17_hits}" ]]; then
  echo 'VIOLATION 17: reinvented debug_assert_* macro defined in foundation source (use stdlib debug_assert!)'
  echo "see ${trigger_doc} (trigger 17)"
  echo "${trigger17_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    17: no reinvented debug_assert_* macros in foundation source"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 18 (core-0a adversarial-reaudit PR-4 §U5) — `new_unchecked` in
# key.rs.
#
# F2 replaced `NonZeroU64::new_unchecked` in `Key::new` with the
# `fetch_update` sentinel pattern, eliminating the UB-on-counter-wrap
# hazard. This trigger guards against reintroducing any `new_unchecked`
# call into `crates/flui-foundation/src/key.rs` — the key counter must
# stay on the safe checked path. (`*_unchecked` constructors elsewhere,
# e.g. id.rs, are out of scope and governed by their own `#![expect(
# unsafe_code, ...)]`.)
# -----------------------------------------------------------------------------
trigger18_hits=$(rg --line-number --column \
    '\bnew_unchecked\b' \
    crates/flui-foundation/src/key.rs 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

if [[ -n "${trigger18_hits}" ]]; then
  echo 'VIOLATION 18: new_unchecked in key.rs (key counter must stay on the checked fetch_update path)'
  echo "see ${trigger_doc} (trigger 18)"
  echo "${trigger18_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    18: no new_unchecked in key.rs"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 19 (engine overhaul T9f; extended to the replay/ submodules in T10e) —
# `Matrix4` in the DrawBatcher record side, `PipelineCache`/`PipelineBuilder`
# (record/pipeline modules), or `GpuReplay` (replay/submit module).
#
# C4 rule: `Matrix4`↔glam conversions must happen at the `Backend` trait
# boundary (crates/flui-engine/src/wgpu/backend.rs). The hot record path
# (`batches/`), the pipeline-cache module (`pipelines.rs`), and the
# replay/submit module (`replay/`) must be glam-only; importing or
# accepting `Matrix4` in any of these leaks the flui-types coordinate type
# into the GPU plumbing layer and breaks the seam contract established in
# the engine overhaul spec (T9/T10 split). The replay side must stay
# glam-only for the same reason as the record side: the `Matrix4`↔glam
# conversion must not migrate into the GPU-emit path.
#
# Allowlist: none. The correct fix is always to extract the needed scalar
# fields (translation, scale) at the caller in backend.rs / painter.rs and
# pass primitives down.
# -----------------------------------------------------------------------------
trigger19_hits=$(rg --line-number --column '\bMatrix4\b' \
    crates/flui-engine/src/wgpu/batches \
    crates/flui-engine/src/wgpu/pipelines.rs \
    crates/flui-engine/src/wgpu/replay 2>/dev/null \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)

if [[ -n "${trigger19_hits}" ]]; then
  echo 'VIOLATION 19: Matrix4 in batches/, pipelines.rs, or replay/ (record/pipeline/replay side must be glam-only; convert at the trait boundary)'
  echo "see ${trigger_doc} (trigger 19)"
  echo "${trigger19_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    19: no Matrix4 in batches/, pipelines.rs, or replay/"
  fi
fi

# -----------------------------------------------------------------------------
# Trigger 20: no warn-fallback strings for gradient/image producers (PR-5)
#
# PR-5 deleted three warn-fallback blocks that previously made gradient and
# image producers silently fall through to SrcOver for advanced blend modes.
# If any of these strings reappear in batches/, renderer.rs, or backend.rs,
# a producer has regressed to the fallback path and advanced blend will
# silently produce wrong output for those draw calls.
#
# replay.rs is excluded: it legitimately uses similar language in its own
# documentation and is never a producer (it is the replay/submit side).
#
# The runtime half of this gate is PipelineCache::get_or_create's
# debug_assert!(!key.blend_mode().is_advanced(), …) which panics in GPU
# tests if an advanced mode reaches the pipeline cache.
# -----------------------------------------------------------------------------
trigger20_hits=$(rg --line-number --column \
    -e 'is not supported by the' \
    -e 'rendering as SrcOver' \
    crates/flui-engine/src/wgpu/batches \
    crates/flui-engine/src/wgpu/renderer.rs \
    crates/flui-engine/src/wgpu/backend.rs 2>/dev/null \
  || true)

if [[ -n "${trigger20_hits}" ]]; then
  echo 'VIOLATION 20: gradient/image warn-fallback strings found in batches/, renderer.rs, or backend.rs'
  echo '  These strings were deleted by PR-5; their reappearance signals a producer has regressed to SrcOver fallback.'
  echo "see ${trigger_doc} (trigger 20)"
  echo "${trigger20_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    20: no warn-fallback strings in batches/, renderer.rs, or backend.rs"
  fi
fi

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
if [[ "${violations}" -gt 0 ]]; then
  echo "port-check: ${violations} violation(s) found"
  echo "fix the violations or update docs/PORT.md if the rule itself needs to change"
  exit 1
fi

echo "port-check: all 20 refusal triggers + FR-033 grep clean"

# -----------------------------------------------------------------------------
# Marker summary (verbose mode only). Non-blocking — markers are Phase B
# work-queue, not violations. See docs/PORT.md ## Inline port markers tier.
# -----------------------------------------------------------------------------
if [[ "${verbose}" -eq 1 ]]; then
  echo ""
  echo "marker budget (TODO(port) / PERF(port) / PORT NOTE):"
  total_todo=0
  total_perf=0
  total_note=0
  # Iterate every crate directory under crates/.
  for crate_dir in crates/*/; do
    crate_name="$(basename "${crate_dir}")"
    # Skip crates without a src/ directory (e.g., flui-macros workspace shim).
    [[ -d "${crate_dir}src" ]] || continue
    c_todo=$(count_markers "TODO(port)" "${crate_dir}src")
    c_perf=$(count_markers "PERF(port)" "${crate_dir}src")
    c_note=$(count_markers "PORT NOTE"  "${crate_dir}src")
    total_todo=$((total_todo + c_todo))
    total_perf=$((total_perf + c_perf))
    total_note=$((total_note + c_note))
    if [[ $((c_todo + c_perf + c_note)) -gt 0 ]]; then
      printf "  %-22s %3d TODO  %3d PERF  %3d NOTE\n" "${crate_name}" "${c_todo}" "${c_perf}" "${c_note}"
    fi
  done
  total_all=$((total_todo + total_perf + total_note))
  if [[ "${total_all}" -eq 0 ]]; then
    echo "  (no markers across crates/)"
  else
    printf "  %-22s %3d TODO  %3d PERF  %3d NOTE\n" "TOTAL" "${total_todo}" "${total_perf}" "${total_note}"
    echo ""
    echo "  Run 'just port-markers' for the per-file breakdown."
  fi
fi

exit 0
