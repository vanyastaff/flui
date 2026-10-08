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

/// The engine neither shapes nor names a shaper: none of a shaper's entry
/// points (cosmic-text's, Parley's, fontique's or swash's) — nor the crates themselves —
/// appears in the text path's source or the manifest. Every glyph a recorder
/// hands it comes from a `ShapedParagraph` built from the layout that
/// measured it, placed through `ShapedRun::placed_glyphs` and rasterised
/// through flui-painting's `SwashRasterizer` (ADR-0065, ADR-0067, ADR-0092
/// §4-§5). The performance overlay's labels arrive shaped too, recorded
/// upstream through the UI runtime's text context, so no text-context, font
/// collection or paragraph-spec name appears either.
fn the_engine_does_not_shape() {
    let sources = [
        ("glyph_atlas.rs", include_str!("glyph_atlas.rs")),
        ("batches/text.rs", include_str!("batches/text.rs")),
        ("layer_dispatcher.rs", include_str!("layer_dispatcher.rs")),
        ("layer_render.rs", include_str!("layer_render.rs")),
        ("command_renderer.rs", include_str!("command_renderer.rs")),
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
            "TextContext",
            "FontCollection",
            "ParagraphSpec",
            "parley_text",
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
    let fonts = FontCollection::with_host_fonts(&flui_painting::HostFonts::scan());
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
    assert_ne!(
        ink_in(&pixels, band(&painter, 0), everywhere(), inked),
        [] as [(u32, u32); 0]
    );
    assert!(
        !ink_in(&pixels, band(&painter, 1), everywhere(), inked).is_empty(),
        "世界 is painted on line 2"
    );
}

/// A colour emoji after U+2028 paints its colour on line 2, and nothing
/// colourful on line 1.
fn colour_emoji_on_line_two(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::with_host_fonts(&flui_painting::HostFonts::scan());
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

/// RTL lines end at their allocated box's right edge, including the shorter
/// second line. Tight allocation fills its width; a loose wrap cap does not
/// expand the paragraph beyond the independently measured longest line.
fn arabic_rtl_right_aligns_each_line(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::with_host_fonts(&flui_painting::HostFonts::scan());
    let max_width = f64::from(WIDE.0) - 16.0;
    let mut painter = laid_out(
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
    let longest = laid_out(
        &fonts,
        "مرحبا بالعالم",
        black(32.0),
        TextDirection::Rtl,
        f64::INFINITY,
    )
    .width();
    assert!(
        longest + 32.0 < max_width,
        "the loose and tight boxes differ"
    );
    for (min_width, allocation) in [(0.0, longest), (max_width, max_width)] {
        painter.layout(
            &mut flui_painting::TextContext::new(&fonts),
            min_width,
            max_width,
        );
        assert!((painter.width() - allocation).abs() < 0.01);
        let pixels = read_back(renderer, &painter, 1.0);
        let right = ORIGIN.dx + allocation;
        for line in 0..2 {
            let ink = ink_in(&pixels, band(&painter, line), everywhere(), inked);
            let edge = ink
                .iter()
                .map(|(x, _)| *x)
                .max()
                .expect("each line is inked");
            // Allow the visual glyph's side bearing, but not a line's slack
            // or a second alignment shift at replay.
            assert!(
                (f64::from(edge) + 1.0 - right).abs() <= 32.0 / 6.0,
                "min width {min_width}, line {} ends at {edge}, expected right edge {right}",
                line + 1
            );
        }
    }
}

/// CR LF breaks the line once: `B` is painted on the line right after `A`,
/// with no empty line between (painting mapping decision 18).
fn crlf_puts_b_on_line_two(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::new();
    let painter = laid_out(
        &fonts,
        "A\r\nB",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    assert_eq!(recorded(&painter).line_count(), 2);
    let pixels = read_back(renderer, &painter, 1.0);
    assert!(
        !ink_in(&pixels, band(&painter, 1), everywhere(), inked).is_empty(),
        "B is painted on line 2"
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

/// A selection of the second line's `cd` in `"ab\ncd"`, filled from
/// `get_boxes_for_selection` under the text as a field paints its highlight,
/// colours line 2 over `cd` and leaves line 1 alone. The cosmic-text caret
/// layout matched its per-line glyph offsets against whole-text offsets and
/// gave this range no box at all.
fn selection_highlights_the_second_line(renderer: &crate::headless::HeadlessRenderer) {
    let fonts = FontCollection::new();
    let painter = laid_out(
        &fonts,
        "ab\ncd",
        black(32.0),
        TextDirection::Ltr,
        f64::from(WIDE.0) - 16.0,
    );
    let boxes = painter.get_boxes_for_selection(3, 5);
    assert_eq!(boxes.len(), 1, "one box for `cd`: {boxes:?}");
    let highlight = boxes[0].rect;
    let mut canvas = Canvas::new();
    let blue = flui_painting::Paint::fill(Color::rgba(0, 0, 255, 255));
    canvas.draw_rect(highlight.translate_offset(ORIGIN), &blue);
    painter.paint(&mut canvas, ORIGIN);
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    let pixels = renderer
        .render_layer_tree(&builder.build(), WIDE)
        .expect("the headless capture path rasterizes a paragraph");

    let columns = ORIGIN.dx + highlight.left() + 1.0..ORIGIN.dx + highlight.right() - 1.0;
    assert!(
        !ink_in(&pixels, band(&painter, 1), columns, saturated).is_empty(),
        "line 2 is highlighted over `cd` ({highlight:?})"
    );
    assert!(
        ink_in(&pixels, band(&painter, 0), everywhere(), saturated).is_empty(),
        "line 1 is not highlighted"
    );
    assert!(
        ink_in(
            &pixels,
            band(&painter, 1),
            ORIGIN.dx + highlight.right() + 2.0..f64::from(WIDE.0),
            saturated
        )
        .is_empty(),
        "nothing is highlighted past `cd`"
    );
}

/// Two image-local font namespaces can reuse a complete glyph key.
fn colliding_font_scene(text: &str, family: &str) -> flui_layer::Scene {
    let fonts = flui_painting::FontCollection::new();
    let mut context = flui_painting::TextContext::new(&fonts);
    let style = TextStyle::new().with_font_family(family);
    let spans = [(text.to_owned(), None)];
    let paragraph = context
        .shape(&flui_painting::parley_text::ParagraphSpec {
            spans: &spans,
            default_style: Some(&style),
            font_size: 48.0,
            max_width: None,
            min_width: 0.0,
            text_align: flui_painting::typography::TextAlign::Start,
            line_height: None,
            direction: TextDirection::Ltr,
            max_lines: None,
            ellipsis: None,
        })
        .to_shaped(None);
    let paragraph = std::sync::Arc::new(
        flui_painting::testing::paragraph_with_font_ids(&paragraph, 1)
            .expect("fixture font ids fit u64"),
    );
    assert_eq!(
        paragraph.runs().len(),
        1,
        "the fixture has no fallback face"
    );
    let run = paragraph.runs().next().expect("the fixture shapes one run");
    assert_eq!(
        run.face().key(),
        flui_painting::glyphs::FaceKey {
            blob_id: 1,
            index: 0
        }
    );
    assert_eq!(run.glyphs().len(), 1);
    assert_eq!(
        run.glyphs()[0].id,
        36,
        "vendored fonts share this glyph key"
    );
    let mut canvas = Canvas::new();
    canvas.draw_paragraph(&paragraph, Offset::new(8.0, 8.0), Color::BLACK);
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    flui_layer::Scene::new(builder.build())
}

#[derive(Clone, Copy)]
enum FontTransition {
    OrdinaryToPlugin,
    ReloadedPlugin,
    PluginToOrdinary,
}

fn check_font_transition(renderer: &crate::headless::HeadlessRenderer, transition: FontTransition) {
    use crate::raster::{PresentDisposition, RasterBackend};

    let before = colliding_font_scene("?", "Roboto");
    let after = colliding_font_scene("w", "Material Icons");
    let expected = |scene: &flui_layer::Scene| {
        let mut capture = renderer
            .retained_capture((SIDE, SIDE))
            .expect("reference capture");
        capture.render_scene(scene).expect("reference renders");
        capture.read_rgba().expect("reference readback")
    };
    let before_pixels = expected(&before);
    let after_pixels = expected(&after);
    assert_ne!(
        before_pixels, after_pixels,
        "the two fonts draw distinct ink"
    );
    let mut capture = renderer
        .retained_capture((SIDE, SIDE))
        .expect("transition capture");
    match transition {
        FontTransition::OrdinaryToPlugin => {
            capture.render_scene(&before).expect("ordinary frame");
        }
        FontTransition::ReloadedPlugin | FontTransition::PluginToOrdinary => {
            capture
                .render_plugin_scene(&before, true)
                .expect("plugin frame");
            // Consume unmanaged-frame damage promotion without changing source.
            capture
                .render_plugin_frame(&before, false)
                .expect("managed plugin frame");
        }
    }
    assert_eq!(capture.read_rgba().expect("before readback"), before_pixels);
    let unchanged = match transition {
        FontTransition::OrdinaryToPlugin => capture.render_scene(&before),
        FontTransition::ReloadedPlugin | FontTransition::PluginToOrdinary => {
            capture.render_plugin_frame(&before, false)
        }
    }
    .expect("unchanged frame");
    assert_eq!(
        unchanged,
        PresentDisposition::NoDamage,
        "the transition starts with no producer damage or pending promotion"
    );
    let disposition = match transition {
        FontTransition::OrdinaryToPlugin => capture.render_plugin_frame(&after, false),
        FontTransition::ReloadedPlugin => capture.render_plugin_frame(&after, true),
        FontTransition::PluginToOrdinary => capture.render_scene(&after),
    }
    .expect("the new font namespace renders");
    assert_eq!(
        disposition,
        PresentDisposition::Presented,
        "source admission must repaint without producer damage"
    );
    assert_eq!(
        capture.read_rgba().expect("after readback"),
        after_pixels,
        "a reused font key must draw the current source's ink"
    );
}

fn ordinary_to_plugin_repaints_with_the_new_font(renderer: &crate::headless::HeadlessRenderer) {
    check_font_transition(renderer, FontTransition::OrdinaryToPlugin);
}

fn a_reloaded_plugin_repaints_with_the_new_font(renderer: &crate::headless::HeadlessRenderer) {
    check_font_transition(renderer, FontTransition::ReloadedPlugin);
}

fn plugin_to_ordinary_repaints_with_the_new_font(renderer: &crate::headless::HeadlessRenderer) {
    check_font_transition(renderer, FontTransition::PluginToOrdinary);
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
    let rows: [Row; 11] = [
        (
            "ordinary_to_plugin_repaints_with_the_new_font",
            ordinary_to_plugin_repaints_with_the_new_font,
        ),
        (
            "a_reloaded_plugin_repaints_with_the_new_font",
            a_reloaded_plugin_repaints_with_the_new_font,
        ),
        (
            "plugin_to_ordinary_repaints_with_the_new_font",
            plugin_to_ordinary_repaints_with_the_new_font,
        ),
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
        ("crlf_puts_b_on_line_two", crlf_puts_b_on_line_two),
        ("a_2x_baseline_row", a_2x_baseline_row),
        (
            "selection_highlights_the_second_line",
            selection_highlights_the_second_line,
        ),
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

/// The overlay's readout shaped through `text`, at the default bounds, for a
/// sample at `fps` with a diagnostic line.
fn overlay(text: &mut flui_painting::TextContext, fps: f64) -> flui_layer::PerformanceOverlayLayer {
    flui_layer::PerformanceOverlayLayer::record(
        text,
        flui_layer::PerformanceOverlayLayer::default_bounds(),
        flui_layer::PerformanceOverlayOption::all(),
        &flui_layer::PerformanceSample {
            fps,
            frame_time_ms: 16.7,
            diagnostic_line: Some("present_p99=16ms input_p99=24ms"),
        },
    )
}

/// The channels of a pixel that must each exceed every channel of the
/// second set by a margin, for the pixel to lean to a label's colour.
type Channels = (&'static [usize], &'static [usize]);

/// `layer` under a uniform `scale`, as a layer tree.
fn scaled(layer: flui_layer::Layer, scale: f64) -> flui_layer::LayerTree {
    let mut tree = flui_layer::LayerTree::new(flui_layer::Layer::from(
        flui_layer::TransformLayer::scale(scale),
    ));
    let root = tree.root();
    tree.push_child(root, layer);
    tree
}

/// The overlay's labels read back where and in the colour they were
/// recorded, at `scale`: inside each label's ink box some pixel leans to its
/// colour (cyan "GPU", green fps, purple "Frame"), a pixel of the overlay
/// away from every label is the background composited over white, and a
/// pixel below the overlay stays white. Fails if a label is missing, carries
/// no glyphs, or is placed anywhere but its recorded offset under the
/// layer's transform.
fn overlay_labels_land_as_recorded(renderer: &crate::headless::HeadlessRenderer, scale: f64) {
    const SIZE: (u32, u32) = (1000, 160);
    let mut text = flui_painting::TextContext::new(&FontCollection::new());
    let layer = overlay(&mut text, 60.0);
    let readout: Vec<_> = layer
        .readout()
        .iter()
        .filter_map(|command| match &command.op {
            flui_painting::DrawOp::Paragraph {
                paragraph, offset, ..
            } => Some((paragraph.clone(), *offset)),
            _ => None,
        })
        .collect();
    let pixels = renderer
        .render_layer_tree(&scaled(flui_layer::Layer::from(layer), scale), SIZE)
        .expect("the headless capture path rasterizes the overlay");
    let at = |x: u32, y: u32| {
        let i = ((y * SIZE.0 + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let lean = |p: [u8; 3], (hi, lo): Channels| {
        hi.iter()
            .all(|h| lo.iter().all(|l| i16::from(p[*h]) - i16::from(p[*l]) > 40))
    };
    // The channels that exceed the others by a margin: cyan is g and b over
    // r, the 60 fps green g over r and b, purple b and r over g.
    let rows: [(&str, Channels); 3] = [
        ("GPU", (&[1, 2], &[0])),
        ("60", (&[1], &[0, 2])),
        ("Frame", (&[0, 2], &[1])),
    ];
    for (label, channels) in rows {
        let (paragraph, offset) = readout
            .iter()
            .find(|(paragraph, _)| paragraph.text() == label)
            .unwrap_or_else(|| panic!("the readout records {label:?}"));
        let ink = paragraph
            .ink_bounds()
            .unwrap_or_else(|| panic!("{label:?} has ink"));
        let (x0, y0) = (
            (ink.left() + offset.dx) * scale,
            (ink.top() + offset.dy) * scale,
        );
        let (x1, y1) = (
            (ink.right() + offset.dx) * scale,
            (ink.bottom() + offset.dy) * scale,
        );
        let hit = (y0.floor() as u32..y1.ceil() as u32)
            .flat_map(|y| (x0.floor() as u32..x1.ceil() as u32).map(move |x| (x, y)))
            .any(|(x, y)| lean(at(x, y), channels));
        assert!(
            hit,
            "{label:?} inks its colour inside its ink box at scale {scale}"
        );
    }
    // The background: rgba(10, 10, 15, 200) over white.
    let background = at((470.0 * scale) as u32, (12.0 * scale) as u32);
    for (channel, expected) in background.iter().zip([63u8, 63, 67]) {
        assert!(
            channel.abs_diff(expected) <= 2,
            "the overlay's background at scale {scale} reads {background:?}"
        );
    }
    assert_eq!(
        at((470.0 * scale) as u32, (70.0 * scale) as u32),
        [255, 255, 255],
        "nothing inks below the overlay at scale {scale}"
    );
}

/// Frames that carry the overlay register no face the scene does not name:
/// an app paragraph and the overlay, shaped over one collection, rendered
/// five times through one painter with a changing fps, leave the glyph
/// registry at the scene's distinct faces. Fails if the engine shapes the
/// overlay over a collection of its own, whose faces the registry would then
/// hold as well.
fn overlay_frames_do_not_grow_the_glyph_registry(renderer: &crate::headless::HeadlessRenderer) {
    let mut capture = renderer
        .retained_capture((500, 100))
        .expect("capture target");
    let mut text = flui_painting::TextContext::new(&FontCollection::new());
    let app = std::sync::Arc::new(
        text.shape(&flui_painting::parley_text::ParagraphSpec {
            spans: &[("Hamburg".to_owned(), None)],
            default_style: None,
            font_size: 14.0,
            max_width: None,
            min_width: 0.0,
            text_align: flui_painting::typography::TextAlign::Start,
            line_height: None,
            direction: TextDirection::Ltr,
            max_lines: None,
            ellipsis: None,
        })
        .to_shaped(None),
    );
    let app_picture = || {
        let mut canvas = Canvas::new();
        canvas.draw_paragraph(&app, Offset::new(8.0, 70.0), Color::BLACK);
        canvas.finish()
    };
    let mut counts = Vec::new();
    let mut named = std::collections::BTreeSet::new();
    for fps in [10.0, 30.0, 60.0, 99.0, 120.0] {
        let layer = overlay(&mut text, fps);
        for list in [layer.readout(), &app_picture()] {
            for command in list {
                if let flui_painting::DrawOp::Paragraph { paragraph, .. } = &command.op {
                    named.extend(paragraph.runs().map(|run| run.face().blob().id()));
                }
            }
        }
        let mut tree = flui_layer::LayerTree::new(flui_layer::Layer::from(
            flui_layer::PictureLayer::new(app_picture()),
        ));
        let root = tree.root();
        tree.push_child(root, flui_layer::Layer::from(layer));
        capture
            .render_unmanaged(&flui_layer::Scene::new(tree))
            .expect("the frame renders");
        counts.push(capture.glyph_face_count());
    }
    assert!(!named.is_empty(), "the scene names faces");
    assert_eq!(
        counts,
        vec![named.len(); 5],
        "the registry holds exactly the faces the scene names, every frame"
    );
}

/// The performance overlay, recorded upstream, reads back as recorded at 1x
/// and 2x, and its frames leave the glyph registry at the scene's faces.
#[test]
fn performance_overlay_labels_read_back() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type Row = (&'static str, fn(&crate::headless::HeadlessRenderer));
    let rows: [Row; 3] = [
        ("labels_at_1x", |renderer| {
            overlay_labels_land_as_recorded(renderer, 1.0);
        }),
        ("labels_at_2x", |renderer| {
            overlay_labels_land_as_recorded(renderer, 2.0);
        }),
        (
            "overlay_frames_do_not_grow_the_glyph_registry",
            overlay_frames_do_not_grow_the_glyph_registry,
        ),
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
        "performance_overlay_labels_read_back: failing rows: {failed:?}"
    );
}
