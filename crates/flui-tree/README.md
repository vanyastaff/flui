# FLUI Tree

[![Crates.io](https://img.shields.io/crates/v/flui-tree)](https://crates.io/crates/flui-tree)
[![Documentation](https://docs.rs/flui-tree/badge.svg)](https://docs.rs/flui-tree)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Tree abstraction traits for the FLUI UI framework.**

FLUI Tree provides trait definitions that enable clean separation of concerns between element management and rendering in UI frameworks. It breaks circular dependencies by defining abstract interfaces that different crates can implement and depend on.

## The Problem

In traditional UI frameworks, element trees and render operations are tightly coupled:

```text
❌ element → render → pipeline → element (CIRCULAR!)
```

This creates maintenance nightmares and compile-time dependency cycles.

## The Solution

FLUI Tree defines abstract traits that break these cycles:

```text
✅                 flui-foundation
                         │
          ┌──────────────┼──────────────┐
          │              │              │
          ▼              ▼              ▼
     flui-tree     flui-element   flui-rendering
          │              │              │
          └──────────────┴──────────────┘
                         │
                         ▼
                  flui-pipeline
                (implements traits)
```

## Features

- **Tree Traits**: `TreeRead`, `TreeWrite`, `TreeNav` for generic tree operations
- **Render Traits**: `RenderTreeAccess`, `DirtyTracking` for render-specific operations
- **Pipeline Traits**: `LayoutVisitable`, `PaintVisitable`, `HitTestVisitable` for phase operations
- **Zero-Allocation Iterators**: Ancestors, Descendants, BFS, DFS traversals
- **Render-Specific Iterators**: `RenderChildren`, `RenderAncestors`, `RenderDescendants`, `RenderPath`
- **Visitor Pattern**: Flexible traversal with early termination and subtree skipping
- **Thread Safe**: All traits require `Send + Sync`
- **Zero-Cost**: Inline stack storage, minimal allocations

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-tree = "0.1"
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

## Implementing Tree Traits

```rust
use flui_tree::{TreeRead, TreeNav, TreeWrite};
use flui_foundation::{ElementId, Slot};

struct MyNode {
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    data: String,
}

struct MyTree {
    nodes: Vec<Option<MyNode>>,
}

impl TreeRead for MyTree {
    type Node = MyNode;
    
    fn get(&self, id: ElementId) -> Option<&MyNode> {
        self.nodes.get(id.index())?.as_ref()
    }
    
    fn contains(&self, id: ElementId) -> bool {
        self.nodes.get(id.index()).is_some_and(|n| n.is_some())
    }
    
    fn len(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }
}

impl TreeNav for MyTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent
    }
    
    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }
    
    fn slot(&self, _id: ElementId) -> Option<Slot> {
        None // Optional slot information
    }
}
```

## Iterators

All iterators are designed for minimal allocations using inline stack storage:

### Ancestor Traversal

| Iterator | Description |
|----------|-------------|
| `Ancestors` | Node to root traversal |
| `AncestorsWithDepth` | With depth information |
| `RenderAncestors` | Only render elements |

```rust
use flui_tree::{TreeNav, Ancestors};

// Find path to root
fn path_to_root<T: TreeNav>(tree: &T, start: ElementId) -> Vec<ElementId> {
    tree.ancestors(start).collect()
}

// Calculate tree depth
fn tree_depth<T: TreeNav>(tree: &T, id: ElementId) -> usize {
    tree.ancestors(id).count() - 1
}
```

### Descendant Traversal

| Iterator | Description |
|----------|-------------|
| `Descendants` | Pre-order depth-first (parent before children) |
| `DescendantsWithDepth` | With depth information |
| `RenderDescendants` | Only render elements |

```rust
// Find all nodes at a specific depth
fn nodes_at_depth<T: TreeNav>(tree: &T, root: ElementId, target: usize) -> Vec<ElementId> {
    tree.descendants_with_depth(root)
        .filter(|(_, depth)| *depth == target)
        .map(|(id, _)| id)
        .collect()
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

## Visitor Pattern

The visitor pattern provides powerful traversal control:

### VisitorResult

- **`Continue`** - Continue normal traversal
- **`SkipChildren`** - Skip this node's children (continue to siblings)
- **`Stop`** - Stop traversal completely

### Basic Usage

```rust
use flui_tree::{TreeVisitor, VisitorResult, visit_depth_first};

struct FindByName<'a> {
    target: &'a str,
    found: Option<ElementId>,
}

impl TreeVisitor for FindByName<'_> {
    fn visit(&mut self, id: ElementId, depth: usize) -> VisitorResult {
        // Your search logic here
        if self.found.is_some() {
            VisitorResult::Stop
        } else {
            VisitorResult::Continue
        }
    }
    
    // Optional hooks
    fn pre_children(&mut self, id: ElementId, depth: usize) {
        // Called before visiting children
    }
    
    fn post_children(&mut self, id: ElementId, depth: usize) {
        // Called after visiting all children
    }
}

let mut visitor = FindByName { target: "button", found: None };
visit_depth_first(&tree, root, &mut visitor);
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

// Create flags
let flags = AtomicDirtyFlags::new();

// Mark dirty (lock-free!)
flags.mark_needs_layout();
flags.mark_needs_paint();

// Check flags
if flags.needs_layout() {
    // Perform layout
    flags.clear_needs_layout();
}

// Reset all flags
flags.clear_all();
```

## Performance

- **Inline stack storage**: Up to 32 levels without heap allocation
- **Zero-cost iteration**: No virtual dispatch in hot paths
- **Atomic dirty flags**: Lock-free flag management (~2ns per operation)
- **Efficient navigation**: O(1) parent/children access for slab-based trees
- **Bitmap-based tracking**: 8 bytes per 64 elements

## Thread Safety

All traits require `Send + Sync`:

- `TreeRead` methods take `&self`
- `TreeWrite` methods take `&mut self`
- `DirtyTracking::mark_*` methods take `&self` (atomic operations)

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Serialization support for tree types |
| `full` | Enable all features |

```toml
[dependencies]
flui-tree = { version = "0.1", features = ["serde"] }
```

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
