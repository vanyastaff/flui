//! Codegen for `#[derive(Diagnosticable)]`.
//!
//! Generates a `debug_fill_properties` implementation that pushes one
//! diagnostic property per (non-skipped) named field. The authoring shape
//! becomes:
//!
//! ```rust,ignore
//! #[derive(Debug, Diagnosticable)]
//! struct Padding {
//!     inset: f32,
//!     #[diagnostic(skip)]
//!     cache_key: u64,
//! }
//! ```
//!
//! No hand-written `impl Diagnosticable for Padding` block.
//!
//! ## Why only `debug_fill_properties` (and not `to_diagnostics_node`)
//!
//! The `Diagnosticable` trait already ships a *default*
//! `to_diagnostics_node` (see `flui-foundation/src/debug.rs`) that
//! constructs the node from the short (module-path-stripped) type name and
//! fills it by calling `debug_fill_properties`. Overriding it in the derive
//! would duplicate that logic and risk drift, so the derive emits the
//! single method the default cannot infer — the per-field property fill.
//! The default's `type_name::<Self>()` strips the module path, yielding the
//! short name (`"TestWidget"`) for non-generic types; for a monomorphized
//! generic it keeps the type arguments (`"Wrap<u32>"`), which is the strictly
//! more informative form. The derive does not touch the node name.
//!
//! ## Generated-code path strategy
//!
//! The emitted `impl` references runtime items via the absolute
//! `::flui_foundation::…` path: every consumer of the derive must have
//! `flui-foundation` as a direct dependency. This matches the
//! `::flui_view::…` strategy used by the `StatelessView` / `StatefulView`
//! derives.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type, ext::IdentExt, parse_quote, spanned::Spanned};

/// One named field that survives the `#[diagnostic(skip)]` filter.
struct IncludedField<'a> {
    /// The field access token (e.g. `width`, or a raw `r#type`).
    ident: &'a syn::Ident,
    /// The field type — used to add a per-field `Debug` where-bound.
    ty: &'a Type,
    /// The diagnostic name string, with any raw-identifier `r#` prefix
    /// stripped (so `r#type` reports as `"type"`).
    name: String,
}

/// Expand `#[derive(Diagnosticable)]` into an `impl Diagnosticable` block.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;

    // Q4: only named-field structs are supported. Reject enums, unions,
    // tuple structs and unit structs with a clean compile-time diagnostic.
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(_) => {
                return Err(syn::Error::new(
                    input.span(),
                    "`#[derive(Diagnosticable)]` does not support tuple structs; \
                     use a struct with named fields",
                ));
            }
            Fields::Unit => {
                return Err(syn::Error::new(
                    input.span(),
                    "`#[derive(Diagnosticable)]` does not support unit structs; \
                     use a struct with named fields",
                ));
            }
        },
        Data::Enum(_) => {
            return Err(syn::Error::new(
                input.span(),
                "`#[derive(Diagnosticable)]` does not support enums; \
                 use a struct with named fields",
            ));
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                input.span(),
                "`#[derive(Diagnosticable)]` does not support unions; \
                 use a struct with named fields",
            ));
        }
    };

    // Collect the included fields, honoring `#[diagnostic(skip)]`.
    let mut included: Vec<IncludedField<'_>> = Vec::with_capacity(fields.len());
    for field in fields {
        if field_is_skipped(field)? {
            continue;
        }
        // Named-field structs always carry a field ident.
        let field_ident = field
            .ident
            .as_ref()
            .expect("named-field struct field has an ident");
        // Q6: strip the raw-identifier `r#` prefix for the diagnostic name.
        let name = field_ident.unraw().to_string();
        included.push(IncludedField {
            ident: field_ident,
            ty: &field.ty,
            name,
        });
    }

    // Q4: support generics — forward the user's params/where-clause and
    // augment with the Debug bounds required by the generated body.
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Q2: `Self: Debug` (the trait supertrait) plus a per-included-field
    // `#ty: Debug` bound, so a generic field type that is not `Debug`
    // surfaces a clear error at the derive site rather than inside the
    // generated `format!`.
    let mut augmented_where = where_clause.cloned().unwrap_or_else(|| parse_quote!(where));
    augmented_where
        .predicates
        .push(parse_quote!(Self: ::std::fmt::Debug));
    for field in &included {
        let ty = field.ty;
        augmented_where
            .predicates
            .push(parse_quote!(#ty: ::std::fmt::Debug));
    }

    let field_names = included.iter().map(|f| &f.name);
    let field_idents = included.iter().map(|f| f.ident);

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::flui_foundation::Diagnosticable for #ident #ty_generics
        #augmented_where
        {
            fn debug_fill_properties(
                &self,
                builder: &mut ::flui_foundation::DiagnosticsBuilder,
            ) {
                #(
                    builder.add(
                        #field_names,
                        ::std::format!("{:?}", &self.#field_idents),
                    );
                )*
            }
        }
    })
}

/// Returns `true` if the field carries `#[diagnostic(skip)]`.
///
/// Q3: parses every `#[diagnostic(...)]` attribute via syn-v2
/// `parse_nested_meta`. A bare `skip` sets the flag; any other sub-attribute
/// (e.g. `diagnostic(foo)`) is a hard error. Non-`diagnostic` attributes are
/// ignored entirely.
fn field_is_skipped(field: &syn::Field) -> syn::Result<bool> {
    let mut skipped = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("diagnostic") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                skipped = true;
                Ok(())
            } else {
                Err(meta.error("unsupported diagnostic attribute; expected `skip`"))
            }
        })?;
    }
    Ok(skipped)
}
