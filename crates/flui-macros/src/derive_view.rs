//! Codegen for `#[derive(StatelessView)]` and `#[derive(StatefulView)]`.
//!
//! The author implements the typed authoring trait (`StatelessView`, or
//! `StatefulView` with its `ViewState`); the derive writes the object-safe
//! `impl View` that mounts the matching element:
//!
//! ```rust,ignore
//! #[derive(Clone, StatelessView)]
//! struct Greeting { name: String }
//!
//! impl StatelessView for Greeting {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(&self.name)
//!     }
//! }
//! ```
//!
//! `#[view(...)]` fills the rest of `View` from the type itself: `inspect` on
//! the type forwards `View::inspect` to its `Inspect`, and `site` on a field
//! returns that field from `View::creation_site`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Member, parse_quote};

use crate::runtime_path::Runtime;

/// The authoring trait a view type implements, and so the element it mounts.
#[derive(Clone, Copy)]
pub(crate) enum ViewKind {
    Stateless,
    Stateful,
}

impl ViewKind {
    /// The trait the author implements by hand.
    fn authoring_trait(self) -> &'static str {
        match self {
            Self::Stateless => "StatelessView",
            Self::Stateful => "StatefulView",
        }
    }

    /// The `ElementKind` constructor that mounts a view of this kind.
    fn element_constructor(self) -> &'static str {
        match self {
            Self::Stateless => "stateless",
            Self::Stateful => "stateful",
        }
    }
}

/// What `#[view(...)]` says about a type and its fields.
struct ViewAttrs {
    /// `#[view(inspect)]` on the type.
    inspect: bool,
    /// The field marked `#[view(site)]`.
    site: Option<Member>,
}

impl ViewAttrs {
    fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let mut attrs = Self {
            inspect: false,
            site: None,
        };
        for attr in view_attributes(&input.attrs) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("inspect") {
                    attrs.inspect = true;
                    Ok(())
                } else {
                    Err(meta.error("expected `inspect`; `site` marks a field"))
                }
            })?;
        }
        match &input.data {
            Data::Struct(data) => {
                for (field, member) in data.fields.iter().zip(data.fields.members()) {
                    for attr in view_attributes(&field.attrs) {
                        attr.parse_nested_meta(|meta| {
                            if !meta.path.is_ident("site") {
                                return Err(
                                    meta.error("expected `site`; `inspect` goes on the type")
                                );
                            }
                            if attrs.site.is_some() {
                                return Err(
                                    meta.error("only one field can be marked `#[view(site)]`")
                                );
                            }
                            attrs.site = Some(member.clone());
                            Ok(())
                        })?;
                    }
                }
            }
            // A view reads its site from `self.<field>`, which only a struct has.
            Data::Enum(data) => {
                for variant in &data.variants {
                    refuse_field_attributes(
                        view_attributes(&variant.attrs).chain(field_attributes(&variant.fields)),
                    )?;
                }
            }
            Data::Union(data) => {
                refuse_field_attributes(
                    data.fields
                        .named
                        .iter()
                        .flat_map(|field| view_attributes(&field.attrs)),
                )?;
            }
        }
        Ok(attrs)
    }
}

fn view_attributes(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attrs.iter().filter(|attr| attr.path().is_ident("view"))
}

fn field_attributes(fields: &syn::Fields) -> impl Iterator<Item = &Attribute> {
    fields
        .iter()
        .flat_map(|field| view_attributes(&field.attrs))
}

fn refuse_field_attributes<'a>(mut attrs: impl Iterator<Item = &'a Attribute>) -> syn::Result<()> {
    attrs.next().map_or(Ok(()), |attr| {
        Err(syn::Error::new_spanned(
            attr,
            "`#[view(...)]` marks a field of a struct; an enum's variants and a union's fields \
             take none",
        ))
    })
}

/// Expand a view derive into the `impl View` of `kind`.
pub(crate) fn expand(input: &DeriveInput, kind: ViewKind) -> syn::Result<TokenStream> {
    // Attribute errors come before the runtime path, so they do not depend on
    // the consumer's manifest.
    let attrs = ViewAttrs::parse(input)?;
    let span = input.ident.span();
    let runtime = Runtime::View.resolve(span)?;
    let ident = &input.ident;
    let authoring_trait = format_ident!("{}", kind.authoring_trait());
    let element_constructor = format_ident!("{}", kind.element_constructor());

    // The type's own generics and where clause carry over. `Self: <authoring trait>` is what
    // the element constructor needs; stated here, a missing `impl StatelessView for T` is
    // reported as that at the derive, not as an unsatisfied bound inside the element types.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    where_clause
        .predicates
        .push(parse_quote!(Self: #runtime::#authoring_trait));

    let mut methods = TokenStream::new();
    if attrs.inspect || attrs.site.is_some() {
        let foundation = Runtime::Foundation.resolve(span)?;
        if attrs.inspect {
            methods.extend(quote! {
                fn inspect(&self, out: &mut #foundation::inspect::Fields<'_>) {
                    <Self as #foundation::inspect::Inspect>::inspect(self, out);
                }
            });
        }
        if let Some(site) = &attrs.site {
            methods.extend(quote! {
                fn creation_site(&self) -> #foundation::CreationSite {
                    self.#site
                }
            });
        }
    }

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::View for #ident #ty_generics
        #where_clause
        {
            fn create_element(self: ::std::rc::Rc<Self>) -> #runtime::element::ElementKind {
                #runtime::element::ElementKind::#element_constructor(self)
            }

            #methods
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    #[test]
    fn a_misplaced_view_attribute_is_refused_with_its_place() {
        let cases: [(&str, DeriveInput, &str); 6] = [
            (
                "site on the type",
                parse_quote! { #[view(site)] struct V { site: CreationSite } },
                "expected `inspect`; `site` marks a field",
            ),
            (
                "inspect on a field",
                parse_quote! { struct V { #[view(inspect)] site: CreationSite } },
                "expected `site`; `inspect` goes on the type",
            ),
            (
                "two sites",
                parse_quote! { struct V(#[view(site)] CreationSite, #[view(site)] CreationSite); },
                "only one field can be marked `#[view(site)]`",
            ),
            (
                "a variant's field",
                parse_quote! { enum V { A { #[view(site)] site: CreationSite } } },
                "marks a field of a struct",
            ),
            (
                "a variant",
                parse_quote! { enum V { #[view(inspect)] A } },
                "marks a field of a struct",
            ),
            (
                "a union's field",
                parse_quote! { union V { #[view(site)] site: CreationSite } },
                "marks a field of a struct",
            ),
        ];
        for (case, input, expected) in cases {
            for kind in [ViewKind::Stateless, ViewKind::Stateful] {
                let error = match expand(&input, kind) {
                    Ok(_) => panic!("BUG: `{case}` expands"),
                    Err(error) => error.to_string(),
                };
                assert!(error.contains(expected), "{case}: {error}");
            }
        }
    }
}
