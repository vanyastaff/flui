//! Names an expansion declares beside its author's tokens.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::Lifetime;

/// An identifier for a local, a pattern binding or a parameter of generated code.
///
/// It resolves at the macro's definition (`Span::mixed_site`), as a local of a
/// `macro_rules!` body does: an expression the author wrote and the expansion
/// pasted beside it (`#[inspect(default = out)]`) cannot name it, and it cannot
/// name a local of the author's. An item of the author's is not a local: a
/// constant or a unit struct called `name` still meets it, as it would meet a
/// `macro_rules!` local.
pub(crate) fn local(name: &str) -> Ident {
    Ident::new(name, Span::mixed_site())
}

/// A label for a block of generated code, resolved as [`local`] resolves a name.
pub(crate) fn label(name: &str) -> Lifetime {
    Lifetime::new(&format!("'{name}"), Span::mixed_site())
}

/// `self` for generated code that is spanned at an author's token.
///
/// `self` is hygienic: written inside `quote_spanned!` at a field or its type,
/// it takes that token's context, and when the field comes from a
/// `macro_rules!` the derive does not, it no longer names the `&self` of the
/// generated signature (E0424). Interpolated from here it keeps the call
/// site's context, which the signature has.
pub(crate) fn receiver() -> TokenStream {
    quote!(self)
}
