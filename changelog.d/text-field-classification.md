### Fixed

- The docs of `TextPainter::set_text`, `Invalidation::Paint`, `InlineSpan::layout_affecting_eq` and `RenderParagraph::set_text` no longer call a span recolour paint-only: a span's colour is carried by its shaped runs, so only a root recolour keeps the layout.

### Changed

- `TextStyle::layout_affecting_eq` and `TextSpan::layout_affecting_eq` name every field, so a field added to either type fails to compile until it is classified as shaping or not.
