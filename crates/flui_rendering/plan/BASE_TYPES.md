# Base Types — Complete Architecture

Полная система base types для flui_rendering с auto-implementation.

## Overview

### Child Storage (3 generic + 6 aliases)

| Generic | Box Alias | Sliver Alias | Flutter |
|---------|-----------|--------------|---------|
| `Child<P>` | `BoxChild` | `SliverChild` | `RenderObjectWithChildMixin` |
| `Children<P, PD>` | `BoxChildren<PD>` | `SliverChildren<PD>` | `ContainerRenderObjectMixin` |
| `Slots<P, S>` | `BoxSlots<S>` | `SliverSlots<S>` | `SlottedContainerRenderObjectMixin` |

### Base Types (5 generic + 9 aliases + 9 mixins)

| Generic | Box Alias | Box Mixin | Sliver Alias | Sliver Mixin |
|---------|-----------|-----------|--------------|--------------|
| `Proxy<T, P>` | `ProxyBox<T>` | `RenderProxyBoxMixin` | `ProxySliver<T>` | `RenderProxySliverMixin` |
| `Shifted<T, P>` | `ShiftedBox<T>` | `RenderShiftedBoxMixin` | `ShiftedSliver<T>` | `RenderShiftedSliverMixin` |
| `AligningShifted<T, P>` | `AligningShiftedBox<T>` | `RenderAligningShiftedBoxMixin` | — | — |
| `Container<T, P, PD>` | `ContainerBox<T, PD>` | `RenderContainerBoxMixin` | `ContainerSliver<T, PD>` | `RenderContainerSliverMixin` |
| `Leaf<T, P>` | `LeafBox<T>` | `RenderLeafBoxMixin` | `LeafSliver<T>` | `RenderLeafSliverMixin` |

### All Mixins (collected in one module)

```rust
// crates/flui_rendering/src/mixins/mod.rs

// Box Mixins
pub use super::traits::RenderProxyBoxMixin;
pub use super::traits::RenderShiftedBoxMixin;
pub use super::traits::RenderAligningShiftedBoxMixin;
pub use super::traits::RenderContainerBoxMixin;
pub use super::traits::RenderLeafBoxMixin;

// Sliver Mixins
pub use super::traits::RenderProxySliverMixin;
pub use super::traits::RenderShiftedSliverMixin;
pub use super::traits::RenderContainerSliverMixin;
pub use super::traits::RenderLeafSliverMixin;
```

---

## Part 1: Child Storage

### Generic Types

```rust
/// Single child storage (optional).
/// Flutter: RenderObjectWithChildMixin
pub struct Child<P: Protocol> {
    inner: Option<RenderHandle<P, Mounted>>,
}

/// Multiple children storage with parent data.
/// Flutter: ContainerRenderObjectMixin
pub struct Children<P: Protocol, PD: ParentData = ()> {
    items: Vec<(RenderHandle<P, Mounted>, ContainerParentData<PD>)>,
}

/// Named slots storage.
/// Flutter: SlottedContainerRenderObjectMixin
pub struct Slots<P: Protocol, S: SlotKey> {
    items: HashMap<S, (RenderHandle<P, Mounted>, Offset)>,
}
```

### Box Aliases

```rust
/// Single Box child.
pub type BoxChild = Child<BoxProtocol>;

/// Multiple Box children with parent data.
pub type BoxChildren<PD = ()> = Children<BoxProtocol, PD>;

/// Named Box slots.
pub type BoxSlots<S> = Slots<BoxProtocol, S>;
```

### Sliver Aliases

```rust
/// Single Sliver child.
pub type SliverChild = Child<SliverProtocol>;

/// Multiple Sliver children with parent data.
pub type SliverChildren<PD = ()> = Children<SliverProtocol, PD>;

/// Named Sliver slots.
pub type SliverSlots<S> = Slots<SliverProtocol, S>;
```

---

## Part 2: ProxyData Trait

```rust
/// Trait bounds for custom data in base types.
pub trait ProxyData: Default + Clone + Debug + 'static {}

// Auto-impl for all matching types
impl<T: Default + Clone + Debug + 'static> ProxyData for T {}

// Optional serde support
#[cfg(feature = "serde")]
pub trait ProxyData: Default + Clone + Debug + Serialize + DeserializeOwned + 'static {}
```

---

## Part 3: Proxy — Delegate All

### Generic Proxy

```rust
pub struct ProxyBase<P: Protocol> {
    pub child: Child<P>,
    pub geometry: P::Geometry,
}

pub struct ProxyInner<T: ProxyData, P: Protocol> {
    base: ProxyBase<P>,
    data: T,
}

pub struct Proxy<T: ProxyData, P: Protocol = BoxProtocol> {
    inner: ProxyInner<T, P>,
}

// Deref chain: Proxy → ProxyInner → T
impl<T: ProxyData, P: Protocol> Deref for Proxy<T, P> {
    type Target = ProxyInner<T, P>;
}

impl<T: ProxyData, P: Protocol> Deref for ProxyInner<T, P> {
    type Target = T;
}
```

### Box: ProxyBox + RenderProxyBoxMixin

```rust
/// Box proxy — delegates everything to child.
pub type ProxyBox<T> = Proxy<T, BoxProtocol>;

/// Mixin for Box proxy render objects (delegates all to child).
pub trait RenderProxyBoxMixin: RenderObject + ProxyBehavior<BoxProtocol> {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
    
    // All have defaults that delegate to child
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { ... }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { ... }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { ... }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { ... }
    fn compute_dry_layout(&self, constraints: &BoxConstraints) -> Size { ... }
    fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> { ... }
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size { ... }
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool { ... }
    fn always_needs_compositing(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
}

// Auto-impl
impl<T: ProxyData> RenderProxyBoxMixin for ProxyBox<T> { ... }

// Blanket impl
impl<T: RenderProxyBoxMixin> RenderProtocol<BoxProtocol> for T { ... }
```

### Sliver: ProxySliver + RenderProxySliverMixin

```rust
/// Sliver proxy — delegates everything to child.
pub type ProxySliver<T> = Proxy<T, SliverProtocol>;

/// Mixin for Sliver proxy render objects (delegates all to child).
pub trait RenderProxySliverMixin: RenderObject + ProxyBehavior<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    // All have defaults that delegate to child
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry { ... }
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { ... }
    fn always_needs_compositing(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
}

// Auto-impl
impl<T: ProxyData> RenderProxySliverMixin for ProxySliver<T> { ... }

// Blanket impl
impl<T: RenderProxySliverMixin> RenderProtocol<SliverProtocol> for T { ... }
```

### Examples

```rust
// RenderOpacity
#[derive(Default, Clone, Debug)]
pub struct OpacityData { pub alpha: f32 }
pub type RenderOpacity = ProxyBox<OpacityData>;

impl RenderProxyBoxMixin for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { /* override */ }
    fn always_needs_compositing(&self) -> bool { self.alpha > 0.0 && self.alpha < 1.0 }
}

// RenderIgnorePointer
#[derive(Default, Clone, Debug)]
pub struct IgnorePointerData { pub ignoring: bool }
pub type RenderIgnorePointer = ProxyBox<IgnorePointerData>;

impl RenderProxyBoxMixin for RenderIgnorePointer {
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        !self.ignoring && self.proxy_hit_test(result, position)
    }
}
```

---

## Part 4: Shifted — Single Child + Offset

### Generic Shifted

```rust
pub struct ShiftedBase<P: Protocol> {
    pub child: Child<P>,
    pub child_offset: Offset,
    pub geometry: P::Geometry,
}

pub struct ShiftedInner<T: ProxyData, P: Protocol> {
    base: ShiftedBase<P>,
    data: T,
}

pub struct Shifted<T: ProxyData, P: Protocol = BoxProtocol> {
    inner: ShiftedInner<T, P>,
}

// Deref chain: Shifted → ShiftedInner → T
```

### Box: ShiftedBox + RenderShiftedBoxMixin

```rust
/// Box with single child at offset.
pub type ShiftedBox<T> = Shifted<T, BoxProtocol>;

/// Mixin for shifted Box render objects (single child + offset).
pub trait RenderShiftedBoxMixin: RenderObject + HasChild<BoxProtocol> {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
    fn child_offset(&self) -> Offset;
    fn set_child_offset(&mut self, offset: Offset);
    
    // Layout MUST be overridden
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    
    // Default: paint child at offset
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child().get() {
            child.paint(ctx, offset + self.child_offset());
        }
    }
    
    // Default: hit test with offset transform
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child().get() {
            child.hit_test(result, position - self.child_offset())
        } else {
            false
        }
    }
    
    // Intrinsics delegate to child
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 { ... }
    fn compute_max_intrinsic_width(&self, height: f32) -> f32 { ... }
    fn compute_min_intrinsic_height(&self, width: f32) -> f32 { ... }
    fn compute_max_intrinsic_height(&self, width: f32) -> f32 { ... }
}

// Auto-impl
impl<T: ProxyData> RenderShiftedBoxMixin for ShiftedBox<T> { ... }

// Blanket impl
impl<T: RenderShiftedBoxMixin> RenderProtocol<BoxProtocol> for T { ... }
```

### Sliver: ShiftedSliver + RenderShiftedSliverMixin

```rust
/// Sliver with single child at offset.
pub type ShiftedSliver<T> = Shifted<T, SliverProtocol>;

/// Mixin for shifted Sliver render objects (single child + offset).
pub trait RenderShiftedSliverMixin: RenderObject + HasChild<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    fn child_offset(&self) -> Offset;
    fn set_child_offset(&mut self, offset: Offset);
    
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { ... }
}

// Auto-impl
impl<T: ProxyData> RenderShiftedSliverMixin for ShiftedSliver<T> { ... }

// Blanket impl
impl<T: RenderShiftedSliverMixin> RenderProtocol<SliverProtocol> for T { ... }
```

### Examples

```rust
// RenderPadding
#[derive(Default, Clone, Debug)]
pub struct PaddingData { pub padding: EdgeInsets }
pub type RenderPadding = ShiftedBox<PaddingData>;

impl RenderShiftedBoxMixin for RenderPadding {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child_mut().get_mut() {
            let inner = constraints.deflate(&self.padding);
            let child_size = child.perform_layout(&inner);
            self.set_child_offset(Offset::new(self.padding.left, self.padding.top));
            let size = child_size + self.padding.size();
            self.set_size(constraints.constrain(size));
        } else {
            self.set_size(constraints.constrain(self.padding.size()));
        }
        self.size()
    }
}

// RenderTransform
#[derive(Clone, Debug)]
pub struct TransformData { pub transform: Matrix4 }
pub type RenderTransform = ShiftedBox<TransformData>;
```

---

## Part 5: AligningShifted — + Alignment

### Generic AligningShifted

```rust
pub struct AligningShiftedBase<P: Protocol> {
    pub shifted: ShiftedBase<P>,
    pub alignment: Alignment,
    pub text_direction: Option<TextDirection>,
}

pub struct AligningShiftedInner<T: ProxyData, P: Protocol> {
    base: AligningShiftedBase<P>,
    data: T,
}

pub struct AligningShifted<T: ProxyData, P: Protocol = BoxProtocol> {
    inner: AligningShiftedInner<T, P>,
}

// Deref chain: AligningShifted → AligningShiftedInner → T
```

### Box: AligningShiftedBox + RenderAligningShiftedBoxMixin

```rust
/// Box with single child + alignment.
pub type AligningShiftedBox<T> = AligningShifted<T, BoxProtocol>;

/// Mixin for aligning shifted Box render objects (child + alignment + RTL).
pub trait RenderAligningShiftedBoxMixin: RenderShiftedBoxMixin {
    fn alignment(&self) -> Alignment;
    fn set_alignment(&mut self, alignment: Alignment);
    fn text_direction(&self) -> Option<TextDirection>;
    fn set_text_direction(&mut self, direction: Option<TextDirection>);
    
    /// Resolve alignment (handles RTL).
    fn resolved_alignment(&self) -> Alignment {
        self.alignment() // TODO: handle AlignmentDirectional
    }
    
    /// Flutter's alignChild() — calculates and sets child_offset.
    fn align_child(&mut self, child_size: Size, container_size: Size) {
        let offset = self.resolved_alignment().compute_offset(child_size, container_size);
        self.set_child_offset(offset);
    }
}

// Auto-impl
impl<T: ProxyData> RenderAligningShiftedBoxMixin for AligningShiftedBox<T> { ... }

// Blanket impl (inherits from RenderShiftedBoxMixin)
impl<T: RenderAligningShiftedBoxMixin> RenderProtocol<BoxProtocol> for T { ... }
```

### Examples

```rust
// RenderAlign (= RenderPositionedBox)
#[derive(Clone, Debug)]
pub struct AlignData {
    pub width_factor: Option<f32>,
    pub height_factor: Option<f32>,
}
pub type RenderAlign = AligningShiftedBox<AlignData>;

impl RenderAligningShiftedBoxMixin for RenderAlign {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let shrink_w = self.width_factor.is_some() || !constraints.has_bounded_width();
        let shrink_h = self.height_factor.is_some() || !constraints.has_bounded_height();
        
        if let Some(child) = self.child_mut().get_mut() {
            let child_size = child.perform_layout(&constraints.loosen());
            let size = constraints.constrain(Size::new(
                if shrink_w { child_size.width * self.width_factor.unwrap_or(1.0) } 
                else { constraints.max_width },
                if shrink_h { child_size.height * self.height_factor.unwrap_or(1.0) } 
                else { constraints.max_height },
            ));
            self.align_child(child_size, size);
            self.set_size(size);
            size
        } else {
            let size = constraints.smallest();
            self.set_size(size);
            size
        }
    }
}

// RenderCenter (alias)
pub type RenderCenter = RenderAlign;
```

---

## Part 6: Container — Multiple Children

### Generic Container

```rust
pub struct ContainerBase<P: Protocol, PD: ParentData> {
    pub children: Children<P, PD>,
    pub geometry: P::Geometry,
}

pub struct ContainerInner<T: ProxyData, P: Protocol, PD: ParentData> {
    base: ContainerBase<P, PD>,
    data: T,
}

pub struct Container<T: ProxyData, P: Protocol = BoxProtocol, PD: ParentData = ()> {
    inner: ContainerInner<T, P, PD>,
}

// Deref chain: Container → ContainerInner → T
```

### Box: ContainerBox + RenderContainerBoxMixin

```rust
/// Box with multiple children.
pub type ContainerBox<T, PD = ()> = Container<T, BoxProtocol, PD>;

/// Mixin for container Box render objects (multiple children).
pub trait RenderContainerBoxMixin<PD: ParentData = ()>: RenderObject + HasChildren<BoxProtocol, PD> {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
    
    // Layout MUST be overridden
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    
    // Default: paint all children at their offsets
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (child, pd) in self.children().iter_with_data() {
            child.paint(ctx, offset + pd.offset);
        }
    }
    
    // Default: hit test children in reverse order
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        for (child, pd) in self.children().iter_with_data().rev() {
            if child.hit_test(result, position - pd.offset) {
                return true;
            }
        }
        false
    }
}

// Auto-impl
impl<T: ProxyData, PD: ParentData + Default> RenderContainerBoxMixin<PD> for ContainerBox<T, PD> { ... }

// Blanket impl
impl<T: RenderContainerBoxMixin<PD>, PD: ParentData> RenderProtocol<BoxProtocol> for T { ... }
```

### Sliver: ContainerSliver + RenderContainerSliverMixin

```rust
/// Sliver with multiple children.
pub type ContainerSliver<T, PD = ()> = Container<T, SliverProtocol, PD>;

/// Mixin for container Sliver render objects (multiple children).
pub trait RenderContainerSliverMixin<PD: ParentData = ()>: RenderObject + HasChildren<SliverProtocol, PD> {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { ... }
}

// Auto-impl
impl<T: ProxyData, PD: ParentData + Default> RenderContainerSliverMixin<PD> for ContainerSliver<T, PD> { ... }

// Blanket impl
impl<T: RenderContainerSliverMixin<PD>, PD: ParentData> RenderProtocol<SliverProtocol> for T { ... }
```

### Examples

```rust
// RenderFlex
#[derive(Clone, Debug)]
pub struct FlexData {
    pub direction: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
    pub main_axis_size: MainAxisSize,
}

#[derive(Default, Clone, Debug)]
pub struct FlexParentData {
    pub flex: f32,
    pub fit: FlexFit,
}

pub type RenderFlex = ContainerBox<FlexData, FlexParentData>;

impl RenderContainerBoxMixin<FlexParentData> for RenderFlex {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        // Complex flex layout algorithm...
    }
}

// RenderStack
#[derive(Clone, Debug)]
pub struct StackData {
    pub alignment: Alignment,
    pub fit: StackFit,
    pub clip_behavior: Clip,
}

#[derive(Default, Clone, Debug)]
pub struct StackParentData {
    pub top: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    pub left: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

pub type RenderStack = ContainerBox<StackData, StackParentData>;
```

---

## Part 7: Leaf — No Children

### Generic Leaf

```rust
pub struct LeafBase<P: Protocol> {
    pub geometry: P::Geometry,
}

pub struct LeafInner<T: ProxyData, P: Protocol> {
    base: LeafBase<P>,
    data: T,
}

pub struct Leaf<T: ProxyData, P: Protocol = BoxProtocol> {
    inner: LeafInner<T, P>,
}

// Deref chain: Leaf → LeafInner → T
```

### Box: LeafBox + RenderLeafBoxMixin

```rust
/// Box with no children.
pub type LeafBox<T> = Leaf<T, BoxProtocol>;

/// Mixin for leaf Box render objects (no children).
pub trait RenderLeafBoxMixin: RenderObject {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
    
    // Layout and paint MUST be overridden
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    
    // Default: no children to hit test
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
    
    // Default: no children for intrinsics
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 { 0.0 }
}

// Auto-impl
impl<T: ProxyData> RenderLeafBoxMixin for LeafBox<T> { ... }

// Blanket impl
impl<T: RenderLeafBoxMixin> RenderProtocol<BoxProtocol> for T { ... }
```

### Sliver: LeafSliver + RenderLeafSliverMixin

```rust
/// Sliver with no children.
pub type LeafSliver<T> = Leaf<T, SliverProtocol>;

/// Mixin for leaf Sliver render objects (no children).
pub trait RenderLeafSliverMixin: RenderObject {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
    
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    fn hit_test(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { false }
}

// Auto-impl
impl<T: ProxyData> RenderLeafSliverMixin for LeafSliver<T> { ... }

// Blanket impl
impl<T: RenderLeafSliverMixin> RenderProtocol<SliverProtocol> for T { ... }
```

### Examples

```rust
// RenderColoredBox
#[derive(Clone, Debug)]
pub struct ColoredBoxData {
    pub color: Color,
}
pub type RenderColoredBox = LeafBox<ColoredBoxData>;

impl RenderLeafBoxMixin for RenderColoredBox {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let size = constraints.biggest();
        self.set_size(size);
        size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        ctx.canvas().draw_rect(
            Rect::from_origin_size(offset, self.size()),
            &Paint::new().with_color(self.color),
        );
    }
}

// RenderImage
#[derive(Clone, Debug)]
pub struct ImageData {
    pub image: Option<Image>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub fit: BoxFit,
    pub alignment: Alignment,
}
pub type RenderImage = LeafBox<ImageData>;
```

---

## Summary: Complete Type System

### Child Storage

```rust
// Generic
Child<P>              // Single optional child
Children<P, PD>       // Multiple children + parent data
Slots<P, S>           // Named slots

// Box
BoxChild              // = Child<BoxProtocol>
BoxChildren<PD>       // = Children<BoxProtocol, PD>
BoxSlots<S>           // = Slots<BoxProtocol, S>

// Sliver
SliverChild           // = Child<SliverProtocol>
SliverChildren<PD>    // = Children<SliverProtocol, PD>
SliverSlots<S>        // = Slots<SliverProtocol, S>
```

### Base Types

```rust
// Generic structs
Proxy<T, P>
Shifted<T, P>
AligningShifted<T, P>
Container<T, P, PD>
Leaf<T, P>

// Box (type alias → mixin → blanket impl)
ProxyBox<T>              → RenderProxyBoxMixin              → RenderProtocol<BoxProtocol>
ShiftedBox<T>            → RenderShiftedBoxMixin            → RenderProtocol<BoxProtocol>
AligningShiftedBox<T>    → RenderAligningShiftedBoxMixin    → RenderProtocol<BoxProtocol>
ContainerBox<T, PD>      → RenderContainerBoxMixin<PD>      → RenderProtocol<BoxProtocol>
LeafBox<T>               → RenderLeafBoxMixin               → RenderProtocol<BoxProtocol>

// Sliver (type alias → mixin → blanket impl)
ProxySliver<T>           → RenderProxySliverMixin           → RenderProtocol<SliverProtocol>
ShiftedSliver<T>         → RenderShiftedSliverMixin         → RenderProtocol<SliverProtocol>
ContainerSliver<T, PD>   → RenderContainerSliverMixin<PD>   → RenderProtocol<SliverProtocol>
LeafSliver<T>            → RenderLeafSliverMixin            → RenderProtocol<SliverProtocol>
```

### What's Auto-Implemented

| Feature | Auto |
|---------|------|
| Struct fields (child, children, geometry) | ✓ |
| `Deref` chain to T | ✓ |
| `RenderObject` (attach/detach/visit) | ✓ |
| `ProxyBehavior<P>` / `HasChild<P>` / `HasChildren<P>` | ✓ |
| Base trait defaults (paint, hit_test) | ✓ |
| `RenderProtocol<P>` blanket impl | ✓ |
| `Debug` (if T: Debug) | ✓ |
| `Serialize`/`Deserialize` (feature flag) | ✓ |

### What Developer Writes

```rust
// 1. Data struct
#[derive(Default, Clone, Debug)]
pub struct MyData { pub my_field: f32 }

// 2. Type alias
pub type RenderMy = ProxyBox<MyData>;  // or ShiftedBox, ContainerBox, LeafBox

// 3. Override only needed methods
impl RenderProxyBoxMixin for RenderMy {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // custom paint
    }
}

// DONE! Everything else is auto-implemented.
```

---

## File Organization

```
crates/flui_rendering/src/
├── lib.rs
├── protocol/
│   ├── mod.rs
│   ├── box_protocol.rs
│   └── sliver_protocol.rs
├── children/
│   ├── mod.rs
│   ├── child.rs          # Child<P>, BoxChild, SliverChild
│   ├── children.rs       # Children<P, PD>, BoxChildren, SliverChildren
│   └── slots.rs          # Slots<P, S>, BoxSlots, SliverSlots
├── base/
│   ├── mod.rs
│   ├── data.rs           # ProxyData trait
│   ├── proxy.rs          # Proxy<T, P>, ProxyBox<T>, ProxySliver<T>
│   ├── shifted.rs        # Shifted<T, P>, ShiftedBox<T>, ShiftedSliver<T>
│   ├── aligning.rs       # AligningShifted<T, P>, AligningShiftedBox<T>
│   ├── container.rs      # Container<T, P, PD>, ContainerBox, ContainerSliver
│   └── leaf.rs           # Leaf<T, P>, LeafBox<T>, LeafSliver<T>
├── mixins/
│   ├── mod.rs            # Re-exports all mixins in one place
│   ├── proxy.rs          # RenderProxyBoxMixin, RenderProxySliverMixin
│   ├── shifted.rs        # RenderShiftedBoxMixin, RenderShiftedSliverMixin
│   ├── aligning.rs       # RenderAligningShiftedBoxMixin
│   ├── container.rs      # RenderContainerBoxMixin, RenderContainerSliverMixin
│   └── leaf.rs           # RenderLeafBoxMixin, RenderLeafSliverMixin
└── blanket/
    ├── mod.rs
    └── render_protocol.rs    # All blanket impls
```

### Mixins Module (collected in one place)

```rust
// crates/flui_rendering/src/mixins/mod.rs

//! All rendering mixins in one place.
//!
//! Mixins provide default implementations for common render object patterns.
//! Each mixin corresponds to a base type and provides auto-implemented behavior.

// Box Protocol Mixins
mod proxy;
mod shifted;
mod aligning;
mod container;
mod leaf;

pub use proxy::RenderProxyBoxMixin;
pub use shifted::RenderShiftedBoxMixin;
pub use aligning::RenderAligningShiftedBoxMixin;
pub use container::RenderContainerBoxMixin;
pub use leaf::RenderLeafBoxMixin;

// Sliver Protocol Mixins
pub use proxy::RenderProxySliverMixin;
pub use shifted::RenderShiftedSliverMixin;
pub use container::RenderContainerSliverMixin;
pub use leaf::RenderLeafSliverMixin;

// Convenience re-export for glob import
pub mod prelude {
    pub use super::{
        // Box
        RenderProxyBoxMixin,
        RenderShiftedBoxMixin,
        RenderAligningShiftedBoxMixin,
        RenderContainerBoxMixin,
        RenderLeafBoxMixin,
        // Sliver
        RenderProxySliverMixin,
        RenderShiftedSliverMixin,
        RenderContainerSliverMixin,
        RenderLeafSliverMixin,
    };
}
```
