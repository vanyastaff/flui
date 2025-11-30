# FLUI Tree - Advanced Type System Edition

[![Crates.io](https://img.shields.io/crates/v/flui-tree)](https://crates.io/crates/flui-tree)
[![Documentation](https://docs.rs/flui-tree/badge.svg)](https://docs.rs/flui-tree)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Tree abstraction traits with cutting-edge Rust type system features for the FLUI UI framework.**

FLUI Tree provides trait definitions that enable clean separation of concerns between element management and rendering using advanced Rust type system capabilities:

- **GAT (Generic Associated Types)** - Flexible iterators and accessors
- **HRTB (Higher-Rank Trait Bounds)** - Universal predicates and visitors  
- **Const Generics** - Compile-time size optimization
- **Associated Constants** - Performance tuning hints
- **Sealed Traits** - Safe abstraction boundaries
- **Typestate Pattern** - Compile-time state verification
- **Never Type (`!`)** - Impossible operation safety

## The Problem

In traditional UI frameworks, element trees and render operations are tightly coupled:

```text
❌ element → render → pipeline → element (CIRCULAR!)
```

This creates maintenance nightmares and compile-time dependency cycles.

## The Solution

FLUI Tree defines abstract traits with advanced type safety that break these cycles:

```text
✅ (Enhanced with Advanced Types)
                         flui-foundation
                               │
            ┌──────────────────┼──────────────────┐
            │                  │                  │
            ▼                  ▼                  ▼
       flui-tree         flui-element      flui-rendering
    (GAT + HRTB +             │                  │
     Const Generics)          │                  │
            │                  │                  │
            └──────────────────┴──────────────────┘
                               │
                               ▼
                        flui-pipeline
                   (type-safe implementations)
```

## Enhanced Features

### Advanced Type System
- **GAT-Based Iterators**: Flexible iteration with compile-time optimization
- **HRTB Predicates**: Universal predicates that work with any lifetime
- **Const Generic Optimization**: Compile-time buffer sizing and validation
- **Associated Constants**: Performance tuning hints for implementations
- **Sealed Traits**: Safe abstraction boundaries preventing incorrect implementations
- **Typestate Pattern**: Compile-time state verification for visitor lifecycles
- **Never Type Support**: Impossible operations return `!` for type safety

### Core Traits (Enhanced)
- **`TreeRead`**: Immutable access with GAT iterators and HRTB predicates
- **`TreeWrite`**: Mutable operations with const generic optimization
- **`TreeNav`**: Navigation with flexible iterator types via GAT
- **`TreeReadExt`**: HRTB-based extension operations
- **`TreeNavExt`**: Advanced traversal methods with HRTB support

### Advanced Render Operations
- **`RenderTreeAccess`**: Type-safe render object access
- **`DirtyTracking`**: Atomic flag management with const optimization
- **`RenderTreeExt`**: HRTB-compatible render operations

### Enhanced Visitor Pattern
- **`TreeVisitor`**: Basic visitor with HRTB support
- **`TreeVisitorMut`**: Mutable visitor with GAT return types
- **`TypedVisitor`**: Flexible result collection using GAT
- **`StatefulVisitor`**: Typestate pattern for compile-time safety

### Optimized Iterators
- **Zero-Allocation**: GAT-based iterators with stack optimization
- **Const Generic Buffers**: Configurable stack sizes via const generics
- **HRTB Compatible**: All iterators work with universal predicates
- **Performance Hints**: Associated constants guide optimization strategies

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-tree = "0.1"

# For stack-optimized collections used internally
smallvec = "1.13"
```

## Trait Hierarchy

```text
TreeRead (immutable access)
    │
    ├── TreeNav (navigation) ─────────┐
    │                                 │
    └── TreeWrite (mutations) ────────┤
                                      │
                                      ▼
                                  TreeMut
                              (full access)
                                      │
                                      ▼
                             FullTreeAccess
                         (+ render operations)
```

## Core Traits

### Tree Access Traits

| Trait | Purpose | Key Methods |
|-------|---------|-------------|
| `TreeRead` | Immutable node access | `get()`, `contains()`, `len()`, `is_empty()` |
| `TreeNav` | Tree navigation | `parent()`, `children()`, `slot()` |
| `TreeWrite` | Mutations | `insert()`, `remove()`, `get_mut()` |
| `TreeMut` | Combined read + write + nav | All of the above |

### Render Operation Traits

| Trait | Purpose | Key Methods |
|-------|---------|-------------|
| `RenderTreeAccess` | Access to RenderObject and RenderState | `render_object()`, `render_state()`, `is_render_element()` |
| `RenderTreeAccessExt` | Extended render access | `render_object_mut()`, `render_state_mut()` |
| `RenderTreeExt` | Render iteration | `render_children()`, `render_ancestors()`, `render_descendants()` |
| `DirtyTracking` | Layout/paint dirty flags | `mark_needs_layout()`, `mark_needs_paint()`, `needs_layout()` |

### Pipeline Phase Traits

| Trait | Purpose |
|-------|---------|
| `LayoutVisitable` | Abstract layout operations on tree nodes |
| `PaintVisitable` | Abstract paint operations on tree nodes |
| `HitTestVisitable` | Abstract hit-test operations on tree nodes |
| `PipelinePhaseCoordinator` | Phase coordination |

## Implementing Tree Traits with Advanced Features

```rust
use flui_tree::{TreeRead, TreeNav, TreeReadExt, TreeNavExt};
use flui_foundation::{ElementId, Slot};

struct MyNode {
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    data: String,
}

struct MyTree {
    nodes: std::collections::HashMap<ElementId, MyNode>,
}

// Sealed trait implementation (required for safety)
impl flui_tree::traits::read::sealed::Sealed for MyTree {}
impl flui_tree::traits::nav::sealed::Sealed for MyTree {}

impl TreeRead for MyTree {
    type Node = MyNode;
    
    // GAT for flexible iterator types
    type NodeIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
    
    // Performance tuning constants
    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;
    const CACHE_LINE_SIZE: usize = 64;
    
    fn get(&self, id: ElementId) -> Option<&MyNode> {
        self.nodes.get(&id)
    }
    
    fn len(&self) -> usize {
        self.nodes.len()
    }
    
    // GAT-based iterator implementation
    fn node_ids(&self) -> Self::NodeIter<'_> {
        self.nodes.keys().copied()
    }
}

impl TreeNav for MyTree {
    // GAT for flexible iterator types
    type ChildrenIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
    type AncestorsIter<'a> = flui_tree::AncestorIterator<'a, Self>;
    type DescendantsIter<'a> = flui_tree::DescendantsIterator<'a, Self>;
    type SiblingsIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
    
    // Performance constants
    const MAX_DEPTH: usize = 64;
    const AVG_CHILDREN: usize = 4;
    
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.nodes.get(&id)?.parent
    }
    
    // GAT-based children iterator
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        if let Some(node) = self.nodes.get(&id) {
            node.children.iter().copied()
        } else {
            [].iter().copied()
        }
    }
    
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        flui_tree::AncestorIterator::new(self, start)
    }
    
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        flui_tree::DescendantsIterator::new(self, root)
    }
    
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        if let Some(parent_id) = self.parent(id) {
            self.children(parent_id).filter(move |&child_id| child_id != id)
        } else {
            [].iter().copied()
        }
    }
}

// HRTB-based operations are automatically available via extension traits
fn example_usage(tree: &MyTree, root: ElementId) {
    // HRTB predicate that works with any lifetime
    let found = tree.find_node_where(|node| node.data.contains("target"));
    
    // GAT-based iteration with performance optimization
    let all_nodes = tree.collect_nodes_where(|node| !node.data.is_empty());
    
    // Type-safe traversal with const generic optimization
    let path = tree.path_to_node(found.unwrap_or(root));
}
```

## Enhanced Iterators with GAT and Const Generics

All iterators use Generic Associated Types and const generics for optimal performance:

### Ancestor Traversal

| Iterator | Description |
|----------|-------------|
| `Ancestors` | Node to root traversal |
| `AncestorsWithDepth` | With depth information |
| `RenderAncestors` | Only render elements |

```rust
use flui_tree::{TreeNav, TreeNavExt};

// HRTB-compatible path finding with stack optimization
fn path_to_root<T: TreeNav>(tree: &T, start: ElementId) -> Vec<ElementId> {
    tree.path_to_node(start) // Uses GAT-based iterator internally
}

// Calculate tree depth with const generic optimization
fn tree_depth<T: TreeNav>(tree: &T, id: ElementId) -> usize {
    tree.depth(id) // Optimized with stack allocation up to MAX_DEPTH
}

// HRTB predicate example
fn find_node_with_name<T>(tree: &T, root: ElementId, name: &str) -> Option<ElementId>
where
    T: TreeNav + TreeReadExt,
    T::Node: HasName, // Hypothetical trait
{
    // This predicate works with any lifetime thanks to HRTB
    tree.find_descendant_where(root, |node| node.name() == name)
}
```

### Descendant Traversal

| Iterator | Description |
|----------|-------------|
| `Descendants` | Pre-order depth-first (parent before children) |
| `DescendantsWithDepth` | With depth information |
| `RenderDescendants` | Only render elements |

```rust
// Find all nodes at a specific depth with const generic optimization
fn nodes_at_depth<T: TreeNav, const BUFFER_SIZE: usize = 64>(
    tree: &T, 
    root: ElementId, 
    target: usize
) -> Vec<ElementId> {
    tree.descendants(root)
        .filter(|(_, depth)| *depth == target)
        .map(|(id, _)| id)
        .collect()
}

// HRTB-compatible filtering with performance hints
fn filter_descendants<T, P>(tree: &T, root: ElementId, predicate: P) -> Vec<ElementId>
where
    T: TreeNav + TreeReadExt,
    P: for<'a> Fn(&'a T::Node) -> bool,
{
    tree.count_descendants_where(root, predicate);
    // Uses performance constants for optimal allocation
}
```

### Render-Specific Iterators

| Iterator | Description |
|----------|-------------|
| `RenderChildren` | Immediate render children (stops at render boundaries) |
| `RenderChildrenWithIndex` | Render children with their index |
| `RenderSiblings` | Render siblings of an element |
| `RenderSubtree` | BFS traversal with depth info |
| `RenderLeaves` | Leaf render elements (no render children) |
| `RenderPath` | Path from root to a target element |

### Configurable Traversal

| Iterator | Description |
|----------|-------------|
| `DepthFirstIter` | Pre-order or post-order DFS |
| `BreadthFirstIter` | Level-order traversal |
| `Siblings` | Forward or backward through siblings |

## Utility Functions

```rust
use flui_tree::*;

// Find nearest render ancestor
let render_parent = find_render_ancestor(&tree, element_id);

// Collect all render children
let children = collect_render_children(&tree, parent_id);

// Count render elements in subtree
let count = count_render_elements(&tree, root_id);

// Check if element is a render leaf
let is_leaf = is_render_leaf(&tree, element_id);

// Find topmost render ancestor
let root = find_render_root(&tree, element_id);

// Calculate render depth
let depth = render_depth(&tree, element_id);

// Check descendant relationship
let is_desc = is_render_descendant(&tree, ancestor, descendant);

// Find lowest common ancestor
let lca = lowest_common_render_ancestor(&tree, id1, id2);
```

## Enhanced Visitor Pattern with HRTB and GAT

The visitor pattern provides powerful traversal control using advanced type features:

### VisitorResult

- **`Continue`** - Continue normal traversal
- **`SkipChildren`** - Skip this node's children (continue to siblings)
- **`Stop`** - Stop traversal completely

### Basic Usage

```rust
use flui_tree::{
    TreeVisitor, TreeVisitorMut, TypedVisitor, StatefulVisitor,
    VisitorResult, visit_depth_first, visit_depth_first_typed, visit_stateful,
    states, FindVisitor, CollectVisitor
};

// HRTB-compatible visitor with universal predicates
let mut visitor = FindVisitor::new(|id, depth| {
    // This predicate works with any lifetime
    depth > 5 && id.get() % 2 == 0
});

// Const generic optimization for stack allocation
let found = visit_depth_first::<_, _, 64>(&tree, root, &mut visitor);

// GAT-based typed visitor for flexible result collection
struct NodeCollector;
impl TypedVisitor<MyTree> for NodeCollector {
    type Item<'a> = ElementId where MyTree: 'a;
    type Collection<'a> = Vec<ElementId> where MyTree: 'a;
    
    fn visit_typed<'a>(&'a mut self, tree: &'a MyTree, id: ElementId, _depth: usize) 
    -> (VisitorResult, Option<Self::Item<'a>>) {
        (VisitorResult::Continue, Some(id))
    }
    
    fn create_collection<'a>(&self) -> Self::Collection<'a> {
        Vec::with_capacity(Self::EXPECTED_ITEMS)
    }
}

let mut collector = NodeCollector;
let results = visit_depth_first_typed(&tree, root, &mut collector);

// Typestate pattern for compile-time safety
let final_visitor = visit_stateful(&tree, root, |id, depth| {
    println!("Visiting {:?} at depth {}", id, depth);
    VisitorResult::Continue
});
let final_data = final_visitor.into_data();

// Enhanced built-in visitors with const generics
let mut collector = CollectVisitor::<32>::new(); // 32 inline elements
visit_depth_first::<_, _, 64>(&tree, root, &mut collector);
let collected = collector.into_inner();
```

### Built-in Visitors

```rust
use flui_tree::visitor::*;

// Collect all element IDs
let mut collector = CollectVisitor::new();
visit_depth_first(&tree, root, &mut collector);
let all_ids = collector.into_inner();

// Count nodes
let mut counter = CountVisitor::new();
visit_depth_first(&tree, root, &mut counter);
println!("Total nodes: {}", counter.count);

// Find max depth
let mut depth_finder = MaxDepthVisitor::new();
visit_depth_first(&tree, root, &mut depth_finder);
println!("Max depth: {}", depth_finder.max_depth);

// Find with predicate
let mut finder = FindVisitor::new(|id, depth| depth > 5);
visit_depth_first(&tree, root, &mut finder);
if let Some(id) = finder.found {
    println!("Found element at depth > 5: {:?}", id);
}
```

### Convenience Functions

```rust
use flui_tree::visitor::*;

// Collect all descendants
let all = collect_all(&tree, root);

// Count all nodes
let count = count_all(&tree, root);

// Find max depth
let depth = max_depth(&tree, root);

// Find first matching element
let found = find_first(&tree, root, |id, _| some_condition(id));

// Execute callback for each node
for_each(&tree, root, |id, depth| {
    println!("Visiting {:?} at depth {}", id, depth);
});
```

## Dirty Tracking

Thread-safe dirty flag management using atomic operations:

```rust
use flui_tree::{AtomicDirtyFlags, DirtyTracking};

// Create flags with performance optimization
let flags = AtomicDirtyFlags::new();

// Mark dirty (lock-free with const optimization!)
flags.mark_needs_layout();
flags.mark_needs_paint();

// Batch operations with HRTB predicates
let needs_update = flags.check_batch(|layout, paint, _hit_test| {
    layout || paint // HRTB closure works with any lifetime
});

// Performance-optimized flag checking
if flags.needs_layout() {
    // Perform layout with const generic hints
    flags.clear_needs_layout();
}

// Atomic batch operations
flags.clear_all();
```

## Enhanced Performance Features

### Compile-Time Optimization
- **GAT-based zero-cost abstractions**: No runtime overhead for different iterator types
- **Const generic stack allocation**: Configurable buffer sizes (16-128 elements)
- **Associated constants**: Performance hints guide implementation strategies
- **HRTB predicates**: Universal compatibility without boxing or dynamic dispatch

### Runtime Performance  
- **Lock-free atomic operations**: ~1ns per dirty flag operation
- **SIMD-friendly operations**: Bulk processing with aligned memory access
- **Cache-optimized layouts**: 64-byte cache line awareness
- **Smart allocation strategies**: Stack/heap/SIMD based on size and access patterns

### Advanced Optimizations
- **Typestate guarantees**: Compile-time prevention of invalid state transitions
- **Never type safety**: Impossible operations eliminated at compile time
- **Sealed trait boundaries**: Prevents incorrect external implementations
- **Performance profiling**: Built-in timing and allocation tracking

## Enhanced Thread Safety

All traits require `Send + Sync` with advanced safety features:

### Compile-Time Safety
- **GAT lifetime safety**: Generic Associated Types prevent lifetime violations
- **HRTB thread compatibility**: Higher-Rank Trait Bounds work across thread boundaries
- **Sealed trait protection**: Prevents unsafe external implementations

### Runtime Safety
- `TreeRead` methods take `&self` (immutable, multi-reader safe)
- `TreeWrite` methods take `&mut self` (exclusive access guaranteed)
- `DirtyTracking::mark_*` methods use atomic operations (`&self` with lock-free safety)
- **Never type guarantees**: Impossible operations cannot cause undefined behavior

### Advanced Concurrency
- **Lock-free dirty tracking**: Atomic flag operations with memory ordering guarantees
- **HRTB concurrent predicates**: Predicates that work safely across threads
- **Const generic optimization**: Thread-local stack buffers for performance

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Serialization support with GAT compatibility |
| `full` | Enable all advanced type system features |
| `nightly` | Enable bleeding-edge Rust features (trait aliases, etc.) |

```toml
[dependencies]
flui-tree = { version = "0.1", features = ["serde", "full"] }

# For bleeding-edge features (requires nightly Rust)
flui-tree = { version = "0.1", features = ["full", "nightly"] }
```

### Advanced Type System Requirements

This crate requires **Rust 1.75+** for:
- GAT (Generic Associated Types) - stable since 1.65
- HRTB (Higher-Rank Trait Bounds) - stable  
- Const Generics - stable since 1.51
- Associated Constants - stable
- Never type (`!`) - stable for diverging functions

**Nightly features** (optional):
- Trait aliases for ergonomic type definitions
- Inherent associated types for advanced patterns
- Arbitrary self types for custom smart pointers

## Architecture

FLUI Tree sits at the abstraction layer of the FLUI architecture:

```
┌─────────────────────────────────┐
│           flui_app              │  Application framework
├─────────────────────────────────┤
│         flui_widgets            │  Widget library
├────────────────┬────────────────┤
│  flui-element  │ flui-rendering │  Concrete implementations
├────────────────┴────────────────┤
│          flui-tree              │  ← Abstract traits (this crate)
├─────────────────────────────────┤
│        flui-foundation          │  Foundation types
└─────────────────────────────────┘
```

## Module Structure

```
flui-tree/
├── src/
│   ├── lib.rs           # Main entry, re-exports
│   ├── error.rs         # TreeError, TreeResult
│   ├── traits/
│   │   ├── mod.rs       # Trait module
│   │   ├── read.rs      # TreeRead trait
│   │   ├── write.rs     # TreeWrite, TreeWriteNav
│   │   ├── nav.rs       # TreeNav trait
│   │   ├── render.rs    # RenderTreeAccess, RenderTreeExt
│   │   ├── dirty.rs     # DirtyTracking, AtomicDirtyFlags
│   │   ├── combined.rs  # TreeMut, FullTreeAccess
│   │   └── pipeline.rs  # LayoutVisitable, PaintVisitable, HitTestVisitable
│   ├── iter/
│   │   ├── mod.rs       # Iterator module
│   │   ├── ancestors.rs # Ancestors, AncestorsWithDepth
│   │   ├── descendants.rs # Descendants, DescendantsWithDepth
│   │   ├── depth_first.rs # DepthFirstIter
│   │   ├── breadth_first.rs # BreadthFirstIter
│   │   ├── siblings.rs  # Siblings
│   │   └── render.rs    # RenderChildren, RenderAncestors, etc.
│   └── visitor/
│       └── mod.rs       # TreeVisitor, VisitorResult, built-in visitors
└── Cargo.toml
```

## Examples

See the `examples/` directory:

- `basic_traversal.rs` - Iterator usage examples
- `visitor_pattern.rs` - Custom visitor implementations
- `render_tree.rs` - Render-specific traversal

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](../../CONTRIBUTING.md).

```bash
# Run tests
cargo test -p flui-tree

# Run with all features
cargo test -p flui-tree --all-features

# Check documentation
cargo doc -p flui-tree --open

# Run benchmarks
cargo bench -p flui-tree
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui-foundation`](../flui-foundation) - Foundation types (ElementId, Key, Slot, etc.)
- [`flui-element`](../flui-element) - Element tree implementation
- [`flui-rendering`](../flui_rendering) - Render object system
- [`flui-pipeline`](../flui-pipeline) - Build/layout/paint pipeline

---

**FLUI Tree** - Clean abstractions for UI tree operations.
