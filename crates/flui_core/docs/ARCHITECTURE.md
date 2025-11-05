# FLUI Core Architecture

## Overview

FLUI Core implements a three-tree reactive UI architecture inspired by Flutter and Xilem. The design separates immutable configuration (Views) from mutable state (Elements) and rendering logic (RenderObjects).

```
View Tree (Immutable)  →  Element Tree (Mutable)  →  Render Tree (Layout/Paint)
     ↓                         ↓                            ↓
Configuration              State Management            Visual Output
```

## Core Design Principles

### 1. **Immutable Views**
- Views are cheap, immutable descriptions of what the UI should look like
- Created fresh on every frame
- Compared (diffed) to determine changes

### 2. **Mutable Element Tree**
- Elements maintain persistent state across frames
- Stored in a `Slab`-based arena for cache efficiency
- Three element types: Component, Render, Provider

### 3. **Render Objects for Layout**
- RenderObjects handle layout and painting
- Three traits based on child count: LeafRender (0), SingleRender (1), MultiRender (N)
- Uses GAT (Generic Associated Types) for type-safe metadata

## The Three Trees

### View Tree

Views are the user-facing API. They implement the `View` trait:

```rust
pub trait View: Clone + 'static {
    type State: 'static;
    type Element: ViewElement;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State);
    fn rebuild(self, prev: &Self, state: &mut Self::State,
               element: &mut Self::Element) -> ChangeFlags;
    fn teardown(&self, state: &mut Self::State, element: &mut Self::Element) {}
}
```

**Key characteristics:**
- `Clone` bound - views must be cheap to clone
- `build()` creates initial element and state
- `rebuild()` efficiently updates existing element
- `teardown()` for cleanup when removed

### Element Tree

Elements are stored in an enum for performance:

```rust
pub enum Element {
    Component(ComponentElement),  // Calls build() to produce child views
    Render(RenderElement),       // Owns RenderObject for layout/paint
    Provider(InheritedElement),  // Propagates data down tree
}
```

**Performance benefits:**
- **3.75x faster** than `Box<dyn>` trait objects
- Direct match dispatch vs vtable indirection
- Better cache locality
- **11% less memory** usage

**Lifecycle states:**
1. `Initial` - Just created
2. `Active` - Mounted in tree
3. `Inactive` - Temporarily deactivated
4. `Defunct` - Removed from tree

### Render Tree

RenderObjects handle layout and painting:

```rust
pub enum RenderNode {
    Leaf(Box<dyn LeafRender<Metadata = ()>>),
    Single {
        render: Box<dyn SingleRender<Metadata = ()>>,
        child: Option<ElementId>,
    },
    Multi {
        render: Box<dyn MultiRender<Metadata = ()>>,
        children: Vec<ElementId>,
    },
}
```

**Render traits:**
- `LeafRender` - No children (Text, Image, etc.)
- `SingleRender` - One child (Padding, Opacity, etc.)
- `MultiRender` - Multiple children (Row, Column, Stack, etc.)

## GAT Metadata Pattern

Each render object can define its own metadata type:

```rust
pub trait SingleRender: Send + Sync + Debug + 'static {
    type Metadata: Any + Send + Sync + 'static;

    fn layout(&mut self, tree: &ElementTree, child_id: ElementId,
              constraints: BoxConstraints) -> Size;
    fn paint(&self, tree: &ElementTree, child_id: ElementId,
             offset: Offset) -> BoxedLayer;
    fn metadata(&self) -> Option<&dyn Any> { None }
}
```

**Zero-cost when unused:**
```rust
impl SingleRender for RenderPadding {
    type Metadata = ();  // No runtime overhead
    // ...
}
```

**Custom metadata for complex layouts:**
```rust
#[derive(Debug, Clone, Copy)]
pub struct FlexItemMetadata {
    pub flex: i32,
    pub fit: FlexFit,
}

impl SingleRender for RenderFlexItem {
    type Metadata = FlexItemMetadata;

    fn metadata(&self) -> Option<&dyn Any> {
        Some(&self.flex_metadata)
    }
}
```

## Element Tree Storage

Elements are stored in a `Slab` arena:

```rust
pub struct ElementTree {
    elements: Slab<Element>,
    root: Option<ElementId>,
    // ...
}
```

**Benefits:**
- O(1) insertion and removal
- Stable ElementIds (generation counter prevents reuse bugs)
- Cache-friendly contiguous storage
- No heap fragmentation

**ElementId structure:**
```rust
pub struct ElementId {
    index: u32,      // Slab index
    generation: u32, // Prevents stale references
}
```

## BuildContext Design

BuildContext is **intentionally read-only** during build:

```rust
pub struct BuildContext {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
    hook_context: Arc<RefCell<HookContext>>,
}
```

**Design rationale:**
- Enables parallel builds (no write locks)
- Matches Flutter semantics
- Prevents lock contention
- Makes build phase side-effect-free

**State changes don't go through BuildContext:**
```rust
// ✅ Correct - Signal handles rebuild scheduling internally
let signal = use_signal(ctx, 0);
signal.set(42);  // Triggers rebuild via callback

// ❌ Wrong - Don't schedule rebuilds during build
// ctx.schedule_rebuild();  // This method doesn't exist!
```

## Hooks System

Hooks provide state management with automatic rebuild scheduling:

```rust
// Signal - reactive state
let count = use_signal(ctx, 0);

// Memo - derived state
let doubled = use_memo(ctx, |_| count.get() * 2);

// Effect - side effects
use_effect_simple(ctx, || {
    println!("Count changed: {}", count.get());
});
```

**Hook Rules (same as React):**
1. Always call hooks in the same order
2. Don't call hooks conditionally
3. Only call hooks at component top level

## Layout and Paint Pipeline

### Layout Phase

1. Root receives constraints from window
2. Each element layouts children via `tree.layout_child()`
3. Children return their size
4. Parent computes its own size
5. Sizes bubble up to root

**Example:**
```rust
impl SingleRender for RenderPadding {
    fn layout(&mut self, tree: &ElementTree, child_id: ElementId,
              constraints: BoxConstraints) -> Size {
        // 1. Deflate constraints by padding
        let child_constraints = constraints.deflate(&self.padding);

        // 2. Layout child
        let child_size = tree.layout_child(child_id, child_constraints);

        // 3. Add padding to size
        Size::new(
            child_size.width + self.padding.horizontal_total(),
            child_size.height + self.padding.vertical_total(),
        )
    }
}
```

### Paint Phase

1. Root paints at offset (0, 0)
2. Each element creates a `BoxedLayer`
3. Children painted with adjusted offsets
4. Layers composed into final frame

**Example:**
```rust
impl SingleRender for RenderPadding {
    fn paint(&self, tree: &ElementTree, child_id: ElementId,
             offset: Offset) -> BoxedLayer {
        // Apply padding offset
        let child_offset = Offset::new(self.padding.left, self.padding.top);
        tree.paint_child(child_id, offset + child_offset)
    }
}
```

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Element lookup | O(1) | Slab indexing |
| Element creation | O(1) | Slab allocation |
| Element removal | O(1) | Slab free |
| Layout | O(n) | n = visible elements |
| Paint | O(n) | n = visible elements |
| Rebuild | O(changed) | Only dirty elements rebuild |

## Memory Layout

```
┌─────────────────────────────────────┐
│         ElementTree (Slab)          │
├─────────────────────────────────────┤
│ [0] Element::Component              │
│     ├─ Base (parent, slot, etc)     │
│     ├─ View (Box<dyn AnyView>)      │
│     └─ State (Box<dyn Any>)         │
├─────────────────────────────────────┤
│ [1] Element::Render                 │
│     ├─ Base                          │
│     ├─ RenderNode                    │
│     │   ├─ render (trait object)    │
│     │   └─ child/children (ElementId)│
│     └─ RenderState (size, offset)   │
├─────────────────────────────────────┤
│ [2] Element::Provider               │
│     ├─ Base                          │
│     ├─ provided (Box<dyn Any>)      │
│     └─ dependents (Vec<ElementId>)  │
└─────────────────────────────────────┘
```

**Size characteristics:**
- `Element` enum: ~200-300 bytes (size of largest variant)
- `ElementId`: 8 bytes (u32 + u32)
- `Slab` growth: 2x when full (amortized O(1))

## Comparison with Flutter

| Aspect | Flutter | FLUI Core |
|--------|---------|-----------|
| **Language** | Dart | Rust |
| **Widget immutability** | Yes | Yes (Views) |
| **Element types** | 3 (Component, Render, Inherited) | 3 (same) |
| **Parent data** | Separate objects | GAT Metadata |
| **State management** | setState + Hooks | Hooks (signals) |
| **Rebuild** | Full subtree | Optimized with ChangeFlags |
| **Memory** | GC | Manual (Slab arena) |

## Comparison with Xilem

| Aspect | Xilem | FLUI Core |
|--------|-------|-----------|
| **Language** | Rust | Rust |
| **View trait** | Yes | Yes |
| **Diffing** | Structural | Structural |
| **State** | Associated type | Associated type |
| **Rebuild** | View-driven | View-driven |
| **Backend** | Vello/GPU | Custom engine |

## Best Practices

### Creating Views

1. **Keep Views cheap** - They're created every frame
2. **Implement PartialEq** - Enables rebuild optimization
3. **Override rebuild()** - Avoid unnecessary work

```rust
impl View for MyView {
    fn rebuild(self, prev: &Self, _state: &mut Self::State,
               element: &mut Self::Element) -> ChangeFlags {
        if self == *prev {
            return ChangeFlags::NONE;  // Skip rebuild!
        }
        element.mark_dirty();
        ChangeFlags::NEEDS_BUILD
    }
}
```

### Creating RenderObjects

1. **Choose correct trait** - Leaf, Single, or Multi based on children
2. **Use `Metadata = ()`** - Unless custom parent data needed
3. **Cache layout results** - Store values needed for paint

```rust
impl SingleRender for RenderAlign {
    type Metadata = ();

    fn layout(&mut self, tree: &ElementTree, child_id: ElementId,
              constraints: BoxConstraints) -> Size {
        let child_size = tree.layout_child(child_id, constraints);
        self.cached_child_size = child_size;  // Cache for paint
        self.calculate_size(child_size, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId,
             offset: Offset) -> BoxedLayer {
        // Use cached value
        let child_offset = self.alignment.align(self.cached_child_size, self.size);
        tree.paint_child(child_id, offset + child_offset)
    }
}
```

### Hook Usage

1. **Call hooks in same order** - Every build must call same hooks
2. **Clone signals for closures** - Signals are cheap (`Rc` increment)
3. **Use memo for expensive computations** - Avoid recomputing

```rust
fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
    // ✅ Correct - hooks at top level
    let count = use_signal(ctx, 0);
    let doubled = use_memo(ctx, |_| count.get() * 2);

    // ❌ Wrong - conditional hooks
    // if some_condition {
    //     let state = use_signal(ctx, 0);  // DON'T DO THIS
    // }

    // Build UI...
}
```

## See Also

- [VIEW_GUIDE.md](./VIEW_GUIDE.md) - Comprehensive guide to using the View trait
- [HOOKS_GUIDE.md](./HOOKS_GUIDE.md) - State management with hooks
- [RENDER_INTEGRATION.md](./RENDER_INTEGRATION.md) - Creating RenderObjects
