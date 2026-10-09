//! The text pipeline end to end: a styled span measured by `TextPainter`
//! records a paragraph on the canvas.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, TextPainter};

fn invalid_font_size_does_not_unwind(font_size: f64, scale: f64) {
    let fonts = flui_painting::FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/Roboto-Regular.ttf"))
        .expect("the fixture font loads");
    let mut context = flui_painting::TextContext::new(&fonts);
    let valid = TextSpan::styled("AAA", TextStyle::new().with_font_size(16.0));
    let mut painter = TextPainter::new()
        .with_text(valid.clone())
        .with_text_direction(TextDirection::Ltr)
        .with_text_scale_factor(scale);
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("valid initial layout");
    let expected = painter.size();
    painter.set_text(Some(
        TextSpan::styled("AAA", TextStyle::new().with_font_size(font_size)).into(),
    ));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        painter.layout(&mut context, 0.0, 400.0)
    }));
    let result = result.expect("an invalid authored/resolved size must not panic");
    assert!(matches!(
        result,
        Err(flui_painting::TextLayoutError::InvalidFontSize { .. })
    ));
    assert!(
        !painter.has_layout(),
        "failed text cannot expose current geometry"
    );
    painter.set_text(Some(valid.into()));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("layout recovers after invalid input");
    assert_eq!(painter.size(), expected);
    assert!(matches!(
        painter.layout(&mut context, 0.0, f64::MAX),
        Err(flui_painting::TextLayoutError::InvalidWidth { .. })
    ));
    assert!(
        !painter.has_layout(),
        "failed constraints cannot retain the previous cache"
    );
    for (min_width, max_width) in [(-1e-50, 400.0), (0.0, -1e-50)] {
        painter
            .layout(&mut context, 0.0, 400.0)
            .expect("valid layout before a negative subnormal width");
        assert!(
            matches!(
                painter.layout(&mut context, min_width, max_width),
                Err(flui_painting::TextLayoutError::InvalidWidth { .. })
            ),
            "a negative width must not become accepted negative zero after narrowing"
        );
        assert!(!painter.has_layout());
    }
}

pub(crate) fn authored_font_size_overflow_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(f64::MAX, 1.0);
}

pub(crate) fn scaled_font_size_overflow_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(f64::MAX, 64.0);
}

pub(crate) fn font_size_narrowing_to_zero_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(1e-50, 1.0);
}

pub(crate) fn changing_to_an_invalid_scale_cannot_reuse_successful_geometry() {
    let fonts = flui_painting::FontCollection::new();
    let mut context = flui_painting::TextContext::new(&fonts);
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("AAA"))
        .with_text_direction(TextDirection::Ltr);
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        painter.set_text_scale_factor(1.0);
        painter
            .layout(&mut context, 0.0, 400.0)
            .expect("valid baseline layout");
        painter.set_text_scale_factor(factor);
        assert!(
            matches!(
                painter.layout(&mut context, 0.0, 400.0),
                Err(flui_painting::TextLayoutError::InvalidScale { .. })
            ),
            "an invalid authored scale must be rejected: {factor}"
        );
        assert!(!painter.has_layout());
    }
}

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

// ============================================================================
// measure_text standalone function
// ============================================================================

// ============================================================================
// Full pipeline: measure -> layout -> paint -> display list
// ============================================================================

pub(crate) fn full_pipeline_with_styled_text() {
    let style = TextStyle::new()
        .with_font_size(20.0)
        .with_font_weight(FontWeight::BOLD);

    let span = TextSpan::new("Styled text").with_style(style);

    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter
        .layout(&mut text_cx(), 0.0, 400.0)
        .expect("valid fixture lays out");
    assert!(painter.width() > 0.0);

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);

    let dl = canvas.finish();
    assert!(!dl.is_empty());
}
