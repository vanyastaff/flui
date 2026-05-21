---
date: 2026-05-20
topic: clipcontext-consolidation
audit_source: docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md
origin: deferred from docs/brainstorms/flui-rendering-zombie-cleanup-requirements.md (Phase 1 Step 1 audit item)
flutter_reference: .flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart
---

# ClipContext consolidation across flui-rendering and flui-painting

## Summary

Collapse the two divergent `ClipContext` traits (one in `flui-rendering`, one in `flui-painting`) into a single Flutter-faithful trait that lives in `flui-painting`, migrate `CanvasContext` onto it, add the missing 4th method `clip_rsuperellipse_and_paint` for Flutter parity, and turn the existing `flui-painting/ARCHITECTURE.md` claim that `CanvasContext` implements `flui-painting::ClipContext` from a doc-lie into truth.

---

## Problem Frame

PR #81 (the flui-rendering Phase 1 zombie cleanup) carried `ClipContext` deletion in an early draft, then deferred it after round-1 `ce-doc-review` uncovered three problems the audit had missed:

1. **Two divergent traits exist, not one.** `flui-rendering::context::clip::ClipContext` ([crates/flui-rendering/src/context/clip.rs](../../crates/flui-rendering/src/context/clip.rs)) and `flui-painting::ClipContext` ([crates/flui-painting/src/clip_context.rs](../../crates/flui-painting/src/clip_context.rs)) have incompatible signatures — `canvas()` vs `canvas_mut()` accessor, `FnOnce(&mut Canvas)` vs `FnOnce(&mut Self)` painter shape, `Clip::None` save/restore semantics differ. Either can be reached from a future caller; neither matches the other.

2. **A production-side doc-lie.** [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md) lines 37 and 94 already document `flui-painting::ClipContext` as the canonical seam with `CanvasContext` as its single production implementer. Today that is false: `CanvasContext` at [crates/flui-rendering/src/context/canvas.rs:695](../../crates/flui-rendering/src/context/canvas.rs) implements `flui-rendering::ClipContext`, not the painting one. ARCHITECTURE.md describes the intended end state, not the current one.

3. **A Flutter parity gap.** Flutter's canonical `painting/clip.dart` `ClipContext` ([.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart)) exposes 4 user methods — `clipPathAndPaint`, `clipRRectAndPaint`, `clipRSuperellipseAndPaint`, `clipRectAndPaint`. Both FLUI traits expose only 3. The rounded-superellipse clip method is missing from both, despite [crates/flui-types/src/geometry/rsuperellipse.rs](../../crates/flui-types/src/geometry/rsuperellipse.rs) and [crates/flui-layer/src/layer/clip_superellipse.rs](../../crates/flui-layer/src/layer/clip_superellipse.rs) already existing in the workspace.

A workspace-wide grep confirms zero production callers of the `clip_*_and_paint` methods on either trait — only test impls call them today. So the cost of moving the cleanup forward now (before a real caller materializes) is bounded; the cost of leaving the divergence is that the first widget needing clip-and-paint hits ambiguity and a doc-lie at the same time.

---

## Requirements

**Line-number policy:** brainstorm cites paths and symbol names; line numbers are illustrative only. Implementer uses `grep` / `rg` on the symbol name at edit time and ignores any cited line position.

**Trait shape (canonical)**

- **R1**: `flui-painting::ClipContext` is the single canonical trait. `flui-rendering::context::clip::ClipContext` and its containing file [crates/flui-rendering/src/context/clip.rs](../../crates/flui-rendering/src/context/clip.rs) are deleted; the module is removed from `crates/flui-rendering/src/context/mod.rs`.

- **R2**: The trait's single required method is renamed from `canvas_mut(&mut self) -> &mut Canvas` to `canvas(&mut self) -> &mut Canvas`, matching Flutter's `Canvas get canvas` getter ([.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart:13](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart)). The Rust `_mut` suffix convention is consciously dropped here in favor of Flutter naming parity — see Key Decisions.

- **R3**: The trait's painter callback signature stays `F: FnOnce(&mut Self)` (the current `flui-painting` shape). This is the closest Rust translation of Flutter's `VoidCallback` closure-captures-receiver semantics and is what allows nested clip operations.

- **R4**: The `Clip::None` short-circuit currently in `flui-painting::ClipContext::clip_and_paint_impl` ([crates/flui-painting/src/clip_context.rs:250-253](../../crates/flui-painting/src/clip_context.rs)) is removed. The trait must always `save()` and `restore()` around the painter regardless of `clip_behavior`, matching Flutter ([.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart:21-37](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart)). `Clip::None` becomes a no-op only on the clip-call step, not on save/restore.

- **R5**: A new method `clip_rsuperellipse_and_paint(rse: RSuperellipse, clip_behavior: Clip, bounds: Rect<Pixels>, painter: F)` is added to the trait, completing the 4-method Flutter parity. The implementation calls `Canvas::clip_rsuperellipse_ext` (the existing painting-layer entry point); if that method does not yet exist on `Canvas`, it lands in the same batch on the cost-prudence argument that exposing the trait method without the underlying Canvas op would create a compile-time-broken trait surface.

- **R6**: All four user methods take `Rect<Pixels>` for the `bounds` argument (and for `rect` in `clip_rect_and_paint`). This is FLUI's typed-unit improvement over Flutter's bare `Rect` and is preserved unchanged — no Flutter parity loss because Dart has no typed-unit equivalent.

**Implementer migration**

- **R7**: `impl super::ClipContext for CanvasContext` at [crates/flui-rendering/src/context/canvas.rs:695](../../crates/flui-rendering/src/context/canvas.rs) is replaced with `impl flui_painting::ClipContext for CanvasContext`. The body changes from `fn canvas(&mut self) -> &mut Canvas { self.canvas() }` (unchanged accessor name after R2) to whatever delegation matches the renamed trait method.

- **R8**: Test impls in [crates/flui-rendering/src/context/clip.rs](../../crates/flui-rendering/src/context/clip.rs) (`TestClipContext`) are deleted as part of R1's file deletion. The corresponding test coverage already exists in [crates/flui-painting/src/clip_context.rs](../../crates/flui-painting/src/clip_context.rs)'s `TestClipContext`; extend the painting-crate tests to cover `clip_rsuperellipse_and_paint` (R5) — no need to recreate the rendering-side test fixture.

**Documentation alignment**

- **R9**: [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md) lines 37 and 94 become true statements without further edits to the prose itself, because R7 makes `CanvasContext` actually implement `flui-painting::ClipContext`. Verify by re-reading both lines post-edit; only update the prose if the count of production impls or the trait shape changes vs what the doc currently claims.

- **R10**: Inline module-level docs on `flui-painting::ClipContext` ([crates/flui-painting/src/clip_context.rs:11-18](../../crates/flui-painting/src/clip_context.rs)) currently reference a `PaintingContext` implementer that does not exist in the workspace. Replace the diagram reference with `CanvasContext` (the actual production implementer), or simplify to a generic "any type with a Canvas".

**Verification gates**

- **R11**: `cargo build --workspace` passes after each commit in the implementation batch.
- **R12**: `cargo test --workspace --lib --tests` passes; the four `clip_*_and_paint` test cases in `flui-painting::ClipContext` exercise the migrated shape, plus a new test covering R5's `clip_rsuperellipse_and_paint`.
- **R13**: `cargo clippy --workspace --all-targets -- -D warnings` passes after the final commit (justfile-aligned form).
- **R14**: `bash scripts/port-check.sh -v` reports 7/7 institutional refusal triggers ok after each commit.
- **R15**: Post-cleanup workspace-wide grep returns zero hits for `flui_rendering::context::clip::ClipContext`, `flui_rendering::ClipContext`, or any `use flui_rendering::.*ClipContext` import patterns outside the deletion diff. The `flui_painting::ClipContext` re-export at [crates/flui-painting/src/lib.rs](../../crates/flui-painting/src/lib.rs) stays.

---

## Acceptance Examples

The trait shape decisions (R2-R5) are not behaviorally-conditional in the "When X, Y" sense — they are structural signature changes. Acceptance Examples are not required for them. The verification gates (R11-R15) act as concrete acceptance criteria.

One behavioral case worth pinning explicitly:

- **AE1** (Covers R4): When a caller invokes `ctx.clip_rect_and_paint(rect, Clip::None, bounds, painter)`, the canvas `save_count()` observed inside `painter` is exactly one greater than the `save_count()` observed immediately before the call, and one less than `save_count()` observed immediately after the call returns. This locks down the "no short-circuit, save/restore always run" behavior of R4 against the prior `flui-painting::ClipContext` short-circuit code path that the migration deletes.

---

## Success Criteria

- One `ClipContext` trait remains in the workspace (in `flui-painting`), with four user methods matching Flutter's `painting/clip.dart` parity.
- `CanvasContext` is the trait's single production implementer; this matches what [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md) already claims.
- The four `clip_*_and_paint` methods exist with the canonical Flutter signature shape adapted to Rust idioms: `canvas(&mut self) -> &mut Canvas` accessor, `Rect<Pixels>` typed-unit Rect, `FnOnce(&mut Self)` painter callback, no `Clip::None` short-circuit.
- Workspace-wide grep audit confirms zero `flui_rendering::ClipContext` references survive outside deletion diff.
- All four verification gates (build, test, clippy, port-check) pass cleanly on the final commit.
- Mythos audit ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)) Step 1 annotation gets an additional status note recording this consolidation as the deferred ClipContext item, with commit-hash references — same pattern as U5 of the Phase 1 cleanup.

---

## Scope Boundaries

**In scope:**

- The four user methods on the consolidated trait, the helper `clip_and_paint_impl`, the `canvas` accessor rename, the `Clip::None` short-circuit removal, the `clip_rsuperellipse_and_paint` addition, the `Canvas::clip_rsuperellipse_ext` underlying op if it does not yet exist, the migration of `CanvasContext`'s impl, the deletion of `flui-rendering::context::clip`, the inline module-doc fix in `clip_context.rs`, and the verification that ARCHITECTURE.md lines 37 and 94 stay accurate post-migration.

**Out of scope:**

- **SUPERELLIPSE_CACHE bounding** ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md) Step 5 item 14). Separate brainstorm.
- **SceneBuilder missing methods** (audit Priority #2). Separate brainstorm.
- **PictureLayer hint fields** (audit Priority #3). Separate brainstorm.
- **RendererBinding redesign** (audit Priority #5). Separate brainstorm.
- **Delegate trait visibility narrowing** (CustomPainter, FlowDelegate, MultiChildLayoutDelegate, SingleChildLayoutDelegate). Separate brainstorm.
- **Lyon tessellation feature-flag move** (audit Step 16). Separate brainstorm.
- **`pipeline.rs` / `pipelines.rs` consolidation** (audit Step 10). Separate brainstorm.
- **`Arc<Mutex<OffscreenRenderer>>` ownership review** (audit Step 12). Separate brainstorm.
- **RenderObject roadmap** (audit Priority #6 — 88% Flutter parity gap). Separate brainstorm.
- **Production integration test for dirty-marking path** (audit Step 4 item 13). Separate brainstorm.
- **Feature-gating `clip_rsuperellipse_and_paint` behind a `superellipse` cargo feature**. Considered and rejected — Flutter exposes the method unconditionally, and `RSuperellipse` already exists workspace-wide. Reintroduce gating only if a downstream consumer measurably suffers from the trait-method byte cost; today there's no measurement, so YAGNI.
- **Adding `PaintingContext` as a distinct type** beyond what `CanvasContext` already provides. The audit doc and current `flui-painting::ClipContext` rustdoc mention `PaintingContext` as the intended implementer, but `CanvasContext` is the actual production type. If a future PaintingContext shape is needed, it's a separate design decision; this brainstorm assumes `CanvasContext` remains the canonical impl.

---

## Key Decisions

- **Canonical trait lives in `flui-painting`, not `flui-rendering`.** Flutter's source organization places `ClipContext` in `painting/clip.dart`, not `rendering/`. The trait deals with painting primitives (Canvas, Clip, Paint, Path) — it logically belongs in the painting layer regardless of which crate's types are richer today. Matches the documented intent in [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md).

- **Accessor name `canvas`, not `canvas_mut`.** Rust convention prefixes mutable accessors with `_mut` (e.g., `field()` vs `field_mut()`), but Flutter's trait uses `canvas` without modifier because Dart has no immutable/mutable distinction. Per the user's "how Flutter does it" directive, Flutter naming wins. The clippy `wrong_self_convention` lint does NOT fire on `&mut self -> &mut T` named without `_mut` (only on `&self -> &mut T` or similar mismatches), so this rename is lint-clean. Reviewers familiar with Rust convention may flag the omission; document the Flutter-parity rationale in the rustdoc.

- **Painter callback `FnOnce(&mut Self)`, not `FnOnce(&mut Canvas)`.** Flutter's `VoidCallback painter` parameter is a closure that, in Dart, captures the enclosing `this` and operates on `this.canvas` (and potentially other state). The closest Rust shape that allows the same capture-the-receiver semantics is `FnOnce(&mut Self)`. The alternative `FnOnce(&mut Canvas)` only exposes the canvas and forces callers to capture other context externally — Flutter doesn't do that. Both shapes are object-unsafe (generic method params), so no `dyn ClipContext` cost difference.

- **`Clip::None` does NOT short-circuit.** Flutter's `_clipAndPaint` ([.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart:21-37](../../.flutter/flutter-master/packages/flutter/lib/src/painting/clip.dart)) saves the canvas, runs the painter, then restores — even when `clipBehavior == Clip.none`. The `Clip::None` branch only skips the clip-call step. flui-painting's current short-circuit ([crates/flui-painting/src/clip_context.rs:250-253](../../crates/flui-painting/src/clip_context.rs)) is a FLUI-invented optimization. Per "how Flutter does it", drop the optimization; the cost (one save + one restore per no-op clip) is marginal and matches the Flutter contract.

- **`Rect<Pixels>` typed-unit stays.** Flutter has bare `Rect`. FLUI uses `Rect<Pixels>` everywhere for unit safety. This is an FLUI improvement that does not violate Flutter parity — Dart has no typed-unit equivalent, so there's no Flutter shape to deviate from. Keep `Rect<Pixels>` on every `rect`/`bounds` parameter.

- **4th method `clip_rsuperellipse_and_paint` lands in this batch, not deferred.** The audit originally flagged the method as missing. With `RSuperellipse` already in `flui-types` and a `ClipSuperellipseLayer` already in `flui-layer`, adding the trait method costs a single function body. Deferring would force a second migration pass at the call site of every implementer; landing now keeps the consolidation atomic.

- **Migration shape: atomic-commit-per-finding.** Same precedent as PR #81 (U1-U5). Likely commits: (a) add `Canvas::clip_rsuperellipse_ext` if missing, (b) flui-painting trait shape edits (rename accessor, drop short-circuit, add 4th method), (c) `CanvasContext` impl migration + flui-rendering trait deletion, (d) ARCHITECTURE.md + module-doc cleanup, (e) audit-doc Step 1 annotation update.

---

## Dependencies / Assumptions

- **No production callers of `clip_*_and_paint`.** Verified by workspace-wide grep ([Phase 1.1 context scan in this brainstorm]). The two trait-definition files are the only places these method names appear. This makes the migration scope bounded — there is no call-site sweep, only the implementer migration and the test impl deletion.

- **`Canvas::clip_rsuperellipse_ext` may or may not exist.** Need to verify at planning time. `ClipSuperellipseLayer` exists in `flui-layer` and `RSuperellipse` exists in `flui-types`, but the `Canvas` painting-API entry point for clipping by an RSuperellipse is not confirmed. If absent, it lands in the same batch (R5).

- **Workspace clippy baseline stays clean.** PR #81 commit `9a4b3f9e` brought the Rust 1.95 baseline to zero clippy errors. This brainstorm assumes that baseline holds at implementation time. If new clippy errors surface during the consolidation (e.g., from added rustdoc or test code), the "no defer-with-excuse" memory directive applies — fix in-commit.

- **The deferred ClipContext consolidation is the natural successor to PR #81 U5.** U5 ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md) status block) explicitly named this work as the follow-up. Landing it closes that loop; the audit doc gets an updated annotation post-merge.

- **No new branch is needed before brainstorming/planning.** Per ce-work convention, code-touching work creates the branch at plan-execution time. The brainstorm and plan docs land on a doc-only branch first; the consolidation implementation lands on its own branch.

---

## Outstanding Questions

### Deferred to Planning

- **Q1 [Affects R5][Needs verification]:** Does `Canvas::clip_rsuperellipse_ext` already exist on the painting-layer `Canvas`? If yes, R5 is a single-line addition to the trait. If no, the underlying op lands first in its own commit, and the trait method follows. The planner verifies at edit time.

- **Q2 [Affects R10][Technical]:** The inline `flui-painting::ClipContext` rustdoc mentions a `PaintingContext` implementer and `TestRecordingPaintingContext` ([crates/flui-painting/src/clip_context.rs:15-19](../../crates/flui-painting/src/clip_context.rs)). Neither exists in the current workspace — `CanvasContext` is the actual production type. Planner decides between (a) rewriting the doc to reference `CanvasContext` explicitly, or (b) keeping the prose generic ("any type with a Canvas") to leave room for a future `PaintingContext`. Default: option (a) — name the real implementer.

- **Q3 [Affects R7][Technical]:** Should `CanvasContext`'s `canvas()` accessor (the existing method on the struct, separate from the trait method introduced by R2) stay named `canvas`, or rename to `canvas_inner` / similar to avoid trait-method shadowing? The trait method `canvas(&mut self) -> &mut Canvas` may shadow or conflict with the existing inherent method. Planner verifies at edit time and resolves with the minimum-rename option.
