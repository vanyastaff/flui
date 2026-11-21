//! IntoView and IntoElement traits for ergonomic View/Element composition.
//!
//! Allows various types to be converted into Views and Elements, enabling
//! a fluent API for building UI trees.

use std::any::Any;
use std::rc::Rc;

use crate::element::ElementKind;

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
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used where a view is expected",
    label = "not a `View`, and has no `IntoView` conversion",
    note = "every `View` is `IntoView` through a blanket impl: make `{Self}` a view (`#[derive(Clone, StatelessView)]` + `impl StatelessView for {Self}`), or implement `IntoView` for a value that converts into one"
)]
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
    /// Convert this value into an element kind.
    fn into_element(self) -> ElementKind;
}

/// Blanket implementation for types that implement View.
///
/// This allows any View to be used where an Element is needed.
impl<V: View> IntoElement for V {
    #[inline]
    fn into_element(self) -> ElementKind {
        Rc::new(self).create_element()
    }
}

/// The view `view` offers, as a `V` in a shared allocation: the innermost
/// [`BoxedView`]'s own `Rc` when `view` is one, otherwise a typed clone of the
/// `V` offered by reference. `None` when the view is not a `V`.
pub(crate) fn shared_view_of<V: View + Clone>(view: &dyn View) -> Option<Rc<V>> {
    let mut effective = view;
    let mut shared: Option<&Rc<dyn View>> = None;
    while let Some(boxed) = (effective as &dyn Any).downcast_ref::<BoxedView>() {
        shared = Some(boxed.shared());
        effective = boxed.as_view();
    }
    match shared {
        Some(shared) => (Rc::clone(shared) as Rc<dyn Any>).downcast::<V>().ok(),
        None => (effective as &dyn Any)
            .downcast_ref::<V>()
            .cloned()
            .map(Rc::new),
    }
}

/// An element kind for type erasure.
///
/// Used when the concrete Element type cannot be known at compile time.
pub struct BoxedElement(ElementKind);

impl std::fmt::Debug for BoxedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BoxedElement")
            .field(&format_args!("<{:?}>", self.0.element().view_type_id()))
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
        self.0.element()
    }

    /// Get a mutable reference to the inner Element.
    pub fn inner_mut(&mut self) -> &mut dyn ElementBase {
        self.0.element_mut()
    }

    /// Consume and return the inner element kind.
    pub fn into_inner(self) -> ElementKind {
        self.0
    }
}

/// A type-erased, shared View.
///
/// Used when the concrete View type cannot be known at compile time,
/// such as in collections of heterogeneous Views, and for every child a
/// widget stores. Cloning shares the view instead of copying it, and that
/// sharing is the view's identity: an element that last took its
/// configuration from a `BoxedView` and is offered the same one again
/// (`widget.child` handed back unchanged by a parent that rebuilt) has
/// nothing to do, so it and everything below it are not built again. See
/// `ARCHITECTURE.md`, "A child the parent hands back unchanged is not built
/// again".
///
/// Boxing a `BoxedView` returns it unchanged, so a view keeps its identity
/// however many wrappers pass it along.
pub struct BoxedView(Rc<dyn View>);

impl BoxedView {
    /// Erase `view`. A `BoxedView` comes back as it is, keeping its identity.
    pub fn new<V: View>(view: V) -> Self {
        let mut slot = Some(view);
        // `Option<V>` is `Option<BoxedView>` only when `V` is `BoxedView`.
        if let Some(erased) = (&mut slot as &mut dyn Any).downcast_mut::<Option<BoxedView>>() {
            return erased
                .take()
                .expect("BUG: the slot was filled one statement above");
        }
        Self(Rc::new(
            slot.take()
                .expect("BUG: the slot was filled one statement above"),
        ))
    }

    /// The erased view.
    #[must_use]
    pub fn as_view(&self) -> &dyn View {
        &*self.0
    }

    /// The shared allocation that is this view's identity.
    pub(crate) fn shared(&self) -> &Rc<dyn View> {
        &self.0
    }

    /// The view in `shared`, which becomes this view's identity. A `BoxedView`
    /// in `shared` stays a layer of its own: the dispatch unwraps it.
    pub(crate) fn from_shared(shared: Rc<dyn View>) -> Self {
        Self(shared)
    }

    /// Create the element of the view inside, which keeps this very shared
    /// allocation as its configuration.
    pub(crate) fn create_shared_element(&self) -> ElementKind {
        Rc::clone(&self.0).create_element()
    }
}

impl From<Box<dyn View>> for BoxedView {
    fn from(view: Box<dyn View>) -> Self {
        let inner: &dyn Any = &*view;
        if inner.is::<BoxedView>() {
            let erased: Box<dyn Any> = view;
            return match erased.downcast::<BoxedView>() {
                Ok(erased) => *erased,
                Err(_) => unreachable!("BUG: the view was just checked to be a BoxedView"),
            };
        }
        Self(Rc::from(view))
    }
}

impl Clone for BoxedView {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
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
    /// Creates the element of the view inside, which keeps this very shared
    /// allocation as its configuration: the first time its parent offers the
    /// same view again is known to be a no-op, and the element copies nothing.
    fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
        Rc::clone(&self.0).create_element()
    }

    fn view_type_id(&self) -> std::any::TypeId {
        self.0.view_type_id()
    }

    fn can_update(&self, old: &dyn View) -> bool {
        self.0.can_update(old)
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        self.0.key()
    }
}

/// Extension trait for boxing Views.
pub trait ViewExt: View + Sized {
    /// Erase this View's type. A [`BoxedView`] is returned as it is.
    fn boxed(self) -> BoxedView {
        BoxedView::new(self)
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
