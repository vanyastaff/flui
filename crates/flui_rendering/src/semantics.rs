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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsNodeId(u64);

impl SemanticsNodeId {
    fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn get(&self) -> u64 {
        self.0
    }
}

// ============================================================================
// SEMANTICS NODE
// ============================================================================

/// A node in the semantics tree.
#[derive(Debug, Clone)]
pub struct SemanticsNode {
    id: SemanticsNodeId,
    render_element: ElementId,
    properties: SemanticsProperties,
    rect: Rect,
    transform: Option<[f32; 16]>,
    actions: Vec<SemanticsAction>,
    parent: Option<SemanticsNodeId>,
    children: Vec<SemanticsNodeId>,
    dirty: bool,
}

impl SemanticsNode {
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

    #[inline]
    pub fn id(&self) -> SemanticsNodeId {
        self.id
    }

    #[inline]
    pub fn render_element(&self) -> ElementId {
        self.render_element
    }

    #[inline]
    pub fn properties(&self) -> &SemanticsProperties {
        &self.properties
    }

    pub fn set_properties(&mut self, properties: SemanticsProperties) {
        self.properties = properties;
        self.dirty = true;
    }

    #[inline]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.dirty = true;
    }

    #[inline]
    pub fn transform(&self) -> Option<&[f32; 16]> {
        self.transform.as_ref()
    }

    pub fn set_transform(&mut self, transform: Option<[f32; 16]>) {
        self.transform = transform;
        self.dirty = true;
    }

    #[inline]
    pub fn actions(&self) -> &[SemanticsAction] {
        &self.actions
    }

    pub fn set_actions(&mut self, actions: Vec<SemanticsAction>) {
        self.actions = actions;
        self.dirty = true;
    }

    pub fn add_action(&mut self, action: SemanticsAction) {
        if !self.actions.contains(&action) {
            self.actions.push(action);
            self.dirty = true;
        }
    }

    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains(&action)
    }

    #[inline]
    pub fn parent(&self) -> Option<SemanticsNodeId> {
        self.parent
    }

    #[inline]
    pub fn children(&self) -> &[SemanticsNodeId] {
        &self.children
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn to_data(&self) -> SemanticsData {
        SemanticsData::new(self.properties.clone(), self.rect)
    }

    pub fn has_content(&self) -> bool {
        self.properties.has_label()
            || self.properties.has_value()
            || self.properties.has_hint()
            || self.properties.role() != SemanticsRole::None
            || !self.actions.is_empty()
    }

    pub fn merge(&mut self, other: &SemanticsNode) {
        if let Some(other_label) = other.properties.label() {
            if let Some(self_label) = &self.properties.label {
                self.properties.label = Some(format!("{} {}", self_label, other_label));
            } else {
                self.properties.label = Some(other_label.to_string());
            }
        }

        if other.properties.has_value() && !self.properties.has_value() {
            self.properties.value = other.properties.value.clone();
        }

        if other.properties.has_hint() && !self.properties.has_hint() {
            self.properties.hint = other.properties.hint.clone();
        }

        if self.properties.role() == SemanticsRole::None
            && other.properties.role() != SemanticsRole::None
        {
            self.properties.role = other.properties.role();
        }

        for action in &other.actions {
            self.add_action(*action);
        }

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
pub struct SemanticsOwner {
    next_id: AtomicU64,
    nodes: RwLock<HashMap<SemanticsNodeId, SemanticsNode>>,
    element_to_node: RwLock<HashMap<ElementId, SemanticsNodeId>>,
    root: RwLock<Option<SemanticsNodeId>>,
    dirty_nodes: RwLock<Vec<SemanticsNodeId>>,
    update_callback: RwLock<Option<SemanticsUpdateCallback>>,
}

/// Callback type for platform semantics updates.
pub type SemanticsUpdateCallback = Arc<dyn Fn(&[SemanticsData]) + Send + Sync>;

impl SemanticsOwner {
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

    pub fn set_update_callback(&self, callback: SemanticsUpdateCallback) {
        *self.update_callback.write() = Some(callback);
    }

    pub fn clear_update_callback(&self) {
        *self.update_callback.write() = None;
    }

    pub fn create_node(&self, render_element: ElementId) -> SemanticsNodeId {
        let id = SemanticsNodeId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let node = SemanticsNode::new(id, render_element);

        self.nodes.write().insert(id, node);
        self.element_to_node.write().insert(render_element, id);
        self.dirty_nodes.write().push(id);

        let mut root = self.root.write();
        if root.is_none() {
            *root = Some(id);
        }

        id
    }

    pub fn remove_node(&self, id: SemanticsNodeId) {
        let mut nodes = self.nodes.write();

        if let Some(node) = nodes.remove(&id) {
            self.element_to_node.write().remove(&node.render_element);

            if let Some(parent_id) = node.parent {
                if let Some(parent) = nodes.get_mut(&parent_id) {
                    parent.children.retain(|&child| child != id);
                }
            }

            let mut root = self.root.write();
            if *root == Some(id) {
                *root = None;
            }
        }

        self.dirty_nodes.write().retain(|&n| n != id);
    }

    pub fn get_node(&self, id: SemanticsNodeId) -> Option<SemanticsNode> {
        self.nodes.read().get(&id).cloned()
    }

    pub fn get_node_for_element(&self, element_id: ElementId) -> Option<SemanticsNodeId> {
        self.element_to_node.read().get(&element_id).copied()
    }

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

    pub fn set_parent(&self, child_id: SemanticsNodeId, parent_id: SemanticsNodeId) {
        let mut nodes = self.nodes.write();

        if let Some(child) = nodes.get(&child_id) {
            if let Some(old_parent_id) = child.parent {
                if let Some(old_parent) = nodes.get_mut(&old_parent_id) {
                    old_parent.children.retain(|&c| c != child_id);
                }
            }
        }

        if let Some(child) = nodes.get_mut(&child_id) {
            child.parent = Some(parent_id);
        }

        if let Some(parent) = nodes.get_mut(&parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }
    }

    pub fn root(&self) -> Option<SemanticsNodeId> {
        *self.root.read()
    }

    pub fn flush(&self) {
        let dirty_ids: Vec<_> = {
            let mut dirty = self.dirty_nodes.write();
            std::mem::take(&mut *dirty)
        };

        if dirty_ids.is_empty() {
            return;
        }

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

        if let Some(callback) = &*self.update_callback.read() {
            callback(&updates);
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    pub fn clear(&self) {
        self.nodes.write().clear();
        self.element_to_node.write().clear();
        *self.root.write() = None;
        self.dirty_nodes.write().clear();
    }

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

// ============================================================================
// SEMANTICS HANDLE
// ============================================================================

/// Handle for managing a semantics node's lifecycle.
#[derive(Debug)]
pub struct SemanticsHandle {
    node_id: SemanticsNodeId,
    owner: Arc<SemanticsOwner>,
}

impl SemanticsHandle {
    pub fn new(node_id: SemanticsNodeId, owner: Arc<SemanticsOwner>) -> Self {
        Self { node_id, owner }
    }

    pub fn node_id(&self) -> SemanticsNodeId {
        self.node_id
    }

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
    fn test_semantics_handle_drop() {
        let owner = Arc::new(SemanticsOwner::new());
        let element_id = ElementId::new(42);

        let node_id;
        {
            let handle = SemanticsHandle::new(owner.create_node(element_id), owner.clone());
            node_id = handle.node_id();
            assert!(owner.get_node(node_id).is_some());
        }
        assert!(owner.get_node(node_id).is_none());
    }
}
