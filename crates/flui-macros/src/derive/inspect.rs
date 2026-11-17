//! Codegen for `#[derive(Inspect)]` (ADR-0242 §4).
//!
//! The derive writes `kind`, `kind_args` for a generic type, and an `inspect` body with one
//! `Fields` call per field. Field types are read through the runtime's `FieldValue`, whose
//! `on_unimplemented` names the ways out, so an unmapped field fails at the derive with that
//! hint. A fieldless enum also gets `ToValue`, so it can be another type's field.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, GenericParam, Ident, LitStr, Member, Path, Type,
    ext::IdentExt, parse_quote, spanned::Spanned,
};

use super::attribute::{is_named, named, parse_entries, refuse_unread};
use crate::generics::mentions_param;
use crate::hygiene::local;
use crate::refusals::Refusals;

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
        match ident.unraw().to_string().as_str() {
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
        match ident.unraw().to_string().as_str() {
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

/// The entries a field's `#[inspect(...)]` list takes, as a refusal names them.
const FIELD_ENTRIES: &str = "`skip`, `name`, `default`, `level`, `unit`, `with` or `debug`";

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

/// An entry of a field's `#[inspect(...)]` list.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Skip,
    Name,
    Default,
    Level,
    Unit,
    With,
    Debug,
}

impl Entry {
    const NAMES: [(&'static str, Self); 7] = [
        ("skip", Self::Skip),
        ("name", Self::Name),
        ("default", Self::Default),
        ("level", Self::Level),
        ("unit", Self::Unit),
        ("with", Self::With),
        ("debug", Self::Debug),
    ];

    /// The entry `meta` names, or the refusal of a name the list does not take.
    fn parse(meta: &ParseNestedMeta<'_>) -> syn::Result<Self> {
        Self::NAMES
            .iter()
            .find(|(name, _)| is_named(&meta.path, name))
            .map(|(_, entry)| *entry)
            .ok_or_else(|| {
                meta.error(format!(
                    "unsupported inspect attribute; expected {FIELD_ENTRIES}"
                ))
            })
    }
}

impl FieldAttrs {
    /// Take what `entry` says from its place in a list, value included.
    fn read(&mut self, entry: Entry, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
        match entry {
            Entry::Skip => self.skip = true,
            Entry::Debug => self.debug = true,
            Entry::Name => self.name = Some(meta.value()?.parse::<LitStr>()?),
            Entry::Default => {
                self.default = Some(if meta.input.peek(syn::Token![=]) {
                    DefaultAttr::Expr(meta.value()?.parse::<Expr>()?)
                } else {
                    DefaultAttr::Type
                });
            }
            Entry::Level => {
                let level: Ident = meta.value()?.call(Ident::parse_any)?;
                self.level = Some(Level::parse(&level)?);
            }
            Entry::Unit => {
                let unit: Ident = meta.value()?.call(Ident::parse_any)?;
                self.unit = Some(Unit::parse(&unit)?);
            }
            Entry::With => self.with = Some(meta.value()?.parse::<Path>()?),
        }
        Ok(())
    }

    /// Read the field's `#[inspect(...)]` lists, then check the combinations of what was
    /// read. An entry given twice is refused, since only one could take effect, and is still
    /// read, so what it holds is checked as well.
    fn parse(field: &syn::Field) -> syn::Result<Self> {
        Refusals::collect(|refusals| {
            let mut attrs = Self::default();
            let mut given: Vec<Entry> = Vec::new();
            for attr in inspect_attributes(&field.attrs) {
                parse_entries(attr, FIELD_ENTRIES, refusals, |meta, refusals| {
                    let entry = Entry::parse(&meta)?;
                    let twice = given.contains(&entry);
                    if !twice {
                        given.push(entry);
                    }
                    let read = attrs.read(entry, &meta);
                    // After the value, so that the refusal underlines the whole entry.
                    if twice {
                        refusals.push(
                            meta.error("this inspect attribute is given twice for the field"),
                        );
                    }
                    read
                });
            }
            if attrs.skip
                && (attrs.name.is_some()
                    || attrs.default.is_some()
                    || attrs.level.is_some()
                    || attrs.unit.is_some()
                    || attrs.with.is_some()
                    || attrs.debug)
            {
                refusals.push(syn::Error::new_spanned(
                    field,
                    "a skipped field takes no other inspect attribute",
                ));
            }
            if attrs.debug && attrs.with.is_some() {
                refusals.push(syn::Error::new_spanned(
                    field,
                    "`debug` and `with` both map the field; keep one",
                ));
            }
            if attrs.debug && attrs.default.is_some() {
                refusals.push(syn::Error::new_spanned(
                    field,
                    "a `debug` field has no typed value to compare a default with; map it with `with` instead",
                ));
            }
            Ok(attrs)
        })
    }
}

fn inspect_attributes(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    named(attrs, "inspect")
}

/// `#[inspect(...)]` describes a field: on the type or on a variant it would do nothing, so it
/// is refused there.
fn refuse_item_attributes(attrs: &[Attribute], refusals: &mut Refusals) {
    for attr in inspect_attributes(attrs) {
        refusals.push(syn::Error::new_spanned(
            attr,
            "`#[inspect(...)]` describes a field; write it on the field it describes",
        ));
    }
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
    /// input is made here, before any path of the runtime is looked up, and all of them are
    /// reported together.
    fn parse(input: &'a DeriveInput) -> syn::Result<Self> {
        Refusals::collect(|refusals| {
            refuse_item_attributes(&input.attrs, refusals);
            refuse_unread(input, &["inspect"], refusals);
            match &input.data {
                Data::Struct(data) => Ok(Self::Struct(described(
                    &data.fields,
                    None,
                    |member, _| quote!(&self.#member),
                    refusals,
                ))),
                Data::Enum(data) => Ok(Self::Enum(
                    data.variants
                        .iter()
                        .map(|variant| {
                            refuse_item_attributes(&variant.attrs, refusals);
                            let fields = described(
                                &variant.fields,
                                Some("variant"),
                                |_, index| variant_binding(index).into_token_stream(),
                                refusals,
                            );
                            (variant, fields)
                        })
                        .collect(),
                )),
                Data::Union(_) => Err(syn::Error::new(
                    input.ident.span(),
                    "`#[derive(Inspect)]` does not support unions: which field is live is unknown; implement `Inspect` by hand",
                )),
            }
        })
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
    local(&format!("field_{index}"))
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
    // The `Fields` the generated `inspect` writes to.
    let out = local("out");

    let body = match &shape {
        Shape::Struct(fields) => {
            let calls = field_calls(fields, &runtime, &type_params, &out, &mut bounds);
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
                let calls = field_calls(fields, &runtime, &type_params, &out, &mut bounds);
                quote! {
                    #[allow(unused_variables, reason = "a skipped field is bound and not read")]
                    #pattern => {
                        #out.value(
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
        let args = local("args");
        quote! {
            fn kind_args(&self, #args: &mut dyn ::core::ops::FnMut(&'static str)) {
                #runtime::kind_args_of(::core::any::type_name::<Self>(), #args);
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

            fn inspect(&self, #out: &mut #runtime::Fields<'_>) {
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
    refusals: &mut Refusals,
) -> Vec<Described<'a>> {
    let mut out: Vec<Described<'a>> = Vec::with_capacity(fields.len());
    for (index, (field, member)) in fields.iter().zip(fields.members()).enumerate() {
        let Some(attrs) = refusals.value(FieldAttrs::parse(field)) else {
            continue;
        };
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
            refusals.push(syn::Error::new_spanned(
                field,
                format!(
                    "`{reported}` names the enum's variant in the snapshot; report this field under another name with `#[inspect(name = \"...\")]`"
                ),
            ));
            continue;
        }
        if out.iter().any(|other| other.reported == reported) {
            refusals.push(syn::Error::new_spanned(
                field,
                format!(
                    "another field already reports as `{reported}`; give one of them another name with `#[inspect(name = \"...\")]`"
                ),
            ));
            continue;
        }
        out.push(Described {
            access: access(&member, index),
            attrs,
            ty: &field.ty,
            reported,
            span: field.ty.span(),
        });
    }
    out
}

fn field_calls(
    fields: &[Described<'_>],
    runtime: &Path,
    type_params: &[&Ident],
    out: &Ident,
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
                quote_spanned!(field.span=> #out.value(#name, #with(#access)))
            } else if field.attrs.debug {
                if generic {
                    bounds.push(quote!(#ty: ::core::fmt::Debug));
                }
                quote_spanned!(field.span=> #out.value(#name, #runtime::Value::Debug(#runtime::Untyped::new(#access))))
            } else {
                if generic {
                    bounds.push(quote!(#ty: #runtime::FieldValue));
                }
                quote_spanned!(field.span=> #out.field(#name, #access))
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
                    let binding = local(&format!("default_{index}"));
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
    use crate::refusals::assert_refused;

    // What an accepted derive writes is tested from a consuming crate: expansion resolves the
    // runtime path from the consuming manifest, which this crate lacks. A refusal is made
    // before that lookup, so `expand` answers with it here.
    #[test]
    fn an_inspect_attribute_that_would_do_nothing_is_refused() {
        let cases: [(&str, DeriveInput, &str); 9] = [
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
            (
                "spelled with a raw identifier on the type",
                parse_quote! { #[r#inspect(skip)] struct S { width: f64 } },
                "describes a field",
            ),
            (
                "an unknown entry behind a raw identifier",
                parse_quote! { struct S { #[r#inspect(nonsense)] width: f64 } },
                "unsupported inspect attribute",
            ),
            (
                "an empty list",
                parse_quote! { struct S { #[inspect()] width: f64 } },
                "the list is empty; expected `skip`",
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

    #[test]
    fn an_entry_and_its_value_read_the_same_spelled_raw() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[inspect(r#skip)]
                a: f64,
                #[r#inspect(r#name = "w", r#level = r#debug, r#unit = r#seconds)]
                b: f64,
            }
        };
        let Data::Struct(data) = &input.data else {
            panic!("BUG: the input is a struct");
        };
        let attrs: Vec<FieldAttrs> = data
            .fields
            .iter()
            .map(|field| FieldAttrs::parse(field).expect("BUG: the field's attributes read"))
            .collect();
        assert!(attrs[0].skip);
        assert_eq!(
            attrs[1].name.as_ref().map(LitStr::value).as_deref(),
            Some("w")
        );
        assert!(matches!(attrs[1].level, Some(Level::Debug)));
        assert!(matches!(attrs[1].unit, Some(Unit::Seconds)));
    }

    #[test]
    fn every_refusal_of_a_described_type_is_reported_in_one_build() {
        let input: DeriveInput = parse_quote! {
            #[inspect(skip)]
            enum E<#[inspect(skip)] T> {
                #[inspect(skip)]
                A {
                    #[inspect(debug = 1, nonsense)]
                    a: f64,
                    #[inspect(name = "x", name = "y")]
                    #[inspect()]
                    b: f64,
                    variant: f64,
                    #[inspect(skip, debug, with = describe, default)]
                    c: T,
                    #[inspect(level = bogus, unit = bogus, level = detail, nonsense = out, name = "d")]
                    d: f64,
                },
            }
        };
        assert_refused(
            expand(&input),
            &[
                "describes a field",
                "`#[inspect]` does nothing here",
                "describes a field",
                "expected `,`",
                "unsupported inspect attribute",
                "given twice",
                "the list is empty",
                "`variant` names the enum's variant",
                "a skipped field takes no other inspect attribute",
                "`debug` and `with` both map the field",
                "a `debug` field has no typed value",
                "expected `summary`, `detail` or `debug`",
                "expected `logical_px`",
                "given twice",
                "unsupported inspect attribute",
            ],
        );
    }
}
