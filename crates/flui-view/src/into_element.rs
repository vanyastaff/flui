//! IntoElement trait for view-to-element conversion.
//!
//! Defines how views are converted into elements for the element tree.

use flui_foundation::ElementId;

// ============================================================================
// INTO ELEMENT TRAIT
// ============================================================================

/// Trait for types that can be converted into elements.
///
/// This is the primary mechanism for composing views. Any type implementing
/// this trait can be returned from a view's `build()` method.
///
/// # Implementors
///
/// - All view types (`StatelessView`, `StatefulView`, etc.)
/// - Tuples of views (for composing multiple children)
/// - `Option<T>` where `T: IntoElement` (for conditional rendering)
/// - `Vec<T>` where `T: IntoElement` (for dynamic lists)
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Can return any IntoElement type
///         Column::new()
///             .child(Text::new("Hello"))
///             .child(Text::new("World"))
///     }
/// }
/// ```
pub trait IntoElement: Send + 'static {
    /// The element type produced by this conversion.
    type Element: Send + 'static;

    /// Convert into an element.
    ///
    /// Called by the framework during the build phase to convert
    /// views into concrete elements.
    fn into_element(self) -> Self::Element;
}

// ============================================================================
// BLANKET IMPLEMENTATIONS
// ============================================================================

/// Unit type produces no element (empty).
impl IntoElement for () {
    type Element = ();

    fn into_element(self) -> Self::Element {}
}

/// Optional element support.
impl<T: IntoElement> IntoElement for Option<T> {
    type Element = Option<T::Element>;

    fn into_element(self) -> Self::Element {
        self.map(IntoElement::into_element)
    }
}

/// Vec of elements support.
impl<T: IntoElement> IntoElement for Vec<T> {
    type Element = Vec<T::Element>;

    fn into_element(self) -> Self::Element {
        self.into_iter().map(IntoElement::into_element).collect()
    }
}

// ============================================================================
// ELEMENT ID AS ELEMENT
// ============================================================================

/// ElementId can be used directly as an element reference.
impl IntoElement for ElementId {
    type Element = ElementId;

    fn into_element(self) -> Self::Element {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_into_element() {
        let _: () = ().into_element();
    }

    #[test]
    fn test_option_into_element() {
        let some: Option<()> = Some(()).into_element();
        assert!(some.is_some());

        let none: Option<()> = None::<()>.into_element();
        assert!(none.is_none());
    }

    #[test]
    fn test_vec_into_element() {
        let vec: Vec<()> = vec![(), (), ()].into_element();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_element_id_into_element() {
        let id = ElementId::new(42);
        let result = id.into_element();
        assert_eq!(result, ElementId::new(42));
    }
}
