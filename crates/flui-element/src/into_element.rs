//! IntoElement trait - Convert types into Element
//!
//! This module provides the `IntoElement` trait for converting various types
//! into Element instances.

use crate::element::Element;

/// Trait for types that can be converted into an Element.
///
/// This is the primary way to create Elements from ViewObjects and other types.
///
/// # Four-Tree Architecture
///
/// In the four-tree architecture, ViewObjects and RenderObjects must be inserted
/// into their respective trees (ViewTree, RenderTree) first to obtain IDs.
/// Then Element can be created referencing those IDs.
///
/// # Example
///
/// ```rust,ignore
/// use flui_element::{IntoElement, Element};
///
/// // Create element from existing element (identity)
/// let element = my_element.into_element();
///
/// // Create element from unit (empty element)
/// let element = ().into_element();
///
/// // Create element from Option
/// let element = Some(my_element).into_element();
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
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
