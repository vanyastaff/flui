# FLUI Rendering System - Technical Documentation

**Rust implementation of Flutter-inspired rendering engine**

---

## Documentation Structure

### Core System
- **[[Protocol]]** - Protocol trait with Object, Geometry, Constraints, ParentData (core/Protocol.md)
- **[[Containers]]** - Type-safe container implementations (Single, Children, Proxy, Shifted, Aligning) (core/Containers.md)
- **[[Lifecycle]]** - Render object lifecycle states (Detached → Painted → Disposed) (core/Lifecycle.md)
- **[[Delegation Pattern]]** - Ambassador-based trait delegation (core/Delegation Pattern.md)
- **[[Render Tree]]** - Tree structure, attach/detach, parent-child relationships (core/Render Tree.md)

### Trait System
- **[[Trait Hierarchy]]** - Complete inheritance tree (RenderObject → RenderBox/RenderSliver) (traits/Trait Hierarchy.md)
- Trait Definitions - All 24 traits with method signatures (see Trait Hierarchy.md)
- Delegation Pattern - Ambassador-based trait delegation (see Delegation Pattern.md)

### Render Objects  
- **[[Object Catalog]]** - All 85 render objects organized by 13 categories (objects/Object Catalog.md)
- Box Objects - 60 objects for 2D layout (basic, layout, effects, animation, interaction, etc.)
- Sliver Objects - 25 objects for scrollable content

### Pipeline & Frame Production
- **[[Pipeline]]** - Frame production phases (layout, compositing, paint, semantics) (pipeline/Pipeline.md)
- PipelineOwner - Dirty node management and flush operations
- Dirty Tracking - Incremental update system
- Layer System - Compositing layer hierarchy (15 layer types)

### Implementation Reference
- **[[Parent Data]]** - 15 parent data types for metadata storage (reference/Parent Data.md)
- **[[Delegates]]** - 6 delegate traits (CustomPainter, FlowDelegate, etc.) (reference/Delegates.md)
- **[[Implementation Guide]]** - Step-by-step implementation instructions (reference/Implementation Guide.md)
- **[[File Organization]]** - Complete project structure (~197 files) (reference/File Organization.md)
- API Reference - Method signatures and type definitions (see Trait Hierarchy.md)

---

## Quick Navigation

### Start Here
1. [[Protocol]] - Understand the foundation
2. [[Trait Hierarchy]] - See the complete structure
3. [[Object Catalog]] - Browse all render objects

### For Implementation
1. [[Containers]] - How to create containers
2. [[Trait Definitions]] - What traits to implement (see Trait Hierarchy.md)
3. [[Delegation Pattern]] - How automatic inheritance works
4. [[Implementation Guide]] - Step-by-step instructions

### System Architecture
1. [[Protocol]] - Type system foundation
2. [[Render Tree]] - Tree structure and lifecycle
3. [[Pipeline]] - Frame production phases
4. [[Delegates]] - Custom behavior delegates

### Reference
1. [[Parent Data]] - Metadata types
2. [[File Organization]] - Complete project structure
3. [[Object Catalog]] - All 85 render objects

---

## System Overview

### Protocol System
FLUI uses associated types to provide compile-time type safety across protocols:

```rust
pub trait Protocol {
    type Object: RenderObject + ?Sized;  // What objects this protocol contains
    type Constraints;                     // Input to layout
    type ParentData;                      // Metadata on children
    type Geometry;                        // Output from layout
}
```

### Two Main Protocols
- **BoxProtocol**: 2D rectangular layout (Object = dyn RenderBox)
- **SliverProtocol**: Scrollable content (Object = dyn RenderSliver)

### Object Categories (13 functional groups)
- basic, layout, effects, animation, interaction, gestures
- media, text, accessibility, platform, scroll, debug, advanced

### Pipeline Phases
1. Layout - Compute sizes and positions
2. Compositing Bits - Determine layer requirements  
3. Paint - Generate display lists
4. Semantics - Build accessibility tree

---

## Statistics

| Component | Count |
|-----------|-------|
| Protocols | 2 (Box, Sliver) |
| Protocol Types | 4 (Object, Constraints, ParentData, Geometry) |
| Traits | 24 (13 Box + 11 Sliver) |
| Containers | 5 (Single, Children, Proxy, Shifted, Aligning) |
| Categories | 13 functional |
| Render Objects | 85 (60 Box + 25 Sliver) |
| Parent Data Types | 15 |
| Delegates | 6 |
| Layer Types | 15 |
| Mixins (from Flutter) | ~17 |
| Total Files | ~150 |

---

## Key Files

- `protocol.rs` - Protocol trait and implementations
- `containers/*.rs` - Generic container types
- `traits/*.rs` - 24 delegatable traits
- `objects/box/*.rs` - 60 Box render objects
- `objects/sliver/*.rs` - 25 Sliver render objects
- `parent_data/*.rs` - 15 parent data types
- `delegates/*.rs` - 6 delegate traits
- `pipeline/*.rs` - PipelineOwner, frame production

---

**Version**: 1.0  
**Status**: Complete specification, ready for implementation

Start reading: [[Protocol]]
