# FLUI Rendering System Documentation

**Complete technical reference for FLUI rendering engine**

---

## What is FLUI?

FLUI is a Flutter-inspired cross-platform UI framework written in Rust. The rendering system uses Protocol-based architecture for compile-time type safety and zero-cost abstractions.

---

## Quick Start

**New to FLUI?** Start here:

1. **[[INDEX.md]]** - Main documentation hub
2. **[[core/Protocol.md]]** - Understand the foundation
3. **[[traits/Trait Hierarchy.md]]** - Learn the trait system
4. **[[reference/Implementation Guide.md]]** - Create your first object

---

## Key Concepts

### Protocol System
FLUI uses `Protocol` trait to define type systems for render object families. Each protocol specifies four associated types:

- **Object** - What type of children to store (`dyn RenderBox`, `dyn RenderSliver`)
- **Constraints** - Layout input (`BoxConstraints`, `SliverConstraints`)
- **ParentData** - Child metadata (`BoxParentData`, etc.)
- **Geometry** - Layout output (`Size`, `SliverGeometry`)

### Type-Safe Containers
Containers use `Protocol::Object` for automatic type selection:

```rust
pub struct Proxy<P: Protocol> {
    child: Single<P>,           // Uses P::Object automatically
    geometry: P::Geometry,      // Size or SliverGeometry
}
```

### Trait Inheritance
Implement ONE trait, get ALL ancestors via blanket implementations:

```rust
impl RenderProxyBox for RenderOpacity {}

// Automatically provides:
// ✅ SingleChildRenderBox
// ✅ RenderBox  
// ✅ RenderObject
```

---

## Architecture Overview

```
Protocol<P>
    ↓ (defines types)
Traits (RenderBox, RenderSliver)
    ↓ (define behavior)
Containers (Proxy, Children, etc.)
    ↓ (store children)
Objects (85 render objects)
    ↓ (concrete implementations)
Pipeline (frame production)
```

---

## Documentation Structure

```
flui-docs/
├── INDEX.md                         # Main navigation hub
├── README.md                        # This file
│
├── core/
│   ├── Protocol.md                  # Protocol trait system
│   └── Containers.md                # Container types
│
├── traits/
│   └── Trait Hierarchy.md           # Complete trait tree
│
├── objects/
│   └── Object Catalog.md            # 85 render objects
│
├── pipeline/
│   └── Pipeline.md                  # Frame production
│
└── reference/
    ├── Parent Data.md               # 15 parent data types
    └── Implementation Guide.md      # How to create objects
```

---

## Statistics

| Component | Count |
|-----------|-------|
| Protocols | 2 (Box, Sliver) |
| Traits | 24 (13 Box + 11 Sliver) |
| Containers | 5 (Single, Children, Proxy, Shifted, Aligning) |
| Render Objects | 85 (60 Box + 25 Sliver) |
| Parent Data Types | 15 |
| Categories | 13 functional |
| Layer Types | 15 |
| Total Files | ~150 |

---

## Learning Path

### Beginner
1. Read [[core/Protocol.md]] - Foundation
2. Read [[core/Containers.md]] - Child storage
3. Browse [[objects/Object Catalog.md]] - Examples

### Intermediate
4. Read [[traits/Trait Hierarchy.md]] - Trait system
5. Read [[pipeline/Pipeline.md]] - Frame production
6. Read [[reference/Parent Data.md]] - Metadata

### Advanced
7. Read [[reference/Implementation Guide.md]] - Create objects
8. Study complex objects (RenderFlex, RenderTable)
9. Optimize with relayout/repaint boundaries

---

## Quick Reference

### Box Protocol
- **Constraints**: `BoxConstraints` (min/max width/height)
- **Geometry**: `Size` (width, height)
- **Use For**: Static layouts, flex layouts, effects

### Sliver Protocol
- **Constraints**: `SliverConstraints` (scroll offset, viewport)
- **Geometry**: `SliverGeometry` (scroll/paint extents)
- **Use For**: Scrollable lists, grids, lazy loading

### Container Selection
- **Proxy<P>**: Size equals child (effects, transformations)
- **Shifted<P>**: Custom positioning (padding, baseline)
- **Aligning<P>**: Alignment + size factors (Center, Align)
- **Children<P, PD>**: Multiple children (Row, Column, Stack)

### Trait Selection
- **RenderProxyBox**: Pass-through to child
- **RenderShiftedBox**: Custom child positioning
- **RenderAligningShiftedBox**: Alignment support
- **MultiChildRenderBox**: Multiple children

---

## Implementation Example

```rust
use ambassador::Delegate;

#[derive(Debug, Delegate)]
#[delegate(RenderProxyBox, target = "proxy")]
pub struct RenderOpacity {
    proxy: ProxyBox,
    opacity: f32,
}

impl RenderProxyBox for RenderOpacity {}

impl RenderBox for RenderOpacity {
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.proxy.child() {
            if self.opacity < 1.0 {
                ctx.push_opacity(self.opacity, |c| c.paint_child(child, offset));
            } else {
                ctx.paint_child(child, offset);
            }
        }
    }
}
```

**Total**: ~30 lines for full render object with all capabilities.

---

## File Organization

```
flui-rendering/src/
├── protocol.rs
├── constraints/
├── geometry/
├── parent_data/
├── containers/
├── traits/
│   ├── render_object.rs
│   ├── box/
│   └── sliver/
├── objects/
│   ├── box/
│   │   ├── basic/       (6)
│   │   ├── layout/      (15)
│   │   ├── effects/     (11)
│   │   ├── animation/   (4)
│   │   ├── interaction/ (6)
│   │   ├── gestures/    (4)
│   │   ├── media/       (3)
│   │   ├── text/        (2)
│   │   ├── accessibility/ (5)
│   │   ├── platform/    (2)
│   │   ├── scroll/      (4)
│   │   └── debug/       (2)
│   └── sliver/
│       ├── basic/       (5)
│       ├── layout/      (11)
│       ├── effects/     (3)
│       ├── interaction/ (1)
│       └── scroll/      (5)
└── pipeline/
```

---

## Key Benefits

### Type Safety
- ✅ Compile-time protocol enforcement
- ✅ Zero downcasts needed
- ✅ Direct method access
- ✅ Runtime errors become compile errors

### Performance
- ✅ Zero-cost abstractions
- ✅ Enum dispatch for closed sets
- ✅ Efficient dirty tracking
- ✅ GPU-accelerated compositing

### Developer Experience
- ✅ Rust-idiomatic naming
- ✅ 13 functional categories
- ✅ Automatic trait inheritance
- ✅ Minimal boilerplate (~50 lines per object)

---

## Support

**Documentation**: All documents in this package  
**Start Here**: [[INDEX.md]]

---

**Version**: 1.0  
**Last Updated**: December 2024  
**Status**: Complete specification
