//! Tree navigation trait with Generic Associated Types.
//!
//! This module provides the [`TreeNav`] trait for tree navigation
//! operations using advanced Rust type system features.

use flui_foundation::Identifier;

use crate::depth::Depth;
use crate::iter::cursor::TreeCursor;
use crate::iter::path::TreePath;
use crate::iter::slot::Slot;

/// Tree navigation with RPITIT and HRTB support.
///
/// This trait provides navigation capabilities (parent, children, ancestors)
/// using advanced Rust type system features:
///
/// - **RPITIT (Return Position Impl Trait In Traits)** for zero-cost iterators
/// - **HRTB (Higher-Rank Trait Bounds)** for universal predicates
/// - **Associated Constants** for performance tuning
/// - **Sealed trait** for safety
/// - **Const generics** for compile-time optimization
///
/// # Generic Parameter
///
/// The `I` parameter specifies the ID type used for node identification,
/// matching the same type used in [`TreeRead<I>`](super::TreeRead).
///
/// # Thread Safety
///
/// All operations are read-only and must be `Send + Sync` compatible.
///
/// # Performance
///
/// Navigation operations should be O(1) for parent/children access.
/// Iterator operations use associated constants for optimal memory usage.
pub trait TreeNav<I: Identifier>: super::TreeRead<I> {
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
    /// `Some(Id)` if the node has a parent, `None` if it's a root.
    ///
    /// # Performance
    ///
    /// Should be O(1) for most implementations.
    fn parent(&self, id: I) -> Option<I>;

    /// Returns an iterator over the immediate children of a node.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent node
    ///
    /// # Returns
    ///
    /// An iterator yielding child IDs in tree order.
    ///
    /// # RPITIT Benefits
    ///
    /// Using RPITIT allows:
    /// - Zero-cost iteration without GAT boilerplate
    /// - Simple implementations without custom iterator types
    /// - Lifetime-safe iteration without boxing
    fn children(&self, id: I) -> impl Iterator<Item = I> + '_;

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
    /// Uses stack-allocated buffer up to `MAX_DEPTH`, then heap allocation.
    fn ancestors(&self, start: I) -> impl Iterator<Item = I> + '_;

    /// Returns an iterator over all descendants in depth-first order.
    ///
    /// Yields tuples of (Id, depth) where depth is relative
    /// to the starting node (start node has depth 0).
    ///
    /// # Arguments
    ///
    /// * `root` - The root of the subtree to traverse
    ///
    /// # Performance
    ///
    /// Optimized for both shallow and deep trees using hybrid allocation.
    fn descendants(&self, root: I) -> impl Iterator<Item = (I, usize)> + '_;

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
    /// Iterator over sibling IDs, excluding the input node.
    fn siblings(&self, id: I) -> impl Iterator<Item = I> + '_;

    /// Returns slot information for a node.
    ///
    /// Slot represents the position of a child within its parent with:
    /// - Parent ID
    /// - Index within parent
    /// - Depth in tree
    /// - Previous/next sibling references
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose slot to retrieve
    ///
    /// # Returns
    ///
    /// `Some(Slot<I>)` with full position context, `None` if node is root or not found.
    ///
    /// # Default Implementation
    ///
    /// Computes slot info from parent/children/depth.
    fn slot(&self, id: I) -> Option<Slot<I>> {
        let parent = self.parent(id)?;
        let children: Vec<I> = self.children(parent).collect();
        let index = children.iter().position(|&c| c == id)?;
        let depth = Depth::new(self.depth(id));

        let previous_sibling = if index > 0 {
            Some(children[index - 1])
        } else {
            None
        };

        let next_sibling = children.get(index + 1).copied();

        Some(Slot::with_siblings(
            parent,
            index,
            depth,
            previous_sibling,
            next_sibling,
        ))
    }

    /// Returns the number of immediate children.
    ///
    /// # Performance
    ///
    /// Default implementation uses iterator count, but implementations
    /// should provide O(1) versions when possible.
    #[inline]
    fn child_count(&self, id: I) -> usize {
        self.children(id).count()
    }

    /// Check if a node has any children.
    ///
    /// # Performance
    ///
    /// More efficient than `child_count() > 0` for some implementations.
    #[inline]
    fn has_children(&self, id: I) -> bool {
        self.children(id).next().is_some()
    }

    /// Check if a node is a leaf (has no children).
    #[inline]
    fn is_leaf(&self, id: I) -> bool {
        !self.has_children(id)
    }

    /// Check if a node is the root (has no parent).
    #[inline]
    fn is_root(&self, id: I) -> bool {
        self.parent(id).is_none()
    }

    /// Find the root of the tree containing the given node.
    ///
    /// Walks up the parent chain until it finds a node with no parent.
    ///
    /// # Performance
    ///
    /// Uses stack allocation for typical tree depths.
    fn find_root(&self, mut id: I) -> I {
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
    fn depth(&self, id: I) -> usize {
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
    fn is_ancestor_of(&self, ancestor: I, descendant: I) -> bool {
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
    /// `Some(Id)` of the LCA, or `None` if nodes are in different trees.
    ///
    /// # Performance
    ///
    /// Uses optimized two-pointer technique with stack allocation.
    fn lowest_common_ancestor(&self, a: I, b: I) -> Option<I> {
        if a == b {
            return Some(a);
        }

        // Collect ancestors of both nodes
        let ancestors_a: Vec<I> = self.ancestors(a).collect();
        let ancestors_b: Vec<I> = self.ancestors(b).collect();

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

/// Extension trait for `TreeNav` with HRTB-based operations.
///
/// This trait provides higher-level operations using Higher-Rank Trait Bounds
/// for maximum flexibility with predicates and visitors.
pub trait TreeNavExt<I: Identifier>: TreeNav<I> {
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
    fn find_child_where<P>(&self, parent: I, mut predicate: P) -> Option<I>
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
    fn find_descendant_where<P>(&self, root: I, mut predicate: P) -> Option<I>
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
    fn visit_subtree<F>(&self, root: I, mut visitor: F)
    where
        F: for<'a> FnMut(I, &'a Self::Node, usize),
    {
        for (id, depth) in self.descendants(root) {
            if let Some(node) = self.get(id) {
                visitor(id, node, depth);
            }
        }
    }

    /// Count descendants matching a predicate.
    fn count_descendants_where<P>(&self, root: I, mut predicate: P) -> usize
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
    /// Returns a [`TreePath`] containing the complete path from root to target.
    /// Use `TreePath` for rich path operations like comparison, truncation,
    /// and validation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flui_tree::{TreeNavExt, ElementId};
    /// # fn example(tree: &impl flui_tree::TreeNav<ElementId>) {
    /// let grandchild = ElementId::new(3);
    /// let path = tree.path_to_node(grandchild);
    /// assert!(!path.is_empty());
    /// # }
    /// ```
    fn path_to_node(&self, target: I) -> TreePath<I> {
        TreePath::from_node(self, target)
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
    fn nth_child(&self, parent: I, index: usize) -> Option<I> {
        self.children(parent).nth(index)
    }

    /// Get the first and last child efficiently.
    ///
    /// # Returns
    ///
    /// `(first, last)` tuple, or `None` if no children.
    fn first_and_last_child(&self, parent: I) -> Option<(I, I)> {
        let mut children = self.children(parent);
        let first = children.next()?;
        let last = children.last().unwrap_or(first);
        Some((first, last))
    }

    // === CURSOR-BASED NAVIGATION ===

    /// Creates a cursor at the given node for interactive navigation.
    ///
    /// Cursors provide stateful navigation with optional history support.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flui_tree::{TreeNavExt, ElementId};
    /// # fn example(tree: &impl flui_tree::TreeNav<ElementId>) {
    /// let some_node = ElementId::new(1);
    /// let mut cursor = tree.cursor_at(some_node);
    /// while cursor.go_first_child() {
    ///     // descended further into the tree
    /// }
    /// # }
    /// ```
    fn cursor_at(&self, position: I) -> TreeCursor<'_, Self, I>
    where
        Self: Sized,
    {
        TreeCursor::new(self, position)
    }

    /// Creates a cursor with history at the given node.
    ///
    /// History allows backtracking to previous positions.
    ///
    /// # Arguments
    ///
    /// * `position` - Starting node ID
    /// * `max_history` - Maximum history stack size
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use flui_tree::{TreeNavExt, ElementId};
    /// # fn example(tree: &impl flui_tree::TreeNav<ElementId>) {
    /// let root = ElementId::new(1);
    /// let mut cursor = tree.cursor_with_history(root, 10);
    /// cursor.go_child(0);
    /// cursor.go_child(1);
    /// cursor.go_back();  // Returns to previous position
    /// # }
    /// ```
    fn cursor_with_history(&self, position: I, max_history: usize) -> TreeCursor<'_, Self, I>
    where
        Self: Sized,
    {
        TreeCursor::with_history(self, position, max_history)
    }

    /// Creates a cursor at the root of the subtree containing `node`.
    fn cursor_at_root(&self, node: I) -> TreeCursor<'_, Self, I>
    where
        Self: Sized,
    {
        TreeCursor::at_root(self, node)
    }
}

// Blanket implementation for all TreeNav types
impl<I: Identifier, T: TreeNav<I>> TreeNavExt<I> for T {}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<I: Identifier, T: TreeNav<I> + ?Sized> TreeNav<I> for &T {
    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: I) -> Option<I> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: I) -> impl Iterator<Item = I> + '_ {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: I) -> impl Iterator<Item = (I, usize)> + '_ {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: I) -> Option<Slot<I>> {
        (**self).slot(id)
    }
}

impl<I: Identifier, T: TreeNav<I> + ?Sized> TreeNav<I> for &mut T {
    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: I) -> Option<I> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: I) -> impl Iterator<Item = I> + '_ {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: I) -> impl Iterator<Item = (I, usize)> + '_ {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: I) -> Option<Slot<I>> {
        (**self).slot(id)
    }
}

impl<I: Identifier, T: TreeNav<I> + ?Sized> TreeNav<I> for Box<T> {
    const MAX_DEPTH: usize = T::MAX_DEPTH;
    const AVG_CHILDREN: usize = T::AVG_CHILDREN;

    #[inline]
    fn parent(&self, id: I) -> Option<I> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).children(id)
    }

    #[inline]
    fn ancestors(&self, start: I) -> impl Iterator<Item = I> + '_ {
        (**self).ancestors(start)
    }

    #[inline]
    fn descendants(&self, root: I) -> impl Iterator<Item = (I, usize)> + '_ {
        (**self).descendants(root)
    }

    #[inline]
    fn siblings(&self, id: I) -> impl Iterator<Item = I> + '_ {
        (**self).siblings(id)
    }

    #[inline]
    fn slot(&self, id: I) -> Option<Slot<I>> {
        (**self).slot(id)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::{Ancestors, DescendantsWithDepth};
    use crate::traits::TreeRead;
    use flui_foundation::ElementId;

    // Test implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    }

    struct TestTree {
        nodes: std::collections::HashMap<ElementId, TestNode>,
    }

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

    impl super::super::TreeRead<ElementId> for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            self.nodes.keys().copied()
        }
    }

    impl TreeNav<ElementId> for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            self.get(id)
                .map(|node| node.children.iter().copied())
                .into_iter()
                .flatten()
        }

        fn ancestors(&self, start: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
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
        assert_eq!(path.as_slice(), &[root, child1]);

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
