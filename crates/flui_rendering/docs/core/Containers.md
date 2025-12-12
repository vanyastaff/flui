# Container System

**Type-safe child storage using Protocol associated types**

---

## Overview

FLUI provides generic container types that work with any Protocol. Containers use `Protocol::Object` to store protocol-specific children at compile time, eliminating runtime type checks and downcasts.

---

## Core Container Types

### Single Child Container

Stores zero or one child of the protocol's object type.

```rust
pub struct Single<P: Protocol> {
    child: Option<Box<P::Object>>,
    _phantom: PhantomData<P>,
}

impl<P: Protocol> Single<P> {
    pub fn new() -> Self {
        Self {
            child: None,
            _phantom: PhantomData,
        }
    }
    
    pub fn child(&self) -> Option<&P::Object> {
        self.child.as_deref()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.as_deref_mut()
    }
    
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child = Some(child);
    }
    
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }
    
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }
}
```

**Type Aliases:**
```rust
/// Single Box child
pub type BoxChild = Single<BoxProtocol>;

/// Single Sliver child
pub type SliverChild = Single<SliverProtocol>;
```

**Usage Example:**
```rust
pub struct RenderOpacity {
    child: BoxChild,  // = Single<BoxProtocol>
    opacity: f32,
}

impl RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child.child() {
            // child is &dyn RenderBox - direct access!
            ctx.push_opacity(self.opacity);
            ctx.paint_child(child, offset);
            ctx.pop();
        }
    }
}
```

---

### Multiple Children Container

Stores a vector of children with parent data.

```rust
pub struct Children<P: Protocol, PD: ParentData = P::ParentData> {
    children: Vec<Box<P::Object>>,
    _phantom: PhantomData<(P, PD)>,
}

impl<P: Protocol, PD: ParentData> Children<P, PD> {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            _phantom: PhantomData,
        }
    }
    
    pub fn len(&self) -> usize {
        self.children.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
    
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.children.get(index).map(|b| &**b)
    }
    
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.children.get_mut(index).map(|b| &mut **b)
    }
    
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> {
        self.children.iter().map(|b| &**b)
    }
    
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.children.iter_mut().map(|b| &mut **b)
    }
    
    pub fn push(&mut self, child: Box<P::Object>) {
        self.children.push(child);
    }
    
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.children.insert(index, child);
    }
    
    pub fn remove(&mut self, index: usize) -> Box<P::Object> {
        self.children.remove(index)
    }
    
    pub fn clear(&mut self) {
        self.children.clear();
    }
}
```

**Type Aliases:**
```rust
/// Multiple Box children with BoxParentData
pub type BoxChildren<PD = BoxParentData> = Children<BoxProtocol, PD>;

/// Multiple Sliver children with SliverParentData
pub type SliverChildren<PD = SliverParentData> = Children<SliverProtocol, PD>;
```

**Usage Example:**
```rust
pub struct RenderFlex {
    children: BoxChildren<FlexParentData>,
    direction: Axis,
}

impl RenderFlex {
    fn layout_children(&mut self, constraints: BoxConstraints) {
        for child in self.children.iter_mut() {
            // child is &mut dyn RenderBox
            let child_size = child.perform_layout(child_constraints);
        }
    }
}
```

---

## Specialized Containers

### Proxy Container

Single child where parent's geometry equals child's geometry.

```rust
pub struct Proxy<P: Protocol> {
    child: Single<P>,
    geometry: P::Geometry,
}

impl<P: Protocol> Proxy<P> {
    pub fn new() -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
        }
    }
    
    pub fn child(&self) -> Option<&P::Object> {
        self.child.child()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.child_mut()
    }
    
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child.set_child(child);
    }
    
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }
    
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
}
```

**Type Aliases:**
```rust
/// Box proxy (geometry is Size)
pub type ProxyBox = Proxy<BoxProtocol>;

/// Sliver proxy (geometry is SliverGeometry)
pub type SliverProxy = Proxy<SliverProtocol>;
```

**Usage Example:**
```rust
pub struct RenderOpacity {
    proxy: ProxyBox,  // child + Size
    opacity: f32,
}

impl RenderBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = if let Some(child) = self.proxy.child_mut() {
            child.perform_layout(constraints)
        } else {
            constraints.smallest()
        };
        self.proxy.set_geometry(size);
        size
    }
    
    fn size(&self) -> Size {
        *self.proxy.geometry()
    }
}
```

---

### Shifted Container

Single child with custom offset positioning.

```rust
pub struct Shifted<P: Protocol> {
    child: Single<P>,
    geometry: P::Geometry,
    offset: Offset,
}

impl<P: Protocol> Shifted<P> {
    pub fn new() -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
        }
    }
    
    pub fn child(&self) -> Option<&P::Object> {
        self.child.child()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.child_mut()
    }
    
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }
    
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }
    
    pub fn offset(&self) -> Offset {
        self.offset
    }
    
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}
```

**Type Aliases:**
```rust
pub type ShiftedBox = Shifted<BoxProtocol>;
pub type ShiftedSliver = Shifted<SliverProtocol>;
```

---

### Aligning Container

Single child with alignment and optional size factors.

```rust
pub struct Aligning<P: Protocol> {
    child: Single<P>,
    geometry: P::Geometry,
    offset: Offset,
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
}

impl<P: Protocol> Aligning<P> {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
            alignment,
            width_factor: None,
            height_factor: None,
        }
    }
    
    pub fn child(&self) -> Option<&P::Object> {
        self.child.child()
    }
    
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.child_mut()
    }
    
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }
    
    pub fn offset(&self) -> Offset {
        self.offset
    }
    
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }
    
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
    
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }
    
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        self.width_factor = factor;
    }
    
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }
    
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        self.height_factor = factor;
    }
}
```

**Type Aliases:**
```rust
pub type AligningBox = Aligning<BoxProtocol>;
pub type AligningSliver = Aligning<SliverProtocol>;
```

---

## Container Selection Guide

| Container | Children | Geometry | Offset | Use Cases |
|-----------|----------|----------|--------|-----------|
| `Single<P>` | 0-1 | No | No | Basic wrapping, minimal state |
| `Children<P, PD>` | 0-N | No | No | Multi-child layouts |
| `Proxy<P>` | 0-1 | Yes | No | Pass-through, effects |
| `Shifted<P>` | 0-1 | Yes | Yes | Custom positioning |
| `Aligning<P>` | 0-1 | Yes | Yes | Alignment, size factors |

---

## Container Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Protocol<P>                              │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────┐        │
│  │type Object │  │type Geometry │  │type Parent   │        │
│  │            │  │              │  │    Data      │        │
│  └────────────┘  └──────────────┘  └──────────────┘        │
└─────────────────────────────────────────────────────────────┘
           │                │                │
           ▼                ▼                ▼
┌──────────────────────────────────────────────────────────────┐
│                    Containers                                 │
│                                                               │
│  Single<P>         Proxy<P>          Shifted<P>              │
│  ┌─────────┐      ┌──────────┐      ┌──────────┐            │
│  │ child:  │      │ child:   │      │ child:   │            │
│  │Box<P::  │      │Single<P> │      │Single<P> │            │
│  │Object>  │      │ geometry:│      │ geometry:│            │
│  │         │      │P::Geo    │      │P::Geo    │            │
│  └─────────┘      │          │      │ offset   │            │
│                   └──────────┘      └──────────┘            │
│                                                               │
│  Children<P,PD>    Aligning<P>                               │
│  ┌─────────┐      ┌──────────┐                              │
│  │children:│      │ child:   │                              │
│  │Vec<Box  │      │Single<P> │                              │
│  │<P::Obj>>│      │ geometry │                              │
│  │         │      │ offset   │                              │
│  └─────────┘      │alignment │                              │
│                   └──────────┘                              │
└──────────────────────────────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────────────────────────────┐
│                  RenderObject Impls                           │
│  RenderOpacity { proxy: ProxyBox, ... }                      │
│  RenderPadding { shifted: ShiftedBox, ... }                  │
│  RenderFlex { children: BoxChildren<FlexPD>, ... }           │
└──────────────────────────────────────────────────────────────┘
```

---

## Implementation Patterns

### Pattern 1: Proxy Objects (Size = Child)

Use `Proxy<P>` when parent size equals child size:

```rust
use ambassador::Delegate;

#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderBox for RenderOpacity {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = self.proxy.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(|| constraints.smallest());
        self.proxy.set_geometry(size);
        size
    }
    
    fn size(&self) -> Size {
        *self.proxy.geometry()
    }
}
```

### Pattern 2: Shifted Objects (Custom Position)

Use `Shifted<P>` when parent computes child's offset:

```rust
#[derive(Debug, Delegate)]
#[delegate(SingleChildRenderBox, target = "shifted")]
pub struct RenderPadding {
    shifted: ShiftedBox,
    padding: EdgeInsets,
}

impl RenderBox for RenderPadding {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = constraints.deflate(self.padding);
        
        let child_size = self.shifted.child_mut()
            .map(|c| c.perform_layout(inner))
            .unwrap_or(Size::ZERO);
        
        let size = Size {
            width: child_size.width + self.padding.horizontal(),
            height: child_size.height + self.padding.vertical(),
        };
        
        // Set child offset
        self.shifted.set_offset(Offset {
            dx: self.padding.left,
            dy: self.padding.top,
        });
        
        self.shifted.set_geometry(size);
        size
    }
}
```

### Pattern 3: Multi-Child Layouts

Use `Children<P, PD>` for multiple children:

```rust
#[derive(Debug)]
pub struct RenderFlex {
    children: BoxChildren<FlexParentData>,
    direction: Axis,
    main_axis_alignment: MainAxisAlignment,
}

impl RenderBox for RenderFlex {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let mut total_flex = 0;
        let mut used_space = 0.0;
        
        // Layout flexible children
        for child in self.children.iter_mut() {
            let parent_data = child.parent_data::<FlexParentData>();
            if let Some(flex) = parent_data.flex {
                total_flex += flex;
            } else {
                let size = child.perform_layout(constraints);
                used_space += self.main_size(size);
            }
        }
        
        // ... distribute remaining space
        
        Size::new(width, height)
    }
}
```

---

## File Organization

```
flui-rendering/src/containers/
├── mod.rs                # Re-exports
├── single.rs             # Single<P>
├── children.rs           # Children<P, PD>
├── proxy.rs              # Proxy<P>
├── shifted.rs            # Shifted<P>
└── aligning.rs           # Aligning<P>
```

---

## Type Aliases Convention

Always provide type aliases for ergonomics:

```rust
// In each container file:
pub type ProxyBox = Proxy<BoxProtocol>;
pub type SliverProxy = Proxy<SliverProtocol>;

pub type BoxChildren<PD = BoxParentData> = Children<BoxProtocol, PD>;
pub type SliverChildren<PD = SliverParentData> = Children<SliverProtocol, PD>;
```

---

## Next Steps

- [[Protocol]] - Understanding the foundation
- [[Trait Hierarchy]] - How containers integrate with traits
- [[Object Catalog]] - Example usage in render objects

---

**See Also:**
- [[Parent Data]] - Metadata storage on children
- [[Implementation Guide]] - Step-by-step container usage
