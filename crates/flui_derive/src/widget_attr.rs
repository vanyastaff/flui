//! Attribute macro for widgets
//!
//! The `#[widget]` attribute automatically adds Debug, Clone derives
//! and generates Widget/DynWidget implementations.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Attribute, DeriveInput};

pub fn widget_attribute(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);

    // Parse widget type from args (stateless, stateful, inherited, render_object)
    let widget_type: String = if args.is_empty() {
        "stateless".to_string() // default
    } else {
        args.to_string().trim().to_string()
    };

    // Check if Debug and Clone are already in derives
    let has_debug = has_derive(&input.attrs, "Debug");
    let has_clone = has_derive(&input.attrs, "Clone");

    // Add Debug and Clone to derives if not present
    if !has_debug || !has_clone {
        let mut derives = vec![];
        if !has_debug {
            derives.push(quote!(Debug));
        }
        if !has_clone {
            derives.push(quote!(Clone));
        }

        let derive_attr: Attribute = syn::parse_quote! {
            #[derive(#(#derives),*)]
        };
        input.attrs.push(derive_attr);
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Generate Widget impl based on widget type
    let widget_impl = match widget_type.as_str() {
        "stateless" => quote! {
            impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause {
                type Element = ::flui_core::element::ComponentElement<Self>;

                fn key(&self) -> ::core::option::Option<&str> {
                    ::core::option::Option::None
                }

                fn into_element(self) -> Self::Element {
                    ::flui_core::element::ComponentElement::new(self)
                }
            }
        },
        "stateful" => quote! {
            impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause {
                type Element = ::flui_core::element::StatefulElement<Self>;

                fn key(&self) -> ::core::option::Option<&str> {
                    ::core::option::Option::None
                }

                fn into_element(self) -> Self::Element {
                    ::flui_core::element::StatefulElement::new(self)
                }
            }
        },
        "inherited" => quote! {
            impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause {
                type Element = ::flui_core::element::InheritedElement<Self>;

                fn key(&self) -> ::core::option::Option<&str> {
                    ::core::option::Option::None
                }

                fn into_element(self) -> Self::Element {
                    ::flui_core::element::InheritedElement::new(self)
                }
            }
        },
        "render_object" => quote! {
            impl #impl_generics ::flui_core::Widget for #name #ty_generics #where_clause
            where
                Self: ::flui_core::RenderObjectWidget,
            {
                type Element = ::flui_core::element::RenderObjectElement<Self, <Self as ::flui_core::RenderObjectWidget>::Arity>;

                fn key(&self) -> ::core::option::Option<&str> {
                    ::core::option::Option::None
                }

                fn into_element(self) -> Self::Element {
                    ::flui_core::element::RenderObjectElement::new(self)
                }
            }
        },
        _ => panic!("Unknown widget type: {}", widget_type),
    };

    let dyn_widget_impl = quote! {
        impl #impl_generics ::flui_core::DynWidget for #name #ty_generics #where_clause {
            fn as_any(&self) -> &dyn ::core::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any {
                self
            }
        }
    };

    let expanded = quote! {
        #input

        #widget_impl
        #dyn_widget_impl
    };

    TokenStream::from(expanded)
}

fn has_derive(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive") {
            // Check meta list
            if let syn::Meta::List(meta_list) = &attr.meta {
                return meta_list.tokens.to_string().contains(name);
            }
        }
        false
    })
}
