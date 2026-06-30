//! SemanticsTree - Slab-based storage for semantics nodes
//!
//! This module provides the SemanticsTree struct for managing the accessibility
//! tree. It implements `TreeRead<SemanticsId>` and `TreeNav<SemanticsId>` from
//! flui-tree, enabling generic tree algorithms and visitors.

use flui_foundation::{ElementId, SemanticsId};
use flui_tree::{
    TreeNav, TreeRead, TreeWrite,
    iter::{Ancestors, DescendantsWithDepth},
};
use slab::Slab;

use crate::node::SemanticsNode;

// ============================================================================
// SEMANTICS TREE
// ============================================================================

/// SemanticsTree - Slab-based storage for accessibility nodes.
///
/// This is the fifth of FLUI's five trees, corresponding to Flutter's Semantics
/// tree used for accessibility services (screen readers, voice control, etc.).
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
/// use flui_semantics::{SemanticsNode, SemanticsTree};
/// use flui_tree::TreeRead;
///
/// let mut tree = SemanticsTree::new();
///
/// // Insert semantics node
/// let mut node = SemanticsNode::new();
/// node.config_mut().set_label("Submit");
/// node.config_mut().set_button(true);
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
    /// use flui_semantics::{SemanticsNode, SemanticsTree};
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

    // NOTE (cycle 3 T-2): the cycle 2 inherent `pub fn remove` was
    // deleted in favour of [`flui_tree::TreeWrite::remove`] (the trait's
    // default cascade impl). The behaviour is identical — post-order
    // cascade via `children()` walks, parent unlink via
    // `remove_shallow`, root reset.
    //
    // Callers go through the trait method now:
    //
    // ```rust
    // use flui_tree::TreeWrite;
    // let _ = tree.remove(id);   // cascade
    // ```
    //
    // The inherent `remove_shallow` is the trait primitive and covers
    // the reparenting opt-out.

    /// Removes a single SemanticsNode from the tree **without**
    /// cascading to descendants. Descendants are orphaned in storage
    /// (their `parent` pointers still reference the now-deleted slot —
    /// use only when the caller will re-attach or drop them
    /// immediately).
    ///
    /// **Cycle 3 T-1 contract change**: the parent's children vector
    /// IS now drained of `id` before the node is dropped. Pre-cycle
    /// this method intentionally left the parent's children vec
    /// pointing at a stale id, expecting the caller to handle
    /// parent-cleanup; the audit found zero production callers actually
    /// exercising that escape-hatch.
    pub fn remove_shallow(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        if !self.contains(id) {
            return None;
        }
        // Unlink from parent's children vec — matches the trait
        // contract.
        if let Some(parent_id) = self.get(id).and_then(SemanticsNode::parent)
            && let Some(parent) = self.get_mut(parent_id)
        {
            parent.remove_child(id);
        }
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

    /// Adds `child_id` as a child of `parent_id`.
    ///
    /// **Auto-detach semantics (U11)** — if `child_id` is currently
    /// attached to a different parent, it is removed from that parent's
    /// children vector first. Re-attaching to the same parent is a
    /// short-circuit no-op (`SemanticsNode::add_child` carries the
    /// containment dedup so the children vector never holds a duplicate
    /// id). Mirrors the layer-side guarantee that [`LayerTree::add_child`]
    /// provides and matches Flutter `semantics.dart` `_SemanticsTreeWalker`
    /// reparent semantics.
    ///
    /// Missing-id lookups (either `parent_id` or `child_id` not in the
    /// tree) are silent no-ops.
    ///
    /// [`LayerTree::add_child`]: ../../flui-layer/src/tree/layer_tree.rs
    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        // Both endpoints must exist — otherwise the call is a no-op.
        if !self.contains(parent_id) || !self.contains(child_id) {
            return;
        }

        // Reject self-attachment outright — `parent_id == child_id` is a
        // 1-cycle, the smallest possible.
        if parent_id == child_id {
            tracing::warn!(
                ?parent_id,
                "SemanticsTree::add_child rejected self-link (cycle)"
            );
            return;
        }

        // Reject attaching an ancestor of `parent_id` under it (would
        // create an N-cycle). The cascading `remove` (U13) would follow
        // such a cycle to unbounded recursion + stack overflow; this
        // guard makes cycles impossible to enter via the public API.
        if self.is_ancestor_of(child_id, parent_id) {
            tracing::warn!(
                ?parent_id,
                ?child_id,
                "SemanticsTree::add_child rejected cycle \
                 (child is ancestor of parent)"
            );
            return;
        }

        // 1. Detach from previous parent if one exists and differs.
        let prev_parent = self.get(child_id).and_then(SemanticsNode::parent);
        if let Some(prev) = prev_parent {
            if prev == parent_id {
                // Already attached to this parent — short-circuit. The
                // node-level dedup in SemanticsNode::add_child would
                // catch a double-add, but bailing here avoids the
                // redundant mutation + dirty-bit ripple.
                return;
            }
            if let Some(prev_node) = self.get_mut(prev) {
                prev_node.remove_child(child_id);
            }
        }

        // 2. Attach to new parent. `SemanticsNode::add_child` already
        //    has the containment dedup (node.rs).
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // 3. Update child's parent pointer.
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Returns `true` if `candidate_ancestor` is an ancestor of `descendant`.
    ///
    /// Walk is bounded by the tree's slab size so a malformed parent
    /// pointer cycle (which `add_child` no longer permits to be created)
    /// can not hang the check.
    fn is_ancestor_of(&self, candidate_ancestor: SemanticsId, descendant: SemanticsId) -> bool {
        let mut current = Some(descendant);
        let mut steps = 0;
        let max_steps = self.nodes.len() + 1;
        while let Some(id) = current {
            if id == candidate_ancestor {
                return true;
            }
            steps += 1;
            if steps > max_steps {
                tracing::warn!(
                    "SemanticsTree::is_ancestor_of: walk exceeded slab \
                     size — malformed parent pointers?"
                );
                return false;
            }
            current = self.get(id).and_then(SemanticsNode::parent);
        }
        false
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
        self.get(id).map(SemanticsNode::children)
    }

    // ========== Dirty Tracking ==========

    /// Returns all dirty node ids in the tree.
    pub fn dirty_nodes(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.is_dirty())
            .map(|(index, _)| SemanticsId::new(index + 1))
    }

    /// Returns `(id, &SemanticsNode)` pairs for every dirty node.
    ///
    /// Lets callers (notably [`crate::owner::SemanticsOwner::flush`]) walk
    /// dirty nodes in a single pass without an intermediate
    /// `Vec<SemanticsId>` collect that would force a per-frame heap
    /// allocation when there is any dirt.
    pub fn iter_dirty(&self) -> impl Iterator<Item = (SemanticsId, &SemanticsNode)> + '_ {
        self.nodes
            .iter()
            .filter(|(_, node)| node.is_dirty())
            .map(|(index, node)| (SemanticsId::new(index + 1), node))
    }

    /// Marks all nodes as clean.
    pub fn mark_all_clean(&mut self) {
        for (_, node) in &mut self.nodes {
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

    /// Returns a mutable iterator over all (SemanticsId, &mut SemanticsNode)
    /// pairs.
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
    fn node_ids(&self) -> impl Iterator<Item = SemanticsId> + '_ {
        self.nodes
            .iter()
            .map(|(index, _)| SemanticsId::new(index + 1))
    }
}

// ============================================================================
// TREE NAV IMPLEMENTATION
// ============================================================================

impl TreeNav<SemanticsId> for SemanticsTree {
    const MAX_DEPTH: usize = 64; // Accessibility trees can be deeper than layer trees
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: SemanticsId) -> Option<SemanticsId> {
        SemanticsTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: SemanticsId) -> impl Iterator<Item = SemanticsId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    #[inline]
    fn ancestors(&self, start: SemanticsId) -> impl Iterator<Item = SemanticsId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: SemanticsId) -> impl Iterator<Item = (SemanticsId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: SemanticsId) -> impl Iterator<Item = SemanticsId> + '_ {
        let parent = self.parent(id);
        parent.into_iter().flat_map(move |p| {
            self.get(p)
                .map(|node| node.children().iter().copied())
                .into_iter()
                .flatten()
                .filter(move |&c| c != id)
        })
    }

    #[inline]
    fn child_count(&self, id: SemanticsId) -> usize {
        self.get(id).map_or(0, |node| node.children().len())
    }

    #[inline]
    fn has_children(&self, id: SemanticsId) -> bool {
        self.get(id).is_some_and(|node| !node.children().is_empty())
    }
}

// ============================================================================
// TREE WRITE IMPLEMENTATION (cycle 3 T-2)
// ============================================================================
//
// Hoists the cycle 2 cascade-by-default `remove` from the inherent API
// up to the unified [`TreeWrite`] trait per memory
// `flui-tree-unified-interface-intent`. Callers now write
// `use flui_tree::TreeWrite; tree.remove(id);` and get cascade
// automatically. The inherent `SemanticsTree::remove_shallow` is the
// trait primitive; the trait default `remove` walks descendants and
// calls `remove_shallow`.

impl TreeWrite<SemanticsId> for SemanticsTree {
    #[inline]
    fn get_mut(&mut self, id: SemanticsId) -> Option<&mut Self::Node> {
        SemanticsTree::get_mut(self, id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> SemanticsId {
        SemanticsTree::insert(self, node)
    }

    #[inline]
    fn remove_shallow(&mut self, id: SemanticsId) -> Option<Self::Node> {
        SemanticsTree::remove_shallow(self, id)
    }

    #[inline]
    fn clear(&mut self) {
        SemanticsTree::clear(self);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    // Cycle 3 T-2: `tree.remove(id)` resolves through the trait now.
    use flui_tree::TreeWrite;

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
        let mut node = SemanticsNode::new();
        node.config_mut().set_label("Test");
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
            node.config_mut().set_label("Modified");
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

// ============================================================================
// SLAB-TREE HYGIENE TESTS (U11 — add_child auto-detach + U13 — remove cascade)
// ============================================================================

#[cfg(test)]
mod slab_hygiene_tests {
    use crate::node::SemanticsNode;
    use crate::tree::SemanticsTree;
    use flui_foundation::SemanticsId;
    // Cycle 3 T-2: `tree.remove(id)` now resolves through the trait.
    use flui_tree::TreeWrite;

    fn empty_node() -> SemanticsNode {
        SemanticsNode::new()
    }

    // ----- U11 add_child auto-detach -----

    #[test]
    fn add_child_attaches_under_new_parent() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent));
        assert_eq!(tree.get(parent).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_auto_detaches_from_previous_parent() {
        let mut tree = SemanticsTree::new();
        let parent_a = tree.insert(empty_node());
        let parent_b = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent_a, child);
        tree.add_child(parent_b, child);

        assert_eq!(tree.get(child).unwrap().parent(), Some(parent_b));
        assert!(tree.get(parent_a).unwrap().children().is_empty());
        assert_eq!(tree.get(parent_b).unwrap().children(), &[child]);
    }

    #[test]
    fn add_child_same_parent_is_idempotent() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let child = tree.insert(empty_node());

        tree.add_child(parent, child);
        tree.add_child(parent, child);

        assert_eq!(tree.get(parent).unwrap().children().len(), 1);
    }

    #[test]
    fn add_child_missing_parent_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let child = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        tree.add_child(phantom, child);
        assert!(tree.get(child).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_missing_child_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let parent = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        tree.add_child(parent, phantom);
        assert!(tree.get(parent).unwrap().children().is_empty());
    }

    // ----- PR #100 followup: cycle rejection -----

    #[test]
    fn add_child_rejects_self_link() {
        let mut tree = SemanticsTree::new();
        let id = tree.insert(empty_node());
        tree.add_child(id, id);
        assert!(tree.get(id).unwrap().children().is_empty());
        assert!(tree.get(id).unwrap().parent().is_none());
    }

    #[test]
    fn add_child_rejects_attaching_ancestor_under_descendant() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        // Would create a 3-cycle: root → mid → leaf → root.
        // Pre-rejection, `tree.remove(root)` would have recursed
        // root → mid → leaf → root → … indefinitely.
        tree.add_child(leaf, root);

        // Tree shape unchanged after rejected call.
        assert_eq!(tree.get(root).unwrap().parent(), None);
        let empty: &[SemanticsId] = &[];
        assert_eq!(tree.get(leaf).unwrap().children(), empty);
        // Cascade terminates.
        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0);
    }

    // ----- U13 remove cascade + remove_shallow -----

    #[test]
    fn remove_cascades_to_descendants() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);
        assert_eq!(tree.len(), 3);

        let removed = tree.remove(root);
        assert!(removed.is_some());
        assert_eq!(tree.len(), 0);
        assert!(!tree.contains(mid));
        assert!(!tree.contains(leaf));
    }

    #[test]
    fn remove_unlinks_parent_children_vector() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let sibling = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(root, sibling);

        let _ = tree.remove(mid);
        assert!(!tree.contains(mid));
        assert_eq!(tree.get(root).unwrap().children(), &[sibling]);
    }

    #[test]
    fn remove_resets_root_when_removing_root() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        tree.set_root(Some(root));
        let _ = tree.remove(root);
        assert_eq!(tree.root(), None);
    }

    #[test]
    fn remove_of_phantom_id_is_a_no_op() {
        let mut tree = SemanticsTree::new();
        let _ = tree.insert(empty_node());
        let phantom = SemanticsId::new(999);
        assert!(tree.remove(phantom).is_none());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn remove_shallow_does_not_cascade() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(empty_node());
        let mid = tree.insert(empty_node());
        let leaf = tree.insert(empty_node());
        tree.add_child(root, mid);
        tree.add_child(mid, leaf);

        let _ = tree.remove_shallow(mid);
        assert!(!tree.contains(mid));
        // Leaf survives (only cascade path drops descendants).
        assert!(tree.contains(leaf));
    }
}
