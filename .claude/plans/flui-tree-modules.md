# Plan: Enhanced flui-tree Modules

## Overview

Add 4 new modules to `flui-tree` crate for type-safe tree operations:
- `depth.rs` - Type-safe depth tracking
- `slot.rs` - Enhanced slot with tree context  
- `path.rs` - Tree path representations
- `cursor.rs` - Stateful tree navigation

## Module 1: `depth.rs`

### Purpose
Type-safe wrapper for tree depth with atomic variant for thread-safety.

### Public API
```rust
// Core types
pub struct Depth(usize);           // Newtype wrapper
pub struct AtomicDepth(AtomicUsize); // Thread-safe variant
pub enum DepthError { MaxDepthExceeded, NoParentForRoot, DepthMismatch }

// Constants
pub const MAX_TREE_DEPTH: usize = 256;
pub const ROOT_DEPTH: usize = 0;

// Depth methods
impl Depth {
    pub const fn new(value: usize) -> Self;
    pub const fn new_checked(value: usize) -> Option<Self>;
    pub const fn root() -> Self;
    pub const fn get(self) -> usize;
    pub const fn is_root(self) -> bool;
    pub const fn child_depth(self) -> Self;        // self + 1
    pub const fn parent_depth(self) -> Option<Self>; // self - 1
    pub const fn distance_to(self, other: &Self) -> usize;
    pub const fn is_deeper_than(self, other: Self) -> bool;
    pub const fn saturating_child_depth(self) -> Self;
}

// AtomicDepth methods  
impl AtomicDepth {
    pub const fn new(value: usize) -> Self;
    pub const fn root() -> Self;
    pub fn get(&self) -> Depth;
    pub fn set(&self, depth: Depth);
    pub fn increment(&self) -> Depth;
    pub fn decrement(&self) -> Option<Depth>;
    pub fn update_from_parent(&self, parent_depth: Depth);
}

// Trait for depth-aware types
pub trait DepthAware {
    fn depth(&self) -> Depth;
    fn set_depth(&mut self, depth: Depth);
}
```

---

## Module 2: `slot.rs`

### Purpose
Enhanced slot information with tree context (parent, siblings, depth).

### Design Decision
Keep base `Slot` in flui-foundation, create `SlotInfo<I>` in flui-tree with tree-aware features.

### Public API
```rust
// Re-export from flui-foundation
pub use flui_foundation::Slot;

// Enhanced slot with tree context
pub struct SlotInfo<I: Identifier> {
    parent: I,
    slot: Slot,
    depth: Depth,
    previous_sibling: Option<I>,
    next_sibling: Option<I>,
}

impl<I: Identifier> SlotInfo<I> {
    pub fn new(parent: I, slot: Slot, depth: Depth) -> Self;
    pub fn with_siblings(parent, slot, depth, prev, next) -> Self;
    pub fn parent(&self) -> I;
    pub fn slot(&self) -> Slot;
    pub fn index(&self) -> usize;
    pub fn depth(&self) -> Depth;
    pub fn is_first_child(&self) -> bool;
    pub fn is_last_child(&self) -> bool;
    pub fn is_only_child(&self) -> bool;
    pub fn previous_sibling(&self) -> Option<I>;
    pub fn next_sibling(&self) -> Option<I>;
    pub fn next_slot_info(&self, self_id: I) -> Self;
}

// Flutter-compatible indexed slot for reconciliation
pub struct IndexedSlot<I: Identifier> {
    index: usize,
    previous: Option<I>,
}

impl<I: Identifier> IndexedSlot<I> {
    pub const fn new(index: usize, previous: Option<I>) -> Self;
    pub const fn first() -> Self;
    pub fn next(self, current_id: I) -> Self;
    pub fn to_slot(&self) -> Slot;
}

// Builder pattern
pub struct SlotInfoBuilder<I: Identifier>;
```

---

## Module 3: `path.rs`

### Purpose
Tree path representations for navigation, serialization, and debugging.

### Design Decision
Provide both ID-based (`TreePath<I>`) and index-based (`IndexPath`) paths.

### Public API
```rust
// ID-based path (stable, for runtime use)
pub struct TreePath<I: Identifier> {
    segments: SmallVec<[I; 8]>,  // root -> target order
}

impl<I: Identifier> TreePath<I> {
    // Constructors
    pub const fn empty() -> Self;
    pub fn root(root_id: I) -> Self;
    pub fn from_node<T: TreeNav<I>>(tree: &T, target: I) -> Self;
    pub fn from_slice(ids: &[I]) -> Self;
    
    // Accessors
    pub fn root(&self) -> Option<I>;
    pub fn target(&self) -> Option<I>;
    pub fn depth(&self) -> usize;
    pub fn at(&self, depth: usize) -> Option<I>;
    pub fn as_slice(&self) -> &[I];
    
    // Navigation
    pub fn parent(&self) -> Option<Self>;
    pub fn child(&self, child: I) -> Self;
    pub fn truncate(&self, depth: usize) -> Self;
    
    // Comparison
    pub fn is_ancestor_of(&self, other: &Self) -> bool;
    pub fn is_descendant_of(&self, other: &Self) -> bool;
    pub fn common_prefix(&self, other: &Self) -> Self;
    pub fn relative_to(&self, ancestor: &Self) -> Option<Self>;
    
    // Resolution
    pub fn validate<T: TreeNav<I>>(&self, tree: &T) -> bool;
    pub fn resolve<T: TreeNav<I>>(&self, tree: &T) -> Option<I>;
}

// Index-based path (portable, for serialization)
pub struct IndexPath {
    indices: SmallVec<[u32; 16]>,  // child indices from root
}

impl IndexPath {
    pub const fn root() -> Self;
    pub fn new(indices: &[u32]) -> Self;
    pub fn from_tree_path<I, T>(tree: &T, path: &TreePath<I>) -> Self;
    pub fn from_node<I, T>(tree: &T, node: I) -> Self;
    
    pub fn depth(&self) -> usize;
    pub fn is_root(&self) -> bool;
    pub fn parent(&self) -> Option<Self>;
    pub fn child(&self, index: u32) -> Self;
    pub fn sibling(&self, index: u32) -> Option<Self>;
    
    pub fn is_ancestor_of(&self, other: &Self) -> bool;
    pub fn common_prefix(&self, other: &Self) -> Self;
    
    pub fn resolve<I, T>(&self, tree: &T, root: I) -> Option<I>;
    pub fn to_tree_path<I, T>(&self, tree: &T, root: I) -> Option<TreePath<I>>;
}

// Extension trait
pub trait TreeNavPathExt<I: Identifier>: TreeNav<I> {
    fn path_to(&self, target: I) -> TreePath<I>;
    fn index_path_to(&self, target: I) -> IndexPath;
    fn child_index(&self, child: I) -> Option<u32>;
}
```

---

## Module 4: `cursor.rs`

### Purpose
Stateful cursor for interactive tree navigation with optional history.

### Public API
```rust
pub struct TreeCursor<'a, T, I>
where
    T: TreeNav<I>,
    I: Identifier,
{
    tree: &'a T,
    current: I,
    depth: usize,
    history: Option<CursorHistory<I>>,
}

impl<'a, T, I> TreeCursor<'a, T, I> {
    // Constructors
    pub fn new(tree: &'a T, position: I) -> Self;
    pub fn with_history(tree: &'a T, position: I, max_history: usize) -> Self;
    
    // State
    pub fn current(&self) -> I;
    pub fn depth(&self) -> usize;
    pub fn path(&self) -> TreePath<I>;
    pub fn index_path(&self) -> IndexPath;
    pub fn tree(&self) -> &'a T;
    
    // Position queries
    pub fn is_at_root(&self) -> bool;
    pub fn is_at_leaf(&self) -> bool;
    pub fn has_next_sibling(&self) -> bool;
    pub fn has_prev_sibling(&self) -> bool;
    pub fn child_count(&self) -> usize;
    
    // Navigation (returns bool for success)
    pub fn go_parent(&mut self) -> bool;
    pub fn go_child(&mut self, index: usize) -> bool;
    pub fn go_first_child(&mut self) -> bool;
    pub fn go_last_child(&mut self) -> bool;
    pub fn go_next_sibling(&mut self) -> bool;
    pub fn go_prev_sibling(&mut self) -> bool;
    pub fn go_to(&mut self, target: I) -> bool;
    pub fn go_to_path(&mut self, path: &TreePath<I>) -> bool;
    pub fn go_root(&mut self);
    
    // History
    pub fn go_back(&mut self) -> bool;
    pub fn push_position(&mut self);
    pub fn pop_position(&mut self) -> Option<I>;
    pub fn clear_history(&mut self);
    
    // DFS traversal
    pub fn go_next_dfs(&mut self) -> bool;
    pub fn go_prev_dfs(&mut self) -> bool;
    
    // Search
    pub fn find_descendant<P>(&mut self, predicate: P) -> bool;
    pub fn find_ancestor<P>(&mut self, predicate: P) -> bool;
}

// Internal history stack
struct CursorHistory<I: Identifier> {
    positions: SmallVec<[I; 16]>,
    max_size: usize,
}
```

---

## Files to Create/Modify

### New Files
1. `crates/flui-tree/src/depth.rs`
2. `crates/flui-tree/src/slot.rs`
3. `crates/flui-tree/src/path.rs`
4. `crates/flui-tree/src/cursor.rs`

### Files to Modify
1. `crates/flui-tree/src/lib.rs` - Add modules and re-exports

---

## Re-exports in lib.rs

```rust
// Modules
pub mod depth;
pub mod slot;
pub mod path;
pub mod cursor;

// Re-exports
pub use depth::{AtomicDepth, Depth, DepthAware, DepthError, MAX_TREE_DEPTH, ROOT_DEPTH};
pub use slot::{IndexedSlot, Slot, SlotInfo, SlotInfoBuilder};
pub use path::{IndexPath, TreeNavPathExt, TreePath};
pub use cursor::TreeCursor;

// Prelude additions
pub mod prelude {
    // ... existing ...
    pub use crate::depth::{AtomicDepth, Depth, DepthAware};
    pub use crate::slot::{IndexedSlot, Slot, SlotInfo};
    pub use crate::path::{IndexPath, TreeNavPathExt, TreePath};
    pub use crate::cursor::TreeCursor;
}
```

---

## Implementation Order

1. **depth.rs** - No dependencies on other new modules
2. **slot.rs** - Depends on depth.rs (uses Depth type)
3. **path.rs** - Depends on TreeNav trait only
4. **cursor.rs** - Depends on path.rs (for path() method)

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Depth storage | Single usize | Depth is scalar, not collection |
| Path storage | SmallVec<[I; 8]> | UI trees rarely > 8 deep |
| IndexPath storage | SmallVec<[u32; 16]> | Compact indices, deeper inline |
| Slot location | Keep in flui-foundation | Simple type, widely used |
| SlotInfo location | New in flui-tree | Tree-aware features |
| Cursor history | Optional, bounded | Memory safety |
| Atomic ordering | Acquire/Release | Safe for single-writer scenarios |
