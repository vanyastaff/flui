# FLUI Rendering Migration - Quick Reference

## Current Status at a Glance

**37 / 129 objects migrated (29% complete)**

```
Interaction: ✅✅✅✅ (100%)
Special:    ✅✅✅✅✅✅⏳⏳ (75%)
Effects:    ✅✅✅✅✅✅✅✅✅✅✅✅✅⏳⏳⏳⏳⏳⏳ (68%)
Layout:     ✅✅✅✅✅✅✅✅✅✅⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳ (29%)
Sliver:     ⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳⏳ (0%)
Text:       ⏳⏳ (0%) - DISABLED
Media:      ⏳⏳⏳ (0%) - DISABLED
Viewport:   ⏳⏳⏳ (0%) - DISABLED
Debug:      ⏳⏳⏳⏳ (0%) - DISABLED
```

## Critical Path (What Blocks Widgets)

### Phase 1A - Next 3-5 days (Re-enable core)
```
PRIORITY | OBJECT           | TIME | IMPACT
---------|------------------|------|--------
P0       | RenderParagraph  | 30m  | Text rendering ⚠️ CRITICAL
P0       | RenderImage      | 30m  | Image display ⚠️ CRITICAL  
P0       | RenderFlex       | 3h   | Row/Column ⚠️ CRITICAL
P0       | RenderStack      | 3h   | Positioned ⚠️ CRITICAL
P0       | RenderViewport   | 4h   | Scrolling ⚠️ CRITICAL
P0       | RenderSliverList | 4h   | List widgets ⚠️ CRITICAL
```
**Est. Total: 15 hours for 2 developers = 1 week**

### Phase 1B - Following week (Layout foundation)
```
PRIORITY | OBJECT                 | TIME | IMPACT
---------|------------------------|------|--------
P1       | RenderSizedBox         | 1h   | Spacer widget
P1       | RenderAlign            | 2h   | Center/Align
P1       | RenderConstrainedBox   | 1h   | Constraints
P1       | RenderDecoratedBox     | 3h   | Styling
P1       | RenderGrid             | 3h   | Grid layout
P1       | RenderSliverMultiBox   | 5h   | List/Grid base
```
**Est. Total: 15 hours = 1 week**

### Phase 1C - Week 3 (Sliver completion)
```
Core slivers: SliverFixedExtentList, SliverGrid, SliverAppBar, etc.
Est. 20 hours = 1 week
```

## Trait Mapping Reference

### RenderBox<A> - Standard 2D Layout
```rust
// Example: RenderPadding (Single child)
impl RenderBox<Single> for RenderPadding {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size { ... }
    fn paint(&self, ctx: &mut PaintContext<'_, Single>) { ... }
}

// Children access patterns:
// - Single: ctx.children.single() → NonZeroUsize
// - Optional: ctx.children.get() → Option<NonZeroUsize>
// - Variable: ctx.children.iter() → Iterator
// - Leaf: ctx.children is empty
```

### SliverRender<A> - Scrollable Layout
```rust
// Example: RenderSliverList (Variable children)
impl SliverRender<Variable> for RenderSliverList {
    fn layout(&mut self, ctx: LayoutContext<'_, Variable, SliverProtocol>) -> SliverGeometry { ... }
    fn paint(&self, ctx: &mut PaintContext<'_, Variable>) { ... }
}
```

### RenderBoxProxy - Pass-through (No logic needed)
```rust
// Just declare - gets RenderBox<Single> automatically!
impl RenderBoxProxy for RenderMouseRegion {
    // Can override proxy_layout, proxy_paint, proxy_hit_test if needed
}
```

## Arity Selection Guide

| Pattern | Arity | Examples |
|---------|-------|----------|
| **No children** | `Leaf` | Text, Image, Spacer, Placeholder |
| **0 or 1 child** | `Optional` | Container, SizedBox, Align, Padding |
| **Exactly 1 child** | `Single` | Opacity, Transform, ClipRect, Positioned |
| **Exactly 2 children** | `Exact<2>` | (rare) - Row with exactly 2 items |
| **Many children** | `Variable` | Flex, Stack, Column, CustomLayout |

## Objects by Implementation Effort

### EASY (30 min - 1 hour each)
```
RenderEmpty                (Leaf)
RenderParagraph            (Leaf, but text rendering details)
RenderImage                (Leaf, but image sizing logic)
RenderColoredBox           (Leaf)
RenderFittedBox            (Single)
RenderMetaData             (Single, proxy)
RenderLimitedBox           (Optional)
RenderOverflowBox          (Optional)
RenderConstrainedOverflowBox (Optional)
```

### MODERATE (1-3 hours each)
```
RenderSizedBox             (Optional, branching logic)
RenderAlign                (Optional, size factor math)
RenderConstrainedBox       (Optional)
RenderFlexItem             (Single, metadata)
RenderPositioned           (Single, metadata)
RenderCustomSingleChildLayoutBox (Single)
RenderFractionalTranslation (Single)
RenderIntrinsicHeight      (Single)
RenderIntrinsicWidth       (Single)
RenderScrollView           (Single)
RenderDecoratedBox         (Optional, complex styling)
RenderGrid                 (Variable)
RenderWrap                 (Variable)
RenderAnimatedSize         (Variable)
RenderFlow                 (Variable)
RenderPhysicalModel        (Optional)
RenderPhysicalShape        (Optional)
RenderSliverAppBar         (Single, sliver)
RenderSliverOpacity        (Single, sliver)
RenderSliverPadding        (Single, sliver)
RenderSliverToBoxAdapter   (Single, sliver)
```

### COMPLEX (3-5 hours each)
```
RenderFlex                 (Variable, alignment logic)
RenderStack                (Variable, positioning)
RenderCustomMultiChildLayoutBox (Variable)
RenderListBody             (Variable, non-sliver)
RenderIndexedStack         (Variable)
RenderListWheelViewport    (Variable)
RenderTable                (Variable)
RenderAbstractViewport     (Variable, sliver)
RenderViewport             (Variable, sliver)
RenderSliverList           (Variable, sliver, lazy loading)
RenderSliverMultiBoxAdapter (Variable, sliver, base class)
RenderSliverFixedExtentList (Variable, sliver)
RenderSliverGrid           (Variable, sliver)
```

## Module Re-enable Order

1. **TEXT** - RenderParagraph (2 objects)
2. **MEDIA** - RenderImage, RenderTexture (2-3 objects)
3. **VIEWPORT** - RenderAbstractViewport, etc. (3 objects)
4. **SLIVER** - All sliver objects (24 objects) - in multiple phases
5. **DEBUG** - Last, if at all (4 objects, low priority)

## Testing Checklist Per Object

After migrating each object:

- [ ] Compiles without trait errors
- [ ] `layout()` correctly uses `ctx.constraints` and `ctx.children`
- [ ] `paint()` correctly uses `ctx.children` and `ctx.offset`
- [ ] `hit_test()` implemented (or uses default)
- [ ] Child count validation works (arity enforced at compile time)
- [ ] Cache fields cleared on layout if present
- [ ] Works with corresponding widget wrapper
- [ ] Example compiles
- [ ] Existing tests pass

## File Locations Quick Map

```
crates/flui_rendering/src/objects/
├── layout/
│   ├── padding.rs          ✅ RenderPadding
│   ├── flex.rs             ❌ RenderFlex (HIGH PRIORITY)
│   ├── stack.rs            ❌ RenderStack (HIGH PRIORITY)
│   ├── align.rs            ❌ RenderAlign
│   ├── sized_box.rs        ❌ RenderSizedBox
│   └── ... (24 total)
├── effects/
│   ├── opacity.rs          ✅ RenderOpacity
│   ├── transform.rs        ✅ RenderTransform
│   ├── decorated_box.rs    ❌ RenderDecoratedBox
│   └── ... (19 total)
├── interaction/
│   ├── absorb_pointer.rs   ✅ RenderAbsorbPointer
│   └── ... (4 total, all done)
├── special/
│   ├── render_view.rs      ✅ RenderView
│   ├── colored_box.rs      ✅ RenderColoredBox
│   ├── fitted_box.rs       ❌ RenderFittedBox
│   └── ... (8 total)
├── text/
│   └── paragraph.rs        ❌ RenderParagraph (DISABLED)
├── media/
│   ├── image.rs            ❌ RenderImage (DISABLED)
│   └── texture.rs          ❌ RenderTexture (DISABLED)
├── viewport/
│   ├── abstract_viewport.rs    ❌ (DISABLED)
│   └── ... (3 total)
├── sliver/
│   ├── sliver_list.rs      ❌ RenderSliverList (DISABLED)
│   ├── sliver_grid.rs      ❌ RenderSliverGrid (DISABLED)
│   └── ... (24 total, all disabled)
└── debug/
    └── ... (4 objects, disabled)
```

## Key Files to Understand

### Core Traits (read-only, reference)
```
crates/flui_core/src/render/render_box.rs        → RenderBox<A> trait
crates/flui_core/src/render/render_silver.rs     → SliverRender<A> trait
crates/flui_core/src/render/render_proxy.rs      → RenderBoxProxy trait
crates/flui_core/src/render/protocol.rs          → Protocol system
crates/flui_core/src/render/arity.rs             → Arity types
```

### Contexts (read-only, usage reference)
```
crates/flui_core/src/render/contexts.rs
├── LayoutContext<'_, A, P>      → Input constraints, layout children
├── PaintContext<'_, A>          → Paint children, access offset
└── HitTestContext<'_, A, P>     → Hit test children
```

## Migration Pattern Template

```rust
// BEFORE (old trait, commented out)
// impl LegacyRender for RenderMyObject { ... }

// AFTER (new trait)
use flui_core::render::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::Size;

#[derive(Debug)]
pub struct RenderMyObject {
    // ... fields ...
    // Cache for paint if needed
    cached_size: Size,
}

impl RenderBox<Single> for RenderMyObject {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        
        // Layout work here
        let child_size = ctx.layout_child(child_id, ctx.constraints);
        
        // Cache for paint
        self.cached_size = child_size;
        
        // Return size
        child_size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();
        // Use self.cached_size here if needed
        ctx.paint_child(child_id, ctx.offset);
    }
    
    // hit_test has default impl, override if needed
}
```

## Next Steps

1. **Today**: Pick Phase 1A objects (Paragraph, Image, Flex, Stack, Viewport, SliverList)
2. **This week**: Complete Phase 1A + Phase 1B (SizedBox, Align, etc.)
3. **Next week**: Phase 1C (core Slivers) + Phase 2 (advanced layouts)
4. **Week 3-4**: Remaining objects + testing

See `FLUI_RENDERING_MIGRATION_AUDIT.md` for full details.
