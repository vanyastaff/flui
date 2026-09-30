//! The engine rasterises the paragraph it is handed and never shapes one
//! itself (ADR-0065).

use flui_foundation::geometry::Offset;
use flui_layer::SceneBuilder;
use flui_painting::FontCollection;
use flui_painting::{Canvas, TextPainter};
use flui_painting::{
    styling::Color,
    typography::{TextDirection, TextSpan, TextStyle},
};

const SIDE: u32 = 96;

fn sample(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * SIDE + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

/// The engine neither shapes nor names a shaper: none of cosmic-text's,
/// Parley's, fontique's or swash's entry points — nor the crates themselves —
/// appears in the text path's source or the manifest. Every glyph a recorder
/// hands it comes from a `ShapedParagraph` built from the layout that
/// measured it, placed through `ShapedRun::placed_glyphs` and rasterised
/// through flui-painting's `SwashRasterizer` (ADR-0065, ADR-0067, ADR-0092
/// §4-§5). The performance overlay's labels, which no recorder shapes, go
/// through flui-painting's `TextContext`, not a shaper crate.
fn the_engine_does_not_shape() {
    let sources = [
        ("glyph_atlas.rs", include_str!("glyph_atlas.rs")),
        ("batches/text.rs", include_str!("batches/text.rs")),
        ("layer_dispatcher.rs", include_str!("layer_dispatcher.rs")),
        ("dispatch.rs", include_str!("dispatch.rs")),
        ("painter/draw.rs", include_str!("painter/draw.rs")),
        ("painter/mod.rs", include_str!("painter/mod.rs")),
        ("renderer.rs", include_str!("renderer.rs")),
        ("headless.rs", include_str!("headless.rs")),
    ];
    for (name, source) in sources {
        for needle in [
            "shape_until_scroll",
            "set_rich_text",
            ".set_text(",
            "cosmic_text",
            "parley::",
            "fontique",
            "swash::",
            "skrifa",
        ] {
            assert!(
                !source.contains(needle),
                "{name} reaches the shaper (`{needle}`); shaping belongs to flui-painting"
            );
        }
    }
    let manifest = include_str!("../Cargo.toml");
    for shaper in ["cosmic-text", "parley", "fontique", "skrifa", "swash"] {
        assert!(
            !manifest.contains(shaper),
            "flui-engine must not depend on {shaper}"
        );
    }
}

/// Shapes `text` at `size` and returns a scene of it painted in `color` at
/// the origin, wrapped by `decorate` (a clip, a layer) around the paint.
fn paragraph_scene(
    text: &str,
    size: f64,
    color: Color,
    decorate: impl FnOnce(&mut Canvas, &mut dyn FnMut(&mut Canvas)),
) -> flui_layer::LayerTree {
    let style = TextStyle::new().with_font_size(size).with_color(color);
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(text).with_style(style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(
        &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
        0.0,
        f64::from(SIDE as f32 * 4.0),
    );
    let mut canvas = Canvas::new();
    decorate(&mut canvas, &mut |canvas| {
        painter.paint(canvas, Offset::ZERO);
    });
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    builder.build()
}

/// A glyph's colour lands as recorded: a mid-grey paragraph on white
/// reaches the target as that grey where the glyph fully covers a pixel.
///
/// The engine draws in gamma space onto a `Unorm` target (see
/// `Renderer::select_surface_format`); a rasteriser that converted the
/// text colour to linear before writing — as the previous one did — turned
/// `#808080` into `#373737` on every mid-tone label while black and white
/// text, being fixed points of the transfer, looked right.
fn glyph_colour_lands_as_recorded() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let grey = Color::rgb(128, 128, 128);
    let tree = paragraph_scene("I", 80.0, grey, |canvas, paint| paint(canvas));
    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize a paragraph");

    let inked: Vec<[u8; 4]> = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .map(|(x, y)| sample(&pixels, x, y))
        .filter(|p| *p != [255, 255, 255, 255])
        .collect();
    assert!(!inked.is_empty(), "the glyph must paint");
    let darkest = inked.iter().map(|p| p[0]).min().unwrap();
    assert!(
        (127..=129).contains(&darkest),
        "a fully covered pixel of a #808080 glyph must read back as #808080, \
         darkest channel found {darkest} (a linear conversion would give ~55)"
    );
}

/// Paragraph contract, read back from the GPU: glyph colour lands as recorded,
/// and the engine does not shape.
#[test]
fn paragraphs_read_back_as_recorded() {
    glyph_colour_lands_as_recorded();
    the_engine_does_not_shape();
}

/// The target the Parley rows paint into, larger than [`SIDE`] so two lines
/// of 32 px text fit.
const WIDE: (u32, u32) = (256, 160);
/// Where each row's paragraph is painted, in logical pixels.
const ORIGIN: Offset<f64> = Offset::new(8.0, 8.0);

/// A laid-out painter for `text` in `style` over `fonts`, at `max_width`.
fn laid_out(
    fonts: &FontCollection,
    text: &str,
    style: TextStyle,
    direction: TextDirection,
    max_width: f64,
) -> TextPainter {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(text).with_style(style))
        .with_text_direction(direction);
    painter.layout(&mut flui_painting::TextContext::new(fonts), 0.0, max_width);
    painter
}

/// The paragraph `painter` records, to read its lines and glyphs.
fn recorded(painter: &TextPainter) -> std::sync::Arc<flui_painting::ShapedParagraph> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    list.iter()
        .find_map(|command| match &command.op {
            flui_painting::DrawOp::Paragraph { paragraph, .. } => Some(paragraph.clone()),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// Whether every glyph of `painter`'s paragraph came from a face that maps
/// its character: a row whose script the host has no face for is skipped.
fn covered(painter: &TextPainter) -> bool {
    recorded(painter)
        .runs()
        .all(|run| run.glyphs().iter().all(|glyph| glyph.id != 0))
}

/// `painter` painted at [`ORIGIN`] under a uniform `scale`, read back from a
/// [`WIDE`] white target.
fn read_back(
    renderer: &crate::headless::HeadlessRenderer,
    painter: &TextPainter,
    scale: f64,
) -> Vec<u8> {
    let mut canvas = Canvas::new();
    canvas.scale(scale, scale);
    painter.paint(&mut canvas, ORIGIN);
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    renderer
        .render_layer_tree(&builder.build(), WIDE)
        .expect("the headless capture path rasterizes a paragraph")
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * WIDE.0 + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

/// A pixel black text darkened.
fn inked(p: [u8; 4]) -> bool {
    p[..3].iter().any(|c| *c < 200)
}

/// A pixel whose channels differ widely: a colour glyph, not grey ink.
fn saturated(p: [u8; 4]) -> bool {
    let max = p[..3].iter().copied().max().unwrap_or(0);
    let min = p[..3].iter().copied().min().unwrap_or(0);
    max - min > 60
}

/// The pixels passing `test` with `y` in `rows` and `x` in `columns`, as
/// `(x, y)`.
fn ink_in(
    pixels: &[u8],
    rows: std::ops::Range<f64>,
    columns: std::ops::Range<f64>,
    test: fn([u8; 4]) -> bool,
) -> Vec<(u32, u32)> {
    let clamp = |v: f64, max: u32| (v.max(0.0) as u32).min(max);
    let (y0, y1) = (clamp(rows.start, WIDE.1), clamp(rows.end, WIDE.1));
    let (x0, x1) = (clamp(columns.start, WIDE.0), clamp(columns.end, WIDE.0));
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| test(pixel(pixels, *x, *y)))
        .collect()
}

/// The device rows of line `index` (0-based) of `painter`'s paragraph at
/// scale 1: from the previous line's bottom to this one's.
fn band(painter: &TextPainter, index: usize) -> std::ops::Range<f64> {
    let lines = recorded(painter).line_count() as f64;
    let line = painter.height() / lines;
    let top = ORIGIN.dy + line * index as f64;
    top..top + line
}

fn everywhere() -> std::ops::Range<f64> {
    0.0..f64::from(WIDE.0)
}

fn black(size: f64) -> TextStyle {
    TextStyle::new()
        .with_font_size(size)
        .with_color(Color::BLACK)
}

/// U+2028 ends the line on Parley; cosmic-text drew the whole text on one.
fn latin_breaks_at_a_line_separator(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::new();
    let painter = laid_out(
        &fonts,
        "Hamburg\u{2028}fonts",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    let hamburg = laid_out(
        &fonts,
        "Hamburg",
        black(32.0),
        TextDirection::Ltr,
        f64::INFINITY,
    )
    .width();
    let pixels = read_back(renderer, &painter, 1.0);
    assert!(
        !ink_in(&pixels, band(&painter, 0), everywhere(), inked).is_empty(),
        "line 1 is inked"
    );
    assert!(
        !ink_in(&pixels, band(&painter, 1), everywhere(), inked).is_empty(),
        "'fonts' is painted on line 2"
    );
    let past = ORIGIN.dx + hamburg + 2.0..f64::from(WIDE.0);
    assert!(
        ink_in(&pixels, band(&painter, 0), past, inked).is_empty(),
        "nothing on line 1 past Hamburg's measured {hamburg} px"
    );
}

/// Bold on the bundled Roboto, which has no bold face, is synthesised: the
/// same word inks more coverage than at regular weight, where cosmic-text
/// painted the regular face for both.
fn synthetic_bold_inks_more_than_regular(renderer: &crate::headless::HeadlessRenderer) {
    use flui_painting::typography::FontWeight;

    let fonts = FontCollection::new();
    let coverage = |weight: FontWeight| -> u64 {
        let painter = laid_out(
            &fonts,
            "Hamburg",
            black(32.0).with_font_weight(weight),
            TextDirection::Ltr,
            f64::INFINITY,
        );
        read_back(renderer, &painter, 1.0)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u64::from(255 - p[0]))
            .sum()
    };
    let (regular, bold) = (coverage(FontWeight::W400), coverage(FontWeight::W700));
    assert!(regular > 0, "the regular word inks");
    let ratio = bold as f64 / regular as f64;
    assert!(
        ratio >= 1.08,
        "synthetic bold inks {bold}, regular {regular}: ratio {ratio:.3}"
    );
}

/// CJK over the host's faces breaks at U+2028 as Latin does.
fn cjk_breaks_at_a_line_separator(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::with_host_faces(&flui_painting::shared_font_system());
    let painter = laid_out(
        &fonts,
        "你好\u{2028}世界",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    if !covered(&painter) {
        println!("cjk_breaks_at_a_line_separator: skipped, no host face covers CJK");
        return;
    }
    let pixels = read_back(renderer, &painter, 1.0);
    assert!(!ink_in(&pixels, band(&painter, 0), everywhere(), inked).is_empty());
    assert!(
        !ink_in(&pixels, band(&painter, 1), everywhere(), inked).is_empty(),
        "世界 is painted on line 2"
    );
}

/// A colour emoji after U+2028 paints its colour on line 2, and nothing
/// colourful on line 1.
fn colour_emoji_on_line_two(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::with_host_faces(&flui_painting::shared_font_system());
    let painter = laid_out(
        &fonts,
        "A\u{2028}😀",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    if !covered(&painter) {
        println!("colour_emoji_on_line_two: skipped, no host face covers the emoji");
        return;
    }
    let pixels = read_back(renderer, &painter, 1.0);
    assert!(
        ink_in(&pixels, band(&painter, 0), everywhere(), saturated).is_empty(),
        "line 1 holds only the black A"
    );
    assert!(
        !ink_in(&pixels, band(&painter, 1), everywhere(), saturated).is_empty(),
        "the emoji paints in colour on line 2"
    );
}

/// Under `Rtl` at a finite width each line's visible end is at the
/// paragraph box's right edge, the shorter second line included: each line
/// is aligned in the paragraph's own box, which the paint offset places at
/// the right of the width it was laid out at.
fn arabic_rtl_right_aligns_each_line(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::with_host_faces(&flui_painting::shared_font_system());
    let max_width = f64::from(WIDE.0) - 16.0;
    let painter = laid_out(
        &fonts,
        "مرحبا بالعالم\u{2028}عالم",
        black(32.0),
        TextDirection::Rtl,
        max_width,
    );
    if !covered(&painter) {
        println!("arabic_rtl_right_aligns_each_line: skipped, no host face covers Arabic");
        return;
    }
    let pixels = read_back(renderer, &painter, 1.0);
    let right = ORIGIN.dx + max_width;
    for line in 0..2 {
        let ink = ink_in(&pixels, band(&painter, line), everywhere(), inked);
        let edge = ink
            .iter()
            .map(|(x, _)| *x)
            .max()
            .expect("each line is inked");
        // Within a sixth of an em: the right side bearing of the line's
        // last visual glyph. A line left in its Parley alignment box, or
        // shifted by the paint offset a second time, lands a line's slack
        // (here tens of pixels) away.
        assert!(
            (f64::from(edge) + 1.0 - right).abs() <= 32.0 / 6.0,
            "line {} ends its ink at {edge}, the box's right edge is {right}",
            line + 1
        );
    }
}

/// CR LF lays out as Parley breaks it: `B` on the third line, the second
/// empty (painting mapping decision 18).
fn crlf_puts_b_on_line_three(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::new();
    let painter = laid_out(
        &fonts,
        "A\r\nB",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    assert_eq!(recorded(&painter).line_count(), 3);
    let pixels = read_back(renderer, &painter, 1.0);
    assert!(ink_in(&pixels, band(&painter, 1), everywhere(), inked).is_empty());
    assert!(
        !ink_in(&pixels, band(&painter, 2), everywhere(), inked).is_empty(),
        "B is painted on line 3"
    );
}

/// At 2× the glyph's bottom row sits on `round(baseline × 2)`: the device
/// baseline cosmic-text placed glyphs on.
fn a_2x_baseline_row(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::new();
    let painter = laid_out(&fonts, "H", black(33.0), TextDirection::Ltr, f64::INFINITY);
    let baseline =
        painter.compute_distance_to_actual_baseline(flui_painting::TextBaseline::Alphabetic);
    let pixels = read_back(renderer, &painter, 2.0);
    let ink = ink_in(&pixels, 0.0..f64::from(WIDE.1), everywhere(), inked);
    let bottom = ink.iter().map(|(_, y)| *y).max().expect("the H is inked");
    let expected = ((ORIGIN.dy + baseline) * 2.0).round();
    assert!(
        (f64::from(bottom) + 1.0 - expected).abs() <= 1.0,
        "the H's last inked row is {bottom}, the 2x baseline row {expected}"
    );
}

/// Parley's runs read back as laid out: hard breaks, synthesis, fallback
/// faces, right alignment and the device baseline, each sampled where Parley
/// paint and the cosmic-text paint it replaced differ.
#[test]
fn parley_runs_read_back() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type Row = (&'static str, fn(&crate::headless::HeadlessRenderer));
    let rows: [Row; 7] = [
        (
            "latin_breaks_at_a_line_separator",
            latin_breaks_at_a_line_separator,
        ),
        (
            "synthetic_bold_inks_more_than_regular",
            synthetic_bold_inks_more_than_regular,
        ),
        (
            "cjk_breaks_at_a_line_separator",
            cjk_breaks_at_a_line_separator,
        ),
        ("colour_emoji_on_line_two", colour_emoji_on_line_two),
        (
            "arabic_rtl_right_aligns_each_line",
            arabic_rtl_right_aligns_each_line,
        ),
        ("crlf_puts_b_on_line_three", crlf_puts_b_on_line_three),
        ("a_2x_baseline_row", a_2x_baseline_row),
    ];
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| row(&renderer))).is_err()
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(
        failed.is_empty(),
        "parley_runs_read_back: failing rows: {failed:?}"
    );
}
