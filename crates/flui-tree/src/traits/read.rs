//! Read-only tree access trait.
//!
//! This module provides the [`TreeRead`] trait for immutable access
//! to tree nodes without navigation capabilities.

use flui_foundation::ElementId;

/// Read-only access to tree nodes.
///
/// This is the most fundamental tree trait, providing only immutable
/// access to nodes by their ID. It intentionally does not include
/// navigation (parent/children) to allow simple implementations.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to enable concurrent read access.
///
/// # Performance
///
/// All operations should be O(1) for slab-based implementations.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::TreeRead;
/// use flui_foundation::ElementId;
///
/// struct SimpleTree<T> {
///     nodes: Vec<Option<T>>,
/// }
///
/// impl<T: Send + Sync> TreeRead for SimpleTree<T> {
///     type Node = T;
///
///     fn get(&self, id: ElementId) -> Option<&Self::Node> {
///         self.nodes.get(id.index())?.as_ref()
///     }
///
///     fn contains(&self, id: ElementId) -> bool {
///         self.nodes.get(id.index()).map_or(false, |n| n.is_some())
///     }
///
///     fn len(&self) -> usize {
///         self.nodes.iter().filter(|n| n.is_some()).count()
///     }
/// }
/// ```
pub trait TreeRead: Send + Sync {
    /// The node type stored in the tree.
    ///
    /// This associated type allows implementations to define their
    /// own node structure while maintaining type safety.
    type Node;

    /// Returns a reference to the node with the given ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier of the node
    ///
    /// # Returns
    ///
    /// `Some(&Node)` if the node exists, `None` otherwise.
    ///
    /// # Performance
    ///
    /// This should be O(1) for slab-based implementations.
    fn get(&self, id: ElementId) -> Option<&Self::Node>;

    /// Returns `true` if the tree contains a node with the given ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier to check
    ///
    /// # Performance
    ///
    /// Default implementation calls `get()`. Implementations may
    /// provide a more efficient version.
    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        self.get(id).is_some()
    }

    /// Returns the number of nodes in the tree.
    ///
    /// # Performance
    ///
    /// Should be O(1) for slab-based implementations that track count.
    fn len(&self) -> usize;

    /// Returns `true` if the tree contains no nodes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an iterator over all node IDs in the tree.
    ///
    /// The order of iteration is implementation-defined but should
    /// be consistent for the same tree state.
    ///
    /// # Default Implementation
    ///
    /// Returns `None` indicating the tree doesn't support enumeration.
    /// Implementations should override this if they can enumerate nodes.
    #[inline]
    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        None
    }
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

/// Blanket implementation for references to TreeRead.
impl<T: TreeRead + ?Sized> TreeRead for &T {
    type Node = T::Node;

    #[inline]
    fn get(&self, id: ElementId) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        (**self).node_ids()
    }
}

/// Blanket implementation for mutable references to TreeRead.
impl<T: TreeRead + ?Sized> TreeRead for &mut T {
    type Node = T::Node;

    #[inline]
    fn get(&self, id: ElementId) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        (**self).node_ids()
    }
}

/// Blanket implementation for Box<dyn TreeRead>.
impl<T: TreeRead + ?Sized> TreeRead for Box<T> {
    type Node = T::Node;

    #[inline]
    fn get(&self, id: ElementId) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        (**self).node_ids()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test implementation
    struct TestTree {
        nodes: Vec<Option<String>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, value: String) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(value));
            id
        }
    }

    impl TreeRead for TestTree {
        type Node = String;

        fn get(&self, id: ElementId) -> Option<&String> {
            let index = id.get() as usize - 1;
            self.nodes.get(index)?.as_ref()
        }

        fn contains(&self, id: ElementId) -> bool {
            let index = id.get() as usize - 1;
            self.nodes.get(index).map_or(false, |n| n.is_some())
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }
    }

    #[test]
    fn test_get() {
        let mut tree = TestTree::new();
        let id = tree.insert("hello".to_string());

        assert_eq!(tree.get(id), Some(&"hello".to_string()));
        assert_eq!(tree.get(ElementId::new(999)), None);
    }

    #[test]
    fn test_contains() {
        let mut tree = TestTree::new();
        let id = tree.insert("hello".to_string());

        assert!(tree.contains(id));
        assert!(!tree.contains(ElementId::new(999)));
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut tree = TestTree::new();

        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);

        tree.insert("one".to_string());
        tree.insert("two".to_string());

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn test_reference_impl() {
        let mut tree = TestTree::new();
        let id = tree.insert("hello".to_string());

        // Test immutable reference
        let tree_ref: &TestTree = &tree;
        assert_eq!(tree_ref.get(id), Some(&"hello".to_string()));

        // Test mutable reference
        let tree_mut: &mut TestTree = &mut tree;
        assert_eq!(tree_mut.get(id), Some(&"hello".to_string()));
    }
}
