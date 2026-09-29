# Flui Typography Values

`flui_painting::typography` holds the text-style vocabulary that widgets, render objects and
the text painter share. It is plain data: shaping and layout live in
[`text_layout`](../text_layout/mod.rs) and [`text_painter`](../text_painter/mod.rs), and embedded
font faces in [`fonts`](../fonts.rs).

## Modules

| Module | Types |
|--------|-------|
| `text_style` | `TextStyle`, `StrutStyle`, `FontWeight`, `FontStyle`, `FontFeature`, `FontVariation`, `TextShadow` |
| `text_spans` | `InlineSpan`, `TextSpan`, `PlaceholderSpan`, `PlaceholderDimensions`, `PlaceholderAlignment`, `MouseCursor` |
| `text_alignment` | `TextAlign`, `TextAlignVertical`, `TextDirection`, `TextAffinity` |
| `text_decoration` | `TextDecoration`, `TextDecorationStyle`, `TextOverflow`, `TextWidthBasis`, `TextHeightBehavior`, `TextLeadingDistribution` |
| `text_metrics` | `TextPosition`, `TextRange`, `TextSelection`, `TextBox`, `GlyphInfo`, `LineMetrics` |

Every item is re-exported at `flui_painting::typography`.

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
compares only the fields that change glyph geometry, so a text engine can keep its shaped
layout when only paint attributes differ.

## Font weights

`FontWeight::W100` through `FontWeight::W900`, with `FontWeight::NORMAL` (`W400`) and
`FontWeight::BOLD` (`W700`). `FontWeight::from_css` maps a numeric CSS weight to the nearest
variant; `value()` returns it back.

## Registering fonts

Fonts register as bytes with the process-wide font system; the face is visible to measurement
and to the engine's glyph pipeline from the next shape onward:

```rust
use flui_painting::shared_font_system;

let bytes = std::fs::read("assets/fonts/MyFont.ttf")?;
shared_font_system().register_font(&bytes)?; // RegisterFontError if no face parses
```

With the `bundled-fonts` feature, `flui_painting::fonts` exposes the faces the crate embeds
(`ROBOTO_REGULAR`, `MATERIAL_ICONS_REGULAR`, `CUPERTINO_ICONS`), so a host with no usable system
fonts still measures and paints text and icons.

## Comparison with Flutter

| Flutter | Flui |
|---------|------|
| `FontWeight.w400` | `FontWeight::W400` |
| `FontStyle.italic` | `FontStyle::Italic` |
| `TextStyle(fontFamily: 'Roboto')` | `TextStyle::new().with_font_family("Roboto")` |
| `TextSpan(text:, style:)` | `TextSpan::new(text).with_style(style)` |
| `FontLoader` | `shared_font_system().register_font(bytes)` |
