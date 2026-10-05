//! Support macros shared across widget families. The impl macro is also
//! exported, doc-hidden, through [`crate::__private`] for the sibling
//! `flui-*` widget crates. The event-callback adapters store a user's
//! `Fn(&mut EventCx<'_>, ..) -> R` so that its outcome is reported.

use std::rc::Rc;

use flui_view::{EventCx, EventOutcome};

pub(crate) mod retirement;
#[cfg(test)]
pub(crate) mod test_cases;

/// Generate the `View` impl for a multi-child render-object widget generic over
/// a single [`ViewSeq`](flui_view::seq::ViewSeq) type parameter `C`.
///
/// `flui_view::impl_render_view!` only handles concrete (non-generic) types, so
/// generic multi-child widgets (`Flex`/`Row`/`Column`/`Stack`) hand off to this
/// macro instead. It mirrors `impl_render_view!`'s body (a `RenderElement` over
/// a `RenderBehavior`) under the standard multi-child bound
/// `C: ViewSeq + Clone + 'static`.
#[doc(hidden)]
#[macro_export]
macro_rules! __generic_render_view_element {
    ($ty:ident) => {
        impl<C> ::flui_view::View for $ty<C>
        where
            C: ::flui_view::seq::ViewSeq + ::core::clone::Clone + 'static,
        {
            fn create_element(&self) -> ::flui_view::element::ElementKind {
                ::flui_view::element::ElementKind::render_variable(self)
            }
        }
    };
}

pub(crate) use crate::__generic_render_view_element as generic_render_view_element;

/// A stored no-argument event callback.
pub(crate) type EventCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

/// A stored event callback that also receives a value.
pub(crate) type ValueCallback<D> = Rc<dyn Fn(&mut EventCx<'_>, D)>;

/// Store a no-argument event callback, adapted to report its outcome: a
/// refused signal write is logged at the dispatch boundary (ADR-0086).
pub(crate) fn event_callback<F, R>(callback: F) -> EventCallback
where
    F: Fn(&mut EventCx<'_>) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>| callback(cx).report())
}

/// [`event_callback`] for a callback that also receives a value by value
/// (gesture details, the new focus state).
pub(crate) fn value_callback<D, F, R>(callback: F) -> ValueCallback<D>
where
    D: 'static,
    F: Fn(&mut EventCx<'_>, D) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, value: D| callback(cx, value).report())
}

/// A stored event callback that also receives a borrowed value.
pub(crate) type RefCallback<T> = Rc<dyn Fn(&mut EventCx<'_>, &T)>;

/// [`event_callback`] for a callback that also receives a borrowed value
/// (the field's text, a pan-zoom event, a form field's value).
pub(crate) fn ref_callback<T, F, R>(callback: F) -> RefCallback<T>
where
    T: ?Sized + 'static,
    F: for<'a> Fn(&mut EventCx<'_>, &'a T) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, value: &T| callback(cx, value).report())
}
