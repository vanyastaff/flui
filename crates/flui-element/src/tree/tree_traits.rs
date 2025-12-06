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
//! - [`RenderTreeAccess`] - Render-specific data access (stub implementation)
//! - [`Lifecycle`] - Element lifecycle management (for Element)
//! - [`DepthTracking`] - Depth tracking (for Element)

use flui_foundation::{ElementId, Slot};
use flui_tree::error::{TreeError, TreeResult};
use flui_tree::{sealed, DepthTracking, Lifecycle, TreeNav, TreeRead, TreeWrite, TreeWriteNav};
use smallvec::SmallVec;

use super::ElementTree;
use crate::Element;

// ============================================================================
// Sealed Trait Implementations
// ============================================================================

impl sealed::TreeReadSealed for ElementTree {}
impl sealed::TreeNavSealed for ElementTree {}

// ============================================================================
// Iterator Wrappers
// ============================================================================

/// Zero-cost wrapper for node ID iterator.
///
/// Wraps the internal Slab iterator without exposing private types.
/// Performance: Same as direct iteration, no overhead.
pub struct NodeIdIter<'a> {
    inner: slab::Iter<'a, super::element_tree::ElementNode>,
}

impl<'a> NodeIdIter<'a> {
    fn new(iter: slab::Iter<'a, super::element_tree::ElementNode>) -> Self {
        Self { inner: iter }
    }
}

impl Iterator for NodeIdIter<'_> {
    type Item = ElementId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(idx, _)| ElementId::new(idx + 1))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for NodeIdIter<'_> {
    #[inline]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

// ============================================================================
// TreeRead Implementation
// ============================================================================

impl TreeRead for ElementTree {
    type Node = Element;

    /// Zero-cost iterator over element IDs using GAT.
    ///
    /// Maps Slab indices (0-based) to ElementIds (1-based) without heap allocation.
    type NodeIter<'a>
        = NodeIdIter<'a>
    where
        Self: 'a;

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

    /// Returns zero-cost iterator over all element IDs.
    ///
    /// Performance: Wrapper around Slab iterator, no heap allocation.
    fn node_ids(&self) -> Self::NodeIter<'_> {
        NodeIdIter::new(self.nodes.iter())
    }
}

// ============================================================================
// TreeNav Implementation
// ============================================================================

impl TreeNav for ElementTree {
    /// Zero-cost iterator over children using GAT.
    ///
    /// Uses Flatten + Option to avoid heap allocation while supporting empty case.
    type ChildrenIter<'a>
        = std::iter::Flatten<
            std::option::IntoIter<std::iter::Copied<std::slice::Iter<'a, ElementId>>>,
        >
    where
        Self: 'a;

    type AncestorsIter<'a>
        = AncestorIter<'a>
    where
        Self: 'a;

    type DescendantsIter<'a>
        = DescendantsIter<'a>
    where
        Self: 'a;

    /// Siblings iterator.
    ///
    /// Note: Uses Box for now due to complex Filter type. Siblings are accessed
    /// less frequently than children, so the allocation overhead is acceptable.
    type SiblingsIter<'a>
        = Box<dyn Iterator<Item = ElementId> + 'a>
    where
        Self: 'a;

    /// Returns the parent of the given element.
    #[inline]
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.get(id)?.parent()
    }

    /// Returns zero-cost iterator over children of the given element.
    ///
    /// Performance: Uses Flatten + Option pattern to avoid Box allocation.
    /// The iterator is stack-allocated and has the same performance as direct
    /// iteration over Vec<ElementId>.
    #[inline]
    fn children(&self, id: ElementId) -> Self::ChildrenIter<'_> {
        self.get(id)
            .map(|e| e.children().iter().copied())
            .into_iter()
            .flatten()
    }

    /// Returns an iterator over ancestors of the given element.
    fn ancestors(&self, start: ElementId) -> Self::AncestorsIter<'_> {
        AncestorIter {
            tree: self,
            current: Some(start),
        }
    }

    /// Returns an iterator over descendants of the given element.
    fn descendants(&self, root: ElementId) -> Self::DescendantsIter<'_> {
        DescendantsIter::new(self, root)
    }

    /// Returns an iterator over siblings of the given element.
    fn siblings(&self, id: ElementId) -> Self::SiblingsIter<'_> {
        let parent = self.parent(id);
        Box::new(
            parent
                .map(|p| {
                    self.get(p)
                        .map(|e| e.children().iter().copied().filter(move |&c| c != id))
                        .into_iter()
                        .flatten()
                })
                .into_iter()
                .flatten(),
        )
    }

    /// Returns the slot of the given element within its parent.
    #[inline]
    fn slot(&self, id: ElementId) -> Option<Slot> {
        self.get(id)?.slot()
    }
}

// ============================================================================
// Iterator Types for TreeNav
// ============================================================================

/// Iterator over ancestors of an element.
#[derive(Debug)]
pub struct AncestorIter<'a> {
    tree: &'a ElementTree,
    current: Option<ElementId>,
}

impl Iterator for AncestorIter<'_> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;

        // Check if current exists in tree
        if !self.tree.contains(current) {
            self.current = None;
            return None;
        }

        // Move to parent for next iteration
        self.current = self.tree.parent(current);

        Some(current)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.current.is_some() {
            // If we have a current node, there's at least 1 more element (itself)
            // The upper bound is the maximum possible depth of the tree.
            (1, Some(32)) // Use default MAX_DEPTH as conservative estimate
        } else {
            (0, Some(0))
        }
    }
}

/// Iterator over descendants of an element in depth-first order.
///
/// Uses SmallVec for stack-allocated traversal buffer. Typical UI trees
/// are shallow (depth < 32), so this avoids heap allocation in most cases.
///
/// Performance:
/// - Depth ≤ 32: Zero heap allocations (stack-only)
/// - Depth > 32: Falls back to heap, same as Vec
#[derive(Debug)]
pub struct DescendantsIter<'a> {
    tree: &'a ElementTree,
    /// Stack buffer with 32 inline elements for typical tree depths.
    ///
    /// Each entry is (ElementId, depth). With 32 inline elements, this
    /// supports trees up to depth 32 without any heap allocation.
    stack: SmallVec<[(ElementId, usize); 32]>,
}

impl<'a> DescendantsIter<'a> {
    /// Creates a new descendants iterator starting from root.
    ///
    /// Performance: Initializes with SmallVec, no heap allocation for
    /// shallow trees (depth ≤ 32).
    fn new(tree: &'a ElementTree, root: ElementId) -> Self {
        let mut stack = SmallVec::new();
        stack.push((root, 0));
        Self { tree, stack }
    }
}

impl Iterator for DescendantsIter<'_> {
    type Item = (ElementId, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (current, depth) = self.stack.pop()?;

            // Check if current exists in tree
            if !self.tree.contains(current) {
                continue;
            }

            // Add children to stack in reverse order for correct DFS order
            if let Some(element) = self.tree.get(current) {
                for &child in element.children().iter().rev() {
                    self.stack.push((child, depth + 1));
                }
            }

            return Some((current, depth));
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.stack.len(), None)
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
            // Use is_ancestor_of from TreeNav trait for efficient cycle detection
            if self.is_ancestor_of(child, parent_id) {
                return Err(TreeError::cycle_detected(child));
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
            child_elem.set_parent(new_parent);
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
// RenderTreeAccess Implementation
// ============================================================================

impl flui_tree::RenderTreeAccess for ElementTree {
    /// Returns the render object for an element (stub: always None).
    ///
    /// The actual render object is stored in ViewObject wrappers in flui-view.
    /// This stub implementation keeps flui-element independent of render types.
    #[inline]
    fn render_object(&self, id: ElementId) -> Option<&dyn std::any::Any> {
        // Delegate to Element's stub method (always None)
        // Phase 5 will implement proper delegation to ViewObject
        let _ = id; // suppress unused warning
        None
    }

    /// Returns a mutable render object (stub: always None).
    #[inline]
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn std::any::Any> {
        let _ = id;
        None
    }

    /// Returns the render state for an element (delegates to Element::render_state).
    #[inline]
    fn render_state(&self, id: ElementId) -> Option<&dyn std::any::Any> {
        self.get(id)?.render_state()
    }

    /// Returns a mutable render state (delegates to Element::render_state_mut).
    #[inline]
    fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn std::any::Any> {
        self.get_mut(id)?.render_state_mut()
    }

    /// Returns true if the element is a render element.
    #[inline]
    fn is_render_element(&self, id: ElementId) -> bool {
        self.get(id).map(|e| e.is_render()).unwrap_or(false)
    }
}

// ============================================================================
// Lifecycle Implementation for Element
// ============================================================================

impl Lifecycle for Element {
    /// Check if element is currently active.
    #[inline]
    fn is_active(&self) -> bool {
        self.lifecycle().is_active()
    }

    /// Check if element is mounted in the tree.
    #[inline]
    fn is_mounted(&self) -> bool {
        Element::is_mounted(self)
    }

    /// Mount element into the tree.
    ///
    /// Sets lifecycle to Active and records parent/slot.
    fn mount(&mut self, parent: Option<ElementId>, slot: Slot) {
        // Calculate depth from parent (root = 0)
        let depth = 0; // Will be set by tree during insertion
        Element::mount(self, parent, Some(slot), depth);
    }

    /// Unmount element from the tree.
    ///
    /// Sets lifecycle to Defunct.
    fn unmount(&mut self) {
        Element::unmount(self);
    }

    /// Mark element as needing rebuild.
    #[inline]
    fn mark_needs_build(&mut self) {
        self.mark_dirty();
    }

    /// Check if element needs rebuild.
    #[inline]
    fn needs_build(&self) -> bool {
        self.is_dirty()
    }

    /// Perform the build operation.
    ///
    /// Clears dirty flag. Actual build logic is in ViewObject.
    fn perform_rebuild(&mut self) {
        self.clear_dirty();
        // Note: Actual build invokes view_object.build(ctx)
        // This is handled by the pipeline, not here
    }

    /// Deactivate element (temporary removal).
    fn deactivate(&mut self) {
        Element::deactivate(self);
    }

    /// Activate previously deactivated element.
    fn activate(&mut self) {
        Element::activate(self);
    }
}

// ============================================================================
// DepthTracking Implementation for Element
// ============================================================================

impl DepthTracking for Element {
    /// Get element's depth in tree (root = 0).
    #[inline]
    fn depth(&self) -> usize {
        Element::depth(self)
    }

    /// Set element's depth.
    #[inline]
    fn set_depth(&mut self, depth: usize) {
        Element::set_depth(self, depth);
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
        let children: Vec<_> = TreeNav::children(&tree, parent_id).collect();
        assert_eq!(children, vec![child_id]);
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

        let ids: Vec<_> = TreeRead::node_ids(&tree).collect();
        assert_eq!(ids.len(), 3);

        // IDs should be 1, 2, 3 (1-based)
        assert!(ids.iter().all(|id| id.get() >= 1 && id.get() <= 3));
    }
}
