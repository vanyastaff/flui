//! IntoView and IntoElement traits for ergonomic View/Element composition.
//!
//! Allows various types to be converted into Views and Elements, enabling
//! a fluent API for building UI trees.

use super::view::{ElementBase, View};

/// Trait for types that can be converted into a View.
///
/// This enables ergonomic composition of Views:
///
/// ```rust,ignore
/// fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///     Column::new()
///         .child("Hello")           // &str implements IntoView
///         .child(42)                // numbers implement IntoView
///         .child(Text::new("Hi"))   // Views implement IntoView
/// }
/// ```
///
/// # Implementing IntoView
///
/// Most View types should implement `IntoView` by returning themselves:
///
/// ```rust,ignore
/// impl IntoView for MyView {
///     type View = Self;
///     fn into_view(self) -> Self::View { self }
/// }
/// ```
pub trait IntoView {
    /// The View type this converts into.
    type View: View;

    /// Convert this value into a View.
    fn into_view(self) -> Self::View;
}

/// Blanket implementation for types that are already Views.
impl<V: View> IntoView for V {
    type View = Self;

    #[inline]
    fn into_view(self) -> Self::View {
        self
    }
}

// ============================================================================
// IntoElement
// ============================================================================

/// Trait for types that can be converted into an Element.
///
/// This enables ergonomic creation of Elements from Views or other types:
///
/// ```rust,ignore
/// fn mount_child<T: IntoElement>(&mut self, child: T) {
///     let element = child.into_element();
///     element.mount(Some(self.id), 0);
/// }
/// ```
///
/// # Blanket Implementation
///
/// All `View` types automatically implement `IntoElement` by calling
/// `create_element()`. This means you can pass any View where an
/// `IntoElement` is expected.
pub trait IntoElement {
    /// Convert this value into a boxed Element.
    fn into_element(self) -> Box<dyn ElementBase>;
}

/// Blanket implementation for types that implement View.
///
/// This allows any View to be used where an Element is needed.
impl<V: View> IntoElement for V {
    #[inline]
    fn into_element(self) -> Box<dyn ElementBase> {
        self.create_element()
    }
}

/// Implementation for boxed Views.
impl IntoElement for Box<dyn View> {
    #[inline]
    fn into_element(self) -> Box<dyn ElementBase> {
        self.create_element()
    }
}

/// A boxed Element for type erasure.
///
/// Used when the concrete Element type cannot be known at compile time.
pub struct BoxedElement(pub Box<dyn ElementBase>);

impl std::fmt::Debug for BoxedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BoxedElement")
            .field(&format_args!("<{:?}>", self.0.view_type_id()))
            .finish()
    }
}

impl BoxedElement {
    /// Create a new BoxedElement from any type that implements IntoElement.
    pub fn new<T: IntoElement>(value: T) -> Self {
        BoxedElement(value.into_element())
    }

    /// Get a reference to the inner Element.
    pub fn inner(&self) -> &dyn ElementBase {
        &*self.0
    }

    /// Get a mutable reference to the inner Element.
    pub fn inner_mut(&mut self) -> &mut dyn ElementBase {
        &mut *self.0
    }

    /// Consume and return the inner boxed Element.
    pub fn into_inner(self) -> Box<dyn ElementBase> {
        self.0
    }
}

/// Extension trait for boxing Elements.
pub trait ElementExt: ElementBase + Sized {
    /// Box this Element for type erasure.
    fn boxed(self) -> BoxedElement {
        BoxedElement(Box::new(self))
    }
}

// Note: Can't impl ElementExt for all ElementBase because ElementBase is not Sized
// Users should use BoxedElement::new() instead

/// A boxed View for type erasure.
///
/// Used when the concrete View type cannot be known at compile time,
/// such as in collections of heterogeneous Views.
pub struct BoxedView(pub Box<dyn View>);

impl Clone for BoxedView {
    fn clone(&self) -> Self {
        BoxedView(dyn_clone::clone_box(&*self.0))
    }
}

impl std::fmt::Debug for BoxedView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BoxedView")
            .field(&format_args!("<{:?}>", self.0.view_type_id()))
            .finish()
    }
}

impl View for BoxedView {
    fn create_element(&self) -> Box<dyn super::view::ElementBase> {
        self.0.create_element()
    }

    fn view_type_id(&self) -> std::any::TypeId {
        self.0.view_type_id()
    }

    fn can_update(&self, old: &dyn View) -> bool {
        self.0.can_update(old)
    }

    fn key(&self) -> Option<&dyn super::view::ViewKey> {
        self.0.key()
    }
}

/// Extension trait for boxing Views.
pub trait ViewExt: View + Sized {
    /// Box this View for type erasure.
    fn boxed(self) -> BoxedView {
        BoxedView(Box::new(self))
    }
}

impl<V: View> ViewExt for V {}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time check that IntoView works with Views
    fn _takes_into_view<T: IntoView>(_: T) {}

    fn _assert_boxed_view_is_view(_: BoxedView) {
        // BoxedView implements View
    }
}
