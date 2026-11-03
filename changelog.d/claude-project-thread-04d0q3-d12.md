### Changed

- `TextPainter::set_text` no longer shapes a paragraph again when only a span's colour changes: a span recolour rebuilds the painted paragraph from the cached layout and reports `Invalidation::Paint`, so a `RenderParagraph` or `RenderEditable` repaints without relayout. A recolour that gives two adjacent spans of one colour different colours still reports `Invalidation::Layout`.
- `ShapedParagraph::describe_spans` prints the colour a span paints with, its paragraph default included, when that differs from the paragraph command's colour.
