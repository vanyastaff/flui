//! ComponentElement for StatelessWidget
//!
//! This element type is created by StatelessWidget and calls build() to create
//! its child widget tree.

use super::{ElementBase, ElementLifecycle};
use crate::{ElementId, Widget};

/// Element for StatelessWidget
///
/// ComponentElement holds a StatelessWidget (type-erased as DynWidget) and calls
/// its build() method to create a child widget. When the widget is updated or
/// marked dirty, it rebuilds by calling build() again.
///
/// # Architecture
///
/// ```text
/// ComponentElement
///   ├─ base: ElementBase (common fields: widget, parent, slot, lifecycle, dirty)
///   └─ child: Option<ElementId> (single child from build())
/// ```
///
/// # Type Erasure
///
/// Unlike the old generic `ComponentElement<W>`, this version uses type erasure
/// to enable storage in `enum Element`. The widget is stored as `Widget`,
/// which is acceptable because:
///
/// - Widget layer is user-extensible (unbounded types)
/// - Element enum provides fast dispatch (5 fixed variants)
/// - Widget access is not performance-critical (rebuild only)
///
/// # Performance
///
/// Widget is Box<dyn>, but this is acceptable:
/// - Element enum dispatch: O(1) match
/// - Widget access: Only during rebuild (rare)
/// - Element operations: Fast via enum
#[derive(Debug)]
pub struct ComponentElement {
    /// Common element data (widget, parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// Child element created by build()
    child: Option<ElementId>,
}

impl ComponentElement {
    /// Create a new ComponentElement from a widget
    ///
    /// # Parameters
    ///
    /// - `widget` - Any widget implementing DynWidget (StatelessWidget via blanket impl)
    ///
    /// # Examples
    ///
    /// ```rust
    /// let element = ComponentElement::new(Box::new(MyWidget::new()));
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
    /// This is checked via `can_update()`.
    pub fn update(&mut self, new_widget: Widget) {
        // Could add debug assertion for can_update check
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
    /// This is called by ElementTree after mounting the child widget
    /// returned from rebuild().
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
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
    /// Calls build() on the widget and returns the child widget that needs
    /// to be mounted.
    ///
    /// # Arguments
    ///
    /// - `element_id`: The ElementId of this element
    /// - `tree`: Shared reference to the ElementTree for creating BuildContext
    ///
    /// # Returns
    ///
    /// Vec<(parent_id, child_widget, slot)> - Children to be inflated
    ///
    /// # Implementation Note
    ///
    /// Takes Arc<RwLock<ElementTree>> to create BuildContext with proper tree access
    /// for dependency tracking during the build phase.
    pub fn rebuild(
        &mut self,
        element_id: ElementId,
        tree: std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> Vec<(ElementId, Widget, usize)> {
        if !self.base.is_dirty() {
            return Vec::new();
        }

        self.base.clear_dirty();

        // Create BuildContext for the build phase
        let context = crate::element::BuildContext::new(tree, element_id);

        // Call build() on the widget (if it's a StatelessWidget)
        if let Some(child_widget) = self.base.widget().build(&context) {
            // Return child to be mounted at slot 0
            vec![(element_id, child_widget, 0)]
        } else {
            // Widget doesn't support building (shouldn't happen for ComponentElement)
            Vec::new()
        }
    }

    /// Forget child element
    ///
    /// Called by ElementTree when child is being removed.
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    /// Update slot for child
    ///
    /// ComponentElement always has slot 0 for its single child, so this is a no-op.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // ComponentElement always has exactly one child at slot 0
        // Nothing to update
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

    impl crate::widget::StatelessWidget for TestWidget {
        fn build(&self, _ctx: &crate::element::BuildContext) -> Widget {
            Widget::stateless(TestWidget {
                value: self.value + 1,
            })
        }

        fn clone_boxed(&self) -> Box<dyn crate::widget::StatelessWidget> {
            Box::new(self.clone())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_component_element_creation() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        let element = ComponentElement::new(widget);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_component_element_mount() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        let mut element = ComponentElement::new(widget);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_component_element_lifecycle() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        let mut element = ComponentElement::new(widget);

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
    fn test_component_element_dirty_flag() {
        let widget = Widget::stateless(TestWidget { value: 42 });
        let mut element = ComponentElement::new(widget);

        // Initially dirty
        assert!(element.is_dirty());

        // Mark dirty
        element.mark_dirty();
        assert!(element.is_dirty());
    }
}
