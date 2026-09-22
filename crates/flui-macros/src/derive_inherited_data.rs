//! `#[derive(InheritedData)]` — field-granular inherited dependencies
//! (issue #1090, ADR-0008 §2, ADR-0074 §5.5).
//!
//! For a struct with named fields, emits:
//!
//! - one `pub const FIELD_<NAME>: FieldMask` per field (declaration order,
//!   bit 0 first), the handle a reader passes to
//!   `BuildContextExt::depend_on_field`;
//! - `impl InheritedData for T` whose `field_mask_diff` compares every field
//!   with `!=` and unions the mask of each one that differs.
//!
//! Every field must be `PartialEq`. More than 64 fields is a compile error:
//! the mask carries 64 bits, and a provider that big should split its data.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields};

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let runtime = crate::runtime_path::Runtime::View.resolve(input.ident.span())?;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "InheritedData can only be derived for a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ident,
                "InheritedData can only be derived for a struct",
            ));
        }
    };
    if fields.len() > 64 {
        return Err(syn::Error::new_spanned(
            ident,
            format!(
                "InheritedData carries 64 fields; this struct has {} — split the provider data",
                fields.len()
            ),
        ));
    }

    let consts = fields.iter().enumerate().map(|(index, field)| {
        let name = field.ident.as_ref().expect("named field");
        let const_ident = format_ident!("FIELD_{}", name.to_string().to_uppercase());
        let index = index as u32;
        let doc = format!("Field mask of `{name}` (bit {index}).");
        quote! {
            #[doc = #doc]
            pub const #const_ident: #runtime::FieldMask = #runtime::FieldMask::bit(#index);
        }
    });
    let diffs = fields.iter().map(|field| {
        let name = field.ident.as_ref().expect("named field");
        let const_ident = format_ident!("FIELD_{}", name.to_string().to_uppercase());
        quote! {
            if self.#name != other.#name {
                changed = changed.union(Self::#const_ident);
            }
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ident #ty_generics #where_clause {
            #(#consts)*
        }

        #[automatically_derived]
        impl #impl_generics #runtime::InheritedData for #ident #ty_generics #where_clause {
            fn field_mask_diff(&self, other: &Self) -> #runtime::FieldMask {
                let mut changed = #runtime::FieldMask::NONE;
                #(#diffs)*
                changed
            }
        }
    })
}
