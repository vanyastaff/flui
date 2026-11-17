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
for the whole item, so rustc accepts it anywhere inside: on the type, a
variant or a field. Each derive reads its input into a typed description
before it looks up a runtime path, and that reading refuses every place, every
repetition and every empty list it could not act on: `#[view(site)]` on an
enum's field or twice on one field, `#[view(inspect)]` twice, `#[view()]`,
`#[inspect(...)]` on a type or a variant, one `inspect` key twice on a field,
`#[route]` on the type or a field, `#[query]` on the type or a variant.
Accepting them would leave the author with a view that reports no creation
site, a field that reports under a name they did not expect, or a route whose
query never parses, and no diagnostic.

rustc resolves a helper written as a raw identifier (`#[r#view(site)]`) to the
same helper, so the derives compare names through `attribute::is_named`, which
drops the `r#`; a comparison of the identifier as written would pass that
spelling by every refusal above. `Routable` collects what it refuses in
`Refusals` and reports the type's and every variant's together, so an author
does not fix one, build again and meet the next.

Because the refusals precede the manifest lookup they are unit tests of `expand`
in this crate (`a_misplaced_view_attribute_is_refused_with_its_place`,
`an_inspect_attribute_that_would_do_nothing_is_refused`,
`a_route_or_query_attribute_out_of_place_is_refused`,
`every_refusal_of_a_route_type_is_reported_in_one_build`); the trybuild cases
`view_attribute_placement` in `flui-view`, `inspect_unsupported_shape` in
`flui-sdk` and `misplaced_attributes` in `flui-widgets` pin what the author sees:
the message and the whole attribute underlined.

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
the same shape. A tuple variant, a generic enum, a struct and a missing, doubled
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
the field. **Tests:** the parser, ordering and conflict rules in
`src/derive_routable/pattern.rs`; `crates/flui-widgets/tests/routable_ui.rs`
(trybuild pass and fail cases) and `crates/flui-widgets/tests/routable_derive.rs`
(the proptest round trip).
