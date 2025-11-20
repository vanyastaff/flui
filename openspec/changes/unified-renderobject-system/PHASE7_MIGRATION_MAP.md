# Phase 7: Render Objects Migration Map

## Migration Strategy

Based on Flutter semantics and meaningful use cases without child.

---

## Category 1: Leaf (0 children) → `Render<Leaf>`

**Total: 7 objects**

No children accessor, compile error if child accessed.

```rust
impl Render<Leaf> for RenderX {
    fn layout(&mut self, ctx: &BoxLayoutContext<Leaf>) -> BoxGeometry {
        // ctx.children is NoChildren type - cannot access any children
    }
}
```

**List:**
1. `RenderEmpty` - `src/objects/layout/empty.rs`
2. `RenderImage` - `src/objects/media/image.rs`
3. `RenderTexture` - `src/objects/media/texture.rs`
4. `RenderParagraph` - `src/objects/text/paragraph.rs`
5. `RenderEditableLine` - `src/objects/layout/editable_line.rs`
6. `RenderPlaceholder` - `src/objects/debug/placeholder.rs`
7. `RenderErrorBox` - `src/objects/debug/error_box.rs`

---

## Category 2: Optional (0-1 child) → `Render<Optional>`

**Total: ~6-8 objects**

Child is optional. Meaningful behavior without child.

```rust
impl Render<Optional> for RenderX {
    fn layout(&mut self, ctx: &BoxLayoutContext<Optional>) -> BoxGeometry {
        if let Some(child) = ctx.children.get() {
            // Layout with child
        } else {
            // Layout without child - meaningful!
        }
    }
}
```

**Confirmed Optional (meaningful without child):**

### Spacer Pattern:
1. ✅ `RenderSizedBox` - `src/objects/layout/sized_box.rs`
   - **Use case**: `SizedBox(width: 10)` = horizontal spacer
   - **Without child**: Occupies specified width/height

### Decoration/Visual Pattern:
2. ✅ `RenderColoredBox` - `src/objects/special/colored_box.rs`
   - **Use case**: Solid color rectangle background
   - **Without child**: Shows colored box

3. ✅ `RenderDecoratedBox` - `src/objects/effects/decorated_box.rs`
   - **Use case**: Border, shadow, gradient без content
   - **Without child**: Shows decoration

4. ✅ `RenderPhysicalModel` - `src/objects/effects/physical_model.rs`
   - **Use case**: Elevation/shadow effect
   - **Without child**: Shows elevation shadow

5. ✅ `RenderPhysicalShape` - `src/objects/effects/physical_shape.rs`
   - **Use case**: Custom shape with shadow
   - **Without child**: Shows shape

### Maybe Optional (need verification):
6. ? `RenderConstrainedBox` - `src/objects/layout/constrained_box.rs`
   - Could enforce minimum size without child
   
7. ? `RenderLimitedBox` - `src/objects/layout/limited_box.rs`
   - Could limit size without child

---

## Category 3: Single (exactly 1 child) → `Render<Single>`

**Total: ~38-40 objects**

Child is required. No meaningful behavior without child.

```rust
impl Render<Single> for RenderX {
    fn layout(&mut self, ctx: &BoxLayoutContext<Single>) -> BoxGeometry {
        let child = ctx.children.single(); // Always returns ElementId
        // Layout child
    }
}
```

### Effects (no child = nothing to affect):
1. `RenderOpacity` - `src/objects/effects/opacity.rs`
2. `RenderTransform` - `src/objects/effects/transform.rs`
3. `RenderVisibility` - `src/objects/effects/visibility.rs`
4. `RenderOffstage` - `src/objects/effects/offstage.rs`
5. `RenderAnimatedOpacity` - `src/objects/effects/animated_opacity.rs`
6. `RenderAnimatedSize` - `src/objects/effects/animated_size.rs`
7. `RenderBackdropFilter` - `src/objects/effects/backdrop_filter.rs`
8. `RenderCustomPaint` - `src/objects/effects/custom_paint.rs`
9. `RenderRepaintBoundary` - `src/objects/effects/repaint_boundary.rs`
10. `RenderShaderMask` - `src/objects/effects/shader_mask.rs`

### Layout (no child = nothing to layout):
11. `RenderPadding` - `src/objects/layout/padding.rs`
12. `RenderAlign` - `src/objects/layout/align.rs`
13. `RenderCenter` - (same as Align with center alignment)
14. `RenderFittedBox` - `src/objects/special/fitted_box.rs`
15. `RenderAspectRatio` - `src/objects/layout/aspect_ratio.rs`
16. `RenderBaseline` - `src/objects/layout/baseline.rs`
17. `RenderConstraintsTransformBox` - `src/objects/layout/constraints_transform_box.rs`
18. `RenderCustomSingleChildLayoutBox` - `src/objects/layout/custom_single_child_layout_box.rs`
19. `RenderFlexItem` - `src/objects/layout/flex_item.rs`
20. `RenderFractionallySizedBox` - `src/objects/layout/fractionally_sized_box.rs`
21. `RenderFractionalTranslation` - `src/objects/layout/fractional_translation.rs`
22. `RenderIntrinsicHeight` - `src/objects/layout/intrinsic_height.rs`
23. `RenderIntrinsicWidth` - `src/objects/layout/intrinsic_width.rs`
24. `RenderOverflowBox` - `src/objects/layout/overflow_box.rs`
25. `RenderConstrainedOverflowBox` - `src/objects/layout/constrained_overflow_box.rs`
26. `RenderSizedOverflowBox` - `src/objects/layout/sized_overflow_box.rs`
27. `RenderPositioned` - `src/objects/layout/positioned.rs`
28. `RenderPositionedBox` - `src/objects/layout/positioned_box.rs`
29. `RenderRotatedBox` - `src/objects/layout/rotated_box.rs`
30. `RenderScrollView` - `src/objects/layout/scroll_view.rs`
31. `RenderShiftedBox` - `src/objects/layout/shifted_box.rs`

### Interaction (no child = nothing to interact with):
32. `RenderAbsorbPointer` - `src/objects/interaction/absorb_pointer.rs`
33. `RenderIgnorePointer` - `src/objects/interaction/ignore_pointer.rs`
34. `RenderMouseRegion` - `src/objects/interaction/mouse_region.rs`
35. `RenderPointerListener` - `src/objects/interaction/pointer_listener.rs`

### Semantics (no child = nothing to annotate):
36. `RenderBlockSemantics` - `src/objects/special/block_semantics.rs`
37. `RenderExcludeSemantics` - `src/objects/special/exclude_semantics.rs`
38. `RenderMergeSemantics` - `src/objects/special/merge_semantics.rs`
39. `RenderMetaData` - `src/objects/special/metadata.rs`
40. `RenderView` - `src/objects/special/render_view.rs`

---

## Category 4: Variable (N children) → `Render<Variable>`

**Total: 13 objects**

Any number of children.

```rust
impl Render<Variable> for RenderX {
    fn layout(&mut self, ctx: &BoxLayoutContext<Variable>) -> BoxGeometry {
        for child in ctx.children.iter() {
            // Layout each child
        }
    }
}
```

**List:**
1. `RenderFlex` - `src/objects/layout/flex.rs` (Row/Column)
2. `RenderStack` - `src/objects/layout/stack.rs`
3. `RenderWrap` - `src/objects/layout/wrap.rs`
4. `RenderFlow` - `src/objects/layout/flow.rs`
5. `RenderGrid` - `src/objects/layout/grid.rs`
6. `RenderTable` - `src/objects/layout/table.rs`
7. `RenderIndexedStack` - `src/objects/layout/indexed_stack.rs`
8. `RenderListBody` - `src/objects/layout/list_body.rs`
9. `RenderListWheelViewport` - `src/objects/layout/list_wheel_viewport.rs`
10. `RenderCustomMultiChildLayoutBox` - `src/objects/layout/custom_multi_child_layout_box.rs`
11. `RenderOverflowIndicator` - `src/objects/debug/overflow_indicator.rs`
12. `RenderViewport` - `src/objects/sliver/viewport.rs`
13. `RenderShrinkWrappingViewport` - `src/objects/viewport/shrink_wrapping_viewport.rs`

---

## Summary

| Category | Count | Arity Type |
|----------|-------|------------|
| Leaf | 7 | `Render<Leaf>` |
| Optional | ~6-8 | `Render<Optional>` |
| Single | ~38-40 | `Render<Single>` |
| Variable | 13 | `Render<Variable>` |
| **Total** | **66** | |

---

## Migration Order

1. **Start with Leaf** (simplest, no children)
2. **Then Optional** (small number, clear use cases)
3. **Then Single** (largest group, but straightforward)
4. **Finally Variable** (most complex child management)
5. **Update widgets** after all render objects migrated
6. **Remove legacy traits** last

---

## Decision Criteria Summary

**Optional** = Render object produces meaningful visual/layout output even without child
- ✅ SizedBox → spacer
- ✅ ColoredBox → background
- ✅ DecoratedBox → border/shadow
- ✅ PhysicalModel → elevation

**Single** = Render object only makes sense when wrapping/affecting a child
- ❌ Padding → nothing to pad
- ❌ Opacity → nothing to make transparent
- ❌ Transform → nothing to transform
- ❌ Align → nothing to align
