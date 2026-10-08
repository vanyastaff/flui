//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_painting::typography::{
    FontFeature, FontVariation, TextDirection, TextPosition, TextSpan, TextStyle,
};
use flui_painting::{Canvas, DrawOp, FontCollection, ShapedParagraph, TextContext, TextPainter};

fn painted_style_paragraph(painter: &TextPainter) -> Arc<ShapedParagraph> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records the measured paragraph")
}

fn styled_probe(fonts: &FontCollection, text: &str, style: TextStyle) -> TextPainter {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(text, style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(&mut TextContext::new(fonts), 0.0, 400.0);
    painter
}

fn word_spacing_changes_geometry(in_child: bool) {
    let fonts = FontCollection::new();
    let baseline = styled_probe(&fonts, "A A A", TextStyle::default());
    let style = TextStyle::default().with_word_spacing(4.0);
    let span = TextSpan::styled("A A A", style);
    let mut spaced = TextPainter::new()
        .with_text(if in_child {
            TextSpan::new("").with_child(span)
        } else {
            span
        })
        .with_text_direction(TextDirection::Ltr);
    spaced.layout(&mut TextContext::new(&fonts), 0.0, 400.0);
    assert!(
        (spaced.width() - baseline.width() - 8.0).abs() < 1e-3,
        "two spaces each grow by four logical pixels"
    );
    let painted = painted_style_paragraph(&spaced);
    assert_eq!(painted.size(), spaced.size());
    let end = spaced.get_offset_for_caret(TextPosition::downstream(5));
    assert!(
        (end.dx - spaced.width()).abs() < 1e-3,
        "the caret reads the spaced layout"
    );
}

pub(crate) fn root_word_spacing_reaches_measurement_paint_and_carets() {
    word_spacing_changes_geometry(false);
}

pub(crate) fn span_word_spacing_reaches_measurement_paint_and_carets() {
    word_spacing_changes_geometry(true);
}

fn feature_probe(features: Vec<FontFeature>) {
    let fonts = FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/probe-arabic-ligature.ttf"))
        .expect("the generated Arabic face loads");
    let text = "\u{0627}\u{0644}\u{0627}\u{0633}\u{0645}";
    let style = TextStyle {
        font_family: Some("FLUI Probe Arabic".to_owned()),
        font_size: Some(32.0),
        font_features: features,
        ..TextStyle::default()
    };
    let painter = styled_probe(&fonts, text, style);
    let paragraph = painted_style_paragraph(&painter);
    let glyphs: Vec<_> = paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.id))
        .collect();
    assert_eq!(
        glyphs.len(),
        5,
        "disabled rlig paints individual letters: {glyphs:?}"
    );
    assert!(!glyphs.contains(&6), "the lam-alef ligature is disabled");
    assert!(
        (painter.width() - 62.4).abs() < 1e-3,
        "the five generated advances are measured: {}",
        painter.width()
    );
    assert_eq!(paragraph.size(), painter.size());
}

pub(crate) fn font_features_change_the_measured_and_painted_glyphs() {
    feature_probe(vec![FontFeature::disable("rlig")]);
}

pub(crate) fn invalid_font_features_do_not_replace_valid_settings() {
    feature_probe(vec![
        FontFeature::disable("rlig"),
        FontFeature::new("rlig", -1),
        FontFeature::new("rlig", 65536),
        FontFeature::enable("rl"),
        FontFeature::enable("r\nig"),
        FontFeature::new("rlig", 65537),
    ]);
}

fn variation_coords(variations: Vec<FontVariation>) -> Vec<i16> {
    weighted_variation_coords(variations, None)
}

fn weighted_variation_coords(
    variations: Vec<FontVariation>,
    weight: Option<flui_painting::typography::FontWeight>,
) -> Vec<i16> {
    let fonts = FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/probe-variable-wght.ttf"))
        .expect("the generated variable face loads");
    let painter = styled_probe(
        &fonts,
        "AA",
        TextStyle {
            font_family: Some("FLUI Probe Variable".to_owned()),
            font_weight: weight,
            font_variations: variations,
            ..TextStyle::default()
        },
    );
    let paragraph = painted_style_paragraph(&painter);
    assert_eq!(paragraph.size(), painter.size());
    let mut runs = paragraph.runs();
    let coords = runs
        .next()
        .expect("the two letters produce a run")
        .coords()
        .to_vec();
    assert!(runs.next().is_none(), "the generated family shapes one run");
    coords
}

pub(crate) fn font_variations_select_the_painted_run_instance() {
    let low = variation_coords(vec![FontVariation::new("wght", 100.0)]);
    let high = variation_coords(vec![FontVariation::new("wght", 900.0)]);
    assert_eq!(low.len(), 1, "the generated font has one axis");
    assert_eq!(high.len(), 1);
    assert!(
        low[0] < 0 && high[0] > 0,
        "opposite sides of the default instance: {low:?}/{high:?}"
    );
    assert_eq!(
        weighted_variation_coords(
            vec![FontVariation::new("wght", 100.0)],
            Some(flui_painting::typography::FontWeight::W900),
        ),
        low,
        "the explicit axis wins over the requested font weight"
    );
    for (settings, expected) in [
        (
            vec![
                FontVariation::new("wght", 100.0),
                FontVariation::new("wght", 900.0),
            ],
            high,
        ),
        (
            vec![
                FontVariation::new("wght", 900.0),
                FontVariation::new("wght", 100.0),
            ],
            low,
        ),
    ] {
        assert_eq!(
            variation_coords(settings),
            expected,
            "the last valid duplicate axis selects the painted instance"
        );
    }
}

pub(crate) fn invalid_font_variations_do_not_replace_valid_settings() {
    let valid = variation_coords(vec![FontVariation::new("wght", 100.0)]);
    let filtered = variation_coords(vec![
        FontVariation::new("wght", 100.0),
        FontVariation::new("wght", f64::NAN),
        FontVariation::new("wght", f64::INFINITY),
        FontVariation::new("wght", f64::MAX),
        FontVariation::new("wg", 100.0),
        FontVariation::new("w\ngt", 100.0),
    ]);
    assert_eq!(filtered, valid);
    assert_eq!(filtered.len(), 1);
    assert!(
        filtered[0] < 0,
        "the valid low-weight instance still reaches paint"
    );
}

pub(crate) fn text_weight_adjustment_shapes_once_and_restores_authored_weights() {
    use flui_painting::typography::{FontWeight, TextPosition};
    use flui_painting::{GlyphContent, GlyphRasterizer, glyphs::SwashRasterizer};

    const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
    let real_fonts = FontCollection::new();
    real_fonts
        .register_font(ROBOTO)
        .expect("vendored Roboto loads");
    let mut real_text = styled_probe(
        &real_fonts,
        "HH",
        TextStyle::default()
            .with_font_family("Roboto")
            .with_font_size(32.0)
            .with_font_weight(FontWeight::W400),
    );
    let raster = |painter: &TextPainter| {
        let paragraph = painted_style_paragraph(painter);
        let mut rasterizer = SwashRasterizer::new();
        let mut images = Vec::new();
        for run in paragraph.runs() {
            assert_eq!(run.face().blob().bytes().as_ref().as_ref(), ROBOTO);
            let key = rasterizer
                .fonts_mut()
                .prepare_run(&run)
                .expect("the submitted font face registers for rasterization");
            for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                let image = rasterizer
                    .rasterize(glyph.key)
                    .expect("Roboto glyph rasterizes");
                assert_eq!(image.content(), GlyphContent::Mask);
                assert!(image.data().iter().any(|coverage| *coverage != 0));
                images.push(image);
            }
        }
        assert_eq!(images.len(), 2, "both H outlines are rasterized");
        images
    };
    let mut context = TextContext::new(&real_fonts);
    let mut authored = None;
    for adjustment in [0, 300, 0, 300, 0] {
        real_text.set_font_weight_adjustment(adjustment);
        real_text.layout(&mut context, 0.0, 400.0);
        let images = raster(&real_text);
        if adjustment == 0 {
            if let Some(authored) = &authored {
                assert_eq!(
                    &images, authored,
                    "off restores the exact authored glyph masks"
                );
            } else {
                authored = Some(images);
            }
        } else {
            let authored = authored.as_ref().expect("baseline rasterized first");
            let coverage = |images: &[flui_painting::GlyphImage]| {
                images
                    .iter()
                    .flat_map(|image| image.data())
                    .map(|value| u64::from(*value))
                    .sum::<u64>()
            };
            assert!(
                coverage(&images) > coverage(authored),
                "accepted weight adjustment must increase real rasterized ink coverage"
            );
        }
    }

    let fonts = FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/probe-variable-wght.ttf"))
        .expect("the generated variable face loads");
    let coords = |painter: &TextPainter| {
        let paragraph = painted_style_paragraph(painter);
        let runs: Vec<_> = paragraph.runs().collect();
        assert_eq!(runs.len(), 1);
        match runs[0].coords() {
            [] => vec![0],
            values => values.to_vec(),
        }
    };
    let default_style = TextStyle::default()
        .with_font_family("FLUI Probe Variable")
        .with_font_variation(FontVariation::new("wght", 100.0));
    let color_only = TextStyle::default().with_color(flui_painting::styling::Color::BLACK);
    let spans = [("AA".to_owned(), Some(color_only))];
    let inherited = TextContext::new(&fonts).shape(&flui_painting::parley_text::ParagraphSpec {
        spans: &spans,
        default_style: Some(&default_style),
        font_size: 16.0,
        font_weight_adjustment: 237,
        max_width: None,
        min_width: 0.0,
        text_align: flui_painting::typography::TextAlign::Start,
        line_height: None,
        direction: TextDirection::Ltr,
        max_lines: None,
        ellipsis: None,
    });
    assert_eq!(
        inherited
            .to_shaped(None)
            .runs()
            .next()
            .expect("default-style run")
            .coords(),
        variation_coords(vec![FontVariation::new("wght", 337.0)]),
        "a color-only span resolves the default authored axis before adjustment"
    );
    let ellipsis_style = TextStyle::default()
        .with_font_family("FLUI Probe Variable")
        .with_font_weight(FontWeight::W400);
    let mut truncated = TextPainter::new()
        .with_text(TextSpan::styled("AA\nAA", ellipsis_style.clone()))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("A".to_owned()));
    truncated.set_font_weight_adjustment(237);
    let mut reference = TextPainter::new()
        .with_text(TextSpan::styled(
            "AA\nAA",
            ellipsis_style.with_font_variation(FontVariation::new("wght", 637.0)),
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("A".to_owned()));
    truncated.layout(&mut TextContext::new(&fonts), 0.0, 100.0);
    reference.layout(&mut TextContext::new(&fonts), 0.0, 100.0);
    assert_eq!(painted_style_paragraph(&truncated).text(), "AAA");
    assert_eq!(coords(&truncated), coords(&reference));
    assert_eq!(truncated.size(), reference.size());
    let boxes = |painter: &TextPainter| {
        painter
            .get_boxes_for_selection(0, 5)
            .into_iter()
            .map(|text_box| (text_box.rect, text_box.direction))
            .collect::<Vec<_>>()
    };
    assert_eq!(boxes(&truncated), boxes(&reference));
    for position in [0, 1, 2, 5] {
        let position = TextPosition::downstream(position);
        let offset = reference.get_offset_for_caret(position);
        assert_eq!(truncated.get_offset_for_caret(position), offset);
        assert_eq!(
            truncated.get_position_for_offset(offset),
            reference.get_position_for_offset(offset),
        );
    }
    for (weight, axes, adjustment, expected) in [
        (FontWeight::W100, vec![], 300, 400.0),
        (FontWeight::W400, vec![], 237, 637.0),
        (FontWeight::W700, vec![], 300, 1000.0),
        (FontWeight::W900, vec![], 300, 1000.0),
        (FontWeight::W400, vec![], -137, 263.0),
        (FontWeight::W400, vec![], i32::MIN, 1.0),
        (FontWeight::W400, vec![], i32::MAX, 1000.0),
        (
            FontWeight::W900,
            vec![FontVariation::new("wght", 233.25)],
            100,
            333.25,
        ),
        (
            FontWeight::W900,
            vec![
                FontVariation::new("wght", 900.0),
                FontVariation::new("wght", 100.0),
                FontVariation::new("wght", f64::NAN),
            ],
            137,
            237.0,
        ),
    ] {
        let mut painter = styled_probe(
            &fonts,
            "AA",
            TextStyle {
                font_family: Some("FLUI Probe Variable".to_owned()),
                font_weight: Some(weight),
                font_variations: axes,
                ..TextStyle::default()
            },
        );
        let authored = coords(&painter);
        let mut resolved = variation_coords(vec![FontVariation::new("wght", expected)]);
        if resolved.is_empty() {
            resolved.push(0);
        }
        for current in [adjustment, 0, adjustment, 0] {
            painter.set_font_weight_adjustment(current);
            let mut context = TextContext::new(&fonts);
            let intrinsic = painter.max_intrinsic_width(&mut context);
            painter.layout(&mut context, 0.0, 400.0);
            assert_eq!(
                &coords(&painter),
                if current == 0 { &authored } else { &resolved }
            );
            if current != 0 {
                let expected_weight = format!("weight={expected}");
                let paragraph = painted_style_paragraph(&painter);
                let description = paragraph
                    .describe_spans()
                    .first()
                    .expect("the shaped span is described");
                assert!(
                    description
                        .split_whitespace()
                        .any(|part| part == expected_weight),
                    "diagnostics must retain the exact resolved shaping weight {expected}"
                );
            }
            assert!(painter.width().is_finite() && painter.height().is_finite());
            assert!((intrinsic - painter.width()).abs() < 1e-3);
            assert!(
                (painter.get_offset_for_caret(TextPosition::downstream(2)).dx - painter.width())
                    .abs()
                    < 1e-3
            );
        }
    }
    let root = TextSpan::styled(
        "A",
        TextStyle::default()
            .with_font_family("FLUI Probe Variable")
            .with_font_weight(FontWeight::W100),
    )
    .with_child(TextSpan::new("A"))
    .with_child(TextSpan::styled(
        "A",
        TextStyle::default().with_font_weight(FontWeight::W400),
    ))
    .with_child(TextSpan::styled(
        "A",
        TextStyle::default().with_font_weight(FontWeight::W900),
    ));
    let mut mixed = TextPainter::new()
        .with_text(root)
        .with_text_direction(TextDirection::Ltr);
    let mut baseline = None;
    for adjustment in [0, 300, 0, 300, 0] {
        mixed.set_font_weight_adjustment(adjustment);
        mixed.layout(&mut TextContext::new(&fonts), 0.0, 400.0);
        let paragraph = painted_style_paragraph(&mixed);
        assert_eq!(paragraph.text(), "AAAA");
        let actual: Vec<_> = paragraph
            .runs()
            .map(|run| {
                if run.coords().is_empty() {
                    vec![0]
                } else {
                    run.coords().to_vec()
                }
            })
            .collect();
        if adjustment == 0 {
            if let Some(baseline) = &baseline {
                assert_eq!(&actual, baseline);
            } else {
                baseline = Some(actual);
            }
        } else {
            let expected: Vec<_> = [400.0, 700.0, 1000.0]
                .into_iter()
                .map(|weight| {
                    let mut coords = variation_coords(vec![FontVariation::new("wght", weight)]);
                    if coords.is_empty() {
                        coords.push(0);
                    }
                    coords
                })
                .collect();
            assert_eq!(
                actual, expected,
                "unstyled child inherits authored weight before one adjustment"
            );
        }
    }
}

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

/// When the ellipsis is wider than the text's narrowest run, skipping
/// truncation must not under-report min intrinsic below the ellipsis
/// (Codex review on #1089).
pub(crate) fn wide_ellipsis_floors_min_intrinsic_width() {
    let ellipsis = "………";
    let text_only = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr);
    let ellipsis_only = TextPainter::new()
        .with_text(TextSpan::new(ellipsis))
        .with_text_direction(TextDirection::Ltr);
    let truncated = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some(ellipsis.to_string()));

    let text_min = text_only.min_intrinsic_width(&mut text_cx());
    let ellipsis_width = ellipsis_only.max_intrinsic_width(&mut text_cx());
    let floored = truncated.min_intrinsic_width(&mut text_cx());

    assert!(
        ellipsis_width > text_min + 0.01,
        "precondition: ellipsis {ellipsis_width} must exceed text min {text_min}"
    );
    assert!(
        floored + 0.01 >= ellipsis_width,
        "min intrinsic {floored} must not under-report below ellipsis {ellipsis_width}"
    );

    // Cached path after layout must keep the same floor.
    let mut laid_out = TextPainter::new()
        .with_text(TextSpan::new("i i"))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some(ellipsis.to_string()));
    laid_out.layout(&mut text_cx(), 0.0, 200.0);
    assert!(
        (laid_out.min_intrinsic_width(&mut text_cx()) - floored).abs() < 0.01,
        "cached min must keep the ellipsis floor"
    );
}

/// The ellipsis floor is shaped in the style truncation paints the ellipsis
/// in, the first run's, not the root's: a 10 px empty root holding a 100 px
/// `"i i"` under one line lays out, at its own min intrinsic width, no wider
/// than that width. Fails if the floor is shaped in the root's 10 px, which
/// leaves a 100 px ellipsis-only line overflowing it.
pub(crate) fn a_rich_span_ellipsis_floors_min_intrinsic_width() {
    use flui_painting::typography::TextStyle;

    let size = |size: f64| TextStyle {
        font_size: Some(size),
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(
            TextSpan::styled("", size(10.0)).with_child(TextSpan::styled("i i", size(100.0))),
        )
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_owned()));

    let min = painter.min_intrinsic_width(&mut text_cx());
    painter.layout(&mut text_cx(), 0.0, min);
    let painted = painter.width();
    assert!(
        painted <= min + 0.01,
        "laid out at its min intrinsic width {min}, the paragraph is {painted} wide"
    );
}

/// An empty paragraph measures the line box and baseline a line of text in
/// the same style measures, so an empty `Text` in a baseline-aligned row
/// sits on its neighbours' baseline (painting mapping decision 15). Every
/// root property that shapes the line counts, not only the font size the
/// painter passes separately: a line height and a family (the probe face,
/// whose line metrics are not Roboto's) each change the empty line too.
pub(crate) fn an_empty_paragraph_measures_a_line_of_its_style() {
    use flui_painting::TextBaseline;
    use flui_painting::typography::{FontWeight, TextStyle};

    const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");

    let fonts = flui_painting::FontCollection::new();
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    let probe = |size: f64| TextStyle {
        font_family: Some("FLUI Probe Mono".to_owned()),
        font_weight: Some(FontWeight::W100),
        font_size: Some(size),
        ..TextStyle::default()
    };
    let rows = [
        TextStyle {
            font_size: Some(14.0),
            ..TextStyle::default()
        },
        TextStyle {
            font_size: Some(32.0),
            ..TextStyle::default()
        },
        TextStyle {
            font_size: Some(14.0),
            height: Some(2.0),
            ..TextStyle::default()
        },
        probe(20.0),
        TextStyle {
            height: Some(2.0),
            ..probe(20.0)
        },
    ];
    let mut failures = Vec::new();
    for style in rows {
        let measure = |text: &str| {
            let mut painter = TextPainter::new()
                .with_text(TextSpan::styled(text, style.clone()))
                .with_text_direction(TextDirection::Ltr);
            painter.layout(
                &mut flui_painting::TextContext::new(&fonts),
                0.0,
                f64::INFINITY,
            );
            (
                painter.height(),
                painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
            )
        };
        let (empty, line) = (measure(""), measure("A"));
        if (empty.0 - line.0).abs() > 1e-3 || (empty.1 - line.1).abs() > 1e-3 {
            failures.push(format!(
                "{style:?}: empty {empty:?}, one line {line:?} (height, baseline)"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Measurement is Parley on the lent context: the probe face is registered on
/// one collection only, so a painter that measured anywhere else would never
/// see it.
pub(crate) mod parley_measurement {
    use flui_painting::parley_text::ParagraphSpec;
    use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
    use flui_painting::{FontCollection, TextContext, TextPainter};

    const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
    const SIZE: f64 = 20.0;
    /// Finite, so a repeated layout is served from the painter's cache when
    /// nothing it is keyed on changed.
    const WIDTH: f64 = 1000.0;

    /// The probe face maps `A` one em wide, so four `A`s measure exactly
    /// four em where it is registered; elsewhere the family falls back to
    /// Roboto, whose `A` is narrower.
    fn probe_style() -> TextStyle {
        TextStyle {
            font_family: Some("FLUI Probe Mono".to_owned()),
            font_weight: Some(FontWeight::W100),
            font_size: Some(SIZE),
            ..TextStyle::default()
        }
    }

    fn probe_painter() -> TextPainter {
        TextPainter::new()
            .with_text(TextSpan::styled("AAAA", probe_style()))
            .with_text_direction(TextDirection::Ltr)
    }

    /// The same painter measures through whichever context it is lent: the
    /// probe width through the UI runtime whose collection holds the face, the
    /// fallback width through one that does not, and on the first exactly
    /// what that context shapes the paragraph to.
    pub(crate) fn measurement_follows_the_context_it_is_given() {
        let with_probe = FontCollection::new();
        with_probe
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let mut a = TextContext::new(&with_probe);
        let mut b = TextContext::new(&FontCollection::new());
        let mut painter = probe_painter();

        painter.layout(&mut a, 0.0, WIDTH);
        let through_a = painter.size();
        painter.layout(&mut b, 0.0, WIDTH);
        let through_b = painter.size();

        assert!(
            (through_a.width - 4.0 * SIZE).abs() < 0.01,
            "four one-em `A`s through A's context are {} px, got {}",
            4.0 * SIZE,
            through_a.width
        );
        assert!(
            (through_b.width - through_a.width).abs() > 1.0,
            "B's collection lacks the face, so the same painter measures the \
             fallback: A {} vs B {}",
            through_a.width,
            through_b.width
        );

        let spans = vec![("AAAA".to_owned(), Some(probe_style()))];
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the probe size is exact in f32"
        )]
        let shaped = a
            .shape(&ParagraphSpec {
                font_weight_adjustment: 0,
                spans: &spans,
                default_style: None,
                font_size: SIZE as f32,
                max_width: None,
                min_width: 0.0,
                text_align: flui_painting::typography::TextAlign::Start,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .metrics();
        painter.layout(&mut a, 0.0, WIDTH);
        assert!((painter.size().width - shaped.width).abs() < f64::EPSILON);
        assert!((painter.size().height - shaped.height).abs() < f64::EPSILON);
    }

    /// The intrinsic widths a layout cached answer only for the context that
    /// laid out: asked through a context over another collection, or after
    /// a face was registered on the same one, the painter measures again.
    pub(crate) fn intrinsic_widths_follow_the_context_they_are_asked_through() {
        let with_probe = FontCollection::new();
        with_probe
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let mut a = TextContext::new(&with_probe);
        let mut b = TextContext::new(&FontCollection::new());
        let mut painter = probe_painter();

        painter.layout(&mut a, 0.0, WIDTH);
        let max_through_a = painter.max_intrinsic_width(&mut a);
        let min_through_a = painter.min_intrinsic_width(&mut a);
        let max_through_b = painter.max_intrinsic_width(&mut b);
        let min_through_b = painter.min_intrinsic_width(&mut b);

        assert!(
            (max_through_a - 4.0 * SIZE).abs() < 0.01,
            "four one-em `A`s through A's context are {} px, got {max_through_a}",
            4.0 * SIZE
        );
        assert!(
            (max_through_b - max_through_a).abs() > 1.0,
            "B lacks the face, so its max intrinsic width is the fallback's: A {max_through_a} vs B {max_through_b}"
        );
        assert!(
            (min_through_b - min_through_a).abs() > 1.0,
            "B lacks the face, so its min intrinsic width is the fallback's: A {min_through_a} vs B {min_through_b}"
        );

        let later = FontCollection::new();
        let mut c = TextContext::new(&later);
        painter.layout(&mut c, 0.0, WIDTH);
        let before = painter.max_intrinsic_width(&mut c);
        later
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        let after = painter.max_intrinsic_width(&mut c);
        assert!(
            (after - 4.0 * SIZE).abs() < 0.01,
            "a face registered since the layout is measured: before {before}, got {after}"
        );
    }

    /// A face registered on the collection after the painter measured makes
    /// the next layout measure again under the same constraints.
    pub(crate) fn a_registration_on_the_collection_invalidates_the_painter_cache() {
        let fonts = FontCollection::new();
        let mut context = TextContext::new(&fonts);
        let mut painter = probe_painter();

        painter.layout(&mut context, 0.0, WIDTH);
        let before = painter.width();
        fonts
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        painter.layout(&mut context, 0.0, WIDTH);
        let after = painter.width();

        assert!(
            (before - 4.0 * SIZE).abs() > 1.0,
            "before registration the family falls back, got {before}"
        );
        assert!(
            (after - 4.0 * SIZE).abs() < 0.01,
            "after registration the cached fallback width is re-measured, got {after}"
        );
    }
}
