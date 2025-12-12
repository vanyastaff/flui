# Protocol System

**Core abstraction for render object protocols in FLUI**

---

## Overview

The Protocol trait defines the type system for render object families. Each protocol specifies four associated types that determine how layout, constraints, and children work within that protocol's domain.

## Protocol Trait

```rust
pub trait Protocol: Send + Sync + Debug + 'static {
    /// The type of render objects this protocol contains
    type Object: RenderObject + ?Sized;
    
    /// Layout input type (passed down from parent)
    type Constraints: Clone + Debug;
    
    /// Child metadata type (stored on each child)
    type ParentData: ParentData;
    
    /// Layout output type (returned from layout)
    type Geometry: Clone + Debug;
    
    /// Default geometry value for uninitialized state
    fn default_geometry() -> Self::Geometry;
    
    /// Protocol name for debugging
    fn name() -> &'static str;
}
```

### Associated Type Purposes

| Type | Direction | Purpose | Example |
|------|-----------|---------|---------|
| `Object` | Compile-time | Child storage type | `dyn RenderBox` |
| `Constraints` | Parent → Child | Layout input | `BoxConstraints` |
| `ParentData` | Parent → Child | Child metadata | `BoxParentData` |
| `Geometry` | Child → Parent | Layout output | `Size` |

---

## Protocol Implementations

### BoxProtocol

2D cartesian layout with rectangular constraints.

```rust
#[derive(Debug, Clone, Copy)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Object = dyn RenderBox;
    type Constraints = BoxConstraints;
    type ParentData = BoxParentData;
    type Geometry = Size;
    
    fn default_geometry() -> Size {
        Size::ZERO
    }
    
    fn name() -> &'static str {
        "box"
    }
}
```

**BoxConstraints Structure:**
```rust
pub struct BoxConstraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}
```

**Size Structure:**
```rust
pub struct Size {
    pub width: f32,
    pub height: f32,
}
```

### SliverProtocol

Scrollable content layout with viewport-aware constraints.

```rust
#[derive(Debug, Clone, Copy)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Object = dyn RenderSliver;
    type Constraints = SliverConstraints;
    type ParentData = SliverParentData;
    type Geometry = SliverGeometry;
    
    fn default_geometry() -> SliverGeometry {
        SliverGeometry::zero()
    }
    
    fn name() -> &'static str {
        "sliver"
    }
}
```

**SliverConstraints Structure:**
```rust
pub struct SliverConstraints {
    pub axis_direction: AxisDirection,
    pub growth_direction: GrowthDirection,
    pub user_scroll_direction: ScrollDirection,
    pub scroll_offset: f32,
    pub preceding_scroll_extent: f32,
    pub overlap: f32,
    pub remaining_paint_extent: f32,
    pub cross_axis_extent: f32,
    pub cross_axis_direction: AxisDirection,
    pub viewport_main_axis_extent: f32,
    pub remaining_cache_extent: f32,
    pub cache_origin: f32,
}
```

**SliverGeometry Structure:**
```rust
pub struct SliverGeometry {
    pub scroll_extent: f32,
    pub paint_extent: f32,
    pub paint_origin: f32,
    pub layout_extent: Option<f32>,
    pub max_paint_extent: f32,
    pub max_scroll_obstruction_extent: f32,
    pub hit_test_extent: Option<f32>,
    pub visible: Option<bool>,
    pub has_visual_overflow: bool,
    pub scroll_offset_correction: Option<f32>,
    pub cache_extent: Option<f32>,
}
```

---

## Type Flow Diagram

```
                    Protocol Trait
                         │
           ┌─────────────┼─────────────┐
           ▼             ▼             ▼
    type Object   type Constraints  type Geometry
           │             │             │
           │             │             │
           ▼             ▼             ▼
    ┌──────────┐   ┌──────────┐   ┌──────────┐
    │Container │   │  Layout  │   │  Layout  │
    │  Storage │   │   Input  │   │  Output  │
    └──────────┘   └──────────┘   └──────────┘
           │             │             │
           │             │             │
           ▼             ▼             ▼
    
    Single<P>     performLayout()   return Size
    Children<P>   ┌─────────────┐   or Geometry
    Proxy<P>      │constraints: │
    etc.          │P::Constraints│
                  └─────────────┘
```

---

## Protocol Benefits

### 1. Type-Safe Child Storage

Containers automatically select the correct child type:

```rust
pub struct Single<P: Protocol> {
    child: Option<Box<P::Object>>,
}

// Type aliases expand correctly:
type BoxChild = Single<BoxProtocol>;
// → child: Option<Box<dyn RenderBox>>

type SliverChild = Single<SliverProtocol>;
// → child: Option<Box<dyn RenderSliver>>
```

**Direct Method Access:**
```rust
impl RenderFlex {
    fn layout_children(&mut self, constraints: BoxConstraints) {
        for child in self.children.iter_mut() {
            // child is &mut dyn RenderBox - no casting needed!
            let size = child.perform_layout(constraints);
        }
    }
}
```

### 2. Compile-Time Protocol Enforcement

Cannot mix protocols at compile time:

```rust
// ✅ Compiles - correct protocol
let proxy: Proxy<BoxProtocol> = Proxy::new();
proxy.set_child(Box::new(RenderPadding::new()));

// ❌ Compile error - wrong protocol
let proxy: Proxy<BoxProtocol> = Proxy::new();
proxy.set_child(Box::new(RenderSliverList::new()));
//                        ^^^^^^^^^^^^^^^^^^^
// Error: expected dyn RenderBox, found dyn RenderSliver
```

### 3. Generic Container Reuse

One container implementation works for all protocols:

```rust
pub struct Proxy<P: Protocol> {
    child: Single<P>,
    geometry: P::Geometry,  // Automatically Size or SliverGeometry
}

// Type aliases provide ergonomics:
pub type ProxyBox = Proxy<BoxProtocol>;
pub type SliverProxy = Proxy<SliverProtocol>;
```

---

## Protocol Selection Guide

| Use Case | Protocol | Reason |
|----------|----------|--------|
| Fixed size widgets | Box | Simple 2D layout |
| Flexible layouts (Row/Column) | Box | Constraint-based sizing |
| Scrollable lists | Sliver | Lazy rendering, viewport-aware |
| Infinite scrolling | Sliver | Only renders visible items |
| Nested scrolling | Sliver | Scroll extent composition |
| Grid layouts | Box or Sliver | Box for fixed, Sliver for scrolling |

---

## Protocol Constraints Comparison

| Feature | BoxConstraints | SliverConstraints |
|---------|----------------|-------------------|
| **Input Type** | Min/max dimensions | Scroll position + viewport |
| **Output Type** | Size (width, height) | SliverGeometry (extents) |
| **Layout Model** | Fill available space | Extend along scroll axis |
| **Lazy Rendering** | No | Yes (viewport-based) |
| **Scroll Awareness** | No | Yes (scroll offset) |
| **Typical Use** | Static layouts | Scrollable content |

---

## Implementation Requirements

### For Each Protocol

1. **Define constraint type** with necessary layout information
2. **Define geometry type** for layout results
3. **Define parent data type** for child metadata
4. **Implement Protocol trait** with all associated types
5. **Create trait object type** (e.g., `dyn RenderBox`)

### Type Safety Rules

1. **Object type** must be `RenderObject + ?Sized`
2. **Constraints type** must be `Clone + Debug`
3. **Geometry type** must be `Clone + Debug`
4. **ParentData type** must implement `ParentData` trait
5. All types must be `Send + Sync + 'static`

---

## File Organization

```
flui-rendering/src/
├── protocol.rs              # Protocol trait + implementations
├── constraints/
│   ├── box_constraints.rs   # BoxConstraints
│   └── sliver_constraints.rs # SliverConstraints
├── geometry/
│   ├── size.rs              # Size (Box geometry)
│   └── sliver_geometry.rs   # SliverGeometry
└── parent_data/
    ├── box_parent_data.rs   # BoxParentData
    └── sliver_parent_data.rs # SliverParentData
```

---

## Next Steps

- [[Containers]] - How to use Protocol with generic containers
- [[Trait Hierarchy]] - Complete trait system built on Protocol
- [[Object Catalog]] - All render objects organized by protocol

---

**See Also:**
- [[Blanket Implementations]] - Automatic trait inheritance
- [[Implementation Guide]] - Step-by-step object creation
