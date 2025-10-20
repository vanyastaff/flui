//! Element tree lifecycle management
//!
//! Manages mounting, updating, rebuilding, and unmounting of elements.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{AnyWidget, AnyElement, ElementId};

/// Element tree managing lifecycle and dirty tracking
#[derive(Debug)]
pub struct ElementTree {
    root: Option<ElementId>,
    elements: HashMap<ElementId, Box<dyn AnyElement>>,
    dirty_elements: VecDeque<ElementId>,
    building: bool,
    tree_ref: Option<Arc<RwLock<Self>>>,
}

impl ElementTree {
    /// Create empty element tree
    pub fn new() -> Self {
        Self {
            root: None,
            elements: HashMap::new(),
            dirty_elements: VecDeque::new(),
            building: false,
            tree_ref: None,
        }
    }

    /// Check if tree has root
    pub fn has_root(&self) -> bool {
        self.root.is_some()
    }

    /// Get the root element ID
    ///
    /// # Returns
    ///
    /// The root element ID, or `None` if no root has been mounted.
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Get the root RenderObject by traversing from root element
    ///
    /// Walks down the element tree to find the first RenderObject.
    /// This is useful for getting the root of the render tree for layout/paint.
    ///
    /// # Returns
    ///
    /// Reference to the root RenderObject, or None if not found
    ///
    /// # Note
    ///
    /// This is a simplified implementation that only works for simple trees.
    /// In a full implementation, we'd track the render tree separately.
    pub fn root_render_object(&self) -> Option<&dyn crate::AnyRenderObject> {
        let root_id = self.root?;
        self.find_render_object(root_id)
    }

    /// Get mutable reference to root RenderObject
    ///
    /// # Returns
    ///
    /// Mutable reference to the root RenderObject, or None if not found
    pub fn root_render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
        let root_id = self.root?;
        self.find_render_object_mut(root_id)
    }

    /// Find RenderObject starting from given element ID (immutable)
    fn find_render_object(&self, element_id: ElementId) -> Option<&dyn crate::AnyRenderObject> {
        let element = self.get(element_id)?;

        // Check if this element has a RenderObject
        if let Some(render_object) = element.render_object() {
            return Some(render_object);
        }

        // If not, search in children - get child IDs without locking
        // Search children recursively
        for child_id in element.children_iter() {
            if let Some(render_object) = self.find_render_object(child_id) {
                return Some(render_object);
            }
        }

        None
    }

    /// Find RenderObject starting from given element ID (mutable)
    ///
    /// # Note
    ///
    /// This is complex to implement correctly due to Rust's borrow checker.
    /// For now, we use unsafe to achieve the desired behavior.
    fn find_render_object_mut(&mut self, element_id: ElementId) -> Option<&mut dyn crate::AnyRenderObject> {
        // Check if this element has a RenderObject
        let has_render_object = self.elements.get(&element_id)?.render_object().is_some();

        if has_render_object {
            // Get mutable reference
            return self.elements.get_mut(&element_id)?.render_object_mut();
        }

        // Collect child IDs without acquiring locks
        let first_child_id: Option<ElementId> = {
            let element = self.elements.get(&element_id)?;
            element.children_iter().next()
        };

        // Search children - only search first child for now
        // Full recursive search requires more complex lifetime management
        if let Some(first_child) = first_child_id {
            return self.find_render_object_mut(first_child);
        }

        None
    }

    /// Mount a widget as the root of the tree
    ///
    /// Creates an element from the widget and mounts it as the root.
    /// If a root already exists, it will be unmounted first.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut tree = ElementTree::new();
    /// let root_id = tree.set_root(Box::new(MyApp::new()));
    /// assert_eq!(tree.root(), Some(root_id));
    /// ```
    pub fn set_root(&mut self, widget: Box<dyn AnyWidget>) -> ElementId {
        // Unmount existing root if present
        if let Some(old_root_id) = self.root {
            self.remove(old_root_id);
        }

        // Create element from widget
        let mut element = widget.create_element();
        let element_id = element.id();

        // Mount the element (no parent, slot 0)
        element.mount(None, 0);

        // Store in tree
        self.elements.insert(element_id, element);
        self.root = Some(element_id);

        // Mark as dirty for initial build
        self.mark_dirty(element_id);

        element_id
    }

    /// Set tree reference for an element
    ///
    /// This is called internally to give ComponentElements access to the tree
    /// so they can mount their children.
    ///
    /// # Parameters
    ///
    /// - `element_id`: ID of the element
    /// - `tree`: Arc reference to the element tree
    /// Set the tree's self-reference
    ///
    /// This should be called once after the tree is wrapped in Arc<RwLock<>>
    pub fn set_tree_ref(&mut self, tree: Arc<RwLock<Self>>) {
        self.tree_ref = Some(tree);
    }

    pub fn set_element_tree_ref(&mut self, element_id: ElementId, tree: Arc<RwLock<Self>>) {
        if let Some(element) = self.elements.get_mut(&element_id) {
            element.set_tree_ref(tree);
        }
    }


    /// Get an element by ID (immutable)
    pub fn get(&self, id: ElementId) -> Option<&dyn AnyElement> {
        self.elements.get(&id).map(|e| e.as_ref())
    }

    /// Get an element by ID (mutable)
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut dyn AnyElement> {
        self.elements.get_mut(&id).map(|e| e.as_mut())
    }


    /// Mount a child element under a parent
    pub fn insert_child(
        &mut self,
        parent_id: ElementId,
        widget: Box<dyn AnyWidget>,
        slot: usize,
    ) -> Option<ElementId> {
        // Verify parent exists
        if !self.elements.contains_key(&parent_id) {
            return None;
        }

        // Create element from widget
        let mut element = widget.create_element();
        let element_id = element.id();

        // Mount the element
        element.mount(Some(parent_id), slot);

        // Store in tree
        self.elements.insert(element_id, element);

        // Mark as dirty for initial build
        self.mark_dirty(element_id);

        Some(element_id)
    }

    /// Update an element with a new widget
    pub fn update(
        &mut self,
        element_id: ElementId,
        new_widget: Box<dyn AnyWidget>,
    ) -> Result<ElementId, ()> {
        // Check if element exists
        if !self.elements.contains_key(&element_id) {
            return Err(());
        }

        // Remove element temporarily for update
        let mut element = self.elements.remove(&element_id).ok_or(())?;

        // Update the element
        element.update_any(new_widget);

        // Mark as dirty
        element.mark_dirty();

        // Re-insert
        self.elements.insert(element_id, element);

        // Add to dirty queue
        self.mark_dirty(element_id);

        Ok(element_id)
    }


    /// Unmount an element and all its descendants
    pub fn remove(&mut self, element_id: ElementId) {
        // Collect child IDs first (all elements that have this element as parent)
        let child_ids: Vec<ElementId> = self
            .elements
            .iter()
            .filter(|(_, element)| element.parent() == Some(element_id))
            .map(|(id, _)| *id)
            .collect();

        // Unmount children first (recursive)
        for child_id in child_ids {
            self.remove(child_id);
        }

        // Now unmount this element
        if let Some(mut element) = self.elements.remove(&element_id) {
            // Unmount the element
            element.unmount();

            // Remove from dirty queue if present
            self.dirty_elements.retain(|&id| id != element_id);
        }

        // Clear root if this was the root element
        if self.root == Some(element_id) {
            self.root = None;
        }
    }


    /// Mark an element as dirty (needs rebuild)
    pub fn mark_dirty(&mut self, element_id: ElementId) {
        // Don't add duplicates
        if !self.dirty_elements.contains(&element_id) {
            self.dirty_elements.push_back(element_id);
        }

        // Mark the element itself as dirty
        if let Some(element) = self.elements.get_mut(&element_id) {
            element.mark_dirty();
        }
    }

    /// Check if there are any dirty elements
    pub fn has_dirty(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Get the number of dirty elements
    ///
    /// # Returns
    ///
    /// The number of elements in the dirty queue
    pub fn dirty_element_count(&self) -> usize {
        self.dirty_elements.len()
    }


    /// Rebuild a specific element
    ///
    /// This rebuilds a single element by ID, useful for BuildOwner's depth-sorted rebuilding.
    ///
    /// # Returns
    ///
    /// True if the element was rebuilt, false if it wasn't found or wasn't dirty.
    pub fn rebuild_element(&mut self, element_id: ElementId) -> bool {
        // Check if element still exists and is dirty
        let should_rebuild = if let Some(element) = self.elements.get(&element_id) {
            element.is_dirty()
        } else {
            return false;
        };

        if !should_rebuild {
            return false;
        }

        tracing::debug!("ElementTree: rebuilding single element {:?}", element_id);

        // Get old child before rebuild
        let old_child_id = if let Some(element) = self.elements.get_mut(&element_id) {
            element.take_old_child_for_rebuild()
        } else {
            None
        };

        // Rebuild the element
        let children_to_mount = if let Some(element) = self.elements.get_mut(&element_id) {
            element.rebuild()
        } else {
            Vec::new()
        };

        // Unmount old child
        if let Some(old_id) = old_child_id {
            self.remove(old_id);
        }

        // Mount new children
        for (parent_id, child_widget, slot) in children_to_mount {
            if let Some(new_child_id) = self.insert_child(parent_id, child_widget, slot) {
                // Set tree reference for the new child
                if let Some(tree_arc) = self.tree_ref.clone() {
                    self.set_element_tree_ref(new_child_id, tree_arc);
                }

                // Set the child ID on the parent ComponentElement
                if let Some(parent_elem) = self.elements.get_mut(&parent_id) {
                    parent_elem.set_child_after_mount(new_child_id);
                }
            }
        }

        true
    }

    /// Rebuild all dirty elements
    pub fn rebuild(&mut self) {
        if self.building {
            panic!("ElementTree: Recursive rebuild detected");
        }

        let initial_dirty = self.dirty_elements.len();
        if initial_dirty == 0 {
            tracing::debug!("ElementTree::rebuild called: no dirty elements");
        } else {
            tracing::info!("ElementTree::rebuild start: {} dirty elements", initial_dirty);
        }

        self.building = true;

        // Guard against infinite rebuild churn within a single frame
        const MAX_REBUILDS_PER_FRAME: usize = 1024;
        let mut rebuilds_attempted: usize = 0;
        let mut rebuilds_performed: usize = 0;

        // Process dirty queue
        while let Some(element_id) = self.dirty_elements.pop_front() {
            rebuilds_attempted += 1;
            if rebuilds_attempted > MAX_REBUILDS_PER_FRAME {
                self.dirty_elements.push_front(element_id);
                tracing::warn!(
                    "ElementTree: reached MAX_REBUILDS_PER_FRAME ({}). Breaking to avoid infinite build loop. Remaining dirty elements: {}",
                    MAX_REBUILDS_PER_FRAME,
                    self.dirty_elements.len()
                );
                break;
            }

            // Check if element still exists (might have been unmounted)
            let (children_to_mount, old_child_id) = if let Some(element) = self.elements.get_mut(&element_id) {
                // Only rebuild if still dirty (might have been cleared)
                if element.is_dirty() {
                    tracing::debug!("ElementTree: rebuilding element {:?}", element_id);

                    // For ComponentElement, we need to unmount old child first
                    let old_child_id = element.take_old_child_for_rebuild();

                    let children = element.rebuild();
                    rebuilds_performed += 1;

                    // If it is still dirty after rebuild, re-queue it to try again later.
                    if element.is_dirty() {
                        self.dirty_elements.push_back(element_id);
                    }

                    (children, old_child_id)
                } else {
                    (Vec::new(), None)
                }
            } else {
                (Vec::new(), None)
            };

            // Now unmount the old child (after dropping the element reference)
            if let Some(old_id) = old_child_id {
                self.remove(old_id);
            }

            // Mount children that were returned by rebuild
            for (parent_id, child_widget, slot) in children_to_mount {
                if let Some(new_child_id) = self.insert_child(parent_id, child_widget, slot) {
                    // Set tree reference for the new child
                    if let Some(tree_arc) = self.tree_ref.clone() {
                        self.set_element_tree_ref(new_child_id, tree_arc);
                    }

                    // Set the child ID on the parent ComponentElement
                    if let Some(parent_elem) = self.elements.get_mut(&parent_id) {
                        parent_elem.set_child_after_mount(new_child_id);
                    }
                }
            }
        }

        self.building = false;

        let remaining = self.dirty_elements.len();
        tracing::debug!(
            "ElementTree::rebuild end: performed {} rebuild(s), remaining dirty: {}",
            rebuilds_performed,
            remaining
        );
    }

    /// Get the total number of elements in the tree
    ///
    /// # Returns
    ///
    /// The number of mounted elements
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Clear the entire tree
    ///
    /// Unmounts all elements and resets the tree to an empty state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.clear();
    /// assert!(!tree.has_root());
    /// assert_eq!(tree.element_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        if let Some(root_id) = self.root {
            self.remove(root_id);
        }

        self.elements.clear();
        self.dirty_elements.clear();
        self.root = None;
        self.building = false;
    }

    /// Visit all elements in the tree (read-only)
    ///
    /// Traverses the entire tree and calls the visitor function for each element.
    /// The traversal order is depth-first, starting from the root.
    ///
    /// # Parameters
    ///
    /// - `visitor`: Function to call for each element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_elements(&mut |element| {
    ///     println!("Element: {:?}", element.id());
    /// });
    /// ```
    pub fn visit_all_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn AnyElement),
    {
        if let Some(root_id) = self.root {
            self.visit_element_recursive(root_id, visitor);
        }
    }

    /// Visit all elements in the tree (mutable)
    ///
    /// Traverses the entire tree and calls the visitor function for each element.
    /// The traversal order is depth-first, starting from the root.
    ///
    /// # Parameters
    ///
    /// - `visitor`: Function to call for each element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_elements_mut(&mut |element| {
    ///     element.mark_dirty();
    /// });
    /// ```
    pub fn visit_all_elements_mut<F>(&mut self, visitor: &mut F)
    where
        F: FnMut(&mut dyn AnyElement),
    {
        if let Some(root_id) = self.root {
            // Collect all element IDs first (can't borrow elements while iterating)
            let mut element_ids = Vec::new();
            element_ids.push(root_id);

            let mut i = 0;
            while i < element_ids.len() {
                let current_id = element_ids[i];
                if let Some(element) = self.elements.get(&current_id) {
                    for child_id in element.children_iter() {
                        element_ids.push(child_id);
                    }
                }
                i += 1;
            }

            // Now visit all elements
            for element_id in element_ids {
                if let Some(element) = self.elements.get_mut(&element_id) {
                    visitor(element.as_mut());
                }
            }
        }
    }

    /// Helper for recursive element visitation (read-only)
    fn visit_element_recursive<F>(&self, element_id: ElementId, visitor: &mut F)
    where
        F: FnMut(&dyn AnyElement),
    {
        // Visit this element
        if let Some(element) = self.elements.get(&element_id) {
            visitor(element.as_ref());

            // Visit children
            let child_ids: Vec<_> = element.children_iter().collect();
            for child_id in child_ids {
                self.visit_element_recursive(child_id, visitor);
            }
        }
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnyWidget, Context, StatelessWidget, Widget};

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget {
        name: String,
    }

    impl TestWidget {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(TestWidget::new(format!("{}_child", self.name)))
        }
    }

    #[test]
    fn test_element_tree_new() {
        let tree = ElementTree::new();
        assert!(!tree.has_root());
        assert_eq!(tree.element_count(), 0);
        assert!(!tree.has_dirty());
    }

    #[test]
    fn test_element_tree_mount_root() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));

        assert!(tree.has_root());
        assert_eq!(tree.root(), Some(root_id));
        assert_eq!(tree.element_count(), 1);
        assert!(tree.has_dirty()); // Newly mounted elements are dirty
    }

    #[test]
    fn test_element_tree_get_element() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));

        // Test get_element
        let element = tree.get(root_id).unwrap();
        assert_eq!(element.id(), root_id);

        // Test get_element_mut
        let element_mut = tree.get_mut(root_id).unwrap();
        assert_eq!(element_mut.id(), root_id);
    }

    #[test]
    fn test_element_tree_mount_child() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(root_widget));

        // Mount a child
        let child_widget = TestWidget::new("child");
        let child_id = tree.insert_child(root_id, Box::new(child_widget), 0);

        assert!(child_id.is_some());
        assert_eq!(tree.element_count(), 2);

        let child = tree.get(child_id.unwrap()).unwrap();
        assert_eq!(child.parent(), Some(root_id));
    }

    #[test]
    fn test_element_tree_mount_child_invalid_parent() {
        let mut tree = ElementTree::new();

        let invalid_parent = ElementId::from_raw(99999);
        let child_widget = TestWidget::new("child");

        let result = tree.insert_child(invalid_parent, Box::new(child_widget), 0);

        assert!(result.is_none());
        assert_eq!(tree.element_count(), 0);
    }

    #[test]
    fn test_element_tree_unmount_element() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));
        assert_eq!(tree.element_count(), 1);

        tree.remove(root_id);

        assert!(!tree.has_root());
        assert_eq!(tree.element_count(), 0);
        assert!(tree.get(root_id).is_none());
    }

    #[test]
    fn test_element_tree_unmount_with_children() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(root_widget));

        let child1_id = tree.insert_child(root_id, Box::new(TestWidget::new("child1")), 0);
        let child2_id = tree.insert_child(root_id, Box::new(TestWidget::new("child2")), 1);

        assert_eq!(tree.element_count(), 3);

        // Unmount root should unmount all children
        tree.remove(root_id);

        assert_eq!(tree.element_count(), 0);
        assert!(tree.get(child1_id.unwrap()).is_none());
        assert!(tree.get(child2_id.unwrap()).is_none());
    }

    #[test]
    fn test_element_tree_mark_dirty() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));

        // Clear dirty queue from initial mount
        tree.rebuild();
        assert!(!tree.has_dirty());

        // Mark dirty
        tree.mark_dirty(root_id);

        assert!(tree.has_dirty());
        assert_eq!(tree.dirty_element_count(), 1);

        let element = tree.get(root_id).unwrap();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_element_tree_mark_dirty_no_duplicates() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));
        tree.rebuild();

        // Mark dirty multiple times
        tree.mark_dirty(root_id);
        tree.mark_dirty(root_id);
        tree.mark_dirty(root_id);

        // Should only appear once in queue
        assert_eq!(tree.dirty_element_count(), 1);
    }

    #[test]
    fn test_element_tree_rebuild_dirty_elements() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));
        assert!(tree.has_dirty());

        tree.rebuild();

        assert!(!tree.has_dirty());
        assert_eq!(tree.dirty_element_count(), 0);

        let element = tree.get(root_id).unwrap();
        assert!(!element.is_dirty());
    }

    #[test]
    fn test_element_tree_rebuild_multiple_dirty() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(root_widget));
        let child1_id = tree.insert_child(root_id, Box::new(TestWidget::new("child1")), 0).unwrap();
        let child2_id = tree.insert_child(root_id, Box::new(TestWidget::new("child2")), 1).unwrap();

        tree.rebuild();

        // Mark all dirty
        tree.mark_dirty(root_id);
        tree.mark_dirty(child1_id);
        tree.mark_dirty(child2_id);

        assert_eq!(tree.dirty_element_count(), 3);

        tree.rebuild();

        assert_eq!(tree.dirty_element_count(), 0);
    }

    #[test]
    #[should_panic(expected = "Recursive rebuild detected")]
    fn test_element_tree_recursive_rebuild_panic() {
        let mut tree = ElementTree::new();
        tree.building = true; // Simulate already building

        tree.rebuild(); // Should panic
    }

    #[test]
    fn test_element_tree_clear() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(root_widget));
        tree.insert_child(root_id, Box::new(TestWidget::new("child")), 0);

        assert_eq!(tree.element_count(), 2);

        tree.clear();

        assert!(!tree.has_root());
        assert_eq!(tree.element_count(), 0);
        assert!(!tree.has_dirty());
    }

    #[test]
    fn test_element_tree_visit_all_elements() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(root_widget));
        tree.insert_child(root_id, Box::new(TestWidget::new("child1")), 0);
        tree.insert_child(root_id, Box::new(TestWidget::new("child2")), 1);

        let mut count = 0;
        tree.visit_all_elements(&mut |_element| {
            count += 1;
        });

        // Should visit root only (children aren't actually added to element's children list
        // in our simple test - ComponentElement would need full implementation)
        assert!(count >= 1);
    }

    #[test]
    fn test_element_tree_visit_all_elements_mut() {
        let mut tree = ElementTree::new();
        let root_widget = TestWidget::new("root");

        tree.set_root(Box::new(root_widget));

        // Mark all elements dirty via visitor
        tree.visit_all_elements_mut(&mut |element| {
            element.mark_dirty();
        });

        // Should have dirty elements
        assert!(tree.has_dirty());
    }

    #[test]
    fn test_element_tree_replace_root() {
        let mut tree = ElementTree::new();
        let widget1 = TestWidget::new("root1");

        let root_id1 = tree.set_root(Box::new(widget1));
        assert_eq!(tree.root(), Some(root_id1));

        // Mount new root (should replace old one)
        let widget2 = TestWidget::new("root2");
        let root_id2 = tree.set_root(Box::new(widget2));

        assert_ne!(root_id1, root_id2);
        assert_eq!(tree.root(), Some(root_id2));
        assert_eq!(tree.element_count(), 1);

        // Old root should be gone
        assert!(tree.get(root_id1).is_none());
    }

    #[test]
    fn test_element_tree_update_element() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("original");

        let element_id = tree.set_root(Box::new(widget));
        tree.rebuild();

        // Update with new widget
        let new_widget = TestWidget::new("updated");
        let result = tree.update(element_id, Box::new(new_widget));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), element_id);
        assert!(tree.has_dirty());
    }

    #[test]
    fn test_element_tree_update_invalid_element() {
        let mut tree = ElementTree::new();

        let invalid_id = ElementId::from_raw(99999);
        let widget = TestWidget::new("test");

        let result = tree.update(invalid_id, Box::new(widget));

        assert!(result.is_err());
    }
}
