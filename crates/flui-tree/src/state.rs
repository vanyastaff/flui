//! Node state markers and lifecycle traits for typestate pattern.
//!
//! This module provides:
//! - **State markers**: `Unmounted`, `Mounted` for compile-time state tracking
//! - **Lifecycle traits**: `Mountable`, `Unmountable` for tree operations
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Typestate Pattern                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │   Node<Unmounted>  ───mount()───►  Node<Mounted>            │
//! │        │                                │                   │
//! │        │ Mountable                      │ Unmountable       │
//! │        │                                │                   │
//! │        ▼                                ▼                   │
//! │   - Can be mounted              - Has parent/depth          │
//! │   - No position info            - Can be unmounted          │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```
//! use flui_tree::{Mounted, Unmounted, Mountable, Unmountable, NodeState, Depth, ElementId};
//! use flui_tree::Identifier;
//! use std::marker::PhantomData;
//!
//! struct MyNode<S: NodeState> {
//!     depth: Depth,
//!     parent: Option<ElementId>,
//!     _state: PhantomData<S>,
//! }
//!
//! impl Mountable for MyNode<Unmounted> {
//!     type Id = ElementId;
//!     type Mounted = MyNode<Mounted>;
//!
//!     fn mount(self, parent: Option<ElementId>, parent_depth: Depth) -> MyNode<Mounted> {
//!         MyNode {
//!             depth: if parent.is_some() { parent_depth.child_depth() } else { Depth::root() },
//!             parent,
//!             _state: PhantomData,
//!         }
//!     }
//! }
//!
//! impl Unmountable for MyNode<Mounted> {
//!     type Id = ElementId;
//!     type Unmounted = MyNode<Unmounted>;
//!
//!     fn parent(&self) -> Option<ElementId> { self.parent }
//!     fn depth(&self) -> Depth { self.depth }
//!     fn unmount(self) -> MyNode<Unmounted> {
//!         MyNode { depth: Depth::root(), parent: None, _state: PhantomData }
//!     }
//! }
//!
//! fn main() {
//!     let node: MyNode<Unmounted> = MyNode {
//!         depth: Depth::root(),
//!         parent: None,
//!         _state: PhantomData,
//!     };
//!     let parent_id = ElementId::zip(1);
//!     let mounted = node.mount(Some(parent_id), Depth::root());
//!     assert_eq!(mounted.depth(), Depth::new(1));
//!     assert_eq!(mounted.parent(), Some(parent_id));
//!     let _unmounted = mounted.unmount();
//! }
//! ```

use crate::depth::Depth;
use flui_foundation::Identifier;

// ============================================================================
// NODE STATE (typestate markers)
// ============================================================================

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Unmounted {}
    impl Sealed for super::Mounted {}
}

/// Marker trait for node states.
///
/// Sealed - only `Unmounted` and `Mounted` can implement.
///
/// # Example
///
/// ```
/// use flui_tree::{NodeState, Mounted, Unmounted};
///
/// fn check_state<S: NodeState>() -> &'static str {
///     if S::IS_MOUNTED {
///         "mounted"
///     } else {
///         "unmounted"
///     }
/// }
///
/// assert_eq!(check_state::<Mounted>(), "mounted");
/// assert_eq!(check_state::<Unmounted>(), "unmounted");
/// assert_eq!(Mounted::name(), "Mounted");
/// assert_eq!(Unmounted::name(), "Unmounted");
/// ```
pub trait NodeState: sealed::Sealed + Send + Sync + Copy + Default + 'static {
    /// Whether this state represents a mounted node.
    const IS_MOUNTED: bool;

    /// Human-readable name.
    fn name() -> &'static str;
}

/// Unmounted state - node not in tree.
///
/// Nodes in this state:
/// - Have no valid parent reference
/// - Have no valid depth
/// - Can be mounted into a tree via [`Mountable::mount`]
///
/// # Example
///
/// ```
/// use flui_tree::{Unmounted, NodeState};
///
/// assert!(!Unmounted::IS_MOUNTED);
/// assert_eq!(Unmounted::name(), "Unmounted");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Unmounted;

impl NodeState for Unmounted {
    const IS_MOUNTED: bool = false;

    #[inline]
    fn name() -> &'static str {
        "Unmounted"
    }
}

/// Mounted state - node in tree with position.
///
/// Nodes in this state:
/// - Have valid parent reference (or None if root)
/// - Have valid depth
/// - Can access tree position via [`Unmountable`] methods
/// - Can be unmounted via [`Unmountable::unmount`]
///
/// # Example
///
/// ```
/// use flui_tree::{Mounted, NodeState};
///
/// assert!(Mounted::IS_MOUNTED);
/// assert_eq!(Mounted::name(), "Mounted");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mounted;

impl NodeState for Mounted {
    const IS_MOUNTED: bool = true;

    #[inline]
    fn name() -> &'static str {
        "Mounted"
    }
}

// ============================================================================
// LIFECYCLE TRAITS
// ============================================================================

/// Trait for nodes that can be mounted into a tree.
///
/// Implemented by nodes in `Unmounted` state to transition to `Mounted`.
///
/// # Type Parameters
///
/// - `Id`: The identifier type for nodes in this tree
/// - `Mounted`: The resulting mounted type
///
/// # Example
///
/// ```
/// use flui_tree::{Mountable, MountableExt, Unmountable, Depth, ElementId};
/// use flui_tree::{Mounted, Unmounted, NodeState, Identifier};
/// use std::marker::PhantomData;
///
/// struct MyNode<S: NodeState> {
///     depth: Depth,
///     parent: Option<ElementId>,
///     _state: PhantomData<S>,
/// }
///
/// impl Mountable for MyNode<Unmounted> {
///     type Id = ElementId;
///     type Mounted = MyNode<Mounted>;
///
///     fn mount(self, parent: Option<ElementId>, parent_depth: Depth) -> MyNode<Mounted> {
///         MyNode {
///             depth: if parent.is_some() { parent_depth.child_depth() } else { Depth::root() },
///             parent,
///             _state: PhantomData,
///         }
///     }
/// }
///
/// // Mount as root (parent = None)
/// let root = MyNode::<Unmounted> { depth: Depth::root(), parent: None, _state: PhantomData };
/// let mounted_root = root.mount(None, Depth::root());
///
/// // Mount as child
/// let child = MyNode::<Unmounted> { depth: Depth::root(), parent: None, _state: PhantomData };
/// let mounted_child = child.mount(Some(ElementId::zip(1)), Depth::root());
/// ```
pub trait Mountable: Sized {
    /// The identifier type for this tree.
    type Id: Identifier;

    /// The mounted version of this type.
    type Mounted;

    /// Mount this node into a tree.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent node ID, or `None` for root
    /// - `parent_depth`: Depth of the parent (use `Depth::root()` for root nodes)
    ///
    /// # Returns
    ///
    /// The mounted node with tree position information.
    fn mount(self, parent: Option<Self::Id>, parent_depth: Depth) -> Self::Mounted;
}

/// Trait for nodes that are mounted and can be unmounted.
///
/// Implemented by nodes in `Mounted` state. Provides access to tree position
/// and ability to transition back to `Unmounted`.
///
/// # Type Parameters
///
/// - `Id`: The identifier type for nodes in this tree
/// - `Unmounted`: The resulting unmounted type
///
/// # Example
///
/// ```
/// use flui_tree::{Mountable, Unmountable, Depth, ElementId};
/// use flui_tree::{Mounted, Unmounted, NodeState, Identifier};
/// use std::marker::PhantomData;
///
/// struct MyNode<S: NodeState> {
///     depth: Depth,
///     parent: Option<ElementId>,
///     _state: PhantomData<S>,
/// }
///
/// impl Mountable for MyNode<Unmounted> {
///     type Id = ElementId;
///     type Mounted = MyNode<Mounted>;
///     fn mount(self, parent: Option<ElementId>, parent_depth: Depth) -> MyNode<Mounted> {
///         MyNode {
///             depth: if parent.is_some() { parent_depth.child_depth() } else { Depth::root() },
///             parent,
///             _state: PhantomData,
///         }
///     }
/// }
///
/// impl Unmountable for MyNode<Mounted> {
///     type Id = ElementId;
///     type Unmounted = MyNode<Unmounted>;
///     fn parent(&self) -> Option<ElementId> { self.parent }
///     fn depth(&self) -> Depth { self.depth }
///     fn unmount(self) -> MyNode<Unmounted> {
///         MyNode { depth: Depth::root(), parent: None, _state: PhantomData }
///     }
/// }
///
/// let node = MyNode::<Unmounted> { depth: Depth::root(), parent: None, _state: PhantomData };
/// let mounted = node.mount(Some(ElementId::zip(1)), Depth::new(2));
///
/// // Access position
/// assert_eq!(mounted.parent(), Some(ElementId::zip(1)));
/// assert_eq!(mounted.depth(), Depth::new(3));
/// assert!(!mounted.is_root());
///
/// // Unmount
/// let _unmounted = mounted.unmount();
/// ```
pub trait Unmountable: Sized {
    /// The identifier type for this tree.
    type Id: Identifier;

    /// The unmounted version of this type.
    type Unmounted;

    /// Get the parent node ID.
    ///
    /// Returns `None` if this is the root node.
    fn parent(&self) -> Option<Self::Id>;

    /// Get the depth in the tree.
    ///
    /// Root nodes have depth 0.
    fn depth(&self) -> Depth;

    /// Check if this is the root node (no parent).
    #[inline]
    fn is_root(&self) -> bool {
        self.parent().is_none()
    }

    /// Unmount from tree.
    ///
    /// Returns the unmounted node. Configuration is typically preserved
    /// for hot-reload support.
    fn unmount(self) -> Self::Unmounted;
}

// ============================================================================
// EXTENSION TRAIT FOR CONVENIENCE METHODS
// ============================================================================

/// Extension trait for `Mountable` with convenience methods.
pub trait MountableExt: Mountable {
    /// Mount as root node.
    ///
    /// Equivalent to `mount(None, Depth::root())`.
    #[inline]
    fn mount_root(self) -> Self::Mounted {
        self.mount(None, Depth::root())
    }

    /// Mount as child of the given parent.
    ///
    /// Equivalent to `mount(Some(parent), parent_depth)`.
    #[inline]
    fn mount_child(self, parent: Self::Id, parent_depth: Depth) -> Self::Mounted {
        self.mount(Some(parent), parent_depth)
    }
}

// Blanket implementation for all Mountable types
impl<T: Mountable> MountableExt for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::marker::PhantomData;

    // Mock ID for testing
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct TestId(std::num::NonZeroUsize);

    impl std::fmt::Display for TestId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestId({})", self.0)
        }
    }

    impl From<TestId> for usize {
        fn from(id: TestId) -> usize {
            id.0.get()
        }
    }

    impl Identifier for TestId {
        fn get(self) -> usize {
            self.0.get()
        }

        fn zip(index: usize) -> Self {
            Self(std::num::NonZeroUsize::new(index).expect("TestId cannot be 0"))
        }

        fn try_zip(index: usize) -> Option<Self> {
            std::num::NonZeroUsize::new(index).map(Self)
        }
    }

    // Mock node for testing
    struct TestNode<S: NodeState> {
        depth: Depth,
        parent: Option<TestId>,
        value: i32,
        _state: PhantomData<S>,
    }

    impl TestNode<Unmounted> {
        fn new(value: i32) -> Self {
            Self {
                depth: Depth::root(),
                parent: None,
                value,
                _state: PhantomData,
            }
        }
    }

    impl Mountable for TestNode<Unmounted> {
        type Id = TestId;
        type Mounted = TestNode<Mounted>;

        fn mount(self, parent: Option<TestId>, parent_depth: Depth) -> TestNode<Mounted> {
            let depth = if parent.is_some() {
                parent_depth.child_depth()
            } else {
                Depth::root()
            };
            TestNode {
                depth,
                parent,
                value: self.value,
                _state: PhantomData,
            }
        }
    }

    impl Unmountable for TestNode<Mounted> {
        type Id = TestId;
        type Unmounted = TestNode<Unmounted>;

        fn parent(&self) -> Option<TestId> {
            self.parent
        }

        fn depth(&self) -> Depth {
            self.depth
        }

        fn unmount(self) -> TestNode<Unmounted> {
            TestNode {
                depth: Depth::root(),
                parent: None,
                value: self.value,
                _state: PhantomData,
            }
        }
    }

    // ===== NodeState Tests =====

    #[test]
    fn test_unmounted_state() {
        assert!(!Unmounted::IS_MOUNTED);
        assert_eq!(Unmounted::name(), "Unmounted");
    }

    #[test]
    fn test_mounted_state() {
        assert!(Mounted::IS_MOUNTED);
        assert_eq!(Mounted::name(), "Mounted");
    }

    #[test]
    fn test_state_default() {
        let _: Unmounted = Default::default();
        let _: Mounted = Default::default();
    }

    #[test]
    fn test_state_copy() {
        let u = Unmounted;
        let u2 = u;
        assert_eq!(u, u2);

        let m = Mounted;
        let m2 = m;
        assert_eq!(m, m2);
    }

    // ===== Mountable Tests =====

    #[test]
    fn test_mount_as_root() {
        let node = TestNode::new(42);
        let mounted = node.mount(None, Depth::root());

        assert!(mounted.is_root());
        assert_eq!(mounted.parent(), None);
        assert_eq!(mounted.depth(), Depth::root());
    }

    #[test]
    fn test_mount_as_child() {
        let node = TestNode::new(42);
        let parent_id = TestId::zip(10);
        let mounted = node.mount(Some(parent_id), Depth::new(2));

        assert!(!mounted.is_root());
        assert_eq!(mounted.parent(), Some(parent_id));
        assert_eq!(mounted.depth(), Depth::new(3)); // parent + 1
    }

    #[test]
    fn test_mount_root_ext() {
        let node = TestNode::new(42);
        let mounted = node.mount_root();

        assert!(mounted.is_root());
        assert_eq!(mounted.depth(), Depth::root());
    }

    #[test]
    fn test_mount_child_ext() {
        let node = TestNode::new(42);
        let parent_id = TestId::zip(5);
        let mounted = node.mount_child(parent_id, Depth::root());

        assert!(!mounted.is_root());
        assert_eq!(mounted.parent(), Some(parent_id));
        assert_eq!(mounted.depth(), Depth::new(1));
    }

    // ===== Unmountable Tests =====

    #[test]
    fn test_unmount() {
        let node = TestNode::new(42);
        let mounted = node.mount_root();
        let unmounted = mounted.unmount();

        // Value preserved
        assert_eq!(unmounted.value, 42);
    }

    #[test]
    fn test_is_root() {
        let root = TestNode::new(1).mount_root();
        let child = TestNode::new(2).mount_child(TestId::zip(1), Depth::root());

        assert!(root.is_root());
        assert!(!child.is_root());
    }

    #[test]
    fn test_roundtrip() {
        let original = TestNode::new(99);
        let mounted = original.mount_child(TestId::zip(5), Depth::new(2));

        assert_eq!(mounted.depth(), Depth::new(3));

        let unmounted = mounted.unmount();
        let remounted = unmounted.mount_root();

        assert!(remounted.is_root());
        assert_eq!(remounted.depth(), Depth::root());
    }
}
