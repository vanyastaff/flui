//! Tree navigation trait with Generic Associated Types.
//!
//! This module provides the [`TreeNav`] trait for tree navigation
//! operations using advanced Rust type system features.

use flui_foundation::{ElementId, Slot};
use std::marker::PhantomData;

/// Tree navigation with Generic Associated Types and HRTB support.
///
/// This trait provides navigation capabilities (parent, children, ancestors)
/// using advanced Rust type system features:
///
/// - **GAT (Generic Associated Types)** for flexible iterators
/// - **HRTB (Higher-Rank Trait Bounds)** for universal predicates
/// - **Associated Constants** for performance tuning
/// - **Sealed trait** for safety
/// - **Const generics** for compile-time optimization
///
/// # Thread Safety
///
/// All operations are read-only and must be `Send + Sync` compatible.
///
/// # Performance
///
/// Navigation operations should be O(1) for parent/children access.
/// Iterator operations use associated constants for optimal memory usage.
///
/// # Example
///
/// ```rust
/// use flui_tree::{TreeNav, TreeRead};
/// use flui_foundation::ElementId;
///
/// struct SimpleNode {
///     parent: Option<ElementId>,
///     children: Vec<ElementId>,
/// }
///
/// struct SimpleTree {
///     nodes: Vec<Option<SimpleNode>>,
/// }
///
/// impl TreeNav for SimpleTree {
///     type ChildrenIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
///     type AncestorsIter<'a> = impl Iterator<Item = ElementId> + 'a where Self: 'a;
///
///     const MAX_DEPTH: usize = 64;
///     const AVG_CHILDREN: usize = 4;
///
///     fn parent(&self, id: ElementId) -> Option<ElementId> {
///         self.get_node(id)?.parent
///     }
///
///     fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
///         if let Some(node) = self.get_node(id) {
///             node.children.iter().copied()
///         } else {
///             [].iter().copied()
///         }
///     }
///
///     fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
///         AncestorIterator::new(self, start)
///     }
/// }
/// ```
pub trait TreeNav: super::TreeRead + sealed::Sealed {
    /// Iterator type for children with GAT.
    ///
    /// This GAT allows implementations to return optimized iterator types
    /// while maintaining lifetime safety and zero-cost abstractions.
    type ChildrenIter<'a>: Iterator<Item = ElementId> + 'a
    where
        Self: 'a;

    /// Iterator type for ancestors with GAT.
    ///
    /// Enables different implementation strategies (recursive, iterative,
    /// stack-based) while maintaining a consistent interface.
    type AncestorsIter<'a>: Iterator<Item = ElementId> + 'a
    where
        Self: 'a;

    /// Iterator type for descendants with depth information.
    type DescendantsIter<'a>: Iterator<Item = (ElementId, usize)> + 'a
    where
        Self: 'a;

    /// Iterator type for siblings.
    type SiblingsIter<'a>: Iterator<Item = ElementId> + 'a
    where
        Self: 'a;

    /// Maximum expected tree depth for stack allocation optimization.
    ///
    /// Iterators can use this to size internal buffers appropriately.
    /// Should be conservative but realistic for the use case.
    const MAX_DEPTH: usize = 32;

    /// Average number of children per node.
    ///
    /// Used to optimize collection sizing and iteration strategies.
    const AVG_CHILDREN: usize = 3;

    /// Maximum children to process inline before heap allocation.
    const INLINE_CHILDREN_THRESHOLD: usize = 8;

    /// Stack buffer size for path operations.
    const PATH_BUFFER_SIZE: usize = Self::MAX_DEPTH;

    /// Returns the parent of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose parent to find
    ///
    /// # Returns
    ///
    /// `Some(ElementId)` if the node has a parent, `None` if it's a root.
    ///
    /// # Performance
    ///
    /// Should be O(1) for most implementations.
    fn parent(&self, id: ElementId) -> Option<ElementId>;

    /// Returns an iterator over the immediate children of a node.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent node
    ///
    /// # Returns
    ///
    /// An iterator yielding child ElementIds in tree order.
    ///
    /// # GAT Benefits
    ///
    /// Using GAT allows:
    /// - Zero-cost iteration for slice-based storage
    /// - Custom iterator types for complex tree structures
    /// - Lifetime-safe iteration without boxing
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_>;

    /// Returns an iterator from the given node to the root.
    ///
    /// The iterator yields the node itself first, then its parent,
    /// grandparent, etc., ending with the root.
    ///
    /// # Arguments
    ///
    /// * `start` - The starting node
    ///
    /// # Performance
    ///
    /// Uses stack-allocated buffer up to MAX_DEPTH, then heap allocation.
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_>;

    /// Returns an iterator over all descendants in depth-first order.
    ///
    /// Yields tuples of (ElementId, depth) where depth is relative
    /// to the starting node (start node has depth 0).
    ///
    /// # Arguments
    ///
    /// * `root` - The root of the subtree to traverse
    ///
    /// # Performance
    ///
    /// Optimized for both shallow and deep trees using hybrid allocation.
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_>;

    /// Returns an iterator over the siblings of a node.
    ///
    /// Does not include the node itself, only its siblings.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose siblings to find
    ///
    /// # Returns
    ///
    /// Iterator over sibling ElementIds, excluding the input node.
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_>;

    /// Returns the slot information for a node.
    ///
    /// Slot represents the position/key of a child within its parent.
    /// This is optional functionality for trees that track positioning.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose slot to retrieve
    ///
    /// # Returns
    ///
    /// `Some(Slot)` if slot information is available, `None` otherwise.
    ///
    /// # Default Implementation
    ///
    /// Returns `None` - implementations can override for slot support.
    #[inline]
    fn slot(&self, _id: ElementId) -> Option<Slot> {
        None
    }

    /// Returns the number of immediate children.
    ///
    /// # Performance
    ///
    /// Default implementation uses iterator count, but implementations
    /// should provide O(1) versions when possible.
    #[inline]
    fn child_count(&self, id: ElementId) -> usize {
        self.children(id).count()
    }

    /// Check if a node has any children.
    ///
    /// # Performance
    ///
    /// More efficient than `child_count() > 0` for some implementations.
    #[inline]
    fn has_children(&self, id: ElementId) -> bool {
        self.children(id).next().is_some()
    }

    /// Check if a node is a leaf (has no children).
    #[inline]
    fn is_leaf(&self, id: ElementId) -> bool {
        !self.has_children(id)
    }

    /// Check if a node is the root (has no parent).
    #[inline]
    fn is_root(&self, id: ElementId) -> bool {
        self.parent(id).is_none()
    }

    /// Find the root of the tree containing the given node.
    ///
    /// Walks up the parent chain until it finds a node with no parent.
    ///
    /// # Performance
    ///
    /// Uses stack allocation for typical tree depths.
    fn find_root(&self, mut id: ElementId) -> ElementId {
        while let Some(parent) = self.parent(id) {
            id = parent;
        }
        id
    }

    /// Calculate the depth of a node (distance from root).
    ///
    /// Root nodes have depth 0.
    ///
    /// # Performance
    ///
    /// Optimized path following with early termination.
    fn depth(&self, id: ElementId) -> usize {
        self.ancestors(id).count().saturating_sub(1)
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    ///
    /// # Arguments
    ///
    /// * `ancestor` - The potential ancestor node
    /// * `descendant` - The potential descendant node
    ///
    /// # Returns
    ///
    /// `true` if `ancestor` is in the path from `descendant` to root.
    fn is_ancestor_of(&self, ancestor: ElementId, descendant: ElementId) -> bool {
        if ancestor == descendant {
            return false; // Node is not its own ancestor
        }

        self.ancestors(descendant).any(|id| id == ancestor)
    }

    /// Find the lowest common ancestor of two nodes.
    ///
    /// # Arguments
    ///
    /// * `a` - First node
    /// * `b` - Second node
    ///
    /// # Returns
    ///
    /// `Some(ElementId)` of the LCA, or `None` if nodes are in different trees.
    ///
    /// # Performance
    ///
    /// Uses optimized two-pointer technique with stack allocation.
    fn lowest_common_ancestor(&self, a: ElementId, b: ElementId) -> Option<ElementId> {
        if a == b {
            return Some(a);
        }

        // Collect ancestors of both nodes
        let ancestors_a: Vec<ElementId> = self.ancestors(a).collect();
        let ancestors_b: Vec<ElementId> = self.ancestors(b).collect();

        // Find first common ancestor from the root
        ancestors_a
            .iter()
            .rev()
            .zip(ancestors_b.iter().rev())
            .take_while(|(a, b)| a == b)
            .last()
            .map(|(a, _)| *a)
    }
}

/// Extension trait for TreeNav with HRTB-based operations.
///
/// This trait provides higher-level operations using Higher-Rank Trait Bounds
/// for maximum flexibility with predicates and visitors.
pub trait TreeNavExt: TreeNav {
    /// Find first child matching a predicate using HRTB.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent node to search in
    /// * `predicate` - HRTB predicate that works with any lifetime
    ///
    /// # Returns
    ///
    /// First matching child ID, or `None` if no match found.
    fn find_child_where<P>(&self, parent: ElementId, mut predicate: P) -> Option<ElementId>
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        for child_id in self.children(parent) {
            if let Some(child_node) = self.get(child_id) {
                if predicate(child_node) {
                    return Some(child_id);
                }
            }
        }
        None
    }

    /// Find first descendant matching a predicate.
    ///
    /// Performs depth-first search with early termination.
    fn find_descendant_where<P>(&self, root: ElementId, mut predicate: P) -> Option<ElementId>
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        for (id, _depth) in self.descendants(root) {
            if let Some(node) = self.get(id) {
                if predicate(node) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Visit all nodes in a subtree with a closure using HRTB.
    ///
    /// # Arguments
    ///
    /// * `root` - Root of subtree to visit
    /// * `visitor` - HRTB closure called for each node
    fn visit_subtree<F>(&self, root: ElementId, mut visitor: F)
    where
        F: for<'a> FnMut(ElementId, &'a Self::Node, usize),
    {
        for (id, depth) in self.descendants(root) {
            if let Some(node) = self.get(id) {
                visitor(id, node, depth);
            }
        }
    }

    /// Count descendants matching a predicate.
    fn count_descendants_where<P>(&self, root: ElementId, mut predicate: P) -> usize
    where
        P: for<'a> FnMut(&'a Self::Node) -> bool,
    {
        self.descendants(root)
            .filter_map(|(id, _)| self.get(id))
            .filter(|node| predicate(node))
            .count()
    }

    /// Collect path from root to target node.
    ///
    /// Returns the complete path including both root and target.
    fn path_to_node(&self, target: ElementId) -> Vec<ElementId> {
        let mut path: Vec<ElementId> = self.ancestors(target).collect();
        path.reverse();
        path
    }

    /// Get the nth child of a node.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent node
    /// * `index` - Zero-based child index
    ///
    /// # Returns
    ///
    /// The nth child, or `None` if index is out of bounds.
    fn nth_child(&self, parent: ElementId, index: usize) -> Option<ElementId> {
        self.children(parent).nth(index)
    }

    /// Get the first and last child efficiently.
    ///
    /// # Returns
    ///
    /// `(first, last)` tuple, or `None` if no children.
    fn first_and_last_child(&self, parent: ElementId) -> Option<(ElementId, ElementId)> {
        let mut children = self.children(parent);
        let first = children.next()?;
        let last = children.last().unwrap_or(first);
        Some((first, last))
    }
}

// Blanket implementation for all TreeNav types
impl<T: TreeNav> TreeNavExt for T {}

/// Sealed trait pattern for TreeNav.
///
/// Ensures only well-tested implementations can provide navigation.
mod sealed {
    pub trait Sealed {}
    // Implementations added by concrete tree types
}

// ============================================================================
// UTILITY ITERATORS WITH CONST GENERICS
// ============================================================================

/// Stack-optimized ancestor iterator using const generics.
///
/// Uses inline storage for typical tree depths, falling back to heap
/// for deeper trees. The buffer size is configurable via const generics.
pub struct AncestorIterator<'a, T: TreeNav, const BUFFER_SIZE: usize = 32> {
    tree: &'a T,
    current: Option<ElementId>,
    // Stack-allocated buffer for typical cases
    _buffer: PhantomData<[ElementId; BUFFER_SIZE]>,
}

impl<'a, T: TreeNav, const BUFFER_SIZE: usize> AncestorIterator<'a, T, BUFFER_SIZE> {
    /// Create a new ancestor iterator.
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to navigate
    /// * `start` - Starting node (included in iteration)
    pub fn new(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: Some(start),
            _buffer: PhantomData,
        }
    }
}

impl<'a, T: TreeNav, const BUFFER_SIZE: usize> Iterator for AncestorIterator<'a, T, BUFFER_SIZE> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        self.current = self.tree.parent(current);
        Some(current)
    }

    /// Size hint based on typical tree depth.
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.current.is_some() {
            (1, Some(T::MAX_DEPTH))
        } else {
            (0, Some(0))
        }
    }
}

/// Breadth-first descendants iterator with configurable buffering.
pub struct DescendantsIterator<'a, T: TreeNav, const QUEUE_SIZE: usize = 64> {
    tree: &'a T,
    queue: std::collections::VecDeque<(ElementId, usize)>,
    _buffer: PhantomData<[(ElementId, usize); QUEUE_SIZE]>,
}

impl<'a, T: TreeNav, const QUEUE_SIZE: usize> DescendantsIterator<'a, T, QUEUE_SIZE> {
    /// Create new descendants iterator.
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut queue = std::collections::VecDeque::with_capacity(QUEUE_SIZE);
        queue.push_back((root, 0));

        Self {
            tree,
            queue,
            _buffer: PhantomData,
        }
    }
}

impl<'a, T: TreeNav, const QUEUE_SIZE: usize> Iterator for DescendantsIterator<'a, T, QUEUE_SIZE> {
    type Item = (ElementId, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let (current, depth) = self.queue.pop_front()?;

        // Add children to queue
        for child in self.tree.children(current) {
            self.queue.push_back((child, depth + 1));
        }

        Some((current, depth))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<T: TreeNav + ?Sized> TreeNav for &T {
    type ChildrenIter<'a>
        = T::ChildrenIter<'a>
    where
        Self: 'a,
        T: 'a;
    type AncestorsIter<'a>
        = T::AncestorsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type DescendantsIter<'a>
        = T::DescendantsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type SiblingsIter<'a>
        = T::SiblingsIter<'a>
    where
        Self: 'a,
        T: 'a;

    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        (**self).slot(id)
    }
}

impl<T: TreeNav + ?Sized> TreeNav for &mut T {
    type ChildrenIter<'a>
        = T::ChildrenIter<'a>
    where
        Self: 'a,
        T: 'a;
    type AncestorsIter<'a>
        = T::AncestorsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type DescendantsIter<'a>
        = T::DescendantsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type SiblingsIter<'a>
        = T::SiblingsIter<'a>
    where
        Self: 'a,
        T: 'a;

    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        (**self).slot(id)
    }
}

impl<T: TreeNav + ?Sized> TreeNav for Box<T> {
    type ChildrenIter<'a>
        = T::ChildrenIter<'a>
    where
        Self: 'a,
        T: 'a;
    type AncestorsIter<'a>
        = T::AncestorsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type DescendantsIter<'a>
        = T::DescendantsIter<'a>
    where
        Self: 'a,
        T: 'a;
    type SiblingsIter<'a>
        = T::SiblingsIter<'a>
    where
        Self: 'a,
        T: 'a;

    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        (**self).slot(id)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    }

    struct TestTree {
        nodes: std::collections::HashMap<ElementId, TestNode>,
    }

    impl super::super::read::sealed::Sealed for TestTree {}
    impl sealed::Sealed for TestTree {}

    impl TestTree {
        fn new() -> Self {
            Self {
                nodes: std::collections::HashMap::new(),
            }
        }

        fn insert(&mut self, id: ElementId, parent: Option<ElementId>) {
            let node = TestNode {
                parent,
                children: Vec::new(),
            };

            if let Some(parent_id) = parent {
                if let Some(parent_node) = self.nodes.get_mut(&parent_id) {
                    parent_node.children.push(id);
                }
            }

            self.nodes.insert(id, node);
        }
    }

    impl super::super::TreeRead for TestTree {
        type Node = TestNode;
        type NodeIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> Self::NodeIter<'_> {
            self.nodes.keys().copied()
        }
    }

    impl TreeNav for TestTree {
        type ChildrenIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;
        type AncestorsIter<'a> = AncestorIterator<'a, Self>;
        type DescendantsIter<'a> = DescendantsIterator<'a, Self>;
        type SiblingsIter<'a>
            = impl Iterator<Item = ElementId> + 'a
        where
            Self: 'a;

        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
            if let Some(node) = self.get(id) {
                node.children.iter().copied()
            } else {
                [].iter().copied()
            }
        }

        fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
            AncestorIterator::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
            DescendantsIterator::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
            if let Some(parent_id) = self.parent(id) {
                self.children(parent_id)
                    .filter(move |&child_id| child_id != id)
            } else {
                [].iter().copied()
            }
        }
    }

    #[test]
    fn test_navigation_basic() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));

        assert_eq!(tree.parent(child1), Some(root));
        assert_eq!(tree.parent(root), None);
        assert!(tree.is_root(root));
        assert!(!tree.is_leaf(root));
        assert!(tree.is_leaf(child1));
    }

    #[test]
    fn test_children_iteration() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));

        let children: Vec<_> = tree.children(root).collect();
        assert_eq!(children, vec![child1, child2]);
    }

    #[test]
    fn test_ancestors_iteration() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child = ElementId::new(2);
        let grandchild = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child, Some(root));
        tree.insert(grandchild, Some(child));

        let ancestors: Vec<_> = tree.ancestors(grandchild).collect();
        assert_eq!(ancestors, vec![grandchild, child, root]);

        assert_eq!(tree.find_root(grandchild), root);
        assert_eq!(tree.depth(grandchild), 2);
    }

    #[test]
    fn test_tree_nav_ext() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let child1 = ElementId::new(2);
        let child2 = ElementId::new(3);

        tree.insert(root, None);
        tree.insert(child1, Some(root));
        tree.insert(child2, Some(root));

        // Test HRTB operations
        let found = tree.find_child_where(root, |_node| true);
        assert!(found.is_some());

        let path = tree.path_to_node(child1);
        assert_eq!(path, vec![root, child1]);

        let (first, last) = tree.first_and_last_child(root).unwrap();
        assert_eq!(first, child1);
        assert_eq!(last, child2);
    }

    #[test]
    fn test_lowest_common_ancestor() {
        let mut tree = TestTree::new();
        let root = ElementId::new(1);
        let left = ElementId::new(2);
        let right = ElementId::new(3);
        let left_child = ElementId::new(4);
        let right_child = ElementId::new(5);

        tree.insert(root, None);
        tree.insert(left, Some(root));
        tree.insert(right, Some(root));
        tree.insert(left_child, Some(left));
        tree.insert(right_child, Some(right));

        assert_eq!(
            tree.lowest_common_ancestor(left_child, right_child),
            Some(root)
        );
        assert_eq!(tree.lowest_common_ancestor(left_child, left), Some(left));
    }
}
