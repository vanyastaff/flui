# ADR-0310: A text decoration is drawn where its font puts it, as rectangles and paths around the paragraph

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §6 "Underline — a
  declared approximation" and the Consequences line restating it only (the composing underline is
  a decoration over the composed bytes);
  [ADR-0309](ADR-0309-every-text-paint-attribute-crosses-the-display-list.md) Consequences
  "decorations stay out of the vocabulary" only (a style accepts a decoration and paints it)
- **Related:** [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  (painted as measured), [ADR-0066](ADR-0066-display-list-command-representation.md) (the closed
  `DrawOp` vocabulary, which gains nothing), [ADR-0149](ADR-0149-interpolation-contracts.md)
  (`TextStyle` interpolation)

## Context and problem statement

A text style could not be underlined, and the one underline FLUI drew was not where the font puts
it. The scope is `flui-painting`'s text style, layout and painter, and `flui-objects`'
`RenderEditable`.

| Fact | Evidence |
|---|---|
| `TextDecoration` was a bit set no style field held and nothing read | `crates/flui-painting/src/typography/text_decoration.rs:11` at `96684b6e`; ADR-0309 Consequences |
| `RenderEditable` drew the composing region as a 1-logical-pixel bar 1 px under the baseline, whatever the font and size | `crates/flui-objects/src/text/editable.rs:72-78,765` at `96684b6e`; ADR-0030 §6 |
| ADR-0030 §6 declared that bar an approximation, to be deleted once a span decoration exists | `docs/adr/ADR-0030-platform-text-input-ime-capability.md:103-114` |
| Each Parley run carries its font's underline and strikethrough offset and thickness, its ascent and x-height, scaled to pixels | `parley-0.11.1/src/layout/run.rs:224-245` |
| For a font without those tables Parley substitutes `units_per_em / 18` in font units, unscaled, and a positive underline offset | `parley-0.11.1/src/layout/data.rs:418-432` |
| Spans that differ only in paint share a Parley run, so a decoration cannot be attributed by run | `parley-0.11.1/src/resolve/range.rs:139-154`; ADR-0309 §3 |

How does a decoration reach the display list where its font puts it, and how does the editable's
composing underline stop being a separate approximation?

## Decision drivers

- An attribute the API accepts is painted (`AGENTS.md`, "Recurring defects": accepted work
  dropped); a decoration is ordinary design vocabulary (links, prices, spelling marks).
- Rules are types: a decoration's thickness cannot be negative, non-finite or unbounded.
- Painted as measured (ADR-0065): a decoration follows the glyphs the layout placed, without a
  second shaping.
- The common paragraph, with no decoration, records and costs what it did.
- One closed `DrawOp` vocabulary (ADR-0066): a feature the existing ops express adds none.
- A font's own metrics are untrusted input: a missing or corrupt table never publishes non-finite
  or absurd geometry.
- One path for one appearance: the composing underline and a span underline are the same line.

## Considered options

1. Keep refusing decorations: delete `TextDecoration` and keep the composing bar.
2. A decoration field on `DrawOp::Paragraph`, drawn by the engine from the font tables.
3. The painter records each decoration as rectangles and paths from the run's font metrics around
   the paragraph op.
4. The painter records decorations at fixed fractions of the font size.

For the composing underline, given option 3:

- a. Rebuild the span tree with the composing bytes as an underlined span (what Flutter's
  `buildTextSpan` does).
- b. Draw a decoration over a byte range through the same code that draws a span's.

## Decision

Chosen option: 3 with b, because it paints what a style says where the font says, with the ops the
engine already rasterises, and the composing underline becomes the same line a span draws.

§1. **The value.** `TextStyle::decoration: Option<TextDecoration>`: `None` inherits,
`TextDecoration::NONE` clears an inherited one. `TextDecoration` is a `#[non_exhaustive]` `Copy`
struct of `lines: TextDecorationLines` (a bit set of `UNDERLINE`, `OVERLINE`, `LINE_THROUGH`),
`style: TextDecorationStyle` (`#[non_exhaustive]`: `Solid`, `Double`, `Dotted`, `Dashed`, `Wavy`),
`color: Option<Color>` (`None` draws in the glyphs' colour) and `thickness: DecorationThickness`.
`DecorationThickness` is a multiple of the font's thickness, built by `try_new` only when finite,
positive and at most `DecorationThickness::MAX` (16); it deserialises through the same check. A
decoration is paint only: `layout_affecting_eq` ignores it, `SpanPaint::of` reads it, and a
decoration-only `set_text` repaints the measured paragraph. Decorations of the same lines and
style interpolate their colour and thickness; others switch at the midpoint.

§2. **Where a line lies.** A line is placed from the font of the run it decorates
(`DecorationMetrics::of` over Parley's `RunMetrics`): an underline at the font's underline offset
and thickness, a line-through at its strikethrough offset and thickness, an overline at the ascent
with the underline's thickness. A font's offset is trusted within an em of the baseline on its
side, and a thickness in (0, em/4]; otherwise an underline is em/18 thick and em/18 below the
baseline, and a line-through is centred half an x-height (or half an em) above it with the
underline's thickness. A stretch that crosses runs of several sizes takes the metrics of its
largest, so one decoration draws one straight line.

§3. **What a line covers.** `ParagraphLayout::decorated_stretches` walks each kept line's clusters
in visual order and resolves each one's decoration by the byte offset its cluster starts at.
Adjacent clusters with an equal resolved value form one stretch, from the first glyph's left edge
to the last visible glyph's right edge: a line's trailing whitespace and its break are not
decorated (no-break spaces are), and the ellipsis takes the decoration of the span it follows. The
colour resolves as the decoration's own, else its glyphs', else the paragraph's; a transparent
colour or no line draws nothing.

§4. **What is recorded, in order.** `TextPainter::paint` records the backgrounds and shadows of
ADR-0309 §2, then each stretch's underlines and overlines, then the `Paragraph` op, then the
line-throughs, as CSS paints them. `Solid` is a filled rectangle; `Double` two rectangles a
thickness apart; `Dotted` one filled path of round dots a thickness wide every 2.5 thicknesses;
`Dashed` one filled path of dashes four thicknesses long every six; `Wavy` one stroked path of
quadratic half-waves three thicknesses long, a thickness wide. A thicker underline or overline grows
down from the font's top edge, a thicker line-through about its centre. A pattern that would take
more than 4096 steps, or whose thickness is finer than its coordinates can place, draws solid,
and a line with non-finite or empty geometry is not recorded. A paragraph with no decoration
records what it did before.

§5. **A decoration over a byte range.** `TextPainter::paint_with_decorations(canvas, offset,
&[(Range<usize>, TextDecoration)])` draws each range's decoration through §2–§4 as though the
bytes' span were styled with it, beside any decoration the span has; `paint` is the call with no
range. `RenderEditable` passes its composing range with `TextDecoration::UNDERLINE`, so composition
changes repaint without relayout, and its own underline geometry, constants and colour rule are
deleted.

### Consequences

- Good, because underlines, overlines and line-throughs paint as a style says, in five styles,
  where the font puts them, and interpolate.
- Good, because the composing underline is the font's underline at every size, and a span
  underline and a composing one under the same bytes are the same op.
- Good, because the engine, the raster contract and the damage model are unchanged: the lines are
  rectangles and paths.
- Good, because a font's metrics cannot publish a non-finite, inverted or absurd line, and a tiny
  font over a wide line cannot record an unbounded path.
- Bad, because a decorated paragraph walks its kept clusters on each paint; a paragraph without a
  decoration does not.
- Bad, because decorations are drawn over the glyphs' ink: an underline does not skip descenders
  (CSS `text-decoration-skip-ink`), which needs glyph outlines in the painter.
- Neutral, because the `TextDecoration` bit set and its `combine`/`has_*` methods go; the value is
  new and had no consumer.

### Confirmation

- `flui-painting` table `text_decorations_contract`: `an_underline_lies_where_the_font_puts_it`,
  `a_line_through_is_drawn_over_the_glyphs`, `each_style_draws_its_shape`,
  `trailing_whitespace_and_breaks_are_not_decorated`, `an_ellipsis_takes_its_spans_decoration`,
  `a_decoration_only_change_repaints_without_relayout`,
  `a_decoration_takes_its_glyphs_colour_unless_it_names_one`,
  `a_child_clears_an_inherited_decoration`, `adjacent_spans_with_one_decoration_draw_one_line`,
  `a_range_decoration_is_the_spans_decoration`, `a_thickness_scales_the_fonts`,
  `a_decoration_thickness_is_bounded`, `decorations_interpolate_their_colour_and_thickness`,
  `a_pattern_far_from_the_origin_is_drawn_in_bounded_steps`.
- `flui-painting` unit test `decoration_metrics_trust_the_font_only_in_range` (substituted and
  non-finite metrics fall back).
- `flui-painting` row `every_style_and_span_field_is_classified_by_what_the_shaped_paragraph_holds`
  classifies `decoration` as paint.
- `flui-objects` harness row `editable_composing_underline_is_the_span_underline_of_its_bytes` (a
  multibyte composing range at 40 px).
- The compiler: `TextDecoration` and `TextDecorationStyle` are `#[non_exhaustive]`, and
  `DecorationThickness` has no public constructor but `try_new`.

## Pros and cons of the options

### 1. Keep refusing decorations

- Good, because it is the smallest change.
- Bad, because applications rebuild underlines from boxes, at guessed offsets, as the editable did.

### 2. A decoration field on the paragraph op

- Good, because the engine has the font tables at raster time.
- Bad, because every paragraph reader classifies a field it does not draw, the engine grows a
  second text feature, and damage must learn the lines' extent.

### 3. Rectangles and paths from the run's font metrics (chosen)

- Good, because the ops exist, and Parley already reports the metrics per run.
- Bad, because the painter sanitises metrics Parley should have scaled.

### 4. Fixed fractions of the font size

- Good, because no font metric is read.
- Bad, because the line is not where the font puts it, which is the defect this replaces.

### a. Rebuild the span tree for the composing range

- Good, because the composing underline is literally a span decoration.
- Bad, because every composition change rebuilds the spans, splits the user's styles at the
  range's edges and lays the text out again.

### b. A decoration over a byte range (chosen)

- Good, because composition changes repaint only, and the line is drawn by the code that draws a
  span's.
- Bad, because the painter's API gains a second paint entry point.

## More information

- CSS Text Decoration Module Level 3, "Line Decoration" and the painting order of CSS 2.1
  Appendix E (underline and overline beneath the text, line-through above it):
  <https://www.w3.org/TR/css-text-decor-3/#line-decoration>, <https://www.w3.org/TR/CSS21/zindex.html>.
- OpenType `post.underlinePosition`/`underlineThickness` and `OS/2.yStrikeoutPosition`/
  `yStrikeoutSize`: <https://learn.microsoft.com/en-us/typography/opentype/spec/post>,
  <https://learn.microsoft.com/en-us/typography/opentype/spec/os2>.
- HarfBuzz's em/18 default, which Parley copies unscaled:
  [harfbuzz/harfbuzz@00492ec7df
  src/hb-ot-metrics.cc:334-347](https://github.com/harfbuzz/harfbuzz/blob/00492ec7df0038f41f78d43d477c183e4e4c506e/src/hb-ot-metrics.cc#L334-L347).

Revisit §2's fallback when Parley scales its substituted metrics, and §4 when skip-ink is wanted.
