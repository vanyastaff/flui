# Object Catalog

**Complete catalog of 85 render objects organized by 13 functional categories**

---

## Organization

Objects are organized by **purpose** rather than implementation. This makes discovery easier: need opacity? Look in `effects/`. Need input handling? Check `interaction/`.

---

## Statistics

| Protocol | Objects | Categories |
|----------|---------|------------|
| Box | 60 | 12 |
| Sliver | 25 | 5 |
| **Total** | **85** | **13** |

---

## Box Protocol Objects (60)

### 1. Basic (6 objects)

Simple single-child modifications.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderPadding** | RenderShiftedBox | Shifted<P> | Adds padding around child |
| **RenderAlign** | RenderAligningShiftedBox | Aligning<P> | Aligns child within available space |
| **RenderConstrainedBox** | RenderProxyBox | Proxy<P> | Applies additional constraints |
| **RenderSizedBox** | RenderProxyBox | Proxy<P> | Forces specific size |
| **RenderAspectRatio** | RenderProxyBox | Proxy<P> | Maintains aspect ratio |
| **RenderBaseline** | RenderShiftedBox | Shifted<P> | Positions child at baseline |

**Files:**
```
objects/box/basic/
├── padding.rs
├── align.rs
├── constrained_box.rs
├── sized_box.rs
├── aspect_ratio.rs
└── baseline.rs
```

---

### 2. Layout (15 objects)

Multi-child layout algorithms.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderFlex** | MultiChildRenderBox | BoxChildren<FlexParentData> | Row/Column layout |
| **RenderStack** | MultiChildRenderBox | BoxChildren<StackParentData> | Z-axis stacking |
| **RenderIndexedStack** | MultiChildRenderBox | BoxChildren<StackParentData> | Shows one child by index |
| **RenderWrap** | MultiChildRenderBox | BoxChildren<WrapParentData> | Wrapping flow layout |
| **RenderFlow** | MultiChildRenderBox | BoxChildren<FlowParentData> | Custom delegate layout |
| **RenderListBody** | MultiChildRenderBox | BoxChildren<ListBodyParentData> | Simple list layout |
| **RenderTable** | MultiChildRenderBox | BoxChildren<TableCellParentData> | Table layout |
| **RenderCustomLayout** | MultiChildRenderBox | BoxChildren<MultiChildLayoutParentData> | Custom delegate layout |
| **RenderIntrinsicWidth** | RenderProxyBox | Proxy<P> | Forces intrinsic width |
| **RenderIntrinsicHeight** | RenderProxyBox | Proxy<P> | Forces intrinsic height |
| **RenderLimitedBox** | RenderProxyBox | Proxy<P> | Limits unbounded constraints |
| **RenderFractionallySizedOverflowBox** | RenderAligningShiftedBox | Aligning<P> | Fractional sizing with overflow |
| **RenderConstrainedOverflowBox** | RenderAligningShiftedBox | Aligning<P> | Constrained sizing with overflow |
| **RenderSizedOverflowBox** | RenderAligningShiftedBox | Aligning<P> | Fixed sizing with overflow |
| **RenderConstraintsTransformBox** | RenderAligningShiftedBox | Aligning<P> | Transforms constraints |

**Files:**
```
objects/box/layout/
├── flex.rs
├── stack.rs
├── indexed_stack.rs
├── wrap.rs
├── flow.rs
├── list_body.rs
├── table.rs
├── custom_layout.rs
├── intrinsic_width.rs
├── intrinsic_height.rs
├── limited_box.rs
├── fractionally_sized_overflow_box.rs
├── constrained_overflow_box.rs
├── sized_overflow_box.rs
└── constraints_transform_box.rs
```

---

### 3. Effects (11 objects)

Visual effects and transformations.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderOpacity** | RenderProxyBox | Proxy<P> | Applies opacity |
| **RenderTransform** | RenderProxyBox | Proxy<P> | Applies matrix transform |
| **RenderFittedBox** | RenderProxyBox | Proxy<P> | Scales child to fit |
| **RenderFractionalTranslation** | RenderProxyBox | Proxy<P> | Translates by fraction |
| **RenderRotatedBox** | SingleChildRenderBox | Single<P> | Rotates by 90° increments |
| **RenderClipRect** | ClipProxy<Rect> | Proxy<P> | Clips to rectangle |
| **RenderClipRRect** | ClipProxy<RRect> | Proxy<P> | Clips to rounded rect |
| **RenderClipOval** | ClipProxy<Rect> | Proxy<P> | Clips to oval |
| **RenderClipPath** | ClipProxy<Path> | Proxy<P> | Clips to path |
| **RenderDecoratedBox** | RenderProxyBox | Proxy<P> | Paints decoration |
| **RenderBackdropFilter** | RenderProxyBox | Proxy<P> | Applies backdrop blur |

**Files:**
```
objects/box/effects/
├── opacity.rs
├── transform.rs
├── fitted_box.rs
├── fractional_translation.rs
├── rotated_box.rs
├── clip_rect.rs
├── clip_rrect.rs
├── clip_oval.rs
├── clip_path.rs
├── decorated_box.rs
└── backdrop_filter.rs
```

---

### 4. Animation (4 objects)

Animated effects that change over time.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderAnimatedOpacity** | RenderProxyBox + AnimatedOpacity | Proxy<P> | Animated opacity transition |
| **RenderAnimatedSize** | RenderAligningShiftedBox | Aligning<P> | Animated size transition |
| **RenderPhysicalModel** | PhysicalModelProxy | Proxy<P> | 3D elevation with shadows |
| **RenderPhysicalShape** | PhysicalModelProxy | Proxy<P> | 3D shape with shadows |

**Files:**
```
objects/box/animation/
├── animated_opacity.rs
├── animated_size.rs
├── physical_model.rs
└── physical_shape.rs
```

---

### 5. Interaction (6 objects)

Low-level input event handling.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderPointerListener** | HitTestProxy | Proxy<P> | Raw pointer events |
| **RenderMouseRegion** | HitTestProxy | Proxy<P> | Mouse enter/exit/hover |
| **RenderAbsorbPointer** | RenderProxyBox | Proxy<P> | Blocks pointer events |
| **RenderIgnorePointer** | RenderProxyBox | Proxy<P> | Passes through events |
| **RenderOffstage** | RenderProxyBox | Proxy<P> | Hides from hit testing |
| **RenderIgnoreBaseline** | RenderProxyBox | Proxy<P> | Ignores baseline queries |

**Files:**
```
objects/box/interaction/
├── pointer_listener.rs
├── mouse_region.rs
├── absorb_pointer.rs
├── ignore_pointer.rs
├── offstage.rs
└── ignore_baseline.rs
```

---

### 6. Gestures (4 objects)

High-level gesture recognition.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderTapRegion** | HitTestProxy | Proxy<P> | Tap gesture region |
| **RenderTapRegionSurface** | RenderProxyBox | Proxy<P> | Tap region coordinator |
| **RenderSemanticsGestureHandler** | HitTestProxy | Proxy<P> | Semantics gesture handling |
| **RenderCustomPaint** | RenderProxyBox | Proxy<P> | Custom paint delegate |

**Files:**
```
objects/box/gestures/
├── tap_region.rs
├── tap_region_surface.rs
├── semantics_gesture_handler.rs
└── custom_paint.rs
```

---

### 7. Media (3 objects)

Image, video, and texture rendering.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderImage** | RenderBox (leaf) | None | Renders image |
| **RenderTexture** | RenderBox (leaf) | None | Renders external texture |
| **RenderVideo** | RenderBox (leaf) | None | Renders video frames |

**Files:**
```
objects/box/media/
├── image.rs
├── texture.rs
└── video.rs
```

---

### 8. Text (2 objects)

Text rendering and editing.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderParagraph** | RenderBox (leaf) | None | Renders text paragraph |
| **RenderEditable** | RenderBox (leaf) | None | Editable text field |

**Files:**
```
objects/box/text/
├── paragraph.rs
└── editable.rs
```

---

### 9. Accessibility (5 objects)

Semantic annotations for accessibility.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSemanticsAnnotations** | RenderProxyBox + SemanticsAnnotations | Proxy<P> | Adds semantic properties |
| **RenderBlockSemantics** | RenderProxyBox | Proxy<P> | Blocks semantics traversal |
| **RenderExcludeSemantics** | RenderProxyBox | Proxy<P> | Excludes from semantics |
| **RenderIndexedSemantics** | RenderProxyBox | Proxy<P> | Adds scroll index |
| **RenderMergeSemantics** | RenderProxyBox | Proxy<P> | Merges child semantics |

**Files:**
```
objects/box/accessibility/
├── semantics_annotations.rs
├── block_semantics.rs
├── exclude_semantics.rs
├── indexed_semantics.rs
└── merge_semantics.rs
```

---

### 10. Platform (2 objects)

Platform-specific rendering.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderPlatformView** | RenderBox (leaf) | None | Embeds platform view |
| **RenderAnnotatedRegion** | RenderProxyBox | Proxy<P> | Platform region annotation |

**Files:**
```
objects/box/platform/
├── platform_view.rs
└── annotated_region.rs
```

---

### 11. Scroll (4 objects)

Viewport and scrolling support.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderView** | RenderBox | Single<P> | Root of render tree |
| **RenderViewport** | RenderBox | SliverChildren | Scrollable viewport |
| **RenderShrinkWrappingViewport** | RenderBox | SliverChildren | Shrink-wrapping viewport |
| **RenderListWheelViewport** | RenderBox | BoxChildren<ListWheelParentData> | Wheel scroll viewport |

**Files:**
```
objects/box/scroll/
├── view.rs
├── viewport.rs
├── shrink_wrapping_viewport.rs
└── list_wheel_viewport.rs
```

---

### 12. Debug (2 objects)

Debugging and performance visualization.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderErrorBox** | RenderBox (leaf) | None | Shows error message |
| **RenderPerformanceOverlay** | RenderBox (leaf) | None | FPS/GPU overlay |

**Files:**
```
objects/box/debug/
├── error_box.rs
└── performance_overlay.rs
```

---

## Sliver Protocol Objects (25)

### 1. Basic (5 objects)

Simple sliver modifications.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSliverPadding** | RenderProxySliver | SliverChild | Adds padding to sliver |
| **RenderSliverToBoxAdapter** | RenderSliverSingleBoxAdapter | BoxChild | Wraps box in sliver |
| **RenderSliverFillRemaining** | RenderSliverSingleBoxAdapter | BoxChild | Fills remaining space |
| **RenderSliverFillRemainingAndOverscroll** | RenderSliverSingleBoxAdapter | BoxChild | Fills + allows overscroll |
| **RenderSliverConstrainedCrossAxis** | RenderProxySliver | SliverChild | Constrains cross axis |

**Files:**
```
objects/sliver/basic/
├── padding.rs
├── to_box_adapter.rs
├── fill_remaining.rs
├── fill_remaining_and_overscroll.rs
└── constrained_cross_axis.rs
```

---

### 2. Layout (11 objects)

Multi-child sliver layouts.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSliverList** | RenderSliverMultiBoxAdaptor | BoxChildren<SliverMultiBoxAdaptorParentData> | Linear list |
| **RenderSliverFixedExtentList** | RenderSliverMultiBoxAdaptor | BoxChildren<SliverMultiBoxAdaptorParentData> | Fixed-height list |
| **RenderSliverGrid** | RenderSliverMultiBoxAdaptor | BoxChildren<SliverGridParentData> | Grid layout |
| **RenderSliverFillViewport** | RenderSliverMultiBoxAdaptor | BoxChildren<SliverMultiBoxAdaptorParentData> | Viewport-filling items |
| **RenderSliverVariedExtentList** | RenderSliverMultiBoxAdaptor | BoxChildren<SliverMultiBoxAdaptorParentData> | Variable-height list |
| **RenderSliverPersistentHeader** | RenderSliverPersistentHeader | BoxChild | Base for headers |
| **RenderSliverScrollingPersistentHeader** | RenderSliverPersistentHeader | BoxChild | Scrolling header |
| **RenderSliverPinnedPersistentHeader** | RenderSliverPersistentHeader | BoxChild | Pinned header |
| **RenderSliverFloatingPersistentHeader** | RenderSliverPersistentHeader | BoxChild | Floating header |
| **RenderSliverFloatingPinnedPersistentHeader** | RenderSliverPersistentHeader | BoxChild | Floating + pinned |
| **RenderTreeSliver** | RenderSliverMultiBoxAdaptor | BoxChildren<TreeSliverNodeParentData> | Tree structure list |

**Files:**
```
objects/sliver/layout/
├── list.rs
├── fixed_extent_list.rs
├── grid.rs
├── fill_viewport.rs
├── varied_extent_list.rs
├── persistent_header.rs
├── scrolling_persistent_header.rs
├── pinned_persistent_header.rs
├── floating_persistent_header.rs
├── floating_pinned_persistent_header.rs
└── tree_sliver.rs
```

---

### 3. Effects (3 objects)

Visual effects for slivers.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSliverOpacity** | RenderProxySliver | SliverChild | Applies opacity |
| **RenderSliverAnimatedOpacity** | RenderProxySliver + AnimatedOpacity | SliverChild | Animated opacity |
| **RenderDecoratedSliver** | RenderProxySliver | SliverChild | Paints decoration |

**Files:**
```
objects/sliver/effects/
├── opacity.rs
├── animated_opacity.rs
└── decorated_sliver.rs
```

---

### 4. Interaction (1 object)

Input handling for slivers.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSliverIgnorePointer** | RenderProxySliver | SliverChild | Passes through events |

**Files:**
```
objects/sliver/interaction/
└── ignore_pointer.rs
```

---

### 5. Scroll (5 objects)

Sliver grouping and organization.

| Object | Trait | Container | Purpose |
|--------|-------|-----------|---------|
| **RenderSliverOffstage** | RenderProxySliver | SliverChild | Hides from layout |
| **RenderSliverMainAxisGroup** | RenderSliver | SliverChildren<SliverPhysicalParentData> | Groups along main axis |
| **RenderSliverCrossAxisGroup** | RenderSliver | SliverChildren<SliverPhysicalParentData> | Groups along cross axis |
| **RenderSliverCrossAxisExpanded** | RenderProxySliver | SliverChild | Expands cross axis |
| **RenderSliverSemanticsAnnotations** | RenderProxySliver + SemanticsAnnotations | SliverChild | Semantic annotations |

**Files:**
```
objects/sliver/scroll/
├── offstage.rs
├── main_axis_group.rs
├── cross_axis_group.rs
├── cross_axis_expanded.rs
└── semantics_annotations.rs
```

---

## Object Selection Guide

### By Number of Children

| Children | Box Objects | Sliver Objects |
|----------|-------------|----------------|
| **Zero (Leaf)** | RenderImage, RenderParagraph, RenderEditable, RenderErrorBox, RenderPerformanceOverlay, RenderTexture, RenderVideo, RenderPlatformView | - |
| **One** | 41 objects (RenderOpacity, RenderPadding, etc.) | 13 objects (RenderSliverOpacity, RenderSliverPadding, etc.) |
| **Multiple** | 11 objects (RenderFlex, RenderStack, RenderTable, etc.) | 12 objects (RenderSliverList, RenderSliverGrid, etc.) |

### By Trait Family

| Trait | Count | Examples |
|-------|-------|----------|
| **RenderProxyBox** | 35 | RenderOpacity, RenderClipRect, RenderDecoratedBox |
| **RenderShiftedBox** | 2 | RenderPadding, RenderBaseline |
| **RenderAligningShiftedBox** | 6 | RenderAlign, RenderAnimatedSize, RenderFractionallySizedOverflowBox |
| **MultiChildRenderBox** | 8 | RenderFlex, RenderStack, RenderWrap, RenderTable |
| **RenderProxySliver** | 7 | RenderSliverOpacity, RenderSliverPadding |
| **RenderSliverSingleBoxAdapter** | 4 | RenderSliverToBoxAdapter, RenderSliverFillRemaining |
| **RenderSliverMultiBoxAdaptor** | 6 | RenderSliverList, RenderSliverGrid |
| **RenderSliverPersistentHeader** | 4 | Various header types |

### By Container Type

| Container | Objects |
|-----------|---------|
| `Proxy<P>` | ~40 |
| `Single<P>` | ~5 |
| `Shifted<P>` | ~2 |
| `Aligning<P>` | ~6 |
| `Children<P, PD>` | ~20 |
| None (leaf) | ~8 |

---

## Implementation Pattern

Every render object follows this structure:

```rust
use ambassador::Delegate;

#[derive(Debug, Delegate)]
#[delegate(TraitName, target = "container_field")]
pub struct RenderObjectName {
    // Container holds child(ren) and geometry
    container: ContainerType<Protocol>,
    
    // Object-specific properties
    property1: Type1,
    property2: Type2,
}

impl TraitName for RenderObjectName {
    // Minimal trait-specific implementation
}

impl ProtocolTrait for RenderObjectName {
    fn perform_layout(&mut self, constraints: Constraints) -> Geometry {
        // Layout logic
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Paint logic
    }
}
```

---

## File Organization

```
flui-rendering/src/objects/
├── mod.rs
├── box/
│   ├── mod.rs
│   ├── basic/           # 6 objects
│   ├── layout/          # 15 objects
│   ├── effects/         # 11 objects
│   ├── animation/       # 4 objects
│   ├── interaction/     # 6 objects
│   ├── gestures/        # 4 objects
│   ├── media/           # 3 objects
│   ├── text/            # 2 objects
│   ├── accessibility/   # 5 objects
│   ├── platform/        # 2 objects
│   ├── scroll/          # 4 objects
│   └── debug/           # 2 objects
└── sliver/
    ├── mod.rs
    ├── basic/           # 5 objects
    ├── layout/          # 11 objects
    ├── effects/         # 3 objects
    ├── interaction/     # 1 object
    └── scroll/          # 5 objects
```

**Total files: ~85 implementation files + ~17 mod.rs files = ~102 files**

---

## Next Steps

- [[Trait Hierarchy]] - Understanding traits
- [[Parent Data]] - Child metadata types
- [[Implementation Guide]] - Creating new objects

---

**See Also:**
- [[Protocol]] - Foundation system
- [[Containers]] - Child storage
- [[Pipeline]] - How objects integrate
