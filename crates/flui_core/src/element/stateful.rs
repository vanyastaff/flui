//! StatefulElement for StatefulWidget
//!
//! This element type is created by StatefulWidget and manages a State object
//! that persists across rebuilds.

use std::fmt;

use crate::{ElementId, StatefulWidget, State, DynWidget, BoxedWidget};
use super::dyn_element::{DynElement, ElementLifecycle};

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
/// StatefulElement<StatefulWidget>
///   ├─ widget: W (immutable config, recreated on update)
///   ├─ state: W::State (mutable state, persists across rebuilds)
///   ├─ child: Option<ElementId> (single child from State.build())
///   └─ lifecycle state
/// ```
///
/// # Lifecycle
///
/// 1. **create_state()** - Widget creates State object
/// 2. **mount()** - Element mounted to tree
/// 3. **init_state()** - State initialization
/// 4. **build()** - State builds child widget tree
/// 5. **did_update_widget()** - Widget config changes
/// 6. **dispose()** - State cleanup
pub struct StatefulElement<W: StatefulWidget> {
    /// The widget this element represents
    widget: W,

    /// The state object created by the widget
    state: W::State,

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

impl<W: StatefulWidget> StatefulElement<W> {
    /// Create a new StatefulElement from a widget
    pub fn new(widget: W) -> Self {
        let state = widget.create_state();

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

    /// Get reference to the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }

    /// Get reference to the state
    pub fn state(&self) -> &W::State {
        &self.state
    }

    /// Get mutable reference to the state
    pub fn state_mut(&mut self) -> &mut W::State {
        &mut self.state
    }

    /// Update with a new widget
    pub fn update(&mut self, new_widget: W) {
        let _old_widget = std::mem::replace(&mut self.widget, new_widget);

        // Call did_update_widget on the state
        self.state.did_update_widget(&self.widget);

        // Mark as dirty to trigger rebuild
        self.dirty = true;
    }

    /// Perform rebuild
    ///
    /// Calls build() on the state and returns the child widget that needs
    /// to be mounted.
    ///
    /// Returns: Vec<(parent_id, child_widget, slot)>
    fn perform_rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, BoxedWidget, usize)> {
        if !self.dirty {
            return Vec::new();
        }

        self.dirty = false;

        // Call build() on the state to get child widget
        let child_widget = self.state.build();

        // Clear old child (will be unmounted by caller if needed)
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    /// Set the child element ID after it's been mounted
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Get child ID
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }
}

impl<W: StatefulWidget> fmt::Debug for StatefulElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("state_type", &std::any::type_name::<W::State>())
            .field("parent", &self.parent)
            .field("child", &self.child)
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .field("initialized", &self.initialized)
            .finish()
    }
}

// ========== Implement DynElement ==========

impl<W: StatefulWidget + DynWidget> DynElement for StatefulElement<W> {
    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;

        // Call init_state on first mount
        if !self.initialized {
            self.state.init_state();
            self.initialized = true;
        }

        self.dirty = true; // Will rebuild on first frame
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;

        // Call dispose on state
        self.state.dispose();

        // Child will be unmounted by ElementTree
        self.child = None;
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Rebuild when reactivated
    }

    fn widget(&self) -> &dyn DynWidget {
        &self.widget
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.update(*widget);
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, BoxedWidget, usize)> {
        self.perform_rebuild(element_id)
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Single child, slot is always 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct CounterWidget {
        initial: i32,
    }

    impl StatefulWidget for CounterWidget {
        type State = CounterState;

        fn create_state(&self) -> Self::State {
            CounterState {
                count: self.initial,
                init_called: false,
                dispose_called: false,
            }
        }
    }

    // Use macro to implement Widget + DynWidget
    crate::impl_widget_for_stateful!(CounterWidget);

    #[derive(Debug)]
    struct CounterState {
        count: i32,
        init_called: bool,
        dispose_called: bool,
    }

    impl State for CounterState {
        type Widget = CounterWidget;

        fn build(&mut self) -> BoxedWidget {
            // Return a simple widget for testing
            Box::new(CounterWidget { initial: self.count + 1 })
        }

        fn init_state(&mut self) {
            self.init_called = true;
        }

        fn did_update_widget(&mut self, new_widget: &CounterWidget) {
            // Update state based on new widget
            if new_widget.initial != self.count {
                self.count = new_widget.initial;
            }
        }

        fn dispose(&mut self) {
            self.dispose_called = true;
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let widget = CounterWidget { initial: 42 };
        let element = StatefulElement::new(widget);

        assert_eq!(element.state().count, 42);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
        assert!(!element.state().init_called);
    }

    #[test]
    fn test_stateful_element_mount() {
        let widget = CounterWidget { initial: 42 };
        let mut element = StatefulElement::new(widget);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.state().init_called); // init_state should be called
    }

    #[test]
    fn test_stateful_element_update() {
        let widget = CounterWidget { initial: 42 };
        let mut element = StatefulElement::new(widget);
        element.mount(Some(0), 0);

        // Update with new widget
        element.update(CounterWidget { initial: 100 });

        assert_eq!(element.widget().initial, 100);
        assert_eq!(element.state().count, 100); // State should be updated via did_update_widget
        assert!(element.is_dirty());
    }

    #[test]
    fn test_stateful_element_rebuild() {
        let widget = CounterWidget { initial: 42 };
        let mut element = StatefulElement::new(widget);
        element.mount(Some(0), 0);

        let children = element.rebuild(1);

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, 1); // parent_id
        assert!(!element.is_dirty()); // Should be clean after rebuild
    }

    #[test]
    fn test_stateful_element_dispose() {
        let widget = CounterWidget { initial: 42 };
        let mut element = StatefulElement::new(widget);
        element.mount(Some(0), 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
        assert!(element.state().dispose_called); // dispose should be called
    }
}
