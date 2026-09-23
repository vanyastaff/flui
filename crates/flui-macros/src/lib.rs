//! # FLUI Macros
//!
//! Procedural macros for the FLUI framework.
//!
//! ## Derives
//!
//! - [`macro@StatelessView`] — emit `impl View` for a `StatelessView`
//!   type. Replaces the legacy `impl_stateless_view!` declarative
//!   macro that this crate deletes.
//! - [`macro@StatefulView`] — emit `impl View` for a `StatefulView`
//!   type. Replaces the legacy `impl_stateful_view!` declarative
//!   macro (also deleted here).
//!
//! Both derives are re-exported from `flui_view::prelude` so widget
//! authors write a single `use flui_view::prelude::*;` and pick up the
//! derives alongside the supporting trait — no extra `use
//! flui_macros::…` import.
//!
//! ## Why a separate crate?
//!
//! `proc-macro` crates must be `[lib] proc-macro = true`, which makes
//! them leaf crates — they cannot depend on or be depended on by
//! ordinary library crates in the usual sense (only their generated
//! tokens reach the consuming crate). Splitting the derives into
//! `flui-macros` keeps `flui-view` itself free of the `proc-macro`
//! constraint while letting widget authors `#[derive(StatelessView)]`
//! after a single `use flui_view::prelude::*;`.
//!
//! ## Generated-code path strategy
//!
//! Derives resolve the owning runtime crate from the consumer's Cargo manifest,
//! honoring renamed dependencies. A direct runtime dependency takes precedence;
//! otherwise the generated code uses `flui::view`, `flui::foundation`, or
//! `flui::animation` through the facade (including a renamed facade).
//! Framework library code and integration targets use the same absolute path.

// Ship bar (wave 1): every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

mod derive_animatable;
mod derive_diagnosticable;
mod derive_inherited_data;
mod derive_stateful;
mod derive_stateless;
mod runtime_path;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Emit `impl View` for a `StatelessView` type.
///
/// Generates the `create_element` boilerplate
/// (`ElementKind::stateless(self)`) so the author only writes the struct
/// + its `impl StatelessView for X { fn build(...) -> impl IntoView }`
///   block.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::prelude::*;
///
/// #[derive(Clone, StatelessView)]
/// struct Greeting { name: String }
///
/// impl StatelessView for Greeting {
///     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
///         // ... return a child View tree ...
///     }
/// }
/// ```
///
/// The derive forwards generic parameters and where clauses to the
/// emitted `impl View` block verbatim, and adds a `Self:
/// StatelessView` predicate so a missing `impl StatelessView for X`
/// block surfaces as a clear "trait not implemented" diagnostic at
/// the derive site rather than as a cascade of unrelated errors deep
/// inside `StatelessBehavior`'s type machinery.
///
/// # Keyed widgets
///
/// The default `View::key()` returns `None`. Authors who need a
/// keyed widget cannot stack a separate `impl View for X { fn key() }`
/// block alongside the derive — Rust forbids two `impl View for X`
/// blocks. The supported pattern is to drop the derive and write the
/// `impl View` block manually:
///
/// ```rust,ignore
/// impl View for Greeting {
///     fn create_element(&self) -> crate::element::ElementKind {
///         crate::element::ElementKind::stateless(self)
///     }
///     fn key(&self) -> Option<&dyn ViewKey> {
///         Some(&self.key)
///     }
/// }
/// ```
///
/// A `#[view(key = "<expr>")]` derive attribute that auto-wires a
/// field-named key is a deferred ergonomics improvement (post-
/// Catalog.1; tracked in
/// `docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`
/// "Open Questions").
#[proc_macro_derive(StatelessView)]
pub fn derive_stateless_view(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_stateless::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive [`InheritedData`] for a provider's data struct: one
/// `pub const FIELD_<NAME>: FieldMask<Self>` per field plus `field_mask_diff`, so an
/// `InheritedView` can report which fields changed and dependents that used
/// `depend_on_field` rebuild only for those (issue #1090).
///
/// Every field must be `PartialEq`; at most 64 fields.
///
/// ```rust,ignore
/// #[derive(Clone, PartialEq, InheritedData)]
/// pub struct MediaQueryData { pub size: Size, pub text_scale_factor: f32, /* … */ }
///
/// impl InheritedView for MediaQuery {
///     type Data = MediaQueryData;
///     fn changed_fields(&self, old: &Self) -> FieldMask<Self::Data> { self.data.field_mask_diff(&old.data) }
///     /* … */
/// }
/// // A reader that depends on the size only:
/// let size = ctx.depend_on_field::<MediaQuery, _>(MediaQueryData::FIELD_SIZE, |mq| mq.data().size);
/// ```
///
/// [`InheritedData`]: ../flui_view/trait.InheritedData.html
#[proc_macro_derive(InheritedData)]
pub fn derive_inherited_data(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_inherited_data::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl View` for a `StatefulView` type.
///
/// Generates the `create_element` boilerplate
/// (`ElementKind::stateful(self)`). The author still writes the
/// `impl StatefulView for X` and the corresponding
/// `impl ViewState<X> for XState` blocks — the derive covers only the
/// `impl View` boilerplate.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::prelude::*;
///
/// #[derive(Clone, StatefulView)]
/// struct Counter { initial: u32 }
///
/// struct CounterState { count: u32 }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///     fn create_state(&self) -> CounterState {
///         CounterState { count: self.initial }
///     }
/// }
///
/// impl ViewState<Counter> for CounterState {
///     fn build(&self, view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
///         // ... return a child View tree ...
///     }
/// }
/// ```
///
/// See [`macro@StatelessView`] for the generated-code path strategy
/// (Cargo-resolved runtime paths) and keyed-widget workaround
/// notes — the same patterns apply.
#[proc_macro_derive(StatefulView)]
pub fn derive_stateful_view(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_stateful::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl Diagnosticable` for a named-field struct.
///
/// Generates the `debug_fill_properties` method that pushes one diagnostic
/// property per field (formatted via `{:?}`). The `Diagnosticable` trait's
/// default `to_diagnostics_node` then builds the node from those properties
/// using the short (module-path-stripped) type name, so the derive deliberately
/// generates *only* `debug_fill_properties` — overriding `to_diagnostics_node`
/// would duplicate the trait default and risk drift.
///
/// # Example
///
/// ```rust,ignore
/// use flui_foundation::Diagnosticable;
/// use flui_macros::Diagnosticable;
///
/// #[derive(Debug, Diagnosticable)]
/// struct Padding {
///     inset: f32,
///     #[diagnostic(skip)]
///     cache_key: u64,
/// }
/// ```
///
/// # Field attributes
///
/// `#[diagnostic(skip)]` omits a field from the diagnostics output. Any other
/// `diagnostic(...)` sub-attribute is a hard error. Non-`diagnostic` attributes
/// are ignored.
///
/// # Generics
///
/// Generic parameters and where-clauses are forwarded verbatim; the derive adds
/// a `Self: Debug` bound (the trait supertrait) plus a `Debug` bound per included
/// field type, so a non-`Debug` field surfaces a clear error at the derive site.
///
/// # Supported shapes
///
/// Only structs with named fields are supported. The derive rejects enums,
/// unions, tuple structs and unit structs with a compile-time diagnostic:
///
/// ```rust,compile_fail
/// use flui_macros::Diagnosticable;
///
/// #[derive(Debug, Diagnosticable)]
/// enum NotSupported {
///     A,
///     B,
/// }
/// ```
///
/// ```rust,compile_fail
/// use flui_macros::Diagnosticable;
///
/// #[derive(Debug, Diagnosticable)]
/// struct AlsoNotSupported(u32, u32);
/// ```
///
/// An unknown `diagnostic(...)` sub-attribute is likewise a hard error:
///
/// ```rust,compile_fail
/// use flui_macros::Diagnosticable;
///
/// #[derive(Debug, Diagnosticable)]
/// struct Bad {
///     #[diagnostic(nonsense)]
///     field: u32,
/// }
/// ```
#[proc_macro_derive(Diagnosticable, attributes(diagnostic))]
pub fn derive_diagnosticable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_diagnosticable::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl TwoWayConverter` for a struct of `f32` fields,
/// so the type can be spring-animated by `flui_animation::AnimatedValue`.
///
/// Every field must be `f32`; a non-`f32` field is a compile error. The type
/// must also be `Clone` (the trait's supertrait).
///
/// # Example
///
/// ```rust,ignore
/// use flui_animation::Animatable;
///
/// #[derive(Clone, Animatable)]
/// struct Translation {
///     x: f32,
///     y: f32,
///     z: f32,
/// }
/// ```
#[proc_macro_derive(Animatable)]
pub fn derive_animatable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_animatable::expand(&input).into()
}
