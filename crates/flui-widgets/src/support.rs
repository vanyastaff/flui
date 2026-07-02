//! Crate-internal support macros shared across widget families.

/// Generate the `View` impl for a multi-child render-object widget generic over
/// a single [`ViewSeq`](flui_view::seq::ViewSeq) type parameter `C`.
///
/// `flui_view::impl_render_view!` only handles concrete (non-generic) types, so
/// generic multi-child widgets (`Flex`/`Row`/`Column`/`Stack`) hand off to this
/// macro instead. It mirrors `impl_render_view!`'s body (a `RenderElement` over
/// a `RenderBehavior`) under the standard multi-child bound
/// `C: ViewSeq + Clone + Send + Sync + 'static`.
macro_rules! generic_render_view_element {
    ($ty:ident) => {
        impl<C> ::flui_view::View for $ty<C>
        where
            C: ::flui_view::seq::ViewSeq + ::core::clone::Clone + Send + Sync + 'static,
        {
            fn create_element(&self) -> ::flui_view::element::ElementKind {
                ::flui_view::element::ElementKind::render_variable(self)
            }
        }
    };
}

pub(crate) use generic_render_view_element;
