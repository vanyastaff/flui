# Change: Refactor Rendering Contexts to Flutter Model

## Why

Current `flui_rendering` uses three GAT-based contexts (`LayoutContext`, `PaintContext`, `HitTestContext`) which diverge from Flutter's proven architecture. Flutter uses:
- **Layout**: No context — constraints passed directly as parameters, stored in RenderObject
- **Paint**: `PaintingContext` — the only real context (owns canvas, manages layers)
- **HitTest**: `HitTestResult` — accumulator object, not a context

This over-engineering adds complexity without benefit and makes the codebase harder to understand for developers familiar with Flutter.

## What Changes

### Phase 1: Remove LayoutContext
- **BREAKING**: Remove `LayoutContext<A, P, T>` and related type aliases
- Modify `RenderBox<A>::layout()` → `perform_layout(&mut self, constraints: BoxConstraints) -> Size`
- Constraints stored in RenderObject (via RenderNode state), not passed via context
- Children accessed via `RenderTree` methods, not context accessor

### Phase 2: Simplify to PaintingContext  
- **BREAKING**: Rename `PaintContext` → `PaintingContext`
- Remove GAT complexity (arity parameter `A` not needed for paint)
- Keep: canvas ownership, `paint_child()`, layer push methods
- Add: `push_opacity()`, `push_clip_rect()`, `push_transform()` (Flutter API)

### Phase 3: Remove HitTestContext
- **BREAKING**: Remove `HitTestContext<A, P, T>` entirely
- Use `BoxHitTestResult`/`SliverHitTestResult` directly (already exists in `flui_interaction`)
- Modify `RenderBox<A>::hit_test()` → `hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool`

### Mixin Updates
- Update all mixins (`ProxyBox`, `ShiftedBox`, `ContainerBox`, `LeafBox`) to new signatures
- Remove context-based child access, use direct tree methods

## Impact

- **Affected specs**: `flui-rendering`
- **Affected code**:
  - `crates/flui_rendering/src/context.rs` — major rewrite
  - `crates/flui_rendering/src/box_render.rs` — trait signature changes
  - `crates/flui_rendering/src/sliver.rs` — trait signature changes
  - `crates/flui_rendering/src/mixins/*.rs` — all mixins updated
  - `crates/flui_rendering/src/tree.rs` — trait method signatures
  - `crates/flui_rendering/src/render_tree.rs` — implementation updates
  - `crates/flui_rendering/src/lib.rs` — re-exports cleanup

## Benefits

1. **Flutter Familiarity**: Developers can directly apply Flutter knowledge
2. **Simpler API**: 3 contexts → 1 context + 1 result type
3. **Less Generics**: Remove GAT complexity from paint/hit-test paths
4. **Clearer Responsibilities**: 
   - `perform_layout()` = compute size from constraints
   - `PaintingContext` = draw to canvas
   - `HitTestResult` = record hit path
