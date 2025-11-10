# FLUI Architecture Patterns

**Quick Reference Guide**
**Last Updated:** 2025-01-10

> This document provides quick reference to common architectural patterns used throughout FLUI. For detailed explanations, see the individual architecture documents linked in each section.

---

## Table of Contents

1. [Core Architecture Patterns](#core-architecture-patterns)
2. [Rendering Patterns](#rendering-patterns)
3. [State Management Patterns](#state-management-patterns)
4. [Layout Patterns](#layout-patterns)
5. [Performance Patterns](#performance-patterns)
6. [Thread-Safety Patterns](#thread-safety-patterns)

---

## Core Architecture Patterns

### Three-Tree Architecture

**Pattern:** View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)

**Purpose:** Separation of concerns between UI definition, state management, and rendering

**Implementation:**
- **View Tree**: Immutable widgets implementing `View` trait
- **Element Tree**: Mutable elements stored in `Slab<Element>`
- **Render Tree**: Layout/paint objects implementing `Render` trait

**Key Files:**
- `crates/flui_core/src/view/view.rs` - View trait definition
- `crates/flui_core/src/element/element.rs` - Element enum
- `crates/flui_core/src/render/render.rs` - Render trait

**See Also:**
- [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md#three-tree-architecture)
- [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#three-tree-architecture)

### Unified View Trait

**Pattern:** Single `View` trait with `build()` method returning `impl IntoElement`

**Purpose:** Simplified widget creation without GATs or complex type parameters

**Algorithm:**
```
1. Implement View trait
2. Define build() method
3. Return (RenderObject, children) OR composed widget
4. Framework handles tree insertion automatically
```

**Key Benefits:**
- 75% less boilerplate vs old Component trait
- No GAT (Generic Associated Types) complexity
- Automatic tree insertion via IntoElement

**Key Files:**
- `crates/flui_core/src/view/view.rs:15-25` - View trait
- `crates/flui_core/src/view/into_element.rs` - IntoElement implementations

**See Also:** [VIEW_API_MIGRATION_COMPLETE.md](../VIEW_API_MIGRATION_COMPLETE.md)

### Element Enum Storage

**Pattern:** Enum-based heterogeneous storage instead of `Box<dyn Trait>`

**Purpose:** 3.75x faster dispatch, better cache locality

**Implementation:**
```rust
pub enum Element {
    Component(ComponentElement),
    Render(RenderElement),
    Provider(ProviderElement),
}
```

**Performance:**
- Element access: 40μs (enum) vs 150μs (Box<dyn>)
- Element dispatch: 50μs (enum) vs 180μs (Box<dyn>)
- Cache hit rate: 80% (enum) vs 40% (Box<dyn>)

**Key Files:**
- `crates/flui_core/src/element/element.rs:25-50` - Element enum definition

**See Also:** ADR-003 (when created) - Enum vs trait object decision

---

## Rendering Patterns

### Unified Render Trait

**Pattern:** Single trait for all RenderObjects (0, 1, or N children) via Arity system

**Purpose:** Simpler than Flutter's 3 mixin traits (LeafRender/SingleRender/MultiRender)

**Algorithm:**
```
1. Implement Render trait
2. Define layout() - compute size from constraints
3. Define paint() - generate layer tree
4. Define arity() - specify child count (Exact(n) or Variable)
5. Framework validates child count at runtime
```

**Key Files:**
- `crates/flui_core/src/render/render.rs:115-196` - Render trait
- `crates/flui_core/src/render/arity.rs` - Arity enum

**See Also:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#unified-render-trait)

### Context Pattern

**Pattern:** LayoutContext/PaintContext for clean RenderObject API

**Purpose:** Avoid parameter bloat, provide consistent interface

**Implementation:**
```rust
// LayoutContext provides:
- tree: &ElementTree (read-only)
- constraints: BoxConstraints
- children: Children
- layout_child(), get_metadata()

// PaintContext provides:
- tree: &ElementTree (read-only)
- offset: Offset (absolute)
- children: Children
- paint_child()
```

**Key Files:**
- `crates/flui_core/src/render/context.rs` - Context structs

**See Also:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#context-pattern)

### ParentData Metadata

**Pattern:** Per-child metadata via `as_any()` downcasting

**Purpose:** Allow parent RenderObjects to access child-specific data (e.g., flex factor)

**Algorithm:**
```
1. Child RenderObject stores metadata struct
2. Child implements as_any() returning self
3. Parent downcasts via as_any().downcast_ref::<T>()
4. Parent uses metadata in layout calculation
```

**Example:** RenderFlex accesses FlexItemMetadata from RenderFlexible children

**Key Files:**
- `crates/flui_rendering/src/objects/layout/flex_item.rs` - Metadata example

**See Also:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#parentdata-and-metadata)

---

## State Management Patterns

### Copy-Based Signals

**Pattern:** 8-byte Copy-able signal handles with thread-local storage

**Purpose:** Ergonomic reactive state without explicit cloning

**Implementation:**
```rust
pub struct Signal<T> {
    id: SignalId,  // 8 bytes, Copy
    _phantom: PhantomData<T>,
}

// Data stored separately in thread-local SignalRuntime
SIGNAL_RUNTIME.with(|rt| rt.get_value(signal.id))
```

**Benefits:**
- No .clone() needed before moving into closures
- Automatic dependency tracking
- Thread-safe (via Arc<Mutex<T>> in runtime)

**Key Files:**
- `crates/flui_core/src/hooks/signal.rs` - Signal implementation
- `crates/flui_core/src/hooks/runtime.rs` - SignalRuntime

**See Also:** [THREAD_SAFE_HOOKS_REFACTORING.md](../THREAD_SAFE_HOOKS_REFACTORING.md)

### Hook Rules

**Pattern:** Strict ordering rules for hook calls

**Purpose:** Ensure consistent state across rebuilds

**Rules:**
1. ✅ Always call hooks in the same order
2. ❌ Never call hooks conditionally
3. ❌ Never call hooks in loops with variable iterations
4. ✅ Only call hooks at component top level
5. ✅ Clone signals before moving into closures

**Violations cause PANICS** - see hook rules documentation for details.

**Key Files:**
- `crates/flui_core/src/hooks/RULES.md` - Complete hook rules

**See Also:** [CLAUDE.md](../../CLAUDE.md#state-management-with-hooks)

### Persistent Object Pattern

**Pattern:** Long-lived objects (AnimationController, FocusNode) managed by widgets

**Purpose:** Object survives widget rebuilds, maintains state

**Algorithm:**
```
1. Create persistent object (Arc-wrapped)
2. Widget receives object reference
3. Widget attaches object to element tree on mount
4. Widget detaches object on unmount
5. Object survives across widget rebuilds
```

**Key Files:**
- `crates/flui_animation/src/animation_controller.rs` - Example implementation

**See Also:** [ANIMATION_ARCHITECTURE.md](ANIMATION_ARCHITECTURE.md#persistent-object-pattern)

---

## Layout Patterns

### Layout Caching

**Pattern:** Cache layout results in Element (not RenderObject)

**Purpose:** Skip redundant layout calculations, enable relayout boundaries

**Algorithm:**
```
1. Element checks cache for (constraints → size) mapping
2. Cache hit: Return cached size (O(1))
3. Cache miss: Call RenderObject.layout(), store result
4. Cache invalidated when RenderObject changes
```

**Performance:**
- Cache hit rate: ~80% in typical UIs
- Enables subtree skipping (relayout boundaries)

**Key Files:**
- `crates/flui_core/src/render/cache.rs` - LayoutCache implementation
- `crates/flui_core/src/element/element.rs:845-872` - Cache usage

**See Also:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#layout-caching)

### Intrinsic Sizing

**Pattern:** Optional intrinsic dimensions for special layout widgets

**Purpose:** Support IntrinsicWidth/IntrinsicHeight widgets

**Implementation:**
```rust
// Render trait provides optional methods:
fn get_intrinsic_width(&self, height: Option<f32>) -> Option<f32> { None }
fn get_intrinsic_height(&self, width: Option<f32>) -> Option<f32> { None }

// Most RenderObjects return None (no intrinsic size)
// Text, Image return computed intrinsic dimensions
```

**Key Files:**
- `crates/flui_core/src/render/render.rs:185-193` - Intrinsic methods

---

## Performance Patterns

### Slab-Based Arena Allocation

**Pattern:** Slab allocator for element storage with stable IDs

**Purpose:** O(1) insert/remove, cache-friendly contiguous storage

**Implementation:**
```rust
pub struct ElementTree {
    nodes: Slab<ElementNode>,  // 0-based indices
}

// CRITICAL: +1/-1 offset pattern
let slab_index = self.nodes.insert(node);  // 0-based
ElementId::new(slab_index + 1)             // 1-based (NonZeroUsize)

// Access:
self.nodes.get(element_id.get() - 1)  // -1 to get slab index
```

**Performance:**
- Insert: O(1) amortized
- Remove: O(1)
- Access: O(1) direct indexing

**Key Files:**
- `crates/flui_core/src/element/element_tree.rs` - Slab usage
- `crates/flui_core/src/foundation/element_id.rs` - ElementId with NonZeroUsize

**See Also:** [CLAUDE.md](../../CLAUDE.md#elementid-offset-pattern)

### Niche Optimization

**Pattern:** NonZeroUsize for Option<ElementId> size optimization

**Purpose:** 8-byte Option<ElementId> instead of 16 bytes

**Implementation:**
```rust
pub struct ElementId(NonZeroUsize);

// Result: Option<ElementId> uses niche optimization
assert_eq!(size_of::<ElementId>(), 8);
assert_eq!(size_of::<Option<ElementId>>(), 8);  // Same size!
```

**Benefit:** 2x memory reduction for optional element references

**Key Files:**
- `crates/flui_core/src/foundation/element_id.rs:10-15`

### Dirty Tracking

**Pattern:** Lock-free dirty flags + topological sort

**Purpose:** Efficient incremental updates in pipeline

**Algorithm:**
```
1. Mark element dirty (atomic flag set)
2. Add to dirty queue (lock-free)
3. Topological sort: Process parents before children
4. Flush dirty elements in order
5. Clear dirty flags
```

**Key Files:**
- `crates/flui_core/src/pipeline/dirty_tracking.rs`
- `crates/flui_core/src/pipeline/pipeline_owner.rs:314-325`

---

## Thread-Safety Patterns

### Arc/Mutex for Shared State

**Pattern:** Arc<Mutex<T>> for thread-safe shared ownership

**Purpose:** Enable parallel build pipeline and multi-threaded UI

**Implementation:**
```rust
// Use parking_lot::Mutex (2-3x faster than std)
use parking_lot::Mutex;

pub struct Signal<T> {
    value: Arc<Mutex<T>>,
    // ...
}
```

**Benefits:**
- parking_lot is 2-3x faster than std::sync::Mutex
- No poisoning (simpler error handling)
- Smaller memory footprint

**Key Files:**
- All hook implementations in `crates/flui_core/src/hooks/`

**See Also:** [THREAD_SAFE_HOOKS_REFACTORING.md](../THREAD_SAFE_HOOKS_REFACTORING.md)

### Send + Sync Bounds

**Pattern:** Explicit Send + Sync bounds on all shared types

**Purpose:** Compile-time thread-safety guarantees

**Implementation:**
```rust
pub trait Render: Send + Sync + Debug + 'static { ... }
pub trait View: 'static { ... }  // View consumed, not shared

// Signal values must be Send
impl<T: Send + 'static> Signal<T> { ... }
```

**Key Files:**
- `crates/flui_core/src/render/render.rs:133` - Render trait bounds
- `crates/flui_core/src/hooks/signal.rs:45` - Signal bounds

### Thread-Local BuildContext

**Pattern:** Thread-local storage for BuildContext during build

**Purpose:** Avoid passing context parameter through deep call chains

**Implementation:**
```rust
// Framework sets up thread-local before calling View::build()
thread_local! {
    static BUILD_CONTEXT: RefCell<Option<&'static BuildContext>> = RefCell::new(None);
}

// Views access context via current_build_context()
impl View for MyWidget {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // ctx available via parameter (no thread-local needed)
    }
}
```

**Key Files:**
- `crates/flui_core/src/view/build_context.rs` - Thread-local setup

---

## Summary

This document provides quick patterns reference. For detailed explanations:

- **Architecture details**: See individual `*_ARCHITECTURE.md` files
- **Integration flows**: See [INTEGRATION.md](INTEGRATION.md)
- **Decision rationale**: See `decisions/ADR-*.md` (when created)
- **Code examples**: See rustdoc and source files

**Key Principle:** Patterns documented here, implementation in code, detailed explanations in architecture docs.

---

## Navigation

- [Back to Architecture Index](README.md)
- [Integration Guide](INTEGRATION.md)
- [Core Architecture](CORE_FEATURES_ROADMAP.md)
- [Rendering Architecture](RENDERING_ARCHITECTURE.md)
