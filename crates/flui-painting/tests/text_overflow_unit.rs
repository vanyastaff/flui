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
use flui_types::typography::{TextDirection, TextSpan};

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
    assert!(layout.was_truncated());
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
fn max_lines_within_limit_is_untouched() {
    let layout = TextLayout::with_overflow(
        "Line 1\nLine 2",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
        Some(5),
        Some("…"),
    );
    assert!(!layout.was_truncated());
    assert_eq!(layout.metrics().line_count, 2);
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
    assert!(layout.was_truncated());
    assert_eq!(metrics.line_count, 1);
    assert!(
        metrics.width <= max_width + 0.5,
        "the ellipsized line must fit the width constraint, got {} > {max_width}",
        metrics.width
    );
}

#[test]
fn baselines_come_from_the_shaper() {
    let layout = TextLayout::new("Hello xyj", None, 14.0, None, None, TextDirection::Ltr);
    let metrics = layout.metrics();
    let lines = layout.get_line_metrics();

    // The first line's reported baseline and the layout-level
    // alphabetic baseline are the SAME shaped quantity.
    let first_line = &lines[0];
    assert!(
        (first_line.baseline - f64::from(metrics.alphabetic_baseline)).abs() < 1e-3,
        "line metrics and layout metrics must agree on the baseline"
    );
    // Ascent + descent tile the line box exactly (they are line-box
    // distances around `line_y`, not font-size fractions).
    assert!(
        (first_line.ascent + first_line.descent - first_line.height).abs() < 1e-3,
        "ascent ({}) + descent ({}) must equal the line height ({})",
        first_line.ascent,
        first_line.descent,
        first_line.height
    );
    // Sanity: the baseline sits strictly inside the line box, and the
    // ideographic baseline is at or below the alphabetic one.
    assert!(metrics.alphabetic_baseline > 0.0);
    assert!(f64::from(metrics.alphabetic_baseline) < first_line.height + 1e-3);
    assert!(metrics.ideographic_baseline >= metrics.alphabetic_baseline);
}

#[test]
fn painter_enforces_max_lines_end_to_end() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(
            "one two three four five six seven eight nine",
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));

    painter.layout(0.0, 60.0);

    assert!(painter.did_exceed_max_lines());
    let one_line_height = painter.height();

    let mut unlimited = TextPainter::new()
        .with_text(TextSpan::new(
            "one two three four five six seven eight nine",
        ))
        .with_text_direction(TextDirection::Ltr);
    unlimited.layout(0.0, 60.0);

    assert!(
        one_line_height < unlimited.height(),
        "the painter's reported size must cover only the kept line — \
         detection without enforcement painted every line anyway"
    );
}
