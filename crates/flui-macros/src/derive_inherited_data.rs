//! `#[derive(InheritedData)]` — field-granular inherited dependencies
//! (issue #1090, ADR-0008 §2, ADR-0074 §5.5).
//!
//! For a non-generic struct with named fields, emits:
//!
//! - one `pub const FIELD_<NAME>: FieldMask` per field (declaration order,
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
use quote::{format_ident, quote};
use syn::ext::IdentExt as _;
use syn::{Data, DeriveInput, Fields, Ident};

/// The mask carries this many fields.
const MAX_FIELDS: usize = 64;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;

    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
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
        return Err(syn::Error::new_spanned(
            ident,
            format!(
                "InheritedData carries {MAX_FIELDS} fields; this struct has {} — split the provider data",
                fields.len()
            ),
        ));
    }

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
            pub const #const_ident: #runtime::FieldMask = #runtime::FieldMask::bit(#index);
        }
    });
    let diffs = entries.iter().map(|(name, const_ident, _)| {
        quote! {
            if self.#name != other.#name {
                changed = changed.union(Self::#const_ident);
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
            fn field_mask_diff(&self, other: &Self) -> #runtime::FieldMask {
                let mut changed = #runtime::FieldMask::NONE;
                #(#diffs)*
                changed
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    // The positive path (constants, `field_mask_diff`, `r#type` → `FIELD_TYPE`)
    // is `crates/flui-view/tests/inherited_data_derive.rs`: expansion resolves
    // the runtime path from the consuming manifest, which this crate lacks.
    fn error_of(input: DeriveInput) -> String {
        match expand(&input) {
            Ok(_) => panic!("BUG: expected the derive to refuse this input"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn more_than_64_fields_is_refused() {
        let fields = (0..65).map(|i| {
            let name = format_ident!("f{i}");
            quote! { #name: u8 }
        });
        let input: DeriveInput = parse_quote! {
            struct Big { #(#fields),* }
        };
        assert!(error_of(input).contains("carries 64 fields; this struct has 65"));
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
}
