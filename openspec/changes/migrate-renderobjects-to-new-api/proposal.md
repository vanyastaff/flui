# Change: Migrate RenderObjects to RenderBox/SliverRender API

## Why

FLUI has completed Phase 3-5 migrations (34 Single arity objects) but 28 objects remain using legacy APIs. The new RenderBox<Arity> and SliverRender<Arity> APIs provide:
- **Compile-time arity validation** - prevent accessing non-existent children
- **Type-safe contexts** - LayoutContext, PaintContext with protocol abstraction
- **Proxy patterns** - zero-boilerplate for pass-through objects
- **Flutter alignment** - matches Flutter's RenderProxyBox/RenderProxySliver patterns

Current status: 54/82 objects migrated (66%). Remaining work focused on Leaf, Variable Box, and all Sliver objects.

## What Changes

### Leaf Objects (2 remaining)
- Migrate RenderParagraph (⚠️ MISCLASSIFIED - actually Variable with inline children)
- Verify RenderEditableLine (already correct as RenderBox<Leaf>)

### Variable Box Objects (6 remaining)
- Migrate: RenderFlow, RenderTable, RenderListWheelViewport, RenderCustomMultiChildLayoutBox, RenderOverflowIndicator, RenderViewport
- Pattern: `RenderBox<Variable>` with multi-child layout algorithms

### Sliver Single Objects (10 total)
- **5 using RenderSliverProxy** (zero-boilerplate):
  - RenderSliverOpacity
  - RenderSliverAnimatedOpacity
  - RenderSliverIgnorePointer
  - RenderSliverOffstage
  - RenderSliverConstrainedCrossAxis
- **5 using manual SliverRender<Single>**:
  - RenderSliverToBoxAdapter (protocol bridge)
  - RenderSliverPadding (geometry transform)
  - RenderSliverFillRemaining
  - RenderSliverEdgeInsetsPadding
  - RenderSliverOverlapAbsorber

### Sliver Variable Objects (16 total)
- All require manual `SliverRender<Variable>` with custom layout logic
- Priority order: RenderSliver (base) → RenderSliverMultiBoxAdaptor → RenderSliverList/Grid
- Includes complex objects: RenderSliverAppBar, persistent headers, viewport containers

### Module Organization
- Uncomment modules in `src/objects/sliver/mod.rs` and `src/objects/mod.rs` after each phase
- Enable exports in groups to verify compilation incrementally

## Impact

### Affected Specs
- `specs/rendering` - RenderObject trait hierarchy and arity system

### Affected Code
- `crates/flui_rendering/src/objects/sliver/` - 26 sliver objects
- `crates/flui_rendering/src/objects/layout/` - 6 variable box objects
- `crates/flui_rendering/src/objects/text/paragraph.rs` - reclassify as Variable
- `crates/flui_rendering/src/objects/mod.rs` - module re-exports
- `crates/flui_rendering/docs/plan.md` - update migration checklist

### Breaking Changes
- **None** - Internal migration only, no public API changes
- All objects maintain same external interface

### Migration Complexity
- **Low**: 5 Sliver Proxy objects (one-line implementation)
- **Medium**: 11 manual Sliver Single + 3 simple Variable Box
- **High**: 13 complex Sliver Variable (list/grid/viewport logic)
- **Critical**: 3 foundation objects (RenderSliver base, RenderSliverMultiBoxAdaptor, RenderViewport)

## Dependencies

- Flutter documentation analysis for proxy pattern classification
- Existing Phase 3-5 migrations as reference patterns
- Context API (LayoutContext, PaintContext) already implemented

## Risks

1. **RenderParagraph complexity** - Contains inline children via ContainerRenderObjectMixin
   - Mitigation: Carefully analyze Flutter's RenderParagraph implementation
2. **Sliver protocol subtleties** - Geometry transformation logic is intricate
   - Mitigation: Port from Flutter source incrementally, validate with tests
3. **Viewport implementation** - Most complex object with scroll coordination
   - Mitigation: Implement foundation (RenderSliver, RenderSliverMultiBoxAdaptor) first

## Success Criteria

- ✅ All 82 objects migrated to new API
- ✅ All modules uncommented and compiling
- ✅ Zero compilation errors
- ✅ Migration documented in plan.md
- ✅ Each phase builds successfully before moving to next
