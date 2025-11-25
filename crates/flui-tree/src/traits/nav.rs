//! Tree navigation trait.
//!
//! This module provides the [`TreeNav`] trait for navigating
//! parent-child relationships in the tree.

use flui_foundation::{ElementId, Slot};

use super::TreeRead;
use crate::iter::{
    Ancestors, AncestorsWithDepth, Descendants, DescendantsWithDepth, Siblings, SiblingsDirection,
};

/// Navigation capabilities for tree structures.
///
/// This trait extends [`TreeRead`] with parent-child navigation methods.
/// It provides both direct access (parent, children) and iterator-based
/// traversal (ancestors, descendants).
///
/// # Design Rationale
///
/// Navigation is separated from read access because:
/// 1. Some trees may not store parent references
/// 2. Children storage may vary (Vec, SmallVec, etc.)
/// 3. Allows simpler implementations when navigation isn't needed
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync`. Navigation methods return
/// borrowed data, ensuring thread-safe access patterns.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{TreeNav, TreeRead};
/// use flui_foundation::ElementId;
///
/// fn find_root<T: TreeNav>(tree: &T, start: ElementId) -> ElementId {
///     tree.ancestors(start).last().unwrap_or(start)
/// }
///
/// fn tree_depth<T: TreeNav>(tree: &T, id: ElementId) -> usize {
///     tree.ancestors(id).count()
/// }
/// ```
pub trait TreeNav: TreeRead {
    /// Returns the parent of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose parent to find
    ///
    /// # Returns
    ///
    /// `Some(parent_id)` if the node has a parent, `None` if the node
    /// is a root or doesn't exist.
    fn parent(&self, id: ElementId) -> Option<ElementId>;

    /// Returns the children of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose children to find
    ///
    /// # Returns
    ///
    /// A slice of child IDs. Returns an empty slice if the node has
    /// no children or doesn't exist.
    ///
    /// # Ordering
    ///
    /// Children are returned in their natural order (typically insertion
    /// order or slot order).
    fn children(&self, id: ElementId) -> &[ElementId];

    /// Returns the slot of the given node within its parent.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose slot to find
    ///
    /// # Returns
    ///
    /// `Some(slot)` if the node has a slot assigned, `None` otherwise.
    ///
    /// # Note
    ///
    /// Default implementation returns `None`. Implementations that
    /// track slots should override this.
    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        let _ = id;
        None
    }

    /// Returns the depth of a node in the tree.
    ///
    /// The root has depth 0, its children have depth 1, etc.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose depth to calculate
    ///
    /// # Returns
    ///
    /// The depth of the node, or 0 if the node doesn't exist.
    ///
    /// # Performance
    ///
    /// O(d) where d is the depth of the node.
    #[inline]
    fn depth(&self, id: ElementId) -> usize
    where
        Self: Sized,
    {
        self.ancestors(id).count().saturating_sub(1)
    }

    /// Returns `true` if the node is a root (has no parent).
    ///
    /// # Arguments
    ///
    /// * `id` - The node to check
    #[inline]
    fn is_root(&self, id: ElementId) -> bool {
        self.parent(id).is_none() && self.contains(id)
    }

    /// Returns `true` if the node is a leaf (has no children).
    ///
    /// # Arguments
    ///
    /// * `id` - The node to check
    #[inline]
    fn is_leaf(&self, id: ElementId) -> bool {
        self.children(id).is_empty() && self.contains(id)
    }

    /// Returns the number of children of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose child count to get
    #[inline]
    fn child_count(&self, id: ElementId) -> usize {
        self.children(id).len()
    }

    /// Returns the first child of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose first child to get
    #[inline]
    fn first_child(&self, id: ElementId) -> Option<ElementId> {
        self.children(id).first().copied()
    }

    /// Returns the last child of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose last child to get
    #[inline]
    fn last_child(&self, id: ElementId) -> Option<ElementId> {
        self.children(id).last().copied()
    }

    /// Returns the child at the given index.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent node
    /// * `index` - The index of the child (0-based)
    #[inline]
    fn child_at(&self, id: ElementId, index: usize) -> Option<ElementId> {
        self.children(id).get(index).copied()
    }

    /// Returns the index of a child within its parent's children.
    ///
    /// # Arguments
    ///
    /// * `child` - The child node
    ///
    /// # Returns
    ///
    /// `Some(index)` if the child's parent is found and the child
    /// is in the parent's children list, `None` otherwise.
    #[inline]
    fn child_index(&self, child: ElementId) -> Option<usize> {
        let parent = self.parent(child)?;
        self.children(parent).iter().position(|&id| id == child)
    }

    /// Returns the next sibling of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose next sibling to find
    #[inline]
    fn next_sibling(&self, id: ElementId) -> Option<ElementId> {
        let parent = self.parent(id)?;
        let children = self.children(parent);
        let index = children.iter().position(|&child| child == id)?;
        children.get(index + 1).copied()
    }

    /// Returns the previous sibling of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The node whose previous sibling to find
    #[inline]
    fn prev_sibling(&self, id: ElementId) -> Option<ElementId> {
        let parent = self.parent(id)?;
        let children = self.children(parent);
        let index = children.iter().position(|&child| child == id)?;
        if index > 0 {
            children.get(index - 1).copied()
        } else {
            None
        }
    }

    /// Returns an iterator over siblings of the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - The starting node
    /// * `direction` - Direction to iterate (forward or backward)
    /// * `include_self` - Whether to include the starting node
    #[inline]
    fn siblings(
        &self,
        id: ElementId,
        direction: SiblingsDirection,
        include_self: bool,
    ) -> Siblings<'_, Self>
    where
        Self: Sized,
    {
        Siblings::new(self, id, direction, include_self)
    }

    /// Returns an iterator over ancestors of the given node.
    ///
    /// The iterator yields nodes from the given node up to the root
    /// (inclusive of the starting node).
    ///
    /// # Arguments
    ///
    /// * `id` - The starting node
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // For tree: root -> parent -> child
    /// let ancestors: Vec<_> = tree.ancestors(child).collect();
    /// // ancestors = [child, parent, root]
    /// ```
    #[inline]
    fn ancestors(&self, id: ElementId) -> Ancestors<'_, Self>
    where
        Self: Sized,
    {
        Ancestors::new(self, id)
    }

    /// Returns an iterator over ancestors with their depths.
    ///
    /// # Arguments
    ///
    /// * `id` - The starting node
    #[inline]
    fn ancestors_with_depth(&self, id: ElementId) -> AncestorsWithDepth<'_, Self>
    where
        Self: Sized,
    {
        AncestorsWithDepth::new(self, id)
    }

    /// Returns an iterator over descendants of the given node.
    ///
    /// Performs pre-order depth-first traversal (parent before children).
    ///
    /// # Arguments
    ///
    /// * `id` - The root of the subtree to traverse
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // For tree: root -> [child1, child2 -> grandchild]
    /// let descendants: Vec<_> = tree.descendants(root).collect();
    /// // descendants = [root, child1, child2, grandchild]
    /// ```
    #[inline]
    fn descendants(&self, id: ElementId) -> Descendants<'_, Self>
    where
        Self: Sized,
    {
        Descendants::new(self, id)
    }

    /// Returns an iterator over descendants with their depths.
    ///
    /// # Arguments
    ///
    /// * `id` - The root of the subtree to traverse
    #[inline]
    fn descendants_with_depth(&self, id: ElementId) -> DescendantsWithDepth<'_, Self>
    where
        Self: Sized,
    {
        DescendantsWithDepth::new(self, id)
    }

    /// Returns `true` if `descendant` is a descendant of `ancestor`.
    ///
    /// A node is not considered its own descendant.
    ///
    /// # Arguments
    ///
    /// * `descendant` - The potential descendant node
    /// * `ancestor` - The potential ancestor node
    ///
    /// # Performance
    ///
    /// O(d) where d is the depth of the descendant.
    #[inline]
    fn is_descendant(&self, descendant: ElementId, ancestor: ElementId) -> bool
    where
        Self: Sized,
    {
        if descendant == ancestor {
            return false;
        }
        self.ancestors(descendant)
            .skip(1) // Skip self
            .any(|id| id == ancestor)
    }

    /// Returns `true` if `ancestor` is an ancestor of `descendant`.
    ///
    /// A node is not considered its own ancestor.
    #[inline]
    fn is_ancestor(&self, ancestor: ElementId, descendant: ElementId) -> bool
    where
        Self: Sized,
    {
        self.is_descendant(descendant, ancestor)
    }

    /// Finds the lowest common ancestor of two nodes.
    ///
    /// # Arguments
    ///
    /// * `a` - First node
    /// * `b` - Second node
    ///
    /// # Returns
    ///
    /// The lowest common ancestor, or `None` if nodes are in different
    /// trees or don't exist.
    ///
    /// # Performance
    ///
    /// O(d1 + d2) where d1, d2 are the depths of the nodes.
    fn lowest_common_ancestor(&self, a: ElementId, b: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        // Collect ancestors of 'a' into a set-like structure
        // Using Vec for simplicity since depths are typically small
        let ancestors_a: Vec<_> = self.ancestors(a).collect();

        // Find first ancestor of 'b' that's in ancestors_a
        for ancestor in self.ancestors(b) {
            if ancestors_a.contains(&ancestor) {
                return Some(ancestor);
            }
        }

        None
    }

    /// Returns the root of the tree containing the given node.
    ///
    /// # Arguments
    ///
    /// * `id` - Any node in the tree
    ///
    /// # Returns
    ///
    /// The root element ID, or the node itself if it's a root.
    #[inline]
    fn root(&self, id: ElementId) -> ElementId
    where
        Self: Sized,
    {
        self.ancestors(id).last().unwrap_or(id)
    }

    /// Returns the subtree size (total descendants including self).
    ///
    /// # Arguments
    ///
    /// * `id` - The root of the subtree
    ///
    /// # Performance
    ///
    /// O(n) where n is the subtree size.
    #[inline]
    fn subtree_size(&self, id: ElementId) -> usize
    where
        Self: Sized,
    {
        self.descendants(id).count()
    }
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

impl<T: TreeNav + ?Sized> TreeNav for &T {
    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> &[ElementId] {
        (**self).children(id)
    }

    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        (**self).slot(id)
    }
}

impl<T: TreeNav + ?Sized> TreeNav for &mut T {
    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> &[ElementId] {
        (**self).children(id)
    }

    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        (**self).slot(id)
    }
}

impl<T: TreeNav + ?Sized> TreeNav for Box<T> {
    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        (**self).parent(id)
    }

    #[inline]
    fn children(&self, id: ElementId) -> &[ElementId] {
        (**self).children(id)
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

    // Test tree implementation
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        slot: Option<Slot>,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, parent: Option<ElementId>, slot: Option<Slot>) -> ElementId {
            let id = ElementId::new(self.nodes.len() as u64 + 1);
            self.nodes.push(Some(TestNode {
                parent,
                children: Vec::new(),
                slot,
            }));

            // Add as child to parent
            if let Some(parent_id) = parent {
                if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    parent_node.children.push(id);
                }
            }

            id
        }
    }

    impl TreeRead for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }
    }

    impl TreeNav for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> &[ElementId] {
            self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
        }

        fn slot(&self, id: ElementId) -> Option<Slot> {
            self.get(id)?.slot
        }
    }

    #[test]
    fn test_parent_children() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let child1 = tree.insert(Some(root), Some(Slot::new(0)));
        let child2 = tree.insert(Some(root), Some(Slot::new(1)));

        assert_eq!(tree.parent(root), None);
        assert_eq!(tree.parent(child1), Some(root));
        assert_eq!(tree.parent(child2), Some(root));

        assert_eq!(tree.children(root), &[child1, child2]);
        assert!(tree.children(child1).is_empty());
    }

    #[test]
    fn test_is_root_is_leaf() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let child = tree.insert(Some(root), None);

        assert!(tree.is_root(root));
        assert!(!tree.is_root(child));

        assert!(!tree.is_leaf(root));
        assert!(tree.is_leaf(child));
    }

    #[test]
    fn test_ancestors() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let parent = tree.insert(Some(root), None);
        let child = tree.insert(Some(parent), None);

        let ancestors: Vec<_> = tree.ancestors(child).collect();
        assert_eq!(ancestors, vec![child, parent, root]);
    }

    #[test]
    fn test_depth() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let parent = tree.insert(Some(root), None);
        let child = tree.insert(Some(parent), None);

        assert_eq!(tree.depth(root), 0);
        assert_eq!(tree.depth(parent), 1);
        assert_eq!(tree.depth(child), 2);
    }

    #[test]
    fn test_is_descendant() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let parent = tree.insert(Some(root), None);
        let child = tree.insert(Some(parent), None);

        assert!(tree.is_descendant(child, root));
        assert!(tree.is_descendant(child, parent));
        assert!(!tree.is_descendant(root, child));
        assert!(!tree.is_descendant(child, child)); // Not its own descendant
    }

    #[test]
    fn test_lowest_common_ancestor() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let left = tree.insert(Some(root), None);
        let right = tree.insert(Some(root), None);
        let left_child = tree.insert(Some(left), None);

        assert_eq!(tree.lowest_common_ancestor(left_child, right), Some(root));
        assert_eq!(tree.lowest_common_ancestor(left, right), Some(root));
        assert_eq!(tree.lowest_common_ancestor(left_child, left), Some(left));
    }

    #[test]
    fn test_siblings() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let child1 = tree.insert(Some(root), None);
        let child2 = tree.insert(Some(root), None);
        let child3 = tree.insert(Some(root), None);

        assert_eq!(tree.next_sibling(child1), Some(child2));
        assert_eq!(tree.next_sibling(child2), Some(child3));
        assert_eq!(tree.next_sibling(child3), None);

        assert_eq!(tree.prev_sibling(child1), None);
        assert_eq!(tree.prev_sibling(child2), Some(child1));
        assert_eq!(tree.prev_sibling(child3), Some(child2));
    }

    #[test]
    fn test_root() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let child = tree.insert(Some(root), None);
        let grandchild = tree.insert(Some(child), None);

        assert_eq!(tree.root(root), root);
        assert_eq!(tree.root(child), root);
        assert_eq!(tree.root(grandchild), root);
    }

    #[test]
    fn test_subtree_size() {
        let mut tree = TestTree::new();
        let root = tree.insert(None, None);
        let child1 = tree.insert(Some(root), None);
        let child2 = tree.insert(Some(root), None);
        let _grandchild = tree.insert(Some(child1), None);

        assert_eq!(tree.subtree_size(root), 4);
        assert_eq!(tree.subtree_size(child1), 2);
        assert_eq!(tree.subtree_size(child2), 1);
    }
}
