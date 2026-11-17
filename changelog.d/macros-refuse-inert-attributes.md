### Changed

- A derive attribute that would do nothing is a compile error: `#[inspect(...)]` on a type or
  a variant, one `inspect` key given twice on a field, `#[view(...)]` on an enum's variant
  or field, on a union's field or twice on one field, `#[view(inspect)]` twice, `#[route]` on a
  `Routable` type or on a field, `#[query]` on the type or on a variant, and an empty
  `#[view()]`, `#[inspect()]` or `#[query()]`. The same holds for a helper written as a raw
  identifier (`#[r#view(site)]`), which the derives used to pass by unread.
- `#[derive(Routable)]` reports what the type and each of its variants get wrong in one build,
  and a refusal of an attribute, a field or a type underlines the whole of it, where it used
  to mark its first character.
- `#[derive(TwoWayConverter)]` marks its impls `#[automatically_derived]` and refuses a type
  with a lifetime parameter with its reason, where rustc used to report a constant expression
  it could not evaluate.

### Fixed

- The view derives and `#[flui::main]` name `::std::rc::Rc`, `::core::include_bytes!` and
  `::core::cfg!` by absolute path, so a module or macro of the same name in the deriving
  crate no longer breaks the expansion.
- `#[flui::main]` in a crate that has `flui-sdk` but no application runtime no longer advises
  adding `flui-sdk`.
