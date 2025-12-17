//! RenderTree - Slab-based render object storage.
//!
//! This module provides efficient storage and tree operations for render objects.
//! Implements `flui-tree` traits for unified tree interface.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_tree::iter::{AllSiblings, Ancestors, DescendantsWithDepth};
use flui_tree::traits::{TreeNav, TreeRead, TreeWrite};
use parking_lot::RwLock;
use slab::Slab;

use crate::pipeline::PipelineOwner;
use crate::traits::RenderObject;

// ============================================================================
// RenderNode
// ============================================================================

/// Internal node wrapper for slab storage.
///
/// Stores a render object along with its tree relationships.
/// The tree structure is managed externally (parent/children IDs) rather
/// than via pointers on the render objects themselves.
#[derive(Debug)]
pub struct RenderNode {
    /// The render object (boxed for trait object storage).
    render_object: Box<dyn RenderObject>,

    /// Parent node ID (None for root).
    parent: Option<RenderId>,

    /// Child node IDs.
    children: Vec<RenderId>,

    /// Depth in the tree (root = 0).
    depth: u16,
}

impl RenderNode {
    /// Creates a new render node.
    #[inline]
    pub fn new(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            render_object,
            parent: None,
            children: Vec::new(),
            depth: 0,
        }
    }

    /// Creates a new render node with a parent.
    #[inline]
    pub fn with_parent(render_object: Box<dyn RenderObject>, parent: RenderId, depth: u16) -> Self {
        Self {
            render_object,
            parent: Some(parent),
            children: Vec::new(),
            depth,
        }
    }

    /// Returns a reference to the render object.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Returns a mutable reference to the render object.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    /// Returns the parent ID.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    /// Sets the parent ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    /// Returns the children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        &self.children
    }

    /// Adds a child ID.
    #[inline]
    pub fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    /// Removes a child ID.
    ///
    /// Returns `true` if the child was found and removed.
    pub fn remove_child(&mut self, child: RenderId) -> bool {
        if let Some(pos) = self.children.iter().position(|&id| id == child) {
            let _ = self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns the depth.
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth as usize
    }

    /// Sets the depth.
    #[inline]
    pub fn set_depth(&mut self, depth: usize) {
        debug_assert!(depth <= u16::MAX as usize, "Tree depth exceeds u16::MAX");
        self.depth = depth as u16;
    }

    /// Consumes the node and returns the render object.
    #[inline]
    pub fn into_render_object(self) -> Box<dyn RenderObject> {
        self.render_object
    }
}

// ============================================================================
// RenderTree
// ============================================================================

/// Slab-based storage for render objects.
///
/// Provides O(1) render object access by RenderId and tree navigation operations.
///
/// # Thread Safety
///
/// RenderTree itself is `Send + Sync`. For multi-threaded access, wrap in
/// `Arc<RwLock<RenderTree>>`.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::tree::RenderTree;
/// use flui_rendering::objects::RenderColoredBox;
///
/// let mut tree = RenderTree::new();
///
/// // Insert root
/// let root_id = tree.insert(Box::new(RenderColoredBox::new(Color::RED)));
/// tree.set_root(Some(root_id));
///
/// // Insert child
/// let child_id = tree.insert_child(root_id, Box::new(RenderColoredBox::new(Color::BLUE)));
///
/// // Access render object
/// if let Some(node) = tree.get(root_id) {
///     println!("Root has {} children", node.children().len());
/// }
/// ```
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for nodes (0-based indexing internally).
    nodes: Slab<RenderNode>,

    /// Root node ID (None if tree is empty).
    root: Option<RenderId>,

    /// Pipeline owner for dirty scheduling (optional).
    owner: Option<Arc<RwLock<PipelineOwner>>>,
}

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            owner: None,
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            owner: None,
        }
    }

    // ========================================================================
    // Pipeline Owner
    // ========================================================================

    /// Returns the pipeline owner.
    #[inline]
    pub fn owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        self.owner.as_ref()
    }

    /// Sets the pipeline owner.
    ///
    /// This will attach all existing nodes to the new owner.
    pub fn set_owner(&mut self, owner: Option<Arc<RwLock<PipelineOwner>>>) {
        self.owner = owner;
        // TODO: Attach/detach existing nodes when owner changes
    }

    // ========================================================================
    // Root Management
    // ========================================================================

    /// Returns the root node ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Sets the root node ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========================================================================
    // Basic Operations
    // ========================================================================

    /// Checks if a node exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of nodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns a reference to a node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `RenderId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a node.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Returns a reference to the render object.
    #[inline]
    pub fn render_object(&self, id: RenderId) -> Option<&dyn RenderObject> {
        self.get(id).map(|node| node.render_object())
    }

    /// Returns a mutable reference to the render object.
    #[inline]
    pub fn render_object_mut(&mut self, id: RenderId) -> Option<&mut dyn RenderObject> {
        self.get_mut(id).map(|node| node.render_object_mut())
    }

    /// Inserts a render object into the tree (no parent).
    ///
    /// Returns the RenderId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `RenderId(1)`
    pub fn insert(&mut self, render_object: Box<dyn RenderObject>) -> RenderId {
        let node = RenderNode::new(render_object);
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1) // 0-based → 1-based
    }

    /// Inserts a render object as a child of the given parent.
    ///
    /// Returns the RenderId of the inserted child.
    pub fn insert_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject>,
    ) -> Option<RenderId> {
        // Get parent depth
        let parent_depth = self.get(parent_id)?.depth();

        // Create child node
        let child_node =
            RenderNode::with_parent(render_object, parent_id, (parent_depth + 1) as u16);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = RenderId::new(child_slab_index + 1);

        // Add child to parent
        if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Removes a node from the tree.
    ///
    /// Returns the removed render object, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Use `remove_recursive` for that.
    pub fn remove(&mut self, id: RenderId) -> Option<Box<dyn RenderObject>> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(|n| n.parent()) {
            if let Some(parent) = self.get_mut(parent_id) {
                parent.remove_child(id);
            }
        }

        self.nodes
            .try_remove(id.get() - 1)
            .map(|node| node.into_render_object())
    }

    /// Removes a node and all its descendants recursively.
    ///
    /// Returns the number of nodes removed.
    pub fn remove_recursive(&mut self, id: RenderId) -> usize {
        let mut count = 0;

        // Get children first (clone to avoid borrow issues)
        let children: Vec<RenderId> = self
            .get(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();

        // Remove children recursively
        for child_id in children {
            count += self.remove_recursive(child_id);
        }

        // Remove the node itself
        if self.remove(id).is_some() {
            count += 1;
        }

        count
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for additional nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========================================================================
    // Tree Navigation
    // ========================================================================

    /// Returns the parent ID of a node.
    #[inline]
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.get(id)?.parent()
    }

    /// Returns the children IDs of a node.
    #[inline]
    pub fn children(&self, id: RenderId) -> &[RenderId] {
        self.get(id).map(|n| n.children()).unwrap_or(&[])
    }

    /// Returns the depth of a node in the tree.
    #[inline]
    pub fn depth(&self, id: RenderId) -> Option<usize> {
        self.get(id).map(|n| n.depth())
    }

    /// Checks if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: RenderId, descendant: RenderId) -> bool {
        let mut current = self.parent(descendant);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.parent(id);
        }
        false
    }

    /// Checks if `descendant` is a descendant of `ancestor`.
    #[inline]
    pub fn is_descendant(&self, descendant: RenderId, ancestor: RenderId) -> bool {
        self.is_ancestor(ancestor, descendant)
    }

    /// Returns the path from root to the given node.
    ///
    /// The path includes the node itself.
    pub fn path_to_root(&self, id: RenderId) -> Vec<RenderId> {
        let mut path = Vec::new();
        let mut current = Some(id);

        while let Some(node_id) = current {
            path.push(node_id);
            current = self.parent(node_id);
        }

        path.reverse();
        path
    }

    // ========================================================================
    // Dirty Node Collection
    // ========================================================================

    /// Collects all nodes that need layout, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_layout() == true`, sorted by depth
    /// (shallow first) for correct layout order.
    pub fn collect_nodes_needing_layout(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.render_object().base().needs_layout())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth()))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    /// Collects all nodes that need paint, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_paint() == true`, sorted by depth
    /// (shallow first) for correct paint order.
    pub fn collect_nodes_needing_paint(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.render_object().base().needs_paint())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth()))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    // ========================================================================
    // Iteration
    // ========================================================================

    /// Returns an iterator over all node IDs.
    pub fn ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes.iter().map(|(idx, _)| RenderId::new(idx + 1))
    }

    /// Returns an iterator over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &RenderNode> + '_ {
        self.nodes.iter().map(|(_, node)| node)
    }

    /// Returns a mutable iterator over all nodes.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut RenderNode> + '_ {
        self.nodes.iter_mut().map(|(_, node)| node)
    }

    /// Returns an iterator over (RenderId, &RenderNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (RenderId, &RenderNode)> + '_ {
        self.nodes
            .iter()
            .map(|(idx, node)| (RenderId::new(idx + 1), node))
    }

    /// Returns a mutable iterator over (RenderId, &mut RenderNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (RenderId, &mut RenderNode)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(idx, node)| (RenderId::new(idx + 1), node))
    }

    // ========================================================================
    // Depth-First Traversal
    // ========================================================================

    /// Visits all nodes in depth-first pre-order starting from root.
    ///
    /// The callback receives (RenderId, &RenderNode) for each node.
    pub fn visit_depth_first<F>(&self, mut f: F)
    where
        F: FnMut(RenderId, &RenderNode),
    {
        if let Some(root_id) = self.root {
            self.visit_depth_first_from(root_id, &mut f);
        }
    }

    /// Visits all nodes in depth-first pre-order starting from a given node.
    fn visit_depth_first_from<F>(&self, id: RenderId, f: &mut F)
    where
        F: FnMut(RenderId, &RenderNode),
    {
        if let Some(node) = self.get(id) {
            f(id, node);

            // Clone children to avoid borrow issues
            let children = node.children().to_vec();
            for child_id in children {
                self.visit_depth_first_from(child_id, f);
            }
        }
    }

    /// Visits all nodes mutably in depth-first pre-order starting from root.
    ///
    /// **Note:** The callback receives only RenderId since we can't provide
    /// mutable references during traversal. Use `get_mut()` inside the callback.
    pub fn visit_depth_first_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Self, RenderId),
    {
        if let Some(root_id) = self.root {
            self.visit_depth_first_mut_from(root_id, &mut f);
        }
    }

    /// Visits all nodes mutably in depth-first pre-order starting from a given node.
    fn visit_depth_first_mut_from<F>(&mut self, id: RenderId, f: &mut F)
    where
        F: FnMut(&mut Self, RenderId),
    {
        // Get children first (clone to avoid borrow issues)
        let children: Vec<RenderId> = self
            .get(id)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();

        // Visit this node
        f(self, id);

        // Visit children
        for child_id in children {
            self.visit_depth_first_mut_from(child_id, f);
        }
    }
}

// Safety: RenderTree is Send + Sync because:
// - Slab<RenderNode> is Send + Sync when RenderNode is
// - RenderNode contains Box<dyn RenderObject> which is Send + Sync
// - Option<RenderId> and Option<Arc<RwLock<PipelineOwner>>> are Send + Sync
unsafe impl Send for RenderTree {}
unsafe impl Sync for RenderTree {}

// ============================================================================
// flui-tree Trait Implementations
// ============================================================================

impl TreeRead<RenderId> for RenderTree {
    type Node = RenderNode;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: RenderId) -> Option<&Self::Node> {
        RenderTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: RenderId) -> bool {
        RenderTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        RenderTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes.iter().map(|(idx, _)| RenderId::new(idx + 1))
    }
}

impl TreeWrite<RenderId> for RenderTree {
    #[inline]
    fn get_mut(&mut self, id: RenderId) -> Option<&mut Self::Node> {
        RenderTree::get_mut(self, id)
    }

    fn insert(&mut self, node: Self::Node) -> RenderId {
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    fn remove(&mut self, id: RenderId) -> Option<Self::Node> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(|n| n.parent()) {
            if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
                parent.remove_child(id);
            }
        }

        self.nodes.try_remove(id.get() - 1)
    }

    #[inline]
    fn clear(&mut self) {
        RenderTree::clear(self);
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        RenderTree::reserve(self, additional);
    }
}

impl TreeNav<RenderId> for RenderTree {
    const MAX_DEPTH: usize = 64;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: RenderId) -> Option<RenderId> {
        RenderTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    #[inline]
    fn ancestors(&self, start: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: RenderId) -> impl Iterator<Item = (RenderId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        AllSiblings::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: RenderId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: RenderId) -> bool {
        self.get(id)
            .map(|node| !node.children().is_empty())
            .unwrap_or(false)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::r#box::basic::RenderPadding;
    use flui_types::EdgeInsets;

    fn test_render_box() -> Box<dyn RenderObject> {
        Box::new(RenderPadding::new(EdgeInsets::all(10.0)))
    }

    #[test]
    fn test_tree_creation() {
        let tree = RenderTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_insert_and_get() {
        let mut tree = RenderTree::new();

        let id = tree.insert(test_render_box());
        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert!(tree.get(id).is_some());
    }

    #[test]
    fn test_render_id_offset() {
        let mut tree = RenderTree::new();

        // First node should have ID 1 (not 0)
        let id1 = tree.insert(test_render_box());
        assert_eq!(id1.get(), 1);

        let id2 = tree.insert(test_render_box());
        assert_eq!(id2.get(), 2);

        // Both should be accessible
        assert!(tree.get(id1).is_some());
        assert!(tree.get(id2).is_some());
    }

    #[test]
    fn test_remove() {
        let mut tree = RenderTree::new();

        let id = tree.insert(test_render_box());
        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_root_management() {
        let mut tree = RenderTree::new();

        let id = tree.insert(test_render_box());
        assert!(tree.root().is_none());

        tree.set_root(Some(id));
        assert_eq!(tree.root(), Some(id));

        tree.set_root(None);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_insert_child() {
        let mut tree = RenderTree::new();

        let parent_id = tree.insert(test_render_box());
        tree.set_root(Some(parent_id));

        let child_id = tree.insert_child(parent_id, test_render_box());
        assert!(child_id.is_some());

        let child_id = child_id.unwrap();

        // Verify relationships
        assert_eq!(tree.parent(child_id), Some(parent_id));
        assert_eq!(tree.children(parent_id), &[child_id]);

        // Verify depth
        assert_eq!(tree.depth(parent_id), Some(0));
        assert_eq!(tree.depth(child_id), Some(1));
    }

    #[test]
    fn test_is_ancestor() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child_id = tree.insert_child(root_id, test_render_box()).unwrap();
        let grandchild_id = tree.insert_child(child_id, test_render_box()).unwrap();

        assert!(tree.is_ancestor(root_id, child_id));
        assert!(tree.is_ancestor(root_id, grandchild_id));
        assert!(tree.is_ancestor(child_id, grandchild_id));
        assert!(!tree.is_ancestor(child_id, root_id));
        assert!(!tree.is_ancestor(grandchild_id, root_id));
    }

    #[test]
    fn test_path_to_root() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child_id = tree.insert_child(root_id, test_render_box()).unwrap();
        let grandchild_id = tree.insert_child(child_id, test_render_box()).unwrap();

        let path = tree.path_to_root(grandchild_id);
        assert_eq!(path, vec![root_id, child_id, grandchild_id]);
    }

    #[test]
    fn test_remove_recursive() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child1 = tree.insert_child(root_id, test_render_box()).unwrap();
        let child2 = tree.insert_child(root_id, test_render_box()).unwrap();
        let _grandchild = tree.insert_child(child1, test_render_box()).unwrap();

        assert_eq!(tree.len(), 4);

        // Remove child1 and its descendants
        let removed = tree.remove_recursive(child1);
        assert_eq!(removed, 2); // child1 + grandchild
        assert_eq!(tree.len(), 2); // root + child2

        // child1 should be removed from root's children
        assert_eq!(tree.children(root_id), &[child2]);
    }

    #[test]
    fn test_iteration() {
        let mut tree = RenderTree::new();

        tree.insert(test_render_box());
        tree.insert(test_render_box());
        tree.insert(test_render_box());

        let ids: Vec<_> = tree.ids().collect();
        assert_eq!(ids.len(), 3);

        let nodes: Vec<_> = tree.nodes().collect();
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn test_visit_depth_first() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child1 = tree.insert_child(root_id, test_render_box()).unwrap();
        let child2 = tree.insert_child(root_id, test_render_box()).unwrap();
        let grandchild = tree.insert_child(child1, test_render_box()).unwrap();

        let mut visited = Vec::new();
        tree.visit_depth_first(|id, _| {
            visited.push(id);
        });

        // Pre-order: root, child1, grandchild, child2
        assert_eq!(visited, vec![root_id, child1, grandchild, child2]);
    }

    #[test]
    fn test_clear() {
        let mut tree = RenderTree::new();

        let id = tree.insert(test_render_box());
        tree.set_root(Some(id));
        tree.insert(test_render_box());

        assert_eq!(tree.len(), 2);

        tree.clear();

        assert!(tree.is_empty());
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RenderTree>();
        assert_send_sync::<RenderNode>();
    }

    // ========================================================================
    // flui-tree Trait Tests
    // ========================================================================

    #[test]
    fn test_tree_read_trait() {
        let mut tree = RenderTree::new();

        let id1 = TreeWrite::insert(&mut tree, RenderNode::new(test_render_box()));
        let id2 = TreeWrite::insert(&mut tree, RenderNode::new(test_render_box()));

        // TreeRead::get
        assert!(TreeRead::get(&tree, id1).is_some());
        assert!(TreeRead::get(&tree, id2).is_some());

        // TreeRead::contains
        assert!(TreeRead::contains(&tree, id1));
        assert!(TreeRead::contains(&tree, id2));

        // TreeRead::len
        assert_eq!(TreeRead::len(&tree), 2);

        // TreeRead::node_ids
        let ids: Vec<_> = TreeRead::node_ids(&tree).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_tree_write_trait() {
        let mut tree = RenderTree::new();

        // TreeWrite::insert
        let id = TreeWrite::insert(&mut tree, RenderNode::new(test_render_box()));
        assert!(TreeRead::contains(&tree, id));

        // TreeWrite::get_mut
        assert!(TreeWrite::get_mut(&mut tree, id).is_some());

        // TreeWrite::remove
        let removed = TreeWrite::remove(&mut tree, id);
        assert!(removed.is_some());
        assert!(!TreeRead::contains(&tree, id));

        // TreeWrite::clear
        TreeWrite::insert(&mut tree, RenderNode::new(test_render_box()));
        TreeWrite::insert(&mut tree, RenderNode::new(test_render_box()));
        assert_eq!(TreeRead::len(&tree), 2);
        TreeWrite::clear(&mut tree);
        assert_eq!(TreeRead::len(&tree), 0);
    }

    #[test]
    fn test_tree_nav_trait_parent_children() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child_id = tree.insert_child(root_id, test_render_box()).unwrap();

        // TreeNav::parent
        assert_eq!(TreeNav::parent(&tree, child_id), Some(root_id));
        assert_eq!(TreeNav::parent(&tree, root_id), None);

        // TreeNav::children
        let children: Vec<_> = TreeNav::children(&tree, root_id).collect();
        assert_eq!(children, vec![child_id]);

        // TreeNav::child_count
        assert_eq!(TreeNav::child_count(&tree, root_id), 1);
        assert_eq!(TreeNav::child_count(&tree, child_id), 0);

        // TreeNav::has_children
        assert!(TreeNav::has_children(&tree, root_id));
        assert!(!TreeNav::has_children(&tree, child_id));
    }

    #[test]
    fn test_tree_nav_trait_ancestors() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child_id = tree.insert_child(root_id, test_render_box()).unwrap();
        let grandchild_id = tree.insert_child(child_id, test_render_box()).unwrap();

        // TreeNav::ancestors (includes self)
        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);

        let ancestors_from_child: Vec<_> = TreeNav::ancestors(&tree, child_id).collect();
        assert_eq!(ancestors_from_child, vec![child_id, root_id]);

        let ancestors_from_root: Vec<_> = TreeNav::ancestors(&tree, root_id).collect();
        assert_eq!(ancestors_from_root, vec![root_id]);
    }

    #[test]
    fn test_tree_nav_trait_descendants() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child1 = tree.insert_child(root_id, test_render_box()).unwrap();
        let child2 = tree.insert_child(root_id, test_render_box()).unwrap();
        let grandchild = tree.insert_child(child1, test_render_box()).unwrap();

        // TreeNav::descendants returns (id, depth) pairs
        let descendants: Vec<_> = TreeNav::descendants(&tree, root_id).collect();

        assert_eq!(descendants.len(), 4);
        assert!(descendants.contains(&(root_id, 0)));
        assert!(descendants.contains(&(child1, 1)));
        assert!(descendants.contains(&(child2, 1)));
        assert!(descendants.contains(&(grandchild, 2)));
    }

    #[test]
    fn test_tree_nav_trait_siblings() {
        let mut tree = RenderTree::new();

        let root_id = tree.insert(test_render_box());
        tree.set_root(Some(root_id));

        let child1 = tree.insert_child(root_id, test_render_box()).unwrap();
        let child2 = tree.insert_child(root_id, test_render_box()).unwrap();
        let child3 = tree.insert_child(root_id, test_render_box()).unwrap();

        // TreeNav::siblings (excludes self, as per AllSiblings behavior)
        let siblings: Vec<_> = TreeNav::siblings(&tree, child2).collect();
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&child1));
        assert!(siblings.contains(&child3));
        // child2 should NOT be in its own siblings list
        assert!(!siblings.contains(&child2));

        // Root has no siblings
        let root_siblings: Vec<_> = TreeNav::siblings(&tree, root_id).collect();
        assert!(root_siblings.is_empty());
    }
}
