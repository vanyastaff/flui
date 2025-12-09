# FLUI Typestate System - Final Design

**Status**: ✅ Implemented
**Date**: 2025-12-09
**Location**: `flui-tree/src/state.rs`

This document describes the final typestate design for FLUI's three-tree architecture after implementing the structural vs lifecycle separation.

---

## Table of Contents

- [Overview](#overview)
- [Core Principle: Structural vs Lifecycle](#core-principle-structural-vs-lifecycle)
- [Type System](#type-system)
- [Traits](#traits)
- [TreeInfo Design](#treeinfo-design)
- [Usage Examples](#usage-examples)
- [RenderHandle Extensions](#renderhandle-extensions)

---

## Overview

FLUI uses typestate pattern to enforce compile-time guarantees about node lifecycle across all three trees (View, Element, Render).

**Key Insight**: Typestate tracks **structural state** (where node is), runtime flags track **lifecycle state** (what needs doing).

### Philosophy

Similar to [`Arity`](../../crates/flui-tree/src/arity/mod.rs), typestate is a **pure abstraction** that works universally:

```rust
// Arity tracks child count constraints (compile-time)
RenderBox<Leaf>      // 0 children
RenderBox<Single>    // 1 child
RenderBox<Variable>  // N children

// NodeState tracks tree membership (compile-time)
ViewHandle<Unmounted>  // Not in tree
ViewHandle<Mounted>    // In tree
```

---

## Core Principle: Structural vs Lifecycle

### The Split

| Aspect | Structural State | Lifecycle State |
|--------|-----------------|-----------------|
| **What** | Where is node? | What needs doing? |
| **Tracked by** | Typestate (compile-time) | Runtime flags |
| **Values** | `Unmounted`, `Mounted` | `needs_build`, `needs_layout`, `needs_paint` |
| **Transitions** | One-way (unmount → mount) | Cyclic (clean → dirty → clean) |
| **Location** | `flui-tree` (universal) | Each crate (domain-specific) |

### Why This Separation?

**Structural state is compile-time:**
```rust
// ✅ Compile-time guarantee
impl ViewHandle<Mounted> {
    pub fn parent(&self) -> Option<usize> {
        self.tree_info.as_ref().unwrap()  // Safe - always Some
    }
}

// ❌ Compile error - no parent() for Unmounted
let unmounted = ViewHandle::<Unmounted>::new(...);
// unmounted.parent();  // Doesn't compile!
```

**Lifecycle state is runtime:**
```rust
// ✅ Runtime flag - node stays Mounted
let mut mounted = view_handle;
mounted.mark_needs_build();  // Sets flag, stays Mounted

// ❌ Wrong - would require type change
// let dirty: ViewHandle<Dirty> = mounted.mark_dirty();
```

### Flutter's Design

This matches Flutter exactly:

```dart
// Flutter Element
abstract class Element {
  Element? _parent;           // Structural
  bool _active = true;        // Structural
  bool _dirty = false;        // Lifecycle ✅

  void markNeedsBuild() {
    _dirty = true;  // Runtime flag
  }
}

// Flutter RenderObject
abstract class RenderObject {
  bool _needsLayout = false;  // Lifecycle ✅
  bool _needsPaint = false;   // Lifecycle ✅
}
```

---

## Type System

### State Markers

Two structural states only:

```rust
/// Not in tree (has config only)
pub struct Unmounted;

/// In tree (has config + live object + position)
pub struct Mounted;

/// Sealed trait - only Unmounted and Mounted
pub trait NodeState: sealed::Sealed + Send + Sync + Copy + 'static {
    const IS_MOUNTED: bool;
    fn state_name() -> &'static str;
}
```

### State Transitions

```text
┌───────────┐                    ┌─────────┐
│ Unmounted │──── mount() ───────▶│ Mounted │
│           │                     │         │
│ Config    │◀──── unmount() ────│ Config  │
│           │                     │ + Live  │
└───────────┘                     │ + Tree  │
                                  └─────────┘
                                       │
                                       │ Runtime flags
                                       │ (stays Mounted)
                                       ▼
                                  needs_build
                                  needs_layout
                                  needs_paint
```

**Structural**: Unmounted ↔ Mounted (one-way per insert/remove)
**Lifecycle**: Clean ↔ Dirty (cyclic, many times)

---

## Traits

### Mountable - Unmounted → Mounted

```rust
pub trait Mountable: Sized {
    /// Bidirectional type-level connection
    type Mounted: Unmountable<Unmounted = Self>;

    /// Consume unmounted, return mounted
    fn mount(self, parent: Option<usize>) -> Self::Mounted;
}

impl Mountable for ViewHandle<Unmounted> {
    type Mounted = ViewHandle<Mounted>;

    fn mount(self, parent: Option<usize>) -> Self::Mounted {
        let tree_info = if let Some(parent_id) = parent {
            TreeInfo::with_parent(parent_id, 0)
        } else {
            TreeInfo::root()
        };

        ViewHandle {
            config: self.config,
            view_object: Some(self.config.create_view_object()),
            tree_info: Some(tree_info),
            _state: PhantomData,
        }
    }
}
```

### Unmountable - Mounted → Unmounted + TreeInfo Access

```rust
pub trait Unmountable: Sized {
    /// Bidirectional type-level connection
    type Unmounted: Mountable<Mounted = Self>;

    /// Consume mounted, return unmounted (preserve config)
    fn unmount(self) -> Self::Unmounted;

    /// Access tree position (always safe for Mounted)
    fn tree_info(&self) -> &TreeInfo;
    fn tree_info_mut(&mut self) -> &mut TreeInfo;
}

impl Unmountable for ViewHandle<Mounted> {
    type Unmounted = ViewHandle<Unmounted>;

    fn unmount(self) -> Self::Unmounted {
        ViewHandle {
            config: self.config,  // Preserve for hot-reload
            view_object: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()  // Safe - always Some
    }

    fn tree_info_mut(&mut self) -> &mut TreeInfo {
        self.tree_info.as_mut().unwrap()
    }
}
```

### NavigableHandle - Extension Trait (Auto-Implemented)

```rust
/// Convenient navigation methods for any mounted handle
pub trait NavigableHandle: Unmountable {
    #[inline]
    fn parent(&self) -> Option<usize> {
        self.tree_info().parent
    }

    #[inline]
    fn children(&self) -> &[usize] {
        &self.tree_info().children
    }

    #[inline]
    fn depth(&self) -> usize {
        self.tree_info().depth
    }

    #[inline]
    fn is_root(&self) -> bool {
        self.tree_info().is_root()
    }

    #[inline]
    fn child_count(&self) -> usize {
        self.tree_info().child_count()
    }

    #[inline]
    fn add_child(&mut self, child_id: usize) {
        self.tree_info_mut().add_child(child_id);
    }

    #[inline]
    fn remove_child(&mut self, child_id: usize) -> bool {
        self.tree_info_mut().remove_child(child_id)
    }
}

// Auto-implement for ALL Unmountable types!
impl<T: Unmountable> NavigableHandle for T {}
```

**Benefits:**
- ✅ TreeCoordinator can use generic bounds: `fn traverse<H: NavigableHandle>(handle: &H)`
- ✅ Zero-cost: all methods inlined
- ✅ Follows Rust patterns (like `IteratorExt`)
- ✅ Works for View, Element, Render handles automatically

---

## TreeInfo Design

### Universal Structure (No Generics)

```rust
/// Tree position for mounted nodes (universal for all trees)
pub struct TreeInfo {
    pub parent: Option<usize>,   // Generic ID
    pub children: Vec<usize>,    // Generic IDs
    pub depth: usize,
}
```

**Why `usize` instead of generic `I: Identifier`?**

1. **Philosophy**: Like Arity works with counts (not typed children), TreeInfo works with positions (not typed IDs)
2. **Simplicity**: No cascading generics (`ViewHandle<S, I>` vs `ViewHandle<S>`)
3. **Flexibility**: Conversion happens at domain boundary

### Type Conversion at Boundary

```rust
// flui-tree: pure abstraction with usize
pub struct TreeInfo {
    pub parent: Option<usize>,
}

// flui-view: domain-specific typed methods
impl ViewHandle<Mounted> {
    /// Convert usize to ViewId at API boundary
    pub fn parent(&self) -> Option<ViewId> {
        self.tree_info()
            .parent
            .map(ViewId::from_raw)  // ← Conversion here
    }

    pub fn children(&self) -> impl Iterator<Item = ViewId> + '_ {
        self.tree_info()
            .children
            .iter()
            .map(|&id| ViewId::from_raw(id))  // ← Conversion here
    }
}

// flui_rendering: same pattern
impl<P: Protocol> RenderHandle<Mounted, P> {
    pub fn parent(&self) -> Option<RenderId> {
        self.tree_info().parent.map(RenderId::from_raw)
    }
}
```

**Type safety**: Ensured at domain layer, not in abstraction layer.

---

## Usage Examples

### Generic TreeCoordinator

```rust
impl TreeCoordinator {
    /// Works for ANY mounted handle type!
    fn traverse<H: NavigableHandle>(&self, handle: &H, visitor: &mut impl Visitor<H>) {
        visitor.visit(handle);

        for child_id in handle.children() {
            // Recursively traverse
        }
    }

    /// Collect all roots (generic)
    fn collect_roots<H: NavigableHandle>(&self, handles: &[H]) -> Vec<usize> {
        handles.iter()
            .enumerate()
            .filter(|(_, h)| h.is_root())
            .map(|(i, _)| i)
            .collect()
    }

    /// Update parent (generic)
    fn reparent<H: Unmountable>(&mut self, handle: &mut H, new_parent: usize) {
        handle.tree_info_mut().parent = Some(new_parent);
    }
}
```

### ViewHandle Usage

```rust
// Create unmounted view
let view = ViewHandle::<Unmounted>::new(Padding::all(16.0));

// Mount it
let mut mounted = view.mount(None);  // Mountable trait

// NavigableHandle methods (auto-implemented via Unmountable)
assert!(mounted.is_root());
assert_eq!(mounted.depth(), 0);

// Modify tree structure
mounted.add_child(child_id);

// Access tree info directly
let info = mounted.tree_info();

// Unmount when done (for hot-reload)
let unmounted = mounted.unmount();
```

### ElementHandle Usage

```rust
pub struct ElementHandle<S: NodeState> {
    config: ElementConfig,
    view_object: Option<Box<dyn ViewObject>>,
    render_id: Option<RenderId>,
    tree_info: Option<TreeInfo>,
    _state: PhantomData<S>,
}

impl Mountable for ElementHandle<Unmounted> {
    type Mounted = ElementHandle<Mounted>;
    // ... implementation
}

impl Unmountable for ElementHandle<Mounted> {
    type Unmounted = ElementHandle<Unmounted>;
    // ... gets NavigableHandle automatically!
}
```

### RenderHandle Usage

```rust
pub struct RenderHandle<S: NodeState, P: Protocol> {
    id: RenderId,
    config: RenderConfig,
    render_object: Option<Box<dyn RenderObject>>,
    state: Option<RenderState<P>>,
    tree_info: Option<TreeInfo>,
    _marker: PhantomData<S>,
}

// Generic over Protocol, not over ID type!
```

---

## RenderHandle Extensions

### Parent Data

For layout hints from parent (like Flutter's ParentData):

```rust
pub struct RenderHandle<S: NodeState, P: Protocol> {
    id: RenderId,
    config: RenderConfig,
    render_object: Option<Box<dyn RenderObject>>,
    state: Option<RenderState<P>>,
    tree_info: Option<TreeInfo>,

    // Parent data - separate from TreeInfo
    parent_data: Option<Box<dyn Any + Send + Sync>>,

    _marker: PhantomData<S>,
}
```

**Why separate from TreeInfo?**

- `TreeInfo` - **structural** (who, where in tree)
- `parent_data` - **data from parent** (layout hints, positioning)
- `RenderState` - **lifecycle + geometry** (flags, size, constraints)

**Responsibility separation:**

```rust
// TreeInfo - universal (flui-tree)
pub struct TreeInfo {
    pub parent: Option<usize>,   // WHO is parent
    pub children: Vec<usize>,    // WHO are children
    pub depth: usize,            // WHERE in tree
}

// RenderState - protocol-specific (flui_rendering)
pub struct RenderState<P: Protocol> {
    flags: AtomicRenderFlags,         // Lifecycle
    geometry: OnceCell<P::Geometry>,  // Layout result
    constraints: OnceCell<P::Constraints>,
    offset: AtomicOffset,
}

// parent_data - parent-specific (flui_rendering)
pub struct StackParentData {
    pub left: Option<f32>,   // Hint from parent
    pub top: Option<f32>,    // Hint from parent
    // ...
}
```

---

## Benefits of This Design

### 1. Clean Separation

```text
┌─────────────────┐
│   flui-tree     │  Pure abstractions
│  (typestate)    │  • Unmounted / Mounted
│                 │  • TreeInfo (usize)
│                 │  • NavigableHandle
└────────┬────────┘
         │ uses
         ▼
┌─────────────────┐
│   flui-view     │  Domain-specific
│ flui-element    │  • ViewId / ElementId
│ flui_rendering  │  • Typed methods
│                 │  • Lifecycle flags
└─────────────────┘
```

### 2. Type Safety at Right Level

- **Compile-time**: Structural state (Unmounted vs Mounted)
- **Runtime**: Lifecycle flags (needs_build, needs_layout)
- **Domain boundary**: Type conversion (usize → ViewId)

### 3. Generic Algorithms

```rust
// TreeCoordinator works with ANY mounted handle
fn process<H: NavigableHandle>(handle: &H) {
    // Works for View, Element, Render!
}
```

### 4. Flutter Alignment

Matches Flutter's actual design:
- Structural lifecycle (mounted/active) ← typestate
- Runtime flags (_dirty, _needsLayout) ← runtime

### 5. Zero-Cost Abstractions

- PhantomData has zero size
- All NavigableHandle methods inline
- Static dispatch (no trait objects)

---

## Migration from 4-State System

### Old Design (Discarded)

```rust
// ❌ Too much in typestate
pub struct Unmounted;
pub struct Mounted;
pub struct Dirty;        // Should be runtime flag
pub struct Reassembling; // Should be runtime flag
```

### New Design (Current)

```rust
// ✅ Structural only
pub struct Unmounted;
pub struct Mounted;

// Runtime flags in each crate
impl Element {
    needs_build: bool,     // ViewTree lifecycle
}

impl RenderObject {
    needs_layout: bool,    // RenderTree lifecycle
    needs_paint: bool,     // RenderTree lifecycle
}
```

**Why change?**

1. **Orthogonal concerns**: Structural state (where) vs Lifecycle (what)
2. **Cyclic transitions**: Dirty → Clean → Dirty many times while Mounted
3. **Flutter alignment**: Matches Element._dirty, RenderObject._needsLayout
4. **Simplicity**: 2 states instead of 4

---

## Summary

### Core Design Decisions

| Decision | Rationale |
|----------|-----------|
| **2 states only** | Structural (Unmounted/Mounted), lifecycle as runtime |
| **TreeInfo uses usize** | Universal abstraction, typed conversion at boundary |
| **NavigableHandle auto-impl** | Convenience methods for all mounted handles |
| **Bidirectional traits** | Type-level guarantee of mount/unmount cycle |
| **parent_data separate** | Different concern from tree position |

### Files

- **Implementation**: `crates/flui-tree/src/state.rs`
- **Tests**: 121 tests pass (including NavigableHandle tests)
- **Exports**: `flui-tree::prelude::*`

### Next Steps

1. **Phase 2**: Apply typestate to ViewHandle (flui-view)
2. **Phase 3**: Apply typestate to ElementHandle (flui-element)
3. **Phase 4**: Apply typestate to RenderHandle (flui_rendering)
4. **Phase 5**: Implement Flutter-like child mounting API

---

## References

- **Typestate Pattern**: [Rust Design Patterns](https://rust-unofficial.github.io/patterns/patterns/behavioural/typestate.html)
- **Flutter Element**: [Element class](https://api.flutter.dev/flutter/widgets/Element-class.html)
- **Flutter RenderObject**: [RenderObject class](https://api.flutter.dev/flutter/rendering/RenderObject-class.html)
- **flui-tree arity**: `crates/flui-tree/src/arity/mod.rs`

---

**Document Status**: ✅ Current as of 2025-12-09 (reflects implemented design)
