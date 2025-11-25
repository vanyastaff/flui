//! IntoElement trait - Convert types into Element nodes
//!
//! # Overview
//!
//! The `IntoElement` trait provides a way to convert various types into
//! Element nodes that can be inserted into the element tree.
//!
//! # Design
//!
//! This trait is intentionally minimal in flui-element.
//! More complex conversions (Vec, tuples, Views) are handled in flui-view.

use crate::Element;

/// Converts a type into an Element.
///
/// This trait enables automatic conversion of types into Element nodes
/// for insertion into the element tree.
///
/// # Core Implementations (this crate)
///
/// - `Element` - Identity conversion
/// - `()` - Empty element
/// - `Option<T>` where `T: IntoElement`
///
/// # Extended Implementations (in flui-view)
///
/// - All view types (`StatelessView`, `StatefulView`, etc.)
/// - ViewObject wrappers
/// - `Vec<T>` and tuples (via Children)
pub trait IntoElement: Send + 'static {
    /// Convert this value into an Element.
    fn into_element(self) -> Element;
}

// ============================================================================
// IMPLEMENTATION FOR ELEMENT
// ============================================================================

impl IntoElement for Element {
    /// Element already is an Element - identity conversion.
    #[inline]
    fn into_element(self) -> Element {
        self
    }
}

// ============================================================================
// IMPLEMENTATION FOR UNIT TYPE
// ============================================================================

impl IntoElement for () {
    /// Unit type converts to an empty element (no content).
    #[inline]
    fn into_element(self) -> Element {
        Element::empty()
    }
}

// ============================================================================
// IMPLEMENTATION FOR OPTION
// ============================================================================

impl<T: IntoElement> IntoElement for Option<T> {
    /// Convert Option into Element.
    ///
    /// - `Some(value)` converts to the inner element
    /// - `None` converts to an empty element
    #[inline]
    fn into_element(self) -> Element {
        match self {
            Some(value) => value.into_element(),
            None => Element::empty(),
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
        let converted = element.into_element();
        assert!(!converted.has_view_object());
    }

    #[test]
    fn test_unit_into_element() {
        let element = ().into_element();
        assert!(!element.has_view_object());
    }

    #[test]
    fn test_option_some_into_element() {
        let some: Option<()> = Some(());
        let element = some.into_element();
        assert!(!element.has_view_object());
    }

    #[test]
    fn test_option_none_into_element() {
        let none: Option<()> = None;
        let element = none.into_element();
        assert!(!element.has_view_object());
    }
}
