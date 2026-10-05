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
    painter.layout(&mut text_cx(), 0.0, 80.0);
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
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);
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
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);
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
