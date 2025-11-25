//! Implementation of flui-tree traits for ElementTree
//!
//! This module provides implementations of abstract tree traits from `flui-tree`,
//! enabling `ElementTree` to be used with generic tree algorithms.
//!
//! # Implemented Traits
//!
//! - [`TreeRead`] - Immutable node access
//! - [`TreeNav`] - Parent/child navigation
//! - [`TreeWrite`] - Mutable tree operations
//! - [`TreeWriteNav`] - Tree structure modifications

use flui_foundation::{ElementId, Slot};
use flui_tree::error::{TreeError, TreeResult};
use flui_tree::{TreeNav, TreeRead, TreeWrite, TreeWriteNav};

use super::ElementTree;
use crate::Element;

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
    /// We subtract 1 to convert: `ElementId(1)` → `nodes[0]`
    #[inline]
    fn get(&self, id: ElementId) -> Option<&Element> {
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
            self.nodes.iter().map(|(idx, _)| ElementId::new(idx + 1)),
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
        let node = super::element_tree::ElementNode { element };
        let slab_index = self.nodes.insert(node);
        ElementId::new(slab_index + 1) // Slab index (0-based) → ElementId (1-based)
    }

    /// Removes an element from the tree.
    ///
    /// Note: This only removes the element itself, not its children.
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
        // Validate both elements exist
        if !self.contains(child) {
            return Err(TreeError::not_found(child));
        }

        if let Some(parent_id) = new_parent {
            if !self.contains(parent_id) {
                return Err(TreeError::not_found(parent_id));
            }

            // Check for cycles
            if parent_id == child {
                return Err(TreeError::cycle_detected(child));
            }

            // Check if new_parent is a descendant of child (would create cycle)
            let mut current = Some(parent_id);
            while let Some(id) = current {
                if let Some(p) = self.get(id).and_then(|e| e.parent()) {
                    if p == child {
                        return Err(TreeError::cycle_detected(child));
                    }
                    current = Some(p);
                } else {
                    break;
                }
            }
        }

        // Remove from old parent's children list
        if let Some(old_parent) = self.get(child).and_then(|e| e.parent()) {
            if let Some(parent_elem) = self.get_mut(old_parent) {
                parent_elem.remove_child(child);
            }
        }

        // Update child's parent reference
        if let Some(child_elem) = self.get_mut(child) {
            child_elem.base_mut().set_parent(new_parent);
        }

        // Add to new parent's children list
        if let Some(parent_id) = new_parent {
            if let Some(parent_elem) = self.get_mut(parent_id) {
                parent_elem.add_child(child);
            }
        }

        Ok(())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_element() -> Element {
        Element::empty()
    }

    #[test]
    fn test_tree_read_get() {
        let mut tree = ElementTree::new();
        let id = TreeWrite::insert(&mut tree, test_element());

        let element: Option<&Element> = TreeRead::get(&tree, id);
        assert!(element.is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = ElementTree::new();
        let id = TreeWrite::insert(&mut tree, test_element());

        assert!(TreeRead::contains(&tree, id));
        assert!(!TreeRead::contains(&tree, ElementId::new(999)));
    }

    #[test]
    fn test_tree_nav_parent_children() {
        let mut tree = ElementTree::new();

        let parent_id = TreeWrite::insert(&mut tree, test_element());
        let child_id = TreeWrite::insert(&mut tree, test_element());

        // Set parent-child relationship via TreeWriteNav
        TreeWriteNav::set_parent(&mut tree, child_id, Some(parent_id)).unwrap();

        // Check via TreeNav
        assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
        assert_eq!(TreeNav::children(&tree, parent_id), &[child_id]);
    }

    #[test]
    fn test_tree_write_insert_remove() {
        let mut tree = ElementTree::new();

        let id = TreeWrite::insert(&mut tree, test_element());
        assert_eq!(TreeRead::len(&tree), 1);

        let removed = TreeWrite::remove(&mut tree, id);
        assert!(removed.is_some());
        assert_eq!(TreeRead::len(&tree), 0);
    }

    #[test]
    fn test_tree_write_nav_cycle_detection() {
        let mut tree = ElementTree::new();

        let a = TreeWrite::insert(&mut tree, test_element());
        let b = TreeWrite::insert(&mut tree, test_element());
        let c = TreeWrite::insert(&mut tree, test_element());

        // a → b → c
        TreeWriteNav::set_parent(&mut tree, b, Some(a)).unwrap();
        TreeWriteNav::set_parent(&mut tree, c, Some(b)).unwrap();

        // Try to make a child of c (would create cycle: a → b → c → a)
        let result = TreeWriteNav::set_parent(&mut tree, a, Some(c));
        assert!(result.is_err());
    }

    #[test]
    fn test_tree_write_nav_self_parent() {
        let mut tree = ElementTree::new();
        let id = TreeWrite::insert(&mut tree, test_element());

        // Element cannot be its own parent
        let result = TreeWriteNav::set_parent(&mut tree, id, Some(id));
        assert!(result.is_err());
    }

    #[test]
    fn test_node_ids_iterator() {
        let mut tree = ElementTree::new();

        TreeWrite::insert(&mut tree, test_element());
        TreeWrite::insert(&mut tree, test_element());
        TreeWrite::insert(&mut tree, test_element());

        let ids: Vec<_> = TreeRead::node_ids(&tree).unwrap().collect();
        assert_eq!(ids.len(), 3);

        // IDs should be 1, 2, 3 (1-based)
        assert!(ids.iter().all(|id| id.get() >= 1 && id.get() <= 3));
    }
}
