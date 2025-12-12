# Trait Hierarchy

**Complete trait inheritance structure for render objects**

---

## Overview

FLUI uses trait inheritance to model render object capabilities. The trait system is organized into three main families: RenderObject (base), RenderBox (2D layout), and RenderSliver (scrollable content).

---

## Complete Trait Tree

```
RenderObject (trait)
    ├── hitTest()
    ├── attach() / detach()
    ├── markNeedsLayout()
    ├── markNeedsPaint()
    └── parent_data<T>()
    │
    ├──── RenderBox (trait)
    │     │   ├── perform_layout(BoxConstraints) -> Size
    │     │   ├── size() -> Size
    │     │   ├── paint(PaintingContext, Offset)
    │     │   ├── hit_test(BoxHitTestResult, Offset) -> bool
    │     │   └── compute_intrinsics(...)
    │     │
    │     ├──── SingleChildRenderBox (trait)
    │     │     │   └── child() -> Option<&dyn RenderBox>
    │     │     │
    │     │     ├──── RenderProxyBox (trait)
    │     │     │     │   (child size = parent size)
    │     │     │     │
    │     │     │     ├──── HitTestProxy (trait)
    │     │     │     │     └── behavior() -> HitTestBehavior
    │     │     │     │
    │     │     │     ├──── ClipProxy (trait)
    │     │     │     │     └── clip<T>(Size) -> T
    │     │     │     │
    │     │     │     └──── PhysicalModelProxy (trait)
    │     │     │           ├── elevation() -> f32
    │     │     │           ├── color() -> Color
    │     │     │           └── shadow_color() -> Color
    │     │     │
    │     │     └──── RenderShiftedBox (trait)
    │     │           │   (custom child positioning)
    │     │           │
    │     │           └──── RenderAligningShiftedBox (trait)
    │     │                 ├── alignment() -> Alignment
    │     │                 └── resolve_alignment(...)
    │     │
    │     └──── MultiChildRenderBox (trait)
    │           └── children() -> Iterator<Item = &dyn RenderBox>
    │
    └──── RenderSliver (trait)
          │   ├── perform_layout(SliverConstraints) -> SliverGeometry
          │   ├── geometry() -> SliverGeometry
          │   ├── paint(PaintingContext, Offset)
          │   └── hit_test(SliverHitTestResult, ...) -> bool
          │
          ├──── RenderProxySliver (trait)
          │     └── child() -> Option<&dyn RenderSliver>
          │
          ├──── RenderSliverSingleBoxAdapter (trait)
          │     └── child() -> Option<&dyn RenderBox>
          │
          ├──── RenderSliverMultiBoxAdaptor (trait)
          │     ├── children() -> Iterator<&dyn RenderBox>
          │     └── child_manager() -> &dyn ChildManager
          │
          └──── RenderSliverPersistentHeader (trait)
                ├── child() -> Option<&dyn RenderBox>
                ├── min_extent() -> f32
                └── max_extent() -> f32
```

---

## Trait Definitions

### Base: RenderObject

All render objects implement this trait.

```rust
#[delegatable_trait]
pub trait RenderObject: Debug + Send + Sync + 'static {
    // Tree structure
    fn parent(&self) -> Option<&dyn RenderObject>;
    fn depth(&self) -> usize;
    fn owner(&self) -> Option<&PipelineOwner>;
    
    // Lifecycle
    fn attach(&mut self, owner: PipelineOwner);
    fn detach(&mut self);
    
    // Dirty marking
    fn markNeedsLayout(&mut self);
    fn markNeedsPaint(&mut self);
    fn markNeedsCompositingBitsUpdate(&mut self);
    
    // Parent data access
    fn parent_data<T: ParentData>(&self) -> &T;
    fn parent_data_mut<T: ParentData>(&mut self) -> &mut T;
    
    // Configuration
    fn sized_by_parent(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
    fn always_needs_compositing(&self) -> bool { false }
    
    // Type inspection
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

---

### Box Protocol Traits

#### RenderBox

2D cartesian layout protocol.

```rust
#[delegatable_trait]
pub trait RenderBox: RenderObject {
    // Layout (required)
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;
    
    // Size access
    fn size(&self) -> Size;
    
    // Paint (required)
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    // Hit testing
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if self.size().contains(position) {
            self.hit_test_children(result, position) || self.hit_test_self(position)
        } else {
            false
        }
    }
    
    fn hit_test_self(&self, position: Offset) -> bool { false }
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool { false }
    
    // Intrinsic dimensions
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { 0.0 }
    
    // Baseline
    fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> { None }
    
    // Dry layout (no side effects)
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        Size::ZERO
    }
}
```

#### SingleChildRenderBox

Box with zero or one child.

```rust
#[delegatable_trait]
pub trait SingleChildRenderBox: RenderBox {
    fn child(&self) -> Option<&dyn RenderBox>;
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;
}
```

#### RenderProxyBox

Single child where parent size equals child size.

```rust
#[delegatable_trait]
pub trait RenderProxyBox: SingleChildRenderBox {
    // Default implementations pass through to child
    
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(|| constraints.smallest())
    }
    
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.child()
            .map(|c| c.hit_test(result, position))
            .unwrap_or(false)
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
    
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child().map(|c| c.compute_min_intrinsic_width(height)).unwrap_or(0.0)
    }
    
    // ... other intrinsic methods
}
```

#### HitTestProxy

Customizes hit test behavior for proxy objects.

```rust
#[delegatable_trait]
pub trait HitTestProxy: RenderProxyBox {
    fn behavior(&self) -> HitTestBehavior;
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        match self.behavior() {
            HitTestBehavior::Opaque => {
                self.hit_test_children(result, position);
                true  // Always hit
            }
            HitTestBehavior::Translucent => {
                let hit = self.hit_test_children(result, position);
                hit || self.size().contains(position)
            }
            HitTestBehavior::DeferToChild => {
                self.hit_test_children(result, position)
            }
        }
    }
}
```

#### ClipProxy

Provides clipping for proxy objects.

```rust
#[delegatable_trait]
pub trait ClipProxy<T: Clone>: RenderProxyBox {
    fn clip(&self, size: Size) -> T;
    fn clip_behavior(&self) -> Clip;
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.clip_behavior() != Clip::None {
            let clip_path = self.clip(self.size());
            context.push_clip_path(
                self.always_needs_compositing(),
                offset,
                Rect::from_size(self.size()),
                clip_path,
                |ctx| {
                    if let Some(child) = self.child() {
                        ctx.paint_child(child, offset);
                    }
                },
                self.clip_behavior(),
            );
        } else if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
}
```

#### PhysicalModelProxy

Adds elevation, shadows, and shape to proxy objects.

```rust
#[delegatable_trait]
pub trait PhysicalModelProxy: ClipProxy<RRect> {
    fn elevation(&self) -> f32;
    fn color(&self) -> Color;
    fn shadow_color(&self) -> Color;
    
    fn always_needs_compositing(&self) -> bool {
        self.elevation() > 0.0
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.elevation() > 0.0 {
            // Draw shadow
            let shadow_path = Path::from_rrect(self.clip(self.size()));
            context.canvas().draw_shadow(
                shadow_path,
                self.shadow_color(),
                self.elevation(),
                self.color().alpha() != 255,
            );
        }
        
        // Paint child with clipping
        ClipProxy::paint(self, context, offset);
        
        // Draw shape
        if self.color().alpha() > 0 {
            let paint = Paint::new()
                .color(self.color())
                .style(PaintStyle::Fill);
            context.canvas().draw_rrect(self.clip(self.size()), paint);
        }
    }
}
```

#### RenderShiftedBox

Single child with custom positioning.

```rust
#[delegatable_trait]
pub trait RenderShiftedBox: SingleChildRenderBox {
    fn child_offset(&self) -> Offset;
    
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let child_position = position - self.child_offset();
        self.child()
            .map(|c| c.hit_test(result, child_position))
            .unwrap_or(false)
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset + self.child_offset());
        }
    }
}
```

#### RenderAligningShiftedBox

Shifted box with alignment support.

```rust
#[delegatable_trait]
pub trait RenderAligningShiftedBox: RenderShiftedBox {
    fn alignment(&self) -> Alignment;
    fn width_factor(&self) -> Option<f32>;
    fn height_factor(&self) -> Option<f32>;
    
    fn resolve_alignment(&self, child_size: Size, parent_size: Size) -> Offset {
        let x = (parent_size.width - child_size.width) * self.alignment().x;
        let y = (parent_size.height - child_size.height) * self.alignment().y;
        Offset::new(x, y)
    }
}
```

#### MultiChildRenderBox

Box with multiple children.

```rust
#[delegatable_trait]
pub trait MultiChildRenderBox: RenderBox {
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox>;
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox>;
    fn child_count(&self) -> usize;
}
```

---

### Sliver Protocol Traits

#### RenderSliver

Scrollable content protocol.

```rust
#[delegatable_trait]
pub trait RenderSliver: RenderObject {
    // Layout (required)
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;
    
    // Geometry access
    fn geometry(&self) -> SliverGeometry;
    
    // Paint (required)
    fn paint(&self, context: &mut PaintingContext, offset: Offset);
    
    // Hit testing
    fn hit_test(&self, result: &mut SliverHitTestResult, 
                main_axis_position: f32, cross_axis_position: f32) -> bool {
        if main_axis_position >= 0.0 && 
           main_axis_position < self.geometry().hit_test_extent() &&
           cross_axis_position >= 0.0 &&
           cross_axis_position < self.constraints().cross_axis_extent {
            self.hit_test_children(result, main_axis_position, cross_axis_position) ||
            self.hit_test_self(main_axis_position, cross_axis_position)
        } else {
            false
        }
    }
    
    fn hit_test_self(&self, main: f32, cross: f32) -> bool { false }
    fn hit_test_children(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { false }
    
    // Constraints access
    fn constraints(&self) -> &SliverConstraints;
    
    // Child paint transform
    fn apply_paint_transform_for_box_child(&self, child: &dyn RenderBox, transform: &mut Matrix4);
}
```

#### RenderProxySliver

Sliver with single sliver child.

```rust
#[delegatable_trait]
pub trait RenderProxySliver: RenderSliver {
    fn child(&self) -> Option<&dyn RenderSliver>;
    fn child_mut(&mut self) -> Option<&mut dyn RenderSliver>;
    
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(SliverGeometry::zero)
    }
    
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }
}
```

#### RenderSliverSingleBoxAdapter

Sliver wrapping a single box child.

```rust
#[delegatable_trait]
pub trait RenderSliverSingleBoxAdapter: RenderSliver {
    fn child(&self) -> Option<&dyn RenderBox>;
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;
}
```

#### RenderSliverMultiBoxAdaptor

Sliver with multiple box children (lists, grids).

```rust
#[delegatable_trait]
pub trait RenderSliverMultiBoxAdaptor: RenderSliver {
    fn children(&self) -> impl Iterator<Item = &dyn RenderBox>;
    fn children_mut(&mut self) -> impl Iterator<Item = &mut dyn RenderBox>;
    fn child_manager(&self) -> &dyn ChildManager;
}
```

#### RenderSliverPersistentHeader

Sliver with persistent header (pins, floats, or scrolls).

```rust
#[delegatable_trait]
pub trait RenderSliverPersistentHeader: RenderSliver {
    fn child(&self) -> Option<&dyn RenderBox>;
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;
    
    fn min_extent(&self) -> f32;
    fn max_extent(&self) -> f32;
}
```

---

## Blanket Implementations

Automatic trait inheritance using blanket impls:

```rust
// ProxyBox → SingleChildRenderBox
impl<T: RenderProxyBox> SingleChildRenderBox for T {
    fn child(&self) -> Option<&dyn RenderBox> {
        RenderProxyBox::child(self)
    }
    
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        RenderProxyBox::child_mut(self)
    }
}

// SingleChildRenderBox → RenderBox (delegation)
impl<T: SingleChildRenderBox> RenderBox for T {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        SingleChildRenderBox::perform_layout(self, constraints)
    }
    
    // ... delegate all RenderBox methods
}

// RenderBox → RenderObject (delegation)
impl<T: RenderBox> RenderObject for T {
    fn mark_needs_layout(&mut self) {
        RenderBox::mark_needs_layout(self)
    }
    
    // ... delegate all RenderObject methods
}
```

**Result:** Implement ONE trait, get ALL ancestors automatically.

---

## Trait Count by Protocol

| Protocol | Trait Count | Traits |
|----------|-------------|--------|
| **Base** | 1 | RenderObject |
| **Box** | 13 | RenderBox, SingleChildRenderBox, RenderProxyBox, HitTestProxy, ClipProxy, PhysicalModelProxy, RenderShiftedBox, RenderAligningShiftedBox, MultiChildRenderBox, TableRow, Flow, CustomLayout, CustomPaint |
| **Sliver** | 11 | RenderSliver, RenderProxySliver, RenderSliverSingleBoxAdapter, RenderSliverMultiBoxAdaptor, RenderSliverPersistentHeader, SliverFloating, SliverPinned, SliverScrolling, SliverMultiBoxGroup, SliverCrossAxisGroup, SliverMainAxisGroup |
| **Total** | 24 | |

---

## Implementation Requirements

### For RenderObject

All types must implement:
- `Debug + Send + Sync + 'static`
- Tree structure (parent, depth, owner)
- Dirty marking (layout, paint, compositing)
- Parent data access

### For RenderBox

Additional requirements:
- `perform_layout(BoxConstraints) -> Size`
- `size() -> Size`
- `paint(PaintingContext, Offset)`
- Optional: hit testing, intrinsics, baseline

### For RenderSliver

Additional requirements:
- `perform_layout(SliverConstraints) -> SliverGeometry`
- `geometry() -> SliverGeometry`
- `paint(PaintingContext, Offset)`
- Optional: hit testing

---

## File Organization

```
flui-rendering/src/traits/
├── mod.rs                        # Re-exports
├── render_object.rs              # RenderObject trait
├── box/
│   ├── mod.rs
│   ├── render_box.rs             # RenderBox
│   ├── single_child.rs           # SingleChildRenderBox
│   ├── proxy_box.rs              # RenderProxyBox
│   ├── hit_test_proxy.rs         # HitTestProxy
│   ├── clip_proxy.rs             # ClipProxy
│   ├── physical_model_proxy.rs   # PhysicalModelProxy
│   ├── shifted_box.rs            # RenderShiftedBox
│   ├── aligning_shifted_box.rs   # RenderAligningShiftedBox
│   └── multi_child.rs            # MultiChildRenderBox
└── sliver/
    ├── mod.rs
    ├── render_sliver.rs          # RenderSliver
    ├── proxy_sliver.rs           # RenderProxySliver
    ├── single_box_adapter.rs     # RenderSliverSingleBoxAdapter
    ├── multi_box_adaptor.rs      # RenderSliverMultiBoxAdaptor
    └── persistent_header.rs      # RenderSliverPersistentHeader
```

---

## Next Steps

- [[Protocol]] - Foundation for trait system
- [[Object Catalog]] - Concrete implementations
- [[Blanket Implementations]] - Automatic inheritance details

---

**See Also:**
- [[Containers]] - How traits use containers
- [[Implementation Guide]] - Step-by-step trait implementation
