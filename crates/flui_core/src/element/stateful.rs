//! StatefulElement for StatefulWidget
//!
//! This element type is created by StatefulWidget and manages a State object
//! that persists across rebuilds.

use std::any::Any;
use std::fmt;

use super::{ElementBase, ElementLifecycle};
use crate::{ElementId, Widget};

// ============================================================================
// Sealed helper trait for auto as_any() implementation
// ============================================================================

mod sealed {
    use super::*;

    /// Sealed trait to prevent external implementations.
    ///
    /// This helper trait provides as_any() automatically for all DynState types.
    /// It's sealed to ensure it's only used internally.
    pub trait AsAnyState: fmt::Debug + Send + Sync + 'static {
        fn as_any_state(&self) -> &dyn Any;
        fn as_any_state_mut(&mut self) -> &mut dyn Any;
    }

    /// Blanket implementation: All 'static types get as_any() for free
    impl<T> AsAnyState for T
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        fn as_any_state(&self) -> &dyn Any {
            self
        }

        fn as_any_state_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}

// ============================================================================
// Main DynState trait
// ============================================================================

/// Object-safe State trait
///
/// This trait provides type-erased interface for widget state objects.
/// It enables StatefulElement to store state without generic parameters.
///
/// # Automatic Downcasting
///
/// The `as_any()` and `as_any_mut()` methods are provided automatically
/// through a helper trait. You don't need to implement them manually.
pub trait DynState: sealed::AsAnyState {
    /// Build widget tree using current state
    ///
    /// Called when element needs rebuild. The state can access
    /// the current widget configuration via the widget parameter.
    ///
    /// # Parameters
    ///
    /// - `widget`: The current widget configuration
    /// - `context`: BuildContext for accessing inherited widgets and tree
    fn build(&mut self, widget: &Widget, context: &crate::element::BuildContext) -> Widget;

    /// Called when widget configuration changes
    ///
    /// Allows state to react to configuration updates.
    /// Both old and new widget are provided for comparison.
    fn did_update_widget(&mut self, old_widget: &Widget, new_widget: &Widget);

    /// Called when state is being disposed
    ///
    /// Use this for cleanup: canceling async operations,
    /// unsubscribing from streams, etc.
    fn dispose(&mut self);

    /// Get as Any for downcasting
    ///
    /// This method is automatically implemented via the helper trait.
    fn as_any(&self) -> &dyn Any {
        self.as_any_state()
    }

    /// Get as Any mutable for downcasting
    ///
    /// This method is automatically implemented via the helper trait.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_any_state_mut()
    }
}

/// Type alias for boxed state
pub type BoxedState = Box<dyn DynState>;

/// Element for StatefulWidget
///
/// StatefulElement holds both the widget (immutable config) and a State object
/// (mutable state). When the widget is updated, the State persists and
/// `did_update_widget()` is called. When dirty, it rebuilds by calling `build()`
/// on the State object.
///
/// # Architecture
///
/// ```text
/// StatefulElement
///   ├─ widget: Widget (type-erased StatefulWidget)
///   ├─ state: Box<dyn DynState> (type-erased State)
///   ├─ child: Option<ElementId> (single child from State.build())
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Both widget and state are type-erased via trait objects:
/// - Widget: `Widget` (user-extensible)
/// - State: `Box<dyn DynState>` (user-extensible)
///
/// This enables storage in `enum Element` while maintaining flexibility.
///
/// # Lifecycle
///
/// 1. **create_state()** - Widget creates State object
/// 2. **mount()** - Element mounted to tree
/// 3. **init_state()** - State initialization (TODO)
/// 4. **build()** - State builds child widget tree
/// 5. **did_update_widget()** - Widget config changes
/// 6. **dispose()** - State cleanup
#[derive(Debug)]
pub struct StatefulElement {
    /// Common element data (widget, parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// The state object (type-erased)
    state: BoxedState,

    /// Child element created by State.build()
    child: Option<ElementId>,

    /// Whether init_state has been called
    #[allow(dead_code)]
    initialized: bool,
}

impl StatefulElement {
    /// Create a new StatefulElement from a widget and state
    ///
    /// # Parameters
    ///
    /// - `widget` - Any widget implementing DynWidget (StatefulWidget)
    /// - `state` - State object created by widget
    pub fn new(widget: Widget, state: BoxedState) -> Self {
        Self {
            base: ElementBase::new(widget),
            state,
            child: None,
            initialized: false,
        }
    }

    /// Get reference to the widget (as DynWidget trait object)
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &Widget {
        self.base.widget()
    }

    /// Get reference to the state (as DynState trait object)
    #[inline]
    #[must_use]
    pub fn state(&self) -> &dyn DynState {
        &*self.state
    }

    /// Get mutable reference to the state
    #[inline]
    #[must_use]
    pub fn state_mut(&mut self) -> &mut dyn DynState {
        &mut *self.state
    }

    /// Update with a new widget
    ///
    /// Calls did_update_widget on the state to notify about configuration change.
    pub fn update(&mut self, new_widget: Widget) {
        let old_widget = self.base.widget().clone();
        self.base.set_widget(new_widget);

        // Call did_update_widget on the state
        self.state
            .did_update_widget(&old_widget, self.base.widget());

        // Mark as dirty to trigger rebuild
        self.base.mark_dirty();
    }

    /// Get child element ID
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID after it's been mounted
    #[allow(dead_code)]
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    // ========== DynElement-like Interface ==========

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
    pub fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    pub fn unmount(&mut self) {
        self.base.unmount();

        // Call dispose on state for cleanup
        self.state.dispose();

        // Child will be unmounted by ElementTree
        self.child = None;
    }

    /// Deactivate element
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element needs rebuild
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
    /// Calls build() on the state and returns the child widget that needs
    /// to be mounted.
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

        // Call build() on the state with BuildContext
        let child_widget = self.state.build(self.base.widget(), &context);

        // Clear old child (will be unmounted by caller if needed)
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    /// Forget child element
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    /// Update slot for child
    ///
    /// StatefulElement always has slot 0 for its single child, so this is a no-op.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // StatefulElement always has exactly one child at slot 0
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

    impl crate::Widget for TestWidget {
        fn clone_boxed(&self) -> crate::Widget {
            Box::new(self.clone())
        }
    }

    // Mock state for testing
    #[derive(Debug)]
    struct TestState {
        count: i32,
    }

    impl DynState for TestState {
        fn build(&mut self, _widget: &Widget, _context: &crate::element::BuildContext) -> Widget {
            Box::new(TestWidget { value: self.count })
        }

        fn did_update_widget(&mut self, _old: &Widget, _new: &Widget) {
            // Could update state based on widget changes
        }

        fn dispose(&mut self) {
            // Cleanup
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 0 });
        let element = StatefulElement::new(widget, state);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_stateful_element_mount() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 0 });
        let mut element = StatefulElement::new(widget, state);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_stateful_element_rebuild() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 10 });
        let mut element = StatefulElement::new(widget, state);
        element.mount(Some(0), 0);

        let tree = std::sync::Arc::new(parking_lot::RwLock::new(super::ElementTree::new()));
        let children = element.rebuild(1, tree);

        assert_eq!(children.len(), 1);
        assert!(!element.is_dirty()); // Should be clean after rebuild
    }

    #[test]
    fn test_stateful_element_state_access() {
        let widget: Widget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 10 });
        let mut element = StatefulElement::new(widget, state);

        // Test state access
        let state = element
            .state()
            .as_any()
            .downcast_ref::<TestState>()
            .unwrap();
        assert_eq!(state.count, 10);

        // Test mutable state access
        let state_mut = element
            .state_mut()
            .as_any_mut()
            .downcast_mut::<TestState>()
            .unwrap();
        state_mut.count = 20;

        let state = element
            .state()
            .as_any()
            .downcast_ref::<TestState>()
            .unwrap();
        assert_eq!(state.count, 20);
    }
}
