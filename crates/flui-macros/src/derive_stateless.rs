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
//! impl StatelessView for Greeting {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(&self.name)
//!     }
//! }
//! ```
//!
//! No `impl View for Greeting` block, no `impl_stateless_view!`
//! invocation, no `Box::new` at the call site.
//!
//! Runtime paths resolve a direct `flui-view` dependency first, otherwise the
//! `flui::view` facade module. Cargo dependency aliases are respected.
//!
//! Authors who need a typed `key()` (to participate in keyed lists)
//! write a single-method `impl View for Greeting { fn key() { … } }`
//! block alongside the derive AND drop the derive — the derive emits
//! the WHOLE `impl View` block, so an additional `impl View for X`
//! site would conflict. The `#[view(key = "expr")]` derive attribute
//! that auto-wires a field-named key is a deferred ergonomics
//! improvement.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_quote};

/// Expand `#[derive(StatelessView)]` into the canonical `impl View` block.
///
/// Reports a compile error when no runtime dependency can be resolved.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let runtime = crate::runtime_path::Runtime::View.resolve(input.ident.span())?;
    let ident = &input.ident;

    // Honor the user's generic parameters: a stateless widget MAY be
    // generic (`struct Padded<C> { child: C, inset: f32 }`). The derive
    // forwards the generics into the `impl View` block verbatim.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // The bound `Self: Clone + 'static` is what
    // `StatelessView` itself requires. `View` adds `Downcast + DynClone`
    // — both satisfied by `Clone + 'static` via the blanket impls in
    // `downcast-rs` / `dyn-clone`. We do not add extra bounds here; the
    // user's own `impl StatelessView for #ident` already enforces them
    // and a mismatch surfaces at the call site of `StatelessElement::new`
    // (which carries the bound), not from the derive.
    //
    // `Self: StatelessView` is the predicate that makes the
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
        .push(parse_quote!(Self: #runtime::StatelessView));

    let inspect = crate::derive_stateless::inspect_method(input)?;

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::View for #ident #ty_generics
        #augmented_where
        {
            fn create_element(&self) -> #runtime::element::ElementKind {
                #runtime::element::ElementKind::stateless(self)
            }

            #inspect
        }
    })
}

/// The `View::inspect` method `#[view(inspect)]` asks for, forwarding to the type's own
/// `Inspect` (ADR-0242 §3), or nothing without the attribute.
pub(crate) fn inspect_method(input: &DeriveInput) -> syn::Result<TokenStream> {
    let mut wanted = false;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("view"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("inspect") {
                wanted = true;
                Ok(())
            } else {
                Err(meta.error("expected `inspect`"))
            }
        })?;
    }
    if !wanted {
        return Ok(TokenStream::new());
    }
    let foundation = crate::runtime_path::Runtime::Foundation.resolve(input.ident.span())?;
    Ok(quote! {
        fn inspect(&self, out: &mut #foundation::inspect::Fields<'_>) {
            <Self as #foundation::inspect::Inspect>::inspect(self, out);
        }
    })
}
