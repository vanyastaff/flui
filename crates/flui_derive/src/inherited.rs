//! Derive macro implementation for InheritedWidget

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
            type Element = ::flui_core::element::InheritedElement<Self>;
            type Arity = ::flui_core::render::arity::SingleArity;

            fn key(&self) -> ::core::option::Option<::flui_core::foundation::Key> {
                ::core::option::Option::None
            }
        }
    };

    TokenStream::from(expanded)
}
