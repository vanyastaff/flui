# Design: Implement Full Viewport and Sliver Protocol

## Context

FLUI aims to provide Flutter-like scrolling capabilities. The sliver protocol is Flutter's solution for efficient, lazy-loading scrollable content. Currently FLUI has:
- `SliverConstraints` and `SliverGeometry` types (mostly complete)
- `ViewportOffset` for scroll position management (complete)
- `RenderAbstractViewport` trait (defined but not implemented)
- `RenderViewport` (placeholder - no `RenderBox` implementation)
- 23+ sliver objects (structure exists, needs verification)

## Goals

- Implement a fully functional `RenderViewport` that can layout and paint slivers
- Support bidirectional scrolling with a `center` sliver
- Provide `RenderShrinkWrappingViewport` for content-sized viewports
- Ensure all existing sliver objects work with the new viewport

## Non-Goals

- Nested scroll views (future work)
- 2D scrolling / TableView (separate capability)
- Animated scrolling (handled by scroll controller, not viewport)

## Decisions

### Decision 1: Use Flutter's Layout Algorithm

**What:** Implement the same two-pass layout algorithm as Flutter's RenderViewport.

**Why:** Flutter's algorithm is battle-tested, handles edge cases, and provides expected behavior for scroll offset corrections.

**Algorithm:**
1. Layout slivers in forward direction (after center) until remainingPaintExtent exhausted
2. Layout slivers in reverse direction (before center) 
3. Handle `scroll_offset_correction` by re-running layout
4. Compute min/max scroll extents and apply to ViewportOffset

### Decision 2: SliverPhysicalContainerParentData

**What:** Use physical offsets for sliver positioning (paint offset from viewport origin).

**Why:** Simpler than logical coordinates, matches Flutter's approach for non-2D viewports.

```rust
pub struct SliverPhysicalContainerParentData {
    /// Paint offset from viewport's top-left
    pub paint_offset: Offset,
}
```

### Decision 3: Keep Existing SliverConstraints Structure

**What:** Add missing fields to existing `SliverConstraints` rather than restructuring.

**Why:** Minimizes breaking changes, existing sliver objects already use current structure.

**Added fields:**
- `user_scroll_direction: ScrollDirection`
- `preceding_scroll_extent: f32`
- `overlap: f32` (promote from method to field)

### Decision 4: Implement as RenderBox<Variable>

**What:** RenderViewport implements `RenderBox<Variable>` (variable number of children).

**Why:** Viewport can contain any number of sliver children, consistent with existing render object patterns.

## Architecture

### Type Hierarchy

```
RenderBox<Variable>
    └── RenderViewport
            ├── center: Option<ElementId>
            ├── anchor: f32
            ├── offset: ViewportOffset
            ├── axis_direction: AxisDirection
            ├── cross_axis_direction: AxisDirection
            ├── cache_extent: f32
            └── clip_behavior: ClipBehavior

RenderBox<Variable>
    └── RenderShrinkWrappingViewport
            ├── offset: ViewportOffset
            ├── axis_direction: AxisDirection
            └── ...
```

### Layout Flow

```
RenderViewport::perform_layout()
    │
    ├── compute initial constraints from size
    │
    ├── layout_child_sequence(forward_children, constraints)
    │   └── for each sliver:
    │       ├── compute SliverConstraints
    │       ├── layout sliver → SliverGeometry
    │       ├── handle scroll_offset_correction
    │       └── update remaining_paint_extent
    │
    ├── layout_child_sequence(reverse_children, constraints)
    │   └── (same as forward, but reversed growth direction)
    │
    └── apply_content_dimensions(min, max) to ViewportOffset
```

### Coordinate System

```
Viewport (axis_direction = TopToBottom)
┌────────────────────────────────────┐
│  ↑ reverse slivers (before center) │  scroll_offset negative
├────────────────────────────────────┤
│  ═══ CENTER SLIVER ═══             │  scroll_offset = 0
├────────────────────────────────────┤
│  ↓ forward slivers (after center)  │  scroll_offset positive
└────────────────────────────────────┘
```

## Risks / Trade-offs

### Risk 1: Breaking Changes to SliverConstraints

**Risk:** Adding fields to `SliverConstraints` may break existing code.

**Mitigation:** 
- Add fields with sensible defaults
- Update all existing usages in single PR
- Document migration in CHANGELOG

### Risk 2: Performance Regression

**Risk:** Full sliver layout may be slower than placeholder.

**Mitigation:**
- Profile layout performance
- Cache sliver geometries when constraints unchanged
- Use early-exit when remaining paint extent exhausted

### Risk 3: Existing Sliver Objects Incompatibility

**Risk:** Existing sliver objects may not work with new viewport.

**Mitigation:**
- Test each sliver object during implementation
- Fix any incompatibilities in same PR
- Add integration tests

## Migration Plan

1. **Phase 1:** Update `SliverConstraints` and `SliverGeometry` types
   - Non-breaking: new fields have defaults
   - Existing code continues to compile

2. **Phase 2:** Implement `RenderViewport` with `RenderBox<Variable>`
   - Replaces placeholder
   - May require updates to sliver objects

3. **Phase 3:** Implement `RenderShrinkWrappingViewport`
   - New functionality
   - No migration needed

4. **Rollback:** If issues arise, revert to placeholder and investigate

## Open Questions

1. **Should we support `cacheExtentStyle`?** Flutter supports both pixel and logical cache extent. For v1, pixels-only is simpler.

2. **How to handle `maxScrollObstructionExtent`?** This is used for pinned headers. Need to verify persistent header slivers use this correctly.

3. **Should `RenderViewportBase` be a trait or abstract struct?** Flutter uses abstract class. In Rust, trait + default impls or struct with generic parameter. Decision: Use trait with default implementations for maximum flexibility.
