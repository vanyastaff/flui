### Added

- **Typed inspection** (`flui-foundation::inspect`, ADR-0242): an object describes itself through
  `Inspect` as a kind and typed fields (`Value`: numbers, text, enum variants, size, offset, rect,
  colour, insets, matrix, duration, links to other trees), each with a detail level, a unit and a
  typed default. A walk allocates only in its sink. `#[derive(Inspect)]` covers named, tuple and
  unit structs and enums, with `skip`, `name`, `default`, `level`, `unit`, `with` and `debug`, and
  works through `flui-sdk`; a field type with no mapping is a compile error that names the fix.
  An `InspectTree` yields an owned `SnapshotNode` that tests read by type (`get::<f64>`,
  `find_by_link`, `find_by_kind().unique()`), a structural `diff` with unit-derived tolerances,
  and one plain or coloured outline (`render`). `flui-protocol` gains the wire snapshot,
  `InspectNode`. It lands beside `Diagnosticable`, which it replaces.

### Changed

- **Geometry prints in one compact form** (`flui-foundation`): `Size` as `360x48`, `Offset` as
  `@8,8`, `Rect` as `8,8 344x32`, and `EdgeInsets` (new) as `8` or `top,right,bottom,left`, each
  number in its shortest round-trip form. They printed `360×48`, `Offset(8, 8)` and
  `Rect(8, 8 - 344×32)` before.
