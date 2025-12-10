# Ambassador-Based Rendering

Автоматическое делегирование трейтов через [ambassador](https://lib.rs/crates/ambassador) crate.

## Зависимость

```toml
[dependencies]
ambassador = "0.4"
```

---

## Architecture Overview

### Core Types (в корне src/)

| File | Type | Description |
|------|------|-------------|
| `object.rs` | `RenderObject` | Базовый трейт для всех render objects |
| `box.rs` | `RenderBox` | Box protocol render object |
| `sliver.rs` | `RenderSliver` | Sliver protocol render object |
| `proxy_box.rs` | `RenderProxyBox` | Proxy trait для Box (без данных) |
| `proxy_sliver.rs` | `RenderProxySliver` | Proxy trait для Sliver (без данных) |
| `protocol.rs` | `Protocol`, `BoxProtocol`, `SliverProtocol` | Protocol system |

### Mixins (в папке mixins/)

| File | Base | Wrapper | Mixin Trait |
|------|------|---------|-------------|
| `proxy.rs` | `ProxyBase<P>` | `ProxyBox<T>`, `ProxySliver<T>` | `RenderProxyBoxMixin`, `RenderProxySliverMixin` |
| `shifted.rs` | `ShiftedBase<P>` | `ShiftedBox<T>`, `ShiftedSliver<T>` | `RenderShiftedBox`, `RenderShiftedSliver` |
| `aligning.rs` | `AligningBase<P>` | `AligningShiftedBox<T>` | `RenderAligningShiftedBox` |
| `container.rs` | `ContainerBase<P,PD>` | `ContainerBox<T,PD>`, `ContainerSliver<T,PD>` | `RenderContainerBox`, `RenderContainerSliver` |
| `leaf.rs` | `LeafBase<P>` | `LeafBox<T>`, `LeafSliver<T>` | `RenderLeafBox`, `RenderLeafSliver` |

### Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                     Delegatable Traits                          │
│  (помечены #[delegatable_trait] — можно делегировать)          │
├─────────────────────────────────────────────────────────────────┤
│  HasChild<P>        │ child(), child_mut()                     │
│  HasChildren<P,PD>  │ children(), children_mut()               │
│  HasBoxGeometry     │ size(), set_size()                       │
│  HasSliverGeometry  │ geometry(), set_geometry()               │
│  HasOffset          │ child_offset(), set_child_offset()       │
│  HasAlignment       │ alignment(), set_alignment(), ...        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ #[delegate(...)]
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              Wrapper Structs (in mixins/*.rs)                   │
│  (автоматически получают impl через derive)                    │
├─────────────────────────────────────────────────────────────────┤
│  ProxyBox<T>           │ delegates HasChild, HasBoxGeometry    │
│  ShiftedBox<T>         │ + HasOffset                           │
│  AligningShiftedBox<T> │ + HasAlignment                        │
│  ContainerBox<T,PD>    │ delegates HasChildren, HasBoxGeometry │
│  LeafBox<T>            │ delegates HasBoxGeometry              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ impl Mixin for Wrapper
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              Mixin Traits (default methods)                     │
├─────────────────────────────────────────────────────────────────┤
│  RenderProxyBoxMixin        │ delegates all to child           │
│  RenderShiftedBox           │ applies offset transform         │
│  RenderAligningShiftedBox   │ + alignment                      │
│  RenderContainerBox         │ iterates children                │
│  RenderLeafBox              │ no children                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ blanket impl
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              RenderProtocol<BoxProtocol>                        │
└─────────────────────────────────────────────────────────────────┘
```

---

## Part 1: Delegatable Traits

### HasChild — Single Child Access

```rust
use ambassador::delegatable_trait;

#[delegatable_trait]
pub trait HasChild<P: Protocol> {
    fn child(&self) -> &Child<P>;
    fn child_mut(&mut self) -> &mut Child<P>;
    
    /// Check if child exists.
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}
```

### HasChildren — Multiple Children Access

```rust
#[delegatable_trait]
pub trait HasChildren<P: Protocol, PD: ParentData = ()> {
    fn children(&self) -> &Children<P, PD>;
    fn children_mut(&mut self) -> &mut Children<P, PD>;
    
    fn child_count(&self) -> usize {
        self.children().len()
    }
}
```

### HasBoxGeometry — Box Size

```rust
#[delegatable_trait]
pub trait HasBoxGeometry {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);
}
```

### HasSliverGeometry — Sliver Geometry

```rust
#[delegatable_trait]
pub trait HasSliverGeometry {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);
}
```

### HasOffset — Child Offset

```rust
#[delegatable_trait]
pub trait HasOffset {
    fn child_offset(&self) -> Offset;
    fn set_child_offset(&mut self, offset: Offset);
}
```

### HasAlignment — Alignment + TextDirection

```rust
#[delegatable_trait]
pub trait HasAlignment {
    fn alignment(&self) -> Alignment;
    fn set_alignment(&mut self, alignment: Alignment);
    fn text_direction(&self) -> Option<TextDirection>;
    fn set_text_direction(&mut self, dir: Option<TextDirection>);
    
    /// Resolve alignment for RTL/LTR.
    fn resolved_alignment(&self) -> Alignment {
        self.alignment() // TODO: handle AlignmentDirectional
    }
}
```

---

## Part 2: Base Structs

### ProxyBase — Child + Geometry

```rust
/// Base for proxy render objects.
#[derive(Debug, Default)]
pub struct ProxyBase<P: Protocol> 
where
    P::Geometry: Default,
{
    child: Child<P>,
    geometry: P::Geometry,
}

impl<P: Protocol> HasChild<P> for ProxyBase<P> {
    fn child(&self) -> &Child<P> { &self.child }
    fn child_mut(&mut self) -> &mut Child<P> { &mut self.child }
}

// Box specialization
impl HasBoxGeometry for ProxyBase<BoxProtocol> {
    fn size(&self) -> Size { self.geometry }
    fn set_size(&mut self, size: Size) { self.geometry = size; }
}

// Sliver specialization
impl HasSliverGeometry for ProxyBase<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry { &self.geometry }
    fn set_geometry(&mut self, geometry: SliverGeometry) { self.geometry = geometry; }
}
```

### ShiftedBase — + Offset

```rust
/// Base for shifted render objects.
#[derive(Debug, Default)]
pub struct ShiftedBase<P: Protocol>
where
    P::Geometry: Default,
{
    proxy: ProxyBase<P>,
    offset: Offset,
}

impl<P: Protocol> HasChild<P> for ShiftedBase<P> {
    fn child(&self) -> &Child<P> { self.proxy.child() }
    fn child_mut(&mut self) -> &mut Child<P> { self.proxy.child_mut() }
}

impl HasBoxGeometry for ShiftedBase<BoxProtocol> {
    fn size(&self) -> Size { self.proxy.size() }
    fn set_size(&mut self, size: Size) { self.proxy.set_size(size); }
}

impl<P: Protocol> HasOffset for ShiftedBase<P> {
    fn child_offset(&self) -> Offset { self.offset }
    fn set_child_offset(&mut self, offset: Offset) { self.offset = offset; }
}
```

### AligningBase — + Alignment

```rust
/// Base for aligning render objects.
#[derive(Debug)]
pub struct AligningBase<P: Protocol>
where
    P::Geometry: Default,
{
    shifted: ShiftedBase<P>,
    alignment: Alignment,
    text_direction: Option<TextDirection>,
}

impl<P: Protocol> HasChild<P> for AligningBase<P> {
    fn child(&self) -> &Child<P> { self.shifted.child() }
    fn child_mut(&mut self) -> &mut Child<P> { self.shifted.child_mut() }
}

impl HasBoxGeometry for AligningBase<BoxProtocol> {
    fn size(&self) -> Size { self.shifted.size() }
    fn set_size(&mut self, size: Size) { self.shifted.set_size(size); }
}

impl<P: Protocol> HasOffset for AligningBase<P> {
    fn child_offset(&self) -> Offset { self.shifted.child_offset() }
    fn set_child_offset(&mut self, offset: Offset) { self.shifted.set_child_offset(offset); }
}

impl<P: Protocol> HasAlignment for AligningBase<P> {
    fn alignment(&self) -> Alignment { self.alignment }
    fn set_alignment(&mut self, alignment: Alignment) { self.alignment = alignment; }
    fn text_direction(&self) -> Option<TextDirection> { self.text_direction }
    fn set_text_direction(&mut self, dir: Option<TextDirection>) { self.text_direction = dir; }
}
```

### ContainerBase — Multiple Children

```rust
/// Base for container render objects.
#[derive(Debug, Default)]
pub struct ContainerBase<P: Protocol, PD: ParentData = ()>
where
    P::Geometry: Default,
{
    children: Children<P, PD>,
    geometry: P::Geometry,
}

impl<P: Protocol, PD: ParentData> HasChildren<P, PD> for ContainerBase<P, PD> {
    fn children(&self) -> &Children<P, PD> { &self.children }
    fn children_mut(&mut self) -> &mut Children<P, PD> { &mut self.children }
}

impl<PD: ParentData> HasBoxGeometry for ContainerBase<BoxProtocol, PD> {
    fn size(&self) -> Size { self.geometry }
    fn set_size(&mut self, size: Size) { self.geometry = size; }
}
```

### LeafBase — No Children

```rust
/// Base for leaf render objects.
#[derive(Debug, Default)]
pub struct LeafBase<P: Protocol>
where
    P::Geometry: Default,
{
    geometry: P::Geometry,
}

impl HasBoxGeometry for LeafBase<BoxProtocol> {
    fn size(&self) -> Size { self.geometry }
    fn set_size(&mut self, size: Size) { self.geometry = size; }
}

impl HasSliverGeometry for LeafBase<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry { &self.geometry }
    fn set_geometry(&mut self, geometry: SliverGeometry) { self.geometry = geometry; }
}
```

---

## Part 3: Generic Wrappers with Ambassador

### ProxyBox<T> — Automatic Delegation

```rust
use ambassador::Delegate;

/// Proxy render object — delegates all to child.
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
pub struct ProxyBox<T: ProxyData> {
    base: ProxyBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> ProxyBox<T> {
    pub fn new(data: T) -> Self {
        Self { base: ProxyBase::default(), data }
    }
}

// Deref for direct field access: self.my_field
impl<T: ProxyData> Deref for ProxyBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

impl<T: ProxyData> DerefMut for ProxyBox<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}
```

### ShiftedBox<T>

```rust
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
#[delegate(HasOffset, target = "base")]
pub struct ShiftedBox<T: ProxyData> {
    base: ShiftedBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> Deref for ShiftedBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

impl<T: ProxyData> DerefMut for ShiftedBox<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}
```

### AligningShiftedBox<T>

```rust
#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
#[delegate(HasOffset, target = "base")]
#[delegate(HasAlignment, target = "base")]
pub struct AligningShiftedBox<T: ProxyData> {
    base: AligningBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> Deref for AligningShiftedBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

impl<T: ProxyData> DerefMut for AligningShiftedBox<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}
```

### ContainerBox<T, PD>

```rust
#[derive(Debug, Delegate)]
#[delegate(HasChildren<BoxProtocol, PD>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
pub struct ContainerBox<T: ProxyData, PD: ParentData = ()> {
    base: ContainerBase<BoxProtocol, PD>,
    pub data: T,
}

impl<T: ProxyData, PD: ParentData> Deref for ContainerBox<T, PD> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

impl<T: ProxyData, PD: ParentData> DerefMut for ContainerBox<T, PD> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}
```

### LeafBox<T>

```rust
#[derive(Debug, Delegate)]
#[delegate(HasBoxGeometry, target = "base")]
pub struct LeafBox<T: ProxyData> {
    base: LeafBase<BoxProtocol>,
    pub data: T,
}

impl<T: ProxyData> Deref for LeafBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

impl<T: ProxyData> DerefMut for LeafBox<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.data }
}
```

---

## Part 4: Render Traits with Defaults

### RenderProxyBox

```rust
/// Trait for proxy Box render objects.
/// All methods delegate to child by default.
pub trait RenderProxyBox: HasChild<BoxProtocol> + HasBoxGeometry {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child_mut().get_mut() {
            let size = child.layout(constraints);
            self.set_size(size);
            size
        } else {
            self.set_size(constraints.smallest());
            constraints.smallest()
        }
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child().get() {
            child.paint(ctx, offset);
        }
    }
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.child().get()
            .map(|c| c.hit_test(result, position))
            .unwrap_or(false)
    }
    
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child().get()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }
    
    // ... other intrinsics delegate similarly
    
    fn always_needs_compositing(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
}

// Blanket impl for all ProxyBox<T>
impl<T: ProxyData> RenderProxyBox for ProxyBox<T> {}
```

### RenderShiftedBox

```rust
/// Trait for shifted Box render objects.
/// Applies offset transform in paint/hit_test.
pub trait RenderShiftedBox: HasChild<BoxProtocol> + HasBoxGeometry + HasOffset {
    /// MUST be overridden — layout logic is specific to each widget.
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child().get() {
            child.paint(ctx, offset + self.child_offset());
        }
    }
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.child().get()
            .map(|c| c.hit_test(result, position - self.child_offset()))
            .unwrap_or(false)
    }
    
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child().get()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }
    
    // ... other defaults
}

// Blanket impl — but perform_layout has no default!
impl<T: ProxyData> RenderShiftedBox for ShiftedBox<T> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!("perform_layout must be overridden for {}", std::any::type_name::<T>())
    }
}
```

### RenderAligningShiftedBox

```rust
/// Trait for aligning shifted Box render objects.
/// Adds align_child() helper.
pub trait RenderAligningShiftedBox: RenderShiftedBox + HasAlignment {
    /// Calculate and set child_offset based on alignment.
    fn align_child(&mut self, child_size: Size, container_size: Size) {
        let offset = self.resolved_alignment().compute_offset(child_size, container_size);
        self.set_child_offset(offset);
    }
}

impl<T: ProxyData> RenderAligningShiftedBox for AligningShiftedBox<T> {}
```

### RenderContainerBox

```rust
/// Trait for container Box render objects.
pub trait RenderContainerBox<PD: ParentData = ()>: HasChildren<BoxProtocol, PD> + HasBoxGeometry {
    /// MUST be overridden.
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        for (child, pd) in self.children().iter_with_data() {
            child.paint(ctx, offset + pd.offset);
        }
    }
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        for (child, pd) in self.children().iter_with_data().rev() {
            if child.hit_test(result, position - pd.offset) {
                return true;
            }
        }
        false
    }
}

impl<T: ProxyData, PD: ParentData> RenderContainerBox<PD> for ContainerBox<T, PD> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!("perform_layout must be overridden")
    }
}
```

### RenderLeafBox

```rust
/// Trait for leaf Box render objects.
pub trait RenderLeafBox: HasBoxGeometry {
    /// MUST be overridden.
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;
    
    /// MUST be overridden.
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    
    fn hit_test(&self, _result: &mut BoxHitTestResult, position: Offset) -> bool {
        let size = self.size();
        position.x >= 0.0 && position.x < size.width &&
        position.y >= 0.0 && position.y < size.height
    }
    
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 { 0.0 }
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 { 0.0 }
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 { 0.0 }
}

impl<T: ProxyData> RenderLeafBox for LeafBox<T> {
    fn perform_layout(&mut self, _: &BoxConstraints) -> Size { panic!("must override") }
    fn paint(&self, _: &mut PaintingContext, _: Offset) { panic!("must override") }
}
```

---

## Part 5: Blanket Impl → RenderProtocol

```rust
// ProxyBox → RenderProtocol
impl<T: ProxyData> RenderProtocol<BoxProtocol> for ProxyBox<T>
where
    Self: RenderProxyBox,
{
    fn perform_layout(&mut self, c: &BoxConstraints) -> Size {
        RenderProxyBox::perform_layout(self, c)
    }
    fn paint(&self, ctx: &mut PaintingContext, o: Offset) {
        RenderProxyBox::paint(self, ctx, o)
    }
    fn hit_test(&self, r: &mut BoxHitTestResult, p: Offset) -> bool {
        RenderProxyBox::hit_test(self, r, p)
    }
    // ... other methods
}

// ShiftedBox → RenderProtocol
impl<T: ProxyData> RenderProtocol<BoxProtocol> for ShiftedBox<T>
where
    Self: RenderShiftedBox,
{
    fn perform_layout(&mut self, c: &BoxConstraints) -> Size {
        RenderShiftedBox::perform_layout(self, c)
    }
    fn paint(&self, ctx: &mut PaintingContext, o: Offset) {
        RenderShiftedBox::paint(self, ctx, o)
    }
    fn hit_test(&self, r: &mut BoxHitTestResult, p: Offset) -> bool {
        RenderShiftedBox::hit_test(self, r, p)
    }
}

// ... similar for other base types
```

---

## Part 6: Usage Examples

### RenderOpacity (ProxyBox)

```rust
#[derive(Default, Clone, Debug)]
pub struct OpacityData {
    pub alpha: f32,
}

pub type RenderOpacity = ProxyBox<OpacityData>;

impl RenderOpacity {
    pub fn new(alpha: f32) -> Self {
        ProxyBox::new(OpacityData { alpha: alpha.clamp(0.0, 1.0) })
    }
}

// Override only what differs!
impl RenderProxyBox for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        match self.alpha {  // self.alpha via Deref!
            a if a == 0.0 => {}
            a if a == 1.0 => {
                if let Some(c) = self.child().get() {
                    c.paint(ctx, offset);
                }
            }
            a => ctx.push_opacity(a, offset, |ctx| {
                if let Some(c) = self.child().get() {
                    c.paint(ctx, Offset::ZERO);
                }
            }),
        }
    }
    
    fn always_needs_compositing(&self) -> bool {
        self.alpha > 0.0 && self.alpha < 1.0
    }
}

// AUTO via ambassador:
// - HasChild<BoxProtocol>: child(), child_mut()
// - HasBoxGeometry: size(), set_size()
// AUTO via blanket:
// - RenderProtocol<BoxProtocol>
```

### RenderPadding (ShiftedBox)

```rust
#[derive(Default, Clone, Debug)]
pub struct PaddingData {
    pub padding: EdgeInsets,
}

pub type RenderPadding = ShiftedBox<PaddingData>;

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        ShiftedBox::new(PaddingData { padding })
    }
}

// MUST override perform_layout
impl RenderShiftedBox for RenderPadding {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let inner = constraints.deflate(&self.padding);  // self.padding via Deref!
        
        if let Some(child) = self.child_mut().get_mut() {
            let child_size = child.layout(&inner);
            self.set_child_offset(Offset::new(self.padding.left, self.padding.top));
            let size = constraints.constrain(child_size + self.padding.size());
            self.set_size(size);
            size
        } else {
            let size = constraints.constrain(self.padding.size());
            self.set_size(size);
            size
        }
    }
}

// AUTO via ambassador:
// - HasChild<BoxProtocol>
// - HasBoxGeometry  
// - HasOffset: child_offset(), set_child_offset()
// AUTO via mixin defaults:
// - paint() — applies child_offset
// - hit_test() — applies child_offset
```

### RenderAlign (AligningShiftedBox)

```rust
#[derive(Clone, Debug)]
pub struct AlignData {
    pub width_factor: Option<f32>,
    pub height_factor: Option<f32>,
}

impl Default for AlignData {
    fn default() -> Self {
        Self { width_factor: None, height_factor: None }
    }
}

pub type RenderAlign = AligningShiftedBox<AlignData>;

impl RenderAlign {
    pub fn new(alignment: Alignment) -> Self {
        let mut this = AligningShiftedBox::new(AlignData::default());
        this.set_alignment(alignment);  // via HasAlignment!
        this
    }
}

impl RenderShiftedBox for RenderAlign {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let shrink_w = self.width_factor.is_some() || !constraints.has_bounded_width();
        let shrink_h = self.height_factor.is_some() || !constraints.has_bounded_height();
        
        if let Some(child) = self.child_mut().get_mut() {
            let child_size = child.layout(&constraints.loosen());
            let size = constraints.constrain(Size::new(
                if shrink_w { child_size.width * self.width_factor.unwrap_or(1.0) }
                else { constraints.max_width },
                if shrink_h { child_size.height * self.height_factor.unwrap_or(1.0) }
                else { constraints.max_height },
            ));
            self.align_child(child_size, size);  // via RenderAligningShiftedBoxMixin!
            self.set_size(size);
            size
        } else {
            let size = constraints.smallest();
            self.set_size(size);
            size
        }
    }
}

// AUTO via ambassador:
// - HasChild, HasBoxGeometry, HasOffset, HasAlignment
// AUTO via trait:
// - align_child() helper
// - paint(), hit_test() with offset
```

### RenderFlex (ContainerBox)

```rust
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

impl RenderContainerBox<FlexParentData> for RenderFlex {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        // Complex flex layout algorithm using:
        // - self.direction, self.main_axis_alignment (via Deref)
        // - self.children_mut() (via HasChildren)
        todo!("flex layout")
    }
}

// AUTO:
// - HasChildren<BoxProtocol, FlexParentData>
// - HasBoxGeometry
// - paint() iterates children
// - hit_test() iterates children in reverse
```

### RenderColoredBox (LeafBox)

```rust
#[derive(Clone, Debug)]
pub struct ColoredBoxData {
    pub color: Color,
}

pub type RenderColoredBox = LeafBox<ColoredBoxData>;

impl RenderLeafBox for RenderColoredBox {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let size = constraints.biggest();
        self.set_size(size);
        size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        ctx.canvas().draw_rect(
            Rect::from_origin_size(offset, self.size()),
            &Paint::new().with_color(self.color),  // self.color via Deref!
        );
    }
}

// AUTO:
// - HasBoxGeometry
// - hit_test() bounds check
```

---

## Summary: What's Generated

### Ambassador generates (trait delegation):

```rust
// From #[delegate(HasChild<BoxProtocol>, target = "base")]
impl<T: ProxyData> HasChild<BoxProtocol> for ProxyBox<T> {
    fn child(&self) -> &Child<BoxProtocol> { self.base.child() }
    fn child_mut(&mut self) -> &mut Child<BoxProtocol> { self.base.child_mut() }
}
```

### We keep Deref for field access:

```rust
// self.alpha instead of self.data.alpha
impl<T: ProxyData> Deref for ProxyBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}
```

### Mixin traits provide defaults:

```rust
// Default paint/hit_test/intrinsics
pub trait RenderShiftedBoxMixin: HasChild<BoxProtocol> + HasBoxGeometry + HasOffset {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Default implementation using delegated traits
    }
}
```

---

## File Organization

### Module Style: Rust 2024 (без mod.rs)

Используем **Rust 2024 edition module style** (Rust 1.90+) — именованные файлы вместо `mod.rs`:

```
# Старый стиль (НЕ используем)
children/
├── mod.rs      ← много открытых "mod.rs" табов в IDE
├── child.rs
└── slots.rs

# Новый стиль (используем)
children.rs     ← точка входа модуля, уникальное имя
children/
├── child.rs
└── slots.rs
```

**Преимущества:**
- Уникальные имена файлов в IDE (не 5 табов "mod.rs")
- Чёткая навигация — сразу видно какой модуль
- Современный Rust style (2018+)

### Структура

```
crates/flui_rendering/src/
├── lib.rs
│
├── object.rs              # RenderObject — базовый трейт для всех
│
├── box.rs                 # RenderBox — Box protocol render object
├── sliver.rs              # RenderSliver — Sliver protocol render object
│
├── proxy_box.rs           # RenderProxyBox — proxy для Box (трейт, без данных)
├── proxy_sliver.rs        # RenderProxySliver — proxy для Sliver (трейт, без данных)
│
├── children.rs            # Re-exports: Child, Children, Slots, BoxChild, etc.
├── children/
│   ├── child.rs           # Child<P>, BoxChild, SliverChild
│   ├── children.rs        # Children<P, PD>, BoxChildren, SliverChildren
│   └── slots.rs           # Slots<P, S>, BoxSlots, SliverSlots
│
├── mixins.rs              # Re-exports всех миксинов
├── mixins/
│   │
│   ├── proxy.rs           # Proxy mixin (всё в одном файле):
│   │                      #   - ProxyBase<P>
│   │                      #   - ProxyBox<T>, ProxySliver<T> (type aliases)
│   │                      #   - RenderProxyBoxMixin, RenderProxySliverMixin
│   │                      #   - blanket impls
│   │
│   ├── shifted.rs         # Shifted mixin:
│   │                      #   - ShiftedBase<P>
│   │                      #   - ShiftedBox<T>, ShiftedSliver<T>
│   │                      #   - RenderShiftedBox, RenderShiftedSliver
│   │                      #   - blanket impls
│   │
│   ├── aligning.rs        # Aligning mixin:
│   │                      #   - AligningBase<P>
│   │                      #   - AligningShiftedBox<T>
│   │                      #   - RenderAligningShiftedBox
│   │                      #   - blanket impls
│   │
│   ├── container.rs       # Container mixin:
│   │                      #   - ContainerBase<P, PD>
│   │                      #   - ContainerBox<T, PD>, ContainerSliver<T, PD>
│   │                      #   - RenderContainerBox, RenderContainerSliver
│   │                      #   - blanket impls
│   │
│   └── leaf.rs            # Leaf mixin:
│                          #   - LeafBase<P>
│                          #   - LeafBox<T>, LeafSliver<T>
│                          #   - RenderLeafBox, RenderLeafSliver
│                          #   - blanket impls
│
└── protocol.rs            # Protocol trait + BoxProtocol, SliverProtocol
```

### Пример содержимого children.rs (точка входа модуля)

```rust
//! Child storage types for render objects.

mod child;
mod children;
mod slots;

pub use child::{Child, BoxChild, SliverChild};
pub use children::{Children, BoxChildren, SliverChildren};
pub use slots::{Slots, BoxSlots, SliverSlots};
```

### Структура одного mixin файла (например proxy.rs):

```rust
//! Proxy mixin — delegates all to single child.

use ambassador::{delegatable_trait, Delegate};

// ============================================
// Part 1: Delegatable Traits
// ============================================

#[delegatable_trait]
pub trait HasChild<P: Protocol> {
    fn child(&self) -> &Child<P>;
    fn child_mut(&mut self) -> &mut Child<P>;
}

// ============================================
// Part 2: Base Struct
// ============================================

#[derive(Debug, Default)]
pub struct ProxyBase<P: Protocol> {
    child: Child<P>,
    geometry: P::Geometry,
}

impl<P: Protocol> HasChild<P> for ProxyBase<P> { ... }
impl HasBoxGeometry for ProxyBase<BoxProtocol> { ... }
impl HasSliverGeometry for ProxyBase<SliverProtocol> { ... }

// ============================================
// Part 3: Wrapper Structs with Ambassador
// ============================================

#[derive(Debug, Delegate)]
#[delegate(HasChild<BoxProtocol>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
pub struct ProxyBoxInner<T: ProxyData> {
    base: ProxyBase<BoxProtocol>,
    pub data: T,
}

/// Type alias for convenience.
pub type ProxyBox<T> = ProxyBoxInner<T>;

impl<T: ProxyData> Deref for ProxyBox<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.data }
}

// Same for Sliver...
pub type ProxySliver<T> = ProxySliverInner<T>;

// ============================================
// Part 4: Render Traits (Mixin behavior)
// ============================================

/// Mixin trait for proxy Box render objects.
pub trait RenderProxyBoxMixin: HasChild<BoxProtocol> + HasBoxGeometry {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size { ... }
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool { ... }
    // ... defaults that delegate to child
}

impl<T: ProxyData> RenderProxyBoxMixin for ProxyBox<T> {}

/// Mixin trait for proxy Sliver render objects.
pub trait RenderProxySliverMixin: HasChild<SliverProtocol> + HasSliverGeometry {
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry { ... }
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    // ... defaults
}

impl<T: ProxyData> RenderProxySliverMixin for ProxySliver<T> {}

// ============================================
// Part 5: Blanket Impls → RenderProtocol
// ============================================

impl<T: ProxyData> RenderProtocol<BoxProtocol> for ProxyBox<T>
where
    Self: RenderProxyBoxMixin,
{
    fn perform_layout(&mut self, c: &BoxConstraints) -> Size {
        RenderProxyBoxMixin::perform_layout(self, c)
    }
    // ...
}

impl<T: ProxyData> RenderProtocol<SliverProtocol> for ProxySliver<T>
where
    Self: RenderProxySliverMixin,
{
    // ...
}
```

---

## Architecture Decisions

### RenderObject trait с layout/paint/hit_test

Добавляем методы layout/paint/hit_test в `RenderObject` trait (как в Flutter), используя enum для type erasure:

```rust
/// Protocol-agnostic geometry result.
pub enum Geometry {
    Box(Size),
    Sliver(SliverGeometry),
}

/// Protocol-agnostic constraints.
pub enum Constraints {
    Box(BoxConstraints),
    Sliver(SliverConstraints),
}

pub trait RenderObject: DowncastSync + fmt::Debug {
    // ========== Existing metadata methods ==========
    fn debug_name(&self) -> &'static str;
    fn visit_children(&self, visitor: &mut dyn FnMut(RenderId));
    fn is_relayout_boundary(&self) -> bool;
    fn is_repaint_boundary(&self) -> bool;
    // ... etc ...
    
    // ========== NEW: Protocol & Layout/Paint/HitTest ==========
    
    /// Returns the protocol (Box or Sliver).
    fn protocol(&self) -> ProtocolId;
    
    /// Performs layout with protocol-specific constraints.
    fn perform_layout(&mut self, constraints: Constraints) -> Geometry;
    
    /// Paints this render object.
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);
    
    /// Hit tests this render object.
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool;
}
```

**Использование в PipelineOwner:**

```rust
// Layout phase
let node = render_tree.get_mut(id)?;
let constraints = Constraints::Box(BoxConstraints::tight(size));
let geometry = node.render_object_mut().perform_layout(constraints);

match geometry {
    Geometry::Box(size) => node.set_cached_size(Some(size)),
    Geometry::Sliver(sliver_geom) => { /* handle sliver */ }
}

// Paint phase  
let node = render_tree.get(id)?;
node.render_object().paint(&mut ctx, offset);
```

**Интеграция с миксинами:**

```rust
impl<T: ProxyData> RenderObject for ProxyBox<T> 
where 
    Self: RenderProxyBoxMixin 
{
    fn protocol(&self) -> ProtocolId {
        ProtocolId::Box
    }
    
    fn perform_layout(&mut self, constraints: Constraints) -> Geometry {
        let Constraints::Box(box_constraints) = constraints else {
            panic!("ProxyBox expects BoxConstraints");
        };
        let size = RenderProxyBoxMixin::perform_layout(self, &box_constraints);
        Geometry::Box(size)
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        RenderProxyBoxMixin::paint(self, ctx, offset);
    }
    
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        RenderProxyBoxMixin::hit_test(self, result, position)
    }
}
```

---

### RenderState переносится в RenderNode

`RenderState` переносится из `RenderElement` в `RenderNode`:

**Причина:** В Flutter state (`_needsLayout`, `_constraints`, `size`) живёт внутри `RenderObject`. В Rust мы не можем использовать наследование, поэтому храним state в `RenderNode` рядом с `render_object`.

**До:**
```rust
// RenderElement (в flui_rendering/element.rs)
pub struct RenderElement {
    state: TypedProtocolState,  // ← state тут
    render_id: Option<RenderId>,
    // ...
}

// RenderNode (в flui_rendering/render_tree.rs)
pub struct RenderNode {
    render_object: Box<dyn RenderObject>,
    cached_size: Option<Size>,  // ← дублирование!
    // ...
}
```

**После:**
```rust
// RenderNode (в flui_rendering/render_tree.rs)
pub struct RenderNode {
    // Tree structure
    parent: Option<RenderId>,
    children: Vec<RenderId>,
    
    // Render object (поведение)
    render_object: Box<dyn RenderObject>,
    
    // State (данные) — всё тут!
    state: TypedProtocolState,  // RenderState<Box> или RenderState<Sliver>
    
    // Cross-tree reference
    element_id: Option<ElementId>,
}

// RenderElement (в flui-element/) — становится легче
pub struct RenderElement {
    id: Option<ElementId>,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    depth: usize,
    
    render_id: Option<RenderId>,  // только ссылка!
    protocol: ProtocolId,
    arity: RuntimeArity,
    
    lifecycle: RenderLifecycle,
    parent_data: Option<Box<dyn ParentData>>,
}
```

**Преимущества:**
- State рядом с RenderObject (как в Flutter)
- Нет дублирования (`cached_size` удаляется)
- RenderElement легче
- PipelineOwner работает только с RenderTree (не нужен ElementTree для state)

---

### RenderElement → flui-element

`RenderElement` переносится из `flui_rendering` в `flui-element`:

**Причина:** `RenderElement` — это Element (имеет `id`, `parent`, `children`, `depth`, `lifecycle`), который владеет ссылкой на RenderObject. По Flutter архитектуре `RenderObjectElement` живёт в `widgets/`, не в `rendering/`.

```
До:
  flui_rendering/element.rs    → RenderElement  ❌

После:
  flui-element/render_element.rs → RenderElement  ✅
  flui_rendering/               → только RenderObject, RenderTree
```

**Связи между деревьями:**
```
RenderElement (в flui-element)
  └── render_id: RenderId → ссылка в RenderTree

RenderNode (в flui_rendering)  
  └── element_id: ElementId → обратная ссылка на Element
```

### PipelineOwner остаётся в flui_rendering

`PipelineOwner` координирует layout/paint фазы и работает с `RenderTree`. Он остаётся в `flui_rendering`.

---

## Refactoring Plan: Current → Target

### Current Files in `src/`

| File | Content | Status |
|------|---------|--------|
| `object.rs` | `RenderObject` | ✅ Keep — base type |
| `box_render.rs` | `RenderBox` | ✅ Keep — rename to `box.rs` |
| `sliver.rs` | `RenderSliver` | ✅ Keep — base type |
| `protocol.rs` | `Protocol`, `BoxProtocol`, `SliverProtocol` | ✅ Keep |
| `proxy.rs` | `RenderProxyBox`, `RenderProxySliver` | ✅ Keep — base types |
| `context.rs` | Layout/Paint/HitTest contexts | ✅ Keep |
| `flags.rs` | `AtomicRenderFlags` | ✅ Keep |
| `state.rs` | `RenderState` | ✅ Keep |
| `parent_data.rs` | `ParentData`, `BoxParentData` | ✅ Keep |
| `element.rs` | `RenderElement` | 🚚 Move to `flui-element` |
| `lifecycle.rs` | `RenderLifecycle` | ✅ Keep |
| `tree.rs` | Tree traits | ✅ Keep |
| `render_tree.rs` | `RenderTree`, `RenderNode` | ✅ Keep |
| `pipeline_owner.rs` | `RenderPipelineOwner` | ✅ Keep |
| `wrapper.rs` | `BoxRenderWrapper`, `SliverRenderWrapper` | ❓ Review |
| `error.rs` | Error types | ✅ Keep |

### New Files to Add

| File | Content | Priority |
|------|---------|----------|
| `viewport.rs` | `RenderViewportBase` | P1 — base type |
| `shifted_box.rs` | `RenderShiftedBox` | P1 — base type |
| `aligning_shifted_box.rs` | `RenderAligningShiftedBox` | P1 — base type |
| `children/mod.rs` | Child storage module | P1 |
| `children/child.rs` | `Child<P>`, `BoxChild`, `SliverChild` | P1 |
| `children/children.rs` | `Children<P, PD>`, `BoxChildren`, `SliverChildren` | P1 |
| `children/slots.rs` | `Slots<P, S>`, `BoxSlots`, `SliverSlots` | P2 |
| `mixins/mod.rs` | Mixins module | P1 |
| `mixins/proxy.rs` | `ProxyBox<T>`, `RenderProxyBoxMixin`, etc. | P1 |
| `mixins/shifted.rs` | `ShiftedBox<T>`, `RenderShiftedBoxMixin`, etc. | P1 |
| `mixins/aligning.rs` | `AligningShiftedBox<T>`, etc. | P2 |
| `mixins/container.rs` | `ContainerBox<T, PD>`, etc. | P1 |
| `mixins/leaf.rs` | `LeafBox<T>`, etc. | P1 |

### Target Structure

> **Note:** Используем Rust 2018 module style — `children.rs` вместо `children/mod.rs`

```
crates/flui_rendering/src/
├── lib.rs
│
│ # ===== Existing (keep) =====
├── object.rs              # RenderObject
├── box.rs                 # RenderBox (renamed from box_render.rs)
├── sliver.rs              # RenderSliver
├── protocol.rs            # Protocol, BoxProtocol, SliverProtocol
├── proxy.rs               # RenderProxyBox, RenderProxySliver (traits)
├── context.rs             # Layout/Paint/HitTest contexts
├── flags.rs               # AtomicRenderFlags
├── state.rs               # RenderState
├── parent_data.rs         # ParentData, BoxParentData
├── element.rs             # RenderElement
├── lifecycle.rs           # RenderLifecycle
├── tree.rs                # Tree operation traits
├── render_tree.rs         # RenderTree, RenderNode
├── pipeline_owner.rs      # RenderPipelineOwner
├── error.rs               # Error types
│
│ # ===== New base types =====
├── viewport.rs            # RenderViewportBase (Box outside, Sliver children)
├── shifted_box.rs         # RenderShiftedBox (single child + offset)
├── aligning_shifted_box.rs # RenderAligningShiftedBox (+ alignment)
│
│ # ===== New child storage (Rust 2018 style) =====
├── children.rs            # Module entry: re-exports Child, Children, Slots
├── children/
│   ├── child.rs           # Child<P>, BoxChild, SliverChild
│   ├── children.rs        # Children<P, PD>, BoxChildren<PD>, SliverChildren<PD>
│   └── slots.rs           # Slots<P, S>, BoxSlots<S>, SliverSlots<S>
│
│ # ===== New mixins (Rust 2018 style) =====
├── mixins.rs              # Module entry: re-exports all mixins
└── mixins/
    ├── proxy.rs           # Proxy mixin:
    │                      #   - ProxyBase<P>
    │                      #   - ProxyBox<T>, ProxySliver<T>
    │                      #   - RenderProxyBoxMixin, RenderProxySliverMixin
    │
    ├── shifted.rs         # Shifted mixin:
    │                      #   - ShiftedBase<P>
    │                      #   - ShiftedBox<T>, ShiftedSliver<T>
    │                      #   - RenderShiftedBoxMixin, RenderShiftedSliverMixin
    │
    ├── aligning.rs        # Aligning mixin:
    │                      #   - AligningBase<P>
    │                      #   - AligningShiftedBox<T>
    │                      #   - RenderAligningShiftedBoxMixin
    │
    ├── container.rs       # Container mixin:
    │                      #   - ContainerBase<P, PD>
    │                      #   - ContainerBox<T, PD>, ContainerSliver<T, PD>
    │                      #   - RenderContainerBox, RenderContainerSliver
    │
    └── leaf.rs            # Leaf mixin:
                           #   - LeafBase<P>
                           #   - LeafBox<T>, LeafSliver<T>
                           #   - RenderLeafBox, RenderLeafSliver
```

### Migration Steps

**Phase 1: Foundation**
1. Add `ambassador` dependency to `Cargo.toml`
2. Rename `box_render.rs` → `box.rs`
3. Create `children/` module with `Child<P>`, `Children<P, PD>`, `Slots<P, S>`

**Phase 2: Base Types**
4. Add `shifted_box.rs` with `RenderShiftedBox` trait
5. Add `aligning_shifted_box.rs` with `RenderAligningShiftedBox` trait
6. Add `viewport.rs` with `RenderViewportBase` trait

**Phase 3: Mixins**
7. Create `mixins/` module structure
8. Implement `mixins/proxy.rs` — ProxyBox<T> + RenderProxyBoxMixin
9. Implement `mixins/shifted.rs` — ShiftedBox<T> + RenderShiftedBoxMixin
10. Implement `mixins/container.rs` — ContainerBox<T, PD> + RenderContainerBox
11. Implement `mixins/leaf.rs` — LeafBox<T> + RenderLeafBox
12. Implement `mixins/aligning.rs` — AligningShiftedBox<T>

**Phase 4: Integration**
13. Update `lib.rs` with new exports
14. Update `flui_widgets` to use new mixins
15. Remove deprecated code

---

## Two Approaches: Mixins vs Manual

Разработчик выбирает сам — использовать миксины или реализовать всё вручную.

### Approach 1: Using Mixins (минимум кода)

```rust
use flui_rendering::mixins::{ShiftedBox, RenderShiftedBox};

#[derive(Default, Clone, Debug)]
pub struct PaddingData {
    pub padding: EdgeInsets,
}

pub type RenderPadding = ShiftedBox<PaddingData>;

impl RenderShiftedBox for RenderPadding {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        // Только логика layout — всё остальное auto!
        let inner = constraints.deflate(&self.padding);
        if let Some(child) = self.child_mut().get_mut() {
            let child_size = child.layout(&inner);
            self.set_child_offset(Offset::new(self.padding.left, self.padding.top));
            self.set_size(constraints.constrain(child_size + self.padding.size()));
        }
        self.size()
    }
}

// AUTO:
// - HasChild, HasBoxGeometry, HasOffset (via ambassador)
// - paint(), hit_test() (via mixin defaults)
// - RenderProtocol<BoxProtocol> (via blanket impl)
```

### Approach 2: Manual Implementation (полный контроль)

```rust
use flui_rendering::{RenderBox, RenderObject, BoxChild, BoxConstraints, Size, Offset};

pub struct RenderPadding {
    child: BoxChild,
    size: Size,
    padding: EdgeInsets,
}

impl RenderObject for RenderPadding {
    fn attach(&mut self) { self.child.attach(); }
    fn detach(&mut self) { self.child.detach(); }
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        self.child.visit(visitor);
    }
}

impl RenderBox for RenderPadding {
    fn size(&self) -> Size { self.size }
    
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let inner = constraints.deflate(&self.padding);
        if let Some(child) = self.child.get_mut() {
            let child_size = child.layout(&inner);
            self.size = constraints.constrain(child_size + self.padding.size());
        }
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.get() {
            let child_offset = Offset::new(self.padding.left, self.padding.top);
            child.paint(ctx, offset + child_offset);
        }
    }
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child.get() {
            let child_offset = Offset::new(self.padding.left, self.padding.top);
            child.hit_test(result, position - child_offset)
        } else {
            false
        }
    }
    
    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.get()
            .map(|c| c.compute_min_intrinsic_width(height) + self.padding.horizontal())
            .unwrap_or(self.padding.horizontal())
    }
    
    // ... и все остальные методы вручную
}
```

### Comparison

| Aspect | With Mixins | Manual |
|--------|-------------|--------|
| Lines of code | ~20 | ~80+ |
| `RenderObject` impl | AUTO | Manual |
| `paint()` | AUTO (default) | Manual |
| `hit_test()` | AUTO (default) | Manual |
| Intrinsics | AUTO (defaults) | Manual |
| `RenderProtocol` | AUTO (blanket) | Manual or impl RenderBox |
| Flexibility | Override what differs | Full control |
| Custom behavior | Override specific methods | Anything possible |

### When to Use What

**Use Mixins when:**
- Standard layout pattern (proxy, shifted, container, leaf)
- Want minimal boilerplate
- Default paint/hit_test behavior is sufficient
- Only need to customize `perform_layout`

**Use Manual when:**
- Unusual layout protocol
- Complex custom behavior
- Need full control over all methods
- Performance-critical code where you want explicit control
- Learning how the system works

---

## Benefits vs Manual Approach

| Aspect | Manual | Ambassador + Deref |
|--------|--------|-------------------|
| Trait delegation code | Write each impl | `#[delegate(...)]` |
| Field access | `self.data.padding` | `self.padding` via Deref |
| Adding new trait | Add impl everywhere | Add `#[delegate]` line |
| Compile-time check | Manual | Automatic |
| IDE support | Good | Good (proc macro expand) |
| Dependencies | None | +ambassador |

**Boilerplate reduction: ~85-90%**
