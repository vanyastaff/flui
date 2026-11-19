//! Codegen for `#[derive(Lerp)]`.
//!
//! A struct interpolates field by field: each field's own `Lerp` runs, so a
//! field keeps its contract (a colour in premultiplied Oklab, a rectangle by
//! its bounds). The endpoints come back exactly: `t == 0` and `t == 1` clone
//! the matching input instead of evaluating each field's formula there.
//!
//! A field whose type names a type parameter adds `FieldType: Lerp` to the
//! impl's `where` clause, so a generic struct interpolates whenever its fields
//! do.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Data, DeriveInput, GenericParam, Ident, parse_quote, spanned::Spanned};

use crate::generics::mentions_param;
use crate::hygiene::{local, receiver};

/// Expand `#[derive(Lerp)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Lerp)] requires a struct: an enum's variants have no interpolation between them",
        ));
    };
    let fields = &data.fields;

    // The shape is checked before the runtime path is needed, so the refusal
    // does not depend on the consumer's manifest.
    let geometry = crate::runtime_path::Runtime::Geometry.resolve(input.ident.span())?;
    let name = &input.ident;
    let type_params: Vec<&Ident> = input
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(param) => Some(&param.ident),
            _ => None,
        })
        .collect();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    // The parameters of the generated method.
    let other = local("other");
    let t = local("t");
    let this = receiver();
    let mut lerps = Vec::with_capacity(fields.len());
    for (field, member) in fields.iter().zip(fields.members()) {
        let ty = &field.ty;
        if mentions_param(ty, &type_params) {
            where_clause
                .predicates
                .push(parse_quote!(#ty: #geometry::Lerp));
        }
        // Spanned at the field's type: a field that does not interpolate is
        // reported there. `member: value` initialises a named field and a
        // tuple field alike.
        lerps.push(quote_spanned! {ty.span()=>
            #member: <#ty as #geometry::Lerp>::lerp_to(&#this.#member, &#other.#member, #t)
        });
    }

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #geometry::Lerp for #name #ty_generics #where_clause {
            #[inline]
            fn lerp_to(&self, #other: &Self, #t: f64) -> Self {
                if #t == 0.0 { return ::core::clone::Clone::clone(self); }
                if #t == 1.0 { return ::core::clone::Clone::clone(#other); }
                Self { #(#lerps),* }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    /// An enum is refused before the runtime crate is looked up, which this
    /// crate's own manifest does not name.
    #[test]
    fn an_enum_is_refused() {
        let input: DeriveInput = parse_quote! { enum V { A(f64), B(f64) } };
        let error = match expand(&input) {
            Ok(_) => panic!("BUG: an enum expands"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("requires a struct"), "{error}");
    }
}
