//! Utility functions for derive macros

use syn::DeriveInput;

/// Get the name of the type being derived
#[allow(dead_code)]
pub fn get_type_name(input: &DeriveInput) -> &syn::Ident {
    &input.ident
}

/// Get generics split for impl blocks
#[allow(dead_code)]
pub fn split_generics(
    input: &DeriveInput,
) -> (
    syn::ImplGenerics<'_>,
    syn::TypeGenerics<'_>,
    Option<&syn::WhereClause>,
) {
    input.generics.split_for_impl()
}
