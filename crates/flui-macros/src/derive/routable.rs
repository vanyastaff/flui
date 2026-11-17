//! Codegen for `#[derive(Routable)]` (ADR-0330 §1).
//!
//! ```rust,ignore
//! #[derive(Routable, Clone, PartialEq)]
//! enum AppRoute {
//!     #[route("/")]
//!     Home,
//!     #[route("/note/:id")]
//!     Note { id: u32 },
//! }
//! ```
//!
//! Each variant carries one `#[route("…")]` pattern. A `:name` segment fills
//! the field `name`, which prints through `Display` and parses through
//! `FromStr`; every other segment is literal text. The derive checks the
//! patterns at compile time (see [`pattern`]) and generates `to_path` and
//! `from_path`; `from_path` tries the patterns from the most specific, so a
//! literal segment wins over a parameter whatever the declaration order.
//!
//! The generated code names only `flui_widgets` items: the trait, `RoutePath`,
//! `RouteParseError` and the hidden `router::__derive` helpers.

mod pattern;

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, LitStr, Path, Type, Variant};

use super::attribute::{is_named, named, parse_entries, refuse_unread};
use crate::generics;
use crate::hygiene::{label, local};
use crate::refusals::Refusals;

use pattern::Segment;

/// One variant and its route.
struct Route<'a> {
    variant: &'a Variant,
    literal: LitStr,
    segments: Vec<Segment>,
    /// Named fields, in declaration order; empty for a unit variant.
    fields: Vec<RouteField<'a>>,
    named: bool,
}

struct RouteField<'a> {
    ident: &'a Ident,
    ty: &'a Type,
    /// The field's name without `r#`, as a `:name` parameter spells it.
    name: String,
    /// How a `#[query]` field reads the query; `None` for a path parameter.
    query: Option<QueryField<'a>>,
}

/// A field read from the location's query.
struct QueryField<'a> {
    /// The query key: the field's name, or its `rename`.
    key: String,
    kind: QueryKind,
    /// The type one value parses as: `T` of `Option<T>` and `Vec<T>`, else
    /// the field's type.
    value: &'a Type,
}

#[derive(Clone, Copy)]
enum QueryKind {
    /// `Option<T>`: `None` without the key.
    Optional,
    /// `Vec<T>`: every value of the key, in order.
    Repeated,
    /// Any other `T: Default`: its default without the key.
    Defaulted,
}

/// Expand `#[derive(Routable)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let routes = Refusals::collect(|refusals| routes(input, refusals))?;
    let patterns: Vec<&[Segment]> = routes.iter().map(|r| r.segments.as_slice()).collect();

    let widgets = crate::runtime_path::Runtime::Widgets.resolve(input.ident.span())?;
    let name = &input.ident;
    let to_path_arms = routes.iter().map(|route| to_path_arm(route, &widgets));
    let candidates = pattern::match_order(&patterns)
        .into_iter()
        .map(|index| candidate(&routes[index]));
    let assertions =
        routes
            .iter()
            .flat_map(|route| &route.fields)
            .map(|field| match &field.query {
                None => {
                    let ty = field.ty;
                    quote_spanned! {ty.span()=>
                        #widgets::router::__derive::assert_segment::<#ty>();
                    }
                }
                Some(QueryField {
                    kind: QueryKind::Defaulted,
                    value,
                    ..
                }) => quote_spanned! {value.span()=>
                    #widgets::router::__derive::assert_query_defaulted::<#value>();
                },
                Some(QueryField { value, .. }) => quote_spanned! {value.span()=>
                    #widgets::router::__derive::assert_segment::<#value>();
                },
            });

    // The parameter and the locals of the generated `from_path`.
    let path = local("path");
    let decoded = local("decoded");
    let segments = local("segments");
    let matcher = local("matcher");
    let segment = local("segment");
    Ok(quote! {
        #[automatically_derived]
        impl #widgets::Routable for #name {
            fn to_path(&self) -> #widgets::RoutePath {
                match self {
                    #(#to_path_arms)*
                }
            }

            fn from_path(
                #path: &#widgets::RoutePath,
            ) -> ::core::result::Result<Self, #widgets::RouteParseError> {
                #(#assertions)*
                let #decoded = #widgets::router::__derive::segments(#path);
                let #segments: ::std::vec::Vec<&str> = #decoded
                    .iter()
                    .map(|#segment| ::core::convert::AsRef::<str>::as_ref(#segment))
                    .collect();
                let #segments = #segments.as_slice();
                let mut #matcher = #widgets::router::__derive::Matcher::new(#path);
                #(#candidates)*
                ::core::result::Result::Err(#matcher.finish())
            }
        }
    })
}

/// Read the type's variants and their routes.
///
/// Every refusal the derive makes of its input is made here, before the
/// runtime path is looked up, and all of them are recorded: one build shows
/// the author each thing to change.
fn routes<'a>(input: &'a DeriveInput, refusals: &mut Refusals) -> syn::Result<Vec<Route<'a>>> {
    if let Some(generic) = generics::declared(&input.generics) {
        refusals.push(syn::Error::new_spanned(
            generic,
            "derive(Routable) takes no generics: a route type is concrete",
        ));
    }
    refuse_misplaced(
        &input.attrs,
        "route",
        "`#[route]` gives a variant its path pattern; the type takes none",
        refusals,
    );
    refuse_misplaced(
        &input.attrs,
        "query",
        "`#[query]` marks a field of a variant; the type takes none",
        refusals,
    );
    refuse_unread(input, &["route", "query"], refusals);
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "derive(Routable) supports enums: one variant per location",
        ));
    };
    if data.variants.is_empty() {
        refusals.push(syn::Error::new(
            input.ident.span(),
            "derive(Routable) needs at least one variant",
        ));
    }

    let mut routes: Vec<Route<'a>> = Vec::with_capacity(data.variants.len());
    for variant in &data.variants {
        refuse_misplaced(
            &variant.attrs,
            "query",
            "`#[query]` marks a field; the variant takes `#[route(\"/path\")]`",
            refusals,
        );
        for field in &variant.fields {
            refuse_misplaced(
                &field.attrs,
                "route",
                "`#[route]` gives a variant its path pattern; it does not go on a field",
                refusals,
            );
        }
        let Some(route) = route(variant, refusals) else {
            continue;
        };
        // Two patterns of one shape match the same locations, and the later
        // variant would never be parsed.
        if let Some(earlier) = routes
            .iter()
            .find(|earlier| pattern::same_shape(&earlier.segments, &route.segments))
        {
            refusals.push(syn::Error::new(
                route.literal.span(),
                format!(
                    "`{}` has the same shape as the pattern of `{}`, `{}`: `{}` could never be \
                     parsed",
                    route.literal.value(),
                    earlier.variant.ident,
                    earlier.literal.value(),
                    route.variant.ident,
                ),
            ));
        }
        routes.push(route);
    }
    Ok(routes)
}

/// Read one variant's route, recording each refusal of it.
///
/// `None` for a variant whose pattern cannot be read. A route returned with a
/// refusal recorded is never expanded; it is there for the shape of its
/// pattern, which the variants after it are compared with.
fn route<'a>(variant: &'a Variant, refusals: &mut Refusals) -> Option<Route<'a>> {
    // The named fields; `None` for a tuple variant, whose fields no parameter
    // could name.
    let fields: Option<Vec<RouteField<'a>>> = match &variant.fields {
        Fields::Unit => Some(Vec::new()),
        Fields::Named(named) => Some(
            named
                .named
                .iter()
                .map(|field| {
                    let ident = field
                        .ident
                        .as_ref()
                        .expect("BUG: a named field has an ident");
                    let name = ident.unraw().to_string();
                    RouteField {
                        ident,
                        ty: &field.ty,
                        query: query_attribute(field, &name, refusals),
                        name,
                    }
                })
                .collect(),
        ),
        Fields::Unnamed(_) => {
            refusals.push(syn::Error::new(
                variant.ident.span(),
                format!(
                    "derive(Routable) takes unit and named-field variants; name the fields of \
                     `{}`. A tuple variant that nests a child route type (`#[nest]`) is not \
                     supported yet",
                    variant.ident
                ),
            ));
            None
        }
    };

    let literal = route_attribute(variant, refusals)?;
    let segments = refusals.value(
        pattern::parse(&literal.value()).map_err(|reason| syn::Error::new(literal.span(), reason)),
    )?;

    if let Some(fields) = &fields {
        // Every parameter names a field exactly once, every field is a
        // parameter or a query field, and no two fields read one query key.
        let mut seen: Vec<&str> = Vec::new();
        for segment in &segments {
            let Segment::Param(param) = segment else {
                continue;
            };
            if seen.contains(&param.as_str()) {
                refusals.push(syn::Error::new(
                    literal.span(),
                    format!("`:{param}` appears twice in the pattern"),
                ));
                continue;
            }
            seen.push(param);
            match fields.iter().find(|field| field.name == *param) {
                None => refusals.push(syn::Error::new(
                    literal.span(),
                    format!("`:{param}` names no field of `{}`", variant.ident),
                )),
                Some(field) if field.query.is_some() => refusals.push(syn::Error::new(
                    field.ident.span(),
                    format!(
                        "field `{param}` is both a path parameter and `#[query]`: drop `:{param}` \
                         from the pattern or the `#[query]` attribute"
                    ),
                )),
                Some(_) => {}
            }
        }
        for field in fields
            .iter()
            .filter(|field| field.query.is_none() && !seen.contains(&field.name.as_str()))
        {
            refusals.push(syn::Error::new(
                field.ident.span(),
                format!(
                    "field `{}` is not in the pattern `{}`: add a `:{}` segment, or mark it \
                     `#[query]` to read it from the query",
                    field.name,
                    literal.value(),
                    field.name
                ),
            ));
        }
        let mut keys: Vec<(&str, &Ident)> = Vec::new();
        for field in fields {
            let Some(query) = &field.query else {
                continue;
            };
            match keys.iter().find(|(key, _)| *key == query.key) {
                Some((key, earlier)) => refusals.push(syn::Error::new(
                    field.ident.span(),
                    format!(
                        "field `{}` reads the query key `{key}`, which field `{}` already reads; \
                         give one of them another key with `#[query(rename = \"...\")]`",
                        field.name,
                        earlier.unraw(),
                    ),
                )),
                None => keys.push((&query.key, field.ident)),
            }
        }
    }

    Some(Route {
        variant,
        literal,
        segments,
        named: matches!(variant.fields, Fields::Named(_)),
        fields: fields.unwrap_or_default(),
    })
}

/// `route` gives a variant its pattern and `query` marks a variant's field.
/// The derive registers both for the whole item, so anywhere else rustc
/// accepts them and the derive would not read them: each one found in
/// `attrs`, a place that takes no `name`, is refused.
fn refuse_misplaced(attrs: &[Attribute], name: &str, message: &str, refusals: &mut Refusals) {
    for attribute in named(attrs, name) {
        refusals.push(syn::Error::new_spanned(attribute, message));
    }
}

/// The pattern of the one `#[route("…")]` of `variant`; `None`, with the
/// refusal recorded, for a variant without one that reads.
fn route_attribute(variant: &Variant, refusals: &mut Refusals) -> Option<LitStr> {
    let mut attributes = named(&variant.attrs, "route");
    let Some(attribute) = attributes.next() else {
        refusals.push(syn::Error::new(
            variant.ident.span(),
            format!(
                "variant `{}` needs a `#[route(\"/path\")]` attribute with its path pattern",
                variant.ident
            ),
        ));
        return None;
    };
    for repeated in attributes {
        refusals.push(syn::Error::new_spanned(
            repeated,
            "a variant takes one `#[route]` attribute",
        ));
    }
    let usage = || {
        syn::Error::new_spanned(
            attribute,
            "`#[route]` takes one string literal, the path pattern: `#[route(\"/note/:id\")]`",
        )
    };
    let parser = |input: syn::parse::ParseStream<'_>| {
        let literal: LitStr = input.parse()?;
        if !input.is_empty() {
            return Err(
                input.error("`#[route]` takes only the path pattern; nothing may follow it")
            );
        }
        Ok(literal)
    };
    let literal = match attribute.meta.require_list() {
        Ok(list) if !list.tokens.is_empty() => parser.parse2(list.tokens.clone()),
        _ => Err(usage()),
    };
    refusals.value(literal)
}

/// The field's `#[query]` or `#[query(rename = "key")]`, read with the kind
/// its type spells; `None` for a path parameter.
///
/// A field that carries the attribute is a query field even when the
/// attribute is refused, so the refusal is not followed by one that the field
/// is missing from the pattern.
fn query_attribute<'a>(
    field: &'a syn::Field,
    name: &str,
    refusals: &mut Refusals,
) -> Option<QueryField<'a>> {
    let mut attributes = named(&field.attrs, "query");
    let attribute = attributes.next()?;
    for repeated in attributes {
        refusals.push(syn::Error::new_spanned(
            repeated,
            "a field takes one `#[query]` attribute",
        ));
    }
    let mut key: Option<String> = None;
    if !matches!(attribute.meta, syn::Meta::Path(_)) {
        let mut renamed = false;
        parse_entries(
            attribute,
            "`rename = \"key\"`, or `#[query]` without the parentheses",
            refusals,
            |meta, refusals| {
                if !is_named(&meta.path, "rename") {
                    return Err(meta.error("`#[query]` takes only `rename = \"key\"`"));
                }
                let twice = std::mem::replace(&mut renamed, true);
                let name = meta
                    .value()
                    .and_then(syn::parse::ParseBuffer::parse::<LitStr>);
                // After the value, so that the refusal underlines the whole entry.
                if twice {
                    refusals.push(meta.error("`rename` is given twice"));
                }
                let name = name?;
                if name.value().is_empty() {
                    return Err(syn::Error::new(name.span(), "a query key is not empty"));
                }
                key = Some(name.value());
                Ok(())
            },
        );
    }
    let (kind, value) = match single_argument(&field.ty, "Option") {
        Some(inner) => (QueryKind::Optional, inner),
        None => match single_argument(&field.ty, "Vec") {
            Some(inner) => (QueryKind::Repeated, inner),
            None => (QueryKind::Defaulted, &field.ty),
        },
    };
    Some(QueryField {
        key: key.unwrap_or_else(|| name.to_owned()),
        kind,
        value,
    })
}

/// `T` when `ty` is spelled `Name<T>` (with any path before `Name`).
fn single_argument<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident.unraw() != name {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    match arguments.args.iter().collect::<Vec<_>>().as_slice() {
        [syn::GenericArgument::Type(inner)] => Some(inner),
        _ => None,
    }
}

/// `Self::V { a, b } => RoutePath::root().join("lit").join(a)…,`
fn to_path_arm(route: &Route<'_>, widgets: &Path) -> TokenStream {
    // The path an arm builds, and a query value it reads.
    let path = local("path");
    let value = local("value");
    let variant = &route.variant.ident;
    let joins = route.segments.iter().map(|segment| match segment {
        Segment::Literal(text) => quote!(.join(#text)),
        Segment::Param(param) => {
            let field = route
                .fields
                .iter()
                .find(|field| field.name == *param)
                .expect("BUG: every parameter was checked to name a field");
            let ident = field.ident;
            quote!(.join(#ident))
        }
    });
    let pattern = if route.named {
        let idents = route.fields.iter().map(|field| field.ident);
        quote!(Self::#variant { #(#idents),* })
    } else {
        quote!(Self::#variant)
    };
    let queries = route.fields.iter().filter_map(|field| {
        let query = field.query.as_ref()?;
        let ident = field.ident;
        let key = &query.key;
        Some(match query.kind {
            QueryKind::Optional => quote! {
                let #path = match #ident {
                    ::core::option::Option::Some(#value) => #path.with_query(#key, #value),
                    ::core::option::Option::None => #path,
                };
            },
            QueryKind::Repeated => quote! {
                let #path = #ident
                    .iter()
                    .fold(#path, |#path, #value| #path.with_query(#key, #value));
            },
            QueryKind::Defaulted => quote! {
                let #path = #path.with_query(#key, #ident);
            },
        })
    });
    quote! {
        #pattern => {
            let #path = #widgets::RoutePath::root() #(#joins)*;
            #(#queries)*
            #path
        }
    }
}

/// One candidate of `from_path`: a slice pattern over the decoded segments,
/// then each field parsed; a field that does not parse falls through to the
/// next candidate.
fn candidate(route: &Route<'_>) -> TokenStream {
    // The locals of `from_path` a candidate reads, and the block a failed parse leaves.
    let segments = local("segments");
    let matcher = local("matcher");
    let candidate = label("candidate");
    let variant = &route.variant.ident;
    let binding = |index: usize| local(&format!("segment_{index}"));
    let slots = route
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| match segment {
            Segment::Literal(text) => quote!(#text),
            Segment::Param(_) => {
                let binding = binding(index);
                quote!(#binding)
            }
        });
    let value_of = |field: &RouteField<'_>| local(&format!("field_{}", field.name));
    let parses = route.fields.iter().map(|field| {
        if let Some(query) = &field.query {
            let value = value_of(field);
            let ty = query.value;
            let name = &field.name;
            let key = &query.key;
            let read = match query.kind {
                QueryKind::Optional => quote!(query_optional),
                QueryKind::Repeated => quote!(query_repeated),
                QueryKind::Defaulted => quote!(query_defaulted),
            };
            return quote! {
                let ::core::option::Option::Some(#value) =
                    #matcher.#read::<#ty>(#name, #key)
                else {
                    break #candidate;
                };
            };
        }
        let index = route
            .segments
            .iter()
            .position(|segment| matches!(segment, Segment::Param(p) if *p == field.name))
            .expect("BUG: every field was checked to be a parameter");
        let segment = binding(index);
        let value = value_of(field);
        let ty = field.ty;
        let name = &field.name;
        quote! {
            let ::core::option::Option::Some(#value) =
                #matcher.field::<#ty>(#name, #segment)
            else {
                break #candidate;
            };
        }
    });
    let value = if route.named {
        let inits = route.fields.iter().map(|field| {
            let ident = field.ident;
            let value = value_of(field);
            quote!(#ident: #value)
        });
        quote!(Self::#variant { #(#inits),* })
    } else {
        quote!(Self::#variant)
    };
    if route.fields.is_empty() {
        quote! {
            if let [#(#slots),*] = #segments {
                return ::core::result::Result::Ok(#value);
            }
        }
    } else {
        quote! {
            if let [#(#slots),*] = #segments {
                #candidate: {
                    #(#parses)*
                    return ::core::result::Result::Ok(#value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    use crate::refusals::assert_refused;

    #[test]
    fn a_route_or_query_attribute_out_of_place_is_refused() {
        let cases: [(&str, DeriveInput, &str); 10] = [
            (
                "route on the type",
                parse_quote! { #[route("/")] enum R { #[route("/")] Home } },
                "`#[route]` gives a variant its path pattern; the type takes none",
            ),
            (
                "query on the type",
                parse_quote! { #[query(rename = "q")] enum R { #[route("/")] Home } },
                "`#[query]` marks a field of a variant; the type takes none",
            ),
            (
                "query on a variant",
                parse_quote! { enum R { #[route("/search")] #[query] Search } },
                "`#[query]` marks a field; the variant takes",
            ),
            (
                "route on a field",
                parse_quote! {
                    enum R { #[route("/note/:id")] Note { #[route("/ignored")] id: u32 } }
                },
                "`#[route]` gives a variant its path pattern; it does not go on a field",
            ),
            (
                "spelled with a raw identifier on the type",
                parse_quote! { #[r#route("/")] enum R { #[route("/")] Home } },
                "`#[route]` gives a variant its path pattern; the type takes none",
            ),
            (
                "an empty query list",
                parse_quote! { enum R { #[route("/search")] Search { #[query()] q: String } } },
                "the list is empty; expected `rename = \"key\"`, or `#[query]` without the \
                 parentheses",
            ),
            (
                "a key renamed twice",
                parse_quote! {
                    enum R {
                        #[route("/search")]
                        Search { #[query(rename = "a", rename = "b")] q: String },
                    }
                },
                "`rename` is given twice",
            ),
            (
                "two fields that read one key",
                parse_quote! {
                    enum R {
                        #[route("/search")]
                        Search { #[query] q: String, #[query(rename = "q")] r#in: String },
                    }
                },
                "field `in` reads the query key `q`, which field `q` already reads",
            ),
            (
                "inside a field's type",
                parse_quote! { enum R { #[route("/")] Home { #[query] f: fn(#[query] u8) } } },
                "`#[query]` does nothing here",
            ),
            (
                "a where clause without a predicate",
                parse_quote! { enum R where { #[route("/")] Home } },
                "derive(Routable) takes no generics",
            ),
        ];
        for (case, input, expected) in cases {
            let Err(refusals) = expand(&input) else {
                panic!("BUG: `{case}` expands");
            };
            let refusals: Vec<String> = refusals
                .into_iter()
                .map(|refusal| refusal.to_string())
                .collect();
            assert!(
                matches!(refusals.as_slice(), [only] if only.contains(expected)),
                "{case}: {refusals:?}"
            );
        }
    }

    #[test]
    fn a_query_field_reads_its_own_name_or_the_one_it_is_renamed_to() {
        let input: DeriveInput = parse_quote! {
            enum R {
                #[r#route("/s")]
                S {
                    #[query]
                    plain: String,
                    #[query(r#rename = "k")]
                    renamed: r#Option<u8>,
                    #[r#query]
                    r#type: Vec<u8>,
                },
            }
        };
        let routes = Refusals::collect(|refusals| routes(&input, refusals))
            .expect("BUG: the route type reads");
        let read: Vec<(&str, QueryKind)> = routes[0]
            .fields
            .iter()
            .map(|field| {
                let query = field
                    .query
                    .as_ref()
                    .expect("BUG: every field carries `#[query]`");
                (query.key.as_str(), query.kind)
            })
            .collect();
        assert!(
            matches!(
                read.as_slice(),
                [
                    ("plain", QueryKind::Defaulted),
                    ("k", QueryKind::Optional),
                    ("type", QueryKind::Repeated),
                ]
            ),
            "{:?}",
            read.iter().map(|(key, _)| key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_refusal_of_a_route_type_is_reported_in_one_build() {
        let input: DeriveInput = parse_quote! {
            #[query]
            enum R<T> where T: Copy {
                #[route("/n/:id")]
                Note(#[route("/x")] u32),
                #[route("nope")]
                #[query]
                Bad,
                #[route("/s")]
                #[route("/again")]
                Search {
                    #[query(nope, rename = "", rename = "z")]
                    a: String,
                    #[query(rename = "x", rename = "y")]
                    b: String,
                    #[query]
                    #[query]
                    c: String,
                    d: u8,
                    e: u8,
                },
                #[route("/a")]
                A,
                #[route("/a")]
                B,
                #[route("/b/:x")]
                C { x: u8 },
                #[route("/b/:y")]
                D { y: u8, #[query(rename = "k")] p: u8, #[query(rename = "k")] q: u8 },
                #[route("/n/:other")]
                Shadow { other: u8 },
            }
        };
        assert_refused(
            expand(&input),
            &[
                "derive(Routable) takes no generics",
                "`#[query]` marks a field of a variant; the type takes none",
                "`#[route]` gives a variant its path pattern; it does not go on a field",
                "name the fields of `Note`",
                "`#[query]` marks a field; the variant takes",
                "a route pattern starts with `/`",
                "`#[query]` takes only `rename = \"key\"`",
                "a query key is not empty",
                "`rename` is given twice",
                "`rename` is given twice",
                "a field takes one `#[query]` attribute",
                "a variant takes one `#[route]` attribute",
                "field `d` is not in the pattern `/s`",
                "field `e` is not in the pattern `/s`",
                "`/a` has the same shape as the pattern of `A`",
                "field `q` reads the query key `k`, which field `p` already reads",
                "`/b/:y` has the same shape as the pattern of `C`",
                "`/n/:other` has the same shape as the pattern of `Note`",
            ],
        );

        // A type the derive cannot read on from still reports what came before.
        let empty: DeriveInput = parse_quote! { #[query] enum R {} };
        assert_refused(
            expand(&empty),
            &[
                "the type takes none",
                "derive(Routable) needs at least one variant",
            ],
        );
        let not_an_enum: DeriveInput = parse_quote! { #[route("/")] struct S<T>(T); };
        assert_refused(
            expand(&not_an_enum),
            &[
                "derive(Routable) takes no generics",
                "the type takes none",
                "derive(Routable) supports enums",
            ],
        );
    }
}
