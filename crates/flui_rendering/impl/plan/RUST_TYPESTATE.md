# Rust Typestate Pattern for RenderObject

This document describes how FLUI's typestate implementation improves upon Flutter's rendering architecture while maintaining conceptual compatibility.

## Flutter vs FLUI Comparison

### Flutter (Dart) Approach

```dart
class RenderObject {
  RenderObject? parent;          // nullable, can be invalid
  int? _depth;                   // nullable, can be stale
  PipelineOwner? _owner;         // nullable, runtime checks

  void attach(PipelineOwner owner) {
    _owner = owner;
    // ... attach children
  }

  void detach() {
    _owner = null;
    // ... detach children
  }
}
```

**Issues:**
- ❌ Runtime checks needed to verify attached state
- ❌ Possible to access `parent` when detached (returns null or stale data)
- ❌ No compile-time guarantee of valid tree position

### FLUI (Rust) Approach

```rust
pub struct RenderNode<S: NodeState> {
    // Common fields (always valid)
    render_object: Box<dyn RenderObject>,
    lifecycle: RenderLifecycle,
    element_id: Option<ElementId>,

    // Mounted-only fields (only accessible when S = Mounted)
    parent: Option<RenderId>,
    depth: Depth,
    children: Vec<RenderId>,
    cached_size: Option<Size>,

    _state: PhantomData<S>,
}

impl RenderNode<Unmounted> {
    fn new<R: RenderObject>(object: R) -> Self { /* ... */ }
}

impl RenderNode<Mounted> {
    fn parent(&self) -> Option<RenderId> { self.parent }
    fn depth(&self) -> Depth { self.depth }
    fn children(&self) -> &[RenderId] { &self.children }
}
```

**Benefits:**
- ✅ **Compile-time safety**: `parent()` only available on `Mounted` nodes
- ✅ **Type-level guarantees**: RenderTree stores `RenderNode<Mounted>` exclusively
- ✅ **Zero-cost**: PhantomData has no runtime overhead
- ✅ **Explicit transitions**: mount()/unmount() make lifecycle clear

## Lifecycle Comparison

### Flutter Lifecycle

```
Constructor → adoptChild → attach → layout → paint → detach → dispose
     ↓            ↓          ↓                         ↓
  Created    InTree    Attached                  Detached
```

**State transitions are implicit and checked at runtime.**

### FLUI Typestate Lifecycle

```
RenderNode::new() → mount() → RenderNode<Mounted> → unmount() → RenderNode<Unmounted>
       ↓              ↓              ↓                    ↓              ↓
   Unmounted      Transition    InTree/Mounted       Transition     Unmounted
  (compile)       (runtime)     (compile)           (runtime)      (compile)
```

**State transitions are explicit and type-checked at compile-time.**

## Mapping Flutter Concepts to Rust Types

| Flutter Concept | FLUI Rust Type | Notes |
|----------------|----------------|-------|
| `RenderObject?` (nullable) | `Option<RenderId>` | Explicit optionality |
| `parent` field | `RenderNode<Mounted>::parent()` | Only accessible when mounted |
| `_depth` field | `RenderNode<Mounted>::depth()` | Type-safe, always valid |
| `attach(owner)` | `node.mount(parent, depth)` | Returns new type |
| `detach()` | `node.unmount()` | Returns new type |
| `adoptChild()` | `tree.add_child(parent, child)` | Updates both nodes |
| `dropChild()` | `tree.remove_child(parent, child)` | Updates both nodes |

## RenderTree Type Safety

### Flutter's RenderTree (conceptual)

```dart
class RenderTree {
  Map<int, RenderObject> nodes;  // Can store detached nodes

  RenderObject? get(int id) {
    return nodes[id];  // May be detached, caller must check
  }
}
```

### FLUI's RenderTree

```rust
pub struct RenderTree {
    nodes: Slab<RenderNode<Mounted>>,  // ONLY mounted nodes
    root: Option<RenderId>,
}

impl RenderTree {
    pub fn insert(&mut self, node: RenderNode<Mounted>) -> RenderId {
        // Type system enforces that only mounted nodes can be inserted
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    pub fn get(&self, id: RenderId) -> Option<&RenderNode<Mounted>> {
        // Return type guarantees valid tree position
        self.nodes.get(id.get() - 1)
    }
}
```

**Key Insight:** The tree itself enforces type-state invariants at compile-time.

## Advanced Pattern: Higher-Rank Trait Bounds

FLUI adds HRTB-based visitors not present in Flutter:

```rust
impl RenderTree {
    /// Visit all render objects with a closure using HRTB.
    pub fn visit_all<F>(&self, mut visitor: F)
    where
        F: for<'a> FnMut(RenderId, &'a dyn RenderObject),
    {
        for (slab_idx, node) in self.nodes.iter() {
            let id = RenderId::new(slab_idx + 1);
            visitor(id, node.render_object());
        }
    }

    /// Find render object matching predicate using HRTB.
    pub fn find_where<P>(&self, mut predicate: P) -> Option<RenderId>
    where
        P: for<'a> FnMut(&'a dyn RenderObject) -> bool,
    {
        // ...
    }
}
```

**Advantage:** Lifetime-polymorphic closures work with any borrow duration.

## Integration with flui-tree

FLUI leverages the typestate-aware `flui-tree` abstractions:

```rust
// Traits from flui-tree
pub trait Mountable {
    type Id: Identifier;
    type Mounted;
    fn mount(self, parent: Option<Self::Id>, parent_depth: Depth) -> Self::Mounted;
}

pub trait Unmountable {
    type Id: Identifier;
    type Unmounted;
    fn parent(&self) -> Option<Self::Id>;
    fn depth(&self) -> Depth;
    fn unmount(self) -> Self::Unmounted;
}

// RenderNode implements these
impl Mountable for RenderNode<Unmounted> { /* ... */ }
impl Unmountable for RenderNode<Mounted> { /* ... */ }
```

This enables:
- Generic tree algorithms that work across View/Element/Render trees
- Reusable visitor patterns
- Type-safe tree operations

## Performance Considerations

### Zero-Cost Typestate

```rust
size_of::<RenderNode<Unmounted>>() == size_of::<RenderNode<Mounted>>()
```

The `PhantomData<S>` marker is zero-sized. All type checking happens at compile-time.

### Layout and Allocation

```rust
// Flutter conceptually:
// Vec<RenderObject?> - nullable pointers

// FLUI:
Slab<RenderNode<Mounted>> - contiguous, cache-friendly
```

Benefits:
- Better cache locality (all nodes contiguous)
- O(1) insert/remove with stable IDs
- No null checks in hot paths

## Rust Naming Conventions

FLUI follows Rust API guidelines instead of Dart conventions:

| Flutter (Dart) | FLUI (Rust) | Rationale |
|---------------|-------------|-----------|
| `markNeedsLayout()` | `mark_needs_layout()` | snake_case for methods |
| `addChild()` | `add_child()` | snake_case for methods |
| `parentUsesSize` | `parent_uses_size` | snake_case for fields |
| `isRepaintBoundary` | `is_repaint_boundary()` | snake_case for accessors |

This improves Rust code readability and follows std library conventions.

## Conclusion

FLUI's typestate pattern provides:

1. **Compile-time safety** - Invalid states are unrepresentable
2. **Zero-cost abstractions** - No runtime overhead
3. **Explicit lifecycle** - Clear state transitions
4. **Rust idioms** - snake_case naming, strong typing
5. **Flutter compatibility** - Same conceptual model

The typestate approach is **strictly superior** to runtime checks while maintaining full compatibility with Flutter's rendering architecture concepts.

## References

- Flutter `RenderObject`: `impl/02_RENDER_OBJECT.md`
- FLUI implementation: `src/render_tree.rs`
- flui-tree typestate: `crates/flui-tree/src/state.rs`
