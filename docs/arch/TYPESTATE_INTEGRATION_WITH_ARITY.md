# Typestate Integration with Existing Arity System

## Key Discovery: flui-tree Already Has Advanced Type System Features

After reviewing `flui-tree/arity`, we found it already uses cutting-edge Rust features:

- ✅ **Const Generics** - `Exact<N>`, `Range<MIN, MAX>`
- ✅ **GAT (Generic Associated Types)** - flexible iterators
- ✅ **HRTB (Higher-Rank Trait Bounds)** - universal predicates
- ✅ **Never type** - impossible operations (`!`)
- ✅ **Typestate patterns** - already mentioned in comments (line 11)!

**Conclusion:** flui-tree is the PERFECT place to add typestate markers!

---

## Current Architecture

```rust
// flui-tree/src/arity/mod.rs - Already advanced!

/// Arity trait with GAT and HRTB support
pub trait Arity: sealed::Sealed + Send + Sync + Debug + Copy + Default + 'static {
    type Accessor<'a, T: 'a + Send + Sync>: ChildrenAccess<'a, T>;
    type Iterator<'a, T: 'a>: Iterator<Item = &'a T> where T: 'a, Self: 'a;

    const EXPECTED_SIZE: usize = 4;
    const INLINE_THRESHOLD: usize = 16;
    const BATCH_SIZE: usize = 32;
    const SUPPORTS_SIMD: bool = false;

    fn runtime_arity() -> RuntimeArity;
    fn validate_count(count: usize) -> bool;
    fn from_slice<T: Send + Sync>(children: &[T]) -> Self::Accessor<'_, T>;
}

// Existing arity types
pub struct Leaf;              // 0 children
pub struct Optional;          // 0-1 child
pub struct Single;            // = Exact<1>
pub struct Exact<const N: usize>;        // N children
pub struct AtLeast<const N: usize>;      // >= N children
pub struct Variable;          // Any number
pub struct Range<const MIN: usize, const MAX: usize>;  // MIN..=MAX
pub struct Never;             // Impossible (!)
```

**Key Insight:** Arity system already uses advanced type features and is ready for typestate!

---

## Proposed Typestate Integration

### Add Typestate Module to flui-tree

```rust
// ============================================================================
// flui-tree/src/state.rs - NEW MODULE
// ============================================================================

//! Node state markers for typestate pattern across all three trees.
//!
//! This module provides compile-time state tracking for:
//! - **ViewTree**: ViewHandle<S> (view config vs live ViewObject)
//! - **ElementTree**: ElementHandle<S> (unmounted vs mounted element)
//! - **RenderTree**: RenderHandle<S> (unmounted vs mounted render object)
//!
//! # Philosophy
//!
//! flui-tree provides pure abstractions that work across all trees.
//! Typestate is a fundamental pattern like Arity - it describes structure,
//! not domain-specific behavior.

use std::marker::PhantomData;

// ============================================================================
// STATE MARKERS
// ============================================================================

/// Sealed trait for node states.
///
/// Prevents external implementations while allowing users to use
/// Unmounted and Mounted types.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Unmounted {}
    impl Sealed for super::Mounted {}
}

/// Marker trait for node lifecycle states.
///
/// Similar to how Arity marks compile-time child count constraints,
/// NodeState marks compile-time lifecycle constraints.
///
/// # Design
///
/// Like Arity, NodeState is:
/// - **Zero-cost**: PhantomData has no runtime overhead
/// - **Sealed**: Only Unmounted and Mounted can implement it
/// - **Universal**: Works for View, Element, and Render trees
pub trait NodeState: sealed::Sealed + Send + Sync + Copy + 'static {
    /// Whether this state represents a mounted node.
    const IS_MOUNTED: bool;

    /// Get a human-readable name for this state.
    fn state_name() -> &'static str;
}

/// Unmounted state - node has configuration but is not in tree.
///
/// Similar to how Leaf indicates "0 children", Unmounted indicates
/// "not yet in tree". The node may have:
/// - View config (for ViewHandle)
/// - Element data (for ElementHandle)
/// - RenderObject config (for RenderHandle)
///
/// But it does NOT have:
/// - Parent reference
/// - Child references
/// - Tree position (depth, etc.)
///
/// # Example
///
/// ```rust,ignore
/// // Create unmounted view
/// let view = ViewHandle::<Unmounted>::new(Padding::all(16.0));
///
/// // Can access config
/// let config = view.config();
///
/// // Cannot access tree info (compile error!)
/// // let parent = view.parent();  // ❌ Doesn't compile!
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Unmounted;

impl NodeState for Unmounted {
    const IS_MOUNTED: bool = false;

    fn state_name() -> &'static str {
        "Unmounted"
    }
}

/// Mounted state - node is in tree with parent/children.
///
/// Similar to how Single indicates "1 child", Mounted indicates
/// "in tree with position". The node has:
/// - Live object (ViewObject, Element, RenderObject)
/// - Parent reference (if not root)
/// - Child references
/// - Tree position information
///
/// # Example
///
/// ```rust,ignore
/// // Mount the view
/// let mounted = unmounted_view.mount(Some(parent_id));
///
/// // Can access tree info
/// let parent = mounted.parent();
/// let children = mounted.children();
///
/// // Can access live object
/// let view_obj = mounted.view_object();
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mounted;

impl NodeState for Mounted {
    const IS_MOUNTED: bool = true;

    fn state_name() -> &'static str {
        "Mounted"
    }
}

// ============================================================================
// TREE INFO (Present only when Mounted)
// ============================================================================

/// Tree position information for mounted nodes.
///
/// This struct is present ONLY when NodeState = Mounted.
/// Similar to how FixedChildren<N> guarantees N children,
/// TreeInfo guarantees tree position data exists.
///
/// # Fields
///
/// - `parent`: Parent node ID (None for root)
/// - `children`: List of child node IDs
/// - `depth`: Distance from root (0 = root)
///
/// # Example
///
/// ```rust,ignore
/// impl<V> ViewHandle<Mounted> {
///     pub fn tree_info(&self) -> &TreeInfo {
///         self.tree_info.as_ref().unwrap()  // Safe - always Some for Mounted
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeInfo {
    /// Parent node ID (None if root).
    pub parent: Option<usize>,

    /// Children node IDs.
    pub children: Vec<usize>,

    /// Depth in tree (0 = root).
    pub depth: usize,
}

impl TreeInfo {
    /// Create new tree info for a root node.
    pub const fn root() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create new tree info with parent.
    pub fn with_parent(parent: usize, depth: usize) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            depth,
        }
    }

    /// Check if this is the root node.
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Get number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }
}

// ============================================================================
// MOUNTABLE TRAIT (Like Arity trait)
// ============================================================================

/// Trait for nodes that can transition between Unmounted and Mounted states.
///
/// This is analogous to the Arity trait - it defines behavior that works
/// across all node types (View, Element, Render).
///
/// # Example
///
/// ```rust,ignore
/// impl Mountable for ViewHandle<Unmounted> {
///     type Mounted = ViewHandle<Mounted>;
///
///     fn mount(self, parent: Option<usize>) -> Self::Mounted {
///         // Create live ViewObject from config
///         // Add tree position info
///         ViewHandle { ... }
///     }
/// }
/// ```
pub trait Mountable: Sized {
    /// The mounted version of this node type.
    type Mounted;

    /// Mount the node, transitioning from Unmounted to Mounted.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent node ID (None for root)
    ///
    /// # Returns
    ///
    /// A mounted version of this node with tree position info.
    fn mount(self, parent: Option<usize>) -> Self::Mounted;
}

/// Trait for nodes that can be unmounted.
///
/// This allows converting back to config-only state for hot-reload,
/// serialization, or tree reconstruction.
pub trait Unmountable: Sized {
    /// The unmounted version of this node type.
    type Unmounted;

    /// Unmount the node, transitioning from Mounted to Unmounted.
    ///
    /// This preserves the configuration but discards:
    /// - Live objects
    /// - Tree position
    /// - Parent/child references
    fn unmount(self) -> Self::Unmounted;
}

// ============================================================================
// MARKER TYPE FOR COMPILE-TIME CHECKS
// ============================================================================

/// Marker struct for nodes with state S.
///
/// This is similar to PhantomData but provides additional utility methods.
///
/// # Example
///
/// ```rust,ignore
/// pub struct ViewHandle<S: NodeState> {
///     // ... fields ...
///     _state: StateMarker<S>,
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StateMarker<S: NodeState>(PhantomData<S>);

impl<S: NodeState> StateMarker<S> {
    /// Create a new state marker.
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    /// Check if this state is mounted (at compile time).
    pub const fn is_mounted() -> bool {
        S::IS_MOUNTED
    }

    /// Get the state name.
    pub fn state_name() -> &'static str {
        S::state_name()
    }
}

// ============================================================================
// INTEGRATION WITH EXISTING ARITY SYSTEM
// ============================================================================

/// Trait for nodes that have both Arity and State.
///
/// This combines compile-time guarantees from both systems:
/// - **Arity**: How many children (Leaf, Single, Variable, etc.)
/// - **State**: Whether mounted (Unmounted, Mounted)
///
/// # Example
///
/// ```rust,ignore
/// // RenderPadding has Single arity and can be Mounted/Unmounted
/// pub struct RenderPadding<A: Arity, S: NodeState> {
///     // Config (present in both states)
///     padding: EdgeInsets,
///
///     // Live data (present only when Mounted)
///     render_object: Option<Box<dyn RenderObject>>,
///     tree_info: Option<TreeInfo>,
///
///     _arity: PhantomData<A>,
///     _state: PhantomData<S>,
/// }
///
/// impl RenderPadding<Single, Unmounted> {
///     pub fn new(padding: EdgeInsets) -> Self {
///         // Create with config only
///     }
///
///     pub fn mount(self, parent: Option<usize>) -> RenderPadding<Single, Mounted> {
///         // Create render object, add tree info
///     }
/// }
///
/// impl RenderPadding<Single, Mounted> {
///     pub fn render(&self) -> &dyn RenderObject {
///         self.render_object.as_ref().unwrap()  // Safe!
///     }
/// }
/// ```
pub trait ArityAndState {
    /// The arity type (Leaf, Single, Variable, etc.).
    type Arity: Arity;

    /// The state type (Unmounted, Mounted).
    type State: NodeState;

    /// Get the runtime arity.
    fn arity(&self) -> RuntimeArity {
        Self::Arity::runtime_arity()
    }

    /// Check if mounted.
    fn is_mounted(&self) -> bool {
        Self::State::IS_MOUNTED
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_markers() {
        assert!(!Unmounted::IS_MOUNTED);
        assert!(Mounted::IS_MOUNTED);

        assert_eq!(Unmounted::state_name(), "Unmounted");
        assert_eq!(Mounted::state_name(), "Mounted");
    }

    #[test]
    fn test_tree_info() {
        let root = TreeInfo::root();
        assert!(root.is_root());
        assert_eq!(root.depth, 0);
        assert_eq!(root.child_count(), 0);

        let child = TreeInfo::with_parent(1, 1);
        assert!(!child.is_root());
        assert_eq!(child.parent, Some(1));
        assert_eq!(child.depth, 1);
    }

    #[test]
    fn test_state_marker() {
        let marker = StateMarker::<Unmounted>::new();
        assert!(!StateMarker::<Unmounted>::is_mounted());
        assert_eq!(StateMarker::<Unmounted>::state_name(), "Unmounted");

        assert!(StateMarker::<Mounted>::is_mounted());
        assert_eq!(StateMarker::<Mounted>::state_name(), "Mounted");
    }
}
```

---

## Updated flui-tree Module Structure

```rust
// flui-tree/src/lib.rs - Add state module

pub mod arity;    // ✅ Already exists
pub mod error;    // ✅ Already exists
pub mod iter;     // ✅ Already exists
pub mod traits;   // ✅ Already exists
pub mod visitor;  // ✅ Already exists
pub mod state;    // 🆕 NEW - Typestate markers

// Re-exports
pub use state::{
    Unmounted,
    Mounted,
    NodeState,
    TreeInfo,
    Mountable,
    Unmountable,
    StateMarker,
    ArityAndState,
};
```

---

## Integration Examples

### Example 1: ViewHandle with Typestate

```rust
// flui-view/src/handle.rs

use flui_tree::{NodeState, Unmounted, Mounted, TreeInfo, Mountable, Single};

/// View handle with typestate.
///
/// Combines Arity (for children) with NodeState (for lifecycle).
pub struct ViewHandle<A: Arity, S: NodeState> {
    // Present in both states
    view_config: AnyView,

    // Present only when Mounted
    view_object: Option<Box<dyn ViewObject>>,
    tree_info: Option<TreeInfo>,

    _arity: PhantomData<A>,
    _state: PhantomData<S>,
}

impl<A: Arity> ViewHandle<A, Unmounted> {
    /// Create unmounted view handle.
    pub fn new(config: AnyView) -> Self {
        Self {
            view_config: config,
            view_object: None,
            tree_info: None,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }

    /// Access view config (only available when unmounted).
    pub fn config(&self) -> &AnyView {
        &self.view_config
    }

    /// Mount: Unmounted → Mounted
    pub fn mount(self, parent: Option<usize>) -> ViewHandle<A, Mounted> {
        let view_object = self.view_config.create_view_object();

        ViewHandle {
            view_config: self.view_config,
            view_object: Some(view_object),
            tree_info: Some(TreeInfo::with_parent(parent.unwrap_or(0), 0)),
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}

impl<A: Arity> ViewHandle<A, Mounted> {
    /// Access ViewObject (only available when mounted).
    pub fn view_object(&self) -> &dyn ViewObject {
        self.view_object.as_ref().unwrap()
    }

    /// Access tree info (only available when mounted).
    pub fn tree_info(&self) -> &TreeInfo {
        self.tree_info.as_ref().unwrap()
    }

    /// Unmount: Mounted → Unmounted
    pub fn unmount(self) -> ViewHandle<A, Unmounted> {
        ViewHandle {
            view_config: self.view_config,
            view_object: None,
            tree_info: None,
            _arity: PhantomData,
            _state: PhantomData,
        }
    }
}
```

### Example 2: Padding Widget Using Both Arity and State

```rust
// flui_widgets/src/basic/padding.rs

use flui_tree::{Single, Unmounted, Mounted, NodeState};

/// Padding widget with both Arity (Single child) and State (Unmounted/Mounted).
pub struct Padding<S: NodeState = Unmounted> {
    // Config (present in both states)
    padding: EdgeInsets,
    child_config: Option<AnyView>,

    // Live data (present only when Mounted)
    child_element: Option<ElementId>,
    tree_info: Option<TreeInfo>,

    _state: PhantomData<S>,
}

impl Padding<Unmounted> {
    pub fn all(padding: f32) -> Self {
        Self {
            padding: EdgeInsets::all(padding),
            child_config: None,
            child_element: None,
            tree_info: None,
            _state: PhantomData,
        }
    }

    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child_config = Some(AnyView::new(child));
        self
    }
}

// Arity is always Single for Padding (at compile time)
impl<S: NodeState> ArityAndState for Padding<S> {
    type Arity = Single;
    type State = S;
}
```

---

## Benefits of This Integration

### 1. Consistent with Existing flui-tree Philosophy

✅ **Pure abstractions**: Typestate markers are like Arity - they describe structure, not behavior
✅ **Zero-cost**: PhantomData has no runtime overhead
✅ **Sealed traits**: Prevents external implementations
✅ **Universal**: Works for all three trees (View, Element, Render)

### 2. Leverages Existing Advanced Features

✅ **Const generics**: NodeState has const IS_MOUNTED
✅ **Sealed traits**: Like Arity's sealed module
✅ **Copy + Send + Sync**: Same bounds as Arity
✅ **Already mentioned in comments**: Line 11 already references typestate!

### 3. Combines Two Compile-Time Guarantees

```rust
// Both Arity and State are compile-time!
pub struct RenderPadding<A: Arity, S: NodeState> {
    // A = Single: Guarantees 1 child
    // S = Mounted: Guarantees tree position exists
}

// Impossible at compile time:
// - RenderPadding<Single, Mounted> with 0 children  ❌ Arity violation
// - RenderPadding<Single, Unmounted>.tree_info()    ❌ State violation
```

### 4. Solves Child Mounting Problem

```rust
// Before: Child stores ViewObject (wrong!)
pub struct Child {
    inner: Option<Box<dyn ViewObject>>,  // ❌ Lost config
}

// After: Child stores unmounted ViewHandle
pub struct Child {
    inner: Option<ViewHandle<Single, Unmounted>>,  // ✅ Preserves config!
}

// Type erasure still needed, but with typestate:
pub trait AnyUnmountedView {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject>;
}

impl AnyUnmountedView for ViewHandle<Single, Unmounted> {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject> {
        (*self).mount(None).view_object
    }
}
```

---

## Implementation Plan

### Phase 1: Add state.rs to flui-tree (Week 1)

1. Create `flui-tree/src/state.rs`
2. Add NodeState, Unmounted, Mounted, TreeInfo
3. Add Mountable, Unmountable traits
4. Write comprehensive tests

### Phase 2: Update flui-view (Week 2)

1. Update ViewHandle to use NodeState
2. Update Child/Children to store unmounted handles
3. Implement AnyUnmountedView trait

### Phase 3: Update flui-element (Week 3)

1. Update Element to use NodeState
2. Separate mount/build phases
3. Remove pending_children (no longer needed)

### Phase 4: Update flui_rendering (Week 4)

1. Update RenderObject to use NodeState
2. Combine Arity + State for render nodes
3. Update layout/paint to work with mounted state

### Phase 5: Update flui_widgets (Week 5)

1. All Views implement Clone
2. Update Padding, Text, etc. to use typestate
3. Verify .child() API works without .leaf()

---

## Conclusion

**Typestate is the RIGHT solution** when integrated with existing flui-tree architecture:

✅ **Consistent**: Follows same patterns as Arity
✅ **Zero-cost**: PhantomData, const generics
✅ **Powerful**: Combines Arity + State compile-time guarantees
✅ **Universal**: Works across all three trees
✅ **Already planned**: Line 11 of arity/mod.rs mentions it!

This elevates typestate from a View-only detail to a **fundamental architectural pattern** alongside Arity in flui-tree.
