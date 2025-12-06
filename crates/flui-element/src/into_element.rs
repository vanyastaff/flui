//! IntoElement trait - Convert types into Element
//!
//! This module provides the `IntoElement` trait for converting various types
//! into Element instances.

use crate::element::{Element, ViewElement};
use crate::ViewObject;
use flui_rendering::{Arity, BoxRenderWrapper, ProtocolId, SliverRenderWrapper, RenderElement};

/// Trait for types that can be converted into an Element.
///
/// This is the primary way to create Elements from ViewObjects and other types.
///
/// # Example
///
/// ```rust,ignore
/// use flui_element::{IntoElement, Element};
///
/// let element = my_view_object.into_element();
/// ```
pub trait IntoElement {
    /// Convert this value into an Element.
    fn into_element(self) -> Element;
}

// ============================================================================
// IMPLEMENTATIONS
// ============================================================================

/// Identity implementation - Element converts to itself.
impl IntoElement for Element {
    #[inline]
    fn into_element(self) -> Element {
        self
    }
}

/// Box<dyn ViewObject> converts to ViewElement wrapped in Element.
impl IntoElement for Box<dyn ViewObject> {
    fn into_element(self) -> Element {
        let mode = self.mode();
        let mut view_element = ViewElement::empty();
        view_element.set_view_object_boxed(self);
        view_element.set_view_mode(mode);
        Element::View(view_element)
    }
}

/// Unit type converts to empty Element.
impl IntoElement for () {
    #[inline]
    fn into_element(self) -> Element {
        Element::empty()
    }
}

/// Option<T: IntoElement> converts to Element or empty.
impl<T: IntoElement> IntoElement for Option<T> {
    fn into_element(self) -> Element {
        match self {
            Some(inner) => inner.into_element(),
            None => Element::empty(),
        }
    }
}

/// Vec<Element> converts to a container Element with pending children.
impl IntoElement for Vec<Element> {
    fn into_element(self) -> Element {
        if self.is_empty() {
            Element::empty()
        } else {
            Element::container(Element::boxed_children(self))
        }
    }
}

// ============================================================================
// RENDER WRAPPER IMPLEMENTATIONS
// ============================================================================

/// BoxRenderWrapper converts to a RenderElement with Box protocol.
impl<A: Arity> IntoElement for BoxRenderWrapper<A> {
    fn into_element(self) -> Element {
        // BoxRenderWrapper implements RenderObject, so we can box it directly
        Element::Render(RenderElement::from_boxed(
            Box::new(self),
            ProtocolId::Box,
            A::runtime_arity(),
        ))
    }
}

/// SliverRenderWrapper converts to a RenderElement with Sliver protocol.
impl<A: Arity> IntoElement for SliverRenderWrapper<A> {
    fn into_element(self) -> Element {
        // SliverRenderWrapper implements RenderObject, so we can box it directly
        Element::Render(RenderElement::from_boxed(
            Box::new(self),
            ProtocolId::Sliver,
            A::runtime_arity(),
        ))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildContext;
    use flui_view::ViewMode;
    use std::any::Any;

    struct TestViewObject;

    impl ViewObject for TestViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None
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
        let element = Element::empty();
        let result = element.into_element();
        assert!(matches!(result, Element::View(_)));
    }

    #[test]
    fn test_unit_into_element() {
        let element = ().into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_option_some_into_element() {
        let element = Some(Element::empty()).into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_option_none_into_element() {
        let element: Element = None::<Element>.into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_boxed_view_object_into_element() {
        let view_obj: Box<dyn ViewObject> = Box::new(TestViewObject);
        let element = view_obj.into_element();

        match element {
            Element::View(view_elem) => {
                assert!(view_elem.has_view_object());
                assert_eq!(view_elem.view_mode(), ViewMode::Stateless);
            }
            Element::Render(_) => {
                panic!("Expected View element, got Render element")
            }
        }
    }

    #[test]
    fn test_vec_empty_into_element() {
        let elements: Vec<Element> = vec![];
        let element = elements.into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_vec_with_children_into_element() {
        let elements = vec![Element::empty(), Element::empty()];
        let element = elements.into_element();

        let Element::View(view_elem) = element else {
            panic!("Expected View element, got Render element")
        };
        assert!(view_elem.has_pending_children());
    }
}
