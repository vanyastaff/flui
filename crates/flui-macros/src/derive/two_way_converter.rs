//! Codegen for `#[derive(TwoWayConverter)]`.
//!
//! Fields implement `TwoWayConverter` and `Lerp`. Their fixed vectors are
//! flattened in declaration order; interpolation delegates to each field,
//! preserving contracts such as premultiplied colour interpolation.
//!
//! A field's vector has a concrete width: stable Rust cannot sum associated
//! constants that depend on a generic parameter in an array length, so a field
//! whose type names a type or const parameter is refused. An array length
//! cannot name a lifetime either, so a type with a lifetime parameter is
//! refused whole.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Data, DeriveInput, GenericParam, spanned::Spanned};

use crate::generics::mentions_param;
use crate::hygiene::{local, receiver};
use crate::refusals::Refusals;

/// Expand `#[derive(TwoWayConverter)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    // Every refusal is recorded before any is reported, so one build shows
    // them all.
    let fields = Refusals::collect(|refusals| {
        // A field that borrows for a lifetime of the type would put that
        // lifetime into the vector's length, which a constant expression
        // cannot name.
        for lifetime in input.generics.lifetimes() {
            refusals.push(syn::Error::new(
                lifetime.span(),
                "#[derive(TwoWayConverter)] requires a type without lifetime parameters: a vector's width cannot depend on one",
            ));
        }

        let fields = match &input.data {
            Data::Struct(data) if !data.fields.is_empty() => &data.fields,
            _ => {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "#[derive(TwoWayConverter)] requires a struct with at least one field",
                ));
            }
        };

        let parameters: Vec<_> = input
            .generics
            .params
            .iter()
            .filter_map(|param| match param {
                GenericParam::Type(param) => Some(&param.ident),
                GenericParam::Const(param) => Some(&param.ident),
                GenericParam::Lifetime(_) => None,
            })
            .collect();
        for field in fields
            .iter()
            .filter(|field| mentions_param(&field.ty, &parameters))
        {
            refusals.push(syn::Error::new_spanned(
                &field.ty,
                "#[derive(TwoWayConverter)] requires concrete field types; stable Rust cannot sum generic-dependent vector widths",
            ));
        }
        Ok(fields)
    })?;

    // Shape errors above are reported before the runtime path is needed, so
    // they do not depend on the consumer's manifest.
    let runtime = crate::runtime_path::Runtime::Animation.resolve(input.ident.span())?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    // The locals and parameters of the generated methods.
    let vector = local("vector");
    let index = local("index");
    let other = local("other");
    let t = local("t");
    let this = receiver();
    let mut width = quote!(0usize);
    let mut reads = Vec::with_capacity(fields.len());
    let mut thresholds = Vec::with_capacity(fields.len());
    let mut writes = Vec::with_capacity(fields.len());
    let mut lerps = Vec::with_capacity(fields.len());
    for (field, member) in fields.iter().zip(fields.members()) {
        let ty = &field.ty;
        let component_count = quote_spanned! {ty.span()=>
            <<#ty as #runtime::TwoWayConverter>::Vector as #runtime::AnimationVector>::COMPONENTS
        };
        let start = width;
        let end = quote!(#start + #component_count);
        // Each piece is spanned at the field's type and names the field's
        // vector only through that type, never through a local: rustc reports
        // a field that is not a converter once, at the field. A local of the
        // vector's type would carry the same failure to wherever it is read.
        // A type a `macro_rules!` passes as a `ty` fragment is reported at the
        // caller's type and again at the fragment, since rustc keeps both spans.
        reads.push(quote_spanned! {ty.span()=>
            #vector[#start..#end].copy_from_slice(::core::convert::AsRef::<[f64]>::as_ref(
                &<#ty as #runtime::TwoWayConverter>::to_vector(&#this.#member),
            ));
        });
        thresholds.push(quote_spanned! {ty.span()=>
            #vector[#start..#end].copy_from_slice(::core::convert::AsRef::<[f64]>::as_ref(
                &<#ty as #runtime::TwoWayConverter>::rest_thresholds(),
            ));
        });
        // `member: value` initialises a named field and a tuple field alike.
        writes.push(quote_spanned! {ty.span()=>
            #member: <#ty as #runtime::TwoWayConverter>::from_vector(
                ::core::array::from_fn(|#index| #vector[#start + #index]),
            )
        });
        lerps.push(quote_spanned! {ty.span()=>
            #member: <#ty as #runtime::Lerp>::lerp_to(&#this.#member, &#other.#member, #t)
        });
        width = end;
    }

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::TwoWayConverter for #name #ty_generics #where_clause {
            type Vector = [f64; #width];

            #[inline]
            fn to_vector(&self) -> Self::Vector {
                let mut #vector = [0.0; #width];
                #(#reads)*
                #vector
            }

            #[inline]
            fn from_vector(#vector: Self::Vector) -> Self {
                Self { #(#writes),* }
            }

            fn rest_thresholds() -> Self::Vector {
                let mut #vector = [0.0; #width];
                #(#thresholds)*
                #vector
            }
        }

        #[automatically_derived]
        impl #impl_generics #runtime::Lerp for #name #ty_generics #where_clause {
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
    use crate::refusals::assert_refused;

    /// A shape the derive cannot convert is refused before the runtime crate
    /// is looked up, which this crate's own manifest does not name.
    #[test]
    fn a_shape_without_a_fixed_vector_is_refused() {
        let cases: [(&str, DeriveInput, &str); 7] = [
            (
                "a field of a type parameter spelled raw",
                parse_quote! { struct V<T> { a: r#T } },
                "requires concrete field types",
            ),
            (
                "an enum",
                parse_quote! { enum V { A(f64) } },
                "requires a struct with at least one field",
            ),
            (
                "a unit struct",
                parse_quote! { struct V; },
                "requires a struct with at least one field",
            ),
            (
                "an empty struct",
                parse_quote! { struct V {} },
                "requires a struct with at least one field",
            ),
            (
                "a field of a type parameter",
                parse_quote! { struct V<T> { a: f64, b: T } },
                "requires concrete field types",
            ),
            (
                "a field sized by a const parameter",
                parse_quote! { struct V<const N: usize>([f64; N]); },
                "requires concrete field types",
            ),
            (
                "a lifetime parameter",
                parse_quote! { struct V<'a>(f64, ::core::marker::PhantomData<&'a ()>); },
                "requires a type without lifetime parameters",
            ),
        ];
        for (case, input, expected) in cases {
            let error = match expand(&input) {
                Ok(_) => panic!("BUG: `{case}` expands"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains(expected), "{case}: {error}");
        }
    }

    #[test]
    fn every_refusal_of_a_converted_type_is_reported_in_one_build() {
        let input: DeriveInput = parse_quote! {
            struct S<'a, T>(T, [T; 2], PhantomData<&'a ()>);
        };
        assert_refused(
            expand(&input),
            &[
                "requires a type without lifetime parameters",
                "requires concrete field types",
                "requires concrete field types",
            ],
        );
    }
}
