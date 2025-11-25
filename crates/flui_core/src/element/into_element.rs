//! IntoElement trait - Convert views/renders into Element nodes.
//!
//! # Overview
//!
//! The `IntoElement` trait provides a way to convert various types (Views, ViewObjects,
//! Elements, etc.) into Element nodes that can be inserted into the element tree.
//!
//! # Sealed Trait
//!
//! This is a sealed trait - only types explicitly defined here can implement it.
//! This prevents external code from creating arbitrary Element-convertible types
//! and maintains control over the element creation protocol.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_core::element::{Element, IntoElement};
//! use flui_core::view::StatelessView;
//!
//! let view = MyStatelessView { text: "Hello".to_string() };
//! let element: Element = view.into_element();
//! ```

use crate::element::Element;
use crate::view::ViewObject;

/// Converts a type into an Element.
///
/// This sealed trait enables automatic conversion of Views, ViewObjects,
/// and other types into Element nodes for insertion into the element tree.
///
/// # Sealed Trait
///
/// This is a sealed trait - only the types explicitly implemented below
/// can implement this trait. This ensures type safety and prevents misuse.
///
/// If you need to convert a custom type to Element, it should go through
/// the appropriate View trait (StatelessView, StatefulView, RenderView, etc).
pub trait IntoElement: sealed::Sealed + Sized + 'static {
    /// Convert this value into an Element.
    fn into_element(self) -> Element;
}

/// Sealed trait marker - prevents external implementations.
pub(crate) mod sealed {
    /// Marker trait to seal IntoElement.
    pub trait Sealed {}
}

// ============================================================================
// IMPLEMENTATIONS FOR ELEMENT
// ============================================================================

impl sealed::Sealed for Element {}

impl IntoElement for Element {
    /// Element already is an Element, so conversion is identity.
    #[inline]
    fn into_element(self) -> Element {
        self
    }
}

// ============================================================================
// IMPLEMENTATIONS FOR VIEWOBJECT
// ============================================================================

impl sealed::Sealed for Box<dyn ViewObject> {}

impl IntoElement for Box<dyn ViewObject> {
    /// Wrap ViewObject in an Element.
    #[inline]
    fn into_element(self) -> Element {
        Element::new(self)
    }
}

// ============================================================================
// IMPLEMENTATIONS FOR OPTION
// ============================================================================

impl<T: IntoElement> sealed::Sealed for Option<T> {}

impl<T: IntoElement> IntoElement for Option<T> {
    /// Convert Option into Element.
    ///
    /// Some(value) converts to the inner element.
    /// None will panic - use Option only when you're sure it's Some.
    fn into_element(self) -> Element {
        match self {
            Some(element) => element.into_element(),
            None => {
                // Use an empty ViewObject that does nothing
                // This allows Option<T>::None to represent "no child" cleanly
                Element::new(Box::new(crate::view::wrappers::StatelessViewWrapper::new(
                    crate::view::EmptyView,
                )))
            }
        }
    }
}

// ============================================================================
// IMPLEMENTATIONS FOR TUPLE (MULTI-CHILD)
// ============================================================================

/// Tuple types can be converted to Elements for multi-child containers.
/// For example: (RenderColumn, vec![child1, child2])

impl<T0: IntoElement, T1: IntoElement> sealed::Sealed for (T0, T1) {}

impl<T0: IntoElement, T1: IntoElement> IntoElement for (T0, T1) {
    fn into_element(self) -> Element {
        // Tuples are used for RenderObject + children patterns in view building.
        // This should not be called directly - the framework handles tuple conversion
        // during the build process through specialized machinery.
        panic!(
            "Direct tuple conversion not supported. \
             Tuples are handled automatically during view building. \
             If you see this error, there may be an issue with your view's build() method."
        )
    }
}

// ============================================================================
// IMPLEMENTATIONS FOR VEC (MULTI-CHILD SEQUENCES)
// ============================================================================

impl<T: IntoElement> sealed::Sealed for Vec<T> {}

impl<T: IntoElement> IntoElement for Vec<T> {
    fn into_element(self) -> Element {
        // Vec<T> is used for multi-child containers in view building.
        // This should not be called directly - the framework handles Vec conversion
        // during the build process through specialized machinery.
        panic!(
            "Direct Vec conversion not supported. \
             Vec<Element> is handled automatically during multi-child view building. \
             If you see this error, there may be an issue with your view's build() method."
        )
    }
}

// ============================================================================
// IMPLEMENTATIONS FOR UNIT (EMPTY/PLACEHOLDER)
// ============================================================================

impl sealed::Sealed for () {}

impl IntoElement for () {
    fn into_element(self) -> Element {
        // Use an empty ViewObject for unit type
        // This allows () to represent "no content" in view building
        Element::new(Box::new(crate::view::wrappers::StatelessViewWrapper::new(
            crate::view::EmptyView,
        )))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ViewMode;
    use std::any::Any;

    struct MockViewObject;

    impl ViewObject for MockViewObject {
        fn build(&mut self, _ctx: &crate::view::BuildContext) -> Element {
            Element::new(Box::new(MockViewObject))
        }
        fn init(&mut self, _ctx: &crate::view::BuildContext) {}
        fn did_change_dependencies(&mut self, _ctx: &crate::view::BuildContext) {}
        fn did_update(&mut self, _new_view: &dyn Any, _ctx: &crate::view::BuildContext) {}
        fn deactivate(&mut self, _ctx: &crate::view::BuildContext) {}
        fn dispose(&mut self, _ctx: &crate::view::BuildContext) {}
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_element_into_element() {
        let element = Element::new(Box::new(MockViewObject));
        let result = element.into_element();
        assert_eq!(result.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_box_view_object_into_element() {
        let view_object: Box<dyn ViewObject> = Box::new(MockViewObject);
        let element = view_object.into_element();
        assert_eq!(element.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_option_some_into_element() {
        let view_object = Box::new(MockViewObject);
        let option: Option<Box<dyn ViewObject>> = Some(view_object);
        let element = option.into_element();
        assert_eq!(element.mode(), ViewMode::Stateless);
    }

    #[test]
    #[should_panic(expected = "Option::None cannot be converted")]
    fn test_option_none_into_element_panics() {
        let option: Option<Element> = None;
        let _ = option.into_element();
    }
}
