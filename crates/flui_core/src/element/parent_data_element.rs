//! ParentDataElement - element for ParentDataWidget
//!
//! Manages a single child and applies parent data to descendant Renders.

use crate::ElementId;
use crate::element::{ElementBase, ElementLifecycle};
use crate::widget::{Widget};

/// Element for ParentDataWidget
///
/// ParentDataElement holds a ParentDataWidget (type-erased as DynWidget) and manages
/// a single child. It applies parent data to descendant Renders by walking
/// down the tree to find the first RenderElement.
///
/// # Architecture
///
/// ```text
/// ParentDataElement
///   ├─ widget: Widget (type-erased ParentDataWidget)
///   ├─ child: Option<ElementId> (single child)
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Like other element types, ParentDataElement uses type erasure to enable storage
/// in `enum Element`. The widget is stored as `Widget`.
///
/// # Parent Data Application
///
/// When the child is mounted, this element walks down the tree to find
/// the first RenderElement and applies parent data to it.
///
/// # Lifecycle
///
/// 1. **mount()** - Insert into tree
/// 2. **rebuild()** - Build child widget
/// 3. **apply_parent_data()** - Set parent data on descendant Render
/// 4. **unmount()** - Remove from tree
#[derive(Debug)]
pub struct ParentDataElement {
    /// Common element data (widget, parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// Child element ID
    child: Option<ElementId>,
}

impl ParentDataElement {
    /// Create a new ParentDataElement from a widget
    ///
    /// # Parameters
    ///
    /// - `widget` - Any widget implementing DynWidget (ParentDataWidget)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let element = ParentDataElement::new(Box::new(Flexible {
    ///     flex: 1,
    ///     child: Box::new(Container::new()),
    /// }));
    /// ```
    pub fn new(widget: Widget) -> Self {
        Self {
            base: ElementBase::new(widget),
            child: None,
        }
    }

    /// Get reference to the widget (as DynWidget trait object)
    ///
    /// Following Rust API Guidelines - no `get_` prefix for getters.
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &Widget {
        self.base.widget()
    }

    /// Update with a new widget
    ///
    /// The new widget must be compatible (same type and key) with the current widget.
    pub fn update(&mut self, new_widget: Widget) {
        self.base.set_widget(new_widget);
    }

    /// Get child element ID
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID after it's been mounted
    ///
    /// This is called by ElementTree after mounting the child widget.
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Forget child element
    ///
    /// Called by ElementTree when child is being removed.
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    // ========== DynElement-like Interface ==========
    //
    // These methods match the DynElement trait and are called by Element enum.
    // Following API Guidelines: is_* for predicates, no get_* prefix.

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get iterator over child element IDs
    #[inline]
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Mount element to tree
    ///
    /// Sets parent, slot, and transitions to Active lifecycle state.
    /// Marks element as dirty to trigger initial build.
    pub fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    ///
    /// Transitions to Defunct lifecycle state and clears child reference.
    /// The child element will be unmounted by ElementTree separately.
    pub fn unmount(&mut self) {
        self.base.unmount();
        // Child will be unmounted by ElementTree
        self.child = None;
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element
    ///
    /// Called when element is reactivated. Marks dirty to trigger rebuild.
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: is_* prefix for boolean predicates.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Perform rebuild
    ///
    /// ParentDataWidget wraps its child widget. Returns the child that needs
    /// to be mounted.
    ///
    /// # Returns
    ///
    /// Vec<(parent_id, child_widget, slot)> - Children to be inflated
    pub fn rebuild(
        &mut self,
        _element_id: ElementId,
        _tree: std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> Vec<(ElementId, Widget, usize)> {
        if !self.base.is_dirty() {
            return Vec::new();
        }

        self.base.clear_dirty();

        // Get child widget from ParentDataWidget via Widget::parent_data_child()
        if let Some(child_widget_ref) = self.base.widget().parent_data_child() {
            // Mark old child for unmounting
            self.child = None;

            // Clone the child widget
            let child_widget = child_widget_ref.clone();

            // Return child to be mounted
            // Note: Parent should be set when this element was mounted
            vec![(self.base.parent().unwrap_or(0), child_widget, 0)]
        } else {
            // Not a ParentDataWidget or no child
            Vec::new()
        }
    }

    /// Update slot for child
    ///
    /// ParentDataElement only has one child, slot is always 0.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // ParentDataElement only has one child, slot is always 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl crate::Widget for TestWidget {
        // Minimal implementation for testing
    }

    #[test]
    fn test_parent_data_element_creation() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let element = ParentDataElement::new(widget);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
        assert_eq!(element.child(), None);
    }

    #[test]
    fn test_parent_data_element_mount() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);

        element.mount(Some(100), 0);

        assert_eq!(element.parent(), Some(100));
        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_parent_data_element_update() {
        let widget1: Widget = Box::new(TestWidget { value: 1 });
        let mut element = ParentDataElement::new(widget1);

        let widget2: Widget = Box::new(TestWidget { value: 2 });
        element.update(widget2);

        assert!(element.is_dirty());
    }

    #[test]
    fn test_parent_data_element_unmount() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);
        element.mount(None, 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert_eq!(element.child(), None);
    }

    #[test]
    fn test_parent_data_element_lifecycle() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);

        // Initial
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        // Mount → Active
        element.mount(Some(0), 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Deactivate → Inactive
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Activate → Active
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_dirty()); // Should mark dirty on activate

        // Unmount → Defunct
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_parent_data_element_dirty_flag() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);

        // Initially dirty
        assert!(element.is_dirty());

        // Rebuild clears dirty
        let tree = std::sync::Arc::new(parking_lot::RwLock::new(super::ElementTree::new()));
        element.rebuild(1, tree);
        assert!(!element.is_dirty());

        // Mark dirty
        element.mark_dirty();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_parent_data_element_child_management() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);

        // No child initially
        assert_eq!(element.child(), None);

        // Set child
        element.set_child(5);
        assert_eq!(element.child(), Some(5));

        // Forget child
        element.forget_child(5);
        assert_eq!(element.child(), None);

        // Forget non-existent child (should be no-op)
        element.set_child(10);
        element.forget_child(999);
        assert_eq!(element.child(), Some(10));
    }

    #[test]
    fn test_parent_data_element_rebuild() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let mut element = ParentDataElement::new(widget);
        element.mount(Some(0), 0);

        // Rebuild when dirty
        let tree = std::sync::Arc::new(parking_lot::RwLock::new(super::ElementTree::new()));
        let children = element.rebuild(1, tree.clone());

        // ParentDataWidget returns child to be inflated
        assert_eq!(children.len(), 0);
        assert!(!element.is_dirty()); // Should be clean after rebuild

        // Rebuild when not dirty should be no-op
        let children = element.rebuild(1, tree);
        assert_eq!(children.len(), 0);
    }
}
