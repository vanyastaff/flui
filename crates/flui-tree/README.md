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

- 🌳 **Tree Traits**: `TreeRead`, `TreeWrite`, `TreeNav` for generic tree operations
- 🎨 **Render Traits**: `RenderTreeAccess`, `DirtyTracking` for render-specific operations
- 🔄 **Zero-Allocation Iterators**: Ancestors, Descendants, BFS, DFS traversals
- 👁️ **Visitor Pattern**: Flexible traversal with early termination
- 🔒 **Thread Safe**: All traits require `Send + Sync`
- ⚡ **Zero-Cost**: Inline stack storage, minimal allocations

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-tree = "0.1"
```

### Implementing Tree Traits

```rust
use flui_tree::{TreeRead, TreeNav, TreeWrite};
use flui_foundation::ElementId;

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
}
```

### Using Iterators

```rust
use flui_tree::{TreeNav, Ancestors, Descendants};

// Find path to root
fn path_to_root<T: TreeNav>(tree: &T, start: ElementId) -> Vec<ElementId> {
    tree.ancestors(start).collect()
}

// Calculate tree depth
fn tree_depth<T: TreeNav>(tree: &T, id: ElementId) -> usize {
    tree.ancestors(id).count() - 1
}

// Find all nodes at a specific depth
fn nodes_at_depth<T: TreeNav>(tree: &T, root: ElementId, target: usize) -> Vec<ElementId> {
    tree.descendants_with_depth(root)
        .filter(|(_, depth)| *depth == target)
        .map(|(id, _)| id)
        .collect()
}
```

### Using Visitors

```rust
use flui_tree::{TreeVisitor, VisitorResult, visit_depth_first};

struct FindByName<'a> {
    target: &'a str,
    found: Option<ElementId>,
}

impl TreeVisitor for FindByName<'_> {
    fn visit(&mut self, id: ElementId, _depth: usize) -> VisitorResult {
        // Check if this node matches (would need tree access in real code)
        if self.found.is_some() {
            VisitorResult::Stop
        } else {
            VisitorResult::Continue
        }
    }
}
```

## Core Traits

### Tree Access

| Trait | Purpose |
|-------|---------|
| `TreeRead` | Immutable node access (`get`, `contains`, `len`) |
| `TreeNav` | Navigation (`parent`, `children`, `ancestors`, `descendants`) |
| `TreeWrite` | Mutations (`insert`, `remove`, `get_mut`) |
| `TreeMut` | Combined read + write + nav |

### Render Operations

| Trait | Purpose |
|-------|---------|
| `RenderTreeAccess` | Access to RenderObject and RenderState |
| `DirtyTracking` | Layout/paint dirty flag management |
| `FullTreeAccess` | All traits combined |

## Iterators

All iterators are designed for minimal allocations:

| Iterator | Description |
|----------|-------------|
| `Ancestors` | Node to root traversal |
| `AncestorsWithDepth` | With depth information |
| `Descendants` | Pre-order DFS |
| `DescendantsWithDepth` | With depth information |
| `DepthFirstIter` | Configurable pre/post order |
| `BreadthFirstIter` | Level-order traversal |
| `Siblings` | Forward/backward through siblings |
| `RenderAncestors` | Only render elements |
| `RenderDescendants` | Only render elements |

## Visitor Pattern

The visitor pattern provides:

- **Early termination** via `VisitorResult::Stop`
- **Subtree skipping** via `VisitorResult::SkipChildren`
- **Pre/post hooks** for children traversal
- **Built-in visitors**: `CollectVisitor`, `CountVisitor`, `FindVisitor`

## Performance

- **Inline stack storage**: Up to 32 levels without heap allocation
- **Zero-cost iteration**: No virtual dispatch in hot paths
- **Atomic dirty flags**: Lock-free flag management
- **Efficient navigation**: O(1) parent/children access for slab-based trees

## Thread Safety

All traits require `Send + Sync`:

- `TreeRead` methods take `&self`
- `TreeWrite` methods take `&mut self`
- `DirtyTracking::mark_*` methods take `&self` (atomic operations)

## Feature Flags

- `serde` - Serialization support for tree types
- `full` - Enable all features

## Architecture

FLUI Tree sits at the abstraction layer of the FLUI architecture:

```
┌─────────────────────────────┐
│         flui_app            │  Application framework
├─────────────────────────────┤
│       flui_widgets          │  Widget library
├──────────────┬──────────────┤
│ flui-element │flui-rendering│  Concrete implementations
├──────────────┴──────────────┤
│        flui-tree            │  ← Abstract traits (this crate)
├─────────────────────────────┤
│      flui-foundation        │  Foundation types
└─────────────────────────────┘
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

- [`flui-foundation`](../flui-foundation) - Foundation types (ElementId, Key, etc.)
- [`flui-element`](../flui-element) - Element tree implementation
- [`flui-rendering`](../flui_rendering) - Render object system
- [`flui-pipeline`](../flui-pipeline) - Build/layout/paint pipeline

---

**FLUI Tree** - Clean abstractions for UI tree operations.
