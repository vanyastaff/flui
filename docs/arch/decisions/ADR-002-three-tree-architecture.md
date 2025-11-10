# ADR-002: Three-Tree Architecture

**Status:** ✅ Accepted
**Date:** 2025-01-10
**Deciders:** Core team
**Last Updated:** 2025-01-10

---

## Context and Problem Statement

Modern UI frameworks use different approaches for managing UI state and rendering:
- **Single tree** (e.g., Immediate Mode GUI) - Rebuild everything every frame
- **Two trees** (e.g., React) - Virtual DOM + Real DOM
- **Three trees** (e.g., Flutter) - Widget + Element + Render

**Problem:** Which tree architecture should FLUI use?

## Decision Drivers

- **Performance** - Minimize redundant work
- **Developer Experience** - Clear mental model
- **Flexibility** - Support advanced optimizations
- **Separation of Concerns** - Each tree has distinct responsibility
- **Proven Pattern** - Learn from existing frameworks

## Considered Options

### Option 1: Single Tree (Immediate Mode)

**Example:** egui, Dear ImGui

**Pros:**
- ✅ Simple mental model
- ✅ No state synchronization
- ✅ Easy to reason about

**Cons:**
- ❌ Rebuild entire UI every frame
- ❌ Hard to optimize (no incremental updates)
- ❌ No persistent state across frames

### Option 2: Two Trees (Virtual DOM)

**Example:** React, Yew

**Pros:**
- ✅ Incremental updates via diffing
- ✅ Clear separation (virtual vs real)
- ✅ Widely understood pattern

**Cons:**
- ❌ Layout lives in "real" tree (couples rendering to layout)
- ❌ No separation between state and layout
- ❌ Harder to implement advanced optimizations

### Option 3: Three Trees (Flutter's Approach)

**Example:** Flutter, FLUI

**Pros:**
- ✅ Clean separation of concerns:
  - Widget Tree: Immutable UI description
  - Element Tree: Mutable state & lifecycle
  - Render Tree: Layout & painting
- ✅ Maximum optimization potential
- ✅ Proven at scale (millions of Flutter apps)
- ✅ Enables relayout boundaries, repaint boundaries

**Cons:**
- ❌ More complex mental model
- ❌ Three trees to understand
- ❌ More implementation complexity

## Decision Outcome

**Chosen option:** **Option 3 - Three Trees (Flutter's Architecture)**

**Justification:**

1. **Proven at scale** - Flutter has validated this design in production
2. **Performance** - Enables sophisticated optimizations:
   - Rebuild only dirty components
   - Relayout only affected subtrees
   - Repaint only changed layers
3. **Separation of concerns** - Each tree has clear responsibility:
   - **View Tree** (immutable) - UI definition, cheap to recreate
   - **Element Tree** (mutable) - State, lifecycle, tree navigation
   - **Render Tree** (specialized) - Layout algorithms, painting logic
4. **Rust fit** - Ownership model naturally supports immutable widgets

## Tree Responsibilities

### View Tree (Immutable)

**Purpose:** Declarative UI description

**Characteristics:**
- Immutable (implement `View` trait)
- Cheap to create and discard
- No state (state lives in hooks or Element)
- Consumed during build (moved, not cloned)

**Example:**
```rust
#[derive(Debug)]
pub struct Container {
    pub child: Option<Box<dyn AnyView>>,
    pub padding: Option<EdgeInsets>,
}

impl View for Container {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Build is called when widget changes
        // Returns Element or RenderObject
    }
}
```

### Element Tree (Mutable)

**Purpose:** State management & lifecycle

**Characteristics:**
- Mutable (stored in `Slab<Element>`)
- Persistent across rebuilds
- Manages child relationships
- Owns RenderObject (if RenderElement)
- Caches layout results

**Example:**
```rust
pub enum Element {
    Component(ComponentElement), // Composes other widgets
    Render(RenderElement),       // Owns RenderObject
    Provider(ProviderElement),   // Provides inherited data
}
```

### Render Tree (Specialized)

**Purpose:** Layout computation & painting

**Characteristics:**
- Implements `Render` trait
- Pure functions (layout, paint)
- No state (uses Element's cache)
- Specialized per layout algorithm

**Example:**
```rust
impl Render for RenderPadding {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // Compute size based on child + padding
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        // Paint child with offset
    }
}
```

## Data Flow

```text
User Code (Widget Tree)
    ↓ build()
Element Tree (State)
    ↓ owns
Render Tree (Layout/Paint)
    ↓ generates
Layer Tree (GPU)
```

**Rebuild Flow:**
1. Signal changes → Mark element dirty
2. Rebuild phase: Call `view.build()` again
3. Diff: Update Element tree incrementally
4. Layout phase: Compute sizes (cached)
5. Paint phase: Generate layers (if needed)

## Consequences

### Positive Consequences

- ✅ **Maximum performance** - Incremental updates at every level
- ✅ **Clear architecture** - Each tree has single responsibility
- ✅ **Optimization potential** - Relayout/repaint boundaries
- ✅ **Flutter compatibility** - Familiar to Flutter developers
- ✅ **Type safety** - Rust ownership prevents common bugs

### Negative Consequences

- ❌ **Complexity** - Three trees to understand (learning curve)
- ❌ **Implementation effort** - More code than simpler approaches
- ❌ **Debugging** - Need tools to visualize all three trees

### Neutral Consequences

- **Memory:** Higher than immediate mode (three trees in memory)
  - Mitigated by: Slab allocator, layout caching
- **Developer ergonomics:** More concepts to learn
  - Mitigated by: Good documentation, examples

## Implementation Notes

**Locations:**
- **View Tree**: `crates/flui_widgets/src/` (high-level widgets)
- **Element Tree**: `crates/flui_core/src/element/` (element management)
- **Render Tree**: `crates/flui_rendering/src/objects/` (RenderObjects)

**Key Optimizations:**
1. **Layout Caching** - Element caches layout results
2. **Dirty Tracking** - Only rebuild/relayout/repaint changed subtrees
3. **Enum Dispatch** - Element enum (3.75x faster than trait objects)

## Validation

**How to verify:**
- ✅ View tree is immutable (widgets don't store state)
- ✅ Element tree is mutable (stored in Slab)
- ✅ Render tree is pure functions (no internal state)
- ✅ Performance benchmarks show incremental update benefits

**Metrics:**
- Rebuild performance: O(changed subtrees) not O(total tree)
- Layout performance: 80% cache hit rate
- Paint performance: Skips unchanged layers

## Links

### Related Documents
- [CORE_FEATURES_ROADMAP.md](../CORE_FEATURES_ROADMAP.md#three-tree-architecture)
- [PATTERNS.md](../PATTERNS.md#three-tree-architecture)
- [INTEGRATION.md](../INTEGRATION.md#flow-1-widget--element--render)

### Related ADRs
- [ADR-001: Unified Render Trait](ADR-001-unified-render-trait.md)
- [ADR-003: Enum vs Trait Objects](ADR-003-enum-vs-trait-objects.md)

### Implementation
- `crates/flui_core/src/view/view.rs` - View trait
- `crates/flui_core/src/element/element.rs` - Element enum
- `crates/flui_core/src/render/render.rs` - Render trait

### External References
- [Flutter's Rendering Pipeline](https://docs.flutter.dev/resources/architectural-overview#rendering-and-layout)
- [React Fiber Architecture](https://github.com/acdlite/react-fiber-architecture) - Two-tree comparison
