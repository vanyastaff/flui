# Flutter Rendering Implementation Analysis

This folder contains detailed documentation of Flutter's rendering architecture, analyzed for implementation in FLUI (Rust).

## Document Index

| File | Topic | Description |
|------|-------|-------------|
| [01_OVERVIEW.md](01_OVERVIEW.md) | Architecture Overview | High-level view of rendering pipeline and key components |
| [02_RENDER_OBJECT.md](02_RENDER_OBJECT.md) | RenderObject | Base class lifecycle, layout, and paint protocols |
| [03_PIPELINE_OWNER.md](03_PIPELINE_OWNER.md) | PipelineOwner | Frame coordination and flush methods |
| [04_PAINTING_CONTEXT.md](04_PAINTING_CONTEXT.md) | PaintingContext | Canvas management and layer operations |
| [05_CONSTRAINTS.md](05_CONSTRAINTS.md) | Constraints | Layout constraint system |
| [06_MIXINS.md](06_MIXINS.md) | Mixins | Child management and composition patterns |
| [07_SEMANTICS.md](07_SEMANTICS.md) | Semantics | Accessibility tree and configuration |
| [08_SLIVER.md](08_SLIVER.md) | Sliver Protocol | Scrollable content and viewport slivers |
| [09_RUST_TYPESTATE.md](09_RUST_TYPESTATE.md) | **Rust Typestate** | **FLUI's typestate implementation with compile-time safety** |
| [10_GAP_ANALYSIS.md](10_GAP_ANALYSIS.md) | **Gap Analysis** | **Comparison of FLUI vs Flutter implementation with roadmap** |


## Architecture Summary

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      Flutter Rendering Architecture                      │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    Pipeline Phases                               │   │
│  │                                                                  │   │
│  │   flushLayout → flushCompositingBits → flushPaint → flushSemantics│  │
│  │        │                │                  │              │       │   │
│  │        ▼                ▼                  ▼              ▼       │   │
│  │   [_nodesNeeding   [_nodesNeeding    [_nodesNeeding  [_nodesNeeding│  │
│  │    Layout]          CompBits]         Paint]         Semantics]   │   │
│  │                                                                   │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    ▼                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    RenderObject Tree                             │   │
│  │                                                                  │   │
│  │   ┌────────────┐                                                 │   │
│  │   │RenderObject│ ─── layout() ──► Size determination             │   │
│  │   │            │ ─── paint() ───► Visual output                  │   │
│  │   │            │ ─── semantics ─► Accessibility                  │   │
│  │   └────┬───────┘                                                 │   │
│  │        │ children                                                │   │
│  │        ▼                                                         │   │
│  │   ┌────────────┐                                                 │   │
│  │   │RenderObject│...                                              │   │
│  │   └────────────┘                                                 │   │
│  │                                                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    ▼                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    Layer Tree                                    │   │
│  │                                                                  │   │
│  │   ContainerLayer                                                 │   │
│  │   ├── PictureLayer (drawing commands)                            │   │
│  │   ├── ClipRectLayer                                              │   │
│  │   │   └── PictureLayer                                           │   │
│  │   ├── TransformLayer                                             │   │
│  │   │   └── PictureLayer                                           │   │
│  │   └── OpacityLayer                                               │   │
│  │       └── PictureLayer                                           │   │
│  │                                                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

## Key Patterns for FLUI Implementation

### 1. Dirty Flag Pattern
- Objects mark themselves dirty via `markNeeds*()` methods
- Pipeline owner collects dirty objects into lists
- Flush methods process dirty lists in appropriate order

### 2. Boundary Pattern
- **Relayout Boundary**: Isolates layout changes to subtree
- **Repaint Boundary**: Isolates paint changes to subtree
- Both reduce work propagation in the tree

### 3. Parent Data Pattern
- Parent stores child-specific data on the child's `parentData` field
- Different container types use different `ParentData` subclasses
- Enables type-safe storage without child knowing parent type

### 4. Phase Separation
- Layout: Constraints down, sizes up
- Paint: Recording commands into layers
- Semantics: Building accessibility tree

## Rust Improvements Over Flutter

### 🎯 Typestate Pattern (FLUI Innovation)

FLUI introduces a **compile-time state tracking** system not present in Flutter:

```rust
// Unmounted node - no tree position
let unmounted = RenderNode::new(render_object);

// Mount into tree - explicit transition
let mounted: RenderNode<Mounted> = unmounted.mount_root();

// Type system guarantees valid tree operations
let parent = mounted.parent();  // ✅ Only available when Mounted
let depth = mounted.depth();    // ✅ Type-safe access

// unmounted.parent();  // ❌ Compile error! Unmounted nodes have no parent
```

**Benefits:**
- ✅ **Compile-time safety**: Invalid states are unrepresentable
- ✅ **Zero-cost**: PhantomData<S> has no runtime overhead
- ✅ **Explicit lifecycle**: mount()/unmount() make transitions clear
- ✅ **Type-level guarantees**: RenderTree stores only `RenderNode<Mounted>`

See [09_RUST_TYPESTATE.md](09_RUST_TYPESTATE.md) for detailed comparison with Flutter.

### Use Traits Instead of Mixins
```rust
// Instead of Dart mixin composition
pub trait SingleChildRenderObject: RenderObject {
    fn child(&self) -> Option<&dyn RenderObject>;
}
```

### Interior Mutability for Dirty Flags
```rust
pub struct RenderObjectBase {
    needs_layout: Cell<bool>,
    needs_paint: Cell<bool>,
}
```

### Arena Allocation for Trees
```rust
pub struct RenderTree {
    nodes: Slab<RenderNode<Mounted>>,  // Type-safe: only mounted nodes
    root: Option<RenderId>,
}
```

### Higher-Rank Trait Bounds (HRTB)
```rust
// Lifetime-polymorphic visitors
tree.visit_all(|id, obj| {
    println!("{}: {}", id, obj.debug_name());
});

tree.find_where(|obj| obj.debug_name() == "MyWidget");
```

## Source Reference

All documentation is based on analysis of:
- `flutter/packages/flutter/lib/src/rendering/object.dart`
- Flutter Framework source code (2024 version)

## Related FLUI Components

| Flutter | FLUI |
|---------|------|
| `RenderObject` | `flui_rendering::core::RenderObject` |
| `PipelineOwner` | `flui-pipeline::PipelineOwner` |
| `Constraints` | `flui_rendering::core::Constraints` |
| `PaintingContext` | `flui_painting::PaintingContext` |
| `SemanticsNode` | `flui_rendering::semantics::SemanticsNode` |
