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
use crate::protocol::{BoxProtocol, RenderObject, SliverProtocol};

use super::node::RenderNode;

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
/// // Insert child`
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

    /// Inserts a Box protocol render object into the tree (no parent).
    ///
    /// Returns the RenderId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `RenderId(1)`
    pub fn insert_box(&mut self, render_object: Box<dyn RenderObject<BoxProtocol>>) -> RenderId {
        let node = RenderNode::new_box(render_object);
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1) // 0-based → 1-based
    }

    /// Inserts a Sliver protocol render object into the tree (no parent).
    pub fn insert_sliver(
        &mut self,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> RenderId {
        let node = RenderNode::new_sliver(render_object);
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    /// Inserts a Box protocol render object as a child of the given parent.
    ///
    /// Returns the RenderId of the inserted child.
    pub fn insert_box_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<BoxProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth
        let parent_depth = self.get(parent_id)?.depth();

        // Create child node
        let child_node =
            RenderNode::new_box_with_parent(render_object, parent_id, (parent_depth + 1) as u16);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = RenderId::new(child_slab_index + 1);

        // Add child to parent's tree structure
        if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Inserts a Sliver protocol render object as a child of the given parent.
    pub fn insert_sliver_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> Option<RenderId> {
        let parent_depth = self.get(parent_id)?.depth();

        let child_node =
            RenderNode::new_sliver_with_parent(render_object, parent_id, (parent_depth + 1) as u16);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = RenderId::new(child_slab_index + 1);

        if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Removes a node from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Use `remove_recursive` for that.
    pub fn remove(&mut self, id: RenderId) -> Option<RenderNode> {
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

        self.nodes.try_remove(id.get() - 1)
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
    pub fn depth(&self, id: RenderId) -> Option<u16> {
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
            .filter(|(_, node)| node.needs_layout())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth() as usize))
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
            .filter(|(_, node)| node.needs_paint())
            .map(|(idx, node)| (RenderId::new(idx + 1), node.depth() as usize))
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

