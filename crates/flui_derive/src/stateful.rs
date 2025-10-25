//! Derive macro implementation for StatefulWidget

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        // Auto-implement Widget trait
        impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause {
            fn key(&self) -> ::core::option::Option<&str> {
                ::core::option::Option::None
            }
        }

        // Auto-implement DynWidget trait
        impl #impl_generics ::flui_core::DynWidget for #name #ty_generics #where_clause {
            fn as_any(&self) -> &dyn ::core::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any {
                self
            }
        }
    };

    TokenStream::from(expanded)
}

pub fn derive_state(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let _name = &input.ident;

    // State derive is currently a marker
    // Users must implement State trait manually
    let expanded = quote! {
        // Empty - user implements State manually
    };

    TokenStream::from(expanded)
}
