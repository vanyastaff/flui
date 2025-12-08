//! Read-only tree access trait.
//!
//! This module provides the [`TreeRead`] trait for immutable access
//! to tree nodes without navigation capabilities.

use flui_foundation::TreeId;

/// Read-only access to tree nodes with Generic Associated Types.
///
/// This is the most fundamental tree trait, providing only immutable
/// access to nodes by their ID. It intentionally does not include
/// navigation (parent/children) to allow simple implementations.
///
/// # Advanced Type Features
///
/// This trait uses several advanced Rust type system features:
/// - **GAT (Generic Associated Types)** for flexible iterators
/// - **Associated Constants** for performance tuning
/// - **Sealed trait** for safety
/// - **HRTB-compatible** design for visitor composition
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to enable concurrent read access.
///
/// # Performance
///
/// All operations should be O(1) for slab-based implementations.
/// Performance can be tuned via associated constants.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{TreeRead, TreeId};
/// use flui_foundation::ElementId;
///
/// // A simple tree storing strings
/// struct SimpleTree {
///     nodes: Vec<Option<String>>,
/// }
///
/// // Implement sealed trait first
/// impl flui_tree::traits::sealed::TreeReadSealed for SimpleTree {}
///
/// impl TreeRead for SimpleTree {
///     type Id = ElementId;
///     type Node = String;
///     type NodeIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;
///
///     fn get(&self, id: ElementId) -> Option<&Self::Node> {
///         self.nodes.get(id.get() - 1)?.as_ref()
///     }
///
///     fn len(&self) -> usize {
///         self.nodes.iter().filter(|n| n.is_some()).count()
///     }
///
///     fn node_ids(&self) -> Self::NodeIter<'_> {
///         Box::new((0..self.nodes.len()).filter_map(|i| {
///             if self.nodes[i].is_some() {
///                 Some(ElementId::new(i + 1))
///             } else {
///                 None
///             }
///         }))
///     }
/// }
/// ```
pub trait TreeRead: sealed::Sealed + Send + Sync {
    /// The ID type used for node identification.
    ///
    /// This associated type allows implementations to use different
    /// ID types (`ElementId`, `ViewId`, `RenderId`, etc.) while
    /// maintaining type safety through the [`TreeId`] trait bound.
    type Id: TreeId;

    /// The node type stored in the tree.
    ///
    /// This associated type allows implementations to define their
    /// own node structure while maintaining type safety.
    type Node;

    /// Iterator type for node IDs with Generic Associated Types.
    ///
    /// This GAT allows implementations to return different iterator types
    /// while maintaining lifetime safety and zero-cost abstractions.
    type NodeIter<'a>: Iterator<Item = Self::Id> + 'a
    where
        Self: 'a;

    /// Default capacity for internal collections.
    ///
    /// This associated constant allows implementations to tune
    /// performance characteristics for their specific use case.
    const DEFAULT_CAPACITY: usize = 32;

    /// Threshold for using inline vs heap allocation.
    ///
    /// Operations involving fewer than this many elements should
    /// prefer stack allocation for better performance.
    const INLINE_THRESHOLD: usize = 16;

    /// Cache line size hint for optimizing memory layout.
    ///
    /// Used by implementations to optimize data structure layout
    /// for better cache performance.
    const CACHE_LINE_SIZE: usize = 64;

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
    fn get(&self, id: Self::Id) -> Option<&Self::Node>;

    /// Returns `true` if the tree contains a node with the given ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The unique identifier to check
    ///
    /// # Performance
    ///
    /// Default implementation calls `get()`. Implementations may
    /// provide a more efficient version using bitmap or other structures.
    #[inline]
    fn contains(&self, id: Self::Id) -> bool {
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
    /// # GAT Benefits
    ///
    /// Using GAT allows implementations to return:
    /// - Zero-cost iterators with appropriate lifetimes
    /// - Different iterator types based on internal structure
    /// - Optimized implementations for specific use cases
    fn node_ids(&self) -> Self::NodeIter<'_>;

    /// Get multiple nodes efficiently.
    ///
    /// This method allows implementations to optimize batch access
    /// patterns, which is common in tree operations.
    ///
    /// # Performance
    ///
    /// Default implementation calls `get()` for each ID, but
    /// implementations may provide vectorized or cache-friendly versions.
    fn get_many<const N: usize>(&self, ids: [Self::Id; N]) -> [Option<&Self::Node>; N] {
        ids.map(|id| self.get(id))
    }

    /// Check if all given IDs exist in the tree.
    ///
    /// This is useful for validation and can be optimized using
    /// bitmap operations or SIMD instructions.
    fn contains_all(&self, ids: &[Self::Id]) -> bool {
        ids.iter().all(|&id| self.contains(id))
    }

    /// Check if any of the given IDs exist in the tree.
    ///
    /// Short-circuits on first found element.
    fn contains_any(&self, ids: &[Self::Id]) -> bool {
        ids.iter().any(|&id| self.contains(id))
    }
}

/// Sealed trait pattern to prevent external implementations.
///
/// This ensures that only well-tested implementations in this crate
/// can implement the core TreeRead trait, preventing subtle bugs
/// from incorrect external implementations.
///
/// External crates can implement the sealed trait via
/// `flui_tree::traits::sealed::TreeReadSealed`.
pub(crate) mod sealed {
    /// Sealed trait marker.
    ///
    /// Only types in this crate can implement this trait,
    /// ensuring TreeRead remains correctly implemented.
    pub trait Sealed {}

    // Blanket implementations for wrapper types
    impl<T: Sealed + ?Sized> Sealed for &T {}
    impl<T: Sealed + ?Sized> Sealed for &mut T {}
    impl<T: Sealed + ?Sized> Sealed for Box<T> {}
}

// ============================================================================
// EXTENSION TRAIT FOR ADDITIONAL FUNCTIONALITY
// ============================================================================

/// Extension trait for TreeRead with additional utility methods.
///
/// This trait provides higher-level operations built on top of
/// the core TreeRead functionality using HRTB patterns.
pub trait TreeReadExt: TreeRead {
    /// Find first node matching a predicate using HRTB.
    ///
    /// This method uses Higher-Rank Trait Bounds to accept
    /// predicates that work with any lifetime.
    fn find_node_where<P>(&self, mut predicate: P) -> Option<Self::Id>
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        for id in self.node_ids() {
            if let Some(node) = self.get(id) {
                if predicate(node) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Count nodes matching a predicate.
    fn count_nodes_where<P>(&self, mut predicate: P) -> usize
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        self.node_ids()
            .filter_map(|id| self.get(id))
            .filter(|node| predicate(node))
            .count()
    }

    /// Collect nodes matching a predicate with capacity optimization.
    fn collect_nodes_where<P>(&self, mut predicate: P) -> Vec<Self::Id>
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        let mut result = Vec::with_capacity(Self::INLINE_THRESHOLD.min(self.len()));

        for id in self.node_ids() {
            if let Some(node) = self.get(id) {
                if predicate(node) {
                    result.push(id);
                }
            }
        }

        result
    }

    /// Execute a closure for each node using HRTB for maximum flexibility.
    fn for_each_node<F>(&self, mut f: F)
    where
        F: for<'a> FnMut(Self::Id, &'a Self::Node),
    {
        for id in self.node_ids() {
            if let Some(node) = self.get(id) {
                f(id, node);
            }
        }
    }
}

// Blanket implementation for all TreeRead types
impl<T: TreeRead> TreeReadExt for T {}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

/// Blanket implementation for references to `TreeRead`.
impl<T: TreeRead + ?Sized> TreeRead for &T {
    type Id = T::Id;
    type Node = T::Node;
    type NodeIter<'a>
        = T::NodeIter<'a>
    where
        Self: 'a,
        T: 'a;

    const DEFAULT_CAPACITY: usize = T::DEFAULT_CAPACITY;
    const INLINE_THRESHOLD: usize = T::INLINE_THRESHOLD;
    const CACHE_LINE_SIZE: usize = T::CACHE_LINE_SIZE;

    #[inline]
    fn get(&self, id: Self::Id) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: Self::Id) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Self::NodeIter<'_> {
        (**self).node_ids()
    }

    #[inline]
    fn get_many<const N: usize>(&self, ids: [Self::Id; N]) -> [Option<&Self::Node>; N] {
        (**self).get_many(ids)
    }

    #[inline]
    fn contains_all(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_all(ids)
    }

    #[inline]
    fn contains_any(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_any(ids)
    }
}

/// Blanket implementation for mutable references to `TreeRead`.
impl<T: TreeRead + ?Sized> TreeRead for &mut T {
    type Id = T::Id;
    type Node = T::Node;
    type NodeIter<'a>
        = T::NodeIter<'a>
    where
        Self: 'a,
        T: 'a;

    const DEFAULT_CAPACITY: usize = T::DEFAULT_CAPACITY;
    const INLINE_THRESHOLD: usize = T::INLINE_THRESHOLD;
    const CACHE_LINE_SIZE: usize = T::CACHE_LINE_SIZE;

    #[inline]
    fn get(&self, id: Self::Id) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: Self::Id) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Self::NodeIter<'_> {
        (**self).node_ids()
    }

    #[inline]
    fn get_many<const N: usize>(&self, ids: [Self::Id; N]) -> [Option<&Self::Node>; N] {
        (**self).get_many(ids)
    }

    #[inline]
    fn contains_all(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_all(ids)
    }

    #[inline]
    fn contains_any(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_any(ids)
    }
}

/// Blanket implementation for Box<dyn TreeRead>.
impl<T: TreeRead + ?Sized> TreeRead for Box<T> {
    type Id = T::Id;
    type Node = T::Node;
    type NodeIter<'a>
        = T::NodeIter<'a>
    where
        Self: 'a,
        T: 'a;

    const DEFAULT_CAPACITY: usize = T::DEFAULT_CAPACITY;
    const INLINE_THRESHOLD: usize = T::INLINE_THRESHOLD;
    const CACHE_LINE_SIZE: usize = T::CACHE_LINE_SIZE;

    #[inline]
    fn get(&self, id: Self::Id) -> Option<&Self::Node> {
        (**self).get(id)
    }

    #[inline]
    fn contains(&self, id: Self::Id) -> bool {
        (**self).contains(id)
    }

    #[inline]
    fn len(&self) -> usize {
        (**self).len()
    }

    #[inline]
    fn node_ids(&self) -> Self::NodeIter<'_> {
        (**self).node_ids()
    }

    #[inline]
    fn get_many<const N: usize>(&self, ids: [Self::Id; N]) -> [Option<&Self::Node>; N] {
        (**self).get_many(ids)
    }

    #[inline]
    fn contains_all(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_all(ids)
    }

    #[inline]
    fn contains_any(&self, ids: &[Self::Id]) -> bool {
        (**self).contains_any(ids)
    }
}

// ============================================================================
// UTILITY TYPES AND FUNCTIONS
// ============================================================================

/// Type alias for higher-rank predicate functions.
///
/// This makes it easier to work with HRTB predicates in function signatures.
pub type NodePredicate<Node> = dyn for<'a> Fn(&'a Node) -> bool;

/// Type alias for higher-rank visitor functions.
pub type NodeVisitor<Id, Node> = dyn for<'a> FnMut(Id, &'a Node);

/// Collect all nodes from a tree that match a predicate.
///
/// This is a convenience function that uses the tree's performance
/// constants for optimal memory allocation.
pub fn collect_matching_nodes<T, P>(tree: &T, predicate: P) -> Vec<T::Id>
where
    T: TreeRead,
    P: for<'a> Fn(&'a T::Node) -> bool,
{
    tree.collect_nodes_where(predicate)
}

/// Count nodes in a tree matching a predicate.
pub fn count_matching_nodes<T, P>(tree: &T, predicate: P) -> usize
where
    T: TreeRead,
    P: for<'a> Fn(&'a T::Node) -> bool,
{
    tree.count_nodes_where(predicate)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;

    // Simple test implementation with sealed trait
    struct TestTree {
        nodes: Vec<Option<String>>,
    }

    impl sealed::Sealed for TestTree {}

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
        type Id = ElementId;
        type Node = String;
        type NodeIter<'a> = Box<dyn Iterator<Item = ElementId> + 'a>;

        const DEFAULT_CAPACITY: usize = 64;
        const INLINE_THRESHOLD: usize = 8;

        fn get(&self, id: ElementId) -> Option<&String> {
            let index = id.get() - 1;
            self.nodes.get(index)?.as_ref()
        }

        fn contains(&self, id: ElementId) -> bool {
            let index = id.get() - 1;
            self.nodes
                .get(index)
                .is_some_and(std::option::Option::is_some)
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> Self::NodeIter<'_> {
            Box::new((0..self.nodes.len()).filter_map(move |i| {
                if self.nodes[i].is_some() {
                    Some(ElementId::new(i + 1))
                } else {
                    None
                }
            }))
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

        let _ = tree.insert("one".to_string());
        let _ = tree.insert("two".to_string());

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn test_node_ids_iterator() {
        let mut tree = TestTree::new();
        let id1 = tree.insert("first".to_string());
        let id2 = tree.insert("second".to_string());

        let collected: Vec<_> = tree.node_ids().collect();
        assert_eq!(collected, vec![id1, id2]);
    }

    #[test]
    fn test_get_many() {
        let mut tree = TestTree::new();
        let id1 = tree.insert("first".to_string());
        let id2 = tree.insert("second".to_string());
        let invalid_id = ElementId::new(999);

        let results = tree.get_many([id1, id2, invalid_id]);
        assert_eq!(results[0], Some(&"first".to_string()));
        assert_eq!(results[1], Some(&"second".to_string()));
        assert_eq!(results[2], None);
    }

    #[test]
    fn test_contains_all_and_any() {
        let mut tree = TestTree::new();
        let id1 = tree.insert("first".to_string());
        let id2 = tree.insert("second".to_string());
        let invalid_id = ElementId::new(999);

        assert!(tree.contains_all(&[id1, id2]));
        assert!(!tree.contains_all(&[id1, invalid_id]));
        assert!(tree.contains_any(&[id1, invalid_id]));
        assert!(!tree.contains_any(&[invalid_id]));
    }

    #[test]
    fn test_tree_read_ext() {
        let mut tree = TestTree::new();
        let _id1 = tree.insert("hello".to_string());
        let _id2 = tree.insert("world".to_string());
        let _id3 = tree.insert("hello again".to_string());

        // Test HRTB predicate
        let hello_nodes = tree.collect_nodes_where(|node| node.contains("hello"));
        assert_eq!(hello_nodes.len(), 2);

        let count = tree.count_nodes_where(|node| node.len() > 5);
        assert_eq!(count, 1); // "hello again"

        // Test find_node_where
        let found = tree.find_node_where(|node| node == "world");
        assert!(found.is_some());
    }

    #[test]
    fn test_reference_impls() {
        let mut tree = TestTree::new();
        let id = tree.insert("hello".to_string());

        // Test immutable reference
        let tree_ref: &TestTree = &tree;
        assert_eq!(tree_ref.get(id), Some(&"hello".to_string()));

        // Test mutable reference
        let tree_mut: &mut TestTree = &mut tree;
        assert_eq!(tree_mut.get(id), Some(&"hello".to_string()));
    }

    #[test]
    fn test_associated_constants() {
        assert_eq!(TestTree::DEFAULT_CAPACITY, 64);
        assert_eq!(TestTree::INLINE_THRESHOLD, 8);
        assert_eq!(TestTree::CACHE_LINE_SIZE, 64);
    }
}
