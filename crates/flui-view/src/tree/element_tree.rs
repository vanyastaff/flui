//! Slab-based Element tree storage.
//!
//! Elements are stored in a Slab for O(1) access by ElementId.
//! This follows Flutter's approach where Elements form the retained tree.

use crate::view::{ElementBase, View};
use flui_foundation::ElementId;
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use slab::Slab;
use std::sync::Arc;

/// A node in the Element tree.
///
/// Contains the Element plus metadata for tree traversal.
pub struct ElementNode {
    /// The actual Element.
    pub(crate) element: Box<dyn ElementBase>,
    /// Parent Element ID (None for root).
    pub(crate) parent: Option<ElementId>,
    /// Depth in the tree (root = 0).
    pub(crate) depth: usize,
    /// Slot index within parent's children.
    pub(crate) slot: usize,
}

impl ElementNode {
    /// Create a new ElementNode.
    pub fn new(element: Box<dyn ElementBase>, parent: Option<ElementId>, slot: usize) -> Self {
        let depth = if parent.is_some() { 1 } else { 0 }; // Will be updated by tree
        Self {
            element,
            parent,
            depth,
            slot,
        }
    }

    /// Get the Element.
    pub fn element(&self) -> &dyn ElementBase {
        &*self.element
    }

    /// Get the Element mutably.
    pub fn element_mut(&mut self) -> &mut dyn ElementBase {
        &mut *self.element
    }

    /// Get the parent ElementId.
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get the depth in the tree.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Get the slot index.
    pub fn slot(&self) -> usize {
        self.slot
    }
}

impl std::fmt::Debug for ElementNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementNode")
            .field("parent", &self.parent)
            .field("depth", &self.depth)
            .field("slot", &self.slot)
            .field("lifecycle", &self.element.lifecycle())
            .finish()
    }
}

/// Slab-based Element tree storage.
///
/// Provides O(1) access to Elements by ElementId.
/// ElementIds use NonZeroUsize (1-based) while Slab uses 0-based indices.
///
/// # Flutter Equivalent
///
/// This roughly corresponds to how Flutter's Element tree is managed,
/// but uses a Slab for efficient allocation/deallocation.
///
/// # Memory Layout
///
/// ```text
/// ElementTree {
///     nodes: Slab<ElementNode>,  // Contiguous storage
///     root: Option<ElementId>,   // Root element
/// }
/// ```
pub struct ElementTree {
    /// Slab storage for element nodes.
    nodes: Slab<ElementNode>,
    /// Root element ID.
    root: Option<ElementId>,
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ElementTree {
    /// Create a new empty ElementTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Create an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    /// Get the root element ID.
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Check if the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the number of elements in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Mount a View as the root of the tree.
    ///
    /// Returns the ElementId of the root element.
    ///
    /// Note: This method does NOT pass PipelineOwner to the element.
    /// For RenderObjectElements that need PipelineOwner, use
    /// `mount_root_with_pipeline_owner` instead.
    pub fn mount_root(&mut self, view: &dyn View) -> ElementId {
        self.mount_root_with_pipeline_owner(view, None)
    }

    /// Mount a View as the root of the tree with PipelineOwner.
    ///
    /// This method passes the PipelineOwner to the root element before mounting,
    /// which is necessary for RenderObjectElements to create their RenderObjects.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this corresponds to `RootWidget.attach(buildOwner, rootElement)`
    /// combined with `_RawViewElement.mount()` which sets up the PipelineOwner.
    ///
    /// # Arguments
    ///
    /// * `view` - The root View to mount
    /// * `pipeline_owner` - Optional PipelineOwner for render tree management
    ///
    /// Returns the ElementId of the root element.
    pub fn mount_root_with_pipeline_owner(
        &mut self,
        view: &dyn View,
        pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    ) -> ElementId {
        let mut element = view.create_element();

        // Pass PipelineOwner to element BEFORE mounting
        // This is critical for RenderObjectElements to create their RenderObjects
        if let Some(ref owner) = pipeline_owner {
            let owner_any: Arc<dyn std::any::Any + Send + Sync> =
                Arc::clone(owner) as Arc<dyn std::any::Any + Send + Sync>;
            element.set_pipeline_owner_any(owner_any);
            tracing::debug!(
                "ElementTree::mount_root_with_pipeline_owner: passed PipelineOwner to root element"
            );
        }

        let node = ElementNode::new(element, None, 0);

        // Slab is 0-indexed, ElementId is 1-indexed
        let slab_index = self.nodes.insert(node);
        let id = ElementId::new(slab_index + 1);

        // Mount the element (now it has PipelineOwner set)
        self.nodes[slab_index].element.mount(None, 0);

        self.root = Some(id);
        id
    }

    /// Insert a new element as a child of the given parent.
    ///
    /// Returns the ElementId of the new element.
    pub fn insert(&mut self, view: &dyn View, parent: ElementId, slot: usize) -> ElementId {
        let element = view.create_element();

        // Get parent depth for calculating child depth
        let parent_depth = self.get(parent).map_or(0, |n| n.depth);

        let mut node = ElementNode::new(element, Some(parent), slot);
        node.depth = parent_depth + 1;

        let slab_index = self.nodes.insert(node);
        let id = ElementId::new(slab_index + 1);

        // Mount the element
        self.nodes[slab_index].element.mount(Some(parent), slot);

        id
    }

    /// Get an element node by ID.
    pub fn get(&self, id: ElementId) -> Option<&ElementNode> {
        let index = id.get() - 1; // Convert 1-based to 0-based
        self.nodes.get(index)
    }

    /// Get an element node mutably by ID.
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut ElementNode> {
        let index = id.get() - 1;
        self.nodes.get_mut(index)
    }

    /// Check if an element exists.
    pub fn contains(&self, id: ElementId) -> bool {
        let index = id.get() - 1;
        self.nodes.contains(index)
    }

    /// Remove an element from the tree.
    ///
    /// This unmounts the element and removes it from storage.
    /// Does NOT automatically remove children - caller must handle that.
    pub fn remove(&mut self, id: ElementId) -> Option<ElementNode> {
        let index = id.get() - 1;

        if self.nodes.contains(index) {
            // Unmount before removing
            self.nodes[index].element.unmount();

            let node = self.nodes.remove(index);

            // Clear root if removing root
            if self.root == Some(id) {
                self.root = None;
            }

            Some(node)
        } else {
            None
        }
    }

    /// Update an element with a new view.
    ///
    /// The view must be compatible (same type) with the existing element.
    pub fn update(&mut self, id: ElementId, view: &dyn View) {
        if let Some(node) = self.get_mut(id) {
            node.element.update(view);
        }
    }

    /// Mark an element as needing rebuild.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.mark_needs_build();
        }
    }

    /// Deactivate an element (temporary removal).
    pub fn deactivate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.deactivate();
        }
    }

    /// Activate an element (re-insertion after deactivation).
    pub fn activate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.activate();
        }
    }

    /// Iterate over all element IDs.
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.nodes
            .iter()
            .map(|(index, _)| ElementId::new(index + 1))
    }

    /// Iterate over all element nodes.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (ElementId, &ElementNode)> + '_ {
        self.nodes.iter().map(|(index, node)| {
            let id = ElementId::new(index + 1);
            (id, node)
        })
    }
}

impl std::fmt::Debug for ElementTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementTree")
            .field("len", &self.nodes.len())
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildContext, StatelessElement, StatelessView, View};

    #[derive(Clone)]
    struct TestView {
        name: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(self.clone())
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(StatelessElement::new(self))
        }
    }

    #[test]
    fn test_tree_creation() {
        let tree = ElementTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_mount_root() {
        let mut tree = ElementTree::new();
        let view = TestView {
            name: "root".to_string(),
        };

        let id = tree.mount_root(&view);

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root(), Some(id));
        assert!(tree.contains(id));
    }

    #[test]
    fn test_insert_child() {
        let mut tree = ElementTree::new();
        let root_view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };

        let root_id = tree.mount_root(&root_view);
        let child_id = tree.insert(&child_view, root_id, 0);

        assert_eq!(tree.len(), 2);
        assert!(tree.contains(child_id));

        let child_node = tree.get(child_id).unwrap();
        assert_eq!(child_node.parent(), Some(root_id));
        assert_eq!(child_node.slot(), 0);
        assert_eq!(child_node.depth(), 1);
    }

    #[test]
    fn test_remove() {
        let mut tree = ElementTree::new();
        let view = TestView {
            name: "test".to_string(),
        };

        let id = tree.mount_root(&view);
        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
        assert!(tree.root().is_none());
    }
}
