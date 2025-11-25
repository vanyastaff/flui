//! ElementTree - Slab-based element storage with tree operations
//!
//! This module provides `ElementTree`, the central data structure for storing
//! and managing elements in a FLUI application.
//!
//! # Architecture
//!
//! ```text
//! ElementTree
//!   ├─ nodes: Slab<ElementNode>  (O(1) access by ElementId)
//!   └─ root: Option<ElementId>   (root of the tree)
//! ```
//!
//! # Slab Offset Pattern
//!
//! ElementId uses 1-based indexing (NonZeroUsize), while Slab uses 0-based:
//! - `ElementId(1)` → `nodes[0]`
//! - `ElementId(2)` → `nodes[1]`
//! - etc.

use slab::Slab;

use flui_foundation::ElementId;

use crate::Element;

/// Internal node wrapper for slab storage
#[derive(Debug)]
pub(crate) struct ElementNode {
    pub(crate) element: Element,
}

/// ElementTree - Slab-based storage for elements
///
/// Provides O(1) element access by ElementId and tree navigation operations.
///
/// # Thread Safety
///
/// ElementTree itself is not thread-safe. Use `Arc<RwLock<ElementTree>>`
/// for multi-threaded access.
#[derive(Debug)]
pub struct ElementTree {
    /// Slab storage for elements (0-based indexing internally)
    pub(crate) nodes: Slab<ElementNode>,

    /// Root element ID (None if tree is empty)
    root: Option<ElementId>,
}

impl ElementTree {
    /// Creates a new empty ElementTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root element ID.
    #[inline]
    #[must_use]
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Set the root element ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ElementId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if an element exists in the tree.
    #[inline]
    #[must_use]
    pub fn contains(&self, id: ElementId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of elements in the tree.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns a reference to an element.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `ElementId(1)` → `nodes[0]`
    #[inline]
    #[must_use]
    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.nodes.get(id.get() - 1).map(|node| &node.element)
    }

    /// Returns a mutable reference to an element.
    #[inline]
    #[must_use]
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Element> {
        self.nodes
            .get_mut(id.get() - 1)
            .map(|node| &mut node.element)
    }

    /// Inserts an element into the tree.
    ///
    /// Returns the ElementId of the inserted element.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `ElementId(1)`
    pub fn insert(&mut self, element: Element) -> ElementId {
        let node = ElementNode { element };
        let slab_index = self.nodes.insert(node);
        ElementId::new(slab_index + 1) // 0-based → 1-based
    }

    /// Removes an element from the tree.
    ///
    /// Returns the removed element, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Use `remove_recursive` for that.
    pub fn remove(&mut self, id: ElementId) -> Option<Element> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes.try_remove(id.get() - 1).map(|node| node.element)
    }

    /// Removes an element and all its descendants recursively.
    ///
    /// Returns the number of elements removed.
    pub fn remove_recursive(&mut self, id: ElementId) -> usize {
        let mut count = 0;

        // Get children first
        let children: Vec<ElementId> = self
            .get(id)
            .map(|e| e.children().to_vec())
            .unwrap_or_default();

        // Remove children recursively
        for child_id in children {
            count += self.remove_recursive(child_id);
        }

        // Remove the element itself
        if self.remove(id).is_some() {
            count += 1;
        }

        count
    }

    /// Clears all elements from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for additional elements.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========== Tree Navigation ==========

    /// Returns the parent of an element.
    #[inline]
    #[must_use]
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent()
    }

    /// Returns the children of an element.
    #[inline]
    #[must_use]
    pub fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id).map(|e| e.children()).unwrap_or(&[])
    }

    /// Returns the depth of an element in the tree.
    ///
    /// Root has depth 0.
    pub fn depth(&self, id: ElementId) -> Option<usize> {
        if !self.contains(id) {
            return None;
        }

        let mut depth = 0;
        let mut current = id;

        while let Some(parent) = self.parent(current) {
            depth += 1;
            current = parent;
        }

        Some(depth)
    }

    /// Checks if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: ElementId, descendant: ElementId) -> bool {
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
    pub fn is_descendant(&self, descendant: ElementId, ancestor: ElementId) -> bool {
        self.is_ancestor(ancestor, descendant)
    }

    // ========== Iteration ==========

    /// Returns an iterator over all element IDs.
    pub fn ids(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.nodes.iter().map(|(idx, _)| ElementId::new(idx + 1))
    }

    /// Returns an iterator over all elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> + '_ {
        self.nodes.iter().map(|(_, node)| &node.element)
    }

    /// Returns a mutable iterator over all elements.
    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> + '_ {
        self.nodes.iter_mut().map(|(_, node)| &mut node.element)
    }

    /// Returns an iterator over (ElementId, &Element) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (ElementId, &Element)> + '_ {
        self.nodes
            .iter()
            .map(|(idx, node)| (ElementId::new(idx + 1), &node.element))
    }

    /// Returns a mutable iterator over (ElementId, &mut Element) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (ElementId, &mut Element)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(idx, node)| (ElementId::new(idx + 1), &mut node.element))
    }

    // ========== Compatibility Stubs ==========
    // These methods provide API compatibility with the old element module.

    /// Alias for root() - returns the root element ID.
    #[inline]
    #[must_use]
    pub fn root_id(&self) -> Option<ElementId> {
        self.root
    }

    /// Returns an iterator over all element IDs (alias for ids()).
    pub fn all_element_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.ids()
    }

    /// Visit all elements mutably with a callback.
    ///
    /// The callback receives (ElementId, &mut Element) for each element.
    pub fn visit_all_elements_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(ElementId, &mut Element),
    {
        for (idx, node) in self.nodes.iter_mut() {
            f(ElementId::new(idx + 1), &mut node.element);
        }
    }

    /// Stub: Layout a render object (returns None - not implemented).
    ///
    /// Actual layout should be performed through the pipeline system.
    #[inline]
    pub fn layout_render_object(
        &mut self,
        _id: ElementId,
        _constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        // TODO: Implement proper layout delegation
        None
    }

    /// Stub: Paint a render object (returns None - not implemented).
    ///
    /// Actual painting should be performed through the pipeline system.
    #[inline]
    pub fn paint_render_object(
        &self,
        _id: ElementId,
        _offset: flui_types::Offset,
    ) -> Option<flui_engine::CanvasLayer> {
        // TODO: Implement proper paint delegation
        None
    }

    /// Stub: Hit test at position (returns empty result).
    ///
    /// Actual hit testing should be performed through the interaction layer.
    #[inline]
    pub fn hit_test(
        &self,
        _root_id: ElementId,
        _position: flui_types::Offset,
    ) -> flui_interaction::HitTestResult {
        flui_interaction::HitTestResult::new()
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create test elements
    fn test_element() -> Element {
        Element::empty()
    }

    #[test]
    fn test_tree_creation() {
        let tree = ElementTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_insert_and_get() {
        let mut tree = ElementTree::new();

        let id = tree.insert(test_element());
        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert!(tree.get(id).is_some());
    }

    #[test]
    fn test_element_id_offset() {
        let mut tree = ElementTree::new();

        // First element should have ID 1 (not 0)
        let id1 = tree.insert(test_element());
        assert_eq!(id1.get(), 1);

        let id2 = tree.insert(test_element());
        assert_eq!(id2.get(), 2);

        // Both should be accessible
        assert!(tree.get(id1).is_some());
        assert!(tree.get(id2).is_some());
    }

    #[test]
    fn test_remove() {
        let mut tree = ElementTree::new();

        let id = tree.insert(test_element());
        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_root_management() {
        let mut tree = ElementTree::new();

        let id = tree.insert(test_element());
        assert!(tree.root().is_none());

        tree.set_root(Some(id));
        assert_eq!(tree.root(), Some(id));

        tree.set_root(None);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_parent_children() {
        let mut tree = ElementTree::new();

        let parent_id = tree.insert(test_element());
        let child_id = tree.insert(test_element());

        // Set up parent-child relationship
        if let Some(child) = tree.get_mut(child_id) {
            child.base_mut().set_parent(Some(parent_id));
        }
        if let Some(parent) = tree.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Verify relationships
        assert_eq!(tree.parent(child_id), Some(parent_id));
        assert_eq!(tree.children(parent_id), &[child_id]);
    }

    #[test]
    fn test_depth() {
        let mut tree = ElementTree::new();

        let root_id = tree.insert(test_element());
        let child_id = tree.insert(test_element());
        let grandchild_id = tree.insert(test_element());

        // Set up relationships
        if let Some(child) = tree.get_mut(child_id) {
            child.base_mut().set_parent(Some(root_id));
        }
        if let Some(grandchild) = tree.get_mut(grandchild_id) {
            grandchild.base_mut().set_parent(Some(child_id));
        }

        assert_eq!(tree.depth(root_id), Some(0));
        assert_eq!(tree.depth(child_id), Some(1));
        assert_eq!(tree.depth(grandchild_id), Some(2));
    }

    #[test]
    fn test_is_ancestor() {
        let mut tree = ElementTree::new();

        let root_id = tree.insert(test_element());
        let child_id = tree.insert(test_element());
        let grandchild_id = tree.insert(test_element());

        // Set up relationships
        if let Some(child) = tree.get_mut(child_id) {
            child.base_mut().set_parent(Some(root_id));
        }
        if let Some(grandchild) = tree.get_mut(grandchild_id) {
            grandchild.base_mut().set_parent(Some(child_id));
        }

        assert!(tree.is_ancestor(root_id, child_id));
        assert!(tree.is_ancestor(root_id, grandchild_id));
        assert!(tree.is_ancestor(child_id, grandchild_id));
        assert!(!tree.is_ancestor(child_id, root_id));
        assert!(!tree.is_ancestor(grandchild_id, root_id));
    }

    #[test]
    fn test_iteration() {
        let mut tree = ElementTree::new();

        tree.insert(test_element());
        tree.insert(test_element());
        tree.insert(test_element());

        let ids: Vec<_> = tree.ids().collect();
        assert_eq!(ids.len(), 3);

        let elements: Vec<_> = tree.elements().collect();
        assert_eq!(elements.len(), 3);
    }

    #[test]
    fn test_clear() {
        let mut tree = ElementTree::new();

        let id = tree.insert(test_element());
        tree.set_root(Some(id));
        tree.insert(test_element());

        assert_eq!(tree.len(), 2);

        tree.clear();

        assert!(tree.is_empty());
        assert!(tree.root().is_none());
    }
}
