//! Implementation of flui-tree traits for ElementTree.
//!
//! This module provides implementations of the abstract tree traits from `flui-tree`,
//! enabling `ElementTree` to be used with generic tree algorithms.
//!
//! # Implemented Traits
//!
//! - [`TreeRead`] - Immutable node access
//! - [`TreeNav`] - Parent/child navigation
//! - [`TreeWrite`] - Mutable tree operations
//! - [`TreeWriteNav`] - Tree structure modifications
//! - [`RenderTreeAccess`] - Access to RenderObject and RenderState
//! - [`DirtyTracking`] - Layout/paint dirty flag management
//!
//! # Architecture
//!
//! By implementing these traits, ElementTree becomes compatible with:
//! - Generic layout algorithms in `flui-rendering`
//! - Tree iterators from `flui-tree`
//! - Mock trees for testing
//!
//! This breaks the circular dependency between `flui-core` and `flui-rendering`.

use std::any::Any;

use flui_foundation::{ElementId, Slot};
use flui_tree::error::{TreeError, TreeResult};
use flui_tree::{DirtyTracking, RenderTreeAccess, TreeNav, TreeRead, TreeWrite, TreeWriteNav};

use super::element_tree::ElementNode;
use super::{Element, ElementTree};

// ============================================================================
// TreeRead Implementation
// ============================================================================

impl TreeRead for ElementTree {
    type Node = Element;

    /// Returns a reference to the element with the given ID.
    ///
    /// # Slab Offset Pattern
    ///
    /// ElementId is 1-based (NonZeroUsize), while Slab uses 0-based indexing.
    /// We subtract 1 to convert: `ElementId(1) → nodes[0]`
    #[inline]
    fn get(&self, id: ElementId) -> Option<&Element> {
        // CRITICAL: Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes.get(id.get() - 1).map(|node| &node.element)
    }

    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    #[inline]
    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn node_ids(&self) -> Option<Box<dyn Iterator<Item = ElementId> + '_>> {
        Some(Box::new(
            self.nodes.iter().map(|(idx, _)| ElementId::new(idx + 1)), // Slab index (0-based) → ElementId (1-based)
        ))
    }
}

// ============================================================================
// TreeNav Implementation
// ============================================================================

impl TreeNav for ElementTree {
    /// Returns the parent of the given element.
    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent()
    }

    /// Returns the children of the given element.
    ///
    /// Returns an empty slice if the element has no children or doesn't exist.
    #[inline]
    fn children(&self, id: ElementId) -> &[ElementId] {
        self.get(id).map(|e| e.children()).unwrap_or(&[])
    }

    /// Returns the slot of the given element within its parent.
    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        self.get(id)?.slot()
    }
}

// ============================================================================
// TreeWrite Implementation
// ============================================================================

impl TreeWrite for ElementTree {
    /// Returns a mutable reference to the element with the given ID.
    #[inline]
    fn get_mut(&mut self, id: ElementId) -> Option<&mut Element> {
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
    /// Slab returns 0-based index, we add 1 to create ElementId (1-based).
    fn insert(&mut self, element: Element) -> ElementId {
        let node = ElementNode { element };
        let slab_index = self.nodes.insert(node);
        ElementId::new(slab_index + 1) // Slab index (0-based) → ElementId (1-based)
    }

    /// Removes an element from the tree.
    ///
    /// Note: This only removes the element itself, not its children.
    /// Use `ElementTree::remove()` for recursive removal with lifecycle callbacks.
    fn remove(&mut self, id: ElementId) -> Option<Element> {
        self.nodes.try_remove(id.get() - 1).map(|node| node.element)
    }

    /// Clears all elements from the tree.
    fn clear(&mut self) {
        self.nodes.clear();
    }

    /// Reserves capacity for additional elements.
    fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }
}

// ============================================================================
// TreeWriteNav Implementation
// ============================================================================

impl TreeWriteNav for ElementTree {
    /// Sets the parent of a child element.
    ///
    /// This method:
    /// 1. Validates no cycles would be created
    /// 2. Removes child from old parent's children list
    /// 3. Updates child's parent reference
    /// 4. Adds child to new parent's children list
    fn set_parent(&mut self, child: ElementId, new_parent: Option<ElementId>) -> TreeResult<()> {
        // Validate child exists
        if !self.contains(child) {
            return Err(TreeError::not_found(child));
        }

        // Validate new parent exists (if specified)
        if let Some(parent_id) = new_parent {
            if !self.contains(parent_id) {
                return Err(TreeError::not_found(parent_id));
            }

            // Check for cycles: new parent cannot be descendant of child
            if self.is_descendant(parent_id, child) || parent_id == child {
                return Err(TreeError::cycle_detected(child));
            }
        }

        // Get old parent
        let old_parent = self.parent(child);

        // Remove from old parent's children
        if let Some(old_parent_id) = old_parent {
            if let Some(parent_elem) = self.get_mut(old_parent_id) {
                parent_elem.forget_child(child);
            }
        }

        // Update child's parent reference
        if let Some(child_elem) = self.get_mut(child) {
            child_elem.mount(new_parent, None);
        }

        // Add to new parent's children
        if let Some(parent_id) = new_parent {
            if let Some(parent_elem) = self.get_mut(parent_id) {
                parent_elem.add_child(child);
            }
        }

        Ok(())
    }
}

// ============================================================================
// RenderTreeAccess Implementation
// ============================================================================

impl RenderTreeAccess for ElementTree {
    /// Returns the RenderObject for an element as `dyn Any`.
    ///
    /// Only render elements (RenderBox/RenderSliver) have RenderObjects.
    /// Returns `None` for component elements (Stateless, Stateful, etc.).
    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        let element = self.get(id)?;
        // Element::render_object() returns Option<&dyn RenderObject>
        // RenderObject trait has as_any() method
        element.render_object().map(|ro| ro.as_any())
    }

    /// Returns a mutable reference to the RenderObject as `dyn Any`.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        let element = self.get_mut(id)?;
        element.render_object_mut().map(|ro| ro.as_any_mut())
    }

    /// Returns the RenderState for an element as `dyn Any`.
    ///
    /// Only render elements have RenderState.
    fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
        let element = self.get(id)?;
        element.render_state().map(|rs| rs as &dyn Any)
    }

    /// Returns a mutable reference to the RenderState as `dyn Any`.
    fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        let element = self.get_mut(id)?;
        element.render_state_mut().map(|rs| rs as &mut dyn Any)
    }

    /// Returns the cached size from RenderState.
    fn get_size(&self, id: ElementId) -> Option<(f32, f32)> {
        let element = self.get(id)?;
        let render_state = element.render_state()?;
        let size = render_state.size();
        Some((size.width, size.height))
    }

    /// Returns the cached offset from RenderState.
    fn get_offset(&self, id: ElementId) -> Option<(f32, f32)> {
        let element = self.get(id)?;
        let render_state = element.render_state()?;
        let offset = render_state.offset();
        Some((offset.dx, offset.dy))
    }
}

// ============================================================================
// DirtyTracking Implementation
// ============================================================================

impl DirtyTracking for ElementTree {
    /// Marks an element as needing layout.
    ///
    /// This sets the needs_layout flag on the element's RenderState.
    /// For non-render elements, this is a no-op.
    fn mark_needs_layout(&self, id: ElementId) {
        // Note: DirtyTracking takes &self for thread-safety
        // We need interior mutability here - RenderState uses atomic flags
        if let Some(element) = self.get(id) {
            if let Some(render_state) = element.render_state() {
                render_state.mark_needs_layout();
            }
        }
    }

    /// Marks an element as needing paint.
    fn mark_needs_paint(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(render_state) = element.render_state() {
                render_state.mark_needs_paint();
            }
        }
    }

    /// Clears the needs-layout flag.
    fn clear_needs_layout(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(render_state) = element.render_state() {
                render_state.clear_needs_layout();
            }
        }
    }

    /// Clears the needs-paint flag.
    fn clear_needs_paint(&self, id: ElementId) {
        if let Some(element) = self.get(id) {
            if let Some(render_state) = element.render_state() {
                render_state.clear_needs_paint();
            }
        }
    }

    /// Returns true if the element needs layout.
    fn needs_layout(&self, id: ElementId) -> bool {
        self.get(id)
            .and_then(|e| e.render_state())
            .map(|rs| rs.needs_layout())
            .unwrap_or(false)
    }

    /// Returns true if the element needs paint.
    fn needs_paint(&self, id: ElementId) -> bool {
        self.get(id)
            .and_then(|e| e.render_state())
            .map(|rs| rs.needs_paint())
            .unwrap_or(false)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::wrappers::StatelessViewWrapper;
    use crate::view::EmptyView;

    fn create_test_element() -> Element {
        Element::new(Box::new(StatelessViewWrapper::new(EmptyView)))
    }

    #[test]
    fn test_tree_read_get() {
        let mut tree = ElementTree::new();
        let elem = create_test_element();
        let id = tree.insert(elem);

        assert!(tree.get(id).is_some());
        assert!(tree.get(ElementId::new(999)).is_none());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = ElementTree::new();
        let elem = create_test_element();
        let id = tree.insert(elem);

        assert!(tree.contains(id));
        assert!(!tree.contains(ElementId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = ElementTree::new();
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());

        tree.insert(create_test_element());
        tree.insert(create_test_element());

        assert_eq!(tree.len(), 2);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_tree_read_node_ids() {
        let mut tree = ElementTree::new();
        let id1 = tree.insert(create_test_element());
        let id2 = tree.insert(create_test_element());

        let ids: Vec<_> = tree.node_ids().unwrap().collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_tree_nav_parent_children() {
        let mut tree = ElementTree::new();

        // Create parent
        let parent_id = tree.insert(create_test_element());

        // Create child with parent
        let mut child = create_test_element();
        child.mount(Some(parent_id), None);
        let child_id = tree.insert(child);

        // Add child to parent's children list
        if let Some(parent) = tree.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Test navigation
        assert_eq!(tree.parent(parent_id), None);
        assert_eq!(tree.parent(child_id), Some(parent_id));
        assert_eq!(tree.children(parent_id), &[child_id]);
        assert!(tree.children(child_id).is_empty());
    }

    #[test]
    fn test_tree_write_remove() {
        let mut tree = ElementTree::new();
        let id = tree.insert(create_test_element());

        assert!(tree.contains(id));

        let removed = TreeWrite::remove(&mut tree, id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_tree_write_clear() {
        let mut tree = ElementTree::new();
        tree.insert(create_test_element());
        tree.insert(create_test_element());

        assert_eq!(tree.len(), 2);

        TreeWrite::clear(&mut tree);
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_tree_write_nav_set_parent() {
        let mut tree = ElementTree::new();

        let parent_id = tree.insert(create_test_element());
        let child_id = tree.insert(create_test_element());

        // Set parent
        let result = tree.set_parent(child_id, Some(parent_id));
        assert!(result.is_ok());

        // Verify
        assert_eq!(tree.parent(child_id), Some(parent_id));
        assert!(tree.children(parent_id).contains(&child_id));
    }

    #[test]
    fn test_tree_write_nav_cycle_detection() {
        let mut tree = ElementTree::new();

        let parent_id = tree.insert(create_test_element());
        let child_id = tree.insert(create_test_element());

        // Setup: parent -> child
        tree.set_parent(child_id, Some(parent_id)).unwrap();

        // Try to create cycle: child -> parent (should fail)
        let result = tree.set_parent(parent_id, Some(child_id));
        assert!(result.is_err());
    }

    #[test]
    fn test_tree_iterators() {
        let mut tree = ElementTree::new();

        let root = tree.insert(create_test_element());
        let child = tree.insert(create_test_element());
        let grandchild = tree.insert(create_test_element());

        tree.set_parent(child, Some(root)).unwrap();
        tree.set_parent(grandchild, Some(child)).unwrap();

        // Test ancestors
        let ancestors: Vec<_> = tree.ancestors(grandchild).collect();
        assert_eq!(ancestors, vec![grandchild, child, root]);

        // Test descendants
        let descendants: Vec<_> = tree.descendants(root).collect();
        assert_eq!(descendants.len(), 3);
        assert!(descendants.contains(&root));
        assert!(descendants.contains(&child));
        assert!(descendants.contains(&grandchild));

        // Test depth
        assert_eq!(tree.depth(root), 0);
        assert_eq!(tree.depth(child), 1);
        assert_eq!(tree.depth(grandchild), 2);

        // Test is_descendant
        assert!(tree.is_descendant(grandchild, root));
        assert!(tree.is_descendant(child, root));
        assert!(!tree.is_descendant(root, grandchild));
    }
}
