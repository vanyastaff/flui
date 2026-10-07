//! Codegen for `#[derive(TwoWayConverter)]`.
//!
//! Generates a `TwoWayConverter` implementation that decomposes a struct of
//! `f64` fields into a `[f64; N]` vector and rebuilds it, and a matching
//! componentwise `Lerp`, so the type can be spring-animated by
//! `flui_animation::AnimatedValue`, tweened, and used as a keyframe value. The
//! authoring shape is:
//!
//! ```rust,ignore
//! #[derive(Clone, TwoWayConverter)]
//! struct Translation {
//!     x: f64,
//!     y: f64,
//!     z: f64,
//! }
//! ```
//!
//! Every field must be `f64` (the scalar component type the spring core
//! operates on); a non-`f64` field is a compile error pointing at the
//! offending field.
//!
//! ## Generated-code path strategy
//!
//! Runtime paths resolve through the SDK, then the owning crate, then the
//! `flui` facade, using the shared resolver and honoring Cargo dependency aliases.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Index, Type, spanned::Spanned};

/// Entry point for `#[proc_macro_derive(TwoWayConverter)]`.
pub fn expand(input: &DeriveInput) -> TokenStream {
    let runtime = match crate::runtime_path::Runtime::Animation.resolve(input.ident.span()) {
        Ok(path) => path,
        Err(error) => return error.to_compile_error(),
    };
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        Data::Enum(_) | Data::Union(_) => {
            return syn::Error::new(
                input.ident.span(),
                "#[derive(TwoWayConverter)] supports only structs of `f64` fields",
            )
            .to_compile_error();
        }
    };

    // Reject any non-`f64` field with a span-located error.
    if let Some(err) = first_non_f64_field(fields) {
        return err.to_compile_error();
    }

    let count = fields.len();
    // `to_vector` reads each field; `from_vector` rebuilds the value;
    // `lerp_to` interpolates each field.
    let (reads, writes, lerps): (Vec<TokenStream>, Vec<TokenStream>, Vec<TokenStream>) =
        match fields {
            Fields::Named(named) => named
                .named
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let ident = f.ident.as_ref().expect("named field has an ident");
                    (
                        quote!(self.#ident),
                        quote!(#ident: v[#i]),
                        quote!(#ident: self.#ident + (other.#ident - self.#ident) * t),
                    )
                })
                .fold(
                    (Vec::new(), Vec::new(), Vec::new()),
                    |(mut r, mut w, mut l), (read, write, lerp)| {
                        r.push(read);
                        w.push(write);
                        l.push(lerp);
                        (r, w, l)
                    },
                ),
            Fields::Unnamed(unnamed) => unnamed
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let index = Index::from(i);
                    (
                        quote!(self.#index),
                        quote!(v[#i]),
                        quote!(self.#index + (other.#index - self.#index) * t),
                    )
                })
                .fold(
                    (Vec::new(), Vec::new(), Vec::new()),
                    |(mut r, mut w, mut l), (read, write, lerp)| {
                        r.push(read);
                        w.push(write);
                        l.push(lerp);
                        (r, w, l)
                    },
                ),
            Fields::Unit => (Vec::new(), Vec::new(), Vec::new()),
        };

    let (from_body, lerp_body) = match fields {
        Fields::Named(_) => (quote!(Self { #(#writes),* }), quote!(Self { #(#lerps),* })),
        Fields::Unnamed(_) => (quote!(Self(#(#writes),*)), quote!(Self(#(#lerps),*))),
        Fields::Unit => (quote!(Self), quote!(Self)),
    };

    quote! {
        impl #impl_generics #runtime::TwoWayConverter for #name #ty_generics #where_clause {
            type Vector = [f64; #count];

            #[inline]
            fn to_vector(&self) -> Self::Vector {
                [#(#reads),*]
            }

            #[inline]
            fn from_vector(v: Self::Vector) -> Self {
                #from_body
            }
        }

        impl #impl_generics #runtime::Lerp for #name #ty_generics #where_clause {
            #[inline]
            fn lerp_to(&self, other: &Self, t: f64) -> Self {
                let _ = (other, t);
                #lerp_body
            }
        }
    }
}

/// Returns an error located at the first field whose type is not `f64`.
fn first_non_f64_field(fields: &Fields) -> Option<syn::Error> {
    fields.iter().find_map(|field| {
        if is_f64(&field.ty) {
            None
        } else {
            Some(syn::Error::new(
                field.ty.span(),
                "#[derive(TwoWayConverter)] requires every field to be `f64` \
                 (the scalar component type the spring core animates)",
            ))
        }
    })
}

/// Whether `ty` is exactly `f64` (by the final path segment).
fn is_f64(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none()
        && p.path.segments.last().is_some_and(|seg| seg.ident == "f64"))
}
