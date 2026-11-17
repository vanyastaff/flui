//! Codegen for `#[derive(Inspect)]` (ADR-0242 §4).
//!
//! The derive writes `kind`, `kind_args` for a generic type, and an `inspect` body with one
//! `Fields` call per field. Field types are read through the runtime's `FieldValue`, whose
//! `on_unimplemented` names the ways out, so an unmapped field fails at the derive with that
//! hint. A fieldless enum also gets `ToValue`, so it can be another type's field.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, GenericParam, Ident, LitStr, Member, Path, Type,
    ext::IdentExt, parse_quote, spanned::Spanned,
};

use crate::generics::mentions_param;

/// A field's `default` attribute.
enum DefaultAttr {
    /// A bare `default`: the type's `Default`.
    Type,
    /// `default = EXPR`.
    Expr(Expr),
}

/// A field's `level` attribute: the runtime's `Level` variant it names.
#[derive(Clone, Copy)]
enum Level {
    Summary,
    Detail,
    Debug,
}

impl Level {
    fn parse(ident: &Ident) -> syn::Result<Self> {
        match ident.to_string().as_str() {
            "summary" => Ok(Self::Summary),
            "detail" => Ok(Self::Detail),
            "debug" => Ok(Self::Debug),
            _ => Err(syn::Error::new(
                ident.span(),
                "expected `summary`, `detail` or `debug`",
            )),
        }
    }

    fn variant(self) -> Ident {
        format_ident!(
            "{}",
            match self {
                Self::Summary => "Summary",
                Self::Detail => "Detail",
                Self::Debug => "Debug",
            }
        )
    }
}

/// A field's `unit` attribute: the runtime's `Unit` variant it names.
#[derive(Clone, Copy)]
enum Unit {
    LogicalPx,
    DevicePx,
    Seconds,
    Degrees,
    Radians,
    Fraction,
}

impl Unit {
    fn parse(ident: &Ident) -> syn::Result<Self> {
        match ident.to_string().as_str() {
            "logical_px" => Ok(Self::LogicalPx),
            "device_px" => Ok(Self::DevicePx),
            "seconds" => Ok(Self::Seconds),
            "degrees" => Ok(Self::Degrees),
            "radians" => Ok(Self::Radians),
            "fraction" => Ok(Self::Fraction),
            _ => Err(syn::Error::new(
                ident.span(),
                "expected `logical_px`, `device_px`, `seconds`, `degrees`, `radians` or `fraction`",
            )),
        }
    }

    fn variant(self) -> Ident {
        format_ident!(
            "{}",
            match self {
                Self::LogicalPx => "LogicalPx",
                Self::DevicePx => "DevicePx",
                Self::Seconds => "Seconds",
                Self::Degrees => "Degrees",
                Self::Radians => "Radians",
                Self::Fraction => "Fraction",
            }
        )
    }
}

/// What `#[inspect(...)]` says about one field.
#[derive(Default)]
struct FieldAttrs {
    skip: bool,
    name: Option<LitStr>,
    default: Option<DefaultAttr>,
    level: Option<Level>,
    unit: Option<Unit>,
    with: Option<Path>,
    debug: bool,
}

impl FieldAttrs {
    fn parse(field: &syn::Field) -> syn::Result<Self> {
        let mut attrs = Self::default();
        for attr in inspect_attributes(&field.attrs) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    set_flag(&mut attrs.skip, &meta)
                } else if meta.path.is_ident("debug") {
                    set_flag(&mut attrs.debug, &meta)
                } else if meta.path.is_ident("name") {
                    let name: LitStr = meta.value()?.parse()?;
                    set_once(&mut attrs.name, name, &meta)
                } else if meta.path.is_ident("default") {
                    let default = if meta.input.peek(syn::Token![=]) {
                        DefaultAttr::Expr(meta.value()?.parse()?)
                    } else {
                        DefaultAttr::Type
                    };
                    set_once(&mut attrs.default, default, &meta)
                } else if meta.path.is_ident("level") {
                    let level: Ident = meta.value()?.parse()?;
                    let level = Level::parse(&level)?;
                    set_once(&mut attrs.level, level, &meta)
                } else if meta.path.is_ident("unit") {
                    let unit: Ident = meta.value()?.parse()?;
                    let unit = Unit::parse(&unit)?;
                    set_once(&mut attrs.unit, unit, &meta)
                } else if meta.path.is_ident("with") {
                    let with: Path = meta.value()?.parse()?;
                    set_once(&mut attrs.with, with, &meta)
                } else {
                    Err(meta.error(
                        "unsupported inspect attribute; expected `skip`, `name`, `default`, `level`, `unit`, `with` or `debug`",
                    ))
                }
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

fn inspect_attributes(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attrs.iter().filter(|attr| attr.path().is_ident("inspect"))
}

/// Store an attribute's value; a second one for the same field is an error, since only one
/// could take effect.
fn set_once<T>(slot: &mut Option<T>, value: T, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        return Err(given_twice(meta));
    }
    Ok(())
}

fn set_flag(flag: &mut bool, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if std::mem::replace(flag, true) {
        return Err(given_twice(meta));
    }
    Ok(())
}

fn given_twice(meta: &ParseNestedMeta<'_>) -> syn::Error {
    meta.error("this inspect attribute is given twice for the field")
}

/// `#[inspect(...)]` describes a field: on the type or on a variant it would do nothing, so it
/// is refused there.
fn refuse_item_attributes(attrs: &[Attribute]) -> syn::Result<()> {
    inspect_attributes(attrs).next().map_or(Ok(()), |attr| {
        Err(syn::Error::new_spanned(
            attr,
            "`#[inspect(...)]` describes a field; write it on the field it describes",
        ))
    })
}

/// One field to describe.
struct Described<'a> {
    attrs: FieldAttrs,
    ty: &'a Type,
    /// The name the field reports under: its `name` attribute, or else its identifier
    /// (`r#` stripped) or its position in a tuple.
    reported: String,
    /// The expression that borrows the field: `&self.width`, or a binding of a `match` arm.
    access: TokenStream,
    span: Span,
}

/// What the derive describes: a struct's fields, or the fields of each of an enum's variants.
enum Shape<'a> {
    Struct(Vec<Described<'a>>),
    Enum(Vec<(&'a syn::Variant, Vec<Described<'a>>)>),
}

impl<'a> Shape<'a> {
    /// Read the type's fields and their attributes. Every refusal the derive makes of its
    /// input is made here, before any path of the runtime is looked up.
    fn parse(input: &'a DeriveInput) -> syn::Result<Self> {
        refuse_item_attributes(&input.attrs)?;
        match &input.data {
            Data::Struct(data) => Ok(Self::Struct(described(
                &data.fields,
                None,
                |member, _| quote!(&self.#member),
            )?)),
            Data::Enum(data) => data
                .variants
                .iter()
                .map(|variant| {
                    refuse_item_attributes(&variant.attrs)?;
                    let fields = described(&variant.fields, Some("variant"), |_, index| {
                        let binding = variant_binding(index);
                        quote!(#binding)
                    })?;
                    Ok((variant, fields))
                })
                .collect::<syn::Result<Vec<_>>>()
                .map(Self::Enum),
            Data::Union(_) => Err(syn::Error::new(
                input.span(),
                "`#[derive(Inspect)]` does not support unions: which field is live is unknown; implement `Inspect` by hand",
            )),
        }
    }

    /// The variants of an enum that has some and whose variants all carry no field: the shape
    /// that is a value of its own.
    fn fieldless_variants(&self) -> Option<impl Iterator<Item = &'a syn::Variant>> {
        let Self::Enum(variants) = self else {
            return None;
        };
        let fieldless = !variants.is_empty()
            && variants
                .iter()
                .all(|(variant, _)| matches!(variant.fields, Fields::Unit));
        fieldless.then(|| variants.iter().map(|(variant, _)| *variant))
    }
}

/// The name a `match` arm binds a variant's field at `index` to.
fn variant_binding(index: usize) -> Ident {
    format_ident!("__inspect_{}", index)
}

/// Expand `#[derive(Inspect)]`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let shape = Shape::parse(input)?;
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

    let body = match &shape {
        Shape::Struct(fields) => {
            let calls = field_calls(fields, &runtime, &type_params, &mut bounds);
            quote!(#(#calls)*)
        }
        Shape::Enum(variants) if variants.is_empty() => quote!(match *self {}),
        Shape::Enum(variants) => {
            let arms = variants.iter().map(|(variant, fields)| {
                let variant_ident = &variant.ident;
                let variant_name = variant_ident.unraw().to_string();
                let bindings = variant.fields.members().enumerate().map(|(index, member)| {
                    let binding = variant_binding(index);
                    match member {
                        Member::Named(name) => quote!(#name: #binding),
                        Member::Unnamed(_) => quote!(#binding),
                    }
                });
                let pattern = match &variant.fields {
                    Fields::Named(_) => quote!(Self::#variant_ident { #(#bindings),* }),
                    Fields::Unnamed(_) => quote!(Self::#variant_ident ( #(#bindings),* )),
                    Fields::Unit => quote!(Self::#variant_ident),
                };
                let calls = field_calls(fields, &runtime, &type_params, &mut bounds);
                quote! {
                    #[allow(unused_variables, reason = "a skipped field is bound and not read")]
                    #pattern => {
                        out.value(
                            "variant",
                            #runtime::Value::Variant { ty: #kind, name: #variant_name },
                        );
                        #(#calls)*
                    }
                }
            });
            // Collected here: the arms borrow `bounds`, which the `where` clause reads next.
            let arms: Vec<TokenStream> = arms.collect();
            quote!(match self { #(#arms)* })
        }
    };

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut where_clause = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    for bound in bounds {
        where_clause.predicates.push(parse_quote!(#bound));
    }
    // Every argument the concrete type spells, const values included, as `type_name` writes it.
    let generic_args = input
        .generics
        .params
        .iter()
        .any(|param| matches!(param, GenericParam::Type(_) | GenericParam::Const(_)));
    let kind_args = generic_args.then(|| {
        quote! {
            fn kind_args(&self, args: &mut dyn ::core::ops::FnMut(&'static str)) {
                #runtime::kind_args_of(::core::any::type_name::<Self>(), args);
            }
        }
    });

    let to_value = shape.fieldless_variants().map(|variants| {
        let arms = variants.map(|variant| {
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
    access: impl Fn(&Member, usize) -> TokenStream,
) -> syn::Result<Vec<Described<'a>>> {
    let mut out: Vec<Described<'a>> = Vec::with_capacity(fields.len());
    for (index, (field, member)) in fields.iter().zip(fields.members()).enumerate() {
        let attrs = FieldAttrs::parse(field)?;
        if attrs.skip {
            continue;
        }
        let reported = attrs.name.as_ref().map_or_else(
            || match &member {
                Member::Named(ident) => ident.unraw().to_string(),
                Member::Unnamed(position) => position.index.to_string(),
            },
            LitStr::value,
        );
        if reserved == Some(reported.as_str()) {
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "`{reported}` names the enum's variant in the snapshot; report this field under another name with `#[inspect(name = \"...\")]`"
                ),
            ));
        }
        if out.iter().any(|other| other.reported == reported) {
            return Err(syn::Error::new(
                field.span(),
                format!(
                    "another field already reports as `{reported}`; give one of them another name with `#[inspect(name = \"...\")]`"
                ),
            ));
        }
        out.push(Described {
            access: access(&member, index),
            attrs,
            ty: &field.ty,
            reported,
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
            let name = &field.reported;
            let generic = mentions_param(ty, type_params);
            let start = if let Some(with) = &field.attrs.with {
                quote_spanned!(field.span=> out.value(#name, #with(#access)))
            } else if field.attrs.debug {
                if generic {
                    bounds.push(quote!(#ty: ::core::fmt::Debug));
                }
                quote_spanned!(field.span=> out.value(#name, #runtime::Value::Debug(#runtime::Untyped::new(#access))))
            } else {
                if generic {
                    bounds.push(quote!(#ty: #runtime::FieldValue));
                }
                quote_spanned!(field.span=> out.field(#name, #access))
            };
            let level = field.attrs.level.map(|level| {
                let variant = level.variant();
                quote!(.level(#runtime::Level::#variant))
            });
            let unit = field.attrs.unit.map(|unit| {
                let variant = unit.variant();
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
            // `#[inspect(debug)]` is the field's opt-in to the untyped escape hatch, so the
            // deprecated constructor it reaches is allowed here (ADR-0303 §7).
            let opt_in = field.attrs.debug.then(|| {
                quote!(#[allow(
                    deprecated,
                    reason = "`#[inspect(debug)]` opts this field into the untyped escape hatch"
                )])
            });
            quote! {
                #default_let
                #opt_in #start #level #unit #default_call;
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    // What an accepted derive writes is tested from a consuming crate: expansion resolves the
    // runtime path from the consuming manifest, which this crate lacks. A refusal is made
    // before that lookup, so `expand` answers with it here.
    #[test]
    fn an_inspect_attribute_that_would_do_nothing_is_refused() {
        let cases: [(&str, DeriveInput, &str); 6] = [
            (
                "on the type",
                parse_quote! { #[inspect(skip)] struct S { width: f64 } },
                "describes a field",
            ),
            (
                "on a variant",
                parse_quote! { enum E { #[inspect(name = "a")] A } },
                "describes a field",
            ),
            (
                "a name given twice",
                parse_quote! { struct S { #[inspect(name = "a", name = "b")] width: f64 } },
                "given twice",
            ),
            (
                "a level given in two attributes",
                parse_quote! {
                    struct S { #[inspect(level = detail)] #[inspect(level = debug)] width: f64 }
                },
                "given twice",
            ),
            (
                "a flag given twice",
                parse_quote! { enum E { A(#[inspect(debug, debug)] f64) } },
                "given twice",
            ),
            (
                "a default given both ways",
                parse_quote! { struct S { #[inspect(default, default = 1.0)] width: f64 } },
                "given twice",
            ),
        ];
        for (case, input, expected) in cases {
            let error = match expand(&input) {
                Ok(_) => panic!("BUG: `{case}` expands"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains(expected), "{case}: {error}");
        }
    }
}
