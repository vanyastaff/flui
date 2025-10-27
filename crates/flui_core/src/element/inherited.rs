//! InheritedElement - element for InheritedWidget
//!
//! Manages data propagation down the tree with efficient dependency tracking.

use std::collections::HashSet;

use crate::ElementId;
use crate::widget::{DynWidget, BoxedWidget};
use crate::element::ElementLifecycle;

/// Element for InheritedWidget
///
/// InheritedElement stores the widget data and tracks which descendant elements
/// depend on it. When the widget updates, only dependent elements are rebuilt.
///
/// # Architecture
///
/// ```text
/// InheritedElement
///   ├─ widget: Box<dyn DynWidget> (type-erased InheritedWidget)
///   ├─ dependents: HashSet<ElementId> (who depends on this)
///   ├─ child_id: ElementId (single child)
///   └─ parent: Option<ElementId>
/// ```
///
/// # Dependency Tracking
///
/// - Descendants call `context.depend_on::<Theme>()` to register dependency
/// - When widget updates, `update_should_notify()` decides if dependents rebuild
/// - Only registered dependents are notified (efficient selective updates)
///
/// # Lifecycle
///
/// 1. **mount()** - Insert into tree
/// 2. **update(new_widget)** - Check if dependents should be notified
/// 3. **unmount()** - Remove from tree, clear dependencies
#[derive(Debug)]
pub struct InheritedElement {
    /// The inherited widget containing data (type-erased)
    widget: BoxedWidget,

    /// Set of elements that depend on this InheritedWidget
    ///
    /// When the widget changes, these elements will be marked dirty for rebuild
    /// if `update_should_notify()` returns true.
    dependents: HashSet<ElementId>,

    /// The single child element
    child_id: Option<ElementId>,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag
    dirty: bool,

    /// Slot in parent
    slot: usize,
}

impl InheritedElement {
    /// Create a new InheritedElement
    pub fn new(widget: BoxedWidget) -> Self {
        Self {
            widget,
            dependents: HashSet::new(),
            child_id: None,
            parent: None,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
            slot: 0,
        }
    }

    /// Get reference to the widget
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &dyn DynWidget {
        &*self.widget
    }

    /// Update with a new widget
    ///
    /// Checks if dependents should be notified via update_should_notify.
    pub fn update(&mut self, new_widget: BoxedWidget) {
        // TODO: Call update_should_notify on the widget to check if dependents should rebuild
        // For now, always mark dependents dirty
        self.widget = new_widget;
        self.dirty = true;

        // Mark all dependents dirty
        // (will be handled by ElementTree)
    }

    /// Register a dependent element
    ///
    /// Called by BuildContext when a descendant element accesses inherited data.
    pub fn add_dependent(&mut self, element_id: ElementId) {
        self.dependents.insert(element_id);
    }

    /// Remove a dependent element
    pub fn remove_dependent(&mut self, element_id: ElementId) {
        self.dependents.remove(&element_id);
    }

    /// Get all dependent element IDs
    #[must_use]
    pub fn dependents(&self) -> &HashSet<ElementId> {
        &self.dependents
    }

    /// Get child element ID
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child_id
    }

    /// Set child element ID
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child_id = Some(child_id);
    }

    // ========== DynElement-like Interface ==========

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get iterator over child element IDs
    #[inline]
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child_id.into_iter())
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    /// Mount element to tree
    pub fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    /// Unmount element from tree
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        self.child_id = None;
        self.dependents.clear();
    }

    /// Deactivate element
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    /// Activate element
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    /// Check if element needs rebuild
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Perform rebuild
    ///
    /// InheritedElement doesn't rebuild itself, it just passes through to child.
    /// Returns empty vec as child is managed separately.
    pub fn rebuild(
        &mut self,
        _element_id: ElementId,
        _tree: std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> Vec<(ElementId, BoxedWidget, usize)> {
        if !self.dirty {
            return Vec::new();
        }

        self.dirty = false;

        // InheritedElement doesn't create child widgets during rebuild
        // Child is set during initial mount
        Vec::new()
    }

    /// Forget child element
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child_id == Some(child_id) {
            self.child_id = None;
        }
    }

    /// Update slot for child
    ///
    /// InheritedElement always has slot 0 for its single child, so this is a no-op.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // InheritedElement always has exactly one child at slot 0
        // Nothing to update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl crate::DynWidget for TestWidget {
        // Minimal implementation
    }

    #[test]
    fn test_inherited_element_creation() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let element = InheritedElement::new(widget);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
        assert!(element.dependents().is_empty());
    }

    #[test]
    fn test_inherited_element_dependents() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let mut element = InheritedElement::new(widget);

        // Add dependents
        element.add_dependent(1);
        element.add_dependent(2);
        element.add_dependent(3);

        assert_eq!(element.dependents().len(), 3);
        assert!(element.dependents().contains(&1));
        assert!(element.dependents().contains(&2));
        assert!(element.dependents().contains(&3));

        // Remove dependent
        element.remove_dependent(2);
        assert_eq!(element.dependents().len(), 2);
        assert!(!element.dependents().contains(&2));
    }

    #[test]
    fn test_inherited_element_mount() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let mut element = InheritedElement::new(widget);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_inherited_element_unmount() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let mut element = InheritedElement::new(widget);
        element.add_dependent(1);
        element.add_dependent(2);
        element.mount(Some(0), 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert!(element.dependents().is_empty());
    }
}
