//! Type-erased view trait
//!
//! AnyView provides type erasure for View, allowing heterogeneous view storage.
//! This is essential for storing different view types together (e.g., in Vec or Option).

use super::IntoElement;
use crate::element::Element;
use std::any::Any;

/// Type-erased view trait
///
/// This trait allows storing views of different types in the same collection
/// or struct field. It's implemented automatically for all types that implement View.
///
/// # Simplified API
///
/// With the new simplified View API, AnyView is much simpler:
/// - No State GAT (use hooks instead)
/// - No rebuild() (framework handles it)
/// - No teardown() (automatic cleanup)
///
/// # Example
///
/// ```rust,ignore
/// // Store different view types together
/// let views: Vec<Box<dyn AnyView>> = vec![
///     Box::new(Counter),
///     Box::new(Text::new("Hello")),
/// ];
///
/// // Build them all
/// for view in views {
///     let element = view.build_any();
/// }
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

    /// Build this view into an element (type-erased)
    ///
    /// Uses thread-local BuildContext to call View::build() and convert to Element.
    ///
    /// # Panics
    ///
    /// Panics if called outside of build phase (when BuildContext is not set).
    fn build_any(&self) -> Element;

    /// Check if this view has the same type as another
    ///
    /// Used to determine if views can be compared or need full rebuild.
    fn same_type(&self, other: &dyn AnyView) -> bool;
}

/// Blanket implementation of AnyView for all View types
///
/// This automatically makes every View type compatible with type erasure.
impl<T: super::view::View + Clone> AnyView for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn AnyView> {
        Box::new(self.clone())
    }

    fn build_any(&self) -> Element {
        use super::build_context::current_build_context;

        // Get BuildContext from thread-local
        let ctx = current_build_context();

        // Call View::build() and convert to Element
        let element_like = self.clone().build(ctx);
        element_like.into_element()
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
    use crate::view::View;

    // Mock types for testing
    #[derive(Clone, Debug)]
    struct MockView {
        value: i32,
    }

    impl View for MockView {
        fn build(self, _ctx: &super::BuildContext) -> impl IntoElement {
            // Return a mock element
            crate::view::LeafRenderBuilder::new(MockRender)
        }
    }

    #[derive(Debug)]
    struct MockRender;

    impl crate::render::LeafRender for MockRender {
        type Metadata = ();

        fn layout(&mut self, constraints: crate::foundation::BoxConstraints) -> crate::foundation::Size {
            constraints.min
        }

        fn paint(&self, _offset: crate::foundation::Offset) -> crate::engine::BoxedLayer {
            Box::new(crate::engine::ContainerLayer::new())
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
