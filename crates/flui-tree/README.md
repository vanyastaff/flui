# flui-tree

> **Internal crate** — not intended for public use.
> This crate is part of FLUI's internal infrastructure and may change without notice.

Tree data structure abstractions with typestate lifecycle management.

## Purpose

`flui-tree` provides:

- **Typestate Pattern** — compile-time lifecycle verification (`Mounted`/`Unmounted`)
- **Trait-based Navigation** — `TreeRead`, `TreeNav`, `TreeWrite` for abstract tree operations
- **Arity System** — type-safe child count validation (`Leaf`, `Single`, `Optional`, `Variable`)
- **Rich Iterators** — `Ancestors`, `Descendants`, `Siblings`, `TreeCursor`, `TreePath`
- **Visitor Pattern** — HRTB-compatible visitors with composition

## Architecture

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

## Module Overview

| Module | Description |
|--------|-------------|
| `state` | `Mounted`, `Unmounted`, `Mountable`, `Unmountable` |
| `depth` | `Depth`, `AtomicDepth` for tree position tracking |
| `traits/` | `TreeRead`, `TreeNav`, `TreeNavExt`, `TreeWrite` |
| `arity/` | `Leaf`, `Single`, `Optional`, `Variable`, `Exact<N>` |
| `iter/` | Iterators and navigation (`TreeCursor`, `TreePath`, `Slot`) |
| `visitor/` | Visitor pattern with composition |
| `diff` | `TreeDiff` for tree comparison |
| `error` | `TreeError`, `TreeResult` |

## Key Types

### Typestate Lifecycle

```rust
use flui_tree::{Mounted, Unmounted, Mountable, Unmountable, Depth};

// Node can only be mounted once
let unmounted: MyNode<Unmounted> = MyNode::new(data);
let mounted: MyNode<Mounted> = unmounted.mount_root();

// Position access only for mounted nodes
let parent = mounted.parent();
let depth = mounted.depth();
```

### Tree Navigation

```rust
use flui_tree::{TreeNav, TreeNavExt};

// RPITIT iterators (zero-cost)
for ancestor in tree.ancestors(node_id) { ... }
for (id, depth) in tree.descendants(root) { ... }

// Rich path operations
let path: TreePath<Id> = tree.path_to_node(target);
assert!(parent_path.is_ancestor_of(&child_path));

// Stateful cursor navigation
let mut cursor = tree.cursor_with_history(root, 10);
cursor.go_first_child();
cursor.go_next_sibling();
cursor.go_back(); // history support
```

### Arity System

```rust
use flui_tree::arity::*;

let single: Single<Id> = Single::new(child);
let child = single.child(); // always valid

let optional: Optional<Id> = Optional::none();
let maybe = optional.child(); // Option<Id>

let pair: Exact<2, Id> = Exact::new([a, b]);
```

### Slot & Position

```rust
use flui_tree::{TreeNav, Slot};

// Rich node position information
if let Some(slot) = tree.slot(node_id) {
    let parent = slot.parent();
    let index = slot.index();
    let depth = slot.depth();
    let prev = slot.previous_sibling();
    let next = slot.next_sibling();
}
```

## Requirements

- **Rust 1.75+** — for GAT and RPITIT support

## Thread Safety

- `TreeRead` — `&self`, multi-reader safe
- `TreeWrite` — `&mut self`, exclusive access
- All iterators are `Send` when tree is `Send`

## Related Crates

- [`flui-foundation`](../flui-foundation) — base types (`ElementId`, `Identifier`)
- [`flui-element`](../flui-element) — Element tree implementation
- [`flui-rendering`](../flui_rendering) — Render object system
