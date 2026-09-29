//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{TextDirection, TextSpan};
use flui_painting::{Canvas, TextBaseline, TextPainter};

#[test]
fn test_text_painter_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    assert!(painter.has_layout());
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

#[test]
fn test_text_baseline() {
    assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
}

/// A painted span must contribute its laid-out box to the display list's
/// bounds.
///
/// `DrawTextSpan` used to report no bounds at all, so a picture whose only
/// content was rich text claimed an empty rect: every consumer that culls,
/// computes damage, or sizes a repaint boundary from those bounds would have
/// dropped visible text, and the failure would have read as a culling bug.
/// The rect is the laid-out size, not the ink extent — the same rect Flutter's
/// `RenderBox.paintBounds` (`Offset.zero & size`) reports for a
/// `RenderParagraph`.
#[test]
fn painted_span_contributes_its_laid_out_box_to_display_list_bounds() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, FLUI!"))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(0.0, f64::INFINITY);

    let size = painter.size();
    assert!(
        size.width > 0.0 && size.height > 0.0,
        "precondition: the text must lay out to a non-empty box, got {size:?}"
    );

    let origin = Offset::new(7.0, 11.0);
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, origin);
    let bounds = canvas.finish().bounds().expect("a painted span has bounds");

    // `TextAlign::Start` under an unbounded width leaves the alignment
    // paint-offset at zero, so the box lands exactly at `origin`.
    assert_eq!(bounds.left(), origin.dx);
    assert_eq!(bounds.top(), origin.dy);
    assert_eq!(bounds.width(), size.width);
    assert_eq!(bounds.height(), size.height);
}

#[test]
fn max_lines_with_ellipsis_keeps_positive_min_intrinsic_and_still_truncates_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(
            "a soft wrapping phrase that exceeds one line under a narrow width",
        ))
        .with_text_direction(TextDirection::Ltr)
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));

    let min = painter.min_intrinsic_width();
    let max = painter.max_intrinsic_width();
    assert!(
        min > 0.0,
        "ellipsis + max_lines must not zero min intrinsic, got {min}"
    );
    assert!(min <= max);

    // Dry layout still enforces truncation — distinct from width intrinsics.
    let dry = painter.dry_size(0.0, 40.0);
    assert!(
        dry.width > 0.0 && dry.width <= 40.0 + 0.01,
        "dry layout under narrow width must stay within the constraint, got {}",
        dry.width
    );
    assert!(
        dry.width + 0.01 < max,
        "truncated dry width {} must be narrower than max intrinsic {max}",
        dry.width
    );

    painter.layout(0.0, 40.0);
    assert!(
        painter.did_exceed_max_lines(),
        "committed narrow layout must still enforce max_lines truncation"
    );
    // After layout, cached intrinsics must still ignore the truncation.
    assert!(
        (painter.min_intrinsic_width() - min).abs() < 0.01,
        "layout must not rewrite min intrinsic from the truncating pass"
    );
}

/// When the ellipsis is wider than the text's narrowest run, skipping
/// truncation must not under-report min intrinsic below the ellipsis
/// (Codex review on #1089).
#[test]
fn wide_ellipsis_floors_min_intrinsic_width() {
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

    let text_min = text_only.min_intrinsic_width();
    let ellipsis_width = ellipsis_only.max_intrinsic_width();
    let floored = truncated.min_intrinsic_width();

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
    laid_out.layout(0.0, 200.0);
    assert!(
        (laid_out.min_intrinsic_width() - floored).abs() < 0.01,
        "cached min must keep the ellipsis floor"
    );
}
