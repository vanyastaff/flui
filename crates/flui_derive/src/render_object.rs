//! Derive macro implementation for RenderObjectWidget

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Parse #[render_object(arity = "...")] attribute if present
    // For now, we just auto-implement Widget and DynWidget

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
