//! StatefulElement for StatefulWidget
//!
//! This element type is created by StatefulWidget and manages a State object
//! that persists across rebuilds.

use std::any::Any;
use std::fmt;

use super::dyn_element::ElementLifecycle;
use crate::{BoxedWidget, DynWidget, ElementId};

/// Object-safe State trait
///
/// This trait provides type-erased interface for widget state objects.
/// It enables StatefulElement to store state without generic parameters.
pub trait DynState: fmt::Debug + Send + Sync + 'static {
    /// Build widget tree using current state
    ///
    /// Called when element needs rebuild. The state can access
    /// the current widget configuration via the widget parameter.
    ///
    /// # Parameters
    ///
    /// - `widget`: The current widget configuration
    /// - `context`: BuildContext for accessing inherited widgets and tree
    fn build(
        &mut self,
        widget: &dyn DynWidget,
        context: &crate::element::BuildContext,
    ) -> BoxedWidget;

    /// Called when widget configuration changes
    ///
    /// Allows state to react to configuration updates.
    /// Both old and new widget are provided for comparison.
    fn did_update_widget(&mut self, old_widget: &dyn DynWidget, new_widget: &dyn DynWidget);

    /// Called when state is being disposed
    ///
    /// Use this for cleanup: canceling async operations,
    /// unsubscribing from streams, etc.
    fn dispose(&mut self);

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as Any mutable for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
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
///   ├─ widget: Box<dyn DynWidget> (type-erased StatefulWidget)
///   ├─ state: Box<dyn DynState> (type-erased State)
///   ├─ child: Option<ElementId> (single child from State.build())
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Both widget and state are type-erased via trait objects:
/// - Widget: `Box<dyn DynWidget>` (user-extensible)
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
    /// The widget this element represents (type-erased)
    widget: BoxedWidget,

    /// The state object (type-erased)
    state: BoxedState,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child element created by State.build()
    child: Option<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag (needs rebuild)
    dirty: bool,

    /// Whether init_state has been called
    initialized: bool,
}

impl StatefulElement {
    /// Create a new StatefulElement from a widget and state
    ///
    /// # Parameters
    ///
    /// - `widget` - Any widget implementing DynWidget (StatefulWidget)
    /// - `state` - State object created by widget
    pub fn new(widget: BoxedWidget, state: BoxedState) -> Self {
        Self {
            widget,
            state,
            parent: None,
            child: None,
            slot: 0,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
            initialized: false,
        }
    }

    /// Get reference to the widget (as DynWidget trait object)
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &dyn DynWidget {
        &*self.widget
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
    pub fn update(&mut self, new_widget: BoxedWidget) {
        let old_widget = std::mem::replace(&mut self.widget, new_widget);

        // Call did_update_widget on the state
        self.state.did_update_widget(&*old_widget, &*self.widget);

        // Mark as dirty to trigger rebuild
        self.dirty = true;
    }

    /// Get child element ID
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID after it's been mounted
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
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
        Box::new(self.child.into_iter())
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
        self.dirty = true; // Will rebuild on first frame
    }

    /// Unmount element from tree
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;

        // Call dispose on state for cleanup
        self.state.dispose();

        // Child will be unmounted by ElementTree
        self.child = None;
    }

    /// Deactivate element
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    /// Activate element
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Rebuild when reactivated
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
    /// Calls build() on the state and returns the child widget that needs
    /// to be mounted.
    pub fn rebuild(
        &mut self,
        element_id: ElementId,
        tree: std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> Vec<(ElementId, BoxedWidget, usize)> {
        if !self.dirty {
            return Vec::new();
        }

        self.dirty = false;

        // Create BuildContext for the build phase
        let context = crate::element::BuildContext::new(tree, element_id);

        // Call build() on the state with BuildContext
        let child_widget = self.state.build(&*self.widget, &context);

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

    impl crate::DynWidget for TestWidget {
        fn clone_boxed(&self) -> crate::BoxedWidget {
            Box::new(self.clone())
        }
    }

    // Mock state for testing
    #[derive(Debug)]
    struct TestState {
        count: i32,
    }

    impl DynState for TestState {
        fn build(
            &mut self,
            _widget: &dyn DynWidget,
            _context: &crate::element::BuildContext,
        ) -> BoxedWidget {
            Box::new(TestWidget { value: self.count })
        }

        fn did_update_widget(&mut self, _old: &dyn DynWidget, _new: &dyn DynWidget) {
            // Could update state based on widget changes
        }

        fn dispose(&mut self) {
            // Cleanup
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 0 });
        let element = StatefulElement::new(widget, state);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_stateful_element_mount() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 0 });
        let mut element = StatefulElement::new(widget, state);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_stateful_element_rebuild() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
        let state: BoxedState = Box::new(TestState { count: 10 });
        let mut element = StatefulElement::new(widget, state);
        element.mount(Some(0), 0);

        let children = element.rebuild(1);

        assert_eq!(children.len(), 1);
        assert!(!element.is_dirty()); // Should be clean after rebuild
    }

    #[test]
    fn test_stateful_element_state_access() {
        let widget: BoxedWidget = Box::new(TestWidget { value: 42 });
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
