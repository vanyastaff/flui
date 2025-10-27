//! ComponentElement for StatelessWidget
//!
//! This element type is created by StatelessWidget and calls build() to create
//! its child widget tree.

use std::fmt;

use crate::{ElementId, StatelessWidget, DynWidget, BoxedWidget};
use super::dyn_element::{DynElement, ElementLifecycle};

/// Element for StatelessWidget
///
/// ComponentElement holds a StatelessWidget and calls its build() method
/// to create a child widget. When the widget is updated or marked dirty,
/// it rebuilds by calling build() again.
///
/// # Architecture
///
/// ```text
/// ComponentElement<StatelessWidget>
///   ├─ widget: W (immutable config)
///   ├─ child: Option<ElementId> (single child from build())
///   └─ lifecycle state
/// ```
pub struct ComponentElement<W: StatelessWidget> {
    /// The widget this element represents
    widget: W,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child element created by build()
    child: Option<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag (needs rebuild)
    dirty: bool,
}

impl<W: StatelessWidget> ComponentElement<W> {
    /// Create a new ComponentElement from a widget
    pub fn new(widget: W) -> Self {
        Self {
            widget,
            parent: None,
            child: None,
            slot: 0,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
        }
    }

    /// Get reference to the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }

    /// Update with a new widget
    pub fn update(&mut self, new_widget: W) {
        self.widget = new_widget;
        self.dirty = true;
    }

    /// Perform rebuild
    ///
    /// Calls build() on the widget and returns the child widget that needs
    /// to be mounted.
    ///
    /// Returns: Vec<(parent_id, child_widget, slot)>
    fn perform_rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, BoxedWidget, usize)> {
        if !self.dirty {
            return Vec::new();
        }

        self.dirty = false;

        // TODO: Create proper BuildContext with tree access
        // For now, this is unimplemented because ComponentElement needs refactoring
        // to work with ElementTree properly
        todo!("ComponentElement::rebuild needs BuildContext - requires ElementTree integration");

        // Clear old child (will be unmounted by caller if needed)
        // self.child = None;

        // Return the child that needs to be mounted
        // vec![(element_id, child_widget, 0)]
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

impl<W: StatelessWidget> fmt::Debug for ComponentElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("child", &self.child)
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

// ========== Implement DynElement ==========

impl<W> DynElement for ComponentElement<W>
where
    W: StatelessWidget + crate::Widget,
    W::Element: DynElement,
{
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
        self.dirty = true; // Will rebuild on first frame
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
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
        use crate::DynWidget;
        // Try to downcast to our widget type
        if let Some(widget) = (&*new_widget as &dyn std::any::Any).downcast_ref::<W>() {
            self.widget = widget.clone();
            self.dirty = true;
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
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self) -> BoxedWidget {
            // Return another TestWidget with different value for testing
            Box::new(TestWidget { value: self.value + 1 })
        }
    }

    #[test]
    fn test_component_element_creation() {
        let widget = TestWidget { value: 42 };
        let element = ComponentElement::new(widget.clone());

        assert_eq!(element.widget().value, 42);
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_component_element_mount() {
        let widget = TestWidget { value: 42 };
        let mut element = ComponentElement::new(widget);

        element.mount(Some(0), 0);

        assert_eq!(element.parent(), Some(0));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_component_element_update() {
        let widget = TestWidget { value: 42 };
        let mut element = ComponentElement::new(widget);

        element.update(TestWidget { value: 100 });

        assert_eq!(element.widget().value, 100);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_component_element_rebuild() {
        let widget = TestWidget { value: 42 };
        let mut element = ComponentElement::new(widget);
        element.mount(Some(0), 0);

        let children = element.rebuild(1);

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, 1); // parent_id
        assert!(!element.is_dirty()); // Should be clean after rebuild
    }
}
