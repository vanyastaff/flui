# flui-tree

**Pure tree abstraction traits with advanced Rust type system features.**

flui-tree provides trait definitions that enable clean separation of concerns between tree management and domain-specific logic using modern Rust type system capabilities:

- **Generic ID Types** - Use any type implementing `TreeId` (associated type pattern)
- **GAT (Generic Associated Types)** - Flexible iterators and accessors
- **HRTB (Higher-Rank Trait Bounds)** - Universal predicates and visitors
- **Const Generics** - Compile-time arity validation
- **Sealed Traits** - Safe abstraction boundaries
- **Type-state Pattern** - Compile-time state verification

## The Problem

In traditional UI frameworks, element trees and render operations are tightly coupled:

```text
element -> render -> pipeline -> element (CIRCULAR!)
```

This creates maintenance nightmares and compile-time dependency cycles.

## The Solution

flui-tree defines abstract traits that break these cycles:

```text
                     flui-foundation
                           |
        +------------------+------------------+
        |                  |                  |
        v                  v                  v
   flui-tree         flui-element      flui-rendering
  (pure traits)            |                  |
        |                  |                  |
        +------------------+------------------+
                           |
                           v
                    flui-pipeline
               (concrete implementations)
```

## Features

### Core Traits

| Trait | Purpose | Key Methods |
|-------|---------|-------------|
| `TreeRead` | Immutable node access | `get()`, `contains()`, `len()`, `is_empty()` |
| `TreeNav` | Tree navigation | `parent()`, `children()`, `depth()`, `is_root()` |
| `TreeWrite` | Mutations | `insert()`, `remove()`, `get_mut()`, `set_parent()` |
| `TreeWriteNav` | Combined write + nav | All of the above |

All traits use an associated type `Id` that must implement `TreeId`:

```rust
pub trait TreeRead {
    type Id: TreeId;  // Generic over ID type
    type Node;
    
    fn get(&self, id: Self::Id) -> Option<&Self::Node>;
    // ...
}
```

### Arity System

Compile-time child count validation:

| Type | Description | Validation |
|------|-------------|------------|
| `Leaf` | No children (0) | Compile-time |
| `Single` | Exactly one child | Compile-time |
| `Optional` | Zero or one child | Compile-time |
| `Variable` | Any number of children | Runtime |
| `Exact<N>` | Exactly N children | Compile-time |
| `AtLeast<N>` | N or more children | Runtime |
| `Range<MIN, MAX>` | Between MIN and MAX | Runtime |

```rust
use flui_tree::arity::*;

// Type-safe child access
let single: Single<ElementId> = Single::new(child_id);
let child = single.child(); // Always valid

let optional: Optional<ElementId> = Optional::none();
let maybe_child = optional.child(); // Returns Option<ElementId>

// Const generic exact count
let pair: Exact<2, ElementId> = Exact::new([id1, id2]);
let [first, second] = pair.children();
```

### Iterators

Zero-allocation iterators with GAT support:

| Iterator | Description |
|----------|-------------|
| `AncestorIterator` | Node to root traversal |
| `DescendantsIterator` | Pre-order depth-first |
| `SiblingsIterator` | Forward/backward through siblings |
| `DepthFirstIterator` | Pre-order or post-order DFS |
| `BreadthFirstIterator` | Level-order traversal |

```rust
use flui_tree::{TreeNav, TreeNavExt};

// Ancestor traversal
for ancestor in tree.ancestors(node_id) {
    println!("Ancestor: {:?}", ancestor);
}

// Descendants with depth
for (id, depth) in tree.descendants_with_depth(root) {
    println!("Node {:?} at depth {}", id, depth);
}

// Find depth of a node
let depth = tree.depth(node_id);
```

### Visitor Pattern

HRTB-compatible visitors with flexible result handling:

```rust
use flui_tree::visitor::*;

// Count nodes
let mut counter = CountVisitor::new();
visit_depth_first(&tree, root, &mut counter);
println!("Total: {}", counter.count());

// Collect matching nodes
let mut collector = CollectVisitor::new();
visit_depth_first(&tree, root, &mut collector);
let all_ids = collector.into_inner();

// Find with predicate
let mut finder = FindVisitor::new(|id, depth| depth > 3);
visit_depth_first(&tree, root, &mut finder);
if let Some(found) = finder.found() {
    println!("Found: {:?}", found);
}

// Custom visitor
struct MyVisitor;
impl<T: TreeNav> TreeVisitor<T> for MyVisitor {
    fn visit(&mut self, tree: &T, id: ElementId, depth: usize) -> VisitorResult {
        if depth > 10 {
            VisitorResult::Stop
        } else {
            VisitorResult::Continue
        }
    }
}
```

### Error Handling

Comprehensive error types with generic ID support:

```rust
use flui_tree::{TreeError, TreeResult};
use flui_foundation::ElementId;

// TreeResult defaults to ElementId
fn process_node<T: TreeRead<Id = ElementId>>(
    tree: &T, 
    id: ElementId
) -> TreeResult<()> {
    let node = tree.get(id).ok_or_else(|| TreeError::not_found(id))?;
    // ...
    Ok(())
}

// Or specify custom ID type
fn process_custom<Id: TreeId, T: TreeRead<Id = Id>>(
    tree: &T,
    id: Id
) -> TreeResult<(), Id> {
    let node = tree.get(id).ok_or_else(|| TreeError::not_found(id))?;
    Ok(())
}

// Error classification
match error {
    e if e.is_structural() => println!("Tree structure error"),
    e if e.is_lookup_error() => println!("Node not found"),
    e if e.is_internal() => println!("Internal error"),
    _ => println!("Other error"),
}
```

## Quick Start

```toml
[dependencies]
flui-tree = { path = "../flui-tree" }
```

### Implementing Tree Traits

```rust
use flui_tree::{TreeRead, TreeNav, TreeWrite};
use flui_foundation::{ElementId, TreeId};

struct MyNode<Id: TreeId> {
    parent: Option<Id>,
    children: Vec<Id>,
    data: String,
}

struct MyTree<Id: TreeId> {
    nodes: HashMap<Id, MyNode<Id>>,
}

impl<Id: TreeId> TreeRead for MyTree<Id> {
    type Id = Id;
    type Node = MyNode<Id>;

    fn get(&self, id: Self::Id) -> Option<&Self::Node> {
        self.nodes.get(&id)
    }

    fn contains(&self, id: Self::Id) -> bool {
        self.nodes.contains_key(&id)
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }
}

impl<Id: TreeId> TreeNav for MyTree<Id> {
    fn parent(&self, id: Self::Id) -> Option<Self::Id> {
        self.nodes.get(&id)?.parent
    }

    fn children(&self, id: Self::Id) -> impl Iterator<Item = Self::Id> + '_ {
        self.nodes.get(&id)
            .map(|n| n.children.iter().copied())
            .into_iter()
            .flatten()
    }

    fn child_count(&self, id: Self::Id) -> usize {
        self.nodes.get(&id).map(|n| n.children.len()).unwrap_or(0)
    }
}
```

## Module Structure

```
flui-tree/
  src/
    lib.rs           # Re-exports
    error.rs         # TreeError, TreeResult
    traits/
      mod.rs         # Trait module
      read.rs        # TreeRead + TreeReadExt
      nav.rs         # TreeNav + TreeNavExt
      write.rs       # TreeWrite, TreeWriteNav
    arity/
      mod.rs         # Arity types and traits
      accessors.rs   # Child access patterns
    iter/
      mod.rs         # Iterator module
      ancestors.rs   # AncestorIterator
      descendants.rs # DescendantsIterator
      siblings.rs    # SiblingsIterator
      depth_first.rs # DepthFirstIterator
      breadth_first.rs # BreadthFirstIterator
    visitor/
      mod.rs         # TreeVisitor, VisitorResult
      composition.rs # Visitor combinators
      fallible.rs    # FallibleVisitor
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Serialization support for tree types |

```toml
[dependencies]
flui-tree = { path = "../flui-tree", features = ["serde"] }
```

## Requirements

- **Rust 1.75+** for GAT and const generics support

## Thread Safety

All traits require `Send + Sync`:

- `TreeRead` methods take `&self` (multi-reader safe)
- `TreeWrite` methods take `&mut self` (exclusive access)
- Iterators are `Send` when the tree is `Send`

## Architecture

flui-tree sits at the abstraction layer:

```
+-----------------------------+
|         flui_app            |  Application
+-----------------------------+
|       flui_widgets          |  Widgets
+-------------+---------------+
| flui-element | flui-rendering |  Implementations
+-------------+---------------+
|        flui-tree            |  <- This crate
+-----------------------------+
|      flui-foundation        |  Foundation types
+-----------------------------+
```

## Related Crates

- [`flui-foundation`](../flui-foundation) - Foundation types (ElementId, TreeId, Key, Slot)
- [`flui-element`](../flui-element) - Element tree implementation
- [`flui-rendering`](../flui_rendering) - Render object system
