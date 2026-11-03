//! Shaper-derived baselines + max_lines/ellipsis ENFORCEMENT.
//!
//! The painter once only *detected* overflow (`did_exceed_max_lines`)
//! while size and paint still covered every line, and baselines were
//! font-size guesses (`height × 0.8`, `alphabetic × 1.125`). Truncation
//! keeps the lines it measured — size, line metrics and painted glyphs
//! agree, an ellipsis shaped into the last kept line — and baselines come
//! from the shaped lines.

use flui_painting::ShapedParagraph;
use flui_painting::text_painter::TextPainter;
use flui_painting::typography::{TextDirection, TextSpan};

/// The rightmost device column any glyph of `paragraph` inks at scale 1 from
/// the origin, drawn by the rasterizer the engine uses.
fn rasterized_ink_right(paragraph: &ShapedParagraph) -> f64 {
    use flui_painting::GlyphRasterizer;
    use flui_painting::glyphs::SwashRasterizer;

    let mut rasterizer = SwashRasterizer::new();
    let mut right = f64::NEG_INFINITY;
    for run in paragraph.runs() {
        let key = rasterizer
            .fonts_mut()
            .prepare_run(&run)
            .expect("a shaped face registers");
        for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
            if let Some(image) = rasterizer.rasterize(glyph.key)
                && image.width() > 0
            {
                right = right.max(f64::from(glyph.x + image.left()) + f64::from(image.width()));
            }
        }
    }
    right
}

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

/// What `paint` records is the paragraph `layout` built — the very `Arc`,
/// not a re-shape — truncated to the lines the measurement kept, ellipsis
/// included, and no glyph of the last line inks past the width it was
/// measured at.
pub(crate) fn a_truncated_paragraph_paints_exactly_the_lines_it_measured() {
    use flui_foundation::geometry::Offset;
    use flui_painting::styling::Color;
    use flui_painting::{Canvas, DrawOp};

    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(
            "one two three four five six seven eight nine ten eleven twelve",
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));
    painter
        .layout(&mut text_cx(), 0.0, 80.0)
        .expect("valid fixture lays out");
    assert!(painter.did_exceed_max_lines());

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    let recorded: Vec<_> = list
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::Paragraph {
                paragraph, color, ..
            } => Some((paragraph, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(recorded.len(), 1, "one paragraph op, got {list:?}");
    let (layout, color) = recorded[0];
    assert_eq!(
        layout.line_count(),
        1,
        "the recorded paragraph has exactly the measured line"
    );
    assert!(
        layout.text().ends_with('…'),
        "the ellipsis is in the recorded text, got {:?}",
        layout.text()
    );
    assert!(
        painter.width() <= 80.0 && layout.size().width <= 80.0,
        "measured {} and painted {} within the 80 px it was laid out at",
        painter.width(),
        layout.size().width
    );
    let ink_right = rasterized_ink_right(layout);
    assert!(
        ink_right <= 80.0 + 1.0,
        "the painted ellipsis line inks to {ink_right}, past the 80 px it was measured at"
    );
    assert_eq!(color, Color::BLACK, "no root colour set → black");
    // The same paint records the same layout: nothing re-shaped at paint.
    let mut again = Canvas::new();
    painter.paint(&mut again, Offset::ZERO);
    let DrawOp::Paragraph {
        paragraph: second, ..
    } = &again.finish()[0].op
    else {
        unreachable!("the first paint recorded a Paragraph");
    };
    assert!(std::sync::Arc::ptr_eq(layout, second));
}

/// A root-only recolour is a paint change: the shaped layout is kept (same
/// `Arc`) and the new colour rides on the paragraph command. A span recolour
/// is baked into the layout and so is a layout change: the next layout
/// shapes again, once.
pub(crate) fn root_recolor_keeps_the_shaped_buffer_and_span_recolor_reshapes_once() {
    use flui_foundation::geometry::Offset;
    use flui_painting::{Canvas, DrawOp, Invalidation};
    use flui_painting::{styling::Color, typography::TextStyle};

    fn styled(root: Color, child: Color) -> TextSpan {
        TextSpan::new("Hello, ")
            .with_style(TextStyle::new().with_color(root))
            .with_child(TextSpan::new("FLUI").with_style(TextStyle::new().with_color(child)))
    }
    fn recorded(painter: &TextPainter) -> (std::sync::Arc<ShapedParagraph>, Color) {
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Offset::ZERO);
        let DrawOp::Paragraph {
            paragraph, color, ..
        } = &canvas.finish()[0].op
        else {
            unreachable!("paint records a Paragraph");
        };
        (std::sync::Arc::clone(paragraph), *color)
    }

    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    let green = Color::rgb(0, 255, 0);
    let mut painter = TextPainter::new()
        .with_text(styled(red, blue))
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut text_cx(), 0.0, f64::INFINITY)
        .expect("valid fixture lays out");
    let (first, first_color) = recorded(&painter);
    assert_eq!(first_color, red);

    // Root recolour: paint-only, same shaped buffer, new colour on the op.
    assert_eq!(
        painter.set_text(Some(styled(green, blue).into())),
        Invalidation::Paint
    );
    let (second, second_color) = recorded(&painter);
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "a root recolour must not reshape"
    );
    assert_eq!(second_color, green);

    // Span recolour: the colour is baked into the layout, so it is a layout
    // change and the next layout shapes once.
    assert_eq!(
        painter.set_text(Some(styled(green, red).into())),
        Invalidation::Layout
    );
    painter
        .layout(&mut text_cx(), 0.0, f64::INFINITY)
        .expect("valid fixture lays out");
    let (third, _) = recorded(&painter);
    assert!(
        !std::sync::Arc::ptr_eq(&second, &third),
        "a span recolour reshapes"
    );
    let runs = third.describe_spans();
    assert!(
        runs.iter().any(|run| run.contains("color=#ff0000ff")),
        "the span's own colour reaches the shaped run, got {runs:?}"
    );
}

/// Every `TextStyle` and `TextSpan` field, changed alone, is classified by
/// what the shaped paragraph holds: `TextPainter::set_text` answers
/// `Layout` for a field the shaped runs depend on and `Paint` for one they do
/// not, and a `Paint` answer keeps a layout whose glyph geometry equals a
/// fresh layout of the new text. Both fixtures name every field, so a new
/// field stops this test compiling until it has a row.
pub(crate) fn every_style_and_span_field_is_classified_by_what_the_shaped_paragraph_holds() {
    use std::sync::Arc;

    use cursor_icon::CursorIcon;
    use flui_foundation::TextSizingIntent;
    use flui_foundation::geometry::Offset;
    use flui_painting::styling::Color;
    use flui_painting::typography::{
        FontFeature, FontStyle, FontVariation, FontWeight, InlineSpan, TextLink, TextShadow,
        TextStyle,
    };
    use flui_painting::{Canvas, DrawOp, Invalidation, TextContext};

    const WIDTH: f64 = 120.0;

    fn laid_out(cx: &mut TextContext, text: InlineSpan) -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(TextDirection::Ltr);
        painter
            .layout(cx, 0.0, WIDTH)
            .expect("valid fixture lays out");
        painter
    }
    fn recorded(painter: &TextPainter) -> (Arc<ShapedParagraph>, Color) {
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Offset::ZERO);
        let DrawOp::Paragraph {
            paragraph, color, ..
        } = &canvas.finish()[0].op
        else {
            unreachable!("paint records a Paragraph");
        };
        (Arc::clone(paragraph), *color)
    }
    /// Everything a paragraph's glyphs land on: its text, size, lines, ink,
    /// spans, and each run's face, glyphs and variation coordinates.
    fn geometry(paragraph: &ShapedParagraph) -> Vec<String> {
        let mut out = vec![
            paragraph.text().to_owned(),
            format!("{:?}", paragraph.size()),
            paragraph.line_count().to_string(),
            format!("{:?}", paragraph.ink_bounds()),
            format!("{:?}", paragraph.describe_spans()),
        ];
        out.extend(paragraph.runs().map(|run| {
            format!(
                "{:?} {:?} {:?}",
                run.face().key(),
                run.glyphs(),
                run.coords()
            )
        }));
        out
    }

    // One context for every layout: a fresh collection per layout would
    // miss the shaping cache and prove nothing about what was kept.
    let mut cx = text_cx();

    // Every field set, none defaulted: a new field fails to compile here.
    let TextStyle {
        color,
        background_color,
        font_size,
        sizing,
        font_weight,
        font_style,
        letter_spacing,
        word_spacing,
        height,
        font_family,
        font_family_fallback,
        font_features,
        font_variations,
        foreground,
        background,
        shadows,
    } = TextStyle {
        color: Some(Color::RED),
        background_color: Some(Color::rgb(250, 240, 10)),
        font_size: Some(30.0),
        sizing: Some(TextSizingIntent::Fixed),
        font_weight: Some(FontWeight::BOLD),
        font_style: Some(FontStyle::Italic),
        letter_spacing: Some(8.0),
        word_spacing: Some(12.0),
        height: Some(2.0),
        font_family: Some("monospace".to_owned()),
        font_family_fallback: vec!["serif".to_owned()],
        font_features: vec![FontFeature::enable("smcp")],
        font_variations: vec![FontVariation::new("wght", 700.0)],
        foreground: Some(Color::GREEN),
        background: Some(Color::rgb(10, 200, 250)),
        shadows: vec![TextShadow::new(Color::BLACK, 1.0, 1.0, 2.0)],
    };
    let TextSpan {
        text,
        style,
        children,
        semantics_label,
        mouse_cursor,
        link,
    } = TextSpan {
        text: Some("narrow".to_owned()),
        style: Some(TextStyle::new().with_font_size(30.0)),
        children: vec![TextSpan::new("!")],
        semantics_label: Some("a label".to_owned()),
        mouse_cursor: Some(CursorIcon::Pointer),
        link: Some(TextLink::new("https://example.com")),
    };

    let root_style = TextStyle::new()
        .with_color(Color::BLUE)
        .with_font_size(16.0);
    let base_child = TextSpan::new("wide FLUI").with_style(TextStyle::new());
    let root = |style: TextStyle, child: TextSpan| -> InlineSpan {
        TextSpan::new("Hello, ")
            .with_style(style)
            .with_child(child)
            .into()
    };
    let child_style = |style: TextStyle| {
        root(
            root_style.clone(),
            TextSpan {
                style: Some(style),
                ..base_child.clone()
            },
        )
    };
    let child_span = |span: TextSpan| root(root_style.clone(), span);
    let base = root(root_style.clone(), base_child.clone());

    use Invalidation::{Layout, Paint};
    // (field, the base text with that field changed, the answer, and for a
    // root recolour the colour the paragraph command must carry)
    let rows: Vec<(&str, InlineSpan, Invalidation, Option<Color>)> = vec![
        (
            "child font_size",
            child_style(TextStyle {
                font_size,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child sizing",
            child_style(TextStyle {
                sizing,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_weight",
            child_style(TextStyle {
                font_weight,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_style",
            child_style(TextStyle {
                font_style,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child letter_spacing",
            child_style(TextStyle {
                letter_spacing,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child word_spacing",
            child_style(TextStyle {
                word_spacing,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child height",
            child_style(TextStyle {
                height,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_family",
            child_style(TextStyle {
                font_family,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_family_fallback",
            child_style(TextStyle {
                font_family_fallback,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_features",
            child_style(TextStyle {
                font_features,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child font_variations",
            child_style(TextStyle {
                font_variations,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        // A span's colour is carried by its runs.
        (
            "child color",
            child_style(TextStyle {
                color,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child foreground",
            child_style(TextStyle {
                foreground,
                ..TextStyle::new()
            }),
            Layout,
            None,
        ),
        (
            "child background_color",
            child_style(TextStyle {
                background_color,
                ..TextStyle::new()
            }),
            Paint,
            None,
        ),
        (
            "child background",
            child_style(TextStyle {
                background,
                ..TextStyle::new()
            }),
            Paint,
            None,
        ),
        (
            "child shadows",
            child_style(TextStyle {
                shadows,
                ..TextStyle::new()
            }),
            Paint,
            None,
        ),
        // The root colour rides on the paragraph command.
        (
            "root color",
            root(
                TextStyle {
                    color,
                    ..root_style.clone()
                },
                base_child.clone(),
            ),
            Paint,
            color,
        ),
        (
            "root foreground",
            root(
                TextStyle {
                    foreground,
                    ..root_style.clone()
                },
                base_child.clone(),
            ),
            Paint,
            foreground,
        ),
        (
            "span text",
            child_span(TextSpan {
                text,
                ..base_child.clone()
            }),
            Layout,
            None,
        ),
        (
            "span style",
            child_span(TextSpan {
                style,
                ..base_child.clone()
            }),
            Layout,
            None,
        ),
        (
            "span children",
            child_span(TextSpan {
                children,
                ..base_child.clone()
            }),
            Layout,
            None,
        ),
        (
            "span semantics_label",
            child_span(TextSpan {
                semantics_label,
                ..base_child.clone()
            }),
            Paint,
            None,
        ),
        (
            "span mouse_cursor",
            child_span(TextSpan {
                mouse_cursor,
                ..base_child.clone()
            }),
            Paint,
            None,
        ),
        (
            "span link",
            child_span(TextSpan {
                link,
                ..base_child.clone()
            }),
            Paint,
            None,
        ),
    ];

    // Controls: the fixture shapes real glyphs, and the geometry oracle sees
    // a spacing change.
    let (base_paragraph, base_color) = recorded(&laid_out(&mut cx, base.clone()));
    assert_eq!(base_color, Color::BLUE);
    assert!(
        base_paragraph.runs().any(|run| !run.glyphs().is_empty()),
        "the fixture shapes glyphs; without a font this test would pass vacuously"
    );
    let spaced = child_style(TextStyle {
        letter_spacing,
        ..TextStyle::new()
    });
    assert_ne!(
        geometry(&base_paragraph),
        geometry(&recorded(&laid_out(&mut cx, spaced)).0),
        "the geometry oracle sees a letter-spacing change"
    );

    let mut failures = Vec::new();
    for (field, varied, expected, command_color) in rows {
        let mut painter = laid_out(&mut cx, base.clone());
        let (before, _) = recorded(&painter);
        let answer = painter.set_text(Some(varied.clone()));
        painter
            .layout(&mut cx, 0.0, WIDTH)
            .expect("valid fixture lays out");
        let (after, color) = recorded(&painter);
        if answer != expected {
            failures.push(format!(
                "{field}: set_text answered {answer:?}, expected {expected:?}"
            ));
            continue;
        }
        if answer == Paint {
            let fresh = recorded(&laid_out(&mut cx, varied)).0;
            if !Arc::ptr_eq(&before, &after) {
                failures.push(format!("{field}: a paint change reshaped"));
            } else if geometry(&after) != geometry(&fresh) {
                failures.push(format!(
                    "{field}: the kept layout differs from a fresh layout of the new text"
                ));
            }
        }
        if let Some(expected_color) = command_color
            && color != expected_color
        {
            failures.push(format!(
                "{field}: the paragraph command carries {color:?}, expected {expected_color:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "misclassified fields:\n{}",
        failures.join("\n")
    );
}
