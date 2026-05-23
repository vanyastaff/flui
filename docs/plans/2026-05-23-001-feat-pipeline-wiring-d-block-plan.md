---
date: 2026-05-23
title: "feat: Pipeline wiring D-block — Core.0 closure"
type: feat
status: active
origin: docs/brainstorms/pipeline-wiring-d-block-requirements.md
memo: docs/research/2026-05-23-d-block-architecture-decision-memo.md
---

# feat: Pipeline wiring D-block — Core.0 closure

## Summary

Close Core.0's D-block by wiring layout / compositing / paint phase orchestration in `crates/flui-rendering/src/pipeline/owner.rs` to Flutter-shape parity. Delivered as **7 PRs in strict serial merge order** (PR-C-1 → PR-C-2 → PR-B-gate → PR-A1 → PR-A2 → PR-C-3, plus PR-B-followup parallel), 52 atomic-commit-per-unit U-IDs total. Origin: [`docs/brainstorms/pipeline-wiring-d-block-requirements.md`](../brainstorms/pipeline-wiring-d-block-requirements.md) (rev 2); binding architecture decisions: [`docs/research/2026-05-23-d-block-architecture-decision-memo.md`](../research/2026-05-23-d-block-architecture-decision-memo.md) (D1–D12).

---

## Problem Frame

Three pipeline phases in `crates/flui-rendering/src/pipeline/owner.rs` are silent no-ops today (D-1 `layout_node_with_children`, D-3 `run_compositing`, D-4 `run_paint`). ROADMAP names them "single most important new work in Core.0." Pipeline has 3 production consumers via `run_frame` (`crates/flui-app/src/bindings/renderer_binding.rs:438`, `crates/flui-app/src/app/binding.rs:406`, `crates/flui-hot-reload/src/pipeline.rs:161`) — re-layout panics will hit `AppBinding::draw_frame` on frame 2 without `RenderState::set_constraints/set_geometry` `OnceCell` → `Option` migration. Core.1 vertical-slice work cannot proceed until pipeline orchestrates work.

Multi-agent pre-plan-write review surfaced 4 P0 plan-breakers (borrow architecture undefined; `set_*` panic on frame 2; `mark_needs_layout` propagation is greenfield not wiring; `RenderEntry::layout` blanket impl is no-op stub) all resolved in the companion decision memo as binding D1–D12 decisions.

---

## Stakeholder and Impact

- **Render machine downstream**: Core.1 vertical-slice work (slice widgets, demo app) unblocked once D-block closes — direct consumer.
- **`flui-app` runner**: re-layout panic fix required for stable multi-frame loop; `AppBinding::draw_frame` test scope expanded.
- **`flui-hot-reload`**: force-paint contract interacts with R8 paint-clear discipline (D8) — interaction tested.
- **All future widget impls**: `RenderObject<P>::perform_layout_raw` signature change (D5) ripples through every existing `RenderObject` impl — bridge in blanket impl absorbs most, manual impls update.
- **Workspace consumers of `flui-log` (7 sites) and `flui-types::geometry` (25 files)**: mechanical migrations during PR-C-1 and PR-C-2; no API surface change.

---

## High-Level Technical Design

> This illustrates the intended approach and is directional guidance for review, not implementation specification. The implementing agent should treat it as context, not code to reproduce.

### Walk shape transformation (D-1)

**Today**: `run_layout` iterates `dirty.needs_layout` (sorted shallow-first), calls `layout_node_with_children(id, depth)` which depth-first walks children but never invokes `entry.layout()`. Per-node layout never runs in production.

**Target (post-D-1)**: `run_layout` iterates `dirty.needs_layout`, calls `layout_dirty_root(id, cached_or_root_constraints)`. The new function:
1. Guards via `currently_laying_out.insert(id)` (D6); returns `RenderError::LayoutCycle(id)` on re-entry.
2. Disjoint-borrows parent + direct children via `RenderTree::get_parent_and_children_mut(id)` (D1).
3. Constructs typed `BoxLayoutCtx` with `&mut [&mut RenderEntry<BoxProtocol>]` slice.
4. Calls `RenderObject::perform_layout_raw(&mut LayoutCtxErased)` — blanket impl (D5) downcasts to `BoxLayoutContext` and invokes typed `RenderBox::perform_layout(ctx)`.
5. Child layout happens via `ctx.layout_child(idx, constraints)` → typed context calls `entry.layout(constraints)` on the child slot synchronously; recursion threads through the same disjoint-borrow primitive.
6. On success: cache-hit short-circuit on top (R2) wraps `perform_layout_raw`; `set_geometry_replace` + `set_constraints_replace` (D2) update state; `clear_needs_layout`.
7. RAII drop-guard removes from `currently_laying_out` regardless of unwind.

### `mark_needs_layout` propagation (D3)

**Today**: bare flag set; production callers use direct `add_node_needing_layout`. No walk.

**Target**: `PipelineOwner::mark_needs_layout(id)` walks parent chain via `RenderEntry::links().parent()`. For each ancestor: idempotent flag check (skip if already marked); set `NEEDS_LAYOUT`; if `IS_RELAYOUT_BOUNDARY` flag set OR no parent, push to `dirty.needs_layout` via `add_node_needing_layout` and return. Mirrors Flutter `.flutter/.../object.dart:2658-2700`.

### Compositing-bits walk (D-3)

**Today**: `run_compositing` logs no-op, returns Ok(()).

**Target**: walk dirty `needs_compositing` queue (already sorted shallow-first). For each node, port Flutter `_updateSubtreeCompositingBits` (`.flutter/.../object.dart:3226-3258`): read `RenderObject::is_repaint_boundary()` + `always_needs_compositing()` (already-existing accessors); compute subtree `needs_compositing` via post-order child walk; propagate to ancestors as needed. Two new flag bits (`NEEDS_COMPOSITING` + `NEEDS_COMPOSITING_BITS_UPDATE`) added to `RenderFlags` bitset.

### Paint flag-clear discipline (D-4)

**Today**: `run_paint` clears `needs_paint` on dirty list but paints only from `root_id` descent — stale-clear bug.

**Target**: `painted: FxHashSet<RenderId>` populated during paint walk. Post-walk: iterate `dirty.needs_paint`; painted nodes clear flag normally; unreached nodes log `tracing::warn!` + clear (preserves R-15 invariant from PR #109). Both classes drop from dirty queue.

---

## Output Structure

New files / directories created during D-block work:

```
crates/
├── flui-foundation/
│   └── src/
│       └── log/                         (NEW: PR-C-1, was flui-log)
│           ├── mod.rs
│           ├── logger.rs
│           └── android_layer.rs
├── flui-geometry/                       (NEW crate: PR-C-2)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       └── (25 files moved from flui-types/src/geometry/)
└── flui-rendering/
    └── tests/
        ├── common/                      (NEW: PR-A1)
        │   └── mod.rs                   (fixture builders)
        └── pipeline/                    (NEW: PR-A1)
            ├── layout_pipeline_test.rs
            ├── compositing_pipeline_test.rs
            └── paint_pipeline_test.rs

scripts/
└── port-check.sh                        (updated: triggers #8/#10/#11/#12/#13 added in PR-C-3)
```

Per-PR `**Files:**` sections below are authoritative for what each unit creates or modifies.

---

## Implementation Units

> Each U-ID is stable; reordering or splitting does not renumber. Units grouped by PR for readability; merge order is strict serial per § Phased Delivery.

### PR-C-1 — `flui-log` merge into `flui-foundation::log`

Goal: collapse the 82-LOC shallow `flui-log` crate into a `flui_foundation::log` module. Mechanical rename + 7 callsite migration + workspace member removal. **Lowest-risk PR, lands first.**

#### U1. Create `flui-foundation::log` module skeleton

- **Goal**: copy `crates/flui-log/src/*` (3 files) into `crates/flui-foundation/src/log/`; ensure `flui-foundation` re-exports `Logger`, `Level`, `debug!`/`error!`/`info!`/`trace!`/`warn!` macros at `flui_foundation::log::*` and at top-level `flui_foundation::{Level, Logger, ...}` for ergonomic re-import.
- **Requirements**: R14
- **Dependencies**: none
- **Files**:
  - `crates/flui-foundation/src/log/mod.rs` (NEW)
  - `crates/flui-foundation/src/log/logger.rs` (NEW, copy of `crates/flui-log/src/logger.rs`)
  - `crates/flui-foundation/src/log/android_layer.rs` (NEW, copy of `crates/flui-log/src/android_layer.rs`)
  - `crates/flui-foundation/src/lib.rs` (MODIFY — add `pub mod log;` + re-exports)
  - `crates/flui-foundation/Cargo.toml` (MODIFY — add `tracing` + `tracing-subscriber` + `tracing-forest` deps that were in `flui-log/Cargo.toml`)
- **Approach**: pure file move + `mod log;` declaration. Re-export at top-level mirrors current `flui-log` surface (`Logger`, `Level`, macros) so consumer migration in U2 is purely path rewrite.
- **Patterns to follow**: existing module structure inside `crates/flui-foundation/src/` (e.g., `error/`, `id/`).
- **Test scenarios**:
  - Happy path: `cargo build -p flui-foundation` succeeds with new `log` module
  - Test expectation: existing `flui-log` tests carry over via copy; verify they still pass under new module path
- **Verification**: `cargo test -p flui-foundation --lib` exits 0; module surface visible via `cargo doc -p flui-foundation`.

#### U2. Migrate 7 production callsites to `flui_foundation::log::`

- **Goal**: rewrite all `flui_log::` references in 5 consumer files (3 crates: `flui-app`, `flui-cli`, `flui-view`).
- **Requirements**: R14
- **Dependencies**: U1
- **Files** (per ce-repo-research-analyst finding 2026-05-23):
  - `crates/flui-app/src/lib.rs` (lines 59, 80 — re-exports)
  - `crates/flui-app/src/app/direct.rs` (lines 74, 76 — `Logger::new` + `Level::DEBUG`)
  - `crates/flui-app/src/app/runner.rs` (lines 74, 76 — same shape)
  - `crates/flui-cli/src/main.rs` (lines 522, 524, 527)
  - `crates/flui-view/src/lib.rs` (lines 183, 213 — re-exports)
  - `crates/flui-app/Cargo.toml` (MODIFY — drop `flui-log` dep)
  - `crates/flui-cli/Cargo.toml` (MODIFY — drop `flui-log` dep)
  - `crates/flui-view/Cargo.toml` (MODIFY — drop `flui-log` dep)
- **Approach**: mechanical `flui_log::` → `flui_foundation::log::` (or top-level `flui_foundation::` where appropriate). Re-export glob ambiguity check: ensure `pub use flui_foundation::log::{Level, Logger, debug, error, info, trace, warn};` does not collide with existing `flui_foundation::` glob in any consumer's prelude.
- **Patterns to follow**: existing `flui_foundation::` imports already used in consumers (verify no conflict).
- **Test scenarios**:
  - Happy path: `cargo build --workspace` succeeds post-migration
  - Edge case: doctest in any consumer file referencing old `flui_log::` path is also updated
  - Integration: `cargo test -p flui-app -p flui-cli -p flui-view --lib` passes
- **Verification**: `cargo build --workspace` exits 0; `cargo clippy --workspace --all-targets -- -D warnings` exits 0; `grep -r "flui_log::" crates/ docs/` returns empty.

#### U3. Remove `flui-log` from workspace

- **Goal**: delete `crates/flui-log/` directory; remove from `[workspace.members]` and `default-members` in root `Cargo.toml`.
- **Requirements**: R14
- **Dependencies**: U2 (no callers remain)
- **Files**:
  - `Cargo.toml` (MODIFY — remove `"crates/flui-log"` from `members` lines 18 and from `default-members`)
  - `crates/flui-log/` (DELETE entire directory)
- **Approach**: `git rm -r crates/flui-log/` + remove 2 lines from root Cargo.toml.
- **Patterns to follow**: no precedent for crate deletion in this workspace; verify no `Cargo.lock` references survive (`cargo update -p flui-log --offline` should fail with "package not found").
- **Test scenarios**:
  - Happy path: `cargo build --workspace` succeeds without `flui-log`
  - Edge case: `cargo metadata --format-version 1` shows no `flui-log` entries
- **Verification**: workspace builds; `grep -r "flui-log" Cargo.toml crates/*/Cargo.toml` returns empty (except optional historical references in docs).

#### U4. PR-C-1 receipts

- **Goal**: annotate the PR with closure note referencing R14; verify workspace + test gate.
- **Requirements**: R14, R18 (cross-cutting gate)
- **Dependencies**: U3
- **Files**:
  - `docs/brainstorms/pipeline-wiring-d-block-requirements.md` (MODIFY — annotate R14 with PR-C-1 merge commit SHA placeholder)
- **Approach**: final commit; documentation update only.
- **Test scenarios**:
  - Test expectation: none — annotation commit
- **Verification**: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace --lib` + `bash scripts/port-check.sh -v` all exit 0.

---

### PR-C-2 — `flui-geometry` split + Constitution amendment

Goal: extract geometry types from `flui-types` into standalone `flui-geometry` crate; amend Constitution to reflect Edition 2024 / `rust-version = 1.94` and updated layer table.

#### U5. Create `flui-geometry` skeleton crate

- **Goal**: new `crates/flui-geometry/` with `Cargo.toml` + empty `src/lib.rs`; add to `[workspace.members]` and `default-members`.
- **Requirements**: R15
- **Dependencies**: none (independent of U1-U4 but lands in PR-C-2 after PR-C-1 merges)
- **Files**:
  - `crates/flui-geometry/Cargo.toml` (NEW — depends on `serde`, `thiserror` only; no flui deps)
  - `crates/flui-geometry/src/lib.rs` (NEW — empty placeholder)
  - `Cargo.toml` (MODIFY — add member entry)
- **Approach**: minimal crate skeleton; no source moved yet (that's U6).
- **Patterns to follow**: `crates/flui-foundation/Cargo.toml` for shape (Layer-0 crate, no flui deps).
- **Test scenarios**: none yet (skeleton).
- **Verification**: `cargo build -p flui-geometry` succeeds.

#### U6. Move 25 geometry source files

- **Goal**: physically move `crates/flui-types/src/geometry/*.rs` (25 files per ce-repo-research-analyst) into `crates/flui-geometry/src/`.
- **Requirements**: R15
- **Dependencies**: U5
- **Files** (25 files to move):
  - All under `crates/flui-types/src/geometry/`: `bezier.rs`, `bounds.rs`, `circle.rs`, `corner.rs`, `corners.rs`, `edges.rs`, `error.rs`, `length.rs`, `line.rs`, `matrix4.rs`, `mod.rs`, `offset.rs`, `point.rs`, `rect.rs`, `relative_rect.rs`, `rotation.rs`, `rrect.rs`, `rsuperellipse.rs`, `size.rs`, `text_path.rs`, `traits.rs`, `transform.rs`, `transform2d.rs`, `units.rs`, `vector.rs`
  - Destination: `crates/flui-geometry/src/` (rename `mod.rs` → keep as `mod.rs` inside subdir OR flatten into `lib.rs`)
- **Approach**: `git mv` preserves history. Update internal `use crate::` paths inside moved files: `crate::geometry::` → `crate::` (since we're now at top-level of `flui-geometry` crate).
- **Patterns to follow**: how `crates/flui-types/src/styling/` is structured (similar shape, will stay in flui-types).
- **Test scenarios**:
  - Happy path: `cargo build -p flui-geometry` succeeds after path rewrites
  - Edge case: each moved file's `use` statements no longer reference non-geometry types from `flui-types` (if any, those types need exposure via trait abstractions OR move to `flui-geometry` too)
- **Verification**: `cargo build -p flui-geometry` exits 0; `cargo test -p flui-geometry --lib` exits 0 (tests carried over via `git mv`).

#### U7. `flui-types` adds dep on `flui-geometry` + re-export bridge

- **Goal**: `flui-types` depends on `flui-geometry`; re-exports the geometry surface (top-level `pub use flui_geometry::{...}` mirroring current `flui-types/src/lib.rs:100` re-exports) for backward compat during transition.
- **Requirements**: R15
- **Dependencies**: U6
- **Files**:
  - `crates/flui-types/Cargo.toml` (MODIFY — add `flui-geometry = { path = "../flui-geometry" }`)
  - `crates/flui-types/src/lib.rs` (MODIFY — replace `pub mod geometry;` with `pub use flui_geometry as geometry;` and update top-level re-exports at line 100 to `pub use flui_geometry::{EdgeInsets, Edges, Matrix4, Offset, Pixels, Point, RRect, Rect, Size};`)
  - `crates/flui-types/src/geometry/` (DELETE — empty after U6 move)
- **Approach**: bridge re-export means existing consumers continue compiling without source changes. Migration of consumers to direct `flui_geometry::` import is OUT OF SCOPE for D-block (deferred to future cleanup).
- **Patterns to follow**: similar re-export pattern in existing `flui-types::*` re-exports.
- **Test scenarios**:
  - Happy path: `cargo build --workspace` succeeds with bridge re-exports
  - Integration: existing consumers of `flui_types::Point`, `flui_types::Size`, etc., continue compiling without modification
- **Verification**: `cargo build --workspace` exits 0; `cargo test --workspace --lib` exits 0.

#### U8. Constitution amendment

- **Goal**: update `.specify/memory/constitution.md` layer table to remove `flui-log`, add `flui-geometry`; correct `edition = "2024"` + `rust-version = "1.94"` (was stale `2021`/`1.91`); bump version 2.3.0 → 2.4.0.
- **Requirements**: R16
- **Dependencies**: U3 (`flui-log` removed) + U5-U7 (`flui-geometry` exists)
- **Files**:
  - `.specify/memory/constitution.md` (MODIFY — Sync Impact Report block + layer table + edition line + version bump)
- **Approach**: hand-edit per audit-verified gaps (per `docs/research/2026-05-22-crate-decomposition-redesign.md` F-2). Update top-of-file Sync Impact Report with rationale ("D-block PR-C-2: flui-geometry split + flui-log merge per FOUNDATIONS Part IV").
- **Test scenarios**:
  - Test expectation: none — documentation update
- **Verification**: `grep -n "edition = \"2021\"" .specify/memory/constitution.md` returns empty; `grep -n "flui-log" .specify/memory/constitution.md` returns empty (or only in historical Sync Impact entries).

#### U9. Update CLAUDE.md version + layer references

- **Goal**: `CLAUDE.md` lines 49-50 reflect new Constitution version (2.4.0); update Active Technologies / Recent Changes blocks with PR-C-2 line.
- **Requirements**: R16
- **Dependencies**: U8
- **Files**:
  - `CLAUDE.md` (MODIFY — version line + layer references)
- **Approach**: minimal edit; keep all other CLAUDE.md content intact.
- **Test scenarios**: none — documentation.
- **Verification**: `grep -n "v2.3.0" CLAUDE.md` returns empty; `grep -n "v2.4.0" CLAUDE.md` returns the updated line.

#### U10. PR-C-2 receipts

- **Goal**: annotate brainstorm R15+R16 with PR-C-2 closure; verify workspace.
- **Requirements**: R15, R16
- **Dependencies**: U9
- **Files**:
  - `docs/brainstorms/pipeline-wiring-d-block-requirements.md` (MODIFY)
- **Test scenarios**:
  - Test expectation: none — annotation
- **Verification**: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace --lib` + `bash scripts/port-check.sh -v` all exit 0.

---

### PR-B-gate — Layer/Semantics Wave 3 + Wave 4 (lifecycle + slab-tree hygiene)

Goal: land the `disposed: AtomicBool` lifecycle protocol + `needs_add_to_scene` dirty-bit propagation (Wave 3) + slab-tree hygiene pair (Wave 4) per [`docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](2026-05-22-004-feat-layer-semantics-repair-plan.md) (waves U8-U13). Gates PR-A2 only.

#### U11. Layer/semantics Wave 3 — `LayerNode::disposed` + Drop guards

- **Goal**: implement Wave 3 U8 from layer/semantics repair plan — `LayerNode::disposed: AtomicBool` + Drop + debug_assert guards mirroring PR #84's `ChangeNotifier::dispose`.
- **Requirements**: R12a (gates D-3)
- **Dependencies**: PR-C-1 + PR-C-2 merged (so flui-foundation::log + flui-geometry exist for any layer-side log calls)
- **Files**: per layer/semantics repair plan Wave 3 U8 — primary `crates/flui-layer/src/tree/layer_tree.rs` (LayerNode), `crates/flui-semantics/src/tree/semantics_tree.rs` (parallel) per plan symmetry rules.
- **Approach**: defer to repair plan U8 specification.
- **Test scenarios**:
  - Happy path: `LayerNode::dispose()` flips flag; subsequent access logs warning + returns gracefully
  - Edge case: double-dispose is idempotent
  - Failure path: post-dispose attempted mutation triggers debug_assert in debug builds
- **Verification**: per repair plan U8 exit criteria.

#### U12. Layer/semantics Wave 3 — `needs_add_to_scene` propagation

- **Goal**: implement Wave 3 U9 from layer/semantics repair plan — `updateSubtreeNeedsAddToScene` propagation matching Flutter.
- **Requirements**: R12a (gates D-3)
- **Dependencies**: U11
- **Files**: per repair plan Wave 3 U9.
- **Approach**: defer to repair plan U9.
- **Test scenarios**: per repair plan U9.
- **Verification**: per repair plan U9.

#### U13. Layer/semantics Wave 4 — slab-tree hygiene

- **Goal**: implement Wave 4 U10-U13 from layer/semantics repair plan — auto-detach on `add_child` + cascade-by-default `remove`, mirrored across `flui-layer` + `flui-semantics`.
- **Requirements**: R12a (gates D-3)
- **Dependencies**: U12
- **Files**: per repair plan Wave 4.
- **Approach**: defer to repair plan Wave 4.
- **Test scenarios**: per repair plan Wave 4.
- **Verification**: per repair plan Wave 4 exit criteria + PR-B-gate cumulative gate.

---

### PR-A1 — D-1 layout pipeline wiring

Goal: rewrite `crates/flui-rendering/src/pipeline/owner.rs` layout phase to actually orchestrate per-node `RenderEntry::layout` calls; author boundary-aware `mark_needs_layout` propagation; bridge blanket `RenderObject<P>` impl to typed `RenderBox::perform_layout`; add cycle detection + dirty-queue dedup. **Largest PR; 17 atomic commits.**

#### U14. PREP — `RenderState::set_constraints` + `set_geometry` OnceCell → Option

- **Goal**: drop `OnceCell` panic-on-already-set; replace with `Option<T>` + `set_constraints_replace` / `set_geometry_replace` (`&mut self`, straight assignment). **MANDATORY prep — without this, R1 wiring panics on frame 2.**
- **Requirements**: R20 (memo D2)
- **Dependencies**: none (independent of all later units; sequenced first within PR-A1)
- **Execution note**: Test-first — write a failing test that calls `RenderEntry::layout` twice with same constraints on a freshly constructed entry, observe panic in current code, then implement fix.
- **Files**:
  - `crates/flui-rendering/src/storage/state/constraints.rs` (MODIFY — `OnceCell<ProtocolConstraints<P>>` → `Option<ProtocolConstraints<P>>`; signatures take `&mut self`; add `set_constraints_replace`)
  - `crates/flui-rendering/src/storage/state/geometry.rs` (MODIFY — analogous; preserve `compute_relayout_boundary` unchanged)
  - `crates/flui-rendering/src/storage/state/mod.rs` (MODIFY — field type updates)
  - `crates/flui-rendering/src/storage/entry.rs` (MODIFY — `RenderEntry::layout` lines 281-283 call `_replace` variants)
- **Approach**: straight field-type swap. Removed: `OnceCell::set` + the explicit panic message. Added: idempotent `Option` replacement. Cite Flutter `.flutter/.../object.dart:2865` (`_constraints = constraints` straight assignment).
- **Patterns to follow**: existing `Option<T>` field patterns inside `RenderState`.
- **Test scenarios**:
  - Happy path: `entry.layout(c)` succeeds twice in a row on the same entry without panic
  - Edge case: `entry.layout(c1)` then `entry.layout(c2)` with different constraints — both succeed; final `state.constraints() == Some(c2)`
  - Integration: AE8 (frame-2 no panic) sub-test
- **Verification**: `cargo test -p flui-rendering --lib state::` exits 0; new test for double-layout passes; `grep -n "OnceCell" crates/flui-rendering/src/storage/state/constraints.rs` returns empty.

#### U15. Author `PipelineOwner::mark_needs_layout(id)` propagation walk

- **Goal**: new method on `PipelineOwner` (phase-agnostic) that walks parent chain, marks each ancestor `NEEDS_LAYOUT`, stops at first `IS_RELAYOUT_BOUNDARY` ancestor OR root, pushes boundary to `dirty.needs_layout`. Mirrors Flutter `markNeedsLayout` (`.flutter/.../object.dart:2658-2700`).
- **Requirements**: R3 (memo D3 — greenfield authoring)
- **Dependencies**: U14 (state setters needed)
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — add `mark_needs_layout(&mut self, id: RenderId)` method)
- **Approach**: loop over parent chain via `entry.links().parent()`; idempotent flag check via `entry.state().needs_layout()`; depth captured via `render_tree.depth(id)`. Return path: push to `dirty.needs_layout` at boundary.
- **Patterns to follow**: existing `add_node_needing_layout` at `pipeline/owner.rs:605`; Flutter source pattern.
- **Test scenarios**:
  - Happy path: marking leaf in a 3-deep tree where root is the only relayout boundary propagates flag to all 3 levels; dirty list contains root only
  - Edge case: marking root directly is idempotent; dirty list contains root once even with re-mark
  - Edge case: marking node in a subtree where intermediate ancestor IS relayout boundary stops propagation at that ancestor; dirty list contains intermediate ancestor only
  - Failure path: marking non-existent RenderId returns silently (Flutter doesn't panic on stale marks)
- **Verification**: unit tests on `mark_needs_layout` cover the 4 scenarios above.

#### U16. Migrate `flui-view` + `flui-hot-reload` callers to `mark_needs_layout`

- **Goal**: update 2 production callsites to use new `PipelineOwner::mark_needs_layout(id)` instead of direct `add_node_needing_layout`.
- **Requirements**: R21 (memo D3 — migration)
- **Dependencies**: U15
- **Files**:
  - `crates/flui-view/src/element/behavior_commons.rs` (MODIFY — line 244 `mark_render_needs_layout_and_paint` helper switches to `owner.mark_needs_layout(render_id)` for layout side; paint side keeps `add_node_needing_paint` direct)
  - `crates/flui-hot-reload/src/pipeline.rs` (MODIFY — line 156 if layout-side mark exists; keep `add_node_needing_paint` direct per force-paint contract)
- **Approach**: replace `owner.add_node_needing_layout(render_id, depth)` with `owner.mark_needs_layout(render_id)`. `depth` parameter no longer needed (the walk recomputes per ancestor).
- **Patterns to follow**: keep `add_node_needing_paint` direct path unchanged (paint doesn't have relayout-boundary semantics).
- **Test scenarios**:
  - Integration: `cargo test -p flui-view --lib` passes after migration
  - Integration: `cargo test -p flui-hot-reload --lib` passes
- **Verification**: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` exit 0.

#### U17. Bootstrap `compute_relayout_boundary` inside `RenderEntry::layout`

- **Goal**: after `set_constraints_replace`, call `state.compute_relayout_boundary(parent_uses_size=false, sized_by_parent=false, has_parent=parent.is_some())`. Bootstrap so `IS_RELAYOUT_BOUNDARY` flag is meaningful for R3 propagation.
- **Requirements**: R26 (memo D3)
- **Dependencies**: U14
- **Files**:
  - `crates/flui-rendering/src/storage/entry.rs` (MODIFY — `RenderEntry::layout` body adds compute call after set_constraints)
- **Approach**: `parent_uses_size = false` for now (Core.2 work brings dynamic recomputation — explicitly out of scope for D-block). `has_parent` derived from `links().parent()`.
- **Test scenarios**:
  - Happy path: root node (no parent) gets `IS_RELAYOUT_BOUNDARY = true` after first layout
  - Happy path: child node with tight constraints gets `IS_RELAYOUT_BOUNDARY = true` (constraints.is_tight())
  - Edge case: child node with loose constraints + no `parent_uses_size` signal gets `IS_RELAYOUT_BOUNDARY = false`
- **Verification**: unit tests verify flag state matches Flutter formula `!parent_uses_size || sized_by_parent || constraints.is_tight() || !has_parent`.

#### U18. Add `RenderNode::layout_erased` + `ErasedConstraints` / `ErasedGeometry` enums

- **Goal**: protocol-erasure dispatch at pipeline → entry seam. Pipeline operates on `RenderNode` (enum over protocols); per-protocol typed dispatch happens inside `RenderNode::layout_erased`.
- **Requirements**: R22 (memo D4)
- **Dependencies**: none structural (independent of U14-U17)
- **Files**:
  - `crates/flui-rendering/src/storage/node.rs` (MODIFY — add `ErasedConstraints`, `ErasedGeometry` enums; add `RenderNode::layout_erased` method)
  - `crates/flui-rendering/src/error.rs` (MODIFY — add `RenderError::ProtocolMismatch` variant)
- **Approach**: enum over `BoxConstraints` and `SliverConstraints`. Match-and-dispatch inside `RenderNode::layout_erased`; mismatch returns `ProtocolMismatch`.
- **Patterns to follow**: existing `RenderNode::is_relayout_boundary` pattern-match at `storage/node.rs:304-307`.
- **Test scenarios**:
  - Happy path: `RenderNode::Box(entry).layout_erased(Box(c))` → `Box(geometry)`
  - Happy path: `RenderNode::Sliver(entry).layout_erased(Sliver(c))` → `Sliver(geometry)` (deferred — no slivers in D-block test surface, but enum + dispatch present)
  - Failure path: `RenderNode::Box(entry).layout_erased(Sliver(c))` → `Err(ProtocolMismatch)`
- **Verification**: unit tests cover all 3 scenarios.

#### U19. Rewrite `RenderObject<P>::perform_layout_raw` signature + `RenderBox` bridge

- **Goal**: change `perform_layout_raw` signature to accept `&mut dyn LayoutCtxErased`. Blanket impl at `traits/render_box.rs:380-395` downcasts to typed `BoxLayoutContext`, calls `T::perform_layout(ctx)`, captures returned size.
- **Requirements**: R23 (memo D5)
- **Dependencies**: U18 (ErasedConstraints exists; LayoutCtxErased uses it)
- **Files**:
  - `crates/flui-rendering/src/traits/render_object.rs` (MODIFY — `perform_layout_raw` signature)
  - `crates/flui-rendering/src/traits/render_box.rs` (MODIFY — blanket impl bridge body)
  - `crates/flui-rendering/src/protocol/box_protocol.rs` (MODIFY — `LayoutCtxErased` trait + downcast helpers)
  - `crates/flui-rendering/src/storage/entry.rs` (MODIFY — `RenderEntry::layout` constructs `LayoutCtxErased` wrapping `BoxLayoutContext` before calling perform_layout_raw)
- **Approach**: `LayoutCtxErased` is a trait-object-friendly wrapper that the blanket impl downcasts. Sliver bridge deferred (Core.2 work).
- **Patterns to follow**: existing erased-via-enum patterns (e.g., `ErasedConstraints` from U18); existing typed `BoxLayoutContext` construction at `protocol/box_protocol.rs:207`.
- **Test scenarios**:
  - Happy path: `RenderColoredBox` (Leaf) layout via blanket bridge returns correct Size
  - Happy path: `RenderPadding` (Single) layout via bridge correctly forwards to `T::perform_layout(BoxLayoutContext<Single>)`
  - Happy path: `RenderFlex` (Variable) layout via bridge correctly handles child slice
- **Verification**: unit tests on all 3 arity classes; ripple check on other `RenderObject` impls (e.g., `RenderViewAdapter`).

#### U20. Implement disjoint-borrow walk in `layout_dirty_root`

- **Goal**: new method `PipelineOwner::layout_dirty_root(id, constraints)` uses `RenderTree::get_parent_and_children_mut(id)` (memo D1). Replaces `layout_node_with_children` recursion shape.
- **Requirements**: R19 (memo D1)
- **Dependencies**: U19 (bridge in place)
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — new `layout_dirty_root` method)
  - `crates/flui-rendering/src/storage/tree.rs` (USE — existing `get_parent_and_children_mut` at lines 215-301; no change needed if signature is sufficient)
- **Approach**: pipeline obtains disjoint mut refs to parent + children, constructs typed context with `&mut [&mut RenderEntry<BoxProtocol>]`, calls `parent.as_box_mut().unwrap().perform_layout_raw(&mut ctx)`. The typed context's `layout_child(idx, constraints)` calls `entry.layout(constraints)` on the child slot synchronously.
- **Patterns to follow**: existing `RenderTree::get_two_mut` at tree.rs:215 (sibling primitive).
- **Test scenarios**:
  - Happy path: 2-level tree (parent + 2 children) — parent's perform_layout sees correct child sizes after `layout_child` calls
  - Edge case: 3-level tree (Padding → Center → ColoredBox) — middle node sees correct leaf size via grandchild propagation
  - Failure path: child slice access out of bounds returns `RenderError::ChildIndexOutOfBounds`
- **Verification**: integration tests AE1, AE3 cover happy paths.

#### U21. Add `currently_laying_out: FxHashSet<RenderId>` + RAII guard + `RenderError::LayoutCycle`

- **Goal**: cycle detection per memo D6. RAII drop-guard ensures unwind safety.
- **Requirements**: R24 (memo D6)
- **Dependencies**: U20
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — add field + guard struct + entry/exit logic in `layout_dirty_root`)
  - `crates/flui-rendering/src/error.rs` (MODIFY — add `LayoutCycle(RenderId)` variant)
  - `Cargo.toml` (MODIFY — add `rustc-hash = "2.0"` to `[workspace.dependencies]` if not present)
- **Approach**: insert on entry to `layout_dirty_root`; RAII drop-guard removes on scope exit (regardless of unwind). Cycle re-entry returns Err.
- **Test scenarios**:
  - Happy path: linear tree layout doesn't trigger cycle detection
  - Edge case: synthetic A→B→A cycle returns `Err(LayoutCycle(A))`; `currently_laying_out` empty post-error (drop-guard works)
  - Failure path: panic during perform_layout — drop-guard cleans up; set is consistent for next call
- **Verification**: AE9 covers cycle case; unit test for drop-guard via `catch_unwind` + assert empty set.

#### U22. Add dirty-queue dedup (flag check + `mid_layout_marks`)

- **Goal**: dedup per memo D7. Flag-gated no-op + mid-layout side queue.
- **Requirements**: R25 (memo D7)
- **Dependencies**: U21
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — `add_node_needing_layout` + `_paint` + `_compositing` + `_semantics` all check flag + currently_laying_out set; add `mid_layout_marks: Vec<DirtyNode>` field + drain logic in outer `run_layout` loop)
- **Approach**: symmetric across all 4 dirty queues. Flag check via `entry.state().needs_*()`. Mid-layout marks accumulate; outer `while !empty` loop drains them.
- **Test scenarios**:
  - Happy path: marking already-dirty node is no-op (no double-push)
  - Edge case: marking self during own perform_layout → side queue; processed next iteration
  - Edge case: cross-mark (A's perform_layout marks B; B's perform_layout marks C) — both side-queued; processed correctly without infinite loop
- **Verification**: AE6 covers mid-layout-mark case.

#### U23. Rewrite `layout_node_with_children` → `layout_dirty_root` in `run_layout`

- **Goal**: `run_layout` outer loop now calls `layout_dirty_root` instead of `layout_node_with_children`. Delete old `layout_node_with_children` body (recursion shape is wrong post-U20).
- **Requirements**: R1
- **Dependencies**: U22 (all infrastructure in place)
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — `run_layout` body lines 749-790; delete `layout_node_with_children` lines 802-896; delete `LAYOUT_DEPTH_LIMIT` constant)
- **Approach**: outer loop unchanged (sort by depth, iterate dirty); inner call switches to `layout_dirty_root(id, get_cached_constraints_or_root_constraints(id))`. Root constraints sourced from existing storage state (set by `RootRenderElement::mount` at `flui-view/src/view/root.rs:202`) OR from a new `pipeline.root_constraints: Option<BoxConstraints>` field set by binding.
- **Patterns to follow**: existing dirty-queue iteration pattern at owner.rs:754-790.
- **Test scenarios**:
  - Happy path: AE1 (3-level Padding/Center/ColoredBox) passes — full integration through new walk
  - Edge case: empty dirty list → no-op
  - Edge case: re-entry during layout → handled by U21 cycle guard
- **Verification**: AE1 + AE2 integration tests; `grep -n "layout_node_with_children" crates/flui-rendering/` returns empty after deletion.

#### U24. Cache-hit short-circuit in `RenderEntry::layout`

- **Goal**: implement R2 short-circuit per memo D2 example. Skip `perform_layout_raw` if cached constraints match AND not dirty.
- **Requirements**: R2 (memo D2)
- **Dependencies**: U14 (Option field exists), U23 (real layout path)
- **Files**:
  - `crates/flui-rendering/src/storage/entry.rs` (MODIFY — `RenderEntry::layout` top-of-body cache check)
- **Approach**: `if let Some(cached) = self.state.constraints() { if cached == &constraints && !self.state.needs_layout() { return Ok(self.state.geometry().cloned().unwrap_or_default()); } }` before invoking `perform_layout_raw`.
- **Test scenarios**:
  - Happy path: re-layout with same constraints and clean state returns cached geometry without invoking perform_layout (validated via tracking a counter on a mock RenderObject)
  - Edge case: re-layout with DIFFERENT constraints → cache miss, perform_layout invoked
  - Edge case: re-layout with same constraints but dirty flag → cache miss, perform_layout invoked
- **Verification**: AE3 covers cache-hit case; unit test counters verify perform_layout invocation count.

#### U25. Test fixture infrastructure

- **Goal**: new `crates/flui-rendering/tests/common/mod.rs` + `tests/pipeline/` directory with fixture builders.
- **Requirements**: R27 (memo D12)
- **Dependencies**: U24 (real layout path)
- **Files**:
  - `crates/flui-rendering/tests/common/mod.rs` (NEW — `make_three_level_box`, `assert_geometry`, `assert_offset`, `make_two_repaint_boundary_subtrees`, `mid_layout_compositing_dirty_scenario`)
  - `crates/flui-rendering/tests/pipeline/` (NEW dir)
  - `crates/flui-rendering/tests/layout_pipeline_test.rs.disabled` (DELETE)
- **Approach**: builder helpers construct `RenderTree` + `PipelineOwner<Idle>` with the test tree shape. `assert_geometry` reads `entry.state().geometry()` and compares.
- **Patterns to follow**: `crates/flui-view/tests/` for per-crate test conventions; Cycle 5 receipts at `docs/research/2026-05-22-cycle5-receipts.md` for `ReconcileEventCollector` pattern.
- **Test scenarios**:
  - Self-test: `make_three_level_box` builds the expected RenderTree with 3 entries + correct parent links
  - Self-test: `assert_geometry` correctly fails when sizes mismatch
- **Verification**: `cargo test -p flui-rendering --test common` exits 0; tests build.

#### U26. AE1 + R4 test (3-level Padding/Center/ColoredBox)

- **Goal**: integration test exercising R1 + R4 + R20 + R23 — ROADMAP Core.0 D-1 gate test.
- **Requirements**: R1, R4, R20, R23
- **Dependencies**: U25
- **Files**:
  - `crates/flui-rendering/tests/pipeline/layout_pipeline_test.rs` (NEW — test_three_level_box function)
- **Approach**: build tree via `make_three_level_box`; call `pipeline.into_layout().run_layout()`; assert geometries via `assert_geometry`. Expected: Padding 200×100, Center inner 184×84, ColoredBox 80×40 positioned at center.
- **Test scenarios**:
  - **Covers AE1, R4**: happy path full integration as described
- **Verification**: test passes; `cargo test -p flui-rendering --test layout_pipeline_test test_three_level_box` exits 0.

#### U27. AE2 + R5 test (sibling subtree isolation)

- **Goal**: integration test for sibling-independent re-layout per R3 propagation.
- **Requirements**: R3, R5, R21
- **Dependencies**: U25
- **Files**:
  - `crates/flui-rendering/tests/pipeline/layout_pipeline_test.rs` (MODIFY — add test_sibling_isolation)
- **Approach**: build tree with 2 RepaintBoundary subtrees A and B under common root. Call `mark_needs_layout` on node inside A. Run layout. Assert: A's subtree nodes have `needs_layout` cleared post-run; B's subtree nodes untouched (perform_layout invocation counter remains zero for B's nodes).
- **Test scenarios**:
  - **Covers AE2, R5**: sibling isolation as described
- **Verification**: test passes.

#### U28. AE3 + R6 test (cached constraints re-layout)

- **Goal**: integration test for relayout-boundary-isolated re-layout using cached constraints (R2 cache-hit path).
- **Requirements**: R2, R6
- **Dependencies**: U25
- **Files**:
  - `crates/flui-rendering/tests/pipeline/layout_pipeline_test.rs` (MODIFY — add test_cached_constraints_relayout)
- **Approach**: layout tree once; capture perform_layout invocation counts. Mark a boundary node dirty (without changing constraints). Layout again. Assert: counts increased only on dirty subtree; cache-hit short-circuit prevents perform_layout on unchanged-constraints case.
- **Test scenarios**:
  - **Covers AE3, R6**: cache-hit short-circuit verified via counter
- **Verification**: test passes.

#### U29. AE8 test (frame-2 no panic)

- **Goal**: regression test for U14 fix — call `run_layout` twice on same tree without panic.
- **Requirements**: R20
- **Dependencies**: U25
- **Files**:
  - `crates/flui-rendering/tests/pipeline/layout_pipeline_test.rs` (MODIFY — add test_frame_two_no_panic)
- **Approach**: build tree, run_layout once, run_layout again, assert no panic.
- **Test scenarios**:
  - **Covers AE8**: frame-2 safety
- **Verification**: test passes; would have panicked pre-U14.

#### U30. AE9 test (layout cycle detection)

- **Goal**: synthetic cycle test for U21 cycle detection.
- **Requirements**: R24
- **Dependencies**: U25
- **Files**:
  - `crates/flui-rendering/tests/pipeline/layout_pipeline_test.rs` (MODIFY — add test_layout_cycle_returns_err)
- **Approach**: construct two RenderObject mocks where A's perform_layout calls layout_child(B) and B's perform_layout calls layout_child(A). Run layout. Assert: Result is `Err(LayoutCycle(A or B))`; no stack overflow.
- **Test scenarios**:
  - **Covers AE9**: cycle detection
  - Edge case: post-error, `currently_laying_out` set is empty (drop-guard works)
- **Verification**: test passes; explicit cycle path returns typed error not stack overflow.

#### U31. PR-A1 receipts

- **Goal**: annotate brainstorm R1-R6 + R19-R26 with PR-A1 closure; full PR gate verification.
- **Requirements**: R18 cross-cutting
- **Dependencies**: U30
- **Files**:
  - `docs/brainstorms/pipeline-wiring-d-block-requirements.md` (MODIFY)
- **Test scenarios**:
  - Test expectation: none — annotation
- **Verification**: full PR-A1 gate — `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace --lib` + `cargo test -p flui-rendering --test layout_pipeline_test` + `cargo test -p flui-app -p flui-hot-reload --lib` + `bash scripts/port-check.sh -v` all exit 0.

---

### PR-A2 — D-3+D-4 compositing+paint pipeline

Goal: implement Flutter `_updateSubtreeCompositingBits` walk in `run_compositing`; implement R8 paint flag-clear discipline in `run_paint`. 9 atomic commits.

#### U32. Add `NEEDS_COMPOSITING` + `NEEDS_COMPOSITING_BITS_UPDATE` flag bits

- **Goal**: extend `RenderFlags` bitset with 2 new bits per memo D3-3.
- **Requirements**: R7
- **Dependencies**: PR-A1 merged (storage flags shape stable)
- **Files**:
  - `crates/flui-rendering/src/storage/flags.rs` (MODIFY — add bitflag constants; expose getter/setter pairs symmetric with existing flags)
  - `crates/flui-rendering/src/storage/state/flags.rs` (MODIFY — `RenderState` wrappers)
- **Approach**: trivial bitflags addition. Allocate bits 4 + 5 (or next available).
- **Test scenarios**:
  - Self-test: set/get/clear each new flag
- **Verification**: `cargo test -p flui-rendering --lib storage::flags::` exits 0.

#### U33. Bootstrap `IS_REPAINT_BOUNDARY` flag at insert

- **Goal**: every node's `IS_REPAINT_BOUNDARY` flag is set at insert from the `RenderObject::is_repaint_boundary()` hardcoded answer. Currently the flag is never set in production (only `was_` variant at owner.rs:1321).
- **Requirements**: R26b
- **Dependencies**: U32
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — `insert` and `set_root_render_object` set the flag based on `entry.render_object().is_repaint_boundary()`)
- **Approach**: read trait method, set flag immediately after insert into slab.
- **Test scenarios**:
  - Happy path: inserting `RenderViewAdapter` (overrides is_repaint_boundary → true) sets the flag
  - Happy path: inserting `RenderColoredBox` (default false) leaves flag clear
- **Verification**: unit test on insert behavior.

#### U34. Implement `_updateSubtreeCompositingBits` walk in `run_compositing`

- **Goal**: rewrite `run_compositing` body per memo D3-3 + Flutter algorithm at `.flutter/.../object.dart:3226-3258`.
- **Requirements**: R7
- **Dependencies**: U33
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — `run_compositing` body at lines 922-981)
- **Approach**: walk dirty queue (already sorted shallow-first); for each node, run subtree post-order: child compositing OR self.always_needs_compositing OR self.is_repaint_boundary → `NEEDS_COMPOSITING` flag. Propagate to ancestors via parent walk if status changed. Delete stale "no-op" log message.
- **Patterns to follow**: Flutter source as authority.
- **Test scenarios**:
  - Happy path: dirty descendant under repaint boundary triggers compositing flag on the boundary
  - Edge case: node with `always_needs_compositing = true` always has flag set after walk
  - Edge case: disposed-layer mid-walk (interacts with PR-B-gate U11+U12 disposed flag) — skip + tracing::warn
- **Verification**: AE4 + AE7 cover compositing scenarios.

#### U35. Implement R8 paint flag-clear discipline

- **Goal**: rewrite `run_paint` clear logic per memo D8 — `painted: FxHashSet` populated during walk; post-walk discrimination of painted vs unreached.
- **Requirements**: R8
- **Dependencies**: U34 (compositing bits available for paint walk decisions)
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — `run_paint` body at lines 1010-1114)
- **Approach**: during paint walk descending from root, insert each visited node into `painted` set. Post-walk: iterate dirty.needs_paint; painted → clear flag; unreached → log warn + clear (preserve R-15 invariant). Drop dirty entries in both cases.
- **Test scenarios**:
  - Happy path: all dirty nodes reachable from root → all painted → all flags cleared
  - Edge case: RepaintBoundary-isolated subtree marked dirty; paint walk descends; flags cleared only inside subtree
  - Edge case: orphan subtree (parent removed mid-frame) marked dirty; unreached during walk; warn logged + flag cleared (no leak)
- **Verification**: AE5 covers RepaintBoundary case; orphan case covered by added unit test.

#### U36. Remove stale TODO comment

- **Goal**: delete stale comment at `owner.rs:958-960` that said "needs the `RenderObject::always_needs_compositing` + `is_repaint_boundary` bool accessors plumbed through the dyn surface" — they already exist; comment is stale and now misleading.
- **Requirements**: R7
- **Dependencies**: U34
- **Files**:
  - `crates/flui-rendering/src/pipeline/owner.rs` (MODIFY — delete 3 lines)
- **Approach**: pure deletion.
- **Test scenarios**: none.
- **Verification**: `grep -n "needs the .RenderObject::always_needs_compositing" crates/flui-rendering/` returns empty.

#### U37. AE4 + R9 test (compositing-bits propagation)

- **Goal**: integration test for compositing walk per ROADMAP D-3 gate.
- **Requirements**: R7, R9, R26b
- **Dependencies**: U36
- **Files**:
  - `crates/flui-rendering/tests/pipeline/compositing_pipeline_test.rs` (NEW)
- **Approach**: build tree with descendant marked `needs_compositing`; run_compositing; assert `NEEDS_COMPOSITING` flag set correctly on boundary + ancestors per Flutter algorithm.
- **Test scenarios**:
  - **Covers AE4**: compositing-bits propagation as described
- **Verification**: test passes.

#### U38. AE5 + R10 test (RepaintBoundary-isolated paint clear)

- **Goal**: integration test for paint flag-clear per ROADMAP D-4 gate.
- **Requirements**: R8, R10
- **Dependencies**: U36
- **Files**:
  - `crates/flui-rendering/tests/pipeline/paint_pipeline_test.rs` (NEW)
- **Approach**: build tree with RepaintBoundary-isolated subtree; mark node inside dirty; run_paint; assert flag cleared only for painted nodes.
- **Test scenarios**:
  - **Covers AE5**: RepaintBoundary isolation verified
- **Verification**: test passes.

#### U39. AE6 test (mid-layout compositing dirty)

- **Goal**: integration test for memo D7 dedup + mid_layout_marks side queue interaction.
- **Requirements**: R25
- **Dependencies**: U37 (compositing test infra exists)
- **Files**:
  - `crates/flui-rendering/tests/pipeline/compositing_pipeline_test.rs` (MODIFY — add test_mid_layout_compositing_dirty)
- **Approach**: build tree; during run_layout, a node's perform_layout marks a sibling needs_compositing; assert side queue captured the mark; next phase (`run_compositing`) sees it.
- **Test scenarios**:
  - **Covers AE6**: mid-layout dirty handled correctly
- **Verification**: test passes.

#### U40. AE7 test (layer disposal mid-walk)

- **Goal**: integration test for PR-B-gate × D-3 interaction.
- **Requirements**: R7, R12a
- **Dependencies**: U37, PR-B-gate merged
- **Files**:
  - `crates/flui-rendering/tests/pipeline/compositing_pipeline_test.rs` (MODIFY — add test_disposed_layer_mid_walk)
- **Approach**: build tree; dispose a LayerNode mid-compositing walk; assert no panic; tracing::warn logged.
- **Test scenarios**:
  - **Covers AE7**: layer disposal handled gracefully
- **Verification**: test passes.

---

### PR-C-3 — Refusal triggers #8 / #10 / #11 / #12 / #13 install

Goal: install 5 new mechanical refusal triggers in `scripts/port-check.sh`. Lands LAST per close-first precedent — D-block PRs (A1+A2) close SP-1 violations first.

#### U41. Trigger #8 — SP-1 stubbed-but-called

- **Goal**: grep `fn` bodies in non-test modules for empty body / `tracing::warn!`+return / `unimplemented!`/`todo!` shapes; `// STUB-OK: <reason>` allowlist with tracking-issue requirement.
- **Requirements**: R13
- **Dependencies**: PR-A1 + PR-A2 merged (closes D-1/D-3/D-4 SP-1 violations)
- **Files**:
  - `scripts/port-check.sh` (MODIFY — add trigger #8 section)
- **Approach**: mirror existing trigger structure at port-check.sh:60+. Allowlist via grep filter for `// STUB-OK:` markers.
- **Test scenarios**:
  - Self-test: trigger fires on synthetic empty-body fn in non-test code
  - Self-test: `// STUB-OK: testing (issue #N)` marker allowlists the trigger
  - Self-test: trigger green on current codebase post-D-block
- **Verification**: `bash scripts/port-check.sh -v` exits 0 reporting trigger #8 green.

#### U42. Trigger #10 — SP-3 parallel cross-crate types

- **Goal**: collect all `pub struct`/`pub enum`/`pub trait` identifiers across `flui-*` crates; flag any defined (not re-exported) more than once.
- **Requirements**: R13
- **Dependencies**: U41
- **Files**:
  - `scripts/port-check.sh` (MODIFY — add trigger #10 section)
- **Approach**: per architecture-correction-plan §SP-3 spec.
- **Test scenarios**:
  - Self-test: synthetic parallel `pub struct Foo` in two crates flagged
  - Self-test: re-exported type via `pub use` is not flagged
- **Verification**: trigger green on current codebase.

#### U43. Trigger #11 — SP-4 speculative scaffolding

- **Goal**: `pub mod`/trait family with zero production (non-test) consumers, not behind `cfg(feature = "unstable-*")`.
- **Requirements**: R13
- **Dependencies**: U42
- **Files**:
  - `scripts/port-check.sh` (MODIFY — add trigger #11 section)
- **Approach**: per architecture-correction-plan §SP-4 spec; zero-external-consumer grep with cfg-feature exclusion.
- **Test scenarios**:
  - Self-test: synthetic zero-consumer `pub mod` flagged
  - Self-test: same `pub mod` behind `cfg(feature = "unstable-test")` passes
- **Verification**: trigger green on current codebase.

#### U44. Trigger #12 — SP-6 lock placement in public API

- **Goal**: `RwLock`/`Mutex`/`Arc<RwLock<...>>` in `pub fn` return type or `pub` field of trait/struct.
- **Requirements**: R13
- **Dependencies**: U43
- **Files**:
  - `scripts/port-check.sh` (MODIFY — add trigger #12 section)
- **Approach**: per architecture-correction-plan §SP-6 spec; grep `pub fn` + trait-method signatures for lock types in return position.
- **Test scenarios**:
  - Self-test: synthetic `pub fn handle(&self) -> RwLockReadGuard<...>` flagged
  - Self-test: private `fn` with same return passes
- **Verification**: trigger green on current codebase.

#### U45. Trigger #13 — SP-8 constructor-time panics

- **Goal**: `unwrap`/`expect`/`panic!`/`unimplemented!`/`assert!` reachable from `pub fn` on its arguments in a library crate; allow `debug_assert!`.
- **Requirements**: R13
- **Dependencies**: U44
- **Files**:
  - `scripts/port-check.sh` (MODIFY — add trigger #13 section)
- **Approach**: per architecture-correction-plan §SP-8 spec; grep `pub fn` bodies (shallow transitively) for the five forms; exclude `#[test]`/`#[cfg(test)]`/`debug_assert!`.
- **Test scenarios**:
  - Self-test: synthetic `pub fn new(x: u8) -> Self { assert!(x > 0); ... }` flagged
  - Self-test: `pub fn new(x: u8) -> Self { debug_assert!(x > 0); ... }` passes
  - Self-test: same `assert!` inside `#[cfg(test)]` passes
- **Verification**: trigger green on current codebase.

#### U46. Document all 5 triggers in `docs/PORT.md`

- **Goal**: add `### 8. SP-1 stubbed-but-called` through `### 13. SP-8 constructor-time panics` headings to `docs/PORT.md` Refusal triggers section with rationale + examples + allowlist rules.
- **Requirements**: R13
- **Dependencies**: U45
- **Files**:
  - `docs/PORT.md` (MODIFY — extend Refusal triggers section)
- **Approach**: mirror existing trigger #1-#7 + #9 documentation style; cite architecture-correction-plan §SP-1..SP-8 as source.
- **Test scenarios**: none (documentation).
- **Verification**: `grep -n "### 8\\.\|### 10\\.\|### 11\\.\|### 12\\.\|### 13\\." docs/PORT.md` returns 5 matches.

#### U47. Update final port-check.sh message

- **Goal**: change final success message at port-check.sh:473 from "all seven refusal triggers + FR-033 grep + trigger 9" to "all 13 refusal triggers clean".
- **Requirements**: R13
- **Dependencies**: U46
- **Files**:
  - `scripts/port-check.sh` (MODIFY — line 473)
- **Approach**: one-line edit.
- **Test scenarios**: none.
- **Verification**: `bash scripts/port-check.sh -v` final line reports "all 13 refusal triggers clean".

---

### PR-B-followup — Layer/Semantics Waves 1 + 2 + 5 + 6 + 7

Goal: complete remaining waves of the layer/semantics repair plan (U1-U7 + U14-U24 from repair plan). Parallel to D-block, does not gate critical path.

#### U48. Wave 1 — zero-consumer deletions

- **Goal**: execute Wave 1 U1 of layer/semantics repair plan — delete `needs_compositing` cache, speculative setters, GC hooks, SceneCompositor stubs.
- **Requirements**: R12b
- **Dependencies**: PR-B-gate merged (U11-U13)
- **Files**: per repair plan U1.
- **Approach**: defer to repair plan U1.
- **Test scenarios**: per repair plan U1.
- **Verification**: per repair plan U1.

#### U49. Wave 2 — Layer enum boxing + Alignment newtype + FollowerLayer

- **Goal**: execute Wave 2 U2-U7 of layer/semantics repair plan.
- **Requirements**: R12b
- **Dependencies**: U48
- **Files**: per repair plan U2-U7.
- **Approach**: defer to repair plan U2-U7.
- **Test scenarios**: per repair plan U2-U7.
- **Verification**: per repair plan U2-U7.

#### U50. Wave 5 — semantics platform routing + Flutter-faithful absorb

- **Goal**: execute Wave 5 U14-U16 of layer/semantics repair plan.
- **Requirements**: R12b
- **Dependencies**: U49
- **Files**: per repair plan U14-U16.
- **Approach**: defer to repair plan U14-U16.
- **Test scenarios**: per repair plan U14-U16.
- **Verification**: per repair plan U14-U16.

#### U51. Wave 6 — type-system cleanups

- **Goal**: execute Wave 6 U17-U20 of layer/semantics repair plan.
- **Requirements**: R12b
- **Dependencies**: U50
- **Files**: per repair plan U17-U20.
- **Approach**: defer to repair plan U17-U20.
- **Test scenarios**: per repair plan U17-U20.
- **Verification**: per repair plan U17-U20.

#### U52. Wave 7 — sync + alloc + hygiene + audit closure annotation

- **Goal**: execute Wave 7 U21-U24 of layer/semantics repair plan.
- **Requirements**: R12b
- **Dependencies**: U51
- **Files**: per repair plan U21-U24.
- **Approach**: defer to repair plan U21-U24.
- **Test scenarios**: per repair plan U21-U24.
- **Verification**: per repair plan U21-U24 final exit gates.

---

## Key Technical Decisions

(Reproduced from companion decision memo for plan-reader convenience; memo is authoritative.)

- **KTD-1 (D1) Borrow architecture via `get_parent_and_children_mut`.** Rejected interior mutability (PORT.md triggers #1+#2 forbid), callback closures (cannot capture `&mut PipelineOwner` as `Fn`), worklist refactor (breaks parent-uses-child-size contract). Picked existing disjoint-borrow primitive at `crates/flui-rendering/src/storage/tree.rs:215-301`.
- **KTD-2 (D2) `OnceCell` → `Option` for `set_constraints` + `set_geometry`.** P0 prep, mandatory before R1 walk rewrite. Flutter parity (`.flutter/.../object.dart:2865` straight assignment). Without this, frame-2 panic on every production binding.
- **KTD-3 (D3) `mark_needs_layout` propagation is greenfield + 2-caller migration + bootstrap.** Not "wire boundary discipline." Cycle 4 R-5 deleted the propagation trait; current production callers use bare flag set.
- **KTD-4 (D4) Protocol-erasure at pipeline → entry seam via `RenderNode::layout_erased` + `ErasedConstraints/ErasedGeometry`.**
- **KTD-5 (D5) `RenderObject<P>::perform_layout_raw` signature change to `&mut dyn LayoutCtxErased`; blanket impl bridges to `RenderBox::perform_layout`.**
- **KTD-6 (D6) Layout cycle detection via `currently_laying_out: FxHashSet<RenderId>` + RAII drop-guard + `RenderError::LayoutCycle(id)`.** Replaces `LAYOUT_DEPTH_LIMIT` (wrong place after walk rewrite).
- **KTD-7 (D7) Dirty-queue dedup symmetric across all 4 queues: flag-gated no-op + mid_layout_marks side queue.**
- **KTD-8 (D8) Paint flag-clear retention = purge-warn.** Preserves R-15 invariant from PR #109.
- **KTD-9 (D9) Strict per-PR file fences; PR-C splits into PR-C-1/C-2/C-3.** Per fence table in companion memo.
- **KTD-10 (D10) Trigger #8 install LAST per close-first precedent (PR #134/FR-036).**
- **KTD-11 (D11) PR-B splits into PR-B-gate (Wave 3+4) + PR-B-followup (Wave 1+2+5+6+7).** Only Wave 3+4 gates D-3.
- **KTD-12 (D12) Test infrastructure at `crates/flui-rendering/tests/pipeline/` + `tests/common/mod.rs` fixture builders.**

---

## Phased Delivery

```
Strict serial merge order — file fences enforce non-overlap:

  ┌─────────┐
  │ PR-C-1  │  flui-log → flui-foundation::log  (U1-U4)
  └────┬────┘
       ▼
  ┌─────────┐
  │ PR-C-2  │  flui-geometry split + Constitution  (U5-U10)
  └────┬────┘
       ▼
  ┌─────────┐
  │ PR-B-   │  layer/semantics Wave 3+4 lifecycle  (U11-U13)
  │ gate    │  (gates PR-A2)
  └────┬────┘
       ▼
  ┌─────────┐
  │ PR-A1   │  D-1 layout pipeline wiring  (U14-U31, ~17 commits)
  └────┬────┘
       ▼
  ┌─────────┐
  │ PR-A2   │  D-3+D-4 compositing+paint pipeline  (U32-U40)
  └────┬────┘
       ▼
  ┌─────────┐
  │ PR-C-3  │  refusal triggers #8/#10/#11/#12/#13  (U41-U47)
  └─────────┘  (close-first-install-after)

  PR-B-followup (U48-U52)  — parallel any time post PR-B-gate;
                             does not gate D-block critical path
```

---

## Risk Analysis & Mitigation

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| R1 | `RenderObject<P>::perform_layout_raw` signature change (KTD-5) ripples to manual impls beyond the blanket — wider trait-surface churn than anticipated | Medium | High | Pre-PR-A1: grep all manual `impl RenderObject<P> for T` in workspace; enumerate + migrate explicitly in U19 |
| R2 | Disjoint-borrow `get_parent_and_children_mut` doesn't support all the access patterns multi-child render objects need (e.g., child-of-child layout) | Low | High | Memo D1 picks single-level disjoint; deeper recursion uses repeated calls. If primitive insufficient, fallback to per-protocol typed context with closure dispatch (Shape B in memo D5) |
| R3 | PR-B-gate (Wave 3+4) reveals additional layer/semantics defects mid-implementation forcing scope expansion | Medium | Medium | PR-B-gate is bounded by repair plan's Wave 3+4 specifications; new findings go to PR-B-followup or separate cycle |
| R4 | `_updateSubtreeCompositingBits` Flutter algorithm has subtle invariants the port misses; AE4 passes but real-world compositing breaks | Medium | High | Read `.flutter/.../object.dart:3226-3258` + adjacent helpers thoroughly during U34 implementation; add edge case tests beyond AE4 |
| R5 | `flui-app::draw_frame` test scope expansion (R18 update) reveals existing flui-app test fragility unrelated to D-block | Low | Medium | Scope D-block exit only to "no NEW failures introduced"; pre-existing flui-app HEAP_CORRUPTION (per Cycle 5 receipts) stays out of scope |
| R6 | PR sequencing serial chain (7 PRs) means each blocker stalls all downstream; PR-A1 18-commit scope hits review bandwidth limit | Medium | Medium | PR-B-followup can land parallel; PR-A1 can split into PR-A1a (prep U14-U17) + PR-A1b (walk rewrite U18-U31) if cumulative diff exceeds review tolerance |
| R7 | Constitution amendment (U8) requires `.specify/memory/constitution.md` v2.4.0 spec.md mention or other downstream doc updates I missed | Low | Low | U9 catches CLAUDE.md; do final grep for "v2.3.0" across docs/ before PR-C-2 finalize |

---

## Dependencies / Assumptions

- **Companion decision memo** at [`docs/research/2026-05-23-d-block-architecture-decision-memo.md`](../research/2026-05-23-d-block-architecture-decision-memo.md) carries D1–D12 binding architecture decisions. Deviations require explicit Key Decision entry update.
- **Origin brainstorm rev 2** at [`docs/brainstorms/pipeline-wiring-d-block-requirements.md`](../brainstorms/pipeline-wiring-d-block-requirements.md) is the requirements source of truth.
- **Layer/semantics repair plan** at [`docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](2026-05-22-004-feat-layer-semantics-repair-plan.md) is the substrate for U11-U13 (PR-B-gate) and U48-U52 (PR-B-followup).
- **Architecture-correction-plan** at [`docs/research/2026-05-22-architecture-correction-plan.md`](../research/2026-05-22-architecture-correction-plan.md) §D-1/D-3/D-4 + §SP-1..SP-8 is the research substrate for D-block + refusal-trigger work.
- **Flutter source** at `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` is the Flutter parity reference for: `markNeedsLayout` (lines 2658-2700), `layout` invariants (2738-2766), `_constraints = constraints` straight assignment (2845), `_updateSubtreeCompositingBits` (3226-3258).
- **Engineering standards** per CLAUDE.md §Engineering Standards & Subagent Dispatch — Senior Rust engineer profile, Flutter port discipline, atomic-commit-per-unit shape, orchestrator catches sloppy work pre-review.

---

## Documentation Plan

- **`docs/PORT.md`**: extend Refusal triggers section with 5 new triggers (#8/#10/#11/#12/#13) in U46 of PR-C-3.
- **`docs/ROADMAP.md`**: post-D-block merge, update Core.0 scoreboard line "rendering machine → machine spec-complete" + delete "NEW — construction unowned by any prior plan" bullet for D-1/D-3/D-4. Annotation commit in a separate doc-update PR after PR-A2 + PR-C-3 land.
- **`docs/FOUNDATIONS.md`**: no changes — D-block implements existing contracts; no new contracts locked.
- **`.specify/memory/constitution.md`**: v2.3.0 → v2.4.0 bump + layer table amendment in U8 of PR-C-2.
- **`CLAUDE.md`**: version reference update in U9 of PR-C-2.
- **Brainstorm rev 2**: per-PR closure annotations in U4/U10/U13/U31 of each PR.
- **Memo update**: post-merge, annotate companion memo with each PR's merge commit SHA.

---

## Outstanding Questions

### Resolve Before Implementation Starts

(none — all P0 forks resolved in companion decision memo; rev 2 brainstorm incorporates resolutions.)

### Deferred to Implementation

- **[Affects U19][Technical]** Exact shape of `LayoutCtxErased` trait — generic-over-protocol enum vs separate type per protocol vs trait-object-friendly wrapper. Implementer picks the option that ripples least through existing `RenderObject<P>` impls during U19.
- **[Affects U34][Needs research]** Flutter's `_updateSubtreeCompositingBits` subtle ordering rules — implementer reads Flutter source thoroughly during U34; if surprising invariants surface, add tests beyond AE4.
- **[Affects U18][Technical]** Final naming + location of `ErasedConstraints` / `ErasedGeometry` enums — `flui-rendering::storage` vs new `flui-rendering::protocol::erased` module. Implementer picks during U18.
- **[Affects U23][Technical]** Root constraints source for `layout_dirty_root` — read from `RenderState::constraints()` cached value (set by `RootRenderElement::mount`) vs add explicit `pipeline.root_constraints: Option<BoxConstraints>` field. Investigate during U23; prefer the option that doesn't require API change on `set_root_render_object`.
- **[Affects U16][Verification]** Confirm `flui-hot-reload::pipeline.rs:156` does or does not have a layout-side mark to migrate. Verify during U16.
- **[Affects U41][Technical]** `// STUB-OK: <reason> (issue #N)` marker grammar — exact regex for the allowlist. Tracking issue requirement enforced via PR review, not script (script only checks marker presence).
