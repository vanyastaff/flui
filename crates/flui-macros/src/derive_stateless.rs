//! Codegen for `#[derive(StatelessView)]`.
//!
//! Generates the `impl View for T` block that backs every stateless
//! widget — the `create_element` boilerplate the manual
//! `impl_stateless_view!` declarative macro used to emit. The trivial
//! authoring shape becomes:
//!
//! ```rust,ignore
//! #[derive(Clone, StatelessView)]
//! struct Greeting { name: String }
//!
//! impl Greeting {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(&self.name)
//!     }
//! }
//! ```
//!
//! No `impl View for Greeting` block, no `impl_stateless_view!`
//! invocation, no `Box::new` at the call site.
//!
//! The generated `impl View` references `flui-view` items via the
//! absolute `::flui_view::…` path: every consumer of the derive must
//! have `flui-view` as a direct dependency, which `flui-view`'s own
//! prelude re-export already enforces — authors write a single
//! `use flui_view::prelude::*;` and pick up both the derive and the
//! supporting trait.
//!
//! Authors who need a typed `key()` (to participate in keyed lists)
//! write a single-method `impl View for Greeting { fn key() { … } }`
//! block alongside the derive AND drop the derive — the derive emits
//! the WHOLE `impl View` block, so an additional `impl View for X`
//! site would conflict. The `#[view(key = "expr")]` derive attribute
//! that auto-wires a field-named key is a deferred ergonomics
//! improvement (post-Catalog.1, plan §"Open Questions").

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_quote};

/// Expand `#[derive(StatelessView)]` into the canonical `impl View` block.
///
/// Returns a `syn::Result` even though the current body never produces
/// an error — the wrap reserves headroom for future attribute parsing
/// (`#[view(key = …)]`, recursive-widget hints) that does need to
/// surface fallible diagnostics through `into_compile_error`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "future-proof against attribute parsing"
)]
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;

    // Honor the user's generic parameters: a stateless widget MAY be
    // generic (`struct Padded<C> { child: C, inset: f32 }`). The derive
    // forwards the generics into the `impl View` block verbatim.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // The bound `Self: Clone + Send + Sync + 'static` is what
    // `StatelessView` itself requires. `View` adds `Downcast + DynClone`
    // — both satisfied by `Clone + 'static` via the blanket impls in
    // `downcast-rs` / `dyn-clone`. We do not add extra bounds here; the
    // user's own `impl StatelessView for #ident` already enforces them
    // and a mismatch surfaces at the call site of `StatelessElement::new`
    // (which carries the bound), not from the derive.
    //
    // `Self: ::flui_view::StatelessView` is the predicate that makes the
    // generated `create_element` body type-check — without it a user
    // who writes `#[derive(StatelessView)]` but forgets the
    // `impl StatelessView for #ident` block would see a confusing
    // error about `StatelessBehavior::ElementBehavior<…>` not being
    // satisfied; with it, the missing impl surfaces immediately as
    // "the trait `StatelessView` is not implemented for `…`" pointing
    // at the derive's call site.
    let mut augmented_where = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    augmented_where
        .predicates
        .push(parse_quote!(Self: ::flui_view::StatelessView));

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::flui_view::View for #ident #ty_generics
        #augmented_where
        {
            fn create_element(&self) -> ::std::boxed::Box<dyn ::flui_view::ElementBase> {
                ::std::boxed::Box::new(
                    ::flui_view::StatelessElement::<Self>::new(
                        self,
                        ::flui_view::StatelessBehavior,
                    ),
                )
            }
        }
    })
}
