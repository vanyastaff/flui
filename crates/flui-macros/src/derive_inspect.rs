//! Codegen for `#[derive(Inspect)]` (ADR-0242 §4).
//!
//! The derive writes `kind`, `kind_args` for a generic type, and an `inspect` body with one
//! `Fields` call per field. Field types are read through the runtime's `FieldValue`, whose
//! `on_unimplemented` names the ways out, so an unmapped field fails at the derive with that
//! hint. A fieldless enum also gets `ToValue`, so it can be another type's field.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{
    Data, DeriveInput, Expr, Fields, GenericParam, Ident, LitStr, Path, Type, ext::IdentExt,
    parse_quote, spanned::Spanned, visit::Visit,
};

/// A field's `default` attribute.
enum DefaultAttr {
    /// A bare `default`: the type's `Default`.
    Type,
    /// `default = EXPR`.
    Expr(Expr),
}

/// What `#[inspect(...)]` says about one field.
#[derive(Default)]
struct FieldAttrs {
    skip: bool,
    name: Option<LitStr>,
    default: Option<DefaultAttr>,
    level: Option<Ident>,
    unit: Option<Ident>,
    with: Option<Path>,
    debug: bool,
}

impl FieldAttrs {
    fn parse(field: &syn::Field) -> syn::Result<Self> {
        let mut attrs = Self::default();
        for attr in &field.attrs {
            if !attr.path().is_ident("inspect") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    attrs.skip = true;
                } else if meta.path.is_ident("debug") {
                    attrs.debug = true;
                } else if meta.path.is_ident("name") {
                    attrs.name = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("default") {
                    attrs.default = Some(if meta.input.peek(syn::Token![=]) {
                        DefaultAttr::Expr(meta.value()?.parse()?)
                    } else {
                        DefaultAttr::Type
                    });
                } else if meta.path.is_ident("level") {
                    let level: Ident = meta.value()?.parse()?;
                    if !matches!(level.to_string().as_str(), "summary" | "detail" | "debug") {
                        return Err(syn::Error::new(
                            level.span(),
                            "expected `summary`, `detail` or `debug`",
                        ));
                    }
                    attrs.level = Some(level);
                } else if meta.path.is_ident("unit") {
                    let unit: Ident = meta.value()?.parse()?;
                    if unit_variant(&unit).is_none() {
                        return Err(syn::Error::new(
                            unit.span(),
                            "expected `logical_px`, `device_px`, `seconds`, `degrees`, `radians` or `fraction`",
                        ));
                    }
                    attrs.unit = Some(unit);
                } else if meta.path.is_ident("with") {
                    attrs.with = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "unsupported inspect attribute; expected `skip`, `name`, `default`, `level`, `unit`, `with` or `debug`",
                    ));
                }
                Ok(())
            })?;
        }
        let span = field.span();
        if attrs.skip
            && (attrs.name.is_some()
                || attrs.default.is_some()
                || attrs.level.is_some()
                || attrs.unit.is_some()
                || attrs.with.is_some()
                || attrs.debug)
        {
            return Err(syn::Error::new(
                span,
                "a skipped field takes no other inspect attribute",
            ));
        }
        if attrs.debug && attrs.with.is_some() {
            return Err(syn::Error::new(
                span,
                "`debug` and `with` both map the field; keep one",
            ));
        }
        if attrs.debug && attrs.default.is_some() {
            return Err(syn::Error::new(
                span,
                "a `debug` field has no typed value to compare a default with; map it with `with` instead",
            ));
        }
        Ok(attrs)
    }
}

fn unit_variant(unit: &Ident) -> Option<&'static str> {
    Some(match unit.to_string().as_str() {
        "logical_px" => "LogicalPx",
        "device_px" => "DevicePx",
        "seconds" => "Seconds",
        "degrees" => "Degrees",
        "radians" => "Radians",
        "fraction" => "Fraction",
        _ => return None,
    })
}

/// Whether a type names one of the given type parameters.
struct MentionsParam<'a> {
    params: &'a [&'a Ident],
    found: bool,
}

impl<'ast> Visit<'ast> for MentionsParam<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(first) = path.segments.first()
            && self.params.iter().any(|param| first.ident == **param)
        {
            self.found = true;
        }
        syn::visit::visit_path(self, path);
    }
}

fn mentions(ty: &Type, params: &[&Ident]) -> bool {
    let mut visitor = MentionsParam {
        params,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

/// One field to describe.
struct Described<'a> {
    attrs: FieldAttrs,
    ty: &'a Type,
    /// The field's name as written: the identifier, or the position of a tuple field.
    name: String,
    /// The expression that borrows the field: `&self.width`, or a binding of a `match` arm.
    access: TokenStream,
    span: Span,
}

impl Described<'_> {
    /// The name the field reports under: its `name` attribute, or its own name.
    fn reported(&self) -> String {
        self.attrs
            .name
            .as_ref()
            .map_or_else(|| self.name.clone(), LitStr::value)
    }
}

/// Expand `#[derive(Inspect)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let foundation = crate::runtime_path::Runtime::Foundation.resolve(input.ident.span())?;
    let runtime: Path = parse_quote!(#foundation::inspect);
    let ident = &input.ident;
    let kind = ident.unraw().to_string();
    let type_params: Vec<&Ident> = input
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            GenericParam::Type(param) => Some(&param.ident),
            _ => None,
        })
        .collect();
    let mut bounds: Vec<TokenStream> = Vec::new();

    let body = match &input.data {
        Data::Struct(data) => {
            let fields = described(&data.fields, None, |name, _| {
                let member: syn::Member =
                    syn::parse_str(name).expect("BUG: a field name parses as a member");
                quote!(&self.#member)
            })?;
            let calls = field_calls(&fields, &runtime, &type_params, &mut bounds);
            quote!(#(#calls)*)
        }
        Data::Enum(data) => {
            let mut arms = Vec::with_capacity(data.variants.len());
            for variant in &data.variants {
                let variant_ident = &variant.ident;
                let variant_name = variant_ident.unraw().to_string();
                let fields = described(&variant.fields, Some("variant"), |_, index| {
                    let binding = format_ident!("__inspect_{}", index);
                    quote!(#binding)
                })?;
                let bindings = variant.fields.iter().enumerate().map(|(index, field)| {
                    let binding = format_ident!("__inspect_{}", index);
                    field
                        .ident
                        .as_ref()
                        .map_or_else(|| quote!(#binding), |name| quote!(#name: #binding))
                });
                let pattern = match &variant.fields {
                    Fields::Named(_) => quote!(Self::#variant_ident { #(#bindings),* }),
                    Fields::Unnamed(_) => quote!(Self::#variant_ident ( #(#bindings),* )),
                    Fields::Unit => quote!(Self::#variant_ident),
                };
                let calls = field_calls(&fields, &runtime, &type_params, &mut bounds);
                arms.push(quote! {
                    #[allow(unused_variables, reason = "a skipped field is bound and not read")]
                    #pattern => {
                        out.value(
                            "variant",
                            #runtime::Value::Variant { ty: #kind, name: #variant_name },
                        );
                        #(#calls)*
                    }
                });
            }
            if arms.is_empty() {
                quote!(match *self {})
            } else {
                quote!(match self { #(#arms)* })
            }
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "`#[derive(Inspect)]` does not support unions: which field is live is unknown; implement `Inspect` by hand",
            ));
        }
    };

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    for bound in bounds {
        where_clause.predicates.push(parse_quote!(#bound));
    }
    let kind_args = (!type_params.is_empty()).then(|| {
        quote! {
            fn kind_args(&self, args: &mut dyn ::core::ops::FnMut(&'static str)) {
                #( args(::core::any::type_name::<#type_params>()); )*
            }
        }
    });

    let fieldless_enum = match &input.data {
        Data::Enum(data) if !data.variants.is_empty() => data
            .variants
            .iter()
            .all(|variant| matches!(variant.fields, Fields::Unit)),
        _ => false,
    };
    let to_value = fieldless_enum.then(|| {
        let Data::Enum(data) = &input.data else {
            unreachable!("BUG: only an enum is a fieldless enum")
        };
        let arms = data.variants.iter().map(|variant| {
            let variant_ident = &variant.ident;
            let name = variant_ident.unraw().to_string();
            quote!(Self::#variant_ident => #name)
        });
        quote! {
            #[automatically_derived]
            impl #impl_generics #runtime::ToValue for #ident #ty_generics #where_clause {
                fn to_value(&self) -> #runtime::Value<'_> {
                    #runtime::Value::Variant {
                        ty: #kind,
                        name: match self { #(#arms,)* },
                    }
                }
            }
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #runtime::Inspect for #ident #ty_generics #where_clause {
            fn kind(&self) -> &'static str {
                #kind
            }

            #kind_args

            fn inspect(&self, out: &mut #runtime::Fields<'_>) {
                #body
            }
        }

        #to_value
    })
}

/// The fields to describe, with their attributes; skipped fields are left out.
///
/// Two fields may not report under one name, nor under one the derive writes itself
/// (`reserved`): a snapshot reads a field by its name, so the second would be unreachable.
fn described<'a>(
    fields: &'a Fields,
    reserved: Option<&str>,
    access: impl Fn(&str, usize) -> TokenStream,
) -> syn::Result<Vec<Described<'a>>> {
    let mut out: Vec<Described<'a>> = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let attrs = FieldAttrs::parse(field)?;
        if attrs.skip {
            continue;
        }
        let name = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), |ident| ident.unraw().to_string());
        let written = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), ToString::to_string);
        let reported = attrs
            .name
            .as_ref()
            .map_or_else(|| name.clone(), LitStr::value);
        if reserved == Some(reported.as_str()) {
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "`{reported}` names the enum's variant in the snapshot; report this field under another name with `#[inspect(name = \"...\")]`"
                ),
            ));
        }
        if out.iter().any(|other| other.reported() == reported) {
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "another field already reports as `{reported}`; give one of them another name with `#[inspect(name = \"...\")]`"
                ),
            ));
        }
        out.push(Described {
            access: access(&written, index),
            attrs,
            ty: &field.ty,
            name,
            span: field.ty.span(),
        });
    }
    Ok(out)
}

fn field_calls(
    fields: &[Described<'_>],
    runtime: &Path,
    type_params: &[&Ident],
    bounds: &mut Vec<TokenStream>,
) -> Vec<TokenStream> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let ty = field.ty;
            let access = &field.access;
            let name = field.reported();
            let generic = mentions(ty, type_params);
            let start = if let Some(with) = &field.attrs.with {
                quote_spanned!(field.span=> out.value(#name, #with(#access)))
            } else if field.attrs.debug {
                if generic {
                    bounds.push(quote!(#ty: ::core::fmt::Debug));
                }
                quote_spanned!(field.span=> out.value(#name, #runtime::Value::Debug(#access)))
            } else {
                if generic {
                    bounds.push(quote!(#ty: #runtime::FieldValue));
                }
                quote_spanned!(field.span=> out.field(#name, #access))
            };
            let level = field.attrs.level.as_ref().map(|level| {
                let variant = format_ident!(
                    "{}",
                    match level.to_string().as_str() {
                        "summary" => "Summary",
                        "detail" => "Detail",
                        _ => "Debug",
                    }
                );
                quote!(.level(#runtime::Level::#variant))
            });
            let unit = field.attrs.unit.as_ref().map(|unit| {
                let variant = format_ident!(
                    "{}",
                    unit_variant(unit).expect("BUG: the unit was checked when parsed")
                );
                quote!(.unit(#runtime::Unit::#variant))
            });
            let (default_let, default_call) = match &field.attrs.default {
                None => (None, None),
                Some(default) => {
                    let binding = format_ident!("__inspect_default_{}", index);
                    let value = match default {
                        DefaultAttr::Type => quote!(::core::default::Default::default()),
                        DefaultAttr::Expr(expr) => quote!(#expr),
                    };
                    let call = field.attrs.with.as_ref().map_or_else(
                        || quote!(.default(&#binding)),
                        |with| quote!(.default_value(#with(&#binding))),
                    );
                    (Some(quote!(let #binding: #ty = #value;)), Some(call))
                }
            };
            quote! {
                #default_let
                #start #level #unit #default_call;
            }
        })
        .collect()
}
