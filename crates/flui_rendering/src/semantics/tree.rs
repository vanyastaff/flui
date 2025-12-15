//! Semantics tree management and updates.
//!
//! This module provides the infrastructure for managing the semantics tree
//! and sending updates to the platform's accessibility API.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use flui_types::Matrix4;

use super::{ActionArgs, SemanticsAction, SemanticsNode, SemanticsNodeData, SemanticsNodeId};

// ============================================================================
// SemanticsOwner
// ============================================================================

/// Owns and manages the semantics tree.
///
/// The semantics owner is responsible for:
/// - Maintaining the semantics tree structure
/// - Tracking dirty nodes that need updates
/// - Building semantics updates for the platform
/// - Dispatching semantics actions
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsOwner` class.
pub struct SemanticsOwner {
    /// All nodes in the tree, indexed by ID.
    nodes: HashMap<SemanticsNodeId, SemanticsNode>,

    /// The root node ID.
    root_id: SemanticsNodeId,

    /// Nodes that are dirty and need to be sent to the platform.
    dirty_nodes: HashSet<SemanticsNodeId>,

    /// Nodes that have been removed since last update.
    removed_nodes: HashSet<SemanticsNodeId>,

    /// Next available node ID.
    next_id: u64,

    /// Callback for when semantics update is ready.
    on_semantics_update: Option<Arc<dyn Fn(SemanticsUpdate) + Send + Sync>>,
}

impl SemanticsOwner {
    /// Creates a new semantics owner with a root node.
    pub fn new() -> Self {
        let root_id = SemanticsNodeId::ROOT;
        let root = SemanticsNode::root();

        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);

        Self {
            nodes,
            root_id,
            dirty_nodes: HashSet::from([root_id]),
            removed_nodes: HashSet::new(),
            next_id: 1, // 0 is reserved for root
            on_semantics_update: None,
        }
    }

    /// Sets the callback for semantics updates.
    pub fn set_on_semantics_update<F>(&mut self, callback: F)
    where
        F: Fn(SemanticsUpdate) + Send + Sync + 'static,
    {
        self.on_semantics_update = Some(Arc::new(callback));
    }

    /// Returns the root node ID.
    pub fn root_id(&self) -> SemanticsNodeId {
        self.root_id
    }

    /// Returns a reference to the root node.
    pub fn root(&self) -> Option<&SemanticsNode> {
        self.nodes.get(&self.root_id)
    }

    /// Returns a mutable reference to the root node.
    pub fn root_mut(&mut self) -> Option<&mut SemanticsNode> {
        let root_id = self.root_id;
        self.dirty_nodes.insert(root_id);
        self.nodes.get_mut(&root_id)
    }

    // ========================================================================
    // Node Management
    // ========================================================================

    /// Allocates a new node ID.
    pub fn allocate_id(&mut self) -> SemanticsNodeId {
        let id = SemanticsNodeId::from_index(self.next_id);
        self.next_id += 1;
        id
    }

    /// Creates a new node with the given ID.
    pub fn create_node(&mut self, id: SemanticsNodeId) -> &mut SemanticsNode {
        let node = SemanticsNode::new(id);
        self.nodes.insert(id, node);
        self.dirty_nodes.insert(id);
        self.nodes.get_mut(&id).unwrap()
    }

    /// Creates a new node and returns its ID.
    pub fn create_node_with_new_id(&mut self) -> SemanticsNodeId {
        let id = self.allocate_id();
        self.create_node(id);
        id
    }

    /// Returns a reference to a node.
    pub fn node(&self, id: SemanticsNodeId) -> Option<&SemanticsNode> {
        self.nodes.get(&id)
    }

    /// Returns a mutable reference to a node.
    pub fn node_mut(&mut self, id: SemanticsNodeId) -> Option<&mut SemanticsNode> {
        if let Some(node) = self.nodes.get_mut(&id) {
            self.dirty_nodes.insert(id);
            Some(node)
        } else {
            None
        }
    }

    /// Removes a node and all its descendants.
    pub fn remove_node(&mut self, id: SemanticsNodeId) {
        // Don't remove root
        if id == self.root_id {
            return;
        }

        // Collect all descendant IDs
        let descendants = self.collect_descendants(id);

        // Remove from parent's children list
        if let Some(node) = self.nodes.get(&id) {
            if let Some(parent_id) = node.parent() {
                if let Some(parent) = self.nodes.get_mut(&parent_id) {
                    parent.remove_child(id);
                    self.dirty_nodes.insert(parent_id);
                }
            }
        }

        // Remove node and all descendants
        for desc_id in descendants {
            if let Some(_) = self.nodes.remove(&desc_id) {
                self.removed_nodes.insert(desc_id);
                self.dirty_nodes.remove(&desc_id);
            }
        }

        if let Some(_) = self.nodes.remove(&id) {
            self.removed_nodes.insert(id);
            self.dirty_nodes.remove(&id);
        }
    }

    /// Collects all descendant IDs of a node.
    fn collect_descendants(&self, id: SemanticsNodeId) -> Vec<SemanticsNodeId> {
        let mut result = Vec::new();
        let mut stack = vec![id];

        while let Some(current_id) = stack.pop() {
            if let Some(node) = self.nodes.get(&current_id) {
                for child_id in node.children() {
                    result.push(*child_id);
                    stack.push(*child_id);
                }
            }
        }

        result
    }

    /// Returns whether a node exists.
    pub fn contains(&self, id: SemanticsNodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Returns the number of nodes in the tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Sets the parent-child relationship between nodes.
    pub fn set_parent(&mut self, child_id: SemanticsNodeId, parent_id: SemanticsNodeId) {
        // Remove from old parent
        if let Some(child) = self.nodes.get(&child_id) {
            if let Some(old_parent_id) = child.parent() {
                if old_parent_id != parent_id {
                    if let Some(old_parent) = self.nodes.get_mut(&old_parent_id) {
                        old_parent.remove_child(child_id);
                        self.dirty_nodes.insert(old_parent_id);
                    }
                }
            }
        }

        // Set new parent
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.set_parent(Some(parent_id));
            self.dirty_nodes.insert(child_id);
        }

        // Add to new parent's children
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            if !parent.children().contains(&child_id) {
                parent.add_child(child_id);
            }
            self.dirty_nodes.insert(parent_id);
        }

        // Update depths
        self.update_depth(child_id);
    }

    /// Updates the depth of a node and its descendants.
    fn update_depth(&mut self, id: SemanticsNodeId) {
        let parent_depth = self
            .nodes
            .get(&id)
            .and_then(|n| n.parent())
            .and_then(|p| self.nodes.get(&p))
            .map(|p| p.depth())
            .unwrap_or(0);

        let mut stack = vec![(id, parent_depth + 1)];

        while let Some((current_id, depth)) = stack.pop() {
            if let Some(node) = self.nodes.get_mut(&current_id) {
                node.set_depth(depth);
                for child_id in node.children().to_vec() {
                    stack.push((child_id, depth + 1));
                }
            }
        }
    }

    // ========================================================================
    // Dirty Management
    // ========================================================================

    /// Returns whether there are dirty nodes.
    pub fn has_dirty_nodes(&self) -> bool {
        !self.dirty_nodes.is_empty() || !self.removed_nodes.is_empty()
    }

    /// Marks a node as dirty.
    pub fn mark_dirty(&mut self, id: SemanticsNodeId) {
        if self.nodes.contains_key(&id) {
            self.dirty_nodes.insert(id);
        }
    }

    /// Clears all dirty flags and returns the set of dirty node IDs.
    fn take_dirty_nodes(&mut self) -> HashSet<SemanticsNodeId> {
        std::mem::take(&mut self.dirty_nodes)
    }

    /// Clears all removed nodes and returns the set.
    fn take_removed_nodes(&mut self) -> HashSet<SemanticsNodeId> {
        std::mem::take(&mut self.removed_nodes)
    }

    // ========================================================================
    // Updates
    // ========================================================================

    /// Sends pending semantics updates to the platform.
    pub fn send_semantics_update(&mut self) {
        if !self.has_dirty_nodes() {
            return;
        }

        let update = self.build_update();

        // Clear dirty flags on updated nodes
        for node_data in &update.nodes {
            if let Some(id) = SemanticsNodeId::new(node_data.id + 1) {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.clear_dirty();
                    node.clear_children_dirty();
                }
            }
        }

        // Send update if callback is set
        if let Some(callback) = &self.on_semantics_update {
            callback(update);
        }
    }

    /// Builds a semantics update from dirty nodes.
    pub fn build_update(&mut self) -> SemanticsUpdate {
        let mut builder = SemanticsUpdateBuilder::new();

        // Add removed nodes
        for id in self.take_removed_nodes() {
            builder.add_removed_node(id);
        }

        // Add updated nodes
        for id in self.take_dirty_nodes() {
            if let Some(node) = self.nodes.get(&id) {
                builder.add_node(node.to_platform_data());
            }
        }

        builder.build()
    }

    // ========================================================================
    // Action Dispatch
    // ========================================================================

    /// Performs a semantics action on a node.
    pub fn perform_action(
        &self,
        id: SemanticsNodeId,
        action: SemanticsAction,
        args: Option<ActionArgs>,
    ) -> bool {
        if let Some(node) = self.nodes.get(&id) {
            if let Some(handler) = node.config().action_handler(action) {
                handler(action, args);
                return true;
            }
        }
        false
    }

    /// Performs a semantics action by external ID.
    pub fn perform_action_by_index(
        &self,
        index: u64,
        action: SemanticsAction,
        args: Option<ActionArgs>,
    ) -> bool {
        let id = SemanticsNodeId::from_index(index);
        self.perform_action(id, action, args)
    }

    // ========================================================================
    // Queries
    // ========================================================================

    /// Finds a node at the given position (in root coordinates).
    pub fn hit_test(&self, position: flui_types::Offset) -> Option<SemanticsNodeId> {
        self.hit_test_node(self.root_id, position, &Matrix4::IDENTITY)
    }

    /// Recursively hit tests nodes.
    fn hit_test_node(
        &self,
        id: SemanticsNodeId,
        position: flui_types::Offset,
        parent_transform: &Matrix4,
    ) -> Option<SemanticsNodeId> {
        let node = self.nodes.get(&id)?;

        // Compute global transform
        let global_transform = *parent_transform * *node.transform();

        // Transform position to local coordinates
        let local_offset = global_transform
            .try_inverse()
            .map(|inverse| {
                let (x, y) = inverse.transform_point(position.dx, position.dy);
                flui_types::Offset::new(x, y)
            })
            .unwrap_or(position);

        // Check if position is within bounds
        let rect = node.rect();
        if !rect.contains_offset(local_offset) {
            return None;
        }

        // If hidden, don't include in hit test
        if node.config().is_hidden() {
            return None;
        }

        // Check children in reverse order (front to back)
        for child_id in node.children().iter().rev() {
            if let Some(hit_id) = self.hit_test_node(*child_id, position, &global_transform) {
                return Some(hit_id);
            }
        }

        // Return this node if it has any actions or is a semantic boundary
        if node.has_actions() || node.config().is_semantics_boundary() {
            Some(id)
        } else {
            None
        }
    }

    /// Finds all nodes with a specific tag.
    pub fn find_nodes_with_tag(&self, tag: &super::SemanticsTag) -> Vec<SemanticsNodeId> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.has_tag(tag))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns an iterator over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = (&SemanticsNodeId, &SemanticsNode)> {
        self.nodes.iter()
    }
}

impl Default for SemanticsOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SemanticsOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsOwner")
            .field("node_count", &self.nodes.len())
            .field("dirty_count", &self.dirty_nodes.len())
            .field("removed_count", &self.removed_nodes.len())
            .finish()
    }
}

// ============================================================================
// SemanticsUpdate
// ============================================================================

/// An update to the semantics tree to be sent to the platform.
///
/// This contains all the information needed to update the platform's
/// accessibility tree.
#[derive(Debug, Clone, Default)]
pub struct SemanticsUpdate {
    /// Nodes that have been added or updated.
    pub nodes: Vec<SemanticsNodeData>,

    /// IDs of nodes that have been removed.
    pub removed_node_ids: Vec<u64>,
}

impl SemanticsUpdate {
    /// Returns whether this update is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.removed_node_ids.is_empty()
    }

    /// Returns the number of node updates.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of removed nodes.
    pub fn removed_count(&self) -> usize {
        self.removed_node_ids.len()
    }
}

// ============================================================================
// SemanticsUpdateBuilder
// ============================================================================

/// Builder for constructing semantics updates.
#[derive(Debug, Default)]
pub struct SemanticsUpdateBuilder {
    nodes: Vec<SemanticsNodeData>,
    removed_node_ids: Vec<u64>,
}

impl SemanticsUpdateBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node to the update.
    pub fn add_node(&mut self, node: SemanticsNodeData) {
        self.nodes.push(node);
    }

    /// Adds a removed node ID.
    pub fn add_removed_node(&mut self, id: SemanticsNodeId) {
        self.removed_node_ids.push(id.to_index());
    }

    /// Builds the update.
    pub fn build(self) -> SemanticsUpdate {
        SemanticsUpdate {
            nodes: self.nodes,
            removed_node_ids: self.removed_node_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantics::SemanticsActionHandler;
    use flui_types::Rect;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_owner_creation() {
        let owner = SemanticsOwner::new();
        assert_eq!(owner.node_count(), 1); // Just root
        assert!(owner.root().is_some());
    }

    #[test]
    fn test_create_nodes() {
        let mut owner = SemanticsOwner::new();

        let id1 = owner.create_node_with_new_id();
        let id2 = owner.create_node_with_new_id();

        assert_ne!(id1, id2);
        assert_eq!(owner.node_count(), 3); // root + 2
    }

    #[test]
    fn test_parent_child_relationship() {
        let mut owner = SemanticsOwner::new();

        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());

        let root = owner.root().unwrap();
        assert!(root.children().contains(&child_id));

        let child = owner.node(child_id).unwrap();
        assert_eq!(child.parent(), Some(owner.root_id()));
    }

    #[test]
    fn test_remove_node() {
        let mut owner = SemanticsOwner::new();

        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());

        assert_eq!(owner.node_count(), 2);

        owner.remove_node(child_id);

        assert_eq!(owner.node_count(), 1);
        assert!(!owner.contains(child_id));
    }

    #[test]
    fn test_remove_node_with_descendants() {
        let mut owner = SemanticsOwner::new();

        let child_id = owner.create_node_with_new_id();
        let grandchild_id = owner.create_node_with_new_id();

        owner.set_parent(child_id, owner.root_id());
        owner.set_parent(grandchild_id, child_id);

        assert_eq!(owner.node_count(), 3);

        owner.remove_node(child_id);

        assert_eq!(owner.node_count(), 1);
        assert!(!owner.contains(child_id));
        assert!(!owner.contains(grandchild_id));
    }

    #[test]
    fn test_dirty_tracking() {
        let mut owner = SemanticsOwner::new();
        assert!(owner.has_dirty_nodes()); // Root is dirty initially

        let update = owner.build_update();
        assert!(!update.is_empty());
        assert!(!owner.has_dirty_nodes());

        // Modify a node
        owner
            .node_mut(owner.root_id())
            .unwrap()
            .config_mut()
            .set_label("Test");
        assert!(owner.has_dirty_nodes());
    }

    #[test]
    fn test_build_update() {
        let mut owner = SemanticsOwner::new();

        owner.root_mut().unwrap().config_mut().set_label("Root");

        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());
        owner
            .node_mut(child_id)
            .unwrap()
            .config_mut()
            .set_label("Child");
        owner
            .node_mut(child_id)
            .unwrap()
            .config_mut()
            .set_button(true);

        let update = owner.build_update();

        assert_eq!(update.node_count(), 2);
        assert!(update.removed_node_ids.is_empty());
    }

    #[test]
    fn test_update_with_removals() {
        let mut owner = SemanticsOwner::new();

        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());

        // Clear initial dirty state
        let _ = owner.build_update();

        // Remove node
        owner.remove_node(child_id);

        let update = owner.build_update();
        assert_eq!(update.removed_count(), 1);
        assert!(update.removed_node_ids.contains(&child_id.to_index()));
    }

    #[test]
    fn test_action_dispatch() {
        let mut owner = SemanticsOwner::new();

        let was_tapped = Arc::new(AtomicBool::new(false));
        let was_tapped_clone = Arc::clone(&was_tapped);

        let handler: SemanticsActionHandler = Arc::new(move |action, _args| {
            if action == SemanticsAction::Tap {
                was_tapped_clone.store(true, Ordering::SeqCst);
            }
        });

        owner
            .root_mut()
            .unwrap()
            .config_mut()
            .add_action(SemanticsAction::Tap, handler);

        let result = owner.perform_action(owner.root_id(), SemanticsAction::Tap, None);

        assert!(result);
        assert!(was_tapped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_hit_test() {
        let mut owner = SemanticsOwner::new();

        // Set up root with bounds
        {
            let root = owner.root_mut().unwrap();
            root.set_rect(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0));
            root.config_mut().set_semantics_boundary(true);
        }

        // Create child
        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());
        {
            let child = owner.node_mut(child_id).unwrap();
            child.set_rect(Rect::from_ltwh(10.0, 10.0, 50.0, 50.0));
            child.config_mut().set_button(true);
            child
                .config_mut()
                .add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        }

        // Hit test inside child
        let result = owner.hit_test(flui_types::Offset::new(25.0, 25.0));
        assert_eq!(result, Some(child_id));

        // Hit test outside child but inside root
        let result = owner.hit_test(flui_types::Offset::new(80.0, 80.0));
        assert_eq!(result, Some(owner.root_id()));

        // Hit test outside root
        let result = owner.hit_test(flui_types::Offset::new(150.0, 150.0));
        assert_eq!(result, None);
    }

    #[test]
    fn test_semantics_update_builder() {
        let mut builder = SemanticsUpdateBuilder::new();

        builder.add_node(SemanticsNodeData {
            id: 0,
            flags: 0,
            actions: 0,
            label: Some("Test".to_string()),
            value: None,
            increased_value: None,
            decreased_value: None,
            hint: None,
            tooltip: None,
            text_direction: None,
            rect: Rect::ZERO,
            transform: Matrix4::IDENTITY,
            children: vec![],
            elevation: 0.0,
            thickness: 0.0,
            platform_view_id: None,
            max_value_length: None,
            current_value_length: None,
            scroll_position: None,
            scroll_extent_max: None,
            scroll_extent_min: None,
            scroll_index: None,
            scroll_child_count: None,
        });

        builder.add_removed_node(SemanticsNodeId::from_index(5));

        let update = builder.build();

        assert_eq!(update.node_count(), 1);
        assert_eq!(update.removed_count(), 1);
    }

    #[test]
    fn test_find_nodes_with_tag() {
        let mut owner = SemanticsOwner::new();

        let tag = super::super::SemanticsTag::new("test_tag");

        let child_id = owner.create_node_with_new_id();
        owner.set_parent(child_id, owner.root_id());
        owner.node_mut(child_id).unwrap().add_tag(tag.clone());

        let found = owner.find_nodes_with_tag(&tag);
        assert_eq!(found.len(), 1);
        assert!(found.contains(&child_id));
    }
}
