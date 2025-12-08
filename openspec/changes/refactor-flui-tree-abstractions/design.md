# Design: Pure Tree Abstractions for flui-tree

## Architectural Overview

### Current State (Problem)

```
flui-tree (18,600 lines)
├── Generic tree code (~5,000 lines) ✓
├── Render-specific code (~5,000 lines) ✗
├── Element-specific code (~2,000 lines) ✗
├── View-specific code (~700 lines) ✗
├── Pipeline-specific code (~600 lines) ✗
├── Debug/validation code (~1,200 lines) ✗
└── Convenience/combined (~400 lines) ✗
```

### Target State (Solution)

```
flui-tree (~3,000 lines)
└── Pure generic tree abstractions ONLY
    ├── Core traits (TreeNode, TreeRead, TreeWrite, TreeNav)
    ├── Generic iterators (ancestors, descendants, siblings, DFS, BFS)
    ├── Generic visitors (collect, count, find, for_each)
    └── Arity system (child count validation)

Each crate has its OWN tree implementation using flui-tree traits:

flui-view/
└── src/tree/
    └── view_tree.rs        # ViewTree impl

flui-element/ (or flui_core)
└── src/element/
    └── element_tree.rs     # ElementTree impl
    └── reconciliation.rs   # Reconciler, etc.
    └── lifecycle.rs        # Lifecycle states
    └── inherited.rs        # InheritedData

flui_rendering/
└── src/tree/
    └── render_tree.rs      # RenderTree impl
    └── dirty.rs            # DirtyTracking
    └── iter.rs             # Render-specific iterators

flui-layer/ (future)
└── src/
    └── layer_tree.rs       # LayerTree impl

flui-semantics/ (future)
└── src/
    └── semantics_tree.rs   # SemanticsTree impl
```

## Tree Type Responsibilities

### flui-tree (abstractions only)

```rust
// Core traits - NO implementations, just interfaces
pub trait TreeNode: Send + Sync {
    fn parent(&self) -> Option<ElementId>;
    fn children(&self) -> impl Iterator<Item = ElementId>;
}

pub trait TreeRead: Send + Sync {
    type Node: TreeNode;
    fn get(&self, id: ElementId) -> Option<&Self::Node>;
    fn len(&self) -> usize;
}

pub trait TreeNav: TreeRead {
    fn ancestors(&self, id: ElementId) -> Ancestors<'_, Self>;
    fn descendants(&self, root: ElementId) -> Descendants<'_, Self>;
    // ...
}

pub trait TreeWrite: TreeNav {
    fn insert(&mut self, node: Self::Node, parent: Option<ElementId>) -> ElementId;
    fn remove(&mut self, id: ElementId) -> Option<Self::Node>;
    // ...
}
```

### ViewTree (in flui-view)

```rust
// Immutable snapshots of View hierarchy
pub struct ViewTree {
    nodes: Slab<ViewNode>,
    root: Option<ElementId>,
}

impl TreeRead for ViewTree { ... }
impl TreeNav for ViewTree { ... }
// ViewTree is mostly read-only after build
```

### ElementTree (in flui-element/flui_core)

```rust
// Mutable element management with lifecycle
pub struct ElementTree {
    nodes: Slab<Element>,
    root: Option<ElementId>,
    dirty_elements: HashSet<ElementId>,
}

impl TreeRead for ElementTree { ... }
impl TreeNav for ElementTree { ... }
impl TreeWrite for ElementTree { ... }

// Element-specific extensions
impl ElementTree {
    pub fn mark_needs_build(&mut self, id: ElementId);
    pub fn reconcile_children(&mut self, ...);
    pub fn activate(&mut self, id: ElementId);
    pub fn deactivate(&mut self, id: ElementId);
}
```

### RenderTree (in flui_rendering)

```rust
// Layout and paint tree
pub struct RenderTree {
    nodes: Slab<RenderNode>,
    root: Option<ElementId>,
    needs_layout: DirtySet,
    needs_paint: DirtySet,
}

impl TreeRead for RenderTree { ... }
impl TreeNav for RenderTree { ... }
impl TreeWrite for RenderTree { ... }

// Render-specific extensions
impl RenderTree {
    pub fn mark_needs_layout(&mut self, id: ElementId);
    pub fn mark_needs_paint(&mut self, id: ElementId);
    pub fn render_parent(&self, id: ElementId) -> Option<ElementId>;
    pub fn render_children(&self, id: ElementId) -> RenderChildrenIter<'_>;
}
```

### LayerTree (future, in flui-layer)

```rust
// Compositing layer tree
pub struct LayerTree {
    nodes: Slab<Layer>,
    root: Option<LayerId>,
}

impl TreeRead for LayerTree { ... }
impl TreeNav for LayerTree { ... }
impl TreeWrite for LayerTree { ... }

// Layer-specific extensions
impl LayerTree {
    pub fn add_picture_layer(&mut self, ...);
    pub fn add_transform_layer(&mut self, ...);
    pub fn composite(&self) -> CompositorFrame;
}
```

### SemanticsTree (future, in flui-semantics)

```rust
// Accessibility tree
pub struct SemanticsTree {
    nodes: Slab<SemanticsNode>,
    root: Option<SemanticsId>,
}

impl TreeRead for SemanticsTree { ... }
impl TreeNav for SemanticsTree { ... }
impl TreeWrite for SemanticsTree { ... }

// Semantics-specific extensions
impl SemanticsTree {
    pub fn update_semantics(&mut self, id: SemanticsId, data: SemanticsData);
    pub fn get_accessible_name(&self, id: SemanticsId) -> Option<&str>;
}
```

## What Moves Where

| Current Location | Target | Reason |
|-----------------|--------|--------|
| `traits/render.rs` | `flui_rendering/src/tree/` | RenderTree-specific |
| `traits/dirty.rs` | `flui_rendering/src/tree/` | RenderTree dirty tracking |
| `iter/render.rs` | `flui_rendering/src/tree/` | RenderTree iterators |
| `iter/render_collector.rs` | `flui_rendering/src/tree/` | RenderTree collector |
| `traits/view.rs` | `flui-view/src/tree/` | ViewTree snapshots |
| `traits/lifecycle.rs` | `flui_core/src/element/` | ElementTree lifecycle |
| `traits/reconciliation.rs` | `flui_core/src/element/` | ElementTree reconciliation |
| `traits/inherited.rs` | `flui_core/src/element/` | ElementTree inherited data |
| `traits/diff.rs` | `flui_core/src/element/` | ElementTree diffing |
| `traits/pipeline.rs` | `flui-pipeline/` | Pipeline coordination |
| `traits/context.rs` | `flui_core/` | BuildContext |
| `traits/validation.rs` | `flui_devtools/` | Debug tools |
| `traits/combined.rs` | DELETE | Not needed |
| `visitor/statistics.rs` | `flui_devtools/` | Debug tools |

## Dependencies After Refactor

```
                    flui-foundation (ElementId, Key, etc.)
                           │
                           ▼
                      flui-tree (TRAITS ONLY)
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
        ▼                  ▼                  ▼
    flui-view         flui_core          flui_rendering
   (ViewTree)      (ElementTree)        (RenderTree)
        │                  │                  │
        └──────────────────┼──────────────────┘
                           │
                           ▼
                    flui-pipeline
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
   flui-layer       flui-semantics       flui_app
  (LayerTree)      (SemanticsTree)
    [future]          [future]
```

## Benefits

1. **Clear separation** - each tree type lives in its domain crate
2. **Shared abstractions** - all trees use same navigation/iteration patterns
3. **Easy to extend** - add LayerTree/SemanticsTree just by implementing traits
4. **No circular deps** - flui-tree has no domain knowledge
5. **Testable** - each tree can be tested in isolation
