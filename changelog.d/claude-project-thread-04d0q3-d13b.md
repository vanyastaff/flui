### Added

- **`flui-painting`**: text decorations. `TextStyle::decoration` takes a `TextDecoration`, which
  holds its lines (`TextDecorationLines`: underline, overline, line-through), a style (solid,
  double, dotted, dashed, wavy), an optional colour and a `DecorationThickness`, a multiple of the
  font's own thickness. Lines are drawn where the run's font puts them, underlines and overlines
  under the glyphs and line-throughs over them. Trailing spaces are not decorated, and an ellipsis
  takes its span's decoration. `TextDecoration::NONE` clears an inherited decoration
  ([ADR-0310](/docs/adr/ADR-0310-a-text-decoration-is-drawn-where-its-font-puts-it.md)).
- **`flui-painting`**: `TextPainter::paint_with_decorations` draws a decoration over byte ranges
  the way a span draws it.
- **`flui-sdk`**: `painting::{TextDecoration, TextDecorationLines, TextDecorationStyle,
  DecorationThickness}`.

### Fixed

- **`flui-objects`**: `RenderEditable` underlines the composing region with the font's underline,
  at its offset and thickness, in place of a one-pixel bar one pixel under the baseline.

### Removed

- **`flui-painting`**: the old `TextDecoration` bit set, its `combine` and `has_*` methods. Nothing
  read it. Use `TextDecorationLines` and `TextDecoration::new`.
