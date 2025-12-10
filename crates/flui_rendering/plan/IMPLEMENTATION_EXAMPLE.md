# Implementation Example: RenderAlign (Flutter-like in Rust)

This document shows how to implement RenderObjects following the plan
in `RENDER_STATE_PROTOCOL.md`, `TRAITS_OVERVIEW.md`, and `CHILD_STORAGE.md`.

## Key Architecture Changes

1. **RenderHandle<P, S>** with `Deref` — call methods directly: `child.perform_layout()`
2. **Child<P>** — children stored inside RenderObject (like Flutter)
3. **Base structs** with Deref chain — avoid boilerplate trait implementations

## Flutter Hierarchy

```
RenderObject
    └── RenderBox
            └── RenderShiftedBox              // Single child + offset
                    └── RenderAligningShiftedBox  // + alignment logic
                            └── RenderPositionedBox   // + width/height factors
```

## Rust Translation: Base Structs with Deref

Instead of traits for inheritance, we use **embedded structs with Deref**:

```
SingleChildBase<P>     // child: Child<P>
      │ Deref
      ▼
ShiftedBoxBase<P>      // + child_offset: Offset
      │ Deref
      ▼
AligningBoxBase<P>     // + alignment: Alignment
      │ Deref
      ▼
RenderPositionedBox    // + width_factor, height_factor
```

---

## Step 1: RenderHandle (from handle.rs)

```rust
// ============================================================================
// flui_rendering/src/handle.rs
// ============================================================================

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use flui_tree::{Depth, Mounted, Unmounted, NodeState};

/// Handle for render object with Protocol + NodeState typestate.
/// 
/// Implements Deref so methods can be called directly:
/// ```rust
/// // Instead of: child.render_object_mut().perform_layout(...)
/// // Just:       child.perform_layout(...)
/// ```
pub struct RenderHandle<P: Protocol, S: NodeState> {
    render_object: Box<dyn RenderProtocol<P>>,
    depth: Depth,
    parent: Option<RenderId>,
    _marker: PhantomData<(P, S)>,
}

impl<P: Protocol, S: NodeState> Deref for RenderHandle<P, S> {
    type Target = dyn RenderProtocol<P>;
    fn deref(&self) -> &Self::Target {
        self.render_object.as_ref()
    }
}

impl<P: Protocol, S: NodeState> DerefMut for RenderHandle<P, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.render_object.as_mut()
    }
}

// === Unmounted State ===

impl<P: Protocol> RenderHandle<P, Unmounted> {
    pub fn new<R: RenderProtocol<P> + 'static>(render_object: R) -> Self {
        Self {
            render_object: Box::new(render_object),
            depth: Depth::root(),
            parent: None,
            _marker: PhantomData,
        }
    }
    
    pub fn mount(self, parent: Option<RenderId>, depth: Depth) -> RenderHandle<P, Mounted> {
        RenderHandle {
            render_object: self.render_object,
            depth,
            parent,
            _marker: PhantomData,
        }
    }
}

// === Mounted State ===

impl<P: Protocol> RenderHandle<P, Mounted> {
    pub fn parent(&self) -> Option<RenderId> { self.parent }
    pub fn depth(&self) -> Depth { self.depth }
    
    pub fn unmount(self) -> RenderHandle<P, Unmounted> {
        RenderHandle {
            render_object: self.render_object,
            depth: Depth::root(),
            parent: None,
            _marker: PhantomData,
        }
    }
    
    pub fn attach(&mut self) { self.render_object.attach(); }
    pub fn detach(&mut self) { self.render_object.detach(); }
}

// === Type Aliases ===

pub type BoxHandle<S> = RenderHandle<BoxProtocol, S>;
pub type SliverHandle<S> = RenderHandle<SliverProtocol, S>;
```

---

## Step 2: Child Storage (from children/child.rs)

```rust
// ============================================================================
// flui_rendering/src/children/child.rs
// ============================================================================

use std::ops::{Deref, DerefMut};
use flui_tree::Mounted;

/// Single child storage (Flutter's RenderObjectWithChildMixin).
pub struct Child<P: Protocol> {
    inner: Option<RenderHandle<P, Mounted>>,
}

impl<P: Protocol> Child<P> {
    pub fn new() -> Self { Self { inner: None } }
    
    pub fn with(child: RenderHandle<P, Mounted>) -> Self {
        Self { inner: Some(child) }
    }
    
    pub fn get(&self) -> Option<&RenderHandle<P, Mounted>> { self.inner.as_ref() }
    pub fn get_mut(&mut self) -> Option<&mut RenderHandle<P, Mounted>> { self.inner.as_mut() }
    pub fn set(&mut self, child: Option<RenderHandle<P, Mounted>>) { self.inner = child; }
    pub fn take(&mut self) -> Option<RenderHandle<P, Mounted>> { self.inner.take() }
    
    pub fn is_some(&self) -> bool { self.inner.is_some() }
    pub fn is_none(&self) -> bool { self.inner.is_none() }
    
    // Lifecycle
    pub fn attach(&mut self) {
        if let Some(child) = &mut self.inner { child.attach(); }
    }
    
    pub fn detach(&mut self) {
        if let Some(child) = &mut self.inner { child.detach(); }
    }
    
    pub fn visit(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.inner { visitor(child.deref()); }
    }
}

impl<P: Protocol> Default for Child<P> {
    fn default() -> Self { Self::new() }
}

impl<P: Protocol> Deref for Child<P> {
    type Target = Option<RenderHandle<P, Mounted>>;
    fn deref(&self) -> &Self::Target { &self.inner }
}

impl<P: Protocol> DerefMut for Child<P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}
```

---

## Step 3: Base Structs with Deref Chain

```rust
// ============================================================================
// flui_rendering/src/base/single_child.rs
// ============================================================================

/// Base for single-child render objects.
/// 
/// Provides child storage and lifecycle methods.
pub struct SingleChildBase<P: Protocol> {
    pub child: Child<P>,
    pub size: Size,
}

impl<P: Protocol> SingleChildBase<P> {
    pub fn new() -> Self {
        Self {
            child: Child::new(),
            size: Size::ZERO,
        }
    }
    
    pub fn with_child(child: RenderHandle<P, Mounted>) -> Self {
        Self {
            child: Child::with(child),
            size: Size::ZERO,
        }
    }
}

impl<P: Protocol> RenderObject for SingleChildBase<P> {
    fn attach(&mut self) { self.child.attach(); }
    fn detach(&mut self) { self.child.detach(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) { self.child.visit(v); }
    fn child_count(&self) -> usize { if self.child.is_some() { 1 } else { 0 } }
}
```

```rust
// ============================================================================
// flui_rendering/src/base/shifted_box.rs
// ============================================================================

use std::ops::{Deref, DerefMut};

/// Base for single-child render objects that position child at offset.
/// 
/// Deref chain: ShiftedBoxBase → SingleChildBase
pub struct ShiftedBoxBase<P: Protocol> {
    base: SingleChildBase<P>,
    pub child_offset: Offset,
}

impl<P: Protocol> ShiftedBoxBase<P> {
    pub fn new() -> Self {
        Self {
            base: SingleChildBase::new(),
            child_offset: Offset::ZERO,
        }
    }
}

// Deref to SingleChildBase — access child, size, lifecycle methods
impl<P: Protocol> Deref for ShiftedBoxBase<P> {
    type Target = SingleChildBase<P>;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl<P: Protocol> DerefMut for ShiftedBoxBase<P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

// Default paint for shifted box
impl<P: Protocol> ShiftedBoxBase<P> {
    pub fn paint_child(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.get() {
            child.paint(ctx, offset + self.child_offset);
        }
    }
}
```

```rust
// ============================================================================
// flui_rendering/src/base/aligning_box.rs
// ============================================================================

/// Base for single-child render objects with alignment.
/// 
/// Deref chain: AligningBoxBase → ShiftedBoxBase → SingleChildBase
pub struct AligningBoxBase<P: Protocol> {
    base: ShiftedBoxBase<P>,
    pub alignment: Alignment,
    pub text_direction: Option<TextDirection>,
}

impl<P: Protocol> AligningBoxBase<P> {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: ShiftedBoxBase::new(),
            alignment,
            text_direction: None,
        }
    }
    
    /// Calculate and set child offset based on alignment.
    /// Flutter's alignChild() method.
    pub fn align_child(&mut self, child_size: Size, container_size: Size) {
        let resolved = self.resolved_alignment();
        self.child_offset = resolved.compute_offset(child_size, container_size);
    }
    
    pub fn resolved_alignment(&self) -> Alignment {
        // TODO: handle AlignmentDirectional + text_direction
        self.alignment
    }
}

// Deref to ShiftedBoxBase
impl<P: Protocol> Deref for AligningBoxBase<P> {
    type Target = ShiftedBoxBase<P>;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl<P: Protocol> DerefMut for AligningBoxBase<P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}
```

---

## Step 4: Concrete Implementation (RenderPositionedBox / RenderAlign)

```rust
// ============================================================================
// flui_objects/src/layout/positioned_box.rs
// ============================================================================

use std::ops::{Deref, DerefMut};
use flui_rendering::{
    AligningBoxBase, BoxConstraints, BoxProtocol, Child, PaintingContext,
    Protocol, RenderHandle, RenderObject, RenderProtocol, Size,
};
use flui_types::{Alignment, Offset, Rect};
use flui_tree::Mounted;

/// Positions its child using an Alignment.
/// 
/// Equivalent to Flutter's RenderPositionedBox / Align widget's render object.
/// 
/// # Layout Behavior
/// 
/// - If `width_factor` is set: width = child_width × factor
/// - If `width_factor` is None: expand to fill available width
/// - Same for height
/// 
/// # Example
/// 
/// ```rust
/// // Center child, expand to fill
/// let center = RenderPositionedBox::new(Alignment::CENTER);
/// 
/// // Center child, size = 2× child size
/// let doubled = RenderPositionedBox::with_factors(
///     Alignment::CENTER,
///     Some(2.0),
///     Some(2.0),
/// );
/// ```
#[derive(Debug)]
pub struct RenderPositionedBox {
    /// Base struct providing child, child_offset, alignment, size
    base: AligningBoxBase<BoxProtocol>,
    
    /// Optional width factor (None = expand to fill)
    width_factor: Option<f32>,
    
    /// Optional height factor (None = expand to fill)
    height_factor: Option<f32>,
}

// ============================================================================
// Deref to AligningBoxBase — inherit all base methods
// ============================================================================

impl Deref for RenderPositionedBox {
    type Target = AligningBoxBase<BoxProtocol>;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderPositionedBox {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

// ============================================================================
// Constructors
// ============================================================================

impl RenderPositionedBox {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: AligningBoxBase::new(alignment),
            width_factor: None,
            height_factor: None,
        }
    }
    
    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            base: AligningBoxBase::new(alignment),
            width_factor,
            height_factor,
        }
    }
    
    /// Create centered (most common case).
    pub fn centered() -> Self {
        Self::new(Alignment::CENTER)
    }
}

// ============================================================================
// Property Accessors
// ============================================================================

impl RenderPositionedBox {
    pub fn width_factor(&self) -> Option<f32> { self.width_factor }
    
    pub fn set_width_factor(&mut self, value: Option<f32>) {
        if self.width_factor != value {
            self.width_factor = value;
            // In real impl: self.mark_needs_layout();
        }
    }
    
    pub fn height_factor(&self) -> Option<f32> { self.height_factor }
    
    pub fn set_height_factor(&mut self, value: Option<f32>) {
        if self.height_factor != value {
            self.height_factor = value;
        }
    }
}

// ============================================================================
// RenderObject (delegated via Deref to SingleChildBase)
// ============================================================================

impl RenderObject for RenderPositionedBox {
    fn debug_name(&self) -> &'static str { "RenderPositionedBox" }
    
    // These delegate through Deref chain to SingleChildBase
    fn attach(&mut self) { self.base.attach(); }
    fn detach(&mut self) { self.base.detach(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) {
        self.base.visit_children(v);
    }
    fn child_count(&self) -> usize { self.base.child_count() }
}

// ============================================================================
// Layout Implementation
// ============================================================================

impl RenderProtocol<BoxProtocol> for RenderPositionedBox {
    fn sized_by_parent(&self) -> bool {
        self.width_factor.is_none() && self.height_factor.is_none()
    }
    
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        let shrink_wrap_width = self.width_factor.is_some() || !constraints.has_bounded_width();
        let shrink_wrap_height = self.height_factor.is_some() || !constraints.has_bounded_height();
        
        // Access child through Deref chain: self → AligningBoxBase → ShiftedBoxBase → SingleChildBase → child
        if let Some(child) = self.child.get_mut() {
            // Layout child with loosened constraints
            // Thanks to RenderHandle's Deref, we call perform_layout directly!
            let child_size = child.perform_layout(&constraints.loosen());
            
            // Compute our size based on factors
            let width = if shrink_wrap_width {
                child_size.width * self.width_factor.unwrap_or(1.0)
            } else {
                constraints.max_width
            };
            
            let height = if shrink_wrap_height {
                child_size.height * self.height_factor.unwrap_or(1.0)
            } else {
                constraints.max_height
            };
            
            let size = constraints.constrain(Size::new(width, height));
            
            // Align child (sets child_offset via AligningBoxBase)
            self.align_child(child_size, size);
            
            // Store and return size (via ShiftedBoxBase → SingleChildBase)
            self.size = size;
            size
        } else {
            // No child
            let size = constraints.constrain(Size::new(
                if shrink_wrap_width { 0.0 } else { constraints.max_width },
                if shrink_wrap_height { 0.0 } else { constraints.max_height },
            ));
            self.size = size;
            size
        }
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        // Use ShiftedBoxBase's helper
        self.paint_child(ctx, offset);
    }
    
    fn paint_bounds(&self) -> Rect {
        Rect::from_size(self.size)
    }
    
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Default: hit if within bounds
        let size = self.size;
        if position.x >= 0.0 && position.x < size.width &&
           position.y >= 0.0 && position.y < size.height {
            // Test children first
            if let Some(child) = self.child.get() {
                let child_pos = position - self.child_offset;
                if child.hit_test(result, child_pos) {
                    return true;
                }
            }
            // Add self
            result.add(/* self id */);
            true
        } else {
            false
        }
    }
}

// ============================================================================
// Type Alias
// ============================================================================

/// Alias: RenderAlign = RenderPositionedBox
pub type RenderAlign = RenderPositionedBox;
```

---

## Step 5: ProxyBox Pattern (for effects like Opacity, Transform)

```rust
// ============================================================================
// flui_rendering/src/base/proxy_box.rs
// ============================================================================

/// Base for render objects that delegate everything to child.
/// 
/// Used for visual effects: Opacity, Transform, Clip, etc.
/// 
/// Deref chain: ProxyBoxBase → SingleChildBase
pub struct ProxyBoxBase<P: Protocol> {
    base: SingleChildBase<P>,
}

impl<P: Protocol> ProxyBoxBase<P> {
    pub fn new() -> Self {
        Self { base: SingleChildBase::new() }
    }
}

impl<P: Protocol> Deref for ProxyBoxBase<P> {
    type Target = SingleChildBase<P>;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl<P: Protocol> DerefMut for ProxyBoxBase<P> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

// Default layout: delegate to child
impl<P: Protocol> ProxyBoxBase<P> {
    pub fn proxy_layout(&mut self, constraints: &P::Constraints) -> P::Geometry 
    where
        P::Geometry: Default,
    {
        if let Some(child) = self.child.get_mut() {
            let geometry = child.perform_layout(constraints);
            // Store geometry if applicable
            geometry
        } else {
            P::Geometry::default()
        }
    }
    
    pub fn proxy_paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.get() {
            child.paint(ctx, offset);
        }
    }
}
```

### RenderOpacity Example

```rust
// flui_objects/src/effects/opacity.rs

pub struct RenderOpacity {
    base: ProxyBoxBase<BoxProtocol>,
    alpha: f32,
}

impl Deref for RenderOpacity {
    type Target = ProxyBoxBase<BoxProtocol>;
    fn deref(&self) -> &Self::Target { &self.base }
}

impl DerefMut for RenderOpacity {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.base }
}

impl RenderOpacity {
    pub fn new(alpha: f32) -> Self {
        Self {
            base: ProxyBoxBase::new(),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
    
    pub fn alpha(&self) -> f32 { self.alpha }
    
    pub fn set_alpha(&mut self, value: f32) {
        let value = value.clamp(0.0, 1.0);
        if self.alpha != value {
            self.alpha = value;
            // mark_needs_paint();
        }
    }
}

impl RenderObject for RenderOpacity {
    fn debug_name(&self) -> &'static str { "RenderOpacity" }
    fn attach(&mut self) { self.base.attach(); }
    fn detach(&mut self) { self.base.detach(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) {
        self.base.visit_children(v);
    }
    fn child_count(&self) -> usize { self.base.child_count() }
}

impl RenderProtocol<BoxProtocol> for RenderOpacity {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        // Delegate to child (ProxyBox pattern)
        if let Some(child) = self.child.get_mut() {
            let size = child.perform_layout(constraints);
            self.size = size;
            size
        } else {
            let size = constraints.smallest();
            self.size = size;
            size
        }
    }
    
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if self.alpha == 0.0 {
            return; // Fully transparent, skip painting
        }
        
        if self.alpha == 1.0 {
            // Fully opaque, paint normally
            self.proxy_paint(ctx, offset);
        } else {
            // Apply opacity via compositing layer
            ctx.push_opacity(self.alpha, offset, |ctx| {
                if let Some(child) = self.child.get() {
                    child.paint(ctx, Offset::ZERO);
                }
            });
        }
    }
    
    fn paint_bounds(&self) -> Rect { Rect::from_size(self.size) }
    
    fn always_needs_compositing(&self) -> bool {
        // Need compositing layer for partial opacity
        self.alpha > 0.0 && self.alpha < 1.0
    }
}
```

---

## Summary: Deref Chain Benefits

### Without Deref (Old Approach)

```rust
// Verbose and repetitive
impl RenderObject for RenderPositionedBox {
    fn attach(&mut self) { self.data.base.base.child.attach(); }
    fn detach(&mut self) { self.data.base.base.child.detach(); }
    fn visit_children(&self, v: &mut dyn FnMut(&dyn RenderObject)) {
        self.data.base.base.child.visit(v);
    }
}

impl LayoutProtocol<BoxProtocol> for RenderPositionedBox {
    fn perform_layout(&mut self, constraints: &BoxConstraints, helper: &mut LayoutHelper) -> Size {
        if let Some(child_id) = self.data.base.base.child {
            let child_size = helper.layout_child(child_id, constraints.loosen());
            // ...
        }
    }
}
```

### With Deref Chain (New Approach)

```rust
// Clean and intuitive
impl RenderObject for RenderPositionedBox {
    fn attach(&mut self) { self.base.attach(); }  // Deref handles delegation
    // ...
}

impl RenderProtocol<BoxProtocol> for RenderPositionedBox {
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
        if let Some(child) = self.child.get_mut() {
            // Direct method call via RenderHandle's Deref!
            let child_size = child.perform_layout(&constraints.loosen());
            // ...
        }
    }
}
```

### Access Pattern

```rust
// Through Deref chain: self → AligningBoxBase → ShiftedBoxBase → SingleChildBase → Child<P>

self.child              // Child<P> (from SingleChildBase)
self.child_offset       // Offset (from ShiftedBoxBase)  
self.alignment          // Alignment (from AligningBoxBase)
self.size               // Size (from SingleChildBase)
self.width_factor       // Option<f32> (own field)

// Child method calls via RenderHandle Deref
if let Some(child) = self.child.get_mut() {
    child.perform_layout(...)   // Direct call!
    child.paint(...)            // Direct call!
}
```

---

## Comparison Table

| Aspect | Flutter | Rust FLUI (Old) | Rust FLUI (New) |
|--------|---------|-----------------|-----------------|
| Code reuse | Class inheritance | Composition + traits | Deref chain + Child<P> |
| Child access | `child.layout()` | `helper.layout_child(id)` | `child.perform_layout()` |
| Child storage | Inside RenderObject | Separate tree (IDs) | Inside via Child<P> |
| Base methods | Inherited | Boilerplate delegates | Auto via Deref |
| Type safety | Runtime | Compile-time | Compile-time |
