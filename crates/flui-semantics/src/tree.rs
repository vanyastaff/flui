//! SemanticsTree - Slab-based storage for semantics nodes
//!
//! This module provides the SemanticsTree struct for managing the accessibility tree.
//! It implements `TreeRead<SemanticsId>` and `TreeNav<SemanticsId>` from flui-tree,
//! enabling generic tree algorithms and visitors.

use slab::Slab;

use flui_foundation::{ElementId, SemanticsId};
use flui_tree::iter::{Ancestors, DescendantsWithDepth};
use flui_tree::{TreeNav, TreeRead};

use crate::node::SemanticsNode;

// ============================================================================
// SEMANTICS TREE
// ============================================================================

/// SemanticsTree - Slab-based storage for accessibility nodes.
///
/// This is the fifth of FLUI's five trees, corresponding to Flutter's Semantics tree
/// used for accessibility services (screen readers, voice control, etc.).
///
/// # Architecture
///
/// ```text
/// SemanticsTree
///   ├─ nodes: Slab<SemanticsNode>  (direct storage)
///   └─ root: Option<SemanticsId>
/// ```
///
/// # Thread Safety
///
/// SemanticsTree itself is not thread-safe. Use `Arc<RwLock<SemanticsTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust
/// use flui_semantics::{SemanticsTree, SemanticsNode, SemanticsProperties, SemanticsRole};
/// use flui_tree::TreeRead;
///
/// let mut tree = SemanticsTree::new();
///
/// // Insert semantics node
/// let node = SemanticsNode::new()
///     .with_properties(
///         SemanticsProperties::new()
///             .with_role(SemanticsRole::Button)
///             .with_label("Submit")
///     );
/// let id = tree.insert(node);
///
/// // Access node
/// let node = tree.get(id).unwrap();
/// assert_eq!(node.label(), Some("Submit"));
/// ```
#[derive(Debug)]
pub struct SemanticsTree {
    /// Slab storage for SemanticsNodes (0-based indexing internally)
    nodes: Slab<SemanticsNode>,

    /// Root SemanticsNode ID (None if tree is empty)
    root: Option<SemanticsId>,
}

impl SemanticsTree {
    /// Creates a new empty SemanticsTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a SemanticsTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root SemanticsNode ID.
    #[inline]
    pub fn root(&self) -> Option<SemanticsId> {
        self.root
    }

    /// Set the root SemanticsNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<SemanticsId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a SemanticsNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: SemanticsId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of SemanticsNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a SemanticsNode into the tree.
    ///
    /// Returns the SemanticsId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `SemanticsId(1)`
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_semantics::{SemanticsTree, SemanticsNode};
    ///
    /// let mut tree = SemanticsTree::new();
    /// let node = SemanticsNode::new();
    /// let id = tree.insert(node);
    /// ```
    pub fn insert(&mut self, node: SemanticsNode) -> SemanticsId {
        let slab_index = self.nodes.insert(node);
        SemanticsId::new(slab_index + 1) // +1 offset
    }

    /// Inserts a SemanticsNode with an associated ElementId.
    pub fn insert_with_element(
        &mut self,
        node: SemanticsNode,
        element_id: ElementId,
    ) -> SemanticsId {
        let node = node.with_element_id(element_id);
        self.insert(node)
    }

    /// Returns a reference to a SemanticsNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `SemanticsId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: SemanticsId) -> Option<&SemanticsNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a SemanticsNode.
    #[inline]
    pub fn get_mut(&mut self, id: SemanticsId) -> Option<&mut SemanticsNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Removes a SemanticsNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent SemanticsNode.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent SemanticsNode.
    pub fn remove_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns the parent of a node.
    pub fn parent(&self, id: SemanticsId) -> Option<SemanticsId> {
        self.get(id)?.parent()
    }

    /// Returns the children of a node.
    pub fn children(&self, id: SemanticsId) -> Option<&[SemanticsId]> {
        self.get(id).map(|node| node.children())
    }

    // ========== Dirty Tracking ==========

    /// Returns all dirty nodes in the tree.
    pub fn dirty_nodes(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.is_dirty())
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    /// Marks all nodes as clean.
    pub fn mark_all_clean(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.mark_clean();
        }
    }

    /// Returns true if any node is dirty.
    pub fn has_dirty_nodes(&self) -> bool {
        self.nodes.iter().any(|(_, node)| node.is_dirty())
    }

    // ========== Iteration ==========

    /// Returns an iterator over all SemanticsIds in the tree.
    pub fn semantics_ids(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    /// Returns an iterator over all (SemanticsId, &SemanticsNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (SemanticsId, &SemanticsNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }

    /// Returns a mutable iterator over all (SemanticsId, &mut SemanticsNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (SemanticsId, &mut SemanticsNode)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }

    // ========== Internal Access for Iterators ==========

    /// Returns a reference to the internal slab (for iterator implementations).
    #[inline]
    pub(crate) fn slab(&self) -> &Slab<SemanticsNode> {
        &self.nodes
    }
}

impl Default for SemanticsTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TREE READ IMPLEMENTATION
// ============================================================================

impl TreeRead<SemanticsId> for SemanticsTree {
    type Node = SemanticsNode;
    type NodeIter<'a> = SemanticsIdIter<'a>;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: SemanticsId) -> Option<&Self::Node> {
        SemanticsTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: SemanticsId) -> bool {
        SemanticsTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        SemanticsTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> Self::NodeIter<'_> {
        SemanticsIdIter::new(self)
    }
}

// ============================================================================
// TREE NAV IMPLEMENTATION
// ============================================================================

impl TreeNav<SemanticsId> for SemanticsTree {
    type ChildrenIter<'a> = ChildrenIter<'a>;
    type AncestorsIter<'a> = Ancestors<'a, SemanticsId, Self>;
    type DescendantsIter<'a> = DescendantsWithDepth<'a, SemanticsId, Self>;
    type SiblingsIter<'a> = SiblingsIter<'a>;

    const MAX_DEPTH: usize = 64; // Accessibility trees can be deeper than layer trees
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: SemanticsId) -> Option<SemanticsId> {
        SemanticsTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: SemanticsId) -> Self::ChildrenIter<'_> {
        ChildrenIter::new(self, id)
    }

    #[inline]
    fn ancestors(&self, start: SemanticsId) -> Self::AncestorsIter<'_> {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: SemanticsId) -> Self::DescendantsIter<'_> {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: SemanticsId) -> Self::SiblingsIter<'_> {
        SiblingsIter::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: SemanticsId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: SemanticsId) -> bool {
        self.get(id)
            .map(|node| !node.children().is_empty())
            .unwrap_or(false)
    }
}

// ============================================================================
// CUSTOM ITERATORS
// ============================================================================

/// Iterator over all SemanticsIds in the tree.
pub struct SemanticsIdIter<'a> {
    inner: slab::Iter<'a, SemanticsNode>,
}

impl<'a> SemanticsIdIter<'a> {
    fn new(tree: &'a SemanticsTree) -> Self {
        Self {
            inner: tree.slab().iter(),
        }
    }
}

impl<'a> Iterator for SemanticsIdIter<'a> {
    type Item = SemanticsId;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for SemanticsIdIter<'_> {}

/// Iterator over children of a semantics node.
pub struct ChildrenIter<'a> {
    children: Option<&'a [SemanticsId]>,
    index: usize,
}

impl<'a> ChildrenIter<'a> {
    fn new(tree: &'a SemanticsTree, id: SemanticsId) -> Self {
        Self {
            children: tree.children(id),
            index: 0,
        }
    }
}

impl<'a> Iterator for ChildrenIter<'a> {
    type Item = SemanticsId;

    fn next(&mut self) -> Option<Self::Item> {
        let children = self.children?;
        if self.index < children.len() {
            let id = children[self.index];
            self.index += 1;
            Some(id)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .children
            .map(|c| c.len().saturating_sub(self.index))
            .unwrap_or(0);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ChildrenIter<'_> {}

/// Iterator over siblings of a semantics node.
pub struct SiblingsIter<'a> {
    children: Option<&'a [SemanticsId]>,
    index: usize,
    exclude_id: SemanticsId,
}

impl<'a> SiblingsIter<'a> {
    fn new(tree: &'a SemanticsTree, id: SemanticsId) -> Self {
        let children = tree
            .parent(id)
            .and_then(|parent_id| tree.children(parent_id));

        Self {
            children,
            index: 0,
            exclude_id: id,
        }
    }
}

impl<'a> Iterator for SiblingsIter<'a> {
    type Item = SemanticsId;

    fn next(&mut self) -> Option<Self::Item> {
        let children = self.children?;
        while self.index < children.len() {
            let id = children[self.index];
            self.index += 1;
            if id != self.exclude_id {
                return Some(id);
            }
        }
        None
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::semantics::SemanticsProperties;

    #[test]
    fn test_semantics_tree_new() {
        let tree = SemanticsTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_semantics_tree_with_capacity() {
        let tree = SemanticsTree::with_capacity(100);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_semantics_tree_insert() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert_eq!(id.get(), 1); // First ID should be 1
    }

    #[test]
    fn test_semantics_tree_get() {
        let mut tree = SemanticsTree::new();
        let node =
            SemanticsNode::new().with_properties(SemanticsProperties::new().with_label("Test"));
        let id = tree.insert(node);

        let node = tree.get(id);
        assert!(node.is_some());
        assert_eq!(node.unwrap().label(), Some("Test"));
    }

    #[test]
    fn test_semantics_tree_get_mut() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        if let Some(node) = tree.get_mut(id) {
            node.set_properties(SemanticsProperties::new().with_label("Modified"));
        }

        assert_eq!(tree.get(id).unwrap().label(), Some("Modified"));
    }

    #[test]
    fn test_semantics_tree_remove() {
        let mut tree = SemanticsTree::new();
        let node = SemanticsNode::new();
        let id = tree.insert(node);

        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
        assert!(tree.is_empty());
    }

    #[test]
    fn test_semantics_tree_parent_child() {
        let mut tree = SemanticsTree::new();

        let parent_node = SemanticsNode::new();
        let child_node = SemanticsNode::new();

        let parent_id = tree.insert(parent_node);
        let child_id = tree.insert(child_node);

        tree.add_child(parent_id, child_id);

        // Check parent has child
        let children = tree.children(parent_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child_id);

        // Check child has parent
        let parent = tree.parent(child_id);
        assert_eq!(parent, Some(parent_id));
    }

    #[test]
    fn test_semantics_tree_remove_child() {
        let mut tree = SemanticsTree::new();

        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 1);

        tree.remove_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 0);
        assert!(tree.parent(child_id).is_none());
    }

    #[test]
    fn test_semantics_tree_set_root() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        assert!(tree.root().is_none());
        tree.set_root(Some(id));
        assert_eq!(tree.root(), Some(id));
    }

    #[test]
    fn test_semantics_tree_clear() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());
        tree.set_root(Some(id));

        tree.clear();
        assert!(tree.is_empty());
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_semantics_tree_iter() {
        let mut tree = SemanticsTree::new();
        let id1 = tree.insert(SemanticsNode::new());
        let id2 = tree.insert(SemanticsNode::new());

        let ids: Vec<_> = tree.semantics_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_semantics_tree_dirty_tracking() {
        let mut tree = SemanticsTree::new();

        let id1 = tree.insert(SemanticsNode::new()); // dirty by default
        let id2 = tree.insert(SemanticsNode::new());

        // All nodes start dirty
        assert!(tree.has_dirty_nodes());
        let dirty: Vec<_> = tree.dirty_nodes().collect();
        assert_eq!(dirty.len(), 2);

        // Mark all clean
        tree.mark_all_clean();
        assert!(!tree.has_dirty_nodes());
        assert_eq!(tree.dirty_nodes().count(), 0);

        // Mark one dirty again
        if let Some(node) = tree.get_mut(id1) {
            node.mark_dirty();
        }
        assert!(tree.has_dirty_nodes());
        let dirty: Vec<_> = tree.dirty_nodes().collect();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0], id1);
    }

    // ========== TreeRead Trait Tests ==========

    #[test]
    fn test_tree_read_get() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        let node: Option<&SemanticsNode> = TreeRead::get(&tree, id);
        assert!(node.is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(SemanticsNode::new());

        assert!(TreeRead::contains(&tree, id));
        assert!(!TreeRead::contains(&tree, SemanticsId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = SemanticsTree::new();
        assert_eq!(TreeRead::<SemanticsId>::len(&tree), 0);

        let _ = tree.insert(SemanticsNode::new());
        assert_eq!(TreeRead::<SemanticsId>::len(&tree), 1);

        let _ = tree.insert(SemanticsNode::new());
        assert_eq!(TreeRead::<SemanticsId>::len(&tree), 2);
    }

    #[test]
    fn test_tree_read_node_ids() {
        let mut tree = SemanticsTree::new();
        let id1 = tree.insert(SemanticsNode::new());
        let id2 = tree.insert(SemanticsNode::new());

        let ids: Vec<_> = TreeRead::<SemanticsId>::node_ids(&tree).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    // ========== TreeNav Trait Tests ==========

    #[test]
    fn test_tree_nav_parent() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child_id);

        assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
        assert_eq!(TreeNav::parent(&tree, parent_id), None);
    }

    #[test]
    fn test_tree_nav_children() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child1_id = tree.insert(SemanticsNode::new());
        let child2_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);

        let children: Vec<_> = TreeNav::children(&tree, parent_id).collect();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1_id));
        assert!(children.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_ancestors() {
        let mut tree = SemanticsTree::new();
        let root_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());
        let grandchild_id = tree.insert(SemanticsNode::new());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);
    }

    #[test]
    fn test_tree_nav_descendants() {
        let mut tree = SemanticsTree::new();
        let root_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());
        let grandchild_id = tree.insert(SemanticsNode::new());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let descendants: Vec<_> = TreeNav::descendants(&tree, root_id).collect();
        assert_eq!(descendants.len(), 3);
        assert_eq!(descendants[0], (root_id, 0));
        assert_eq!(descendants[1], (child_id, 1));
        assert_eq!(descendants[2], (grandchild_id, 2));
    }

    #[test]
    fn test_tree_nav_siblings() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child1_id = tree.insert(SemanticsNode::new());
        let child2_id = tree.insert(SemanticsNode::new());
        let child3_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);
        tree.add_child(parent_id, child3_id);

        let siblings: Vec<_> = TreeNav::siblings(&tree, child2_id).collect();
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&child1_id));
        assert!(siblings.contains(&child3_id));
        assert!(!siblings.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_child_count() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child1_id = tree.insert(SemanticsNode::new());
        let child2_id = tree.insert(SemanticsNode::new());

        assert_eq!(TreeNav::child_count(&tree, parent_id), 0);

        tree.add_child(parent_id, child1_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 1);

        tree.add_child(parent_id, child2_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 2);
    }

    #[test]
    fn test_tree_nav_has_children() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        assert!(!TreeNav::has_children(&tree, parent_id));

        tree.add_child(parent_id, child_id);
        assert!(TreeNav::has_children(&tree, parent_id));
    }

    #[test]
    fn test_tree_nav_is_leaf() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        assert!(TreeNav::is_leaf(&tree, parent_id));

        tree.add_child(parent_id, child_id);
        assert!(!TreeNav::is_leaf(&tree, parent_id));
        assert!(TreeNav::is_leaf(&tree, child_id));
    }

    #[test]
    fn test_tree_nav_is_root() {
        let mut tree = SemanticsTree::new();
        let parent_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());

        tree.add_child(parent_id, child_id);

        assert!(TreeNav::is_root(&tree, parent_id));
        assert!(!TreeNav::is_root(&tree, child_id));
    }

    #[test]
    fn test_tree_nav_find_root() {
        let mut tree = SemanticsTree::new();
        let root_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());
        let grandchild_id = tree.insert(SemanticsNode::new());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::find_root(&tree, grandchild_id), root_id);
        assert_eq!(TreeNav::find_root(&tree, child_id), root_id);
        assert_eq!(TreeNav::find_root(&tree, root_id), root_id);
    }

    #[test]
    fn test_tree_nav_depth() {
        let mut tree = SemanticsTree::new();
        let root_id = tree.insert(SemanticsNode::new());
        let child_id = tree.insert(SemanticsNode::new());
        let grandchild_id = tree.insert(SemanticsNode::new());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::depth(&tree, root_id), 0);
        assert_eq!(TreeNav::depth(&tree, child_id), 1);
        assert_eq!(TreeNav::depth(&tree, grandchild_id), 2);
    }
}
