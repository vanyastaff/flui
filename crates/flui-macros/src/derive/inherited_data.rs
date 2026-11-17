//! `#[derive(InheritedData)]` — field-granular inherited dependencies
//! (issue #1090, ADR-0074 §5.5).
//!
//! For a non-generic struct with named fields, emits:
//!
//! - one `pub const FIELD_<NAME>: FieldMask<Self>` per field (declaration order,
//!   bit 0 first; a raw identifier such as `r#type` becomes `FIELD_TYPE`),
//!   the handle a reader passes to `BuildContextExt::depend_on_field`;
//! - `impl InheritedData for T` whose `field_mask_diff` compares every field
//!   with `!=` and unions the mask of each one that differs.
//!
//! Every field must be `PartialEq` (the generated `!=` says so at the use
//! site). Refused with a compile error: more than 64 fields (the mask carries
//! 64 bits — split the provider data), tuple/unit structs, enums and unions,
//! and generic structs (a provider's data type is concrete; a generic one
//! would need per-parameter `PartialEq` bounds the derive cannot infer).

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt as _;
use syn::{Data, DeriveInput, Fields, Ident};

use crate::hygiene::local;
use crate::refusals::Refusals;

/// The mask carries this many fields.
const MAX_FIELDS: usize = 64;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    // Every refusal is recorded before any is reported, so one build shows
    // them all.
    let fields = Refusals::collect(|refusals| {
        if !input.generics.params.is_empty() {
            refusals.push(syn::Error::new_spanned(
                &input.generics,
                "InheritedData cannot be derived for a generic type: a provider's data type is \
                 concrete (the derive would have to invent `PartialEq` bounds per parameter)",
            ));
        }

        let fields = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(named) => &named.named,
                Fields::Unnamed(_) | Fields::Unit => {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "InheritedData can only be derived for a struct with named fields",
                    ));
                }
            },
            Data::Enum(_) | Data::Union(_) => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "InheritedData can only be derived for a struct",
                ));
            }
        };
        if fields.len() > MAX_FIELDS {
            refusals.push(syn::Error::new_spanned(
                ident,
                format!(
                    "InheritedData carries {MAX_FIELDS} fields; this struct has {} — split the provider data",
                    fields.len()
                ),
            ));
        }
        Ok(fields)
    })?;

    // Shape errors above are reported before the runtime path is needed, so
    // they do not depend on the consumer's manifest.
    let runtime = crate::runtime_path::Runtime::View.resolve(ident.span())?;

    // One pass: (field name, its FIELD_* constant, its bit).
    let entries: Vec<(&Ident, Ident, u32)> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = field
                .ident
                .as_ref()
                .expect("BUG: Fields::Named yields only named fields");
            // `unraw` so `r#type` names `FIELD_TYPE`, not the invalid `FIELD_R#TYPE`.
            let const_ident = format_ident!("FIELD_{}", name.unraw().to_string().to_uppercase());
            (name, const_ident, index as u32)
        })
        .collect();

    let consts = entries.iter().map(|(name, const_ident, index)| {
        let doc = format!("Field mask of `{}` (bit {index}).", name.unraw());
        quote! {
            #[doc = #doc]
            pub const #const_ident: #runtime::FieldMask<Self> = #runtime::FieldMask::bit(#index);
        }
    });
    // The parameter and the local of the generated `field_mask_diff`.
    let other = local("other");
    let changed = local("changed");
    // Spanned at the field, so a field that is not `PartialEq` is reported
    // there and not at the derive.
    let diffs = entries.iter().map(|(name, const_ident, _)| {
        quote_spanned! {name.span()=>
            if self.#name != #other.#name {
                #changed = #changed.union(Self::#const_ident);
            }
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #ident {
            #(#consts)*
        }

        #[automatically_derived]
        impl #runtime::InheritedData for #ident {
            fn field_mask_diff(&self, #other: &Self) -> #runtime::FieldMask<Self> {
                let mut #changed = #runtime::FieldMask::NONE;
                #(#diffs)*
                #changed
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    use crate::refusals::assert_refused;

    // The positive path (constants, `field_mask_diff`, `r#type` → `FIELD_TYPE`)
    // is exercised from a consuming crate's tests: expansion resolves the
    // runtime path from the consuming manifest, which this crate lacks.
    fn error_of(input: DeriveInput) -> String {
        match expand(&input) {
            Ok(_) => panic!("BUG: expected the derive to refuse this input"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn tuple_structs_enums_and_generics_are_refused_with_named_reasons() {
        let tuple: DeriveInput = parse_quote! { struct T(u32, u32); };
        assert!(error_of(tuple).contains("struct with named fields"));
        let unit: DeriveInput = parse_quote! { struct U; };
        assert!(error_of(unit).contains("struct with named fields"));
        let en: DeriveInput = parse_quote! { enum E { A, B } };
        assert!(error_of(en).contains("only be derived for a struct"));
        let generic: DeriveInput = parse_quote! { struct G<T> { value: T } };
        assert!(error_of(generic).contains("cannot be derived for a generic type"));
    }

    #[test]
    fn every_refusal_of_a_providers_data_is_reported_in_one_build() {
        let input: DeriveInput = parse_quote! { struct S<T>(T); };
        assert_refused(
            expand(&input),
            &[
                "cannot be derived for a generic type",
                "can only be derived for a struct with named fields",
            ],
        );
    }
}
