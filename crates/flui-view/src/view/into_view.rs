//! IntoView trait for ergonomic View composition.
//!
//! Allows various types to be converted into Views, enabling
//! a fluent API for building UI trees.

use super::view::View;

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

/// A boxed View for type erasure.
///
/// Used when the concrete View type cannot be known at compile time,
/// such as in collections of heterogeneous Views.
pub struct BoxedView(pub Box<dyn View>);

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

    fn as_any(&self) -> &dyn std::any::Any {
        self.0.as_any()
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
