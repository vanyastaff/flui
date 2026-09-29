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

#[test]
fn rich_inheritance_merges_parent_style_into_children() {
    use flui_painting::typography::TextStyle;

    // A child WITHOUT its own size inherits the parent's 28px — the
    // tree must measure exactly like the flat 28px equivalent.
    let inherited = {
        let mut root = TextSpan::new("AA");
        root.style = Some(TextStyle::new().with_font_size(28.0));
        root.children.push(TextSpan::new("BB"));
        root
    };
    let flat = TextSpan::new("AABB").with_style(TextStyle::new().with_font_size(28.0));

    let mut a = TextPainter::new()
        .with_text(inherited)
        .with_text_direction(TextDirection::Ltr);
    a.layout(0.0, f64::INFINITY);
    let mut b = TextPainter::new()
        .with_text(flat)
        .with_text_direction(TextDirection::Ltr);
    b.layout(0.0, f64::INFINITY);

    assert!(
        (a.width() - b.width()).abs() < 0.5,
        "style inheritance must shape the child at the parent's size \
         ({} vs {})",
        a.width(),
        b.width()
    );
}

#[test]
fn max_lines_truncates_the_shaped_buffer() {
    let layout = TextLayout::with_overflow(
        "Line 1\nLine 2\nLine 3\nLine 4",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
        Some(2),
        None,
    );
    let metrics = layout.metrics();
    assert!(layout.metrics().truncated);
    assert_eq!(
        metrics.line_count, 2,
        "lines beyond max_lines must not exist in the buffer — not \
         merely be skipped at paint"
    );
    let two_line_height = metrics.height;

    let full = TextLayout::new(
        "Line 1\nLine 2\nLine 3\nLine 4",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
    );
    assert!(
        two_line_height < full.metrics().height,
        "the truncated layout's height must shrink with the dropped lines"
    );
}

#[test]
fn ellipsis_fits_within_the_width_constraint() {
    // A long unbroken-ish line forced to wrap at 80px, then truncated
    // to one line with an ellipsis: the kept line (including the
    // ellipsis) must fit the constraint.
    let max_width = 80.0;
    let layout = TextLayout::with_overflow(
        "The quick brown fox jumps over the lazy dog again and again",
        None,
        14.0,
        Some(max_width),
        None,
        TextDirection::Ltr,
        Some(1),
        Some("…"),
    );
    let metrics = layout.metrics();
    assert!(layout.metrics().truncated);
    assert_eq!(metrics.line_count, 1);
    assert!(
        metrics.width <= max_width + 0.5,
        "the ellipsized line must fit the width constraint, got {} > {max_width}",
        metrics.width
    );
}

/// What `paint` records is the layout that was measured — the very `Arc`,
/// not a re-shape — so a truncated paragraph paints exactly the lines it
/// measured, ellipsis included.
#[test]
fn a_truncated_paragraph_paints_exactly_the_lines_it_measured() {
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
#[test]
fn root_recolor_keeps_the_shaped_buffer_and_span_recolor_reshapes_once() {
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
