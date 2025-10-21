//! Element tree - mutable state holders for widgets
//!
//! This module provides the Element trait and implementations, which form the middle
//! layer of the three-tree architecture (Widget → Element → RenderObject).
//!
//! # Module Structure
//!
//! - `traits` - Element trait definition
//! - `lifecycle` - ElementLifecycle enum and InactiveElements manager
//! - `component` - ComponentElement for StatelessWidget
//! - `stateful` - StatefulElement for StatefulWidget
//! - `render_object` - RenderObjectElement for RenderObjectWidget
//! - `render` - Specialized render elements (Leaf, Single, Multi)

// Submodules
pub mod any_element;
mod component;
mod lifecycle;
pub mod render;
mod render_object;
mod stateful;
mod traits;




// Re-export main types
pub use any_element::AnyElement;
pub use traits::Element;
pub use lifecycle::{ElementLifecycle, InactiveElements};
pub use component::ComponentElement;
pub use stateful::StatefulElement;
pub use render_object::RenderObjectElement;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnyWidget, ElementId, StatelessWidget, Context};

    #[test]
    fn test_element_id_unique() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestStatelessWidget {
        value: i32,
    }

    impl StatelessWidget for TestStatelessWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            // Return self for testing purposes
            Box::new(TestStatelessWidget { value: self.value })
        }
    }

    #[test]
    fn test_component_element_creation() {
        let widget = TestStatelessWidget { value: 42 };
        let element = ComponentElement::new(widget);

        assert!(element.is_dirty());
        assert_eq!(element.parent(), None);
    }

    #[test]
    fn test_component_element_mount() {
        let widget = TestStatelessWidget { value: 42 };
        let mut element = ComponentElement::new(widget);

        element.mount(None, 0);
        assert_eq!(element.parent(), None);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_component_element_mark_dirty() {
        let widget = TestStatelessWidget { value: 42 };
        let element = ComponentElement::new(widget);

        // Element starts dirty
        assert!(element.is_dirty());

        // Rebuild clears dirty flag (but needs tree reference)
        // Can't test rebuild without tree setup
    }

    // Phase 3: Element Lifecycle Tests

    #[test]
    fn test_element_lifecycle_enum() {
        assert_eq!(ElementLifecycle::Initial.is_active(), false);
        assert_eq!(ElementLifecycle::Active.is_active(), true);
        assert_eq!(ElementLifecycle::Inactive.is_active(), false);
        assert_eq!(ElementLifecycle::Defunct.is_active(), false);
    }

    #[test]
    fn test_element_lifecycle_can_reactivate() {
        assert_eq!(ElementLifecycle::Initial.can_reactivate(), false);
        assert_eq!(ElementLifecycle::Active.can_reactivate(), false);
        assert_eq!(ElementLifecycle::Inactive.can_reactivate(), true);
        assert_eq!(ElementLifecycle::Defunct.can_reactivate(), false);
    }

    #[test]
    fn test_element_lifecycle_is_mounted() {
        assert_eq!(ElementLifecycle::Initial.is_mounted(), false);
        assert_eq!(ElementLifecycle::Active.is_mounted(), true);
        assert_eq!(ElementLifecycle::Inactive.is_mounted(), true);
        assert_eq!(ElementLifecycle::Defunct.is_mounted(), false);
    }

    #[test]
    fn test_inactive_elements_new() {
        let inactive = InactiveElements::new();
        assert!(inactive.is_empty());
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_elements_add() {
        let mut inactive = InactiveElements::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        inactive.add(id1);
        assert_eq!(inactive.len(), 1);
        assert!(inactive.contains(id1));
        assert!(!inactive.contains(id2));

        inactive.add(id2);
        assert_eq!(inactive.len(), 2);
        assert!(inactive.contains(id1));
        assert!(inactive.contains(id2));
    }

    #[test]
    fn test_inactive_elements_remove() {
        let mut inactive = InactiveElements::new();
        let id = ElementId::new();

        inactive.add(id);
        assert!(inactive.contains(id));

        let removed = inactive.remove(id);
        assert_eq!(removed, Some(id));
        assert!(!inactive.contains(id));
        assert_eq!(inactive.len(), 0);

        // Removing again returns None
        let removed_again = inactive.remove(id);
        assert_eq!(removed_again, None);
    }

    #[test]
    fn test_inactive_elements_drain() {
        let mut inactive = InactiveElements::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        inactive.add(id1);
        inactive.add(id2);
        inactive.add(id3);
        assert_eq!(inactive.len(), 3);

        let drained: Vec<_> = inactive.drain().collect();
        assert_eq!(drained.len(), 3);
        assert!(drained.contains(&id1));
        assert!(drained.contains(&id2));
        assert!(drained.contains(&id3));

        assert!(inactive.is_empty());
    }

    #[test]
    fn test_inactive_elements_clear() {
        let mut inactive = InactiveElements::new();
        inactive.add(ElementId::new());
        inactive.add(ElementId::new());
        assert_eq!(inactive.len(), 2);

        inactive.clear();
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_element_lifecycle_default() {
        let widget = TestStatelessWidget { value: 42 };
        let element = ComponentElement::new(widget);

        // Default lifecycle is Active
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_element_deactivate_activate_default() {
        let widget = TestStatelessWidget { value: 42 };
        let mut element = ComponentElement::new(widget);

        // Default implementations should not panic
        element.deactivate();
        element.activate();
    }

    #[test]
    fn test_element_did_change_dependencies_default() {
        let widget = TestStatelessWidget { value: 42 };
        let mut element = ComponentElement::new(widget);

        // Default implementation should not panic
        element.did_change_dependencies();
    }

    #[test]
    fn test_element_update_slot_for_child_default() {
        let widget = TestStatelessWidget { value: 42 };
        let mut element = ComponentElement::new(widget);
        let child_id = ElementId::new();

        // Default implementation should not panic
        element.update_slot_for_child(child_id, 1);
    }

    #[test]
    fn test_element_forget_child_default() {
        let widget = TestStatelessWidget { value: 42 };
        let mut element = ComponentElement::new(widget);
        let child_id = ElementId::new();

        // Default implementation should not panic
        element.forget_child(child_id);
    }
}



