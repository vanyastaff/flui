//! Codegen for `#[derive(StatefulView)]`.
//!
//! Generates the `impl View for T` block that backs every stateful
//! widget — the `create_element` boilerplate the manual
//! `impl_stateful_view!` declarative macro used to emit. The trivial
//! authoring shape becomes:
//!
//! ```rust,ignore
//! #[derive(Clone, StatefulView)]
//! struct Counter { initial: u32 }
//!
//! struct CounterState { count: u32 }
//!
//! impl StatefulView for Counter {
//!     type State = CounterState;
//!     fn create_state(&self) -> CounterState {
//!         CounterState { count: self.initial }
//!     }
//! }
//!
//! impl ViewState<Counter> for CounterState {
//!     fn build(&self, view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(format!("Count: {}", self.count))
//!     }
//! }
//! ```
//!
//! No `impl View for Counter` block, no `impl_stateful_view!`
//! invocation. The state-handle machinery is wired by the framework
//! through the typed `StatefulBehavior::new(view)` constructor the
//! generated body calls.
//!
//! See [`crate::derive_stateless`] for the cross-crate path strategy
//! (absolute `::flui_view::…` paths, derived generics forwarding,
//! `Self: StatefulView` predicate for upfront diagnostics).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_quote};

/// Expand `#[derive(StatefulView)]` into the canonical `impl View` block.
///
/// See [`crate::derive_stateless::expand`] for the rationale behind the
/// `&DeriveInput` shape and the `syn::Result` future-proofing wrap.
#[allow(
    clippy::unnecessary_wraps,
    reason = "future-proof against attribute parsing"
)]
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut augmented_where = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    augmented_where
        .predicates
        .push(parse_quote!(Self: ::flui_view::StatefulView));

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::flui_view::View for #ident #ty_generics
        #augmented_where
        {
            fn create_element(&self) -> ::std::boxed::Box<dyn ::flui_view::ElementBase> {
                ::std::boxed::Box::new(
                    ::flui_view::StatefulElement::<Self>::new(
                        self,
                        ::flui_view::StatefulBehavior::new(self),
                    ),
                )
            }
        }
    })
}
