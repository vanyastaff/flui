//! Tree navigation trait with Generic Associated Types.
//!
//! This module provides the [`TreeNav`] trait for tree navigation
//! operations using advanced Rust type system features.

use flui_foundation::TreeId;

use smallvec::SmallVec;

use crate::{
    depth::{Depth, INLINE_TREE_DEPTH},
    iter::slot::Slot,
};

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
pub trait TreeNav<I: TreeId>: super::TreeRead<I> {
    /// Maximum expected tree depth for stack allocation optimization.
    ///
    /// Iterators can use this to size internal buffers appropriately.
    /// Defaults to the canonical [`crate::depth::INLINE_TREE_DEPTH`]
    /// (= 32), which is the single source of truth for this depth
    /// constant across the crate. Implementations override when their
    /// tree shape has different
    /// depth characteristics (e.g. `RenderTree` keeps 64 for
    /// scroll-heavy hierarchies).
    const MAX_DEPTH: usize = crate::depth::INLINE_TREE_DEPTH;

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
    /// `Some(Slot<I>)` with full position context, `None` if node is root or
    /// not found.
    ///
    /// # Default Implementation
    ///
    /// Computes slot info from parent/children/depth via a single
    /// streaming pass over the parent's children — no `Vec`
    /// allocation. An earlier version of this default collected the
    /// parent's children into a `Vec<I>` for index lookup, which
    /// allocated on every slot query.
    fn slot(&self, id: I) -> Option<Slot<I>> {
        let parent = self.parent(id)?;

        // Single-pass scan: track previous, target index, and next
        // sibling without materializing the children iterator.
        let mut previous_sibling: Option<I> = None;
        let mut index: Option<usize> = None;
        let mut next_sibling: Option<I> = None;
        let mut prev_candidate: Option<I> = None;
        for (i, child) in self.children(parent).enumerate() {
            if let Some(target_idx) = index {
                // We've passed the target — `child` is the next sibling.
                let _ = target_idx;
                next_sibling = Some(child);
                break;
            }
            if child == id {
                index = Some(i);
                previous_sibling = prev_candidate;
            }
            prev_candidate = Some(child);
        }
        let index = index?;
        let depth = Depth::new(self.depth(id));

        Some(
            Slot::with_siblings()
                .parent(parent)
                .index(index)
                .depth(depth)
                .maybe_prev_sibling(previous_sibling)
                .maybe_next_sibling(next_sibling)
                .call(),
        )
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

        // Collect ancestors of both nodes into stack-allocated buffers.
        // Inline capacity `INLINE_TREE_DEPTH` = 32 matches typical
        // widget-tree depth; deeper trees spill to the heap.
        let ancestors_a: SmallVec<[I; INLINE_TREE_DEPTH]> = self.ancestors(a).collect();
        let ancestors_b: SmallVec<[I; INLINE_TREE_DEPTH]> = self.ancestors(b).collect();

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
pub trait TreeNavExt<I: TreeId>: TreeNav<I> {
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
        P: FnMut(&Self::Node) -> bool,
    {
        for child_id in self.children(parent) {
            if let Some(child_node) = self.get(child_id)
                && predicate(child_node)
            {
                return Some(child_id);
            }
        }
        None
    }

    /// Find first descendant matching a predicate.
    ///
    /// Performs depth-first search with early termination.
    fn find_descendant_where<P>(&self, root: I, mut predicate: P) -> Option<I>
    where
        P: FnMut(&Self::Node) -> bool,
    {
        for (id, _depth) in self.descendants(root) {
            if let Some(node) = self.get(id)
                && predicate(node)
            {
                return Some(id);
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
        F: FnMut(I, &Self::Node, usize),
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
        P: FnMut(&Self::Node) -> bool,
    {
        self.descendants(root)
            .filter_map(|(id, _)| self.get(id))
            .filter(|node| predicate(node))
            .count()
    }

    // `path_to_node` (returning `TreePath<I>`) was removed: `TreePath`
    // lived in `iter::path`, which had zero in-workspace consumers.
    // Callers that need a path collect one via
    // `tree.ancestors(target).collect::<Vec<_>>().iter().rev()`.

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

    // `cursor_at`, `cursor_with_history`, and `cursor_at_root` were
    // removed: `TreeCursor` lived in `iter::cursor` (1,057 LOC) and had
    // zero in-workspace consumers. Callers that need stateful
    // navigation can compose `tree.children(id)` + `tree.parent(id)`
    // directly.
}

// Blanket implementation for all TreeNav types
impl<I: TreeId, T: TreeNav<I>> TreeNavExt<I> for T {}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<I: TreeId, T: TreeNav<I> + ?Sized> TreeNav<I> for &T {
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

impl<I: TreeId, T: TreeNav<I> + ?Sized> TreeNav<I> for &mut T {
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

impl<I: TreeId, T: TreeNav<I> + ?Sized> TreeNav<I> for Box<T> {
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
    use flui_foundation::ElementId;

    use super::*;
    use crate::{
        iter::{Ancestors, DescendantsWithDepth},
        traits::TreeRead,
    };

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

            if let Some(parent_id) = parent
                && let Some(parent_node) = self.nodes.get_mut(&parent_id)
            {
                parent_node.children.push(id);
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

        // `path_to_node` was removed; a root-to-target path composes
        // from `ancestors().collect::<Vec<_>>().iter().rev()` instead.
        let mut path: Vec<_> = tree.ancestors(child1).collect();
        path.reverse();
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
