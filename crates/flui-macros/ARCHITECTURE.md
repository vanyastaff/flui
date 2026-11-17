# FLUI macro architecture

The proc-macro crate owns derive expansion. Runtime traits remain in their
owning crates; generated code resolves them from the consuming manifest.

## Mapping decisions

### Compose fixed-width animation fields

`TwoWayConverter` derives flatten each field's associated `AnimationVector`
in declaration order. `AnimationVector::COMPONENTS` is the width of its sealed
scalar array; no type names, memory layout assumptions or heap buffers determine
the representation. Derived `Lerp` delegates to each field, preserving geometry
extrapolation and premultiplied color interpolation, and returns exact authored
endpoints. The shared scalar `Lerp` handles representable extreme differences.
The derive also concatenates `rest_thresholds` in that same field order, retaining
each representation's units ([ADR-0186](../../docs/adr/ADR-0186-typed-animation-rest-thresholds.md)).
Thresholds are not converted through `to_vector`: a nonlinear value converter
need not preserve distances. The nested spring case in
`two_way_converter_derive_contract` observes nested geometry retaining its own
completion timing rather than adopting the scalar field's threshold.

Empty structs are refused because they have no motion components. A field that
references a type or const parameter is refused at its type: stable Rust
cannot sum generic-dependent associated widths into an array length. A type with
a lifetime parameter is refused at the parameter, since an array length names no
lifetime. Unused const parameters with concrete fields remain supported. A manual
implementation can choose a concrete vector representation when a generic author
needs one.

`two_way_converter_derive_contract` in `flui-animation` tests named, tuple and
nested fields, color behavior and actual registered retargeting.
`trybuild_ui::ui_tests` in `flui-sdk` tests SDK expansion, aliases, concrete const
generics, empty structs, missing field traits and generic-dependent widths.
`external_consumers_extend_and_test_through_the_facade` runs a mounted custom
painter in external projects with both ordinary and renamed facade dependencies.
Its `nested_custom_motion_repaints_and_retargets_through_the_facade` case reads
the submitted rectangle's geometry and color, preserves its velocity at a
retarget seam, and asserts that the card does not rebuild on moving frames.

### Resolve runtime paths through Cargo dependencies

All the derives use `proc-macro-crate` to resolve names in the consumer's
manifest, in this order:

1. `flui-sdk`, giving `::flui_sdk::{view,foundation,animation,widgets}`: a
   package builds on the SDK alone (ADR-0088 §4).
2. The owning crate (`flui-view`, `flui-foundation`, `flui-animation`,
   `flui-widgets` for `Routable`), so framework crates and advanced consumers
   keep their explicit owner/version selection.
3. The `flui` facade, whose runtime modules serve an application.

Every step honors Cargo renames without per-derive configuration attributes.
`#[flui::main]` skips the first step and looks up `flui-app`, then the facade:
a package has no entry function, so the SDK does not re-export the application
runtime.

The SDK comes first because `proc-macro-crate` reads `[dependencies]` and
`[dev-dependencies]` alike: a package with the facade or an internal crate as a
dev-dependency (for its tests or examples) would otherwise expand to a crate its
library build does not link. The one shape this order cannot serve is an owner
as a normal dependency beside `flui-sdk` as a dev-dependency only; no crate has
it, and such a crate should name the owner through the SDK instead.

When Cargo reports the consuming package itself, expansions use its absolute
crate name. Runtime libraries provide a private `extern crate self` alias;
the same generated path also works in the package's separate integration-test
targets. Using `crate::` would refer to those test crates instead of the library.

The resolver follows [proc-macro-crate 3.5.0](https://docs.rs/proc-macro-crate/3.5.0/proc_macro_crate/)
and [Cargo dependency renaming](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml).
Owner unit tests cover expansion inside libraries; the consumer integration test
`external_consumers_extend_and_test_through_the_facade` (`tests/facade_consumer.rs` at the root)
covers facade-only and renamed-facade manifests. SDK-only manifests and the SDK beside a facade
dev-dependency are **Unasserted:** no test pins this.

### An attribute that would do nothing is a compile error

A derive registers its helper attribute (`view`, `inspect`, `route`, `query`)
for the whole item, so rustc accepts it anywhere inside. A derive reads the
attributes of three places: the type, its variants and their fields. Reading
its input into a typed description, before it looks up a runtime path, it
refuses there each attribute it could not act on:

- one in a place that takes none: `#[view(...)]` on an enum's variant or field
  or a union's field, `#[inspect(...)]` on a type or a variant, `#[route]` on
  the type or a field, `#[query]` on the type or a variant;
- one repeated where a single one takes effect: `#[view(inspect)]` twice, `key`
  or `site` twice on a field or on two fields, one `inspect` key twice on a
  field, a second `#[route]` on a variant or `#[query]` on a field, `rename`
  twice;
- a list without an entry: `#[view()]`, `#[inspect()]`, `#[query()]`.

Everywhere else in the item that syn parses (a generic parameter, a parameter
of a function pointer in a field's type, an expression or an item inside a
discriminant) `derive::attribute::refuse_unread` refuses the helper: no derive
reads it there, and the generated `impl` would repeat a generic parameter's
attribute outside the helper's scope
(`a_helper_outside_the_type_its_variants_and_their_fields_is_refused`). The
tokens of a macro call inside the item are not parsed, so a helper written
among them is not seen.

Accepting any of these would leave the author with a view that reports no
creation site, a field that reports under a name they did not expect, or a
route whose query never parses, and no diagnostic.

rustc resolves a helper written as a raw identifier (`#[r#view(site)]`) to the
same helper. The derives therefore compare every name without its `r#`: a
helper and an entry of its list through `derive::attribute::is_named`, an
identifier taken as a value (`level = r#debug`), and a generic parameter named
in a field's type (`generics::mentions_param`). A comparison of the identifier
as written would pass a raw helper by every refusal above and refuse a raw
entry the author meant (`an_entry_reads_the_same_spelled_raw`,
`an_entry_and_its_value_read_the_same_spelled_raw`,
`a_query_field_reads_its_own_name_or_the_one_it_is_renamed_to`).

### A macro reports every refusal of its input at once

An author shown one refusal a build fixes it, builds again and meets the next.
Each derive, and `#[flui::main]` for the signature of its function, records
what it refuses in `refusals::Refusals`. The only way to a record is
`Refusals::collect`, which runs the reading and answers with its value or with
every refusal recorded, so a refusal that was recorded cannot be left
unreported (`a_collection_is_its_value_or_every_refusal_in_the_order_found`).

Two things end a reading early. A shape the macro does not take (a union, a
struct where `Routable` takes an enum, a tuple struct where `InheritedData`
takes named fields) leaves no variants or fields to read; what was recorded
before it is reported with it. Inside one attribute's list, an entry the derive
does not know ends that list, since the tokens after it may be its value; the
item's other lists are still read. `Routable` reads on past a variant it
refuses: a variant whose pattern reads is compared with the patterns after it
whatever its fields got wrong, and a field whose `#[query]` is refused still
counts as a query field, so it is not also reported as missing from the
pattern.

One test per macro counts the refusals of an input that earns several:
`every_refusal_of_a_views_attributes_is_reported_in_one_build`,
`every_refusal_of_a_described_type_is_reported_in_one_build`,
`every_refusal_of_a_route_type_is_reported_in_one_build`,
`every_refusal_of_a_converted_type_is_reported_in_one_build`,
`every_refusal_of_a_providers_data_is_reported_in_one_build`,
`every_refusal_of_an_entry_signature_is_reported_in_one_build`.

Because the refusals precede the manifest lookup they are unit tests of `expand`
in this crate (`a_misplaced_view_attribute_is_refused_with_its_place`,
`an_inspect_attribute_that_would_do_nothing_is_refused`,
`a_route_or_query_attribute_out_of_place_is_refused`); the trybuild cases
`view_attribute_placement` in `flui-view`, `inspect_unsupported_shape` in
`flui-sdk`, `misplaced_attributes` and `generic_enum` in `flui-widgets` and
`main_attribute_refuses_generics` at the root pin what the author sees: the
message and what it underlines. An attribute out of place or empty is
underlined whole, and an item that is generic at its parameter list or, with a
`where` clause alone, at the keyword (`generics::declared`): a clause may have
no predicate, and then prints as nothing a span could cover.

### A name the expansion declares resolves at the macro

An expansion declares parameters, locals and pattern bindings beside tokens
its author wrote: `#[inspect(default = EXPR)]` and `#[inspect(with = path)]`
paste an expression into the generated `inspect`. Every such name comes from
`hygiene::local` (a block's label from `hygiene::label`), spanned
`Span::mixed_site()`, the hygiene of a `macro_rules!` local: the author's
expression cannot name the derive's `out`, and a function of the author's
called `out` is what `with = out` means
(`an_attributes_expression_names_the_authors_items_not_the_derives_locals` in
`flui-foundation`). The names stay plain, so rustdoc shows
`fn inspect(&self, out: &mut Fields<'_>)` on a derived impl. The limit is the
one `macro_rules!` has: an item is not a local, so a constant or a unit struct
of the author's with a local's lower-case name still meets it in a pattern.

Such a name is a different span from the author's tokens around it, and rustc
reports one failure once per span. Code spanned at a field's type therefore
names what depends on that type through the type alone, never through a local
of the expansion: `TwoWayConverter` writes each field's part of the vector as
one expression over `<FieldType as TwoWayConverter>`, so a field that is not a
converter is reported once, at the field (`non_motion_field` in `flui-sdk`).
The field of an enum's variant has to be read through a binding the derive
declares, so `Inspect` reports its unmapped type at the derive and labels the
field (`inspect_unmapped_field`).

### A view's key is a field

`#[view(key)]` writes `View::key` from the marked field through
`flui_foundation::key::AsViewKey`, so the field keeps the type the view needs:
a key, an `Option` of one, an `Rc<dyn ViewKey>` or a `Box<dyn ViewKey>`. The
derive names no key type. A field that holds no key fails at the field's type
with the trait's `on_unimplemented` note (trybuild `view_key_field_is_not_a_key`
in `flui-view`), and a field type that names one of the view's parameters adds
`FieldType: AsViewKey` to the impl, so the view is a `View` for the parameters
that make the field a key. A view with a key therefore derives like any other
(`a_derived_view_reports_the_key_its_marked_field_holds`); an `impl View`
written by hand is for a view that mounts its own element or overrides
`can_update` or `should_skip_rebuild`.

### Inspect derive: the field type is the mapping

`#[derive(Inspect)]` (ADR-0242 §4) writes one `Fields` call per field and leaves the value
mapping to the runtime's `FieldValue`/`ToValue` traits instead of matching type names, so a
type alias, a re-export or a user's own `ToValue` impl all work, and an unmapped type fails at
the derive through the trait's `#[diagnostic::on_unimplemented]` hint (`skip`, `with`, `debug`,
or an impl). Where-bounds are added only for fields that mention a type parameter; a concrete
field is checked at its call. The kind is the identifier (`r#` stripped), with no
`type_name` scanning; a generic type adds `kind_args`, one `core::any::type_name` per type
parameter. A fieldless enum also gets `ToValue` (a `Value::Variant`), which is what lets it be
another type's field. A union is refused: which field is live is unknown. The diagnostics are
pinned by `inspect_unmapped_field` and `inspect_unsupported_shape` in
`crates/flui-sdk/tests/compiler_guards.rs`; behaviour by `the_derive_describes_every_shape_with_its_attributes`
in `flui-foundation`.

### Routable derive: specificity order, compile-time pattern validation, hidden helpers

**Oracle:** The shape follows Dioxus's `#[derive(Routable)]` with `#[route("…")]`
per variant.

**Choice:** `#[derive(Routable)]` (ADR-0330 §1) checks every pattern while
expanding: a leading `/`, no empty segment, trailing `/`, `?`, `#` or `%`;
each `:param` names one field and each field one parameter; no two patterns of
the same shape; no two fields of a variant that read one query key. A tuple
variant, a generic enum, a struct and a missing, doubled
or over-long `#[route]` are errors with the span on the literal or item at
fault. `from_path` tries patterns by specificity, not declaration order:
shorter first, then segment by segment with a literal above a parameter, so
`/s/new` wins over `/s/:slug`. It decodes the path's segments once, matches a
slice pattern with literal equality per candidate, and parses fields with
`FromStr`; the first field that fails in a candidate whose literals matched is
the `Param` error when nothing matches in full, otherwise `NoMatch`. The
generated code calls only `flui_widgets::router::__derive` (`segments`,
`Matcher`, `assert_segment`), a `#[doc(hidden)]` module with no semver
promise, so `RouteParseError` stays `#[non_exhaustive]`; `assert_segment` is
spanned at each field type, so a missing `FromStr` or `Display` is reported on
the field. **Tests:** the parser, ordering and shape rules in
`src/derive/routable/pattern.rs`; `crates/flui-widgets/tests/routable_ui.rs`
(trybuild pass and fail cases) and `crates/flui-widgets/tests/routable_derive.rs`
(the proptest round trip).
