# FLUI Rendering Crate Migration Audit

**Date**: November 20, 2025  
**Status**: Complete - Ready for Implementation

## Executive Summary

The `flui_rendering` crate contains **129 RenderObjects** across 9 modules. Currently:
- **37 objects are migrated** (28%) to the new `RenderBox<A>` / `SliverRender<A>` trait system
- **92 objects need migration** (72%) - still using old trait signatures
- **4 modules are completely disabled** (basic, debug, media, sliver sections)
- **4 modules are partially enabled** (layout, effects, interaction, special)

The new architecture uses:
- **`RenderBox<A>`** - for standard 2D box layout (BoxConstraints → Size)
- **`SliverRender<A>`** - for scrollable sliver layout (SliverConstraints → SliverGeometry)
- **`RenderBoxProxy`** / **`RenderSliverProxy`** - for pass-through decorators
- **Arity system** - compile-time child count validation (Leaf, Optional, Single, Variable, etc.)

---

## Part 1: Core Render Trait Architecture

### Overview

The new unified render system in `flui_core` provides:

```
View (immutable) → Element (mutable, in Slab) → RenderElement → RenderObject
                                                     ↓
                                                 LayoutContext
                                                 PaintContext
                                                 HitTestContext
```

### Main Traits

#### 1. **RenderBox<A: Arity>**
- **Used for**: Standard 2D box layout protocol
- **Constraints**: `BoxConstraints` (min/max width/height)
- **Geometry**: `Size`
- **Methods**:
  - `layout(&mut self, ctx: LayoutContext<'_, A, BoxProtocol>) -> Size`
  - `paint(&self, ctx: &mut PaintContext<'_, A>)`
  - `hit_test(&self, ctx: HitTestContext<'_, A, BoxProtocol>, result: &mut BoxHitTestResult) -> bool`

#### 2. **SliverRender<A: Arity>**
- **Used for**: Scrollable sliver layout (viewports, lists, grids)
- **Constraints**: `SliverConstraints` (scroll offset, remaining extents)
- **Geometry**: `SliverGeometry` (scroll/paint/layout extents)
- **Methods**:
  - `layout(&mut self, ctx: LayoutContext<'_, A, SliverProtocol>) -> SliverGeometry`
  - `paint(&self, ctx: &mut PaintContext<'_, A>)`
  - `hit_test(&self, ctx: HitTestContext<'_, A, SliverProtocol>, result: &mut SliverHitTestResult) -> bool`

#### 3. **RenderBoxProxy** (pass-through decorator for Single-child boxes)
- **Used for**: Objects that don't modify layout/paint/hit-test
- **Trait methods** (optional to override):
  - `proxy_layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size`
  - `proxy_paint(&self, ctx: &mut PaintContext<'_, Single>)`
  - `proxy_hit_test(&self, ctx: HitTestContext<'_, Single, BoxProtocol>, result: &mut BoxHitTestResult) -> bool`
- **Automatically implements** `RenderBox<Single>`
- **Perfect for**: Semantics, metadata, debug wrappers, event listeners

#### 4. **RenderSliverProxy** (pass-through decorator for Single-child slivers)
- **Similar to RenderBoxProxy** but for sliver protocol

### Arity System

Compile-time child count validation using Rust's type system:

| Arity Type | Child Count | Use Case | Accessor Methods |
|-----------|------------|----------|------------------|
| `Leaf` | 0 | Text, Image, Spacer | None (empty) |
| `Optional` | 0-1 | Container, SizedBox | `.get()`, `.is_some()`, `.map()` |
| `Exact<1>` (alias `Single`) | exactly 1 | Padding, Opacity | `.single()` |
| `Exact<2>` | exactly 2 | - | `.pair()`, `.first()`, `.second()` |
| `Exact<N>` | exactly N | - | `.as_slice()`, indexing |
| `AtLeast<N>` | N+ | - | `.as_slice()`, `.iter()`, `.get()` |
| `Variable` | any | Flex, Stack | `.iter()`, `.len()`, `.get()` |

### Protocol System

Two layout protocols with unified trait interface:

```rust
pub trait Protocol {
    type Constraints: /* ... */;
    type Geometry: /* ... */;
    const ID: LayoutProtocol;
}

// BoxProtocol: BoxConstraints → Size
impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = Size;
}

// SliverProtocol: SliverConstraints → SliverGeometry
impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
}
```

---

## Part 2: Complete RenderObject Catalog

### Status Legend
- ✅ = Migrated to new trait system
- ❌ = TODO - needs migration
- 🔒 = Disabled module (commented out in mod.rs)

### LAYOUT MODULE (10 Migrated ✅ / 24 TODO ❌)

**Migrated (Single Arity) ✅:**

| Object | File | Arity | Trait | Status | Notes |
|--------|------|-------|-------|--------|-------|
| RenderAspectRatio | aspect_ratio.rs | Single | RenderBox<Single> | ✅ | Maintains aspect ratio of child |
| RenderBaseline | baseline.rs | Single | RenderBox<Single> | ✅ | Aligns child to baseline |
| RenderFractionallySizedBox | fractionally_sized_box.rs | Optional | RenderBox<Optional> | ✅ | Sizes child as fraction of available space |
| RenderIntrinsicHeight | intrinsic_height.rs | Single | RenderBox<Single> | ✅ | Queries child's intrinsic height |
| RenderIntrinsicWidth | intrinsic_width.rs | Single | RenderBox<Single> | ✅ | Queries child's intrinsic width |
| RenderPadding | padding.rs | Single | RenderBox<Single> | ✅ | Adds padding around child |
| RenderPositionedBox | positioned_box.rs | Single | RenderBox<Single> | ✅ | Positions child at fixed offset |
| RenderRotatedBox | rotated_box.rs | Single | RenderBox<Single> | ✅ | Rotates child 90° increments |
| RenderShiftedBox | shifted_box.rs | Single | RenderBox<Single> | ✅ | Applies fractional translation |
| RenderSizedOverflowBox | sized_overflow_box.rs | Single | RenderBox<Single> | ✅ | Allows child to overflow constraints |

**Not Yet Migrated (TODO) ❌:**

| Object | File | Arity | Expected Trait | Notes |
|--------|------|-------|----------------|---------| 
| RenderAlign | align.rs | Optional | RenderBox<Optional> | Aligns child within space, with optional size factors |
| RenderConstrainedBox | constrained_box.rs | Optional | RenderBox<Optional> | Applies additional constraints to child |
| RenderConstrainedOverflowBox | constrained_overflow_box.rs | Optional | RenderBox<Optional> | Allows overflow while constraining layout |
| RenderConstraintsTransformBox | constraints_transform_box.rs | Single | RenderBox<Single> | Transforms parent constraints before passing to child |
| RenderCustomMultiChildLayoutBox | custom_multi_child_layout_box.rs | Variable | RenderBox<Variable> | Custom multi-child layout via delegate pattern |
| RenderCustomSingleChildLayoutBox | custom_single_child_layout_box.rs | Single | RenderBox<Single> | Custom single-child layout via delegate |
| RenderEditableLine | editable_line.rs | Optional | RenderBox<Optional> | Text editing with selection (advanced) |
| RenderEmpty | empty.rs | Leaf | RenderBox<Leaf> | Empty spacer (simple, low priority) |
| RenderFlex | flex.rs | Variable | RenderBox<Variable> | **HIGH PRIORITY** - Row/Column layout (used everywhere) |
| RenderFlexItem | flex_item.rs | Single | RenderBox<Single> | Metadata provider for flexible children |
| RenderFlow | flow.rs | Variable | RenderBox<Variable> | Custom layout with transform-based positioning |
| RenderFractionalTranslation | fractional_translation.rs | Single | RenderBox<Single> | Fractional offset (simpler than PositionedBox) |
| RenderGrid | grid.rs | Variable | RenderBox<Variable> | **MEDIUM PRIORITY** - Grid layout system |
| RenderIndexedStack | indexed_stack.rs | Variable | RenderBox<Variable> | Shows one child at a time (similar to IndexedStack) |
| RenderLimitedBox | limited_box.rs | Optional | RenderBox<Optional> | Limits max width/height constraints |
| RenderListBody | list_body.rs | Variable | RenderBox<Variable> | Vertical scrollable list (non-sliver) |
| RenderListWheelViewport | list_wheel_viewport.rs | Variable | RenderBox<Variable> | Wheel/picker-style scrolling |
| RenderOverflowBox | overflow_box.rs | Optional | RenderBox<Optional> | Allows child to overflow constraints |
| RenderPositioned | positioned.rs | Single | RenderBox<Single> | Metadata provider for Positioned children in Stack |
| RenderScrollView | scroll_view.rs | Single | RenderBox<Single> | Non-sliver scrollable container (low priority) |
| RenderSizedBox | sized_box.rs | Optional | RenderBox<Optional> | Forces exact size (Spacer widget) |
| RenderStack | stack.rs | Variable | RenderBox<Variable> | **HIGH PRIORITY** - Layered/overlapping layout |
| RenderTable | table.rs | Variable | RenderBox<Variable> | Table layout system |
| RenderWrap | wrap.rs | Variable | RenderBox<Variable> | **MEDIUM PRIORITY** - Wrapping layout (like word wrap) |

**Analysis**:
- 10 objects already migrated (mostly Single arity, simple)
- 14 objects are Optional or Single arity (medium complexity)
- 10 objects are Variable arity (complex, need metadata handling)
- High-impact missing: **RenderFlex, RenderStack** (used by most widgets)

---

### EFFECTS MODULE (13 Migrated ✅ / 6 TODO ❌)

**Migrated (Single Arity) ✅:**

| Object | File | Arity | Trait | Status |
|--------|------|-------|-------|--------|
| RenderAnimatedOpacity | animated_opacity.rs | Single | RenderBox<Single> | ✅ |
| RenderBackdropFilter | backdrop_filter.rs | Single | RenderBox<Single> | ✅ |
| RenderClipOval | clip_oval.rs | Single | RenderBox<Single> | ✅ |
| RenderClipPath | clip_path.rs | Single | RenderBox<Single> | ✅ |
| RenderClipRect | clip_rect.rs | Single | RenderBox<Single> | ✅ |
| RenderClipRRect | clip_rrect.rs | Single | RenderBox<Single> | ✅ |
| RenderCustomPaint | custom_paint.rs | Single | RenderBox<Single> | ✅ |
| RenderOffstage | offstage.rs | Single | RenderBox<Single> | ✅ |
| RenderOpacity | opacity.rs | Single | RenderBox<Single> | ✅ |
| RenderRepaintBoundary | repaint_boundary.rs | Single | RenderBox<Single> | ✅ |
| RenderShaderMask | shader_mask.rs | Single | RenderBox<Single> | ✅ |
| RenderTransform | transform.rs | Single | RenderBox<Single> | ✅ |
| RenderVisibility | visibility.rs | Single | RenderBox<Single> | ✅ |

**Not Yet Migrated (TODO) ❌:**

| Object | File | Arity | Expected Trait | Notes |
|--------|------|-------|----------------|-------|
| RenderAnimatedSize | animated_size.rs | Variable | RenderBox<Variable> | Animation + size changes |
| RenderDecoratedBox | decorated_box.rs | Optional | RenderBox<Optional> | Complex decorations (border, shadow, etc.) |
| RenderPhysicalModel | physical_model.rs | Optional | RenderBox<Optional> | 3D shadow effects |
| RenderPhysicalShape | physical_shape.rs | Optional | RenderBox<Optional> | 3D shadow with custom shape |

**Analysis**:
- All Single arity effects migrated (simple pass-through pattern)
- Only 4 complex effects remaining (Optional/Variable arity)
- **RenderDecoratedBox** is important for styling

---

### INTERACTION MODULE (4 Migrated ✅ / 0 TODO ❌)

**Migrated (All use RenderBoxProxy pattern) ✅:**

| Object | File | Arity | Trait | Status | Notes |
|--------|------|-------|-------|--------|-------|
| RenderAbsorbPointer | absorb_pointer.rs | Single | RenderBoxProxy | ✅ | Prevents hit test pass-through |
| RenderIgnorePointer | ignore_pointer.rs | Single | RenderBoxProxy | ✅ | Ignores pointer events |
| RenderMouseRegion | mouse_region.rs | Single | RenderBoxProxy | ✅ | Mouse hover/enter/exit events |
| RenderPointerListener | pointer_listener.rs | Single | RenderBoxProxy | ✅ | Generic pointer event listener |

**Analysis**:
- ✅ **100% Complete** - All interaction objects migrated
- Perfect use case for RenderBoxProxy pattern (just forward events)

---

### SPECIAL MODULE (6 Migrated ✅ / 2 TODO ❌)

**Migrated (All use RenderBoxProxy except RenderView) ✅:**

| Object | File | Arity | Trait | Status | Notes |
|--------|------|-------|-------|--------|-------|
| RenderAnnotatedRegion | annotated_region.rs | Single | RenderBoxProxy | ✅ | Semantic annotation |
| RenderBlockSemantics | block_semantics.rs | Single | RenderBoxProxy | ✅ | Semantic blocking |
| RenderColoredBox | colored_box.rs | Leaf | RenderBox<Leaf> | ✅ | Simple color background |
| RenderExcludeSemantics | exclude_semantics.rs | Single | RenderBoxProxy | ✅ | Exclude from semantics |
| RenderMergeSemantics | merge_semantics.rs | Single | RenderBoxProxy | ✅ | Merge semantics |
| RenderView | render_view.rs | Single | RenderBox<Single> | ✅ | Root render object |

**Not Yet Migrated (TODO) ❌:**

| Object | File | Arity | Expected Trait | Notes |
|--------|------|-------|---|-------|
| RenderFittedBox | fitted_box.rs | Single | RenderBox<Single> | Scales/fits child with optional aspect ratio |
| RenderMetaData | metadata.rs | Single | RenderBoxProxy | Generic metadata provider |

**Analysis**:
- 6/8 migrated (75%)
- RenderFittedBox needs migration (similar to other effects)
- RenderMetaData could use RenderBoxProxy

---

### DEBUG MODULE (Disabled 🔒 / 4 objects)

Located at `debug/` - currently disabled in `mod.rs`

**Objects (Not Migrated) ❌:**

| Object | File | Arity | Expected Trait | Priority |
|--------|------|-------|---|----------|
| RenderErrorBox | error_box.rs | Single | RenderBox<Single> | Low - debug only |
| RenderOverflowIndicator | overflow_indicator.rs | Single | RenderBox<Single> | Low - debug only |
| RenderPerformanceOverlay | performance_overlay.rs | Leaf | RenderBox<Leaf> | Low - debug only |
| RenderPlaceholder | placeholder.rs | Single | RenderBox<Single> | Low - debug only |

**Analysis**:
- Low priority (debug-only)
- Can be migrated after core objects
- All Single or Leaf arity (simple)

---

### MEDIA MODULE (Disabled 🔒 / 3 objects)

Located at `media/` - currently disabled in `mod.rs`

**Objects (Not Migrated) ❌:**

| Object | File | Arity | Expected Trait | Priority | Notes |
|--------|------|-------|---|----------|-------|
| RenderImage | image.rs | Leaf | RenderBox<Leaf> | HIGH | Image rendering (commonly used) |
| RenderTexture | texture.rs | Leaf | RenderBox<Leaf> | MEDIUM | GPU texture rendering |

**Analysis**:
- **RenderImage is HIGH priority** (commonly used in widgets)
- Both are Leaf arity (simple)
- Should be re-enabled early

---

### TEXT MODULE (Disabled 🔒 / 2 objects)

Located at `text/` - currently disabled in `mod.rs`

**Objects (Not Migrated) ❌:**

| Object | File | Arity | Expected Trait | Priority | Notes |
|--------|------|-------|---|----------|-------|
| RenderParagraph | paragraph.rs | Leaf | RenderBox<Leaf> | HIGH | Multi-line text (core feature) |

**Analysis**:
- **RenderParagraph is CRITICAL** (text rendering is essential)
- Leaf arity (simple)
- Must be re-enabled immediately

---

### VIEWPORT MODULE (Disabled 🔒 / 3 objects)

Located at `viewport/` - currently disabled in `mod.rs`

**Objects (Not Migrated) ❌:**

| Object | File | Arity | Expected Trait | Protocol | Priority |
|--------|------|-------|---|----------|----------|
| RenderAbstractViewport | abstract_viewport.rs | Variable | RenderSliver<Variable> | SliverProtocol | HIGH |
| RenderShrinkWrappingViewport | shrink_wrapping_viewport.rs | Single | RenderBox<Single> | BoxProtocol | MEDIUM |

**Analysis**:
- Critical for scrollable layouts
- Mix of Box and Sliver protocols
- **RenderAbstractViewport is HIGH priority** (needed for lists/grids)

---

### SLIVER MODULE (Disabled 🔒 / 24 objects)

Located at `sliver/` - currently disabled in `mod.rs`

**Objects (All Not Migrated) ❌:**

| Object | File | Arity | Expected Trait | Priority | Notes |
|--------|------|-------|---|----------|-------|
| RenderSliverAnimatedOpacity | sliver_animated_opacity.rs | Single | SliverRender<Single> | MEDIUM | Animated opacity for slivers |
| RenderSliverAppBar | sliver_app_bar.rs | Single | SliverRender<Single> | HIGH | Sticky app bar (common) |
| RenderSliverConstrainedCrossAxis | sliver_constrained_cross_axis.rs | Single | SliverRender<Single> | MEDIUM | Cross axis constraints |
| RenderSliverCrossAxisGroup | sliver_cross_axis_group.rs | Variable | SliverRender<Variable> | MEDIUM | Group slivers across axis |
| RenderSliverEdgeInsetsPadding | sliver_edge_insets_padding.rs | Single | SliverRender<Single> | MEDIUM | Padding for slivers |
| RenderSliverFillRemaining | sliver_fill_remaining.rs | Single | SliverRender<Single> | MEDIUM | Fill remaining space |
| RenderSliverFillViewport | sliver_fill_viewport.rs | Single | SliverRender<Single> | MEDIUM | Fill viewport |
| RenderSliverFixedExtentList | sliver_fixed_extent_list.rs | Variable | SliverRender<Variable> | HIGH | **Optimized for fixed-size lists** |
| RenderSliverFloatingPersistentHeader | sliver_floating_persistent_header.rs | Single | SliverRender<Single> | MEDIUM | Floating header |
| RenderSliverGrid | sliver_grid.rs | Variable | SliverRender<Variable> | HIGH | **Grid in sliver context** |
| RenderSliverIgnorePointer | sliver_ignore_pointer.rs | Single | SliverRender<Single> | LOW | Ignore pointer for slivers |
| RenderSliverList | sliver_list.rs | Variable | SliverRender<Variable> | HIGH | **Core list with lazy loading** |
| RenderSliverMainAxisGroup | sliver_main_axis_group.rs | Variable | SliverRender<Variable> | MEDIUM | Group slivers along main axis |
| RenderSliverMultiBoxAdaptor | sliver_multi_box_adaptor.rs | Variable | SliverRender<Variable> | HIGH | **Base for list/grid adapters** |
| RenderSliverOffstage | sliver_offstage.rs | Single | SliverRender<Single> | LOW | Offstage for slivers |
| RenderSliverOpacity | sliver_opacity.rs | Single | SliverRender<Single> | MEDIUM | Opacity for slivers |
| RenderSliverOverlapAbsorber | sliver_overlap_absorber.rs | Single | SliverRender<Single> | MEDIUM | Handle overlapping content |
| RenderSliverPadding | sliver_padding.rs | Single | SliverRender<Single> | MEDIUM | Padding for slivers |
| RenderSliverPersistentHeader | sliver_persistent_header.rs | Single | SliverRender<Single> | MEDIUM | Sticky header |
| RenderSliverPinnedPersistentHeader | sliver_pinned_persistent_header.rs | Single | SliverRender<Single> | MEDIUM | Pinned header |
| RenderSliverPrototypeExtentList | sliver_prototype_extent_list.rs | Variable | SliverRender<Variable> | MEDIUM | List with prototype child |
| RenderSliverSafeArea | sliver_safe_area.rs | Single | SliverRender<Single> | LOW | Safe area padding |
| RenderSliverToBoxAdapter | sliver_to_box_adapter.rs | Single | SliverRender<Single> | HIGH | **Bridges Box to Sliver protocol** |
| Viewport (RenderViewport) | viewport.rs | Variable | SliverRender<Variable> | HIGH | **Core viewport container** |

**Analysis**:
- **24 sliver objects** - comprehensive scrolling support
- Critical for production use: List, Grid, Viewport, MultiBoxAdapter
- These are complex but high-value
- Blocked on SliverRender trait implementation in flui_core (already done ✅)

---

## Part 3: Migration Priority Matrix

### CRITICAL (Blocks flui_widgets functionality)

**Phase 1A - Enable Core (Week 1)**

| Priority | Object | Module | Arity | Impl Effort | Impact | Est. Time |
|----------|--------|--------|-------|------------|---------|-----------|
| P0 | RenderParagraph | text | Leaf | 30m | Text rendering | HIGH |
| P0 | RenderImage | media | Leaf | 30m | Image display | HIGH |
| P0 | RenderFlex | layout | Variable | 3h | Row/Column layouts | **CRITICAL** |
| P0 | RenderStack | layout | Variable | 3h | Positioned layouts | **CRITICAL** |
| P0 | RenderViewport | sliver | Variable | 4h | Scrollable containers | **CRITICAL** |
| P0 | RenderSliverList | sliver | Variable | 4h | Scrollable lists | **CRITICAL** |

**Phase 1B - Complete Layout Foundation (Week 1-2)**

| Priority | Object | Module | Arity | Impl Effort | Impact | Est. Time |
|----------|--------|--------|-------|------------|---------|-----------|
| P1 | RenderSizedBox | layout | Optional | 1h | Spacer widget | HIGH |
| P1 | RenderEmpty | layout | Leaf | 30m | Empty spacer | MEDIUM |
| P1 | RenderAlign | layout | Optional | 2h | Center/Align widget | HIGH |
| P1 | RenderConstrainedBox | layout | Optional | 1h | Constraint widget | MEDIUM |
| P1 | RenderDecoratedBox | effects | Optional | 3h | Styling/decoration | HIGH |
| P1 | RenderGridLayout | layout | Variable | 3h | Grid layouts | HIGH |
| P1 | RenderWrap | layout | Variable | 2h | Wrapping layouts | MEDIUM |

**Phase 1C - Critical Slivers (Week 2-3)**

| Priority | Object | Module | Arity | Impl Effort | Impact | Est. Time |
|----------|--------|--------|-------|------------|---------|-----------|
| P1 | RenderSliverMultiBoxAdapter | sliver | Variable | 5h | Base adapter | **CRITICAL** |
| P1 | RenderSliverFixedExtentList | sliver | Variable | 3h | Optimized lists | HIGH |
| P1 | RenderSliverGrid | sliver | Variable | 4h | Grids in slivers | HIGH |
| P1 | RenderSliverAppBar | sliver | Single | 2h | App bars | MEDIUM |
| P1 | RenderAbstractViewport | viewport | Variable | 3h | Viewport base | HIGH |

### HIGH PRIORITY (Used by most widgets)

**Phase 2 - Layout Completeness (Week 3-4)**

| Priority | Object | Module | Arity | Impl Effort | Impact | Est. Time |
|----------|--------|--------|-------|------------|---------|-----------|
| P2 | RenderFittedBox | special | Single | 1h | Responsive images | HIGH |
| P2 | RenderFlexItem | layout | Single | 1h | Flex metadata | MEDIUM |
| P2 | RenderPositioned | layout | Single | 1h | Stack positioning | MEDIUM |
| P2 | RenderIntrinsicHeight | layout | Single | 2h | Layout helpers | MEDIUM |
| P2 | RenderIntrinsicWidth | layout | Single | 2h | Layout helpers | MEDIUM |
| P2 | RenderScrollView | layout | Single | 2h | Non-sliver scrolling | MEDIUM |
| P2 | RenderSliverPersistentHeader | sliver | Single | 2h | Sticky headers | MEDIUM |
| P2 | RenderSliverToBoxAdapter | sliver | Single | 1h | Protocol bridge | MEDIUM |

### MEDIUM PRIORITY (Nice to have, not blocking)

**Phase 3 - Advanced Features (Week 4-5)**

| Priority | Object | Module | Arity | Impl Effort | Impact | Est. Time |
|----------|--------|--------|-------|------------|---------|-----------|
| P3 | RenderFlow | layout | Variable | 3h | Custom positioning | LOW |
| P3 | RenderCustomMultiChildLayoutBox | layout | Variable | 3h | Custom layouts | LOW |
| P3 | RenderCustomSingleChildLayoutBox | layout | Single | 2h | Custom layouts | LOW |
| P3 | RenderIndexedStack | layout | Variable | 2h | Index-based display | MEDIUM |
| P3 | RenderListWheelViewport | layout | Variable | 3h | Picker wheels | LOW |
| P3 | RenderListBody | layout | Variable | 2h | Non-sliver lists | LOW |
| P3 | RenderEditableLine | layout | Optional | 4h | Text editing | LOW |
| P3 | RenderPhysicalModel | effects | Optional | 2h | 3D shadows | LOW |
| P3 | RenderPhysicalShape | effects | Optional | 2h | 3D shadows | LOW |
| P3 | RenderAnimatedSize | effects | Variable | 3h | Size animations | MEDIUM |
| P3 | RenderTable | layout | Variable | 3h | Table layouts | LOW |
| P3 | RenderLimitedBox | layout | Optional | 1h | Size limiting | LOW |
| P3 | RenderOverflowBox | layout | Optional | 1h | Overflow handling | LOW |
| P3 | RenderConstrainedOverflowBox | layout | Optional | 1h | Overflow handling | LOW |
| P3 | RenderMetaData | special | Single | 1h | Generic metadata | LOW |
| P3 | Various Sliver objects (9) | sliver | Single/Var | 15h total | Advanced scrolling | LOW |

### LOW PRIORITY (Debug/nice-to-have only)

**Phase 4 - Debug & Utilities**

| Priority | Objects | Module | Total Count | Est. Time |
|----------|---------|--------|-------------|-----------|
| P4 | RenderErrorBox, RenderPlaceholder, etc. | debug | 4 | 2h |
| P4 | RenderTexture | media | 1 | 1h |
| P4 | RenderShrinkWrappingViewport | viewport | 1 | 1h |
| P4 | Various low-use sliver objects (7) | sliver | 7 | 4h |

---

## Part 4: Migration Pattern & Implementation Guide

### Pattern for Migrated Objects (RenderBox<Single>)

**Example: RenderOpacity** (already migrated)

```rust
use flui_core::render::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::Size;

#[derive(Debug)]
pub struct RenderOpacity {
    pub opacity: f32,
}

impl RenderBox<Single> for RenderOpacity {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        // Child layout returns child size directly
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();
        // Paint with opacity layer
        ctx.paint_child_with_opacity(child_id, ctx.offset, self.opacity);
    }
}
```

### Pattern for Optional Arity (RenderBox<Optional>)

**Example: RenderSizedBox** (needs migration)

```rust
use flui_core::render::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Optional};
use flui_types::{Size, BoxConstraints};

#[derive(Debug)]
pub struct RenderSizedBox {
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, ctx: LayoutContext<'_, Optional, BoxProtocol>) -> Size {
        let width = self.width.unwrap_or(ctx.constraints.max_width);
        let height = self.height.unwrap_or(ctx.constraints.max_height);
        
        let size = Size::new(width, height).constrain(&ctx.constraints);
        
        // If child exists, layout it with tight constraints
        if let Some(child_id) = ctx.children.get() {
            ctx.layout_child(child_id, BoxConstraints::tight(size));
        }
        
        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Optional>) {
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, ctx.offset);
        }
    }
}
```

### Pattern for Variable Arity (RenderBox<Variable>)

**Example: RenderFlex** (needs migration)

```rust
use flui_core::render::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Variable};
use flui_types::{Size, Offset};

#[derive(Debug)]
pub struct RenderFlex {
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    // ... other properties
    child_offsets: Vec<Offset>,  // Cache for paint
}

impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
        let children = ctx.children;
        self.child_offsets.clear();
        
        // Layout each child
        let mut total_height = 0.0;
        let mut max_width = 0.0;
        
        for &child_id in children.iter() {
            let child_size = ctx.layout_child(child_id, child_constraints);
            self.child_offsets.push(Offset::new(0.0, total_height));
            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
        }
        
        // Apply alignment
        // ... alignment logic ...
        
        Size::new(max_width, total_height)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
        for (idx, &child_id) in ctx.children.iter().enumerate() {
            let child_offset = ctx.offset + self.child_offsets[idx];
            ctx.paint_child(child_id, child_offset);
        }
    }
}
```

### Pattern for RenderBoxProxy (pass-through objects)

**Example: RenderMouseRegion** (already migrated)

```rust
use flui_core::render::RenderBoxProxy;

#[derive(Debug)]
pub struct RenderMouseRegion {
    pub on_enter: Option<Box<dyn Fn() + Send + Sync>>,
    pub on_exit: Option<Box<dyn Fn() + Send + Sync>>,
}

impl RenderBoxProxy for RenderMouseRegion {
    // All methods use default implementations
    // Just override if you need custom behavior
}
// RenderBox<Single> implemented automatically!
```

### Pattern for Sliver Objects (SliverRender<A>)

**Example: RenderSliverList** (needs migration)

```rust
use flui_core::render::{SliverProtocol, LayoutContext, PaintContext, SliverRender, Variable};
use flui_types::{SliverConstraints, SliverGeometry};

#[derive(Debug)]
pub struct RenderSliverList {
    // ... fields ...
}

impl SliverRender<Variable> for RenderSliverList {
    fn layout(&mut self, ctx: LayoutContext<'_, Variable, SliverProtocol>) -> SliverGeometry {
        let constraints = &ctx.constraints;
        let children = ctx.children;
        
        // Layout visible children
        let mut paint_extent = 0.0;
        for &child_id in children.iter() {
            let child_geometry = ctx.layout_child(child_id, constraints);
            paint_extent += child_geometry.paint_extent;
        }
        
        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent: paint_extent.min(constraints.remaining_paint_extent),
            ..Default::default()
        }
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
        // Paint visible children only
        let offset = ctx.offset;
        for &child_id in ctx.children.iter() {
            ctx.paint_child(child_id, offset);
        }
    }
}
```

---

## Part 5: Dependency Graph & Migration Order

### Dependency Analysis

```
RenderParagraph (leaf)
    ↓ (needed by)
RenderText (widget, uses Paragraph)

RenderImage (leaf)
    ↓ (needed by)
RenderImage widget

RenderFlex (variable)
    ↓ (needs metadata support for FlexItem)
RenderFlexItem (single, metadata provider)
    ↓ (needed by)
Row/Column widgets

RenderStack (variable)
    ↓ (needs metadata support)
RenderPositioned (single, metadata provider)
    ↓ (needed by)
Positioned widget

RenderViewport (variable, sliver)
    ↓ (base for)
RenderSliverList (variable)
    ↓ (needs)
RenderSliverMultiBoxAdapter (variable, base adapter)
    ↓ (needed by)
ListView, GridView

RenderGrid (variable, box)
    ↓ (needed by)
Grid widget

RenderCustomMultiChildLayoutBox (variable)
    ↓ (needed by)
CustomMultiChildLayout widget

RenderScrollView (single, box)
    ↓ (optional, non-sliver scrolling)
SingleChildScrollView widget
```

### Recommended Migration Sequence

**Week 1: Foundation (26 objects)**
1. Re-enable TEXT module: RenderParagraph ✅ (leaf, simple)
2. Re-enable MEDIA module: RenderImage ✅ (leaf, simple)
3. Migrate LAYOUT/empty.rs: RenderEmpty ✅ (leaf, simple)
4. Migrate LAYOUT/flex.rs: RenderFlex ✅ (variable, complex but essential)
5. Migrate LAYOUT/stack.rs: RenderStack ✅ (variable, complex but essential)
6. Migrate LAYOUT/flex_item.rs: RenderFlexItem ✅ (single, metadata)
7. Migrate LAYOUT/positioned.rs: RenderPositioned ✅ (single, metadata)
8. Migrate LAYOUT/aligned.rs: RenderAlign ✅ (optional, common)
9. Migrate LAYOUT/sized_box.rs: RenderSizedBox ✅ (optional, common)
10. Migrate LAYOUT/constrained_box.rs: RenderConstrainedBox ✅ (optional)
11. Migrate SPECIAL/fitted_box.rs: RenderFittedBox ✅ (single)
12. Migrate SPECIAL/metadata.rs: RenderMetaData ✅ (single, proxy)
13. Migrate EFFECTS/decorated_box.rs: RenderDecoratedBox ✅ (optional, important)
14. Migrate LAYOUT/grid.rs: RenderGrid ✅ (variable)
15. Migrate LAYOUT/wrap.rs: RenderWrap ✅ (variable)

**Week 2: Scrolling Support (16 objects)**
16. Re-enable VIEWPORT module
17. Migrate VIEWPORT/abstract_viewport.rs: RenderAbstractViewport ✅ (variable, sliver)
18. Migrate SLIVER/viewport.rs: RenderViewport ✅ (variable, sliver)
19. Migrate SLIVER/sliver_multi_box_adaptor.rs: RenderSliverMultiBoxAdapter ✅ (variable, base)
20. Migrate SLIVER/sliver_list.rs: RenderSliverList ✅ (variable)
21. Migrate SLIVER/sliver_grid.rs: RenderSliverGrid ✅ (variable)
22. Migrate SLIVER/sliver_fixed_extent_list.rs: RenderSliverFixedExtentList ✅ (variable)
23. Migrate SLIVER/sliver_app_bar.rs: RenderSliverAppBar ✅ (single)
24. Migrate SLIVER/sliver_to_box_adapter.rs: RenderSliverToBoxAdapter ✅ (single)
25. Migrate SLIVER/sliver_persistent_header.rs: RenderSliverPersistentHeader ✅ (single)
26. Migrate SLIVER/sliver_floating_persistent_header.rs: RenderSliverFloatingPersistentHeader ✅ (single)
27. Other sliver single-child objects (8 objects) ✅ (various slivers)

**Week 3: Advanced Layouts (15 objects)**
28. Migrate LAYOUT/custom_multi_child_layout_box.rs: RenderCustomMultiChildLayoutBox ✅ (variable)
29. Migrate LAYOUT/custom_single_child_layout_box.rs: RenderCustomSingleChildLayoutBox ✅ (single)
30. Migrate LAYOUT/flow.rs: RenderFlow ✅ (variable)
31. Migrate LAYOUT/indexed_stack.rs: RenderIndexedStack ✅ (variable)
32. Migrate LAYOUT/list_body.rs: RenderListBody ✅ (variable)
33. Migrate LAYOUT/scroll_view.rs: RenderScrollView ✅ (single)
34. Migrate LAYOUT/table.rs: RenderTable ✅ (variable)
35. Migrate remaining LAYOUT objects (5 objects) ✅
36. Migrate remaining EFFECT objects (4 objects) ✅
37. Migrate SLIVER variable-arity objects (5 objects) ✅

**Week 4: Debug & Polish (9 objects)**
38. Re-enable DEBUG module & migrate (4 objects) ✅
39. Migrate VIEWPORT/shrink_wrapping_viewport.rs ✅ (single)
40. Migrate SLIVER low-priority objects (4 objects) ✅

---

## Part 6: Key Migration Challenges & Solutions

### Challenge 1: Child Count Arity Mismatch

**Problem**: Old code doesn't distinguish child count; new system requires exact arity.

**Solution**:
```rust
// Old: Single method handles variable children
// pub fn layout(&mut self, children: &[ElementId]) { ... }

// New: Type system enforces arity
impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
        let children = ctx.children;  // Slice-like accessor, fully typed
        for child_id in children.iter() { ... }
    }
}
```

**Action**: Review each object's child handling (0, 1, 0-1, many) and pick correct arity.

### Challenge 2: Metadata/ParentData System

**Problem**: Objects like RenderFlex need per-child metadata (flex factor, fit).

**Solution**: Use the RenderElement's `metadata()` getter in parent:
```rust
// In RenderFlex layout:
for &child_id in children.iter() {
    if let Some(metadata) = tree.get_metadata::<FlexItemMetadata>(child_id) {
        // Use metadata.flex, metadata.fit
    }
}
```

**Action**: For objects with metadata providers (FlexItem, Positioned), ensure parent can query it.

### Challenge 3: Cache Fields in RenderObjects

**Problem**: Objects cache sizes/offsets from layout for use in paint (e.g., `child_offsets: Vec`).

**Solution**: Store cache as fields and initialize in layout:
```rust
#[derive(Debug)]
pub struct RenderFlex {
    // ...
    child_offsets: Vec<Offset>,  // Computed during layout
}

impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
        self.child_offsets.clear();
        // Populate cache
    }
    
    fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
        // Use cache
    }
}
```

**Action**: Ensure cache is cleared/recomputed on every layout.

### Challenge 4: Optional vs Variable Arity Choice

**Problem**: Some objects accept 0-1 children (Optional) or many (Variable).

**Solution**:
- **Optional**: Sized/Container/SizedBox - can work without child
- **Variable**: Flex/Stack - expects multiple children but framework can handle 0-N

**Action**: Check original widget semantics and existing code patterns.

### Challenge 5: Sliver vs Box Protocol for the Same Object

**Problem**: Some objects can work with both protocols (e.g., opacity can be box or sliver).

**Solution**: Implement for each protocol:
```rust
impl RenderBox<Single> for RenderOpacity { ... }      // Box variant
impl SliverRender<Single> for RenderSliverOpacity { ... }  // Sliver variant
```

**Action**: Check if object appears in both box and sliver contexts.

---

## Part 7: Testing & Validation Strategy

### Per-Object Testing

For each migrated object, verify:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::render::{LayoutContext, BoxProtocol};
    use flui_types::BoxConstraints;

    #[test]
    fn test_layout_basic() {
        let mut render = RenderPadding::new(EdgeInsets::all(10.0));
        let constraints = BoxConstraints::loose(Size::new(100.0, 100.0));
        let ctx = MockLayoutContext::new(constraints, vec![child_id]);
        
        let size = render.layout(ctx);
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_children_accessor() {
        let children = vec![id1, id2, id3];
        let single_children = Single::from_slice(&children[..1]);
        assert_eq!(single_children.single(), children[0]);
    }
}
```

### Widget Integration Testing

Once objects are migrated, test widgets that use them:

```rust
// In flui_widgets tests
#[test]
fn test_row_widget_renders() {
    let row = Row::new().children(vec![
        Box::new(Text::new("Hello")),
        Box::new(Spacer::new(Size::new(10.0, 0.0))),
        Box::new(Text::new("World")),
    ]);
    
    let mut app = MockApp::new(row);
    app.render_frame();
    // Verify layout results
}
```

### Regression Testing

Before/after comparison:
1. Run existing examples (simplified_view.rs, demo examples)
2. Verify visual output unchanged
3. Performance benchmarks (should be same or better)

---

## Part 8: Summary Statistics

### Overall Status

| Category | Total | Migrated | Pending | % Complete |
|----------|-------|----------|---------|------------|
| **Layout** | 34 | 10 | 24 | 29% |
| **Effects** | 19 | 13 | 6 | 68% |
| **Interaction** | 4 | 4 | 0 | 100% ✅ |
| **Special** | 8 | 6 | 2 | 75% |
| **Debug** | 4 | 0 | 4 | 0% |
| **Media** | 3 | 0 | 3 | 0% |
| **Text** | 2 | 0 | 2 | 0% |
| **Viewport** | 3 | 0 | 3 | 0% |
| **Sliver** | 24 | 0 | 24 | 0% |
| **TOTAL** | **129** | **37** | **92** | **29%** |

### Effort Estimation

| Phase | Objects | Complexity | Est. Time | Team Size |
|-------|---------|-----------|-----------|-----------|
| Phase 1A (Core) | 6 | Leaf + High-complexity Variable | 15h | 2 devs (1 week) |
| Phase 1B (Layout) | 12 | Mix of Optional/Single/Variable | 20h | 2 devs (1 week) |
| Phase 1C (Slivers) | 8 | Complex Variable + Single | 20h | 2 devs (1 week) |
| Phase 2 (Advanced) | 10 | Mix | 15h | 1 dev (1 week) |
| Phase 3 (Complex) | 15 | Advanced layouts, low priority | 18h | 1 dev (1 week) |
| Phase 4 (Polish) | 9 | Debug, low priority | 8h | 0.5 dev (3 days) |
| **TOTAL** | **60** | - | **~96h** | **2 devs, ~6 weeks** |

### Blocking Dependencies

**Critical path to unblock flui_widgets:**
1. ✅ Traits defined (RenderBox, SliverRender, protocols) - DONE
2. ✅ Arity system working - DONE
3. ⏳ **Paragraph + Image (text rendering)** - 1h
4. ⏳ **Flex + Stack** - 6h (CRITICAL - blocks Row/Column/Scaffold)
5. ⏳ **Optional arity objects** - 10h (blocks Padding/Container)
6. ⏳ **Sliver objects + Viewport** - 20h (blocks scrolling widgets)

**Minimum viable for flui_widgets v0.6.0**: Objects in phases 1A + 1B + 1C = ~55h = 2 devs × 2 weeks

---

## Conclusion

The flui_rendering crate is **mid-migration** with a clear path to completion:

✅ **Already Done**:
- Interaction module (100%)
- Effect module (68%)
- Special module (75%)
- Core traits in flui_core

❌ **Needs Immediate Work**:
- **Paragraph** (text) & **Image** (media) - enable + simple migration
- **Flex & Stack** - complex but unlocks Row/Column widgets
- **Sliver system** - 24 objects, complex but essential for production

📋 **Recommended Action**:
1. **Week 1**: Prioritize Paragraph, Image, Flex, Stack, SizedBox
2. **Week 2**: Complete Flex ecosystem (FlexItem, Align, etc.) + basic Slivers
3. **Week 3-4**: Advanced layouts and remaining Slivers
4. **Week 5-6**: Polish and integration testing

With focused effort on the 6 critical objects in Phase 1A, core widget functionality can be restored within 1 week. Full completion to 100% in 6 weeks with standard team velocity.
