# FLUI RenderObject Arity Classification

## Complete mapping of all 82 Flutter RenderObjects to FLUI Arity types

---

## 🎉 MIGRATION PROGRESS

**Status as of 2025-01-19:**

| Phase | Arity | Objects | Status | Progress |
|-------|-------|---------|--------|----------|
| Phase 1 | Leaf | 9 | 🔄 In Progress | 0/9 (0%) |
| Phase 2 | Optional | 6 | ⏳ Not Started | 0/6 (0%) |
| **Phase 3-5** | **Single** | **34** | **✅ COMPLETE** | **34/34 (100%)** |
| Phase 6 | Variable | 13 | ⏳ Not Started | 0/13 (0%) |
| Phase 7+ | Sliver | 20 | ⏳ Not Started | 0/20 (0%) |
| **TOTAL** | **All** | **82** | **🔄 41% Complete** | **34/82** |

**Completed Phases:**
- ✅ **Phase 3:** Single - Layout (10 objects)
- ✅ **Phase 4:** Single - Effects (14 objects)
- ✅ **Phase 5:** Single - Interaction & Semantics (10 objects)

**Key Achievements:**
- ✅ Implemented unmounted_children system for child mounting
- ✅ Created `box_single_with_child()` constructor
- ✅ Enabled IntoElement for Single arity tuples
- ✅ All 34 Single objects compile with 0 errors

---

**Decision Criteria:**
- **Leaf** — No children, renders content directly
- **Optional** — 0-1 child, meaningful behavior without child (spacer, decoration)
- **Single** — Exactly 1 child required, wrapper/effect that's meaningless without child
- **Variable** — Any number of children (0..N)

---

## Summary

| Arity | Box Protocol | Sliver Protocol | Total | Percentage |
|-------|-------------|-----------------|-------|------------|
| **Leaf** | 9 | 0 | **9** | 11% |
| **Optional** | 6 | 0 | **6** | 7% |
| **Single** | 28 | 10 | **38** | 46% |
| **Variable** | 13 | 16 | **29** | 35% |
| **Total** | **56** | **26** | **82** | 100% |

---

## Leaf (0 children) — 9 objects

**Trait:** `Render<Leaf>`

No children accessor. Compile error if child accessed.

| # | RenderObject | Category | Description | Path |
|---|--------------|----------|-------------|------|
| 1 | RenderParagraph | Text | Multi-line text rendering | `src/objects/text/paragraph.rs` |
| 2 | RenderEditableLine | Text | Editable text line | `src/objects/layout/editable_line.rs` |
| 3 | RenderImage | Media | Raster image | `src/objects/media/image.rs` |
| 4 | RenderTexture | Media | GPU texture | `src/objects/media/texture.rs` |
| 5 | RenderErrorBox | Debug | Red error box | `src/objects/debug/error_box.rs` |
| 6 | RenderPlaceholder | Debug | Placeholder rectangle | `src/objects/debug/placeholder.rs` |
| 7 | RenderPerformanceOverlay | Debug | Performance metrics | `src/objects/debug/performance_overlay.rs` |
| 8 | RenderColoredBox | Visual | Solid color rectangle | `src/objects/special/colored_box.rs` |
| 9 | RenderEmpty | Special | Empty render object | `src/objects/layout/empty.rs` |

**Example - RenderBox<Leaf>:**
```rust
use flui_rendering::{RenderBox, Leaf, LayoutContext, BoxProtocol, PaintContext};
use flui_types::{Size, Color};

/// Simple colored rectangle with no children
#[derive(Debug)]
pub struct RenderColoredBox {
    pub color: Color,
}

impl RenderBox<Leaf> for RenderColoredBox {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        // No children - just return size based on constraints
        // ctx.children is NoChildren - compile error if you try to access!
        ctx.constraints.biggest()
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: PaintTree,
    {
        // Paint self - no children to paint
        let rect = Rect::from_ltrb(
            ctx.offset.x,
            ctx.offset.y,
            ctx.offset.x + self.size.width,
            ctx.offset.y + self.size.height,
        );

        ctx.canvas().draw_rect(rect, Paint::new().set_color(self.color));
    }
}
```

---

## Optional (0-1 children) — 6 objects

**Trait:** `Render<Optional>`

Child is optional. Meaningful visual/layout output without child.

| # | RenderObject | Category | Without Child Use Case | Path |
|---|--------------|----------|------------------------|------|
| 1 | RenderSizedBox | Layout | Spacer with specified size | `src/objects/layout/sized_box.rs` |
| 2 | RenderConstrainedBox | Layout | Enforce min/max constraints | `src/objects/layout/constrained_box.rs` |
| 3 | RenderLimitedBox | Layout | Limit size for unbounded parents | `src/objects/layout/limited_box.rs` |
| 4 | RenderDecoratedBox | Visual | Border/shadow/gradient decoration | `src/objects/effects/decorated_box.rs` |
| 5 | RenderPhysicalModel | Visual | Elevation/shadow effect | `src/objects/effects/physical_model.rs` |
| 6 | RenderPhysicalShape | Visual | Custom shape with shadow | `src/objects/effects/physical_shape.rs` |

**Example:**
```rust
impl RenderBox<Optional> for RenderSizedBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Optional, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        let child = ctx.children.get();
        match child {
            // Without child: return specified size (spacer)
            None => ctx.constraints.constrain(Size::new(self.width, self.height)),
            // With child: layout child with constraints
            Some(child_id) => {
                let inner = ctx.constraints.deflate(&self.padding);
                ctx.layout_child(child_id, inner)
            }
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Optional>)
    where
        T: PaintTree,
    {
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, ctx.offset);
        }
        // Empty canvas handled automatically if no child
    }
}
```

---

## Single (exactly 1 child) — 38 objects

**Trait:** `Render<Single>` or `SliverRender<Single>`

Child is required. No meaningful behavior without child.

### Box Protocol Single — 28 objects

#### Layout Single (10)

| # | RenderObject | Description | Path |
|---|--------------|-------------|------|
| 1 | RenderPadding | Adds padding around child | `src/objects/layout/padding.rs` |
| 2 | RenderPositionedBox | Align/Center within parent | `src/objects/layout/positioned_box.rs` |
| 3 | RenderAspectRatio | Fixed aspect ratio | `src/objects/layout/aspect_ratio.rs` |
| 4 | RenderFractionallySizedBox | Size as fraction of parent | `src/objects/layout/fractionally_sized_box.rs` |
| 5 | RenderIntrinsicWidth | Width = intrinsic width | `src/objects/layout/intrinsic_width.rs` |
| 6 | RenderIntrinsicHeight | Height = intrinsic height | `src/objects/layout/intrinsic_height.rs` |
| 7 | RenderBaseline | Baseline alignment | `src/objects/layout/baseline.rs` |
| 8 | RenderShiftedBox | Base class for shifts | `src/objects/layout/shifted_box.rs` |
| 9 | RenderRotatedBox | Rotate 90°/180°/270° | `src/objects/layout/rotated_box.rs` |
| 10 | RenderSizedOverflowBox | Size ≠ child size | `src/objects/layout/sized_overflow_box.rs` |

#### Visual Effects Single (14)

| # | RenderObject | Description | Path |
|---|--------------|-------------|------|
| 11 | RenderOpacity | Transparency (0.0-1.0) | `src/objects/effects/opacity.rs` |
| 12 | RenderAnimatedOpacity | Animated transparency | `src/objects/effects/animated_opacity.rs` |
| 13 | RenderTransform | Matrix transformations | `src/objects/effects/transform.rs` |
| 14 | RenderClipRect | Clip with rectangle | `src/objects/effects/clip_rect.rs` |
| 15 | RenderClipRRect | Clip with rounded rectangle | `src/objects/effects/clip_rrect.rs` |
| 16 | RenderClipOval | Clip with oval | `src/objects/effects/clip_oval.rs` |
| 17 | RenderClipPath | Clip with arbitrary path | `src/objects/effects/clip_path.rs` |
| 18 | RenderBackdropFilter | Blur background behind widget | `src/objects/effects/backdrop_filter.rs` |
| 19 | RenderShaderMask | Shader mask | `src/objects/effects/shader_mask.rs` |
| 20 | RenderRepaintBoundary | Separate paint layer | `src/objects/effects/repaint_boundary.rs` |
| 21 | RenderOffstage | Hides child (doesn't paint) | `src/objects/effects/offstage.rs` |
| 22 | RenderVisibility | Shows/hides child | `src/objects/effects/visibility.rs` |
| 23 | RenderFittedBox | Scales child by BoxFit | `src/objects/special/fitted_box.rs` |
| 24 | RenderCustomPaint | Custom drawing | `src/objects/effects/custom_paint.rs` |

#### Interaction Single (4)

| # | RenderObject | Description | Path |
|---|--------------|-------------|------|
| 25 | RenderPointerListener | Pointer events | `src/objects/interaction/pointer_listener.rs` |
| 26 | RenderIgnorePointer | Passes through hit tests | `src/objects/interaction/ignore_pointer.rs` |
| 27 | RenderAbsorbPointer | Blocks hit tests | `src/objects/interaction/absorb_pointer.rs` |
| 28 | RenderMouseRegion | Mouse enter/exit/hover | `src/objects/interaction/mouse_region.rs` |

#### Semantics/Special Single (6)

| # | RenderObject | Description | Path |
|---|--------------|-------------|------|
| 29 | RenderMetaData | Metadata for parent | `src/objects/special/metadata.rs` |
| 30 | RenderAnnotatedRegion | Metadata for system UI | `src/objects/special/annotated_region.rs` |
| 31 | RenderBlockSemantics | Blocks semantics | `src/objects/special/block_semantics.rs` |
| 32 | RenderExcludeSemantics | Excludes semantics | `src/objects/special/exclude_semantics.rs` |
| 33 | RenderMergeSemantics | Merges semantics | `src/objects/special/merge_semantics.rs` |
| 34 | RenderView | Root view | `src/objects/special/render_view.rs` |

### Sliver Protocol Single — 10 objects

| # | RenderSliver | Description | Path |
|---|--------------|-------------|------|
| 1 | RenderSliverToBoxAdapter | Box → Sliver adapter | `src/objects/sliver/to_box_adapter.rs` |
| 2 | RenderSliverPadding | Padding for sliver | `src/objects/sliver/padding.rs` |
| 3 | RenderSliverOpacity | Sliver opacity | `src/objects/sliver/opacity.rs` |
| 4 | RenderSliverAnimatedOpacity | Animated sliver opacity | `src/objects/sliver/animated_opacity.rs` |
| 5 | RenderSliverIgnorePointer | Ignore pointer for sliver | `src/objects/sliver/ignore_pointer.rs` |
| 6 | RenderSliverOffstage | Hides sliver | `src/objects/sliver/offstage.rs` |
| 7 | RenderSliverFillRemaining | Fills remaining space | `src/objects/sliver/fill_remaining.rs` |
| 8 | RenderSliverEdgeInsetsPadding | Edge insets padding | `src/objects/sliver/edge_insets_padding.rs` |
| 9 | RenderSliverConstrainedCrossAxis | Cross-axis constraints | `src/objects/sliver/constrained_cross_axis.rs` |
| 10 | RenderSliverOverlapAbsorber | Absorbs overlap | `src/objects/sliver/overlap_absorber.rs` |

**Example - SliverRender<Single>:**
```rust
use flui_rendering::{SliverRender, Single, LayoutContext, SliverProtocol, PaintContext};
use flui_types::{SliverGeometry, EdgeInsets};

/// RenderSliver that adds padding around a child sliver
pub struct RenderSliverPadding {
    pub padding: EdgeInsets,
}

impl SliverRender<Single> for RenderSliverPadding {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let child_id = ctx.children.single();

        // Adjust constraints for padding
        let adjusted_constraints = SliverConstraints {
            scroll_offset: (ctx.constraints.scroll_offset - self.padding.top).max(0.0),
            overlap: ctx.constraints.overlap,
            remaining_paint_extent: ctx.constraints.remaining_paint_extent - self.padding.vertical_total(),
            cross_axis_extent: ctx.constraints.cross_axis_extent - self.padding.horizontal_total(),
            ..ctx.constraints
        };

        // Layout child with adjusted constraints
        let child_geometry = ctx.layout_child(child_id, adjusted_constraints);

        // Return geometry with padding applied
        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent + self.padding.vertical_total(),
            paint_extent: (child_geometry.paint_extent + self.padding.vertical_total())
                .min(ctx.constraints.remaining_paint_extent),
            layout_extent: child_geometry.layout_extent + self.padding.vertical_total(),
            ..child_geometry
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        let child_id = ctx.children.single();
        // Paint child with padding offset
        let child_offset = ctx.offset + Offset::new(self.padding.left, self.padding.top);
        ctx.paint_child(child_id, child_offset);
    }
}
```

### Proxy Pattern - RenderBoxProxy Trait

For render objects that simply pass constraints/painting to their child without modification, use the **`RenderBoxProxy` trait** for automatic implementation:

**Using RenderBoxProxy (Recommended):**
```rust
use flui_rendering::RenderBoxProxy;

/// Example: Wrapper that only tracks metadata - zero boilerplate!
#[derive(Debug)]
pub struct RenderMetadataWrapper {
    pub label: String,
}

// Just implement the marker trait - get full RenderBox<Single> for free!
impl RenderBoxProxy for RenderMetadataWrapper {}

// That's it! Now has:
// - layout: passes constraints to child via ctx.proxy()
// - paint: paints child at same offset via ctx.proxy()
// - hit_test: default implementation (no hit)
```

**Customizing Proxy Behavior:**
```rust
use flui_rendering::{RenderBoxProxy, LayoutContext, BoxProtocol, LayoutTree};
use flui_types::Size;

#[derive(Debug)]
pub struct RenderLogging {
    pub name: String,
}

impl RenderBoxProxy for RenderLogging {
    // Override layout to add logging
    fn proxy_layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        tracing::debug!("Layout: {}", self.name);
        ctx.proxy() // Still delegates to child
    }

    // paint and hit_test use default implementations
}
```

**Manual Proxy (if you need full control):**
```rust
#[derive(Debug)]
pub struct RenderManualProxy;

impl RenderBox<Single> for RenderManualProxy {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        ctx.proxy() // Manual use of proxy helper
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        ctx.proxy(); // Manual use of proxy helper
    }
}
```

**RenderSliverProxy (for slivers):**
```rust
use flui_rendering::RenderSliverProxy;

#[derive(Debug)]
pub struct RenderSliverMetadata {
    pub scroll_id: usize,
}

// Just implement the marker trait - get full SliverRender<Single> for free!
impl RenderSliverProxy for RenderSliverMetadata {}

// That's it! Now has:
// - layout: passes sliver constraints to child via ctx.proxy()
// - paint: paints child at same offset via ctx.proxy()
// - hit_test: default implementation (no hit)
```

**When to use proxy pattern:**
- Semantic wrappers (accessibility, debugging)
- Event listeners that don't affect layout
- Metadata annotations for scrollable content
- Testing/placeholder objects

**✨ Both RenderBoxProxy and RenderSliverProxy now use context-based API!**

---

## Variable (N children) — 29 objects

**Trait:** `Render<Variable>` or `SliverRender<Variable>`

Any number of children (0..N).

### Box Protocol Variable — 13 objects

| # | RenderObject | Category | Description | Path |
|---|--------------|----------|-------------|------|
| 1 | RenderFlex | Layout | Row/Column (linear flex) | `src/objects/layout/flex.rs` |
| 2 | RenderStack | Layout | Positioned layers | `src/objects/layout/stack.rs` |
| 3 | RenderIndexedStack | Layout | Shows child by index | `src/objects/layout/indexed_stack.rs` |
| 4 | RenderWrap | Layout | Wrap with line breaks | `src/objects/layout/wrap.rs` |
| 5 | RenderFlow | Layout | Custom layout delegate | `src/objects/layout/flow.rs` |
| 6 | RenderTable | Layout | Table layout | `src/objects/layout/table.rs` |
| 7 | RenderGrid | Layout | Grid layout (CSS Grid) | `src/objects/layout/grid.rs` |
| 8 | RenderListBody | Layout | Simple scrollable list | `src/objects/layout/list_body.rs` |
| 9 | RenderListWheelViewport | Layout | 3D wheel picker | `src/objects/layout/list_wheel_viewport.rs` |
| 10 | RenderCustomMultiChildLayoutBox | Layout | Custom multi-child | `src/objects/layout/custom_multi_child_layout_box.rs` |
| 11 | RenderOverflowIndicator | Debug | Overflow indicator | `src/objects/debug/overflow_indicator.rs` |
| 12 | RenderViewport | Sliver | Viewport for slivers | `src/objects/sliver/viewport.rs` |
| 13 | RenderShrinkWrappingViewport | Sliver | Shrink-wrap viewport | `src/objects/viewport/shrink_wrapping_viewport.rs` |

### Sliver Protocol Variable — 16 objects

| # | RenderSliver | Description | Path |
|---|--------------|-------------|------|
| 1 | RenderSliverList | Scrollable list | `src/objects/sliver/list.rs` |
| 2 | RenderSliverFixedExtentList | Fixed height items | `src/objects/sliver/fixed_extent_list.rs` |
| 3 | RenderSliverPrototypeExtentList | Prototype height items | `src/objects/sliver/prototype_extent_list.rs` |
| 4 | RenderSliverGrid | Scrollable grid | `src/objects/sliver/grid.rs` |
| 5 | RenderSliverFillViewport | Fills viewport | `src/objects/sliver/fill_viewport.rs` |
| 6 | RenderSliverAppBar | Floating/pinned app bar | `src/objects/sliver/app_bar.rs` |
| 7 | RenderSliverPersistentHeader | Sticky header | `src/objects/sliver/persistent_header.rs` |
| 8 | RenderSliverFloatingPersistentHeader | Floating header | `src/objects/sliver/floating_persistent_header.rs` |
| 9 | RenderSliverPinnedPersistentHeader | Pinned header | `src/objects/sliver/pinned_persistent_header.rs` |
| 10 | RenderSliverMainAxisGroup | Main-axis grouping | `src/objects/sliver/main_axis_group.rs` |
| 11 | RenderSliverCrossAxisGroup | Cross-axis grouping | `src/objects/sliver/cross_axis_group.rs` |
| 12 | RenderSliverMultiBoxAdaptor | Base for lists | `src/objects/sliver/multi_box_adaptor.rs` |
| 13 | RenderSliverSafeArea | Safe area sliver | `src/objects/sliver/safe_area.rs` |
| 14 | RenderSliver | Base trait | `src/objects/sliver/mod.rs` |
| 15 | RenderAbstractViewport | Abstract viewport | `src/objects/viewport/abstract_viewport.rs` |

**Example - RenderBox<Variable>:**
```rust
use flui_rendering::{RenderBox, Variable, LayoutContext, BoxProtocol, PaintContext};
use flui_types::{Size, Axis};

impl RenderBox<Variable> for RenderFlex {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        let mut main_used = 0.0;
        let mut cross_max = 0.0;

        for child_id in ctx.children.iter() {
            let child_size = ctx.layout_child(child_id, ctx.constraints);
            main_used += child_size.main(self.direction);
            cross_max = cross_max.max(child_size.cross(self.direction));
        }

        Size::from_main_cross(self.direction, main_used, cross_max)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        let mut offset = ctx.offset;
        for child_id in ctx.children.iter() {
            ctx.paint_child(child_id, offset);
            // Advance offset for next child
            offset = offset + self.spacing;
        }
    }
}
```

**Example - SliverRender<Variable>:**
```rust
use flui_rendering::{SliverRender, Variable, LayoutContext, SliverProtocol, PaintContext};
use flui_types::{SliverGeometry, SliverConstraints};

/// Scrollable list of sliver children
pub struct RenderSliverList {
    pub item_extent: f32, // Fixed height per item
}

impl SliverRender<Variable> for RenderSliverList {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let mut scroll_extent = 0.0;
        let mut paint_extent = 0.0;
        let mut max_paint_extent = 0.0;

        // Layout each child
        for child_id in ctx.children.iter() {
            // Create constraints for this child
            let child_constraints = SliverConstraints {
                scroll_offset: (ctx.constraints.scroll_offset - scroll_extent).max(0.0),
                overlap: 0.0,
                remaining_paint_extent: ctx.constraints.remaining_paint_extent - paint_extent,
                cross_axis_extent: ctx.constraints.cross_axis_extent,
                ..ctx.constraints
            };

            let child_geometry = ctx.layout_child(child_id, child_constraints);

            scroll_extent += child_geometry.scroll_extent;
            paint_extent += child_geometry.paint_extent;
            max_paint_extent += child_geometry.max_paint_extent;
        }

        SliverGeometry {
            scroll_extent,
            paint_extent: paint_extent.min(ctx.constraints.remaining_paint_extent),
            max_paint_extent,
            layout_extent: paint_extent.min(ctx.constraints.remaining_paint_extent),
            ..Default::default()
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        let mut current_offset = ctx.offset;

        for child_id in ctx.children.iter() {
            ctx.paint_child(child_id, current_offset);
            // Advance offset for next item
            current_offset = current_offset + Offset::new(0.0, self.item_extent);
        }
    }
}
```

---

## Migration Checklist

### Phase 1: Leaf (Week 1)
- [ ] RenderEmpty
- [ ] RenderImage
- [ ] RenderTexture
- [ ] RenderParagraph
- [ ] RenderEditableLine
- [ ] RenderPlaceholder
- [ ] RenderErrorBox
- [ ] RenderColoredBox
- [ ] RenderPerformanceOverlay

### Phase 2: Optional (Week 1-2)
- [ ] RenderSizedBox
- [ ] RenderConstrainedBox
- [ ] RenderLimitedBox
- [ ] RenderDecoratedBox
- [ ] RenderPhysicalModel
- [ ] RenderPhysicalShape

### Phase 3: Single - Layout (Week 2) ✅ COMPLETE
- [x] RenderPadding
- [x] RenderPositionedBox
- [x] RenderAspectRatio
- [x] RenderFractionallySizedBox
- [x] RenderIntrinsicWidth
- [x] RenderIntrinsicHeight
- [x] RenderBaseline
- [x] RenderShiftedBox
- [x] RenderRotatedBox
- [x] RenderSizedOverflowBox

### Phase 4: Single - Effects (Week 2-3) ✅ COMPLETE
- [x] RenderOpacity
- [x] RenderAnimatedOpacity
- [x] RenderTransform
- [x] RenderClipRect
- [x] RenderClipRRect
- [x] RenderClipOval
- [x] RenderClipPath
- [x] RenderBackdropFilter
- [x] RenderShaderMask
- [x] RenderRepaintBoundary
- [x] RenderOffstage
- [x] RenderVisibility
- [x] RenderFittedBox
- [x] RenderCustomPaint

### Phase 5: Single - Interaction & Semantics (Week 3) ✅ COMPLETE
- [x] RenderPointerListener
- [x] RenderIgnorePointer
- [x] RenderAbsorbPointer
- [x] RenderMouseRegion
- [x] RenderMetaData
- [x] RenderAnnotatedRegion
- [x] RenderBlockSemantics
- [x] RenderExcludeSemantics
- [x] RenderMergeSemantics
- [x] RenderView

### Phase 6: Variable - Box (Week 3-4)
- [ ] RenderFlex
- [ ] RenderStack
- [ ] RenderIndexedStack
- [ ] RenderWrap
- [ ] RenderFlow
- [ ] RenderTable
- [ ] RenderGrid
- [ ] RenderListBody
- [ ] RenderListWheelViewport
- [ ] RenderCustomMultiChildLayoutBox
- [ ] RenderOverflowIndicator
- [ ] RenderViewport
- [ ] RenderShrinkWrappingViewport

### Phase 7: Sliver Single (Week 4)
- [ ] RenderSliverToBoxAdapter
- [ ] RenderSliverPadding
- [ ] RenderSliverOpacity
- [ ] RenderSliverAnimatedOpacity
- [ ] RenderSliverIgnorePointer
- [ ] RenderSliverOffstage
- [ ] RenderSliverFillRemaining
- [ ] RenderSliverEdgeInsetsPadding
- [ ] RenderSliverConstrainedCrossAxis
- [ ] RenderSliverOverlapAbsorber

### Phase 8: Sliver Variable (Week 4-5)
- [ ] RenderSliverList
- [ ] RenderSliverFixedExtentList
- [ ] RenderSliverPrototypeExtentList
- [ ] RenderSliverGrid
- [ ] RenderSliverFillViewport
- [ ] RenderSliverAppBar
- [ ] RenderSliverPersistentHeader
- [ ] RenderSliverFloatingPersistentHeader
- [ ] RenderSliverPinnedPersistentHeader
- [ ] RenderSliverMainAxisGroup
- [ ] RenderSliverCrossAxisGroup
- [ ] RenderSliverMultiBoxAdaptor
- [ ] RenderSliverSafeArea
- [ ] RenderSliver
- [ ] RenderAbstractViewport

---

## API Reference

### Trait Implementations

```rust
// Box Protocol
impl RenderBox<Leaf> for MyRender { ... }      // 0 children
impl RenderBox<Optional> for MyRender { ... }  // 0-1 children
impl RenderBox<Single> for MyRender { ... }    // 1 child
impl RenderBox<Variable> for MyRender { ... }  // N children

// Sliver Protocol
impl SliverRender<Single> for MyRender { ... }    // 1 child
impl SliverRender<Variable> for MyRender { ... }  // N children

// Proxy Traits (auto-implement RenderBox/SliverRender)
impl RenderBoxProxy for MyWrapper { ... }     // Box with Single child
impl RenderSliverProxy for MyWrapper { ... }  // Sliver with Single child
```

### Context API

```rust
// Layout
fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, A, P>) -> Size/SliverGeometry
where T: LayoutTree

// Paint
fn paint<T>(&self, ctx: &mut PaintContext<'_, T, A>)
where T: PaintTree

// Hit Test
fn hit_test<T>(&self, ctx: &HitTestContext<'_, T, A, P>, result: &mut HitTestResult) -> bool
where T: HitTestTree
```

### Children Accessors

| Arity | Accessor Type | Key Methods |
|-------|---------------|-------------|
| `Leaf` | `NoChildren` | None |
| `Optional` | `OptionalChild` | `get()`, `map()`, `is_some()`, `is_none()` |
| `Single` | `FixedChildren<1>` | `single()` |
| `Variable` | `SliceChildren` | `iter()`, `get(i)`, `first()`, `last()`, `len()` |

---

## Key Differences from Flutter

| Aspect | Flutter | FLUI |
|--------|---------|------|
| Single-child | Always `Widget?` (nullable) | `Single` = required, `Optional` = nullable |
| Validation | Runtime only | Compile-time + runtime |
| Without child | Always allowed | Only for Optional (6 types) |
| Child count checks | Manual if-checks | Type-enforced accessors |

**FLUI is stricter than Flutter** — this catches bugs earlier and reduces boilerplate checks.

---

## Notes

### Why Optional is Limited

Only 6 render objects are `Optional` because only these have **meaningful behavior without child**:

- **SizedBox** → horizontal/vertical spacer
- **ConstrainedBox** → enforce constraints on empty space
- **LimitedBox** → limit unbounded space
- **DecoratedBox** → show border/shadow/gradient
- **PhysicalModel** → show elevation shadow
- **PhysicalShape** → show custom shape shadow

All others (Padding, Opacity, Transform, Align, etc.) are meaningless without a child to affect.

### Lifecycle with Single

Single arity works with transactional updates during reconciliation:

```rust
element.begin_children_update();
element.remove_child(old_child);  // Temporarily 0 - OK during transaction
element.push_child(new_child);    // Back to 1
element.commit_children_update(); // Validates final state
```

This handles the lifecycle issue Flutter solves with nullable children.

---

*Document prepared for FLUI Framework*  
*Last updated: November 2025*
