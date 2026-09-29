//! Integration tests for the TextLayout pipeline wiring.
//!
//! Validates that cosmic-text TextLayout is properly connected to TextPainter
//! and produces correct DrawCommand entries on Canvas. This covers the full
//! measurement -> layout -> paint pipeline.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, TextPainter};

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

    painter.layout(0.0, 400.0);
    assert!(painter.width() > 0.0);

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);

    let dl = canvas.finish();
    assert!(!dl.is_empty());
}
