//! Semantics tree for accessibility support.
//!
//! This module provides the semantics tree that parallels the render tree
//! for accessibility purposes. Screen readers and other assistive technologies
//! interact with the semantics tree rather than the render tree directly.
//!
//! # Flutter Protocol
//!
//! Similar to Flutter's semantics system:
//! - [`SemanticsNode`] - Individual node in the semantics tree
//! - [`SemanticsOwner`] - Manages the semantics tree lifecycle
//! - [`SemanticsHandle`] - Opaque handle for platform accessibility
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      RenderTree                              │
//! │  ┌─────────┐    ┌─────────┐    ┌─────────┐                  │
//! │  │ RenderA │───▶│ RenderB │───▶│ RenderC │                  │
//! │  │ (bound) │    │  (no)   │    │ (bound) │                  │
//! │  └────┬────┘    └─────────┘    └────┬────┘                  │
//! │       │                              │                       │
//! └───────┼──────────────────────────────┼───────────────────────┘
//!         │                              │
//!         ▼                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    SemanticsTree                             │
//! │  ┌─────────────┐              ┌─────────────┐               │
//! │  │ SemanticsA  │─────────────▶│ SemanticsC  │               │
//! │  │ (merged B)  │              │             │               │
//! │  └─────────────┘              └─────────────┘               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! Render objects with `is_semantics_boundary() == true` create nodes.
//! Non-boundary objects merge their semantics into the parent boundary.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flui_foundation::ElementId;
use flui_types::semantics::{SemanticsAction, SemanticsData, SemanticsProperties, SemanticsRole};
use flui_types::Rect;
use parking_lot::RwLock;

// ============================================================================
// SEMANTICS NODE ID
// ============================================================================

/// Unique identifier for a semantics node.
///
/// Similar to Flutter's semantics node ID. This is separate from ElementId
/// because the semantics tree has different structure than the render tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsNodeId(u64);

impl SemanticsNodeId {
    /// Creates a new semantics node ID.
    fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    pub fn get(&self) -> u64 {
        self.0
    }
}

// ============================================================================
// SEMANTICS NODE
// ============================================================================

/// A node in the semantics tree.
///
/// Each semantics node corresponds to one or more render objects in the
/// render tree. Non-boundary render objects are merged into their parent
/// semantics boundary.
///
/// # Flutter Protocol
///
/// Similar to Flutter's `SemanticsNode`:
/// - Properties for screen readers (label, hint, value)
/// - Supported actions (tap, scroll, increase/decrease)
/// - Geometry for spatial navigation
/// - Tree structure (parent, children)
#[derive(Debug)]
pub struct SemanticsNode {
    /// Unique identifier for this node.
    id: SemanticsNodeId,

    /// The render element that owns this semantics node.
    render_element: ElementId,

    /// Semantic properties (label, role, etc.).
    properties: SemanticsProperties,

    /// Bounding rectangle in global coordinates.
    rect: Rect,

    /// Transform matrix (stored as 4x4 column-major).
    transform: Option<[f32; 16]>,

    /// Supported semantic actions.
    actions: Vec<SemanticsAction>,

    /// Parent node ID (None for root).
    parent: Option<SemanticsNodeId>,

    /// Child node IDs.
    children: Vec<SemanticsNodeId>,

    /// Whether this node is marked dirty and needs update.
    dirty: bool,
}

impl SemanticsNode {
    /// Creates a new semantics node.
    pub fn new(id: SemanticsNodeId, render_element: ElementId) -> Self {
        Self {
            id,
            render_element,
            properties: SemanticsProperties::default(),
            rect: Rect::ZERO,
            transform: None,
            actions: Vec::new(),
            parent: None,
            children: Vec::new(),
            dirty: true,
        }
    }

    /// Returns the node ID.
    #[inline]
    pub fn id(&self) -> SemanticsNodeId {
        self.id
    }

    /// Returns the associated render element ID.
    #[inline]
    pub fn render_element(&self) -> ElementId {
        self.render_element
    }

    /// Returns the semantic properties.
    #[inline]
    pub fn properties(&self) -> &SemanticsProperties {
        &self.properties
    }

    /// Sets the semantic properties.
    pub fn set_properties(&mut self, properties: SemanticsProperties) {
        self.properties = properties;
        self.dirty = true;
    }

    /// Returns the bounding rectangle.
    #[inline]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Sets the bounding rectangle.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.dirty = true;
    }

    /// Returns the transform matrix.
    #[inline]
    pub fn transform(&self) -> Option<&[f32; 16]> {
        self.transform.as_ref()
    }

    /// Sets the transform matrix.
    pub fn set_transform(&mut self, transform: Option<[f32; 16]>) {
        self.transform = transform;
        self.dirty = true;
    }

    /// Returns the supported actions.
    #[inline]
    pub fn actions(&self) -> &[SemanticsAction] {
        &self.actions
    }

    /// Sets the supported actions.
    pub fn set_actions(&mut self, actions: Vec<SemanticsAction>) {
        self.actions = actions;
        self.dirty = true;
    }

    /// Adds an action to the supported actions.
    pub fn add_action(&mut self, action: SemanticsAction) {
        if !self.actions.contains(&action) {
            self.actions.push(action);
            self.dirty = true;
        }
    }

    /// Returns true if this action is supported.
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains(&action)
    }

    /// Returns the parent node ID.
    #[inline]
    pub fn parent(&self) -> Option<SemanticsNodeId> {
        self.parent
    }

    /// Returns the child node IDs.
    #[inline]
    pub fn children(&self) -> &[SemanticsNodeId] {
        &self.children
    }

    /// Returns true if this node is dirty.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks this node as clean.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Marks this node as dirty.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Converts this node to SemanticsData for platform consumption.
    pub fn to_data(&self) -> SemanticsData {
        SemanticsData::new(self.properties.clone(), self.rect)
    }

    /// Returns true if this node has semantic content.
    ///
    /// A node has semantic content if it has:
    /// - A label, value, or hint
    /// - A role other than None
    /// - Any supported actions
    pub fn has_content(&self) -> bool {
        self.properties.has_label()
            || self.properties.has_value()
            || self.properties.has_hint()
            || self.properties.role() != SemanticsRole::None
            || !self.actions.is_empty()
    }

    /// Merges another node's properties into this one.
    ///
    /// Used when a non-boundary render object's semantics should be
    /// merged into its parent boundary.
    pub fn merge(&mut self, other: &SemanticsNode) {
        // Merge label (concatenate with space)
        if let Some(other_label) = other.properties.label() {
            if let Some(self_label) = &self.properties.label {
                self.properties.label = Some(format!("{} {}", self_label, other_label));
            } else {
                self.properties.label = Some(other_label.to_string());
            }
        }

        // Merge value (prefer other if self is empty)
        if other.properties.has_value() && !self.properties.has_value() {
            self.properties.value = other.properties.value.clone();
        }

        // Merge hint (prefer other if self is empty)
        if other.properties.has_hint() && !self.properties.has_hint() {
            self.properties.hint = other.properties.hint.clone();
        }

        // Merge role (prefer more specific role)
        if self.properties.role() == SemanticsRole::None
            && other.properties.role() != SemanticsRole::None
        {
            self.properties.role = other.properties.role();
        }

        // Merge actions
        for action in &other.actions {
            self.add_action(*action);
        }

        // Expand rect to include other's rect
        if self.rect == Rect::ZERO {
            self.rect = other.rect;
        } else if other.rect != Rect::ZERO {
            self.rect = self.rect.union(&other.rect);
        }

        self.dirty = true;
    }
}

// ============================================================================
// SEMANTICS OWNER
// ============================================================================

/// Manages the semantics tree and coordinates with platform accessibility.
///
/// The SemanticsOwner builds and maintains the semantics tree based on
/// the render tree. It handles:
/// - Creating/destroying semantics nodes
/// - Updating node properties when render objects change
/// - Sending updates to the platform accessibility layer
///
/// # Flutter Protocol
///
/// Similar to Flutter's `SemanticsOwner`:
/// - Owns the semantics tree
/// - Tracks dirty nodes
/// - Sends updates to platform
pub struct SemanticsOwner {
    /// ID counter for generating unique node IDs.
    next_id: AtomicU64,

    /// All semantics nodes by ID.
    nodes: RwLock<HashMap<SemanticsNodeId, SemanticsNode>>,

    /// Mapping from render element to semantics node.
    element_to_node: RwLock<HashMap<ElementId, SemanticsNodeId>>,

    /// Root node ID.
    root: RwLock<Option<SemanticsNodeId>>,

    /// Dirty nodes that need update.
    dirty_nodes: RwLock<Vec<SemanticsNodeId>>,

    /// Callback for sending updates to platform.
    update_callback: RwLock<Option<SemanticsUpdateCallback>>,
}

/// Callback type for platform semantics updates.
pub type SemanticsUpdateCallback = Arc<dyn Fn(&[SemanticsData]) + Send + Sync>;

impl SemanticsOwner {
    /// Creates a new semantics owner.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            nodes: RwLock::new(HashMap::new()),
            element_to_node: RwLock::new(HashMap::new()),
            root: RwLock::new(None),
            dirty_nodes: RwLock::new(Vec::new()),
            update_callback: RwLock::new(None),
        }
    }

    /// Sets the callback for platform updates.
    pub fn set_update_callback(&self, callback: SemanticsUpdateCallback) {
        *self.update_callback.write() = Some(callback);
    }

    /// Clears the update callback.
    pub fn clear_update_callback(&self) {
        *self.update_callback.write() = None;
    }

    /// Creates a new semantics node for a render element.
    ///
    /// Returns the node ID if successful.
    pub fn create_node(&self, render_element: ElementId) -> SemanticsNodeId {
        let id = SemanticsNodeId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let node = SemanticsNode::new(id, render_element);

        self.nodes.write().insert(id, node);
        self.element_to_node.write().insert(render_element, id);
        self.dirty_nodes.write().push(id);

        // Set as root if first node
        let mut root = self.root.write();
        if root.is_none() {
            *root = Some(id);
        }

        id
    }

    /// Removes a semantics node.
    pub fn remove_node(&self, id: SemanticsNodeId) {
        let mut nodes = self.nodes.write();

        if let Some(node) = nodes.remove(&id) {
            // Remove from element mapping
            self.element_to_node.write().remove(&node.render_element);

            // Remove from parent's children
            if let Some(parent_id) = node.parent {
                if let Some(parent) = nodes.get_mut(&parent_id) {
                    parent.children.retain(|&child| child != id);
                }
            }

            // Clear root if this was root
            let mut root = self.root.write();
            if *root == Some(id) {
                *root = None;
            }
        }

        // Remove from dirty list
        self.dirty_nodes.write().retain(|&n| n != id);
    }

    /// Gets a node by ID.
    pub fn get_node(&self, id: SemanticsNodeId) -> Option<SemanticsNode> {
        self.nodes.read().get(&id).cloned()
    }

    /// Gets a node by render element ID.
    pub fn get_node_for_element(&self, element_id: ElementId) -> Option<SemanticsNodeId> {
        self.element_to_node.read().get(&element_id).copied()
    }

    /// Updates a node's properties.
    pub fn update_node<F>(&self, id: SemanticsNodeId, f: F)
    where
        F: FnOnce(&mut SemanticsNode),
    {
        let mut nodes = self.nodes.write();
        if let Some(node) = nodes.get_mut(&id) {
            f(node);
            if node.is_dirty() {
                let mut dirty = self.dirty_nodes.write();
                if !dirty.contains(&id) {
                    dirty.push(id);
                }
            }
        }
    }

    /// Sets a node's parent.
    pub fn set_parent(&self, child_id: SemanticsNodeId, parent_id: SemanticsNodeId) {
        let mut nodes = self.nodes.write();

        // Remove from old parent
        if let Some(child) = nodes.get(&child_id) {
            if let Some(old_parent_id) = child.parent {
                if let Some(old_parent) = nodes.get_mut(&old_parent_id) {
                    old_parent.children.retain(|&c| c != child_id);
                }
            }
        }

        // Set new parent
        if let Some(child) = nodes.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }

        // Add to new parent's children
        if let Some(parent) = nodes.get_mut(&parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }
    }

    /// Returns the root node ID.
    pub fn root(&self) -> Option<SemanticsNodeId> {
        *self.root.read()
    }

    /// Flushes dirty nodes and sends updates to the platform.
    ///
    /// Call this after the frame is complete to sync semantics.
    pub fn flush(&self) {
        let dirty_ids: Vec<_> = {
            let mut dirty = self.dirty_nodes.write();
            std::mem::take(&mut *dirty)
        };

        if dirty_ids.is_empty() {
            return;
        }

        // Collect data for dirty nodes
        let mut updates = Vec::with_capacity(dirty_ids.len());
        {
            let mut nodes = self.nodes.write();
            for id in dirty_ids {
                if let Some(node) = nodes.get_mut(&id) {
                    updates.push(node.to_data());
                    node.mark_clean();
                }
            }
        }

        // Send to platform
        if let Some(callback) = &*self.update_callback.read() {
            callback(&updates);
        }
    }

    /// Returns the number of nodes in the tree.
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    /// Clears all nodes.
    pub fn clear(&self) {
        self.nodes.write().clear();
        self.element_to_node.write().clear();
        *self.root.write() = None;
        self.dirty_nodes.write().clear();
    }

    /// Visits all nodes in depth-first order.
    pub fn visit_nodes<F>(&self, mut f: F)
    where
        F: FnMut(&SemanticsNode),
    {
        let nodes = self.nodes.read();
        if let Some(root_id) = *self.root.read() {
            self.visit_node_recursive(&nodes, root_id, &mut f);
        }
    }

    fn visit_node_recursive<F>(
        &self,
        nodes: &HashMap<SemanticsNodeId, SemanticsNode>,
        id: SemanticsNodeId,
        f: &mut F,
    ) where
        F: FnMut(&SemanticsNode),
    {
        if let Some(node) = nodes.get(&id) {
            f(node);
            for &child_id in &node.children {
                self.visit_node_recursive(nodes, child_id, f);
            }
        }
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
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .field("node_count", &self.nodes.read().len())
            .field("root", &*self.root.read())
            .field("dirty_count", &self.dirty_nodes.read().len())
            .field("has_callback", &self.update_callback.read().is_some())
            .finish()
    }
}

impl Clone for SemanticsNode {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            render_element: self.render_element,
            properties: self.properties.clone(),
            rect: self.rect,
            transform: self.transform,
            actions: self.actions.clone(),
            parent: self.parent,
            children: self.children.clone(),
            dirty: self.dirty,
        }
    }
}

// ============================================================================
// SEMANTICS HANDLE
// ============================================================================

/// Handle for managing a semantics node's lifecycle.
///
/// When dropped, the associated semantics node is removed from the tree.
/// This provides RAII-style cleanup for semantics nodes.
#[derive(Debug)]
pub struct SemanticsHandle {
    node_id: SemanticsNodeId,
    owner: Arc<SemanticsOwner>,
}

impl SemanticsHandle {
    /// Creates a new handle.
    pub fn new(node_id: SemanticsNodeId, owner: Arc<SemanticsOwner>) -> Self {
        Self { node_id, owner }
    }

    /// Returns the node ID.
    pub fn node_id(&self) -> SemanticsNodeId {
        self.node_id
    }

    /// Updates the node's properties.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut SemanticsNode),
    {
        self.owner.update_node(self.node_id, f);
    }
}

impl Drop for SemanticsHandle {
    fn drop(&mut self) {
        self.owner.remove_node(self.node_id);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_node_new() {
        let id = SemanticsNodeId::new(1);
        let element_id = ElementId::new(42);
        let node = SemanticsNode::new(id, element_id);

        assert_eq!(node.id(), id);
        assert_eq!(node.render_element(), element_id);
        assert!(node.is_dirty());
        assert!(!node.has_content());
    }

    #[test]
    fn test_semantics_node_properties() {
        let id = SemanticsNodeId::new(1);
        let element_id = ElementId::new(42);
        let mut node = SemanticsNode::new(id, element_id);

        let props = SemanticsProperties::new()
            .with_role(SemanticsRole::Button)
            .with_label("Submit");

        node.set_properties(props);

        assert_eq!(node.properties().role(), SemanticsRole::Button);
        assert_eq!(node.properties().label(), Some("Submit"));
        assert!(node.has_content());
    }

    #[test]
    fn test_semantics_node_actions() {
        let id = SemanticsNodeId::new(1);
        let element_id = ElementId::new(42);
        let mut node = SemanticsNode::new(id, element_id);

        node.add_action(SemanticsAction::Tap);
        node.add_action(SemanticsAction::LongPress);
        node.add_action(SemanticsAction::Tap); // Duplicate

        assert_eq!(node.actions().len(), 2);
        assert!(node.has_action(SemanticsAction::Tap));
        assert!(node.has_action(SemanticsAction::LongPress));
        assert!(!node.has_action(SemanticsAction::ScrollUp));
    }

    #[test]
    fn test_semantics_node_merge() {
        let id1 = SemanticsNodeId::new(1);
        let id2 = SemanticsNodeId::new(2);
        let element_id = ElementId::new(42);

        let mut node1 = SemanticsNode::new(id1, element_id);
        node1.set_properties(SemanticsProperties::new().with_label("First"));
        node1.add_action(SemanticsAction::Tap);

        let mut node2 = SemanticsNode::new(id2, element_id);
        node2.set_properties(
            SemanticsProperties::new()
                .with_label("Second")
                .with_role(SemanticsRole::Button),
        );
        node2.add_action(SemanticsAction::LongPress);

        node1.merge(&node2);

        assert_eq!(node1.properties().label(), Some("First Second"));
        assert_eq!(node1.properties().role(), SemanticsRole::Button);
        assert!(node1.has_action(SemanticsAction::Tap));
        assert!(node1.has_action(SemanticsAction::LongPress));
    }

    #[test]
    fn test_semantics_owner_create_node() {
        let owner = SemanticsOwner::new();
        let element_id = ElementId::new(42);

        let node_id = owner.create_node(element_id);

        assert!(owner.get_node(node_id).is_some());
        assert_eq!(owner.get_node_for_element(element_id), Some(node_id));
        assert_eq!(owner.root(), Some(node_id));
        assert_eq!(owner.node_count(), 1);
    }

    #[test]
    fn test_semantics_owner_remove_node() {
        let owner = SemanticsOwner::new();
        let element_id = ElementId::new(42);

        let node_id = owner.create_node(element_id);
        owner.remove_node(node_id);

        assert!(owner.get_node(node_id).is_none());
        assert!(owner.get_node_for_element(element_id).is_none());
        assert_eq!(owner.node_count(), 0);
    }

    #[test]
    fn test_semantics_owner_parent_child() {
        let owner = SemanticsOwner::new();

        let parent_id = owner.create_node(ElementId::new(1));
        let child_id = owner.create_node(ElementId::new(2));

        owner.set_parent(child_id, parent_id);

        let parent = owner.get_node(parent_id).unwrap();
        let child = owner.get_node(child_id).unwrap();

        assert!(parent.children().contains(&child_id));
        assert_eq!(child.parent(), Some(parent_id));
    }

    #[test]
    fn test_semantics_owner_flush() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let owner = SemanticsOwner::new();
        let update_count = Arc::new(AtomicUsize::new(0));
        let update_count_clone = update_count.clone();

        owner.set_update_callback(Arc::new(move |updates: &[SemanticsData]| {
            update_count_clone.fetch_add(updates.len(), Ordering::SeqCst);
        }));

        let _node_id = owner.create_node(ElementId::new(42));
        owner.flush();

        assert_eq!(update_count.load(Ordering::SeqCst), 1);

        // Flush again should do nothing (no dirty nodes)
        owner.flush();
        assert_eq!(update_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_semantics_handle_drop() {
        let owner = Arc::new(SemanticsOwner::new());
        let element_id = ElementId::new(42);

        let node_id;
        {
            let handle = SemanticsHandle::new(owner.create_node(element_id), owner.clone());
            node_id = handle.node_id();
            assert!(owner.get_node(node_id).is_some());
        }
        // Handle dropped, node should be removed
        assert!(owner.get_node(node_id).is_none());
    }
}
