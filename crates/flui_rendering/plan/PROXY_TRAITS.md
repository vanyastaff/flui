# Proxy Traits

Flutter-подобные proxy structs с auto-implementation и zero boilerplate.

## Overview

| Type | Protocol | Mixin | Blanket |
|------|----------|-------|---------|
| `ProxyBox<T>` | `BoxProtocol` | `RenderProxyBoxMixin` | `RenderProtocol<BoxProtocol>` |
| `ProxySliver<T>` | `SliverProtocol` | `RenderProxySliverMixin` | `RenderProtocol<SliverProtocol>` |

## Architecture

```
Proxy<T, P: Protocol>
    │ Deref
    ▼
ProxyInner<T, P>
    │ Deref              │ methods
    ▼                    ▼
    T (data fields)   child(), geometry(), set_geometry()
```

**Доступ:**
- `self.ignoring` — поля из T через Deref chain
- `self.child()` — метод ProxyInner
- `self.geometry()` — метод ProxyInner

---

## Core Types

### ProxyData Trait

```rust
/// Trait bounds for proxy data.
/// 
/// All data types must implement these traits for:
/// - `Default` — создание пустого proxy
/// - `Clone` — копирование состояния
/// - `Debug` — отладка
/// - `'static` — хранение в trait objects
pub trait ProxyData: Default + Clone + Debug + 'static {}

// Auto-impl for all matching types
impl<T: Default + Clone + Debug + 'static> ProxyData for T {}
```

### ProxyBase

```rust
/// Base proxy state — child + geometry.
#[derive(Debug)]
pub struct ProxyBase<P: Protocol> {
    pub child: Child<P>,
    pub geometry: P::Geometry,
}

impl<P: Protocol> Default for ProxyBase<P>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            child: Child::new(),
            geometry: P::Geometry::default(),
        }
    }
}
```

### ProxyInner

```rust
/// Inner proxy — holds base + custom data.
/// 
/// Deref to T for direct field access.
#[derive(Debug)]
pub struct ProxyInner<T: ProxyData, P: Protocol = BoxProtocol> {
    base: ProxyBase<P>,
    data: T,
}

impl<T: ProxyData, P: Protocol> ProxyInner<T, P> {
    pub fn new() -> Self
    where
        P::Geometry: Default,
    {
        Self {
            base: ProxyBase::default(),
            data: T::default(),
        }
    }

    pub fn with_data(data: T) -> Self
    where
        P::Geometry: Default,
    {
        Self {
            base: ProxyBase::default(),
            data,
        }
    }

    // === Convenience Methods ===

    pub fn child(&self) -> &Child<P> { &self.base.child }
    pub fn child_mut(&mut self) -> &mut Child<P> { &mut self.base.child }
    pub fn geometry(&self) -> &P::Geometry { &self.base.geometry }
    pub fn set_geometry(&mut self, geometry: P::Geometry) { self.base.geometry = geometry; }
}

// Deref → T (direct field access)
impl<T: ProxyData, P: Protocol> Deref for ProxyInner<T, P> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.data }
}

impl<T: ProxyData, P: Protocol> DerefMut for ProxyInner<T, P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.data }
}
```

### Proxy

```rust
/// Generic proxy render object.
/// 
/// Delegates all behavior to single child.
/// Override only methods that differ.
#[derive(Debug)]
pub struct Proxy<T: ProxyData, P: Protocol = BoxProtocol> {
    inner: ProxyInner<T, P>,
}

impl<T: ProxyData, P: Protocol> Proxy<T, P> {
    pub fn new() -> Self
    where
        P::Geometry: Default,
    {
        Self { inner: ProxyInner::new() }
    }

    pub fn with_data(data: T) -> Self
    where
        P::Geometry: Default,
    {
        Self { inner: ProxyInner::with_data(data) }
    }
}

// Deref → ProxyInner
impl<T: ProxyData, P: Protocol> Deref for Proxy<T, P> {
    type Target = ProxyInner<T, P>;
    fn deref(&self) -> &Self::Target { &self.inner }
}

impl<T: ProxyData, P: Protocol> DerefMut for Proxy<T, P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}

// Display — delegates to T if Display
impl<T: ProxyData + Display, P: Protocol> Display for Proxy<T, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Proxy({})", &self.inner.data)
    }
}
```

---

## Type Aliases

```rust
/// Box proxy — most common case.
pub type ProxyBox<T> = Proxy<T, BoxProtocol>;

/// Sliver proxy — for scrollable content.
pub type ProxySliver<T> = Proxy<T, SliverProtocol>;
```

---

## Auto-Implementations

### ProxyBehavior

```rust
/// Generic proxy behavior — child access.
pub trait ProxyBehavior<P: Protocol> {
    fn child(&self) -> &Child<P>;
    fn child_mut(&mut self) -> &mut Child<P>;
}

// Auto-impl for all Proxy types
impl<T: ProxyData, P: Protocol> ProxyBehavior<P> for Proxy<T, P> {
    fn child(&self) -> &Child<P> { self.inner.child() }
    fn child_mut(&mut self) -> &mut Child<P> { self.inner.child_mut() }
}
```

### RenderObject

```rust
// Auto-impl RenderObject for all Proxy types
impl<T: ProxyData, P: Protocol> RenderObject for Proxy<T, P> {
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }

    fn attach(&mut self) {
        self.inner.base.child.attach();
    }

    fn detach(&mut self) {
        self.inner.base.child.detach();
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        self.inner.base.child.visit(visitor);
    }

    fn child_count(&self) -> usize {
        if self.inner.base.child.is_some() { 1 } else { 0 }
    }
}
```

### RenderProxyBoxMixin

```rust
/// Box-specific proxy mixin.
pub trait RenderProxyBoxMixin: RenderObject + ProxyBehavior<BoxProtocol> {
    fn size(&self) -> Size;
    fn set_size(&mut self, size: Size);

    // All methods have defaults that delegate to child...
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

// Auto-impl for ProxyBox<T>
impl<T: ProxyData> RenderProxyBoxMixin for ProxyBox<T> {
    fn size(&self) -> Size { self.inner.base.geometry }
    fn set_size(&mut self, size: Size) { self.inner.base.geometry = size; }
}

// Blanket impl: RenderProxyBoxMixin → RenderProtocol<BoxProtocol>
impl<T: RenderProxyBoxMixin> RenderProtocol<BoxProtocol> for T {
    fn compute_min_intrinsic_width(&self, h: f32) -> f32 { RenderProxyBoxMixin::compute_min_intrinsic_width(self, h) }
    fn compute_max_intrinsic_width(&self, h: f32) -> f32 { RenderProxyBoxMixin::compute_max_intrinsic_width(self, h) }
    fn compute_min_intrinsic_height(&self, w: f32) -> f32 { RenderProxyBoxMixin::compute_min_intrinsic_height(self, w) }
    fn compute_max_intrinsic_height(&self, w: f32) -> f32 { RenderProxyBoxMixin::compute_max_intrinsic_height(self, w) }
    fn compute_dry_layout(&self, c: &BoxConstraints) -> Size { RenderProxyBoxMixin::compute_dry_layout(self, c) }
    fn get_distance_to_baseline(&self, b: TextBaseline) -> Option<f32> { RenderProxyBoxMixin::get_distance_to_baseline(self, b) }
    fn perform_layout(&mut self, c: &BoxConstraints) -> Size { RenderProxyBoxMixin::perform_layout(self, c) }
    fn paint(&self, ctx: &mut PaintingContext, o: Offset) { RenderProxyBoxMixin::paint(self, ctx, o) }
    fn hit_test(&self, r: &mut BoxHitTestResult, p: Offset) -> bool { RenderProxyBoxMixin::hit_test(self, r, p) }
    fn always_needs_compositing(&self) -> bool { RenderProxyBoxMixin::always_needs_compositing(self) }
    fn is_repaint_boundary(&self) -> bool { RenderProxyBoxMixin::is_repaint_boundary(self) }
}
```

### RenderProxySliverMixin

```rust
/// Sliver-specific proxy mixin.
pub trait RenderProxySliverMixin: RenderObject + ProxyBehavior<SliverProtocol> {
    fn geometry(&self) -> &SliverGeometry;
    fn set_geometry(&mut self, geometry: SliverGeometry);

    // All methods have defaults that delegate to child...
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry { ... }
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) { ... }
    fn hit_test(&self, result: &mut SliverHitTestResult, main: f32, cross: f32) -> bool { ... }
    fn always_needs_compositing(&self) -> bool { false }
    fn is_repaint_boundary(&self) -> bool { false }
}

// Auto-impl for ProxySliver<T>
impl<T: ProxyData> RenderProxySliverMixin for ProxySliver<T> {
    fn geometry(&self) -> &SliverGeometry { &self.inner.base.geometry }
    fn set_geometry(&mut self, geometry: SliverGeometry) { self.inner.base.geometry = geometry; }
}

// Blanket impl: RenderProxySliverMixin → RenderProtocol<SliverProtocol>
impl<T: RenderProxySliverMixin> RenderProtocol<SliverProtocol> for T { ... }
```

---

## Serde Support (Optional)

```rust
#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

/// Extended trait bounds with serde support.
#[cfg(feature = "serde")]
pub trait ProxyData: Default + Clone + Debug + Serialize + for<'de> Deserialize<'de> + 'static {}

#[cfg(feature = "serde")]
impl<T> ProxyData for T
where
    T: Default + Clone + Debug + Serialize + for<'de> Deserialize<'de> + 'static
{}

// Proxy derives Serialize/Deserialize when feature enabled
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ProxyInner<T: ProxyData, P: Protocol = BoxProtocol> {
    #[cfg_attr(feature = "serde", serde(skip))]
    base: ProxyBase<P>,  // Skip child — not serializable
    data: T,
}
```

---

## Usage Examples

### RenderOpacity (Box)

```rust
#[derive(Default, Clone, Debug)]
pub struct OpacityData {
    pub alpha: f32,
}

pub type RenderOpacity = ProxyBox<OpacityData>;

impl RenderOpacity {
    pub fn new(alpha: f32) -> Self {
        Proxy::with_data(OpacityData { alpha: alpha.clamp(0.0, 1.0) })
    }
}

// Override only paint!
impl RenderProxyBoxMixin for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        match self.alpha {
            a if a == 0.0 => {},
            a if a == 1.0 => {
                if let Some(c) = self.child().get() { c.paint(ctx, offset); }
            }
            a => ctx.push_opacity(a, offset, |ctx| {
                if let Some(c) = self.child().get() { c.paint(ctx, Offset::ZERO); }
            }),
        }
    }

    fn always_needs_compositing(&self) -> bool {
        self.alpha > 0.0 && self.alpha < 1.0
    }
}

// AUTO: RenderObject, ProxyBehavior, RenderProtocol<BoxProtocol>
```

### RenderIgnorePointer (Box)

```rust
#[derive(Default, Clone, Debug)]
pub struct IgnorePointerData {
    pub ignoring: bool,
}

pub type RenderIgnorePointer = ProxyBox<IgnorePointerData>;

impl RenderIgnorePointer {
    pub fn new(ignoring: bool) -> Self {
        Proxy::with_data(IgnorePointerData { ignoring })
    }
}

// Override only hit_test!
impl RenderProxyBoxMixin for RenderIgnorePointer {
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        !self.ignoring && self.child()
            .get()
            .map(|c| c.hit_test(result, position))
            .unwrap_or(false)
    }
}

// AUTO: RenderObject, ProxyBehavior, RenderProtocol<BoxProtocol>
```

### RenderRepaintBoundary (Box)

```rust
#[derive(Default, Clone, Debug)]
pub struct RepaintBoundaryData;

pub type RenderRepaintBoundary = ProxyBox<RepaintBoundaryData>;

impl RenderRepaintBoundary {
    pub fn new() -> Self {
        Proxy::new()
    }
}

// Override only compositing flags!
impl RenderProxyBoxMixin for RenderRepaintBoundary {
    fn is_repaint_boundary(&self) -> bool { true }
    fn always_needs_compositing(&self) -> bool { true }
}

// AUTO: RenderObject, ProxyBehavior, RenderProtocol<BoxProtocol>
// Layout, paint, hit_test all delegate automatically!
```

### RenderSliverOpacity (Sliver)

```rust
#[derive(Default, Clone, Debug)]
pub struct SliverOpacityData {
    pub alpha: f32,
}

pub type RenderSliverOpacity = ProxySliver<SliverOpacityData>;

impl RenderSliverOpacity {
    pub fn new(alpha: f32) -> Self {
        Proxy::with_data(SliverOpacityData { alpha: alpha.clamp(0.0, 1.0) })
    }
}

impl RenderProxySliverMixin for RenderSliverOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if self.alpha > 0.0 {
            ctx.push_opacity(self.alpha, offset, |ctx| {
                if let Some(c) = self.child().get() { c.paint(ctx, Offset::ZERO); }
            });
        }
    }

    fn always_needs_compositing(&self) -> bool {
        self.alpha > 0.0 && self.alpha < 1.0
    }
}

// AUTO: RenderObject, ProxyBehavior, RenderProtocol<SliverProtocol>
```

---

## What's Auto-Implemented

| Trait | ProxyBox<T> | ProxySliver<T> |
|-------|-------------|----------------|
| `ProxyBehavior<P>` | ✓ | ✓ |
| `RenderObject` | ✓ | ✓ |
| `RenderProxyBoxMixin` | ✓ (override as needed) | — |
| `RenderProxySliverMixin` | — | ✓ (override as needed) |
| `RenderProtocol<BoxProtocol>` | ✓ (blanket) | — |
| `RenderProtocol<SliverProtocol>` | — | ✓ (blanket) |
| `Debug` | ✓ (if T: Debug) | ✓ (if T: Debug) |
| `Display` | ✓ (if T: Display) | ✓ (if T: Display) |
| `Serialize` | ✓ (feature: serde) | ✓ (feature: serde) |
| `Deserialize` | ✓ (feature: serde) | ✓ (feature: serde) |

---

## File Organization

```
crates/flui_rendering/src/
├── base/
│   ├── mod.rs           # Re-exports
│   ├── data.rs          # ProxyData trait
│   └── proxy.rs         # ProxyBase, ProxyInner, Proxy<T, P>, ProxyBox<T>, ProxySliver<T>
├── mixins/
│   ├── mod.rs           # Re-exports all mixins
│   └── proxy.rs         # RenderProxyBoxMixin, RenderProxySliverMixin + blanket impls
```

---

## Benefits

| Aspect | Without Proxy | With ProxyBox<T> |
|--------|---------------|------------------|
| Struct definition | Manual fields | `type X = ProxyBox<Data>` |
| `RenderObject` impl | ~15 lines | AUTO |
| `ProxyBehavior` impl | ~5 lines | AUTO |
| `RenderProxyBoxMixin` impl | ALL methods | Override only needed |
| `RenderProtocol` impl | ALL methods | AUTO (blanket) |
| Field access | `self.field` | `self.field` (via Deref) |
| Child access | manual | `self.child()` |
| Debug/Display | manual | AUTO (if T impl) |
| Serde | manual | AUTO (feature flag) |

**Total boilerplate reduction: ~90%**
