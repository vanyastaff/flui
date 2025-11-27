# Change: Implement Full Viewport and Sliver Protocol

## Why

The current `RenderViewport` is a placeholder that does not implement the `RenderBox` trait or the full sliver protocol. This prevents proper scrolling functionality in FLUI. The sliver constraints and geometry types exist in `flui_types`, but the viewport doesn't actually use them to perform real layout with sliver children.

## What Changes

### Phase 1: Fix Core Types
- **MODIFIED** `SliverConstraints`: Add missing `user_scroll_direction` and `preceding_scroll_extent` fields
- **MODIFIED** `SliverGeometry`: Rename `max_scroll_obsolescence` to `max_scroll_obstruction_extent`

### Phase 2: Implement RenderViewport
- **MODIFIED** `RenderViewport`: Implement full `RenderBox<Variable>` trait
- **ADDED** Bidirectional scrolling support with `center` sliver
- **ADDED** `RenderAbstractViewport` trait implementation
- **ADDED** Real sliver layout algorithm (not placeholder)

### Phase 3: Implement RenderShrinkWrappingViewport
- **ADDED** `RenderShrinkWrappingViewport`: Viewport that sizes to its content

### Phase 4: Integration
- **MODIFIED** Existing sliver objects to work with new viewport
- **ADDED** Integration tests for scroll scenarios

## Impact

- **Affected specs**: None (no specs exist yet)
- **Affected code**:
  - `crates/flui_types/src/sliver_constraints.rs`
  - `crates/flui_types/src/sliver_geometry.rs`
  - `crates/flui_rendering/src/objects/viewport/render_viewport.rs`
  - `crates/flui_rendering/src/objects/viewport/shrink_wrapping_viewport.rs`
  - `crates/flui_rendering/src/objects/viewport/mod.rs`
  - All sliver objects in `crates/flui_rendering/src/objects/sliver/`

## Dependencies

- Requires understanding of Flutter's RenderViewport implementation
- Must work with existing `ViewportOffset` (already implemented)
- Must integrate with existing sliver objects

## Risk Assessment

- **Medium risk**: Core scrolling infrastructure change
- **Mitigation**: Implement in phases, test each phase thoroughly
- **Rollback**: Can keep existing placeholder if issues arise
