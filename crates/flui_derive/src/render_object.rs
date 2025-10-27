//! Derive macro implementation for RenderObjectWidget

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        // Auto-implement Widget trait
        // Note: RenderObjectWidget::Arity is used to determine the Element type
        impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause
        where
            Self: ::flui_core::RenderObjectWidget,
        {
            type Element = ::flui_core::element::RenderObjectElement<Self, <Self as ::flui_core::RenderObjectWidget>::Arity>;
            type Arity = <Self as ::flui_core::RenderObjectWidget>::Arity;

            fn key(&self) -> ::core::option::Option<::flui_core::foundation::Key> {
                ::core::option::Option::None
            }
        }
    };

    TokenStream::from(expanded)
}
