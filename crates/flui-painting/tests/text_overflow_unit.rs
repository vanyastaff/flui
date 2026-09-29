//! Shaper-derived baselines + max_lines/ellipsis ENFORCEMENT.
//!
//! Pre-fix the painter only *detected* overflow (`did_exceed_max_lines`)
//! while size and paint still covered every line, and baselines were
//! font-size guesses (`height × 0.8`, `alphabetic × 1.125`). Now the
//! truncation re-shapes the kept prefix — size, line metrics, and
//! painted glyphs agree — and baselines come from cosmic-text's
//! per-line `line_y`.

use flui_painting::text_layout::TextLayout;
use flui_painting::text_painter::TextPainter;
use flui_painting::typography::{TextDirection, TextSpan};

/// What `paint` records is the layout that was measured — the very `Arc`,
/// not a re-shape — so a truncated paragraph paints exactly the lines it
/// measured, ellipsis included.
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
    painter.layout(0.0, 80.0);
    assert!(painter.did_exceed_max_lines());

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    let recorded: Vec<_> = list
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::Paragraph { layout, color, .. } => Some((layout, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(recorded.len(), 1, "one paragraph op, got {list:?}");
    let (layout, color) = recorded[0];
    let metrics = layout.metrics();
    assert_eq!(
        metrics.line_count, 1,
        "the recorded layout has exactly the measured line"
    );
    assert!(metrics.truncated);
    assert!(
        layout.text().ends_with('…'),
        "the ellipsis is in the recorded text, got {:?}",
        layout.text()
    );
    assert_eq!(color, Color::BLACK, "no root colour set → black");
    // The same paint records the same layout: nothing re-shaped at paint.
    let mut again = Canvas::new();
    painter.paint(&mut again, Offset::ZERO);
    let DrawOp::Paragraph { layout: second, .. } = &again.finish()[0].op else {
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
    fn recorded(painter: &TextPainter) -> (std::sync::Arc<TextLayout>, Color) {
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, Offset::ZERO);
        let DrawOp::Paragraph { layout, color, .. } = &canvas.finish()[0].op else {
            unreachable!("paint records a Paragraph");
        };
        (std::sync::Arc::clone(layout), *color)
    }

    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    let green = Color::rgb(0, 255, 0);
    let mut painter = TextPainter::new()
        .with_text(styled(red, blue))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(0.0, f64::INFINITY);
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
    painter.layout(0.0, f64::INFINITY);
    let (third, _) = recorded(&painter);
    assert!(
        !std::sync::Arc::ptr_eq(&second, &third),
        "a span recolour reshapes"
    );
    let runs = third.describe_runs();
    assert!(
        runs.iter().any(|run| run.contains("color=#ff0000ff")),
        "the span's own colour reaches the shaped run, got {runs:?}"
    );
}
