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
//! the type forwards `View::inspect` to its `Inspect`, `key` on a field makes
//! that field the view's `View::key`, and `site` on a field returns that field
//! from `View::creation_site`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Field, GenericParam, Ident, Member, parse_quote};

use super::attribute::{is_named, named, parse_entries, refuse_unread};
use crate::generics::mentions_param;
use crate::hygiene::local;
use crate::refusals::Refusals;
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

/// A field `#[view(...)]` marks, as `self.<member>` reads it.
struct Marked<'a> {
    member: Member,
    field: &'a Field,
}

/// What `#[view(...)]` says about a type and its fields.
struct ViewAttrs<'a> {
    /// `#[view(inspect)]` on the type.
    inspect: bool,
    /// The field marked `#[view(key)]`.
    key: Option<Marked<'a>>,
    /// The field marked `#[view(site)]`.
    site: Option<Marked<'a>>,
}

impl<'a> ViewAttrs<'a> {
    /// Read `#[view(...)]` from the type and its fields. Every refusal is
    /// recorded before any is reported, so one build shows them all.
    fn parse(input: &'a DeriveInput) -> syn::Result<Self> {
        Refusals::collect(|refusals| {
            let mut attrs = Self {
                inspect: false,
                key: None,
                site: None,
            };
            refuse_unread(input, &["view"], refusals);
            for attr in view_attributes(&input.attrs) {
                parse_entries(attr, "`inspect`", refusals, |meta, _| {
                    if !is_named(&meta.path, "inspect") {
                        return Err(meta.error("expected `inspect`; `key` and `site` mark a field"));
                    }
                    if std::mem::replace(&mut attrs.inspect, true) {
                        return Err(meta.error("`inspect` is given twice"));
                    }
                    Ok(())
                });
            }
            match &input.data {
                Data::Struct(data) => {
                    for (field, member) in data.fields.iter().zip(data.fields.members()) {
                        for attr in view_attributes(&field.attrs) {
                            parse_entries(attr, "`key` or `site`", refusals, |meta, _| {
                                let (slot, name) = if is_named(&meta.path, "key") {
                                    (&mut attrs.key, "key")
                                } else if is_named(&meta.path, "site") {
                                    (&mut attrs.site, "site")
                                } else {
                                    return Err(meta.error(
                                        "expected `key` or `site`; `inspect` goes on the type",
                                    ));
                                };
                                let marked = Marked {
                                    member: member.clone(),
                                    field,
                                };
                                mark(slot, name, &meta, marked)
                            });
                        }
                    }
                }
                Data::Enum(data) => {
                    for variant in &data.variants {
                        refuse_outside_a_struct(
                            view_attributes(&variant.attrs)
                                .chain(field_attributes(&variant.fields)),
                            refusals,
                        );
                    }
                }
                Data::Union(data) => {
                    refuse_outside_a_struct(
                        data.fields
                            .named
                            .iter()
                            .flat_map(|field| view_attributes(&field.attrs)),
                        refusals,
                    );
                }
            }
            Ok(attrs)
        })
    }
}

/// Record the one field that carries `#[view(<name>)]`.
fn mark<'a>(
    slot: &mut Option<Marked<'a>>,
    name: &str,
    meta: &ParseNestedMeta<'_>,
    marked: Marked<'a>,
) -> syn::Result<()> {
    match slot {
        Some(earlier) if earlier.member == marked.member => {
            return Err(meta.error(format!("`{name}` is given twice for the field")));
        }
        Some(_) => {
            return Err(meta.error(format!("only one field can be marked `#[view({name})]`")));
        }
        None => {}
    }
    *slot = Some(marked);
    Ok(())
}

fn view_attributes(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    named(attrs, "view")
}

fn field_attributes(fields: &syn::Fields) -> impl Iterator<Item = &Attribute> {
    fields
        .iter()
        .flat_map(|field| view_attributes(&field.attrs))
}

/// A view reads its key and its site from `self.<field>`, which only a struct has:
/// `#[view(...)]` on an enum's variant, on a variant's field or on a union's field is refused.
fn refuse_outside_a_struct<'a>(
    attrs: impl Iterator<Item = &'a Attribute>,
    refusals: &mut Refusals,
) {
    for attr in attrs {
        refusals.push(syn::Error::new_spanned(
            attr,
            "`#[view(...)]` marks a field of a struct; an enum's variants and a union's fields \
             take none",
        ));
    }
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
    if attrs.inspect || attrs.key.is_some() || attrs.site.is_some() {
        let foundation = Runtime::Foundation.resolve(span)?;
        if attrs.inspect {
            let out = local("out");
            methods.extend(quote! {
                fn inspect(&self, #out: &mut #foundation::inspect::Fields<'_>) {
                    <Self as #foundation::inspect::Inspect>::inspect(self, #out);
                }
            });
        }
        if let Some(Marked { member, field }) = &attrs.key {
            let ty = &field.ty;
            // A key type the view's parameters decide is a key only for some of them, so the
            // view is a `View` for those. A concrete type needs no bound: the call below
            // already fails for one that is not a key.
            let parameters: Vec<&Ident> = input
                .generics
                .params
                .iter()
                .filter_map(|param| match param {
                    GenericParam::Type(param) => Some(&param.ident),
                    GenericParam::Const(param) => Some(&param.ident),
                    GenericParam::Lifetime(_) => None,
                })
                .collect();
            if mentions_param(ty, &parameters) {
                where_clause
                    .predicates
                    .push(parse_quote!(#ty: #foundation::key::AsViewKey));
            }
            // Spanned at the field's type, where a type that is not a key is reported.
            let key = quote_spanned! {ty.span()=>
                <#ty as #foundation::key::AsViewKey>::as_view_key(&self.#member)
            };
            methods.extend(quote! {
                fn key(&self) -> ::core::option::Option<&dyn #foundation::ViewKey> {
                    #key
                }
            });
        }
        if let Some(Marked { member, .. }) = &attrs.site {
            methods.extend(quote! {
                fn creation_site(&self) -> #foundation::CreationSite {
                    self.#member
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
    use crate::refusals::assert_refused;

    #[test]
    fn a_misplaced_view_attribute_is_refused_with_its_place() {
        let cases: [(&str, DeriveInput, &str); 15] = [
            (
                "site on the type",
                parse_quote! { #[view(site)] struct V { site: CreationSite } },
                "expected `inspect`; `key` and `site` mark a field",
            ),
            (
                "key on the type",
                parse_quote! { #[view(key)] struct V { key: UniqueKey } },
                "expected `inspect`; `key` and `site` mark a field",
            ),
            (
                "inspect on a field",
                parse_quote! { struct V { #[view(inspect)] site: CreationSite } },
                "expected `key` or `site`; `inspect` goes on the type",
            ),
            (
                "two sites",
                parse_quote! { struct V(#[view(site)] CreationSite, #[view(site)] CreationSite); },
                "only one field can be marked `#[view(site)]`",
            ),
            (
                "two keys",
                parse_quote! { struct V { #[view(key)] a: UniqueKey, #[view(key)] b: UniqueKey } },
                "only one field can be marked `#[view(key)]`",
            ),
            (
                "one site twice",
                parse_quote! { struct V { #[view(site, site)] site: CreationSite } },
                "`site` is given twice for the field",
            ),
            (
                "one key twice",
                parse_quote! { struct V { #[view(key)] #[view(key)] key: UniqueKey } },
                "`key` is given twice for the field",
            ),
            (
                "inspect twice",
                parse_quote! { #[view(inspect)] #[view(inspect)] struct V; },
                "`inspect` is given twice",
            ),
            (
                "a variant's field",
                parse_quote! { enum V { A { #[view(site)] site: CreationSite } } },
                "marks a field of a struct",
            ),
            (
                "a variant's key",
                parse_quote! { enum V { A { #[view(key)] key: UniqueKey } } },
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
            (
                "spelled with a raw identifier on a variant",
                parse_quote! { enum V { #[r#view(inspect)] A } },
                "marks a field of a struct",
            ),
            (
                "an empty list on the type",
                parse_quote! { #[view()] struct V; },
                "the list is empty; expected `inspect`",
            ),
            (
                "an empty list on a field",
                parse_quote! { struct V { #[view()] site: CreationSite } },
                "the list is empty; expected `key` or `site`",
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

    #[test]
    fn an_entry_reads_the_same_spelled_raw() {
        let input: DeriveInput = parse_quote! {
            #[view(r#inspect)]
            struct V {
                #[view(r#key)]
                a: UniqueKey,
                #[r#view(r#site)]
                b: CreationSite,
            }
        };
        let attrs = ViewAttrs::parse(&input).expect("BUG: the view's attributes read");
        assert!(attrs.inspect);
        let a: Member = parse_quote!(a);
        let b: Member = parse_quote!(b);
        assert!(attrs.key.is_some_and(|marked| marked.member == a));
        assert!(attrs.site.is_some_and(|marked| marked.member == b));
    }

    #[test]
    fn every_refusal_of_a_views_attributes_is_reported_in_one_build() {
        let input: DeriveInput = parse_quote! {
            #[view(inspect, inspect)]
            struct V<#[view(site)] T> {
                call: fn(#[view(key)] T),
                #[view(key, key)]
                a: UniqueKey,
                #[view(site)]
                b: CreationSite,
                #[view(site)]
                c: CreationSite,
                #[view()]
                d: u8,
                #[view(nonsense = 1, key)]
                e: UniqueKey,
            }
        };
        assert_refused(
            expand(&input, ViewKind::Stateless),
            &[
                "`#[view]` does nothing here",
                "`#[view]` does nothing here",
                "`inspect` is given twice",
                "`key` is given twice for the field",
                "only one field can be marked `#[view(site)]`",
                "the list is empty; expected `key` or `site`",
                "expected `key` or `site`; `inspect` goes on the type",
                "only one field can be marked `#[view(key)]`",
            ],
        );
    }
}
