//! Element tree lifecycle management with Slab-based arena allocation
//!
//! Clean architecture using Slab for efficient memory management and O(1) access.

use std::collections::VecDeque;
use std::sync::Arc;
use slab::Slab;
use parking_lot::RwLock;

use crate::{DynWidget, DynElement, ElementId};
use crate::render::RenderState;

/// Element tree managing lifecycle and dirty tracking
///
/// Uses Slab for arena-based allocation - elements are stored contiguously
/// in memory with direct index-based access.
#[derive(Debug)]
pub struct ElementTree {
    /// Arena storage for all elements
    nodes: Slab<ElementNode>,

    /// Root element index
    root: Option<ElementId>,

    /// Dirty elements that need rebuild (using indices for efficiency)
    dirty_nodes: VecDeque<ElementId>,

    /// Whether we're currently building
    building: bool,

    /// Build scope isolation - prevent infinite rebuild loops
    in_build_scope: bool,
    deferred_dirty: VecDeque<ElementId>,
}

/// Internal node structure containing element and tree relationships
#[derive(Debug)]
struct ElementNode {
    /// The actual element
    element: Box<dyn DynElement>,

    /// Parent-child relationships (via Slab indices)
    parent: Option<ElementId>,
    children: Vec<ElementId>,

    /// Render state (for RenderObjectElements)
    ///
    /// Stores layout/paint state (size, constraints, dirty flags) for elements
    /// that have a RenderObject. This moves state from RenderObject into the
    /// tree where it logically belongs (state is per-element, not per-data).
    ///
    /// Uses RwLock for interior mutability - allows modifying state through &self
    /// during layout/paint operations without requiring &mut ElementTree.
    /// RwLock is thread-safe (unlike RefCell).
    render_state: parking_lot::RwLock<Option<RenderState>>,

    /// Parent data (type-erased)
    ///
    /// This is data that the parent RenderObject attaches to this child.
    /// For example, Stack attaches StackParentData to position children,
    /// Flex attaches FlexParentData for flex factors.
    ///
    /// Stored as `Box<dyn ParentData>` for type erasure, downcasted when needed.
    /// Uses the ParentData trait which provides DowncastSync for type-safe downcasting.
    ///
    /// **Note**: Stored separately from RenderState because parent_data is about
    /// parent-child relationship (set once at mount), not layout/paint state
    /// (changes every frame).
    ///
    /// Uses RwLock for interior mutability - allows reading from multiple threads.
    parent_data: parking_lot::RwLock<Option<Box<dyn crate::ParentData>>>,
}

impl ElementTree {
    /// Create empty element tree
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
            dirty_nodes: VecDeque::new(),
            building: false,
            in_build_scope: false,
            deferred_dirty: VecDeque::new(),
        }
    }

    /// Create element tree with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
            dirty_nodes: VecDeque::new(),
            building: false,
            in_build_scope: false,
            deferred_dirty: VecDeque::new(),
        }
    }

    /// Check if tree has root
    #[inline]
    pub fn has_root(&self) -> bool {
        self.root.is_some()
    }

    /// Get the root element ID
    #[inline]
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Get an element by ID (immutable)
    #[inline]
    pub fn get(&self, id: ElementId) -> Option<&dyn DynElement> {
        self.nodes.get(id).map(|node| node.element.as_ref())
    }

    /// Get an element by ID (mutable)
    #[inline]
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut dyn DynElement> {
        self.nodes.get_mut(id).map(|node| node.element.as_mut())
    }

    /// Get children of an element
    #[inline]
    pub fn children(&self, id: ElementId) -> &[ElementId] {
        self.nodes.get(id)
            .map(|node| node.children.as_slice())
            .unwrap_or(&[])
    }

    /// Get parent of an element
    #[inline]
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.nodes.get(id).and_then(|node| node.parent)
    }

    /// Set root widget
    pub fn set_root(&mut self, widget: Box<dyn DynWidget>) -> ElementId {
        // Unmount existing root if present
        if let Some(old_root_id) = self.root {
            self.remove(old_root_id);
        }

        // Create element from widget
        let mut element = widget.create_element();

        // Mount the element (no parent, slot 0)
        element.mount(None, 0);

        // Insert into Slab - this generates the ID
        let element_id = self.nodes.insert(ElementNode {
            element,
            parent: None,
            children: Vec::new(),
            render_state: parking_lot::RwLock::new(Some(RenderState::new())), // Create state for root
            parent_data: parking_lot::RwLock::new(None),
        });

        self.root = Some(element_id);

        // Mark as dirty for initial build
        self.mark_dirty(element_id);

        element_id
    }

    /// Insert a child element
    pub fn insert_child(
        &mut self,
        parent_id: ElementId,
        widget: Box<dyn DynWidget>,
        slot: usize,
    ) -> Option<ElementId> {
        // Verify parent exists
        if !self.nodes.contains(parent_id) {
            return None;
        }

        // Create element from widget
        let mut element = widget.create_element();

        // Mount the element
        element.mount(Some(parent_id), slot);

        // Insert into Slab - this generates the ID
        let element_id = self.nodes.insert(ElementNode {
            element,
            parent: Some(parent_id),
            children: Vec::new(),
            render_state: parking_lot::RwLock::new(Some(RenderState::new())), // Create state for child
            parent_data: parking_lot::RwLock::new(None),
        });

        // Add to parent's children list
        if let Some(parent_node) = self.nodes.get_mut(parent_id) {
            if !parent_node.children.contains(&element_id) {
                parent_node.children.push(element_id);
            }

            // Notify parent element about the new child
            parent_node.element.set_child_after_mount(element_id);
        }

        // Mark as dirty for initial build
        self.mark_dirty(element_id);

        Some(element_id)
    }

    /// Add child relationship
    pub fn add_child(&mut self, parent_id: ElementId, child_id: ElementId) {
        // Add to parent's children
        if let Some(parent_node) = self.nodes.get_mut(parent_id) {
            if !parent_node.children.contains(&child_id) {
                parent_node.children.push(child_id);
            }
        }

        // Set child's parent
        if let Some(child_node) = self.nodes.get_mut(child_id) {
            child_node.parent = Some(parent_id);
        }
    }

    /// Remove child relationship
    pub fn remove_child(&mut self, parent_id: ElementId, child_id: ElementId) {
        if let Some(parent_node) = self.nodes.get_mut(parent_id) {
            parent_node.children.retain(|&id| id != child_id);
        }

        if let Some(child_node) = self.nodes.get_mut(child_id) {
            child_node.parent = None;
        }
    }

    /// Remove an element and all its descendants
    pub fn remove(&mut self, element_id: ElementId) {
        // Collect child IDs first
        let child_ids: Vec<ElementId> = self.children(element_id).to_vec();

        // Unmount children recursively
        for child_id in child_ids {
            self.remove(child_id);
        }

        // Unmount this element
        if let Some(mut node) = self.nodes.try_remove(element_id) {
            node.element.unmount();

            // Remove from dirty queue
            self.dirty_nodes.retain(|&id| id != element_id);
        }

        // Clear root if this was the root element
        if self.root == Some(element_id) {
            self.root = None;
        }
    }

    /// Mark an element as dirty (needs rebuild)
    pub fn mark_dirty(&mut self, element_id: ElementId) {
        // If in build scope, defer the dirty marking
        if self.in_build_scope {
            if !self.deferred_dirty.contains(&element_id) {
                self.deferred_dirty.push_back(element_id);
            }
            return;
        }

        // Normal path: mark dirty immediately
        if !self.dirty_nodes.contains(&element_id) {
            self.dirty_nodes.push_back(element_id);
        }

        // Mark the element itself as dirty
        if let Some(node) = self.nodes.get_mut(element_id) {
            node.element.mark_dirty();
        }
    }

    /// Check if there are any dirty elements
    #[inline]
    pub fn has_dirty(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    /// Get the number of dirty elements
    #[inline]
    pub fn dirty_element_count(&self) -> usize {
        self.dirty_nodes.len()
    }

    /// Set build scope state
    pub(crate) fn set_in_build_scope(&mut self, value: bool) {
        self.in_build_scope = value;
    }

    /// Check if currently in build scope
    #[inline]
    pub fn is_in_build_scope(&self) -> bool {
        self.in_build_scope
    }

    /// Flush deferred dirty elements
    pub(crate) fn flush_deferred_dirty(&mut self) {
        while let Some(element_id) = self.deferred_dirty.pop_front() {
            // Check if element still exists
            if !self.nodes.contains(element_id) {
                continue;
            }

            // Now safe to mark dirty
            if !self.dirty_nodes.contains(&element_id) {
                self.dirty_nodes.push_back(element_id);
            }

            if let Some(node) = self.nodes.get_mut(element_id) {
                node.element.mark_dirty();
            }
        }
    }

    /// Rebuild all dirty elements
    #[tracing::instrument(
        level = "debug",
        skip(self),
        fields(
            dirty_count = self.dirty_nodes.len(),
            element_count = self.nodes.len()
        )
    )]
    pub fn rebuild(&mut self, tree_arc: Arc<RwLock<Self>>) {
        if self.building {
            panic!("ElementTree: Recursive rebuild detected");
        }

        let initial_dirty = self.dirty_nodes.len();
        if initial_dirty == 0 {
            tracing::debug!("ElementTree::rebuild called: no dirty elements");
        } else {
            tracing::info!("ElementTree::rebuild start: {} dirty elements", initial_dirty);
        }

        self.building = true;

        const MAX_REBUILDS_PER_FRAME: usize = 1024;
        let mut rebuilds_attempted: usize = 0;
        let mut rebuilds_performed: usize = 0;

        while let Some(element_id) = self.dirty_nodes.pop_front() {
            rebuilds_attempted += 1;
            if rebuilds_attempted > MAX_REBUILDS_PER_FRAME {
                self.dirty_nodes.push_front(element_id);
                tracing::warn!(
                    "ElementTree: reached MAX_REBUILDS_PER_FRAME ({}). Remaining dirty: {}",
                    MAX_REBUILDS_PER_FRAME,
                    self.dirty_nodes.len()
                );
                break;
            }

            // Check if element still exists
            if !self.nodes.contains(element_id) {
                continue;
            }

            // Rebuild if still dirty
            if let Some(node) = self.nodes.get_mut(element_id) {
                if node.element.is_dirty() {
                    tracing::debug!("ElementTree: rebuilding element {}", element_id);

                    let children_to_mount = node.element.rebuild(element_id);
                    rebuilds_performed += 1;

                    // Mount all children returned by rebuild
                    for (parent_id, child_widget, slot) in children_to_mount {
                        if let Some(child_id) = self.insert_child(parent_id, child_widget, slot) {
                            // Set tree reference on newly inserted child
                            if let Some(child_node) = self.nodes.get_mut(child_id) {
                                child_node.element.set_tree_ref(tree_arc.clone());
                            }
                            tracing::trace!("ElementTree: mounted child {} for parent {} at slot {}", child_id, parent_id, slot);
                        }
                    }

                    // If still dirty after rebuild, re-queue
                    if self.nodes.get(element_id).map_or(false, |n| n.element.is_dirty()) {
                        self.dirty_nodes.push_back(element_id);
                    }
                }
            }
        }

        self.building = false;

        let remaining = self.dirty_nodes.len();
        tracing::debug!(
            "ElementTree::rebuild end: performed {} rebuild(s), remaining dirty: {}",
            rebuilds_performed,
            remaining
        );
    }

    /// Get the total number of elements in the tree
    #[inline]
    pub fn element_count(&self) -> usize {
        self.nodes.len()
    }

    /// Clear the entire tree
    pub fn clear(&mut self) {
        if let Some(root_id) = self.root {
            self.remove(root_id);
        }

        self.nodes.clear();
        self.dirty_nodes.clear();
        self.root = None;
        self.building = false;
    }

    /// Visit all elements in the tree (read-only)
    pub fn visit_all_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(ElementId, &dyn DynElement),
    {
        if let Some(root_id) = self.root {
            self.visit_element_recursive(root_id, visitor);
        }
    }

    /// Helper for recursive element visitation
    fn visit_element_recursive<F>(&self, element_id: ElementId, visitor: &mut F)
    where
        F: FnMut(ElementId, &dyn DynElement),
    {
        if let Some(node) = self.nodes.get(element_id) {
            visitor(element_id, node.element.as_ref());

            // Visit children
            let child_ids = node.children.clone();
            for child_id in child_ids {
                self.visit_element_recursive(child_id, visitor);
            }
        }
    }

    // ========== RenderState Management ==========
    //
    // These methods provide access to the render_state stored in ElementNode.
    // This state was previously stored inside RenderBox, but has been moved
    // to the tree for better memory locality and clearer ownership.

    /// Get immutable reference to render_state for an element
    ///
    /// Returns None if element doesn't exist or doesn't have a RenderObject.
    /// Uses RwLock for interior mutability.
    #[inline]
    pub fn render_state(&self, element_id: ElementId) -> Option<parking_lot::MappedRwLockReadGuard<RenderState>> {
        let node = self.nodes.get(element_id)?;
        let guard = node.render_state.read();
        // Use RwLockReadGuard::try_map to extract RenderState from Option
        parking_lot::RwLockReadGuard::try_map(guard, |opt| opt.as_ref()).ok()
    }

    /// Get mutable reference to render_state for an element
    ///
    /// Returns None if element doesn't exist or doesn't have a RenderObject.
    /// Uses RwLock for interior mutability - works through &self!
    #[inline]
    pub fn render_state_mut(&self, element_id: ElementId) -> Option<parking_lot::MappedRwLockWriteGuard<RenderState>> {
        let node = self.nodes.get(element_id)?;
        let guard = node.render_state.write();
        // Use RwLockWriteGuard::try_map to extract RenderState from Option
        parking_lot::RwLockWriteGuard::try_map(guard, |opt| opt.as_mut()).ok()
    }

    /// Ensure element has render_state (create if missing)
    ///
    /// Useful for elements that dynamically gain a RenderObject.
    /// Uses RwLock for interior mutability - works through &self!
    pub fn ensure_render_state(&self, element_id: ElementId) {
        if let Some(node) = self.nodes.get(element_id) {
            let mut state = node.render_state.write();
            if state.is_none() {
                *state = Some(RenderState::new());
            }
        }
    }

    /// Remove render_state from an element
    ///
    /// Useful when an element no longer has a RenderObject.
    /// Uses RwLock for interior mutability - works through &self!
    pub fn clear_render_state(&self, element_id: ElementId) {
        if let Some(node) = self.nodes.get(element_id) {
            *node.render_state.write() = None;
        }
    }

    // ========== ParentData Access Methods ==========
    //
    // ParentData is stored separately from RenderState because it represents
    // the parent-child relationship (set once), not layout/paint state
    // (changes every frame).

    /// Get parent_data for an element
    ///
    /// Returns a read guard to the parent_data if it exists.
    /// Use downcast to get the actual type:
    ///
    /// ```rust,ignore
    /// if let Some(data) = tree.parent_data(child_id) {
    ///     if let Some(flex_data) = data.downcast_ref::<FlexParentData>() {
    ///         println!("flex: {}", flex_data.flex);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn parent_data(&self, element_id: ElementId) -> Option<parking_lot::MappedRwLockReadGuard<Box<dyn crate::ParentData>>> {
        let node = self.nodes.get(element_id)?;
        let guard = node.parent_data.read();
        parking_lot::RwLockReadGuard::try_map(guard, |opt| opt.as_ref()).ok()
    }

    /// Get mutable parent_data for an element
    ///
    /// Returns a write guard to the parent_data if it exists.
    #[inline]
    pub fn parent_data_mut(&self, element_id: ElementId) -> Option<parking_lot::MappedRwLockWriteGuard<Box<dyn crate::ParentData>>> {
        let node = self.nodes.get(element_id)?;
        let guard = node.parent_data.write();
        parking_lot::RwLockWriteGuard::try_map(guard, |opt| opt.as_mut()).ok()
    }

    /// Set parent_data for an element
    ///
    /// Typically called by parent's setup_parent_data() during mount.
    pub fn set_parent_data(&self, element_id: ElementId, parent_data: Box<dyn crate::ParentData>) {
        if let Some(node) = self.nodes.get(element_id) {
            *node.parent_data.write() = Some(parent_data);
        }
    }

    /// Clear parent_data for an element
    pub fn clear_parent_data(&self, element_id: ElementId) {
        if let Some(node) = self.nodes.get(element_id) {
            *node.parent_data.write() = None;
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
    use crate::{Context, StatelessWidget};

    // Test widget
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
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
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
    fn test_element_tree_with_capacity() {
        let tree = ElementTree::with_capacity(100);
        assert_eq!(tree.nodes.capacity(), 100);
    }

    #[test]
    fn test_element_tree_set_root() {
        let mut tree = ElementTree::new();
        let widget = TestWidget::new("root");

        let root_id = tree.set_root(Box::new(widget));

        assert!(tree.has_root());
        assert_eq!(tree.root(), Some(root_id));
        assert_eq!(tree.element_count(), 1);
        assert!(tree.has_dirty());
    }

    #[test]
    fn test_element_tree_children() {
        let mut tree = ElementTree::new();
        let root_id = tree.set_root(Box::new(TestWidget::new("root")));

        let child_id = tree.insert_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();

        assert_eq!(tree.children(root_id), &[child_id]);
        assert_eq!(tree.parent(child_id), Some(root_id));
    }

    #[test]
    fn test_element_tree_mark_dirty() {
        let mut tree = ElementTree::new();
        let root_id = tree.set_root(Box::new(TestWidget::new("root")));

        // Clear initial dirty state
        tree.rebuild();
        assert!(!tree.has_dirty());

        // Mark dirty
        tree.mark_dirty(root_id);

        assert!(tree.has_dirty());
        assert_eq!(tree.dirty_element_count(), 1);
    }

    #[test]
    fn test_element_tree_clear() {
        let mut tree = ElementTree::new();
        let root_id = tree.set_root(Box::new(TestWidget::new("root")));
        tree.insert_child(root_id, Box::new(TestWidget::new("child")), 0);

        assert_eq!(tree.element_count(), 2);

        tree.clear();

        assert!(!tree.has_root());
        assert_eq!(tree.element_count(), 0);
        assert!(!tree.has_dirty());
    }
}
