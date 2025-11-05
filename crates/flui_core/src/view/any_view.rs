//! Type-erased view trait
//!
//! AnyView provides type erasure for View, allowing heterogeneous view storage.
//! This is essential for ComponentElement to store different view types.

use super::view::{ChangeFlags, ViewElement};
use super::build_context::BuildContext;
use std::any::Any;

/// Type-erased view trait
///
/// This trait allows storing views of different types in the same collection
/// or struct field. It's implemented automatically for all types that implement View.
///
/// # Example
///
/// ```rust,ignore
/// // Store different view types together
/// let views: Vec<Box<dyn AnyView>> = vec![
///     Box::new(Counter { count: 0 }),
///     Box::new(Text { content: "Hello".to_string() }),
/// ];
/// ```
pub trait AnyView: 'static {
    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as mutable Any for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Clone this view into a box
    ///
    /// Required because dyn AnyView is not Clone.
    fn clone_box(&self) -> Box<dyn AnyView>;

    /// Build initial element from this view (type-erased)
    ///
    /// Returns:
    /// - Box<dyn ViewElement>: The created element
    /// - Box<dyn Any>: The view state
    fn build_any(&self, ctx: &mut BuildContext) -> (Box<dyn ViewElement>, Box<dyn Any>);

    /// Rebuild existing element with new view (type-erased)
    ///
    /// # Parameters
    ///
    /// - `prev`: Previous view (must be same concrete type)
    /// - `state`: Mutable state from previous build
    /// - `element`: Element to update
    ///
    /// # Returns
    ///
    /// ChangeFlags indicating what changed
    ///
    /// # Panics
    ///
    /// Panics if `prev`, `state`, or `element` are not the correct concrete types.
    fn rebuild_any(
        &self,
        prev: &dyn AnyView,
        state: &mut dyn Any,
        element: &mut dyn ViewElement,
    ) -> ChangeFlags;

    /// Teardown when view is removed (type-erased)
    ///
    /// # Panics
    ///
    /// Panics if `state` or `element` are not the correct concrete types.
    fn teardown_any(&self, state: &mut dyn Any, element: &mut dyn ViewElement);

    /// Check if this view has the same type as another
    ///
    /// Used to determine if views can be diffed or need full rebuild.
    fn same_type(&self, other: &dyn AnyView) -> bool;
}

/// Blanket implementation of AnyView for all View types
///
/// This automatically makes every View type compatible with type erasure.
impl<T: super::view::View> AnyView for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn AnyView> {
        Box::new(self.clone())
    }

    fn build_any(&self, ctx: &mut BuildContext) -> (Box<dyn ViewElement>, Box<dyn Any>) {
        let (element, state) = self.clone().build(ctx);
        (Box::new(element), Box::new(state))
    }

    fn rebuild_any(
        &self,
        prev: &dyn AnyView,
        state: &mut dyn Any,
        element: &mut dyn ViewElement,
    ) -> ChangeFlags {
        // Downcast to concrete types
        let prev = prev.as_any()
            .downcast_ref::<T>()
            .expect("rebuild_any called with wrong prev type");

        let state = state
            .downcast_mut::<T::State>()
            .expect("rebuild_any called with wrong state type");

        let element = element
            .as_any_mut()
            .downcast_mut::<T::Element>()
            .expect("rebuild_any called with wrong element type");

        self.clone().rebuild(prev, state, element)
    }

    fn teardown_any(&self, state: &mut dyn Any, element: &mut dyn ViewElement) {
        // Downcast to concrete types
        let state = state
            .downcast_mut::<T::State>()
            .expect("teardown_any called with wrong state type");

        let element = element
            .as_any_mut()
            .downcast_mut::<T::Element>()
            .expect("teardown_any called with wrong element type");

        self.teardown(state, element)
    }

    fn same_type(&self, other: &dyn AnyView) -> bool {
        other.as_any().is::<T>()
    }
}

impl Clone for Box<dyn AnyView> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::view::{View, ViewElement, ChangeFlags};
    use crate::element::Element;

    // Mock types for testing
    #[derive(Clone, Debug)]
    struct MockView {
        value: i32,
    }

    struct MockElement;

    impl ViewElement for MockElement {
        fn into_element(self: Box<Self>) -> Element {
            unimplemented!("test mock")
        }

        fn mark_dirty(&mut self) {}

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    impl View for MockView {
        type State = ();
        type Element = MockElement;

        fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
            (MockElement, ())
        }
    }

    #[test]
    fn test_any_view_clone() {
        let view = MockView { value: 42 };
        let boxed: Box<dyn AnyView> = Box::new(view);
        let cloned = boxed.clone();

        assert_eq!(
            boxed.as_any().downcast_ref::<MockView>().unwrap().value,
            cloned.as_any().downcast_ref::<MockView>().unwrap().value
        );
    }

    #[test]
    fn test_any_view_same_type() {
        let view1 = MockView { value: 42 };
        let view2 = MockView { value: 100 };

        let boxed1: Box<dyn AnyView> = Box::new(view1);
        let boxed2: Box<dyn AnyView> = Box::new(view2);

        assert!(boxed1.same_type(&*boxed2));
    }

    #[test]
    fn test_any_view_downcast() {
        let view = MockView { value: 42 };
        let boxed: Box<dyn AnyView> = Box::new(view);

        let concrete = boxed.as_any().downcast_ref::<MockView>().unwrap();
        assert_eq!(concrete.value, 42);
    }
}
