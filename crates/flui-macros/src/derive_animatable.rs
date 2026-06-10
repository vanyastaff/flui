//! Codegen for `#[derive(Animatable)]`.
//!
//! Generates a `TwoWayConverter` implementation that decomposes a struct of
//! `f32` fields into a `[f32; N]` vector and rebuilds it, so the type can be
//! spring-animated by `flui_animation::AnimatedValue`. The authoring shape is:
//!
//! ```rust,ignore
//! #[derive(Clone, Animatable)]
//! struct Translation {
//!     x: f32,
//!     y: f32,
//!     z: f32,
//! }
//! ```
//!
//! No hand-written `impl TwoWayConverter`. Every field must be `f32` (the scalar
//! component type the spring core operates on); a non-`f32` field is a compile
//! error pointing at the offending field.
//!
//! ## Generated-code path strategy
//!
//! The emitted `impl` references the trait via the absolute
//! `::flui_animation::TwoWayConverter` path, so every consumer of the derive
//! must have `flui-animation` as a direct dependency. This matches the
//! `::flui_foundation::…` strategy used by the other FLUI derives.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Index, Type, spanned::Spanned};

/// Entry point for `#[proc_macro_derive(Animatable)]`.
pub fn expand(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new(
                input.ident.span(),
                "#[derive(Animatable)] supports only structs of `f32` fields",
            )
            .to_compile_error();
        }
    };

    // Reject any non-`f32` field with a span-located error.
    if let Some(err) = first_non_f32_field(fields) {
        return err.to_compile_error();
    }

    let count = fields.len();
    // `to_vector` reads each field; `from_vector` rebuilds the value.
    let (reads, writes): (Vec<TokenStream>, Vec<TokenStream>) = match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let ident = f.ident.as_ref().expect("named field has an ident");
                (quote!(self.#ident), quote!(#ident: v[#i]))
            })
            .unzip(),
        Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let index = Index::from(i);
                (quote!(self.#index), quote!(v[#i]))
            })
            .unzip(),
        Fields::Unit => (Vec::new(), Vec::new()),
    };

    let from_body = match fields {
        Fields::Named(_) => quote!(Self { #(#writes),* }),
        Fields::Unnamed(_) => quote!(Self(#(#writes),*)),
        Fields::Unit => quote!(Self),
    };

    quote! {
        impl #impl_generics ::flui_animation::TwoWayConverter for #name #ty_generics #where_clause {
            type Vector = [f32; #count];

            #[inline]
            fn to_vector(&self) -> Self::Vector {
                [#(#reads),*]
            }

            #[inline]
            fn from_vector(v: Self::Vector) -> Self {
                #from_body
            }
        }
    }
}

/// Returns an error located at the first field whose type is not `f32`.
fn first_non_f32_field(fields: &Fields) -> Option<syn::Error> {
    fields.iter().find_map(|field| {
        if is_f32(&field.ty) {
            None
        } else {
            Some(syn::Error::new(
                field.ty.span(),
                "#[derive(Animatable)] requires every field to be `f32` \
                 (the scalar component type the spring core animates)",
            ))
        }
    })
}

/// Whether `ty` is exactly `f32` (by the final path segment).
fn is_f32(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none()
        && p.path.segments.last().is_some_and(|seg| seg.ident == "f32"))
}
