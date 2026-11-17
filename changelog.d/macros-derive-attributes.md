### Added

- `#[view(key)]` on a field of a `#[derive(StatelessView)]` or `#[derive(StatefulView)]` type
  makes that field the view's key, so a keyed view keeps its derive. The field is a `ViewKey`,
  an `Option` of one, or an `Rc<dyn ViewKey>` or `Box<dyn ViewKey>`
  (`flui_foundation::key::AsViewKey`).

### Changed

- The `StatelessView`, `StatefulView` and `InheritedData` derives are exported from `flui_view`
  beside the traits of the same names, not only from its prelude: `use flui_view::StatelessView`
  brings the trait and the derive together.
- A derive attribute that would do nothing is a compile error. That is one in a place that
  takes none: `#[inspect(...)]` on a type or a variant, `#[view(...)]` on an enum's variant or
  field or on a union's field, `#[route]` on a `Routable` type or on a field, `#[query]` on the
  type or on a variant, and any of them on a generic parameter, inside a field's type or inside
  a discriminant. It is one repeated where a single one takes effect: an `inspect` key twice on
  a field, `#[view(inspect)]` twice, `key` or `site` twice on a field, `rename` twice in a
  `#[query]`. And it is a list without an entry: `#[view()]`, `#[inspect()]`, `#[query()]`.
- A helper attribute, an entry of its list and an identifier it takes as a value mean the same
  written as a raw identifier: `#[r#inspect(r#skip)]` skips its field and `level = r#debug` is
  the debug level. The derives used to pass a raw helper by unread and to refuse a raw value.
- `#[derive(Routable)]` refuses two fields of a variant that read the same query key.
- Every derive, and `#[flui::main]` for the signature of its function, reports all it refuses
  in one build: the type's, each variant's and each field's, and every refused entry of one
  attribute's list. Each used to stop at the first. A
  refusal of an attribute out of place or empty underlines the whole attribute, and a refusal
  of a field or of a type the whole of it, where each used to mark its first character.
- `#[derive(TwoWayConverter)]` marks its impls `#[automatically_derived]` and refuses a type
  with a lifetime parameter with its reason, where rustc used to report a constant expression
  it could not evaluate.

### Removed

- `flui_foundation::Keyed` and `WithKey` (`with_view_key`, `with_value_key`,
  `with_unique_key`). The wrapper they made was not a view, so the key it carried was read by
  nothing; a view holds its key in a field marked `#[view(key)]`.

### Fixed

- An expression in `#[inspect(default = …)]` or `#[inspect(with = …)]` that names a function
  called like one of the derive's own locals (`out`) means that function; it used to name the
  derive's local. The parameters and locals every expansion declares resolve at the macro.
- `#[derive(Inspect)]` adds its bound, and `#[derive(TwoWayConverter)]` makes its refusal, for
  a field whose type names a parameter as a raw identifier (`r#T`).
- The view derives and `#[flui::main]` name `::std::rc::Rc`, `::core::include_bytes!` and
  `::core::cfg!` by absolute path, so a module or macro of the same name in the deriving
  crate no longer breaks the expansion.
- `#[flui::main]` on a function named `app` builds for Android: the parameter of the generated
  `android_main` shadowed it.
- `#[derive(InheritedData)]` reports a field that is not `PartialEq` at the field, where it
  pointed at the derive.
- `#[flui::main]` in a crate that has `flui-sdk` but no application runtime no longer advises
  adding `flui-sdk`.
