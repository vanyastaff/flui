# flui-element

Element tree and lifecycle management for the FLUI UI framework.

This crate provides the core `Element` type and `ElementTree` data structure for managing the element layer of FLUI's three-tree architecture.

## Architecture

```
View (immutable) --> Element (mutable) --> RenderObject (layout/paint)
                     ^^^^^^^^^^^^^^^^
                     This crate!
```

In FLUI's three-tree architecture:

- **View Tree** - Immutable configuration objects (widgets)
- **Element Tree** - Mutable instances managing lifecycle and state *(this crate)*
- **Render Tree** - Layout computation and painting

The Element layer acts as a bridge between the declarative View layer and the imperative Render layer.

## Key Features

- **Unified Element struct** - Single type for all element variants via type erasure
- **Slab-based storage** - O(1) element access by `ElementId`
- **Lock-free dirty tracking** - Atomic flags for thread-safe state updates
- **Abstract tree traits** - Implements `flui-tree` traits for generic algorithms
- **Lifecycle management** - Initial, Active, Inactive, Defunct states

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
flui-element = { path = "../flui-element" }
```

Or with serde support:

```toml
[dependencies]
flui-element = { path = "../flui-element", features = ["serde"] }
```

## Usage

### Creating and Managing Elements

```rust
use flui_element::{Element, ElementTree, ElementLifecycle};
use flui_foundation::ElementId;

// Create a tree
let mut tree = ElementTree::new();

// Insert elements
let root_id = tree.insert(Element::empty());
let child_id = tree.insert(Element::empty());

// Set up parent-child relationship
if let Some(child) = tree.get_mut(child_id) {
    child.base_mut().set_parent(Some(root_id));
}
if let Some(root) = tree.get_mut(root_id) {
    root.add_child(child_id);
}

// Set tree root
tree.set_root(Some(root_id));

// Check lifecycle state
if let Some(element) = tree.get(root_id) {
    assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
}
```

### Element Lifecycle

Elements go through the following lifecycle states:

```
    +----------+
    | Initial  |  (Created but not mounted)
    +----+-----+
         | mount()
         v
    +---------+
 +->| Active  |<-+  (Mounted in tree)
 |  +----+----+  |
 |       |       | activate()
 |       | deactivate()
 |       v       |
 |  +---------+  |
 +--| Inactive|--+  (Unmounted but state preserved)
    +----+----+
         | dispose()
         v
    +---------+
    | Defunct |  (Permanently removed)
    +---------+
```

```rust
use flui_element::{Element, ElementLifecycle};
use flui_foundation::{ElementId, Slot};

let mut element = Element::empty();

// Initial state
assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

// Mount to tree
element.mount(Some(ElementId::new(1)), Some(Slot::new(0)));
assert_eq!(element.lifecycle(), ElementLifecycle::Active);
assert!(element.is_mounted());

// Temporarily deactivate
element.deactivate();
assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

// Reactivate
element.activate();
assert_eq!(element.lifecycle(), ElementLifecycle::Active);

// Permanently remove
element.unmount();
assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
```

### Type-Erased View Objects

Element uses type erasure via `Box<dyn Any + Send + Sync>` to store view-specific behavior without depending on the `ViewObject` trait:

```rust
use flui_element::Element;
use std::any::Any;

// Custom view object type
#[derive(Debug)]
struct MyViewObject {
    value: i32,
}

// Create element with typed view object
let element = Element::new(MyViewObject { value: 42 });
assert!(element.has_view_object());

// Downcast to access the concrete type
if let Some(view_obj) = element.view_object_as::<MyViewObject>() {
    assert_eq!(view_obj.value, 42);
}
```

### Dirty Tracking (Thread-Safe)

Elements use atomic flags for lock-free dirty tracking:

```rust
use flui_element::Element;

let element = Element::empty();

// Mark for rebuild (can be called from any thread!)
element.mark_dirty();
assert!(element.is_dirty());

// Clear dirty flag
element.clear_dirty();
assert!(!element.is_dirty());

// Layout/paint flags
element.mark_needs_layout();
assert!(element.needs_layout());

element.mark_needs_paint();
assert!(element.needs_paint());
```

### Using Tree Traits

`ElementTree` implements abstract tree traits from `flui-tree`:

```rust
use flui_element::{Element, ElementTree, TreeRead, TreeNav, TreeWrite, TreeWriteNav};

let mut tree = ElementTree::new();

// TreeWrite - insert/remove
let parent_id = TreeWrite::insert(&mut tree, Element::empty());
let child_id = TreeWrite::insert(&mut tree, Element::empty());

// TreeWriteNav - set up relationships (with cycle detection!)
TreeWriteNav::set_parent(&mut tree, child_id, Some(parent_id)).unwrap();

// TreeNav - navigation
assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
assert_eq!(TreeNav::children(&tree, parent_id), &[child_id]);

// TreeRead - immutable access
assert_eq!(TreeRead::len(&tree), 2);
assert!(TreeRead::contains(&tree, child_id));
```

### IntoElement Trait

The `IntoElement` trait enables automatic conversion to elements:

```rust
use flui_element::{Element, IntoElement};

// Element -> Element (identity)
let elem: Element = Element::empty().into_element();

// () -> empty Element
let unit_elem: Element = ().into_element();

// Option<T> handling
let some: Element = Some(Element::empty()).into_element();
let none: Element = None::<Element>.into_element();
```

## API Reference

### Core Types

| Type | Description |
|------|-------------|
| `Element` | Unified element struct with type-erased view object |
| `ElementBase` | Internal foundation for lifecycle management |
| `ElementLifecycle` | Lifecycle states (Initial, Active, Inactive, Defunct) |
| `ElementTree` | Slab-based storage with O(1) access |

### Key Methods on Element

| Method | Description |
|--------|-------------|
| `new(view_object)` | Create element with typed view object |
| `empty()` | Create empty placeholder element |
| `mount(parent, slot)` | Mount element to tree |
| `unmount()` | Permanently remove from tree |
| `activate()` / `deactivate()` | Toggle active state |
| `lifecycle()` | Get current lifecycle state |
| `mark_dirty()` | Mark for rebuild (thread-safe) |
| `view_object_as::<T>()` | Downcast view object to concrete type |
| `children()` / `add_child()` | Child management |

### Tree Traits Implemented

| Trait | Purpose |
|-------|---------|
| `TreeRead` | Immutable node access |
| `TreeNav` | Parent/child navigation |
| `TreeWrite` | Mutable tree operations |
| `TreeWriteNav` | Structure modifications with cycle detection |

## Design Decisions

### Type Erasure vs Trait Objects

Element stores `Box<dyn Any + Send + Sync>` instead of `Box<dyn ViewObject>`. This breaks the dependency cycle:

```
flui-element (this crate)
    |
    v  depends on
flui-foundation (ElementId, Slot, Flags)
    
flui-view (ViewObject trait)
    |
    v  depends on
flui-element (can access via downcast)
```

Benefits:
- flui-element has minimal dependencies
- ViewObject additions don't require rebuilding flui-element
- Allows flui-view to be optional for testing

### Slab Offset Pattern

**Critical implementation detail:**

- `ElementId` uses 1-based indexing (`NonZeroUsize`)
- `Slab` uses 0-based indexing

Conversion:
- Insert: `slab_index + 1` -> `ElementId`
- Get: `element_id.get() - 1` -> `slab_index`

This enables `Option<ElementId>` to use niche optimization (8 bytes total).

### Lock-Free Dirty Tracking

`ElementBase` uses `AtomicElementFlags` for zero-contention dirty tracking:

```rust
// Can be called from any thread, no locks!
element.mark_dirty();  // Atomic CAS operation
```

This enables efficient multi-threaded UI updates.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `serde` | Serialization support for `ElementLifecycle` |
| `full` | Enable all features |

## Crate Dependencies

```
flui-foundation (ElementId, Slot, AtomicElementFlags)
       |
       v
flui-tree (TreeRead, TreeNav, TreeWrite, TreeWriteNav)
       |
       v
flui-element (Element, ElementTree, IntoElement)  <-- This crate
       |
       v
flui-view (ViewObject, BuildContext, View traits)
```

## Testing

```bash
# Run tests
cargo test -p flui-element

# Run with all features
cargo test -p flui-element --all-features
```

## Performance Characteristics

| Operation | Complexity |
|-----------|------------|
| `tree.get(id)` | O(1) |
| `tree.insert(element)` | O(1) amortized |
| `tree.remove(id)` | O(1) |
| `tree.parent(id)` | O(1) |
| `tree.children(id)` | O(1) |
| `tree.depth(id)` | O(depth) |
| `element.mark_dirty()` | O(1), lock-free |

## License

Same as the FLUI framework - see root LICENSE file.
