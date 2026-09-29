//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use flui_painting::TextPainter;
use flui_painting::typography::{TextDirection, TextSpan};

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
