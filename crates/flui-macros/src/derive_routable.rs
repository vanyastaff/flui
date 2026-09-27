//! Codegen for `#[derive(Routable)]` (ADR-0093 §1).
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
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Path, Type, Variant};

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
}

/// Entry point for `#[proc_macro_derive(Routable, attributes(route))]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "derive(Routable) supports enums: one variant per location",
        ));
    };
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "derive(Routable) takes no generics: a route type is concrete",
        ));
    }
    if data.variants.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            "derive(Routable) needs at least one variant",
        ));
    }

    let mut errors: Option<syn::Error> = None;
    let mut push = |error: syn::Error| match &mut errors {
        Some(all) => all.combine(error),
        None => errors = Some(error),
    };
    let mut routes = Vec::with_capacity(data.variants.len());
    for variant in &data.variants {
        match route(variant) {
            Ok(route) => routes.push(route),
            Err(error) => push(error),
        }
    }
    if let Some(errors) = errors {
        return Err(errors);
    }

    let patterns: Vec<Vec<Segment>> = routes.iter().map(|r| r.segments.clone()).collect();
    if let Some((earlier, later)) = pattern::first_conflict(&patterns) {
        return Err(syn::Error::new(
            routes[later].literal.span(),
            format!(
                "`{}` has the same shape as the pattern of `{}`, `{}`: `{}` could never be \
                 parsed",
                routes[later].literal.value(),
                routes[earlier].variant.ident,
                routes[earlier].literal.value(),
                routes[later].variant.ident,
            ),
        ));
    }

    let widgets = crate::runtime_path::Runtime::Widgets.resolve(input.ident.span())?;
    let name = &input.ident;
    let to_path_arms = routes.iter().map(|route| to_path_arm(route, &widgets));
    let candidates = pattern::match_order(&patterns)
        .into_iter()
        .map(|index| candidate(&routes[index]));
    let assertions = routes.iter().flat_map(|route| &route.fields).map(|field| {
        let ty = field.ty;
        quote_spanned! {ty.span()=>
            #widgets::router::__derive::assert_segment::<#ty>();
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #widgets::Routable for #name {
            fn to_path(&self) -> #widgets::RoutePath {
                match self {
                    #(#to_path_arms)*
                }
            }

            fn from_path(
                path: &#widgets::RoutePath,
            ) -> ::core::result::Result<Self, #widgets::RouteParseError> {
                #(#assertions)*
                let __flui_decoded = #widgets::router::__derive::segments(path);
                let __flui_segments: ::std::vec::Vec<&str> = __flui_decoded
                    .iter()
                    .map(|segment| ::core::convert::AsRef::<str>::as_ref(segment))
                    .collect();
                let __flui_segments = __flui_segments.as_slice();
                let mut __flui_matcher = #widgets::router::__derive::Matcher::new(path);
                #(#candidates)*
                ::core::result::Result::Err(__flui_matcher.finish())
            }
        }
    })
}

/// Validate one variant and read its route.
fn route(variant: &Variant) -> syn::Result<Route<'_>> {
    let (fields, named) = match &variant.fields {
        Fields::Unit => (Vec::new(), false),
        Fields::Named(named) => (
            named
                .named
                .iter()
                .map(|field| {
                    let ident = field
                        .ident
                        .as_ref()
                        .expect("BUG: a named field has an ident");
                    RouteField {
                        ident,
                        ty: &field.ty,
                        name: ident.unraw().to_string(),
                    }
                })
                .collect(),
            true,
        ),
        Fields::Unnamed(_) => {
            return Err(syn::Error::new(
                variant.ident.span(),
                format!(
                    "derive(Routable) takes unit and named-field variants; name the fields of \
                     `{}`. A tuple variant that nests a child route type (`#[nest]`) is not \
                     supported yet, see ADR-0093's implementation series",
                    variant.ident
                ),
            ));
        }
    };

    let literal = route_attribute(variant)?;
    let segments = pattern::parse(&literal.value())
        .map_err(|reason| syn::Error::new(literal.span(), reason))?;

    // Every parameter names a field exactly once, and every field is a
    // parameter.
    let mut seen: Vec<&str> = Vec::new();
    for segment in &segments {
        let Segment::Param(param) = segment else {
            continue;
        };
        if seen.contains(&param.as_str()) {
            return Err(syn::Error::new(
                literal.span(),
                format!("`:{param}` appears twice in the pattern"),
            ));
        }
        seen.push(param);
        if !fields.iter().any(|field| field.name == *param) {
            return Err(syn::Error::new(
                literal.span(),
                format!("`:{param}` names no field of `{}`", variant.ident),
            ));
        }
    }
    if let Some(field) = fields.iter().find(|f| !seen.contains(&f.name.as_str())) {
        return Err(syn::Error::new(
            field.ident.span(),
            format!(
                "field `{}` is not in the pattern `{}`: add a `:{}` segment",
                field.name,
                literal.value(),
                field.name
            ),
        ));
    }

    Ok(Route {
        variant,
        literal,
        segments,
        fields,
        named,
    })
}

/// The one `#[route("…")]` of `variant`.
fn route_attribute(variant: &Variant) -> syn::Result<LitStr> {
    let mut attributes = variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("route"));
    let Some(attribute) = attributes.next() else {
        return Err(syn::Error::new(
            variant.ident.span(),
            format!(
                "variant `{}` needs a `#[route(\"/path\")]` attribute with its path pattern",
                variant.ident
            ),
        ));
    };
    if let Some(second) = attributes.next() {
        return Err(syn::Error::new(
            second.span(),
            "a variant takes one `#[route]` attribute",
        ));
    }
    let usage = || {
        syn::Error::new(
            attribute.span(),
            "`#[route]` takes one string literal, the path pattern: `#[route(\"/note/:id\")]`",
        )
    };
    let list = attribute.meta.require_list().map_err(|_| usage())?;
    let parser = |input: syn::parse::ParseStream<'_>| {
        let literal: LitStr = input.parse()?;
        if !input.is_empty() {
            return Err(
                input.error("`#[route]` takes only the path pattern; nothing may follow it")
            );
        }
        Ok(literal)
    };
    parser.parse2(list.tokens.clone()).map_err(|error| {
        if list.tokens.is_empty() {
            usage()
        } else {
            error
        }
    })
}

/// `Self::V { a, b } => RoutePath::root().join("lit").join(a)…,`
fn to_path_arm(route: &Route<'_>, widgets: &Path) -> TokenStream {
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
    quote! {
        #pattern => #widgets::RoutePath::root() #(#joins)*,
    }
}

/// One candidate of `from_path`: a slice pattern over the decoded segments,
/// then each field parsed; a field that does not parse falls through to the
/// next candidate.
fn candidate(route: &Route<'_>) -> TokenStream {
    let variant = &route.variant.ident;
    let binding = |index: usize| format_ident!("__flui_segment_{}", index);
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
    let value_of = |field: &RouteField<'_>| format_ident!("__flui_field_{}", field.name);
    let parses = route.fields.iter().map(|field| {
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
                __flui_matcher.field::<#ty>(#name, #segment)
            else {
                break '__flui_candidate;
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
            if let [#(#slots),*] = __flui_segments {
                return ::core::result::Result::Ok(#value);
            }
        }
    } else {
        quote! {
            if let [#(#slots),*] = __flui_segments {
                '__flui_candidate: {
                    #(#parses)*
                    return ::core::result::Result::Ok(#value);
                }
            }
        }
    }
}
