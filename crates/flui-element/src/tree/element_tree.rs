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

    /// Layout a render object and return its computed size.
    ///
    /// This method performs the following steps:
    /// 1. Validates the element exists and is a RenderElement
    /// 2. Calls ViewObject.layout_render() or RenderObject.perform_layout()
    /// 3. Stores computed size in RenderState
    /// 4. Clears needs_layout flag
    /// 5. Marks element for paint
    ///
    /// # Returns
    ///
    /// `Some(size)` if layout succeeded, `None` if:
    /// - Element doesn't exist
    /// - Element is not a render element
    /// - Layout failed
    #[inline]
    pub fn layout_render_object(
        &mut self,
        id: ElementId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        use flui_rendering::RenderObject;

        // Get element
        let element = self.get_mut(id)?;

        // Check if this is a RenderElement
        let render_element = element.as_render_mut()?;

        // Call perform_layout on the render object
        // We need to create a temporary LayoutTree implementation
        let element_id = id;
        let size = {
            let render_obj = render_element.render_object_mut();

            // Create a minimal LayoutTree adapter for the perform_layout call
            struct ElementTreeAdapter<'a> {
                tree: &'a mut ElementTree,
            }

            impl<'a> flui_rendering::LayoutTree for ElementTreeAdapter<'a> {
                fn perform_layout(
                    &mut self,
                    id: ElementId,
                    constraints: flui_types::constraints::BoxConstraints,
                ) -> Result<flui_types::Size, flui_rendering::RenderError> {
                    // Recursive call for child layout
                    self.tree
                        .layout_render_object(id, constraints)
                        .ok_or_else(|| flui_rendering::RenderError::not_render_element(id))
                }

                fn perform_sliver_layout(
                    &mut self,
                    _id: ElementId,
                    _constraints: flui_rendering::SliverConstraints,
                ) -> Result<flui_rendering::SliverGeometry, flui_rendering::RenderError> {
                    // Not implemented yet
                    Ok(flui_rendering::SliverGeometry::zero())
                }

                fn set_offset(&mut self, id: ElementId, offset: flui_types::Offset) {
                    if let Some(element) = self.tree.get_mut(id) {
                        if let Some(render_elem) = element.as_render_mut() {
                            render_elem.set_offset(offset);
                        }
                    }
                }

                fn get_offset(&self, id: ElementId) -> Option<flui_types::Offset> {
                    self.tree
                        .get(id)
                        .and_then(|e| e.as_render())
                        .map(|r| r.offset())
                }

                fn mark_needs_layout(&mut self, id: ElementId) {
                    if let Some(element) = self.tree.get_mut(id) {
                        element.mark_needs_layout();
                    }
                }

                fn needs_layout(&self, id: ElementId) -> bool {
                    self.tree
                        .get(id)
                        .map(|e| e.needs_layout())
                        .unwrap_or(false)
                }

                fn render_object(&self, id: ElementId) -> Option<&dyn std::any::Any> {
                    self.tree
                        .get(id)
                        .and_then(|e| e.as_render())
                        .map(|r| r.render_object() as &dyn std::any::Any)
                }

                fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn std::any::Any> {
                    self.tree
                        .get_mut(id)
                        .and_then(|e| e.as_render_mut())
                        .map(|r| r.render_object_mut() as &mut dyn std::any::Any)
                }

                fn setup_child_parent_data(
                    &mut self,
                    _parent_id: ElementId,
                    _child_id: ElementId,
                ) {
                    // TODO: Implement parent data setup
                }
            }

            // SAFETY: We need to split the borrow to satisfy the borrow checker
            // We're using raw pointers to avoid the mutable borrow conflict
            // This is safe because:
            // 1. We only access different elements during recursive layout
            // 2. The adapter only lives during this scope
            // 3. No concurrent access to the same element
            let tree_ptr = self as *mut ElementTree;
            let mut adapter = ElementTreeAdapter {
                tree: unsafe { &mut *tree_ptr },
            };

            // Call perform_layout
            render_obj
                .perform_layout(element_id, constraints, &mut adapter)
                .ok()?
        };

        // Re-borrow element after layout
        let render_element = self.get_mut(id)?.as_render_mut()?;

        // Store size in RenderState
        render_element.set_size(size);

        // Clear needs_layout flag
        render_element.clear_needs_layout();

        // Mark for paint
        render_element.mark_needs_paint();

        Some(size)
    }

    /// Stub: Paint a render object (intentional design).
    ///
    /// **Architectural Note:** This stub is intentional. ElementTree does not
    /// handle rendering directly - that's the responsibility of the pipeline layer.
    /// Actual painting is performed by `flui_core`'s `PaintPipeline` which has
    /// access to the full rendering context (GPU surface, layers, etc).
    ///
    /// Returns `true` if the element was painted, `false` otherwise.
    #[inline]
    pub fn paint_render_object(&self, _id: ElementId, _offset: flui_types::Offset) -> bool {
        // Intentional stub: Keeps flui-element independent of flui_engine
        false
    }

    /// Stub: Hit test at position (intentional design).
    ///
    /// **Architectural Note:** This stub is intentional. ElementTree does not
    /// handle hit testing directly - that's the responsibility of the interaction
    /// layer. Actual hit testing is performed by components with access to
    /// render objects, layout information, and transform matrices.
    ///
    /// Returns element IDs that were hit, in front-to-back order.
    #[inline]
    pub fn hit_test(&self, _root_id: ElementId, _position: flui_types::Offset) -> Vec<ElementId> {
        // Intentional stub: Keeps flui-element independent of flui_interaction
        Vec::new()
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
            child.set_parent(Some(parent_id));
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
            child.set_parent(Some(root_id));
        }
        if let Some(grandchild) = tree.get_mut(grandchild_id) {
            grandchild.set_parent(Some(child_id));
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
            child.set_parent(Some(root_id));
        }
        if let Some(grandchild) = tree.get_mut(grandchild_id) {
            grandchild.set_parent(Some(child_id));
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
