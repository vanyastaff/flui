### Changed

- `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext` in every build, and paints the runs of that same layout (ADR-0092 §10 step 4). Carets, selection, line metrics and hit-testing still come from a cosmic-text layout, built on the first such query.
- Measurement and paint run over the app's font collection, which holds the host's faces, generics and fallback order, so named host families (Cupertino's `-apple-system`/`Segoe UI` chain among them), CJK and emoji measure and paint in the host's faces. A context over `FontCollection::new()` (standalone pipelines, the hot-reload plugin) holds only the bundled faces and measures and paints anything else in Roboto. A face registered through `shared_font_system().register_font` changes the caret layout but neither the measured size nor the painted text.
- Hard breaks are Parley's. Migration: a trailing `\n` or U+2029 now adds an empty line (`"A\n"` is two lines tall); U+2028 and `\r` break a line; U+0085 does not.
- `"A\r\nB"` lays out as three lines, with an empty one between `A` and `B`; normalize CR LF to `\n` before handing text to a painter to get two.
- Bold on a face with no bold weight is synthesised at raster time; with `bundled-fonts`, bold text in the default family, in "Roboto" or in a generic paints emboldened instead of as Regular.
- The ellipsis is shaped into the last kept line and measured: text is dropped until the line with the ellipsis fits the width, so a truncated paragraph no longer paints past its measured width, and `dry_size`/`intrinsic_height` measure the ellipsized paragraph.
- `DrawOp::Paragraph` carries `paragraph: Arc<ShapedParagraph>` (runs with their faces, a type that names no shaper) instead of `layout: Arc<TextLayout>`; `Canvas::draw_paragraph` and `WgpuPainter::draw_paragraph` take `Arc<ShapedParagraph>`. Migration: shape through `TextContext::shape(..).to_shaped(color)` or a `TextPainter`.
- The glyph raster types moved to `flui_painting::glyphs`, and `ParleyGlyphKey` is now `GlyphKey`, its old name's cosmic-text key being gone. `ParagraphSpec` has an `ellipsis` field. Migration: `parley_text::{FaceKey, FontRegistry, SwashRasterizer, …}` become `glyphs::{…}`; add `ellipsis: None` to a `ParagraphSpec` literal.
- The engine's glyph atlas rasterizes through swash over the faces each paragraph carries and takes no process-wide font lock.
- The `flui-painting` features `parley` and `parley-layout` are removed. Migration: delete them from `features = [...]`; the Parley path is always compiled.
- With `bundled-fonts`, the process font system installs the bundled Roboto, Material Icons and CupertinoIcons, each in place of any host face of that family, and binds its generic families to Roboto. Text whose style names no family, names "Roboto" or names a generic (`monospace` included) is Roboto on every host, and `monospace` loses its fixed pitch. An app that relied on the host's sans-serif names the family.
- A word wider than the line breaks between its glyphs instead of overflowing the line.
- Letter spacing and line height set on a style without a font size apply at the default 14 px.
- `TextPainter::with_max_lines(Some(0))` / `set_max_lines(Some(0))` mean no limit and read back as `None`; `ParagraphSpec::max_lines` of `Some(0)` keeps every line.
- An empty paragraph's alphabetic baseline is the one a line of text in its style has (13.19 px at 14 px Roboto, was 8.40 px); its height is unchanged.
- Without `bundled-fonts`, the first family registered on an empty `FontCollection` becomes every generic family, so text that names no family measures in it.
- `RenderErrorBox` shapes its debug message at layout through the realm's text context; a new message is a layout change in debug builds.

### Added

- `flui_painting::ShapedParagraph`, with `FontBlob`, `FontFace`, `ShapedRun` and `ShapedGlyph` (`flui_painting::display_list`), `ParagraphLayout::to_shaped`, and `FontRegistry::prepare_run` with its `RunKey`.
- `flui_painting::testing::PROBE_MONO_100`, a generated test face (family "FLUI Probe Mono", `A` one em wide) for consumers' tests.

### Removed

- `flui_painting::testing::measure_with_parley`: every painter measures on Parley.
- `WgpuPainter::draw_text`. Migration: shape the text with a `TextContext` (or a `TextPainter`) and call `draw_paragraph`.
- `TextLayout::placed_glyphs`, `TextLayout::describe_runs`, `SharedFontSystem::rasterize` and `SharedFontSystem`'s `GlyphRasterizer` impl, and the cosmic-text `GlyphKey`: glyphs cross the display list as `ShapedParagraph` runs.
