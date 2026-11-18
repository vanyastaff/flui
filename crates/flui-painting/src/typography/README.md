# Flui Typography Values

`flui_painting::typography` holds the text-style vocabulary that widgets, render objects and
the text painter share. It is plain data: shaping and layout live in
[`text_layout`](../text_layout/mod.rs) and [`text_painter`](../text_painter/mod.rs), and embedded
font faces in [`fonts`](../fonts.rs).

## Modules

| Module | Types |
|--------|-------|
| `text_style` | `TextStyle`, `StrutStyle`, `FontWeight`, `FontStyle` |
| `font_settings` | `FontFeature`, `FontVariation`, `OpenTypeTag` |
| `text_spans` | `InlineSpan`, `TextSpan`, `TextLink`, `PlaceholderSpan`, `PlaceholderDimensions`, `PlaceholderAlignment` |
| `text_alignment` | `TextAlign`, `TextAlignVertical`, `TextAffinity` |
| `text_baseline` | `TextBaseline` |
| `text_decoration` | `TextDecoration`, `TextDecorationLines`, `TextDecorationStyle`, `DecorationThickness`, `TextOverflow`, `TextWidthBasis`, `TextHeightBehavior`, `TextLeadingDistribution` |
| `text_metrics` | `TextPosition`, `TextRange`, `TextBox`, `GlyphInfo`, `LineMetrics` |

The files are private: every item has the one path `flui_painting::typography::<Item>`. Text
direction is `flui_foundation::TextDirection`.

## Styling text

Lengths are logical pixels as `f64`:

```rust
use flui_painting::styling::Color;
use flui_painting::typography::{FontStyle, FontWeight, TextSpan, TextStyle};

let style = TextStyle::new()
    .with_font_family("Roboto")
    .with_font_size(24.0)
    .with_font_weight(FontWeight::BOLD)
    .with_font_style(FontStyle::Italic)
    .with_color(Color::rgb(0x21, 0x21, 0x21));

let span = TextSpan::new("Hello, World!").with_style(style);
```

`TextStyle::merge` combines two styles, the argument taking precedence; `layout_affecting_eq`
compares only the fields that change glyph geometry. FLUI's `TextPainter` also compares the
span colours its shaped runs carry, so it keeps the shaped layout when only the root colour or
a field shaping does not read (background, shadows, decoration) differs.

## Font weights

`FontWeight` is any CSS weight from 1 to 1000 (`FontWeight::try_new`), with `FontWeight::W100`
through `FontWeight::W900`, `NORMAL` (`W400`) and `BOLD` (`W700`) naming the standard steps. A
static font answers with its nearest face; a variable font draws the weight itself, so an
animated weight passes through every weight between its ends.

## OpenType settings

`FontFeature` switches a feature and `FontVariation` places a variable-font axis, each named by
an `OpenTypeTag`: four printable ASCII bytes, checked when the tag is built
(`OpenTypeTag::new(*b"smcp")`, or `OpenTypeTag::try_new` for text), so a setting the shaper
would refuse never reaches it.

## Registering fonts

An app registers a font as bytes through `flui::register_font`, which loads the face into the
app's `FontCollection`. Measurement, paint and carets all read the layout shaped on that
collection, so the face is visible to all three from the next shape onward, and every UI runtime lays
its text out again on its next frame:

```rust
let bytes = std::fs::read("assets/fonts/MyFont.ttf")?;
flui::register_font(&bytes)?; // FontRegistrationError if no face parses
```

Below the app, `FontCollection::register_font` is the same door for a collection a caller built
itself.

With the `bundled-fonts` feature, `flui_painting::fonts` exposes the faces the crate embeds
(`ROBOTO_REGULAR`, `MATERIAL_ICONS_REGULAR`, `CUPERTINO_ICONS`), so a host with no usable system
fonts still measures and paints text and icons.
