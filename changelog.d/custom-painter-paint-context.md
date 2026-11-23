### Changed

- **`flui-rendering`** (breaking): `CustomPainter::paint` takes a `CustomPaintContext`
  (`fn paint(&self, ctx: &mut CustomPaintContext<'_>)`) in place of `(canvas: &mut Canvas, size: Size)`.
  The context gives the canvas (`ctx.canvas()`), the size (`ctx.size()`) and, on demand, the UI
  runtime's text context (`ctx.text()`), so a painter lays out a `TextPainter` for a chart label
  and paints it; a painter that asks is repainted when a font is registered. A painter body moves
  over with `let size = ctx.size(); let canvas = ctx.canvas();`, and a test that records a
  painter on its own canvas builds `CustomPaintContext::without_text(&mut canvas, size)`.
  `CustomPaintContext` is exported beside `CustomPainter` (`flui::painting`, the widgets prelude).
- **`flui-rendering`** (breaking): `PaintContext::new` and `RenderObject::paint_raw` take the node's
  `TextSource`, and `PaintContext::text()` lends the text context to a render object that shapes
  in paint. `RenderError` gains `TextContextUnavailable`.
