//! Names an expansion declares beside its author's tokens.

use proc_macro2::{Ident, Span};
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
