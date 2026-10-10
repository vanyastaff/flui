//! Codegen for `#[derive(TwoWayConverter)]`.
//!
//! Fields implement `TwoWayConverter` and `Lerp`. Their fixed vectors are
//! flattened in declaration order; interpolation delegates to each field,
//! preserving contracts such as premultiplied colour interpolation.
//! Field types must have concrete vector widths: stable Rust cannot sum
//! generic-dependent associated constants in an array length.
//!
//! Runtime paths resolve through the SDK, then the owning crate, then the
//! facade, honoring Cargo dependency aliases.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Data, DeriveInput, Fields, GenericParam, Index, spanned::Spanned, visit::Visit};

/// Entry point for `#[proc_macro_derive(TwoWayConverter)]`.
pub fn expand(input: &DeriveInput) -> TokenStream {
    let runtime = match crate::runtime_path::Runtime::Animation.resolve(input.ident.span()) {
        Ok(path) => path,
        Err(error) => return error.to_compile_error(),
    };
    let fields = match &input.data {
        Data::Struct(data) if !data.fields.is_empty() => &data.fields,
        _ => {
            return syn::Error::new(
                input.ident.span(),
                "#[derive(TwoWayConverter)] requires a struct with at least one field",
            )
            .to_compile_error();
        }
    };

    let parameters: Vec<_> = input
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(param) => Some(&param.ident),
            GenericParam::Const(param) => Some(&param.ident),
            GenericParam::Lifetime(_) => None,
        })
        .collect();
    for field in fields {
        let mut dependency = GenericDependency {
            parameters: &parameters,
            found: false,
        };
        dependency.visit_type(&field.ty);
        if dependency.found {
            return syn::Error::new(
                field.ty.span(),
                "#[derive(TwoWayConverter)] requires concrete field types; stable Rust cannot sum generic-dependent vector widths",
            ).to_compile_error();
        }
    }
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut width = quote!(0usize);
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut lerps = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let ty = &field.ty;
        let member = field.ident.as_ref().map_or_else(
            || {
                let index = Index::from(index);
                quote!(#index)
            },
            |ident| quote!(#ident),
        );
        let component_count = quote_spanned! {ty.span()=>
            <<#ty as #runtime::TwoWayConverter>::Vector as #runtime::AnimationVector>::COMPONENTS
        };
        let start = width;
        let end = quote!(#start + #component_count);
        reads.push(quote_spanned! {ty.span()=>
            {
                let field = <#ty as #runtime::TwoWayConverter>::to_vector(&self.#member);
                vector[#start..#end].copy_from_slice(::core::convert::AsRef::<[f64]>::as_ref(&field));
            }
        });
        let write = quote_spanned! {ty.span()=> {
            let mut field = [0.0; #component_count];
            field.copy_from_slice(&vector[#start..#end]);
            <#ty as #runtime::TwoWayConverter>::from_vector(field)
        }};
        let lerp = quote_spanned! {ty.span()=>
            <#ty as #runtime::Lerp>::lerp_to(&self.#member, &other.#member, t)
        };
        if let Some(ident) = &field.ident {
            writes.push(quote!(#ident: #write));
            lerps.push(quote!(#ident: #lerp));
        } else {
            writes.push(write);
            lerps.push(lerp);
        }
        width = end;
    }
    let (from_body, lerp_body) = match fields {
        Fields::Named(_) => (quote!(Self { #(#writes),* }), quote!(Self { #(#lerps),* })),
        Fields::Unnamed(_) => (quote!(Self(#(#writes),*)), quote!(Self(#(#lerps),*))),
        Fields::Unit => unreachable!("nonempty fields checked above"),
    };

    quote! {
        impl #impl_generics #runtime::TwoWayConverter for #name #ty_generics #where_clause {
            type Vector = [f64; #width];

            #[inline]
            fn to_vector(&self) -> Self::Vector {
                let mut vector = [0.0; #width];
                #(#reads)*
                vector
            }

            #[inline]
            fn from_vector(vector: Self::Vector) -> Self {
                #from_body
            }
        }

        impl #impl_generics #runtime::Lerp for #name #ty_generics #where_clause {
            #[inline]
            fn lerp_to(&self, other: &Self, t: f64) -> Self {
                if t == 0.0 { return self.clone(); }
                if t == 1.0 { return other.clone(); }
                #lerp_body
            }
        }
    }
}

/// Walk field syntax rather than comparing type spellings or guessing widths.
struct GenericDependency<'a> {
    parameters: &'a [&'a syn::Ident],
    found: bool,
}

impl<'ast> Visit<'ast> for GenericDependency<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.leading_colon.is_none()
            && path.segments.first().is_some_and(|segment| {
                self.parameters
                    .iter()
                    .any(|parameter| **parameter == segment.ident)
            })
        {
            self.found = true;
        }
        syn::visit::visit_path(self, path);
    }
}
