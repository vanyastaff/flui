//! # FLUI Macros
//!
//! The procedural macros of the FLUI framework. Each is re-exported beside
//! the trait or function it serves, so an author imports it from there and
//! never names this crate:
//!
//! | Macro | Writes | Re-exported as |
//! |-------|--------|----------------|
//! | [`macro@StatelessView`], [`macro@StatefulView`] | `impl View` for a type that implements the authoring trait | `flui_view::StatelessView`, `flui_view::StatefulView` |
//! | [`macro@InheritedData`] | one field mask per field and `impl InheritedData` | `flui_view::InheritedData` |
//! | [`macro@Inspect`] | `impl Inspect` for a struct or an enum | `flui_foundation::inspect::Inspect` |
//! | [`macro@TwoWayConverter`] | `impl TwoWayConverter` and a fieldwise `impl Lerp` | `flui_animation::TwoWayConverter` |
//! | [`macro@Routable`] | `impl Routable` from `#[route("…")]` patterns | `flui_widgets::router::Routable` |
//! | [`macro@main`] | the entry every platform starts an application from | `flui::main` |
//!
//! ## Paths in generated code
//!
//! An expansion names its runtime crate through the consumer's Cargo
//! manifest, under whatever name the manifest gives it. A derive looks for
//! `flui-sdk` first (a package), then the crate that owns the trait (a
//! framework crate), then the `flui` facade (an application). `#[flui::main]`
//! looks for `flui-app`, then the facade: the SDK has no application runtime.
//! A crate with none of them gets a compile error that says which dependency
//! to add.

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
// A macro that prints writes into the compiler's output of every crate that uses it.
#![warn(clippy::print_stdout, clippy::print_stderr)]

mod derive;
mod entry;
mod generics;
mod hygiene;
mod refusals;
mod runtime_path;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

use derive::view::ViewKind;

/// Emit `impl View` for a type that implements `StatelessView`.
///
/// The author writes the struct and its `impl StatelessView`; the derive
/// writes `View::create_element`, which mounts a stateless element.
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
/// The type's generic parameters and where clause carry over to the `impl`,
/// which also requires `Self: StatelessView`: a type that derives without
/// implementing the authoring trait is told so at the derive.
///
/// # Inspection
///
/// `#[view(inspect)]` on the type fills `View::inspect` from the type's own
/// `Inspect`, so a view that also derives `Inspect` shows its fields in the
/// element tree's snapshot (ADR-0242 §3):
///
/// ```rust,ignore
/// #[derive(Clone, StatelessView, Inspect)]
/// #[view(inspect)]
/// struct Greeting { name: String }
/// ```
///
/// # Creation sites
///
/// A field of type `CreationSite` marked `#[view(site)]` is the view's
/// creation site (`View::creation_site`). Its `#[track_caller]` constructor
/// fills it with `CreationSite::here()`, and the element copies it at mount.
/// One field of a struct, named or positional, can carry the mark.
///
/// ```rust,ignore
/// #[derive(Clone, StatelessView)]
/// struct Badge {
///     label: String,
///     #[view(site)]
///     site: CreationSite,
/// }
///
/// impl Badge {
///     #[track_caller]
///     fn new(label: impl Into<String>) -> Self {
///         Self { label: label.into(), site: CreationSite::here() }
///     }
/// }
/// ```
///
/// # Keys
///
/// A field marked `#[view(key)]` is the view's key (`View::key`), by which
/// the parent matches it to the element it had before. The field is a
/// `ViewKey` (`ValueKey<T>`, `UniqueKey`, `GlobalKey<T>`, …), an `Option` of
/// one for a view that is keyed only sometimes, or an `Rc<dyn ViewKey>` or
/// `Box<dyn ViewKey>` for a key whose type the view does not name
/// (`flui_foundation::key::AsViewKey` lists them). One field of a struct can
/// carry the mark; without it the view has no key.
///
/// ```rust,ignore
/// #[derive(Clone, StatelessView)]
/// struct Row {
///     #[view(key)]
///     key: ValueKey<u64>,
///     label: String,
/// }
/// ```
///
/// # Refused attributes
///
/// `#[view(...)]` is a compile error wherever the derive could not act on it,
/// and one build reports every such place:
/// `inspect` on a field or given twice, `key` or `site` on the type, on two
/// fields or twice on one, an empty list, any `#[view(...)]` inside an
/// enum or a union, whose values have no field of `self` to read, and one on
/// a generic parameter or inside a field's type. A name spelled as a raw
/// identifier (`#[r#view(r#key)]`) is read as the name.
#[proc_macro_derive(StatelessView, attributes(view))]
pub fn derive_stateless_view(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::view::expand(&input, ViewKind::Stateless)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl View` for a type that implements `StatefulView`.
///
/// The author writes the struct, its `impl StatefulView` and the state's
/// `impl ViewState`; the derive writes `View::create_element`, which mounts a
/// stateful element.
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
///     fn create_state(&self, _ctx: &dyn LifecycleContext) -> CounterState {
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
/// `#[view(inspect)]`, `#[view(key)]`, `#[view(site)]` and their refusals work
/// as they do for [`macro@StatelessView`].
#[proc_macro_derive(StatefulView, attributes(view))]
pub fn derive_stateful_view(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::view::expand(&input, ViewKind::Stateful)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `InheritedData` (`flui_view`) for a provider's data struct: one
/// `pub const FIELD_<NAME>: FieldMask<Self>` per field plus `field_mask_diff`, so an
/// `InheritedView` can report which fields changed and dependents that used
/// `depend_on_field` rebuild only for those (issue #1090).
///
/// The type is a struct with named fields and no generic parameters: a
/// provider's data type is concrete, and the constants are named after the
/// fields (`r#type` gives `FIELD_TYPE`). Every field must be `PartialEq`,
/// which the generated comparison reports at the field. A struct with more
/// than 64 fields is refused, since a mask carries 64 bits: split the
/// provider's data.
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
#[proc_macro_derive(InheritedData)]
pub fn derive_inherited_data(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::inherited_data::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl Inspect` (`flui_foundation::inspect`) for a struct or an enum (ADR-0242 §4).
///
/// Re-exported as `flui_foundation::inspect::Inspect`, beside the trait, which documents the
/// field attributes. `#[inspect(...)]` describes a field: on the type, on a variant, on a generic
/// parameter or inside a field's type, with an empty list or with one attribute given twice, it
/// is a compile error, and one build reports every one. A name or an identifier value spelled as
/// a raw identifier (`r#skip`, `level = r#debug`) is read as the name.
///
/// A field whose type has no value mapping is a compile error naming the ways out (`skip`,
/// `with`, `debug`, or a `ToValue` impl); a union is refused, because which of its fields is
/// live is unknown.
#[proc_macro_derive(Inspect, attributes(inspect))]
pub fn derive_inspect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::inspect::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl TwoWayConverter` and a fieldwise `impl Lerp` for a nonempty struct,
/// so the type can be spring-animated by
/// `flui_animation::AnimatedValue`, tweened, and used as a
/// `flui_animation::Keyframes` value.
///
/// Each field must implement `TwoWayConverter` and `Lerp`, with a concrete
/// vector width. Nested values are flattened in declaration order, and each
/// field retains its interpolation contract. A width that depends on a type,
/// const or lifetime parameter cannot be summed in an array length on stable
/// Rust, so such a type is refused. The struct must be `Clone`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_animation::TwoWayConverter;
///
/// #[derive(Clone, TwoWayConverter)]
/// struct Translation {
///     x: f64,
///     y: f64,
///     z: f64,
/// }
/// ```
#[proc_macro_derive(TwoWayConverter)]
pub fn derive_two_way_converter(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::two_way_converter::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Emit `impl Routable` for a route enum: `to_path` and `from_path` from one
/// `#[route("…")]` pattern per variant (ADR-0330 §1).
///
/// ```rust,ignore
/// use flui_widgets::prelude::*;
///
/// #[derive(Routable, Clone, PartialEq)]
/// enum AppRoute {
///     #[route("/")]
///     Home,
///     #[route("/note/:id")]
///     Note { id: u32 },
/// }
/// ```
///
/// A pattern starts with `/` (`/` alone is the root) and has no empty
/// segment, no trailing `/`, and no `?`, `#` or `%`. A `:name` segment fills
/// the field `name` (`:type` fills `r#type`), which prints through `Display`
/// and parses through `FromStr`; every other segment is literal text, encoded
/// by `RoutePath`. Every field appears as exactly one parameter, except a
/// field marked `#[query]`, which is read from the location's query by its
/// name (`#[query(rename = "k")]` names another key): `Option<T>` is `None`
/// without the key, `Vec<T>` takes every value of the key, and any other
/// `T: Default` is its default without it. `to_path` prints the query
/// fields in declaration order and leaves out `None` and an empty `Vec`.
///
/// Rejected at compile time: a struct, union or generic type; a type without
/// a variant; a tuple
/// variant; `#[route]` on the type or on a field, `#[query]` on the type
/// or on a variant, and either inside a field's type; a variant with no
/// `#[route]`, two, or one with anything
/// after the pattern; a malformed pattern; a parameter that names no field,
/// or a field no parameter names, a parameter that names a `#[query]` field,
/// a malformed, empty or doubled `#[query(...)]`, a key renamed twice, two
/// fields of a variant that read one query key; and two patterns of the same
/// shape,
/// since the second could never be parsed. Everything the type, its variants
/// and their fields get wrong is reported in one build.
///
/// `from_path` tries the patterns from the most specific: a literal segment
/// wins over a parameter in the same place whatever the declaration order,
/// so `/s/new` parses as the `/s/new` variant even beside `/s/:slug`. A path
/// no pattern matches is `RouteParseError::NoMatch`; a path whose pattern
/// matches but whose segment does not parse as its field is
/// `RouteParseError::Param`.
#[proc_macro_derive(Routable, attributes(route, query))]
pub fn derive_routable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive::routable::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Mark the function an application starts from, so that one entry runs on
/// every platform, as Flutter's `main` does.
///
/// ```rust,ignore
/// // src/lib.rs
/// #[flui::main]
/// pub fn run() -> Result<(), flui::LaunchError> {
///     flui::run_app(Counter)
/// }
///
/// // src/main.rs: the desktop, iOS and the web start here
/// fn main() -> Result<(), flui::LaunchError> {
///     counter::run()
/// }
/// ```
///
/// The desktop, iOS and the web start
/// from the binary's `fn main`, which calls it. On Android, whose system
/// loads the library as a `cdylib` and calls an exported `android_main`
/// with its `AndroidApp`, the attribute writes that `android_main`: it hands
/// the `AndroidApp` to the run the function starts and logs the function's
/// `Err`. So an application writes no Android code.
///
/// The function starts by recording the URL schemes and web domains the
/// crate's `[package.metadata.flui.declares]` lists (`url-schemes`,
/// `associated-domains`, a feature's while the crate is compiled with it),
/// read as the crate compiles: the run the function starts opens those, the
/// ones `flui build` registers with the operating system, unless the
/// application gives `Application::declares`.
///
/// The function takes nothing and returns `()` or `Result<(), E>` with a
/// displayable `E`; an `async`, `unsafe`, generic or argument-taking
/// function is a compile error on every target, each such part of the
/// signature reported in one build, and so are arguments on the attribute.
#[proc_macro_attribute]
pub fn main(args: TokenStream, item: TokenStream) -> TokenStream {
    entry::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
