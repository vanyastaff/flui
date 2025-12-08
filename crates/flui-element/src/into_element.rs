//! IntoElement trait - Convert types into Element
//!
//! This module provides the `IntoElement` trait for converting various types
//! into Element instances.

use crate::element::{Element, ViewElement};
use crate::ViewObject;
use flui_rendering::{Arity, BoxRenderWrapper, ProtocolId, RenderElement, SliverRenderWrapper};

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
///
/// **DEPRECATED**: In the four-tree architecture, view objects must be inserted into
/// ViewTree first to get a ViewId. This implementation now returns an empty element.
///
/// **Migration**:
/// ```rust,ignore
/// // Old way:
/// let element = view_object.into_element();
///
/// // New way:
/// let view_id = view_tree.insert(view_object);
/// let element = Element::view(Some(view_id), mode);
/// ```
impl IntoElement for Box<dyn ViewObject> {
    fn into_element(self) -> Element {
        // Cannot create element without inserting into ViewTree first
        tracing::warn!(
            "IntoElement for Box<dyn ViewObject> is deprecated. \
             View objects must be inserted into ViewTree first."
        );
        Element::empty()
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
///
/// **DEPRECATED**: In the four-tree architecture, render objects must be inserted into
/// RenderTree first to get a RenderId. This implementation now returns an empty element.
///
/// **Migration**:
/// ```rust,ignore
/// // Old way:
/// let element = box_render_wrapper.into_element();
///
/// // New way:
/// let render_id = render_tree.insert(box_render_wrapper);
/// let element = Element::render_with_arity(Some(render_id), ProtocolId::Box, arity);
/// ```
impl<A: Arity> IntoElement for BoxRenderWrapper<A> {
    fn into_element(self) -> Element {
        // Cannot create element without inserting into RenderTree first
        tracing::warn!(
            "IntoElement for BoxRenderWrapper is deprecated. \
             Render objects must be inserted into RenderTree first."
        );
        Element::empty()
    }
}

/// SliverRenderWrapper converts to a RenderElement with Sliver protocol.
///
/// **DEPRECATED**: In the four-tree architecture, render objects must be inserted into
/// RenderTree first to get a RenderId. This implementation now returns an empty element.
///
/// **Migration**:
/// ```rust,ignore
/// // Old way:
/// let element = sliver_render_wrapper.into_element();
///
/// // New way:
/// let render_id = render_tree.insert(sliver_render_wrapper);
/// let element = Element::render_with_arity(Some(render_id), ProtocolId::Sliver, arity);
/// ```
impl<A: Arity> IntoElement for SliverRenderWrapper<A> {
    fn into_element(self) -> Element {
        // Cannot create element without inserting into RenderTree first
        tracing::warn!(
            "IntoElement for SliverRenderWrapper is deprecated. \
             Render objects must be inserted into RenderTree first."
        );
        Element::empty()
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
    #[allow(deprecated)]
    fn test_boxed_view_object_into_element() {
        let view_obj: Box<dyn ViewObject> = Box::new(TestViewObject);
        let element = view_obj.into_element();

        // In four-tree architecture, this returns empty element (deprecated behavior)
        match element {
            Element::View(view_elem) => {
                assert_eq!(view_elem.view_mode(), ViewMode::Empty);
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
