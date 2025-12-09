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
//! Typestate is a fundamental pattern like [`Arity`](crate::arity::Arity) - it describes structure,
//! not domain-specific behavior.
//!
//! # Design Principles
//!
//! Similar to the arity system, typestate follows these principles:
//! - **Zero-cost**: PhantomData has no runtime overhead
//! - **Sealed**: Only Unmounted and Mounted can implement NodeState
//! - **Universal**: Works for View, Element, and Render trees
//! - **Type-safe**: Invalid state transitions caught at compile time
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{Unmounted, Mounted, NodeState};
//!
//! // Node with unmounted state
//! struct ViewHandle<S: NodeState> {
//!     config: ViewConfig,
//!     live_object: Option<ViewObject>,
//!     tree_info: Option<TreeInfo>,
//!     _state: PhantomData<S>,
//! }
//!
//! impl ViewHandle<Unmounted> {
//!     fn config(&self) -> &ViewConfig {
//!         &self.config  // ✅ OK - config always present
//!     }
//!
//!     fn mount(self, parent: Option<usize>) -> ViewHandle<Mounted> {
//!         // Transition to mounted state
//!     }
//! }
//!
//! impl ViewHandle<Mounted> {
//!     fn live_object(&self) -> &ViewObject {
//!         self.live_object.as_ref().unwrap()  // ✅ Safe - always Some for Mounted
//!     }
//!
//!     fn tree_info(&self) -> &TreeInfo {
//!         self.tree_info.as_ref().unwrap()  // ✅ Safe - always Some for Mounted
//!     }
//! }
//! ```

use std::marker::PhantomData;

// ============================================================================
// STATE MARKERS
// ============================================================================

/// Sealed trait pattern - prevents external implementations.
mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Unmounted {}
    impl Sealed for super::Mounted {}
    impl Sealed for super::Dirty {}
    impl Sealed for super::Reassembling {}
}

/// Marker trait for node lifecycle states.
///
/// Similar to how [`Arity`](crate::arity::Arity) marks compile-time child count constraints,
/// `NodeState` marks compile-time lifecycle constraints.
///
/// # States
///
/// - [`Unmounted`] - Node has configuration but is not in tree
/// - [`Mounted`] - Node is in tree, clean (no rebuild needed)
/// - [`Dirty`] - Node is in tree but needs rebuild
/// - [`Reassembling`] - Node is being hot-reloaded
///
/// # State Transitions
///
/// ```text
/// Unmounted ─mount()→ Mounted
///     ↑                  ↓
///     │            mark_dirty()
///     │                  ↓
///     │               Dirty ←─┐
///     │                  ↓    │
///     │              rebuild() │
///     │                  ↓    │
///     │               Mounted │
///     │                  ↓    │
///     │           reassemble() │
///     │                  ↓    │
///     └─unmount()─ Reassembling ─┘
/// ```
///
/// # Design
///
/// Like Arity, NodeState is:
/// - **Zero-cost**: PhantomData has no runtime overhead
/// - **Sealed**: Only predefined states can implement it
/// - **Universal**: Works for View, Element, and Render trees
/// - **Copy**: Enables efficient passing and cloning
pub trait NodeState: sealed::Sealed + Send + Sync + Copy + 'static {
    /// Whether this state represents a mounted node.
    ///
    /// Mounted means the node is in the tree (Mounted, Dirty, Reassembling).
    /// This is a const, enabling compile-time checks and optimizations.
    const IS_MOUNTED: bool;

    /// Whether this state needs rebuild.
    ///
    /// True for Dirty and Reassembling states.
    const NEEDS_REBUILD: bool;

    /// Whether this state is being reassembled (hot reload).
    const IS_REASSEMBLING: bool;

    /// Get a human-readable name for this state.
    ///
    /// Used for debugging and error messages.
    fn state_name() -> &'static str;
}

/// Unmounted state - node has configuration but is not in tree.
///
/// Similar to how [`Leaf`](crate::arity::Leaf) indicates "0 children", `Unmounted` indicates
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
/// # Compile-Time Guarantees
///
/// Methods that require tree position will not compile for `Unmounted` nodes:
///
/// ```rust,ignore
/// let unmounted = ViewHandle::<Unmounted>::new(config);
/// // unmounted.parent();  // ❌ Compile error - no parent() method for Unmounted!
/// ```
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
///
/// // Must mount first
/// let mounted = view.mount(Some(parent_id));
/// let parent = mounted.parent();  // ✅ OK now!
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Unmounted;

impl NodeState for Unmounted {
    const IS_MOUNTED: bool = false;
    const NEEDS_REBUILD: bool = false;
    const IS_REASSEMBLING: bool = false;

    #[inline]
    fn state_name() -> &'static str {
        "Unmounted"
    }
}

/// Mounted state - node is in tree with parent/children.
///
/// Similar to how [`Single`](crate::arity::Single) indicates "1 child", `Mounted` indicates
/// "in tree with position". The node has:
/// - Live object (ViewObject, Element, RenderObject)
/// - Parent reference (if not root)
/// - Child references
/// - Tree position information
///
/// # Compile-Time Guarantees
///
/// Methods that require tree position will only compile for `Mounted` nodes:
///
/// ```rust,ignore
/// impl ViewHandle<Mounted> {
///     pub fn parent(&self) -> Option<usize> {
///         self.tree_info().parent  // ✅ Safe - tree_info always present
///     }
/// }
/// ```
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
/// let depth = mounted.depth();
///
/// // Can access live object
/// let view_obj = mounted.view_object();
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mounted;

impl NodeState for Mounted {
    const IS_MOUNTED: bool = true;
    const NEEDS_REBUILD: bool = false;
    const IS_REASSEMBLING: bool = false;

    #[inline]
    fn state_name() -> &'static str {
        "Mounted"
    }
}

/// Dirty state - node is in tree but needs rebuild.
///
/// This state indicates that the node's configuration has changed
/// and needs to be rebuilt. Similar to Flutter's `markNeedsBuild()`.
///
/// # When to Use Dirty
///
/// - State changed (e.g., counter incremented)
/// - Props changed from parent
/// - Dependencies changed (context, inherited widget)
/// - Manual `mark_dirty()` call
///
/// # Compile-Time Guarantees
///
/// Methods that require a clean state will not compile for `Dirty` nodes:
///
/// ```rust,ignore
/// impl ViewHandle<Mounted> {
///     pub fn paint(&self) { /* ... */ }  // ✅ OK for Mounted
/// }
///
/// // let dirty: ViewHandle<Dirty> = ...;
/// // dirty.paint();  // ❌ Compile error - paint() not available for Dirty!
/// ```
///
/// # Example
///
/// ```rust,ignore
/// // State changes - mark as dirty
/// let mut mounted = view_handle;
/// let dirty = mounted.mark_dirty();  // Mounted → Dirty
///
/// // Framework rebuilds
/// let rebuilt = dirty.rebuild(ctx);  // Dirty → Mounted
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dirty;

impl NodeState for Dirty {
    const IS_MOUNTED: bool = true;  // Still in tree
    const NEEDS_REBUILD: bool = true;  // Needs rebuild
    const IS_REASSEMBLING: bool = false;

    #[inline]
    fn state_name() -> &'static str {
        "Dirty"
    }
}

/// Reassembling state - node is being hot-reloaded.
///
/// This state is entered during hot reload when the code changes
/// and views need to be recreated. Similar to Flutter's `reassemble()`.
///
/// # Hot Reload Process
///
/// ```text
/// 1. Code change detected
/// 2. Mounted → reassemble() → Reassembling
/// 3. Recreate ViewObject from updated config
/// 4. Reassembling → finish_reassemble() → Mounted
/// 5. Recursively reassemble children
/// ```
///
/// # Compile-Time Guarantees
///
/// During reassembly, the node is in a transitional state.
/// Only reassembly-related operations are available:
///
/// ```rust,ignore
/// impl ViewHandle<Reassembling> {
///     pub fn finish_reassemble(self) -> ViewHandle<Mounted> {
///         // Complete the reassembly process
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// // Hot reload triggered
/// let reassembling = mounted.reassemble();  // Mounted → Reassembling
///
/// // Framework recreates ViewObject from config
/// // ...
///
/// // Finish reassembly
/// let mounted = reassembling.finish_reassemble();  // Reassembling → Mounted
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reassembling;

impl NodeState for Reassembling {
    const IS_MOUNTED: bool = true;  // Still in tree
    const NEEDS_REBUILD: bool = true;  // Will rebuild after reassembly
    const IS_REASSEMBLING: bool = true;  // In reassembly process

    #[inline]
    fn state_name() -> &'static str {
        "Reassembling"
    }
}

// ============================================================================
// TREE INFO (Present only when Mounted, Dirty, Reassembling)
// ============================================================================

/// Tree position information for mounted nodes.
///
/// This struct is present when `NodeState::IS_MOUNTED = true`, which includes:
/// - [`Mounted`] - Clean state
/// - [`Dirty`] - Needs rebuild
/// - [`Reassembling`] - Hot reload in progress
///
/// Similar to how [`FixedChildren<N>`](crate::arity::FixedChildren) guarantees N children,
/// `TreeInfo` guarantees tree position data exists for all mounted states.
///
/// # Fields
///
/// - `parent`: Parent node ID (None for root)
/// - `children`: List of child node IDs
/// - `depth`: Distance from root (0 = root)
///
/// # Usage
///
/// ```rust,ignore
/// impl ViewHandle<Mounted> {
///     pub fn tree_info(&self) -> &TreeInfo {
///         // Safe unwrap - TreeInfo is always Some for Mounted nodes
///         self.tree_info.as_ref().unwrap()
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
    ///
    /// Root nodes have:
    /// - No parent (`parent = None`)
    /// - Depth 0
    /// - Empty children list
    #[inline]
    #[must_use]
    pub const fn root() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Create new tree info with parent.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent node ID
    /// - `depth`: Distance from root
    #[inline]
    #[must_use]
    pub fn with_parent(parent: usize, depth: usize) -> Self {
        Self {
            parent: Some(parent),
            children: Vec::new(),
            depth,
        }
    }

    /// Check if this is the root node.
    #[inline]
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    /// Get number of children.
    #[inline]
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Add a child node ID.
    #[inline]
    pub fn add_child(&mut self, child: usize) {
        self.children.push(child);
    }

    /// Remove a child node ID.
    ///
    /// Returns `true` if the child was found and removed.
    #[inline]
    pub fn remove_child(&mut self, child: usize) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child) {
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get the parent ID, panicking if this is a root node.
    ///
    /// # Panics
    ///
    /// Panics if this node is the root (has no parent).
    #[inline]
    #[must_use]
    pub fn parent_unchecked(&self) -> usize {
        self.parent.expect("Cannot get parent of root node")
    }
}

// ============================================================================
// MOUNTABLE TRAIT (Like Arity trait)
// ============================================================================

/// Trait for nodes that can transition from Unmounted to Mounted state.
///
/// This is analogous to the [`Arity`](crate::arity::Arity) trait - it defines behavior that works
/// across all node types (View, Element, Render).
///
/// # Type Safety
///
/// The trait enforces that mounting consumes the unmounted node and
/// returns a new mounted node, preventing accidental reuse.
///
/// # Example
///
/// ```rust,ignore
/// impl Mountable for ViewHandle<Unmounted> {
///     type Mounted = ViewHandle<Mounted>;
///
///     fn mount(self, parent: Option<usize>) -> Self::Mounted {
///         // Create live ViewObject from config
///         let view_object = self.config.create_view_object();
///
///         // Create tree info
///         let tree_info = if let Some(parent_id) = parent {
///             TreeInfo::with_parent(parent_id, 0)
///         } else {
///             TreeInfo::root()
///         };
///
///         ViewHandle {
///             config: self.config,
///             view_object: Some(view_object),
///             tree_info: Some(tree_info),
///             _state: PhantomData,
///         }
///     }
/// }
/// ```
pub trait Mountable: Sized {
    /// The mounted version of this node type.
    type Mounted;

    /// Mount the node, transitioning from Unmounted to Mounted.
    ///
    /// This consumes the unmounted node and returns a new mounted node
    /// with live objects and tree position information.
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
/// This allows converting back to config-only state for:
/// - Hot-reload (recreate views from config)
/// - Serialization (save tree state)
/// - Tree reconstruction (rebuild after changes)
///
/// # Type Safety
///
/// Like [`Mountable`], this trait enforces that unmounting consumes
/// the mounted node and returns a new unmounted node.
///
/// # Example
///
/// ```rust,ignore
/// impl Unmountable for ViewHandle<Mounted> {
///     type Unmounted = ViewHandle<Unmounted>;
///
///     fn unmount(self) -> Self::Unmounted {
///         ViewHandle {
///             config: self.config,  // Preserve config
///             view_object: None,    // Discard live object
///             tree_info: None,      // Discard tree info
///             _state: PhantomData,
///         }
///     }
/// }
/// ```
pub trait Unmountable: Sized {
    /// The unmounted version of this node type.
    type Unmounted;

    /// Unmount the node, transitioning from Mounted to Unmounted.
    ///
    /// This preserves the configuration but discards:
    /// - Live objects (ViewObject, RenderObject, etc.)
    /// - Tree position information
    /// - Parent/child references
    ///
    /// # Returns
    ///
    /// An unmounted version with only the configuration.
    fn unmount(self) -> Self::Unmounted;
}

// ============================================================================
// MARKER TYPE FOR COMPILE-TIME CHECKS
// ============================================================================

/// Marker struct for nodes with state S.
///
/// This is similar to `PhantomData` but provides additional utility methods.
/// Use this instead of raw `PhantomData<S>` for consistency.
///
/// # Example
///
/// ```rust,ignore
/// pub struct ViewHandle<S: NodeState> {
///     config: ViewConfig,
///     // ... other fields ...
///     _state: StateMarker<S>,
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct StateMarker<S: NodeState>(PhantomData<S>);

impl<S: NodeState> StateMarker<S> {
    /// Create a new state marker.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self(PhantomData)
    }

    /// Check if this state is mounted (at compile time).
    ///
    /// Returns true for Mounted, Dirty, and Reassembling.
    /// This is a const function, enabling compile-time checks.
    #[inline]
    #[must_use]
    pub const fn is_mounted() -> bool {
        S::IS_MOUNTED
    }

    /// Check if this state needs rebuild (at compile time).
    ///
    /// Returns true for Dirty and Reassembling.
    #[inline]
    #[must_use]
    pub const fn needs_rebuild() -> bool {
        S::NEEDS_REBUILD
    }

    /// Check if this state is reassembling (at compile time).
    ///
    /// Returns true only for Reassembling.
    #[inline]
    #[must_use]
    pub const fn is_reassembling() -> bool {
        S::IS_REASSEMBLING
    }

    /// Get the state name.
    ///
    /// Useful for debugging and error messages.
    #[inline]
    #[must_use]
    pub fn state_name() -> &'static str {
        S::state_name()
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
        // Test IS_MOUNTED
        assert!(!Unmounted::IS_MOUNTED);
        assert!(Mounted::IS_MOUNTED);
        assert!(Dirty::IS_MOUNTED);
        assert!(Reassembling::IS_MOUNTED);

        // Test NEEDS_REBUILD
        assert!(!Unmounted::NEEDS_REBUILD);
        assert!(!Mounted::NEEDS_REBUILD);
        assert!(Dirty::NEEDS_REBUILD);
        assert!(Reassembling::NEEDS_REBUILD);

        // Test IS_REASSEMBLING
        assert!(!Unmounted::IS_REASSEMBLING);
        assert!(!Mounted::IS_REASSEMBLING);
        assert!(!Dirty::IS_REASSEMBLING);
        assert!(Reassembling::IS_REASSEMBLING);

        // Test state names
        assert_eq!(Unmounted::state_name(), "Unmounted");
        assert_eq!(Mounted::state_name(), "Mounted");
        assert_eq!(Dirty::state_name(), "Dirty");
        assert_eq!(Reassembling::state_name(), "Reassembling");
    }

    #[test]
    fn test_tree_info_root() {
        let root = TreeInfo::root();
        assert!(root.is_root());
        assert_eq!(root.depth, 0);
        assert_eq!(root.child_count(), 0);
        assert_eq!(root.parent, None);
    }

    #[test]
    fn test_tree_info_with_parent() {
        let child = TreeInfo::with_parent(1, 1);
        assert!(!child.is_root());
        assert_eq!(child.parent, Some(1));
        assert_eq!(child.depth, 1);
        assert_eq!(child.child_count(), 0);
    }

    #[test]
    fn test_tree_info_children() {
        let mut info = TreeInfo::root();
        assert_eq!(info.child_count(), 0);

        info.add_child(10);
        info.add_child(20);
        info.add_child(30);
        assert_eq!(info.child_count(), 3);
        assert_eq!(info.children, vec![10, 20, 30]);

        assert!(info.remove_child(20));
        assert_eq!(info.child_count(), 2);
        assert_eq!(info.children, vec![10, 30]);

        assert!(!info.remove_child(999));
        assert_eq!(info.child_count(), 2);
    }

    #[test]
    #[should_panic(expected = "Cannot get parent of root node")]
    fn test_tree_info_parent_unchecked_panic() {
        let root = TreeInfo::root();
        let _ = root.parent_unchecked();
    }

    #[test]
    fn test_tree_info_parent_unchecked_success() {
        let child = TreeInfo::with_parent(42, 1);
        assert_eq!(child.parent_unchecked(), 42);
    }

    #[test]
    fn test_state_marker() {
        // Unmounted
        let marker_unmounted = StateMarker::<Unmounted>::new();
        assert!(!StateMarker::<Unmounted>::is_mounted());
        assert!(!StateMarker::<Unmounted>::needs_rebuild());
        assert!(!StateMarker::<Unmounted>::is_reassembling());
        assert_eq!(StateMarker::<Unmounted>::state_name(), "Unmounted");

        // Mounted
        let marker_mounted = StateMarker::<Mounted>::new();
        assert!(StateMarker::<Mounted>::is_mounted());
        assert!(!StateMarker::<Mounted>::needs_rebuild());
        assert!(!StateMarker::<Mounted>::is_reassembling());
        assert_eq!(StateMarker::<Mounted>::state_name(), "Mounted");

        // Dirty
        let marker_dirty = StateMarker::<Dirty>::new();
        assert!(StateMarker::<Dirty>::is_mounted());
        assert!(StateMarker::<Dirty>::needs_rebuild());
        assert!(!StateMarker::<Dirty>::is_reassembling());
        assert_eq!(StateMarker::<Dirty>::state_name(), "Dirty");

        // Reassembling
        let marker_reassembling = StateMarker::<Reassembling>::new();
        assert!(StateMarker::<Reassembling>::is_mounted());
        assert!(StateMarker::<Reassembling>::needs_rebuild());
        assert!(StateMarker::<Reassembling>::is_reassembling());
        assert_eq!(StateMarker::<Reassembling>::state_name(), "Reassembling");

        // StateMarker should be zero-sized
        assert_eq!(std::mem::size_of_val(&marker_unmounted), 0);
        assert_eq!(std::mem::size_of_val(&marker_mounted), 0);
        assert_eq!(std::mem::size_of_val(&marker_dirty), 0);
        assert_eq!(std::mem::size_of_val(&marker_reassembling), 0);
    }

    #[test]
    fn test_state_marker_copy() {
        let marker1 = StateMarker::<Unmounted>::new();
        let marker2 = marker1; // Should be Copy
        assert_eq!(marker1, marker2);
    }

    #[test]
    fn test_tree_info_clone() {
        let mut info1 = TreeInfo::with_parent(5, 2);
        info1.add_child(10);
        info1.add_child(20);

        let info2 = info1.clone();
        assert_eq!(info1, info2);
        assert_eq!(info2.parent, Some(5));
        assert_eq!(info2.depth, 2);
        assert_eq!(info2.children, vec![10, 20]);
    }
}
