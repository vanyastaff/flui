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
}

/// Marker trait for node lifecycle states.
///
/// Similar to how [`Arity`](crate::arity::Arity) marks compile-time child count constraints,
/// `NodeState` marks compile-time lifecycle constraints.
///
/// # States
///
/// - [`Unmounted`] - Node has configuration but is not in tree
/// - [`Mounted`] - Node is in tree with parent/children
///
/// # Design
///
/// Like Arity, NodeState is:
/// - **Zero-cost**: PhantomData has no runtime overhead
/// - **Sealed**: Only Unmounted and Mounted can implement it
/// - **Universal**: Works for View, Element, and Render trees
/// - **Copy**: Enables efficient passing and cloning
pub trait NodeState: sealed::Sealed + Send + Sync + Copy + 'static {
    /// Whether this state represents a mounted node.
    ///
    /// This is a const, enabling compile-time checks and optimizations.
    const IS_MOUNTED: bool;

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

    #[inline]
    fn state_name() -> &'static str {
        "Mounted"
    }
}

// ============================================================================
// TREE INFO (Present only when Mounted)
// ============================================================================

/// Tree position information for mounted nodes.
///
/// This struct is present ONLY when `NodeState = Mounted`.
/// Similar to how [`FixedChildren<N>`](crate::arity::FixedChildren) guarantees N children,
/// `TreeInfo` guarantees tree position data exists.
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
    /// This is a const function, enabling compile-time checks.
    #[inline]
    #[must_use]
    pub const fn is_mounted() -> bool {
        S::IS_MOUNTED
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
        // Test const values
        assert!(!Unmounted::IS_MOUNTED);
        assert!(Mounted::IS_MOUNTED);

        // Test state names
        assert_eq!(Unmounted::state_name(), "Unmounted");
        assert_eq!(Mounted::state_name(), "Mounted");
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
        let marker_unmounted = StateMarker::<Unmounted>::new();
        assert!(!StateMarker::<Unmounted>::is_mounted());
        assert_eq!(StateMarker::<Unmounted>::state_name(), "Unmounted");

        let marker_mounted = StateMarker::<Mounted>::new();
        assert!(StateMarker::<Mounted>::is_mounted());
        assert_eq!(StateMarker::<Mounted>::state_name(), "Mounted");

        // StateMarker should be zero-sized
        assert_eq!(std::mem::size_of_val(&marker_unmounted), 0);
        assert_eq!(std::mem::size_of_val(&marker_mounted), 0);
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
