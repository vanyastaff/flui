# Full Typestate Lifecycle in FLUI

## Four States

FLUI uses a full typestate system matching Flutter's lifecycle:

| State | IS_MOUNTED | NEEDS_REBUILD | IS_REASSEMBLING | Description |
|-------|------------|---------------|-----------------|-------------|
| **Unmounted** | ❌ | ❌ | ❌ | Config only, not in tree |
| **Mounted** | ✅ | ❌ | ❌ | In tree, clean, ready to paint |
| **Dirty** | ✅ | ✅ | ❌ | In tree, needs rebuild |
| **Reassembling** | ✅ | ✅ | ✅ | In tree, hot reload in progress |

## State Transition Diagram

```
                        ┌─────────────┐
                        │  Unmounted  │  Config only
                        └──────┬──────┘
                               │ mount()
                               ↓
┌──────────────────────────────────────────────────────┐
│                    IN TREE                           │
│                                                      │
│  ┌──────────┐  mark_dirty()  ┌──────────┐          │
│  │ Mounted  │ ───────────────→│  Dirty   │          │
│  │  Clean   │                 │  Needs   │          │
│  │          │←─────────────── │  Rebuild │          │
│  └────┬─────┘    rebuild()    └──────────┘          │
│       │                                              │
│       │ reassemble()                                 │
│       ↓                                              │
│  ┌─────────────┐  finish_reassemble()  ┌──────────┐ │
│  │Reassembling │ ────────────────────→  │ Mounted  │ │
│  │  Hot Reload │                        │  Clean   │ │
│  └─────────────┘                        └──────────┘ │
│                                                      │
└──────────────────────────────────────────────────────┘
                               │ unmount()
                               ↓
                        ┌─────────────┐
                        │  Unmounted  │  Config preserved
                        └─────────────┘
```

## State-Specific APIs

### Unmounted: Config Access Only

```rust
use flui_tree::{Unmounted, Mounted};

impl<A: Arity> ViewHandle<A, Unmounted> {
    /// Create unmounted view from config
    pub fn new(config: AnyView) -> Self {
        Self {
            view_config: config,
            view_object: None,
            tree_info: None,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }

    /// ✅ Access config (only available when unmounted)
    pub fn config(&self) -> &AnyView {
        &self.view_config
    }

    // ❌ Cannot access tree_info() - not mounted!
    // pub fn tree_info(&self) -> &TreeInfo { ... }  // Compile error!

    /// Transition: Unmounted → Mounted
    pub fn mount(self, parent: Option<usize>) -> ViewHandle<A, Mounted> {
        let view_object = self.view_config.create_view_object();
        let tree_info = if let Some(parent_id) = parent {
            TreeInfo::with_parent(parent_id, 0)
        } else {
            TreeInfo::root()
        };

        ViewHandle {
            view_config: self.view_config,
            view_object: Some(view_object),
            tree_info: Some(tree_info),
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}
```

### Mounted: Clean State, Ready to Paint

```rust
use flui_tree::{Mounted, Dirty, Reassembling};

impl<A: Arity> ViewHandle<A, Mounted> {
    /// ✅ Access tree info (only when mounted)
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()  // Safe - always Some
    }

    /// ✅ Access ViewObject (only when mounted and clean)
    pub fn view_object(&self) -> &dyn ViewObject {
        self.view_object.as_ref().unwrap()
    }

    /// ✅ Paint (only when clean, not dirty)
    pub fn paint(&self, ctx: &PaintContext) {
        // Paint to canvas - only valid for clean Mounted state
    }

    /// Transition: Mounted → Dirty
    pub fn mark_dirty(self) -> ViewHandle<A, Dirty> {
        ViewHandle {
            view_config: self.view_config,
            view_object: self.view_object,
            tree_info: self.tree_info,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }

    /// Transition: Mounted → Reassembling
    pub fn reassemble(self) -> ViewHandle<A, Reassembling> {
        ViewHandle {
            view_config: self.view_config,
            view_object: self.view_object,  // Will be recreated
            tree_info: self.tree_info,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }

    /// Transition: Mounted → Unmounted
    pub fn unmount(self) -> ViewHandle<A, Unmounted> {
        ViewHandle {
            view_config: self.view_config,  // Preserve config!
            view_object: None,  // Drop live object
            tree_info: None,    // Drop tree info
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}
```

### Dirty: Needs Rebuild

```rust
use flui_tree::{Dirty, Mounted};

impl<A: Arity> ViewHandle<A, Dirty> {
    /// ✅ Access tree info (still mounted)
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()
    }

    // ❌ Cannot paint() - dirty state!
    // pub fn paint(&self, ctx: &PaintContext) { ... }  // Compile error!

    /// Transition: Dirty → Mounted
    pub fn rebuild(mut self, ctx: &BuildContext) -> ViewHandle<A, Mounted> {
        // Rebuild ViewObject
        if let Some(view_obj) = &mut self.view_object {
            view_obj.rebuild(ctx);
        }

        ViewHandle {
            view_config: self.view_config,
            view_object: self.view_object,
            tree_info: self.tree_info,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }

    /// Transition: Dirty → Reassembling
    pub fn reassemble(self) -> ViewHandle<A, Reassembling> {
        ViewHandle {
            view_config: self.view_config,
            view_object: self.view_object,
            tree_info: self.tree_info,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}
```

### Reassembling: Hot Reload in Progress

```rust
use flui_tree::{Reassembling, Mounted};

impl<A: Arity> ViewHandle<A, Reassembling> {
    /// ✅ Access tree info (still mounted)
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()
    }

    // ❌ Cannot paint() - reassembling!
    // pub fn paint(&self, ctx: &PaintContext) { ... }  // Compile error!

    /// Recreate ViewObject from updated config
    pub fn recreate_view_object(&mut self) {
        // Hot reload: recreate ViewObject from config
        self.view_object = Some(self.view_config.create_view_object());
    }

    /// Transition: Reassembling → Mounted
    pub fn finish_reassemble(mut self) -> ViewHandle<A, Mounted> {
        // Ensure ViewObject is recreated
        self.recreate_view_object();

        ViewHandle {
            view_config: self.view_config,
            view_object: self.view_object,
            tree_info: self.tree_info,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}
```

## Complete Lifecycle Example

### Counter Widget with State Changes

```rust
use flui_tree::{Unmounted, Mounted, Dirty};

// 1. Create unmounted view
let counter_config = Counter::new(0);
let unmounted = ViewHandle::<Single, Unmounted>::new(AnyView::new(counter_config));

// ✅ Can access config
let initial_count = unmounted.config().downcast_ref::<Counter>().unwrap().count;
assert_eq!(initial_count, 0);

// ❌ Cannot access tree_info() - compile error!
// let _ = unmounted.tree_info();  // ERROR: method not found

// 2. Mount into tree
let mounted = unmounted.mount(Some(parent_id));

// ✅ Can access tree info
let parent = mounted.tree_info().parent;
assert_eq!(parent, Some(parent_id));

// ✅ Can paint
mounted.paint(&paint_ctx);

// 3. State changes - mark dirty
let mut dirty = mounted.mark_dirty();

// ❌ Cannot paint() when dirty - compile error!
// dirty.paint(&paint_ctx);  // ERROR: method not found

// ✅ Can still access tree info
let parent = dirty.tree_info().parent;

// 4. Rebuild
let mounted = dirty.rebuild(&build_ctx);

// ✅ Can paint again
mounted.paint(&paint_ctx);
```

### Hot Reload Example

```rust
use flui_tree::{Mounted, Reassembling};

// Code change detected - trigger hot reload
let reassembling = mounted.reassemble();

// ❌ Cannot paint() during reassembly
// reassembling.paint(&paint_ctx);  // ERROR: method not found

// Recreate ViewObject from updated config
reassembling.recreate_view_object();

// Finish reassembly
let mounted = reassembling.finish_reassemble();

// ✅ Can paint with new ViewObject
mounted.paint(&paint_ctx);

// Recursively reassemble children
for child_id in mounted.tree_info().children {
    let child = tree.get_mut(child_id);
    child.reassemble_subtree();
}
```

## Framework Usage: TreeCoordinator

```rust
// Framework manages state transitions
impl TreeCoordinator {
    /// Mount new node
    fn mount_node(&mut self, config: AnyView, parent: Option<ElementId>) -> ElementId {
        // Create unmounted handle
        let unmounted = ViewHandle::<Variable, Unmounted>::new(config);

        // Mount: Unmounted → Mounted
        let mounted = unmounted.mount(parent.map(|id| id.get()));

        // Store in tree
        let element_id = self.elements.insert(Element::from_mounted(mounted));
        element_id
    }

    /// Mark node as dirty (state changed)
    fn mark_node_dirty(&mut self, element_id: ElementId) {
        let element = self.elements.get_mut(element_id);

        // Transition: Mounted → Dirty
        if let ElementState::Mounted(mounted) = element.state {
            element.state = ElementState::Dirty(mounted.mark_dirty());
        }
    }

    /// Rebuild dirty nodes
    fn rebuild_phase(&mut self) {
        let dirty_nodes: Vec<_> = self.elements.iter()
            .filter(|(_, e)| e.is_dirty())
            .map(|(id, _)| id)
            .collect();

        for element_id in dirty_nodes {
            let element = self.elements.get_mut(element_id);

            // Transition: Dirty → Mounted
            if let ElementState::Dirty(dirty) = element.state {
                element.state = ElementState::Mounted(dirty.rebuild(&build_ctx));
            }
        }
    }

    /// Hot reload all nodes
    fn hot_reload(&mut self) {
        // Phase 1: Mounted → Reassembling
        for (_, element) in self.elements.iter_mut() {
            if let ElementState::Mounted(mounted) = element.state {
                element.state = ElementState::Reassembling(mounted.reassemble());
            }
        }

        // Phase 2: Recreate ViewObjects
        for (_, element) in self.elements.iter_mut() {
            if let ElementState::Reassembling(reassembling) = &mut element.state {
                reassembling.recreate_view_object();
            }
        }

        // Phase 3: Reassembling → Mounted
        for (_, element) in self.elements.iter_mut() {
            if let ElementState::Reassembling(reassembling) = element.state {
                element.state = ElementState::Mounted(reassembling.finish_reassemble());
            }
        }
    }
}
```

## Benefits of Full Typestate

### 1. Compile-Time Guarantees

```rust
// ✅ OK: Paint only when clean
impl ViewHandle<Mounted> {
    pub fn paint(&self, ctx: &PaintContext) { /* ... */ }
}

let mounted: ViewHandle<Mounted> = ...;
mounted.paint(&ctx);  // ✅ Compiles

let dirty: ViewHandle<Dirty> = ...;
// dirty.paint(&ctx);  // ❌ Compile error - method not found!
```

### 2. Clear State Transitions

```rust
// All transitions are explicit and type-safe
Unmounted → mount() → Mounted
Mounted → mark_dirty() → Dirty
Dirty → rebuild() → Mounted
Mounted → reassemble() → Reassembling
Reassembling → finish_reassemble() → Mounted
```

### 3. Impossible States Are Impossible

```rust
// ❌ Cannot be both Mounted and Dirty
// let weird: ViewHandle<Mounted> = ViewHandle<Dirty> { ... };  // Type mismatch!

// ❌ Cannot paint while dirty
// impl ViewHandle<Dirty> {
//     pub fn paint(&self) { ... }  // Wrong! paint() not available for Dirty
// }
```

### 4. Hot Reload Safety

```rust
// ✅ Config preserved through all states
Unmounted → Mounted → Dirty → Mounted  // Config always present
Mounted → Reassembling → Mounted       // Config used for hot reload
```

## Summary

**Four states, full lifecycle control:**

- **Unmounted**: Config only, not in tree
- **Mounted**: In tree, clean, can paint
- **Dirty**: In tree, needs rebuild
- **Reassembling**: In tree, hot reload in progress

**Compile-time guarantees:**
- ✅ Only clean Mounted nodes can paint
- ✅ All transitions are explicit and type-safe
- ✅ Impossible states caught at compile time
- ✅ Config preserved for hot reload

**Flutter-like semantics:**
- `mount()` = Flutter's `Element.mount()`
- `mark_dirty()` = Flutter's `Element.markNeedsBuild()`
- `rebuild()` = Flutter's `Element.rebuild()`
- `reassemble()` = Flutter's `Element.reassemble()`
