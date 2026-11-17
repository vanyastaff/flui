### Changed

- A derive attribute that would do nothing is a compile error: `#[inspect(...)]` on a type or
  a variant, one `inspect` key given twice on a field, and `#[view(...)]` on an enum's variant
  or field, or a union's field. `#[derive(TwoWayConverter)]` marks its impls
  `#[automatically_derived]`.

### Fixed

- The view derives and `#[flui::main]` name `::std::rc::Rc`, `::core::include_bytes!` and
  `::core::cfg!` by absolute path, so a module or macro of the same name in the deriving
  crate no longer breaks the expansion.
