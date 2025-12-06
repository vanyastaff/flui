# Flutter Rendering Patterns for FLUI

This document describes how FLUI's rendering layer maps to Flutter's proven patterns and best practices.

## Table of Contents

- [Core Architecture](#core-architecture)
- [RenderObject Protocol](#renderobject-protocol)
- [Layout Constraints](#layout-constraints)
- [Dirty Tracking](#dirty-tracking)
- [Performance Boundaries](#performance-boundaries)
- [Common Patterns](#common-patterns)
- [Migration Guide](#migration-guide)

## Core Architecture

### Three-Phase Pipeline

Both Flutter and FLUI use a three-phase rendering pipeline:

| Phase | Flutter | FLUI | Purpose |
|-------|---------|------|---------|
| **Build** | `build()` → Widget tree | `build()` → View tree | Declarative UI description |
| **Layout** | `performLayout()` → Sizes | `layout()` → Sizes | Compute sizes and positions |
| **Paint** | `paint()` → Canvas | `paint()` → Canvas | Draw to screen |

```dart
// Flutter
class RenderBox {
  void performLayout() {
    size = constraints.biggest();
    if (child != null) {
      child.layout(constraints, parentUsesSize: true);
    }
  }

  void paint(PaintingContext context, Offset offset) {
    if (child != null) {
      context.paintChild(child, offset + childParentData.offset);
    }
  }
}
```

```rust
// FLUI
impl RenderBox<Single> for MyRenderObject {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        let size = ctx.constraints.biggest();
        let child_size = ctx.layout_single_child()?;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        ctx.paint_single_child(ctx.offset + child_offset)?;
    }
}
```

### Constraint-Based Layout

**Flutter's "Constraints go down, sizes come up" protocol:**

```text
Parent (400×600)
  ↓ constraints: BoxConstraints(0-400w × 0-600h)
Child
  ↑ size: Size(300×400)
Parent positions child
```

**FLUI follows this exactly:**

```rust
// Parent receives constraints from its parent
fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
    // 1. Pass constraints down to child
    let child_constraints = ctx.constraints.deflate(&padding);
    let child_size = ctx.layout_child(child_id, child_constraints)?;

    // 2. Child returns size up
    // 3. Parent computes own size
    let size = Size::new(
        child_size.width + padding.horizontal(),
        child_size.height + padding.vertical(),
    );

    // 4. Parent positions child
    ctx.set_child_offset(child_id, Offset::new(padding.left, padding.top));

    Ok(ctx.constraints.constrain(size))
}
```

## RenderObject Protocol

### Core Methods

| Flutter Method | FLUI Method | Called When |
|----------------|-------------|-------------|
| `performLayout()` | `layout()` | Constraints changed or marked dirty |
| `paint()` | `paint()` | Needs painting or layout changed |
| `hitTest()` | `hit_test()` | Pointer event received |
| `markNeedsLayout()` | `mark_needs_layout()` | Layout invalidated |
| `markNeedsPaint()` | `mark_needs_paint()` | Visual properties changed |
| `setupParentData()` | `setup_parent_data()` | Child added |
| `visitChildren()` | `visit_children()` | Tree traversal needed |

### sizedByParent Optimization

**Flutter:**
```dart
class RenderConstrainedBox extends RenderProxyBox {
  @override
  bool get sizedByParent => true;

  @override
  void performResize() {
    size = constraints.constrain(Size.zero);
  }

  @override
  void performLayout() {
    // Only position children, size already set
    if (child != null) {
      child.layout(constraints, parentUsesSize: false);
    }
  }
}
```

**FLUI:**
```rust
impl RenderObject for RenderConstrainedBox {
    fn sized_by_parent(&self) -> bool {
        true
    }

    fn perform_resize(&mut self, constraints: &dyn Any) -> RenderResult<()> {
        let box_constraints = constraints.downcast_ref::<BoxConstraints>()?;
        self.cached_size = box_constraints.constrain(Size::ZERO);
        Ok(())
    }
}

impl RenderBox<Single> for RenderConstrainedBox {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Size already set by perform_resize, just layout child
        ctx.layout_single_child()?;
        Ok(self.cached_size)
    }
}
```

## Layout Constraints

### BoxConstraints

**Flutter API:**
```dart
BoxConstraints(
  minWidth: 0,
  maxWidth: double.infinity,
  minHeight: 0,
  maxHeight: double.infinity,
)

constraints.isTight        // min == max
constraints.isNormalized   // min <= max
constraints.hasBoundedWidth
constraints.biggest()
constraints.smallest()
constraints.constrain(size)
```

**FLUI API (identical):**
```rust
BoxConstraints::new(
    min_width: 0.0,
    max_width: f32::INFINITY,
    min_height: 0.0,
    max_height: f32::INFINITY,
)

constraints.is_tight()        // min == max
constraints.is_normalized()   // min <= max
constraints.has_bounded_width()
constraints.biggest()
constraints.smallest()
constraints.constrain(size)
```

### Common Constraint Operations

| Operation | Flutter | FLUI |
|-----------|---------|------|
| Tight constraints | `BoxConstraints.tight(size)` | `BoxConstraints::tight(size)` |
| Loose constraints | `BoxConstraints.loose(size)` | `BoxConstraints::loose(size)` |
| Expand to fill | `BoxConstraints.expand()` | `BoxConstraints::expand()` |
| Deflate (padding) | `constraints.deflate(edges)` | `constraints.deflate(&edges)` |
| Tighten width | `constraints.tighten(width: w)` | `constraints.tighten_width(w)` |
| Loosen | `constraints.loosen()` | `constraints.loosen()` |

## Dirty Tracking

### Dirty Flags

**Flutter's dirty system:**
```dart
// RenderObject fields
bool _needsLayout = false;
bool _needsPaint = false;
bool _needsCompositingBitsUpdate = false;

void markNeedsLayout() {
  if (_needsLayout) return;  // Already dirty
  _needsLayout = true;

  if (_relayoutBoundary != this) {
    // Propagate to parent
    parent?.markNeedsLayout();
  } else {
    // Register with pipeline owner
    owner._nodesNeedingLayout.add(this);
  }
}
```

**FLUI's atomic dirty tracking:**
```rust
// AtomicRenderFlags - lock-free bitflags
const NEEDS_LAYOUT: u32 = 1 << 0;
const NEEDS_PAINT: u32 = 1 << 1;
const NEEDS_COMPOSITING: u32 = 1 << 2;
const IS_RELAYOUT_BOUNDARY: u32 = 1 << 3;
const IS_REPAINT_BOUNDARY: u32 = 1 << 4;

impl AtomicRenderFlags {
    fn mark_needs_layout(&self) {
        if self.needs_layout() {
            return;  // Already dirty
        }
        self.set(RenderFlags::NEEDS_LAYOUT);

        if !self.is_relayout_boundary() {
            // Propagate to parent (via tree)
            parent.mark_needs_layout();
        } else {
            // Register with pipeline owner
            owner.register_dirty_layout(element_id);
        }
    }
}
```

### Propagation Rules

**markNeedsLayout():**
1. If already dirty → return early
2. Mark self dirty
3. If NOT relayout boundary → propagate to parent recursively
4. If IS relayout boundary → register with pipeline owner

**markParentNeedsLayout():**
1. Mark self dirty
2. ALWAYS propagate to parent (even through boundaries)
3. Used when intrinsic size changes

**markNeedsPaint():**
1. If already dirty → return early
2. Mark self dirty
3. If NOT repaint boundary → propagate to parent
4. If IS repaint boundary → register with pipeline owner

## Performance Boundaries

### Relayout Boundaries

**When to use:**
- Size doesn't depend on parent constraints
- Changes don't affect parent's layout

**Flutter example:**
```dart
class RenderSizedBox extends RenderProxyBox {
  @override
  void performLayout() {
    // Relayout boundary if hasChild
    if (child != null) {
      // Child's changes don't affect our size
      child.layout(constraints, parentUsesSize: false);
      size = child.size;
    } else {
      size = constraints.smallest();
    }
  }
}
```

**FLUI equivalent:**
```rust
impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Optional>) -> RenderResult<Size> {
        if let Some(child_id) = ctx.children.single() {
            // Set as relayout boundary
            ctx.tree().set_relayout_boundary(child_id, true);

            let size = ctx.layout_child(*child_id, ctx.constraints)?;
            Ok(size)
        } else {
            Ok(ctx.constraints.smallest())
        }
    }
}
```

### Repaint Boundaries

**When to use:**
- Creates compositing layer
- Isolates paint changes from parent
- Enables layer caching

**Flutter example:**
```dart
class RenderRepaintBoundary extends RenderProxyBox {
  @override
  bool get isRepaintBoundary => true;

  @override
  bool get alwaysNeedsCompositing => child != null;
}
```

**FLUI equivalent:**
```rust
impl RenderBox<Single> for RenderRepaintBoundary {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Mark as repaint boundary
        ctx.tree().set_repaint_boundary(ctx.element_id(), true);
        ctx.layout_single_child()
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // Paint to layer (cached)
        if let Some(layer) = &self.layer {
            ctx.push_layer(layer);
            ctx.paint_single_child(Offset::ZERO)?;
            ctx.pop_layer();
        }
    }
}
```

## Common Patterns

### Pattern 1: Simple Wrapper (Pass-through)

**Use case:** Transform, Opacity, Padding

```rust
impl RenderBox<Single> for RenderWrapper {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Pass constraints through (possibly modified)
        let child_constraints = self.modify_constraints(&ctx.constraints);
        let child_size = ctx.layout_single_child_with(|_| child_constraints)?;

        // Compute own size from child
        Ok(self.compute_size(child_size, &ctx.constraints))
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // Apply effect, paint child, restore
        ctx.canvas_mut().save();
        self.apply_effect(ctx.canvas_mut());
        ctx.paint_single_child(ctx.offset)?;
        ctx.canvas_mut().restore();
    }
}
```

### Pattern 2: Multi-Child Layout (Flex, Stack)

```rust
impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
        let mut total_main = 0.0;
        let mut max_cross = 0.0;

        // Layout each child
        for child_id in ctx.children.element_ids() {
            let child_constraints = self.child_constraints(&ctx.constraints, ...);
            let child_size = ctx.layout_child(child_id, child_constraints)?;

            // Position child
            let offset = self.compute_offset(total_main, child_size);
            ctx.set_child_offset(child_id, offset);

            // Accumulate
            total_main += child_size.main_axis(self.direction);
            max_cross = max_cross.max(child_size.cross_axis(self.direction));
        }

        Ok(self.compute_size(total_main, max_cross, &ctx.constraints))
    }
}
```

### Pattern 3: Intrinsic Sizing

**Flutter:**
```dart
class RenderParagraph extends RenderBox {
  @override
  double computeMinIntrinsicWidth(double height) {
    return _textPainter.minIntrinsicWidth;
  }

  @override
  double computeMaxIntrinsicWidth(double height) {
    return _textPainter.maxIntrinsicWidth;
  }
}
```

**FLUI:**
```rust
impl RenderBox<Leaf> for RenderParagraph {
    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        Some(self.text_painter.min_intrinsic_width())
    }

    fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
        let intrinsic = self.measure_text(&ctx.constraints);
        Ok(ctx.constraints.constrain(intrinsic))
    }
}
```

### Pattern 4: Custom ParentData

**Flutter:**
```dart
class FlexParentData extends ContainerBoxParentData<RenderBox> {
  int flex = 0;
  FlexFit fit = FlexFit.tight;
}

class RenderFlex extends RenderBox {
  @override
  void setupParentData(RenderObject child) {
    if (child.parentData is! FlexParentData) {
      child.parentData = FlexParentData();
    }
  }
}
```

**FLUI:**
```rust
#[derive(Debug, Clone)]
struct FlexParentData {
    flex: i32,
    fit: FlexFit,
    offset: Offset,
}

impl ParentData for FlexParentData {}

impl ParentDataWithOffset for FlexParentData {
    fn offset(&self) -> Offset { self.offset }
    fn set_offset(&mut self, offset: Offset) { self.offset = offset; }
}

impl RenderBox<Variable> for RenderFlex {
    fn setup_parent_data(&self, child_id: ElementId, tree: &mut dyn LayoutTree) {
        tree.set_parent_data(child_id, Box::new(FlexParentData {
            flex: 0,
            fit: FlexFit::Tight,
            offset: Offset::ZERO,
        }));
    }
}
```

## Migration Guide

### From Flutter to FLUI

| Flutter Pattern | FLUI Equivalent | Notes |
|-----------------|-----------------|-------|
| `RenderProxyBox` | `impl RenderBox<Single>` | Single child wrapper |
| `RenderBox with ContainerRenderObjectMixin` | `impl RenderBox<Variable>` | Multi-child layout |
| `RenderSliver` | `impl RenderSliver<A>` | Scrollable content |
| `parentUsesSize: true` | (default) | Parent reads child size |
| `parentUsesSize: false` | Relayout boundary | Optimization |
| `context.paintChild()` | `ctx.paint_child()` | Paint delegation |
| `visitChildren((child) {})` | `visit_children(\|child_id\| {})` | Tree traversal |

### Key Differences

1. **Arity System**: FLUI uses compile-time arity checking (`Leaf`, `Single`, `Variable`) instead of runtime child count
2. **Atomic Flags**: FLUI uses lock-free atomics for dirty tracking (faster)
3. **Explicit Protocols**: FLUI separates Box and Sliver protocols at type level
4. **Error Handling**: FLUI uses `Result` types instead of assertions
5. **Ownership**: FLUI uses Rust ownership model (no garbage collection)

### Best Practices

**DO:**
- ✅ Always satisfy constraints: `Ok(ctx.constraints.constrain(size))`
- ✅ Use relayout/repaint boundaries for optimization
- ✅ Cache computed values when possible
- ✅ Use `sized_by_parent` when size doesn't depend on children
- ✅ Clear dirty flags after processing
- ✅ Use atomic flags for hot path checks

**DON'T:**
- ❌ Call layout during paint
- ❌ Return unconstrained sizes
- ❌ Access children before laying them out
- ❌ Forget to position children (set offsets)
- ❌ Use `println!` for logging (use `tracing!`)
- ❌ Clone large data structures in hot paths

## Performance Tips

### 1. Use Boundaries Wisely

```rust
// Good: Isolate animations
flags.set_repaint_boundary(true);

// Good: Stop layout propagation
if !size_depends_on_parent {
    flags.set_relayout_boundary(true);
}
```

### 2. Cache Intrinsic Sizes

```rust
struct CachedLayout {
    intrinsic_width: Option<f32>,
    intrinsic_height: Option<f32>,
}

fn layout(&mut self, ctx: BoxLayoutContext) -> RenderResult<Size> {
    if self.cache.intrinsic_width.is_none() {
        self.cache.intrinsic_width = Some(self.compute_width());
    }
    // Use cached value
}
```

### 3. Batch Child Layouts

```rust
// Bad: Layout children one by one
for child in children {
    layout_child(child);
}

// Good: Collect children, layout in batch
let children: Vec<_> = ctx.children.collect();
ctx.layout_children_parallel(&children)?;
```

### 4. Use Lock-Free Operations

```rust
// Good: Atomic flag checks (1-5ns)
if flags.needs_layout() { ... }

// Bad: RwLock (50-100ns)
if *rwlock.read().unwrap() { ... }
```

## References

- [Flutter RenderObject API](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- [Flutter Box Protocol](https://api.flutter.dev/flutter/rendering/RenderBox-class.html)
- [Flutter Sliver Protocol](https://api.flutter.dev/flutter/rendering/RenderSliver-class.html)
- [FLUI Architecture Documentation](../../docs/arch/RENDERING_ARCHITECTURE.md)
- [FLUI Core Patterns](../../docs/arch/PATTERNS.md)

---

**Last Updated:** December 2025
**FLUI Version:** 0.1.x
**Flutter Compatibility:** 3.x layout protocol
