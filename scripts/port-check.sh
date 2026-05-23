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
#   - flui-rendering/src/objects (per-render-object paint impls)
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
  crates/flui-rendering/src/objects \
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
# FR-033 (Phase 3 §U29): downcast_ref::<...View...> in the View-type update
# dispatch path. Scoped to `crates/flui-view/src/element/{generic.rs,
# dispatch.rs}` — the body of `ElementCore::update_view` and its dispatch
# helper. Legitimate non-View-type `downcast_ref` uses (slot attachment in
# `unified.rs`) live outside this scope and are not flagged here; per-line
# whitelist via `// PORT-CHECK-OK-DOWNCAST: <reason>` markers is reserved
# for sites that enter the scope but should be sanctioned individually.
#
# This is a SPEC requirement (FR-033, SC-004) but NOT a numbered refusal
# trigger — refusal trigger #9 (FR-036, plan §U30) is the broader
# sanctioned-`dyn` enforcement; this grep targets a single defect class on
# a tighter scope.
# -----------------------------------------------------------------------------
fr033_hits=$(rg --line-number --column 'downcast_ref::<[^>]*View[^>]*>' \
  crates/flui-view/src/element/generic.rs \
  crates/flui-view/src/element/dispatch.rs 2>/dev/null \
  | grep -Ev '//\s*PORT-CHECK-OK-DOWNCAST:' \
  | grep -Ev ':\s*(//!|///|//)' \
  || true)
if [[ -n "${fr033_hits}" ]]; then
  echo "VIOLATION FR-033: downcast_ref::<...View...> in update-dispatch path"
  echo "see docs/PORT.md (FR-033 enforcement, spec § specs/004-view-element-core/spec.md FR-033)"
  echo "${fr033_hits}"
  echo ""
  violations=$((violations + 1))
else
  if [[ "${verbose}" -eq 1 ]]; then
    echo "ok    FR-033: downcast_ref::<...View...> in update-dispatch path"
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

echo "port-check: all seven refusal triggers + FR-033 grep clean"
exit 0
