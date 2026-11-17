### Changed

- A derive attribute that would do nothing is a compile error: `#[inspect(...)]` on a type or
  a variant, one `inspect` key given twice on a field, `#[view(...)]` on an enum's variant
  or field, on a union's field or twice on one field, `#[route]` on a `Routable` type or on a
  field, and `#[query]` on the type or on a variant. `#[derive(TwoWayConverter)]` marks its
  impls `#[automatically_derived]` and refuses a type with a lifetime parameter with its
  reason, where rustc used to report a constant expression it could not evaluate.

### Fixed

- The view derives and `#[flui::main]` name `::std::rc::Rc`, `::core::include_bytes!` and
  `::core::cfg!` by absolute path, so a module or macro of the same name in the deriving
  crate no longer breaks the expansion.
- `#[flui::main]` in a crate that has `flui-sdk` but no application runtime no longer advises
  adding `flui-sdk`.
