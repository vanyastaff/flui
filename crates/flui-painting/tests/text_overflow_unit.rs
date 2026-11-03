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
        } = &canvas.finish()[0].op
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
            paragraph: Arc::clone(paragraph),
            color: *color,
            glyphs,
        }
    }

    // One context for every painter: the faces, and so the glyph keys, are
    // the same ones.
    let mut cx: TextContext = text_cx();
    let mut rasterizer = SwashRasterizer::new();
    fn painter_on(text: TextSpan, cx: &mut TextContext, width: f64, truncate: bool) -> TextPainter {
        let mut painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(TextDirection::Ltr);
        if truncate {
            painter.set_max_lines(Some(1));
            painter.set_ellipsis(Some("…".to_string()));
        }
        painter
            .layout(cx, 0.0, width)
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
        let mut painter = painter_on(row.before, &mut cx, row.width, row.truncate);
        let mut previous = painted(&painter, &mut rasterizer);
        for ((after, expected), keeps) in row.steps.into_iter().zip(row.keeps) {
            assert_eq!(
                painter.set_text(Some(after.clone().into())),
                expected,
                "{name}: what the recolour invalidates"
            );
            if expected == Invalidation::Layout {
                painter
                    .layout(&mut cx, 0.0, row.width)
                    .expect("valid fixture lays out");
            }
            let recoloured = painted(&painter, &mut rasterizer);
            let fresh = painted(
                &painter_on(after, &mut cx, row.width, row.truncate),
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
                    .layout(&mut cx, 0.0, row.width)
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
    let mut painter = painter_on(cut_in_child(red, blue), &mut cx, narrow, true);
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
