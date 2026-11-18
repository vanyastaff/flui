### Changed

- Every `flui-painting` item has one path. The crate root keeps the recording types
  (`Canvas`, `DisplayList`, `DrawOp`, `ShapedParagraph`, `FontBlob`, …), the layout values
  (`Alignment`, `BoxFit`, `BoxShape`, `FittedSizes`) and the text engine (`TextPainter`,
  `TextSizing`, `FontCollection`, `TextContext`, …); `paint`, `styling`, `typography` and
  `glyphs` hold their own values. The files behind them (`paint::style`, `styling::color`,
  `typography::text_style`, `display_list::command`, `text_painter::measure`, …) are private.
- The paint vocabulary (`Paint`, `PaintBuilder`, `PaintStyle`, `Shader`, `BlendMode`,
  `PointMode`, `StrokeCap`, `StrokeJoin`) is reached only as `flui::painting::paint::*` and
  `flui_painting::paint::*`; the root copies are gone.
- `TextBaseline` moved to `typography` (`flui::painting::typography::TextBaseline`).
- The glyph seam (`GlyphContent`, `GlyphImage`, `GlyphImageError`, `GlyphKey`,
  `GlyphRasterizer`, `PlacedGlyph`) is reached only through `flui_painting::glyphs`.
- `material_shapes` is `flui_painting::paint::material_shapes`.

### Removed

- The second paths: `paint::{Alignment, BoxFit, FittedSizes}`,
  `styling::{BlendMode, BoxFit, ColorFilter, ImageRepeat, TileMode}` (use `paint` or the crate
  root), `styling::{Corners, Radius}` (use `flui_foundation::geometry`), and
  `typography::TextDirection` (use `flui_foundation::TextDirection` or
  `flui::foundation::TextDirection`).
- `TextBaseline::is_alphabetic`, which nothing called.
