//! Support macros shared across widget families. The impl macro is also
//! exported, doc-hidden, through [`crate::__private`] for the sibling
//! `flui-*` widget crates.

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
