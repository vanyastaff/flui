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
fn text_ctx() -> flui_painting::TextContext {
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
        .layout(&mut text_ctx(), 0.0, 80.0)
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
    } = again
        .finish()
        .get(0)
        .expect("the paint recorded a command")
        .op
        .clone()
    else {
        unreachable!("the first paint recorded a Paragraph");
    };
    assert!(std::sync::Arc::ptr_eq(layout, &second));
}

/// A recolour paints what a fresh layout of the new text paints, and shapes
/// again only when it must: a root-only recolour keeps the painted paragraph
/// (the same `Arc`), a span recolour rebuilds it from the kept layout, and
/// only a recolour that splits a run of one colour lays out again.
///
/// Each row lays a painter out on `before`, recolours it to `after`, and
/// compares what it paints with a fresh painter laid out on `after`: the
/// placed glyphs with their colours, the size, the span descriptions and the
/// command's colour. A paint-only row then lays out again under the same
/// constraints and must keep the recoloured paragraph.
pub(crate) fn a_recolour_paints_what_a_fresh_layout_paints() {
    use flui_foundation::geometry::Offset;
    use flui_painting::glyphs::{PlacedGlyph, SwashRasterizer};
    use flui_painting::{Canvas, DrawOp, Invalidation, TextContext};
    use flui_painting::{styling::Color, typography::TextStyle};
    use std::sync::Arc;

    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    let green = Color::rgb(0, 255, 0);

    fn spans(root: (&str, Color), child: (&str, Color)) -> TextSpan {
        TextSpan::new(root.0)
            .with_style(TextStyle::new().with_color(root.1))
            .with_child(TextSpan::new(child.0).with_style(TextStyle::new().with_color(child.1)))
    }
    fn styled(root: Color, child: Color) -> TextSpan {
        spans(("Hello, ", root), ("FLUI", child))
    }
    // The cut falls inside the child: a short root and a child that wraps.
    fn cut_in_child(root: Color, child: Color) -> TextSpan {
        spans(("A ", root), ("several words that wrap", child))
    }
    // The cut falls inside the root; the child is past it.
    fn cut_in_root(root: Color, child: Color) -> TextSpan {
        spans(("several words that wrap ", root), ("tail", child))
    }

    struct Painted {
        paragraph: Arc<ShapedParagraph>,
        color: Color,
        glyphs: Vec<PlacedGlyph>,
    }
    fn painted(painter: &TextPainter, rasterizer: &mut SwashRasterizer) -> Painted {
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Offset::ZERO);
        let DrawOp::Paragraph {
            paragraph, color, ..
        } = canvas
            .finish()
            .get(0)
            .expect("paint records a command")
            .op
            .clone()
        else {
            unreachable!("paint records a Paragraph");
        };
        let glyphs = paragraph
            .runs()
            .flat_map(|run| {
                let key = rasterizer
                    .fonts_mut()
                    .prepare_run(&run)
                    .expect("a shaped face registers");
                run.placed_glyphs(key, (0.0, 0.0), 1.0)
                    .collect::<Vec<PlacedGlyph>>()
            })
            .collect::<Vec<PlacedGlyph>>();
        Painted {
            paragraph,
            color,
            glyphs,
        }
    }

    // One context for every painter: the faces, and so the glyph keys, are
    // the same ones.
    let mut ctx: TextContext = text_ctx();
    let mut rasterizer = SwashRasterizer::new();
    fn painter_on(
        text: TextSpan,
        ctx: &mut TextContext,
        width: f64,
        truncate: bool,
    ) -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(TextDirection::Ltr);
        if truncate {
            painter.set_max_lines(Some(1));
            painter.set_ellipsis(Some("…".to_string()));
        }
        painter
            .layout(ctx, 0.0, width)
            .expect("valid fixture lays out");
        painter
    }

    struct Row {
        name: &'static str,
        steps: Vec<(TextSpan, Invalidation)>,
        before: TextSpan,
        width: f64,
        truncate: bool,
        /// Whether a paint-only step keeps the paragraph painted before it.
        keeps: Vec<bool>,
    }
    let wide = f64::INFINITY;
    let narrow = 90.0;
    let rows = [
        Row {
            name: "a root recolour keeps the painted paragraph",
            before: styled(red, blue),
            steps: vec![(styled(green, blue), Invalidation::Paint)],
            width: wide,
            truncate: false,
            keeps: vec![true],
        },
        Row {
            name: "a span recolour repaints from the kept layout",
            before: styled(red, blue),
            steps: vec![(styled(red, green), Invalidation::Paint)],
            width: wide,
            truncate: false,
            keeps: vec![false],
        },
        Row {
            name: "a recolour that splits a run of one colour lays out again",
            before: styled(red, red),
            steps: vec![(styled(red, green), Invalidation::Layout)],
            width: wide,
            truncate: false,
            keeps: vec![false],
        },
        Row {
            name: "the ellipsis takes the recolour of the span it follows",
            before: cut_in_child(red, blue),
            steps: vec![(cut_in_child(red, green), Invalidation::Paint)],
            width: narrow,
            truncate: true,
            keeps: vec![false],
        },
        Row {
            name: "a recolour of a span the ellipsis dropped",
            before: cut_in_root(red, blue),
            steps: vec![(cut_in_root(red, green), Invalidation::Paint)],
            width: narrow,
            truncate: true,
            keeps: vec![false],
        },
        Row {
            name: "a span takes the colour the root had",
            before: styled(red, blue),
            steps: vec![(styled(green, red), Invalidation::Paint)],
            width: wide,
            truncate: false,
            keeps: vec![false],
        },
        Row {
            name: "a span takes the root's colour",
            before: styled(red, blue),
            steps: vec![(styled(red, red), Invalidation::Paint)],
            width: wide,
            truncate: false,
            keeps: vec![false],
        },
        Row {
            name: "a root recolour after a span recolour keeps its paragraph",
            before: styled(red, blue),
            steps: vec![
                (styled(red, green), Invalidation::Paint),
                (styled(blue, green), Invalidation::Paint),
            ],
            width: wide,
            truncate: false,
            keeps: vec![false, true],
        },
    ];

    for row in rows {
        let name = row.name;
        let mut painter = painter_on(row.before, &mut ctx, row.width, row.truncate);
        let mut previous = painted(&painter, &mut rasterizer);
        for ((after, expected), keeps) in row.steps.into_iter().zip(row.keeps) {
            assert_eq!(
                painter.set_text(Some(after.clone().into())),
                expected,
                "{name}: what the recolour invalidates"
            );
            if expected == Invalidation::Layout {
                painter
                    .layout(&mut ctx, 0.0, row.width)
                    .expect("valid fixture lays out");
            }
            let recoloured = painted(&painter, &mut rasterizer);
            let fresh = painted(
                &painter_on(after, &mut ctx, row.width, row.truncate),
                &mut rasterizer,
            );
            assert_eq!(recoloured.glyphs, fresh.glyphs, "{name}: placed glyphs");
            assert_eq!(
                recoloured.paragraph.size(),
                fresh.paragraph.size(),
                "{name}: size"
            );
            assert_eq!(
                recoloured.paragraph.describe_spans(),
                fresh.paragraph.describe_spans(),
                "{name}: spans"
            );
            assert_eq!(recoloured.color, fresh.color, "{name}: command colour");
            assert_eq!(
                Arc::ptr_eq(&previous.paragraph, &recoloured.paragraph),
                keeps,
                "{name}: whether the painted paragraph is kept"
            );
            if expected == Invalidation::Paint {
                // A layout under the same constraints is a cache hit, and
                // paints the recoloured paragraph.
                painter
                    .layout(&mut ctx, 0.0, row.width)
                    .expect("valid fixture lays out");
                let again = painted(&painter, &mut rasterizer);
                assert!(
                    Arc::ptr_eq(&recoloured.paragraph, &again.paragraph),
                    "{name}: the next layout keeps the recoloured paragraph"
                );
            }
            previous = recoloured;
        }
    }

    // The ellipsis glyph, last of the truncated line, paints in the colour
    // of the span it follows, and the cut fell inside that span.
    let mut painter = painter_on(cut_in_child(red, blue), &mut ctx, narrow, true);
    let before = painted(&painter, &mut rasterizer);
    assert!(
        before.paragraph.text().ends_with('…') && before.paragraph.text().contains("A s"),
        "the cut falls inside the child, got {:?}",
        before.paragraph.text()
    );
    assert_eq!(
        painter.set_text(Some(cut_in_child(red, green).into())),
        Invalidation::Paint
    );
    let after = painted(&painter, &mut rasterizer);
    assert_eq!(
        after.glyphs.last().map(|glyph| glyph.color),
        Some(Some(green)),
        "the ellipsis paints in the recoloured child's colour"
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
    use flui_painting::styling::{Color, Shadow};
    use flui_painting::typography::{
        FontFeature, FontStyle, FontVariation, FontWeight, InlineSpan, TextDecoration, TextLink,
        TextStyle,
    };
    use flui_painting::{Canvas, DrawOp, Invalidation, TextContext};

    const WIDTH: f64 = 120.0;

    fn laid_out(ctx: &mut TextContext, text: InlineSpan) -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(TextDirection::Ltr);
        painter
            .layout(ctx, 0.0, WIDTH)
            .expect("valid fixture lays out");
        painter
    }
    fn recorded(painter: &TextPainter) -> (Arc<ShapedParagraph>, Color) {
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Offset::ZERO);
        // The span's background and shadow are recorded before its glyphs.
        canvas
            .finish()
            .iter()
            .find_map(|command| match &command.op {
                DrawOp::Paragraph {
                    paragraph, color, ..
                } => Some((Arc::clone(paragraph), *color)),
                _ => None,
            })
            .expect("paint records a Paragraph")
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
    let mut ctx = text_ctx();

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
        shadows,
        decoration,
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
        shadows: Some(vec![Shadow::new(Color::BLACK, Offset::new(1.0, 1.0), 2.0)]),
        decoration: Some(TextDecoration::UNDERLINE),
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
        // The base child paints in the root's colour, in one glyph run with
        // the root text: a colour of its own splits that run. A span that
        // already has its own colour recolours paint-only
        // (`a_recolour_paints_what_a_fresh_layout_paints`).
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
            "child background_color",
            child_style(TextStyle {
                background_color,
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
        (
            "child decoration",
            child_style(TextStyle {
                decoration,
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
    let (base_paragraph, base_color) = recorded(&laid_out(&mut ctx, base.clone()));
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
        geometry(&recorded(&laid_out(&mut ctx, spaced)).0),
        "the geometry oracle sees a letter-spacing change"
    );

    let mut failures = Vec::new();
    for (field, varied, expected, command_color) in rows {
        let mut painter = laid_out(&mut ctx, base.clone());
        let (before, _) = recorded(&painter);
        let answer = painter.set_text(Some(varied.clone()));
        painter
            .layout(&mut ctx, 0.0, WIDTH)
            .expect("valid fixture lays out");
        let (after, color) = recorded(&painter);
        if answer != expected {
            failures.push(format!(
                "{field}: set_text answered {answer:?}, expected {expected:?}"
            ));
            continue;
        }
        if answer == Paint {
            let fresh = recorded(&laid_out(&mut ctx, varied)).0;
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
