# ADR-0309: Every text paint attribute a style accepts crosses the display list

- **Status:** Accepted (2026-11-03)
- **Date:** 2026-11-03
- **Supersedes:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §4 "with span colour and
  decoration as today" only (a run carries a colour and nothing else);
  [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md) Part 2 "one
  `Paragraph` op on the wire" only (a paragraph also crosses as the shadows it casts)
- **Related:** [ADR-0066](ADR-0066-display-list-command-representation.md) (the closed `DrawOp`
  vocabulary), [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md) (the raster contract a new op
  joins), [ADR-0149](ADR-0149-interpolation-contracts.md) (`TextStyle` interpolation),
  [ADR-0306](ADR-0306-display-list-commands-hold-local-geometry-under-interned-transforms.md) (the
  command representation the new op follows)

## Context and problem statement

`TextStyle` took paint attributes it never painted. The scope is `flui-painting`'s text style,
painter and display list, and the engine's replay of a paragraph.

| Fact | Evidence |
|---|---|
| `TextStyle` carries `background_color`, `foreground`, `background` and `shadows` beside `color` | `crates/flui-painting/src/typography/text_style.rs:229-264` |
| `TextPainter::paint` reads only the root's glyph colour and records one `Paragraph` op | `crates/flui-painting/src/text_painter/paint.rs:119-147`, `crates/flui-painting/src/text_layout/mod.rs:33-37` |
| The op holds a paragraph, an offset and a root colour; a run holds a colour and nothing else | `crates/flui-painting/src/display_list/command.rs:165-172`, `crates/flui-painting/src/display_list/paragraph.rs:103-111` |
| The engine colours glyphs and draws nothing else for a paragraph | `crates/flui-engine/src/layer_dispatcher.rs` (`render_paragraph`), `crates/flui-engine/src/batches/text.rs` |
| `IconThemeData::shadows` is copied into the glyph's style, and dropped with it | `crates/flui-widgets/src/icon/icon.rs:161`, `crates/flui-objects/src/text/icon.rs` (`paint` calls `TextPainter::paint`) |
| `IconThemeData::opacity` is accepted and documented as deferred | `crates/flui-widgets/src/icon/icon_theme_data.rs:36-45`, `crates/flui-widgets/src/icon/icon.rs:44` |
| `set_text` classifies a shadow-only change as a repaint, and `AnimatedDefaultTextStyle` promises interpolated shadows; both repaint the same op | `crates/flui-painting/src/text_painter/mod.rs` (`set_text`), `crates/flui-widgets/src/text/animated_default_text_style.rs:14` |
| `foreground` and `background` are `Option<Color>` copies of fields that are paints elsewhere, so each duplicates `color` and `background_color` | `text_style.rs:258-262`, `crates/flui-painting/src/text_layout/mod.rs:33-37` |
| An empty `shadows` inherits the parent's, so a span cannot clear an inherited shadow | `text_style.rs:438-442` |
| `TextShadow` duplicates `styling::Shadow<f64>` with split offset fields | `crates/flui-painting/src/typography/text_style.rs`, `crates/flui-painting/src/styling/shadow.rs:22-35` |
| ADR-0092 §4 says a run carries "decoration as today"; no style field takes a decoration | `docs/adr/ADR-0092-per-realm-text-over-parley.md:340-343`; `rg TextDecoration` finds no consumer |
| A blur exists only as a layer image filter, whose sigma is validated into a sticky recording error that blanks the segment | `crates/flui-engine/src/painter/layer.rs` (`save_layer_with_image_filter`), `crates/flui-engine/src/command_ir.rs` (`support_radius`) |
| Parley merges adjacent spans whose shaping styles are equal, so a span that differs from its neighbour only in a shadow shares its glyph run | `parley-0.11.1/src/resolve/range.rs:139-154` |

A shadow, a span background or an icon opacity set by an application did nothing, without a
warning. How does a paint attribute reach the screen, and how does the next one not get dropped
the same way?

## Decision drivers

- An attribute the API accepts is painted, or the API does not accept it (`AGENTS.md`, "Recurring
  defects": accepted work dropped).
- Rules are types: a new paint field fails to compile until something paints it.
- Painted as measured (ADR-0065): a shadow is the measured glyphs, not a second shaping.
- A style value never fails or stalls a frame: no sticky recording error, no unbounded blur.
- The common paragraph, with no background and no shadow, records and costs what it did.
- One closed `DrawOp` vocabulary the engine matches without a wildcard (ADR-0066).

## Considered options

1. Remove the unpainted attributes (`shadows`, `background_color`, `foreground`, `background`,
   `IconThemeData::{shadows, opacity}`).
2. Paint them, with shadows as an `ink` field on `DrawOp::Paragraph`.
3. Paint them, with shadows as a new `DrawOp::ParagraphShadow` op and backgrounds as rectangles.
4. Paint them, with shadows as a general `SaveLayer` image-filter op around a recolored paragraph.

## Decision

Chosen option: 3, because it paints what applications already write, and every site that matches
a paragraph op has to classify a shadow before it compiles.

§1. **One paint vocabulary.** A `TextStyle` paints with `color` (the glyphs), `background_color`
(the span's line boxes) and `shadows: Option<Vec<Shadow<f64>>>` (`None` inherits, an empty list
clears). `foreground`, `background` and `TextShadow` are removed. The paint fields are read in one
exhaustive destructure, `SpanPaint::of`, the twin of `TextStyle::layout_affecting_eq`: a new field
does not compile until both classify it.

§2. **What a paragraph paints, in order.** `TextPainter::paint` records, for the current span tree
over the measured layout:

1. a filled rectangle per line box of each span with a background, the ellipsis included under the
   span whose style it takes;
2. for each distinct shadow list, in span order, one `ParagraphShadow` op per shadow in list order,
   carrying the glyphs of exactly the spans that cast that list;
3. the `Paragraph` op, as before.

A paragraph with no background and no shadow records one `Paragraph` op with no allocation. The
selection a render object paints before the paragraph stays beneath it.

§3. **The shadow op.** `DrawOp::ParagraphShadow { paragraph, offset, color, sigma }`:

- `paragraph` is built from the measured layout without shaping: the glyphs whose cluster starts in
  a casting span, every run colour cleared. Glyphs are attributed by byte offset, never by Parley
  run or style, since spans that differ only in paint share a run.
- every glyph's coverage is inked in `color`, a colour glyph (emoji) included: a shadow is a
  silhouette, as CSS draws it.
- the coverage is blurred by a Gaussian of `sigma` logical pixels; zero is sharp.
- its damage is the paragraph's box and ink at `offset`, grown by `3σ`.

§4. **Blur bounds.** `BlurSigma` is a finite, non-negative sigma in logical pixels, at most
`BlurSigma::MAX` (128). `from_blur_radius` converts a shadow's blur radius as Flutter does
(`r · 0.57735 + 0.5` for `r > 0`, else 0), clamps a finite radius past the maximum and refuses a
non-finite one; a refused shadow, or one whose offset sum is not finite, is not recorded. The
engine converts sigma to device pixels with the transform's largest scale, clamps it to the
renderer's maximum, draws it sharp when its square is not a normal float, and never turns it into
a recording error.

§5. **Icon opacity** multiplies the icon colour's alpha.

### Consequences

- Good, because shadows, span backgrounds and icon opacity paint as documented, and their
  interpolation and paint-only invalidation now show.
- Good, because a missed match site fails to compile: the sites that read every `Paragraph` op's
  colour as the text colour keep their meaning.
- Good, because a blurred shadow uses the engine's existing blur layer, so the raster contract
  gains one op and no new filter.
- Bad, because `foreground`, `background` and `TextShadow` go: callers move to `color`,
  `background_color` and `Shadow<f64>`.
- Bad, because a blurred shadow costs a layer per shadow per paragraph; text shadows are rare and
  short, and a cost model is the scene's (ADR-0304), not this op's.
- Neutral, because decorations stay out of the vocabulary: no style field accepts one, so nothing
  is dropped, and painting them is a feature of its own.

### Confirmation

- `flui-painting` rows `span_shadows_and_backgrounds_reach_the_display_list` (a span differing only
  in paint, a coloured span, the shadowed span first),
  `an_ellipsis_takes_its_spans_shadow_and_background`,
  `a_shadow_only_change_repaints_without_relayout`,
  `refused_shadows_are_dropped_and_the_rest_painted`,
  `blur_sigma_converts_and_bounds_a_blur_radius` and the damage row
  `a_text_shadow_extent_covers_its_blur`.
- `flui-widgets` row `an_icon_theme_shadow_and_opacity_paint` (contracts table).
- `flui-engine` readback rows `a_blurred_text_shadow_inks_outside_the_silhouette` at device pixel
  ratios 1 and 2, `a_huge_text_shadow_blur_does_not_fail_the_frame` (a window far larger than the
  text included) and `a_colour_emoji_casts_a_silhouette`.

## Pros and cons of the options

### 1. Remove the attributes

- Good, because it is small and closes the defect by absence.
- Bad, because shadows on text and icons are ordinary design vocabulary, and applications would
  rebuild them from stacked text.

### 2. An `ink` field on the paragraph op

- Good, because one op covers text and shadow.
- Bad, because every `DrawOp::Paragraph { .. }` pattern keeps compiling and reads a shadow as text
  (damage, bounds, and the package tests that take each paragraph's colour as the label's).

### 3. A `ParagraphShadow` op and background rectangles (chosen)

- Good, because the compiler lists every site that must decide what a shadow is.
- Good, because a background is an ordinary filled rectangle.
- Bad, because the shadow paragraph is a second, filtered copy of the measured glyphs.

### 4. A general image-filter layer op

- Good, because it would serve any blurred content.
- Bad, because it widens every `SaveLayer` reader and the damage of an open-ended group for one
  text feature, and a recolored paragraph still needs to ignore span colours.

## More information

Flutter paints a text shadow as the glyphs drawn with a blurred paint, colour glyphs in colour;
CSS draws a silhouette in the shadow colour, which this ADR follows. Revisit §4's maximum if a
design needs a wider text blur.
