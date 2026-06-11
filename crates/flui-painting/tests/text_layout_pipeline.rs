//! Integration tests for the TextLayout pipeline wiring.
//!
//! Validates that cosmic-text TextLayout is properly connected to TextPainter
//! and produces correct DrawCommand entries on Canvas. This covers the full
//! measurement -> layout -> paint pipeline.

use flui_painting::{
    Canvas, DisplayListCore, TextPainter, detect_text_direction, measure_inline_span, measure_text,
};
use flui_types::{
    geometry::{Offset, px},
    typography::{
        FontWeight, InlineSpan, TextAlign, TextDirection, TextPosition, TextSpan, TextStyle,
    },
};

// ============================================================================
// measure_text standalone function
// ============================================================================

#[test]
fn measure_text_returns_positive_metrics() {
    let result = measure_text("Hello, World!", None, 16.0, None, None);

    assert!(result.width > 0.0, "width should be positive");
    assert!(result.height > 0.0, "height should be positive");
    assert_eq!(result.line_count, 1);
    assert!(result.alphabetic_baseline > 0.0);
}

#[test]
fn measure_text_wraps_with_constraint() {
    let text = "This is a long sentence that should wrap when given a narrow constraint";

    let unconstrained = measure_text(text, None, 14.0, None, None);
    let constrained = measure_text(text, None, 14.0, Some(80.0), None);

    assert!(
        constrained.line_count > unconstrained.line_count,
        "constrained text should have more lines: {} vs {}",
        constrained.line_count,
        unconstrained.line_count
    );
}

#[test]
fn measure_text_respects_font_size() {
    let small = measure_text("Hello", None, 10.0, None, None);
    let large = measure_text("Hello", None, 30.0, None, None);

    assert!(
        large.width > small.width,
        "larger font should produce wider text"
    );
    assert!(
        large.height > small.height,
        "larger font should produce taller text"
    );
}

#[test]
fn measure_text_empty_string() {
    let result = measure_text("", None, 14.0, None, None);

    // Empty text should still produce a line with height
    assert_eq!(result.line_count, 1);
    assert!(result.height > 0.0);
}

#[test]
fn measure_text_multiline() {
    let result = measure_text("Line 1\nLine 2\nLine 3", None, 14.0, None, None);

    assert_eq!(result.line_count, 3);
}

#[test]
fn measure_text_result_size() {
    let result = measure_text("Test", None, 14.0, None, None);
    let size = result.size();

    assert!((size.width.0 - result.width).abs() < f32::EPSILON);
    assert!((size.height.0 - result.height).abs() < f32::EPSILON);
}

// ============================================================================
// measure_inline_span
// ============================================================================

#[test]
fn measure_inline_span_works() {
    let span = TextSpan::new("Hello, World!");
    let result = measure_inline_span(&InlineSpan::from(span), 14.0, None, 1.0);

    assert!(result.width > 0.0);
    assert!(result.height > 0.0);
    assert_eq!(result.line_count, 1);
}

#[test]
fn measure_inline_span_respects_scale() {
    let normal = measure_inline_span(&InlineSpan::from(TextSpan::new("Hello")), 14.0, None, 1.0);
    let scaled = measure_inline_span(&InlineSpan::from(TextSpan::new("Hello")), 14.0, None, 2.0);

    assert!(
        scaled.width > normal.width,
        "scaled text should be wider: {} vs {}",
        scaled.width,
        normal.width
    );
}

// ============================================================================
// detect_text_direction
// ============================================================================

#[test]
fn detect_direction_ltr() {
    assert_eq!(detect_text_direction("Hello"), Some(TextDirection::Ltr));
}

#[test]
fn detect_direction_rtl() {
    // Arabic text
    assert_eq!(
        detect_text_direction("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}"),
        Some(TextDirection::Rtl)
    );
}

#[test]
fn detect_direction_neutral() {
    // Pure numbers are neutral
    assert_eq!(detect_text_direction("123"), None);
}

// ============================================================================
// TextPainter -> TextLayout -> Canvas pipeline
// ============================================================================

#[test]
fn text_painter_layout_produces_valid_metrics() {
    let span = TextSpan::new("Hello, World!");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    assert!(painter.did_layout());
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

#[test]
fn text_painter_paint_emits_draw_command() {
    let span = TextSpan::new("Hello, World!");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);

    let display_list = canvas.finish();
    assert!(
        display_list.len() > 0,
        "painting should produce at least one draw command"
    );
}

#[test]
fn text_painter_caret_and_hit_test_roundtrip() {
    let span = TextSpan::new("Hello, World!");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Get caret offset at position 5
    let caret = painter.get_offset_for_caret(TextPosition::upstream(5));
    assert!(caret.dx.get() >= 0.0, "caret x should be non-negative");

    // Hit test at that offset should return approximately position 5
    let hit = painter.get_position_for_offset(caret);
    // Allow some tolerance due to glyph boundary rounding
    assert!(
        hit.offset <= 6 && hit.offset >= 4,
        "hit test should return near position 5, got {}",
        hit.offset
    );
}

#[test]
fn text_painter_line_metrics() {
    let span = TextSpan::new("Line 1\nLine 2\nLine 3");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    let metrics = painter.get_line_metrics();
    assert_eq!(metrics.len(), 3, "should have 3 lines");
    assert_eq!(metrics[0].line_number, 0);
    assert_eq!(metrics[1].line_number, 1);
    assert_eq!(metrics[2].line_number, 2);

    // Each line should have positive dimensions
    for m in &metrics {
        assert!(m.height > 0.0);
    }
}

#[test]
fn text_painter_selection_boxes() {
    let span = TextSpan::new("Hello, World!");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    // Select "ello"
    let boxes = painter.get_boxes_for_selection(1, 5);
    assert!(!boxes.is_empty(), "selection should produce boxes");

    let first = &boxes[0];
    assert!(
        first.rect.width().get() > 0.0,
        "selection box should have width"
    );
    assert!(
        first.rect.height().get() > 0.0,
        "selection box should have height"
    );
}

#[test]
fn text_painter_word_boundary() {
    let span = TextSpan::new("Hello World");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 300.0);

    let boundary = painter.get_word_boundary(TextPosition::upstream(2));
    assert!(boundary.start <= 2);
    assert!(boundary.end >= 2);
}

#[test]
fn text_painter_alignment_affects_offset() {
    // Left-aligned
    let mut left = TextPainter::new()
        .with_text(TextSpan::new("Short"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_align(TextAlign::Left);
    left.layout(0.0, 300.0);

    // Right-aligned
    let mut right = TextPainter::new()
        .with_text(TextSpan::new("Short"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_align(TextAlign::Right);
    right.layout(0.0, 300.0);

    // Both should have the same size
    assert!(
        (left.width() - right.width()).abs() < f32::EPSILON,
        "alignment should not change intrinsic width"
    );
}

#[test]
fn text_painter_scale_factor_affects_size() {
    let mut normal = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr);
    normal.layout(0.0, 500.0);

    let mut scaled = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_scale_factor(2.0);
    scaled.layout(0.0, 500.0);

    assert!(
        scaled.width() > normal.width(),
        "2x scale should produce wider text: {} vs {}",
        scaled.width(),
        normal.width()
    );
}

#[test]
fn text_painter_layout_caching() {
    let span = TextSpan::new("Hello");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    // First layout
    painter.layout(0.0, 200.0);
    let w1 = painter.width();

    // Same constraints -- should return cached result
    painter.layout(0.0, 200.0);
    let w2 = painter.width();

    assert!(
        (w1 - w2).abs() < f32::EPSILON,
        "cached layout should return same width"
    );
}

#[test]
fn text_painter_invalidation_on_setter() {
    use flui_painting::Invalidation;

    let span = TextSpan::new("Hello");
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);
    assert!(painter.did_layout());

    // Alignment is a PAINT offset over the shaped lines (the
    // shaped/paint split) — the layout cache survives and only the
    // offset moves. The pre-split contract re-shaped here, which is
    // exactly the Flutter behavior the split exists to beat.
    let inv = painter.set_text_align(TextAlign::Center);
    assert_eq!(inv, Invalidation::Paint);
    assert!(
        painter.did_layout(),
        "an alignment change must keep the shaped layout"
    );

    // A font-size change rewrites glyph geometry — full relayout.
    let inv = painter.set_text(Some(
        TextSpan::new("Hello")
            .with_style(flui_types::typography::TextStyle::new().with_font_size(22.0))
            .into(),
    ));
    assert_eq!(inv, Invalidation::Layout);
    assert!(
        !painter.did_layout(),
        "a layout-affecting change must drop the shaped layout"
    );
}

// ============================================================================
// Full pipeline: measure -> layout -> paint -> display list
// ============================================================================

#[test]
fn full_pipeline_measure_layout_paint() {
    // Step 1: Measure text to determine size
    let text = "The quick brown fox jumps over the lazy dog";
    let metrics = measure_text(text, None, 16.0, Some(200.0), None);
    assert!(metrics.width > 0.0);
    assert!(metrics.line_count >= 1);

    // Step 2: Create TextPainter and layout with the same constraints
    let span = TextSpan::new(text);
    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    // TextPainter's layout should agree with standalone measure_text
    // (both use cosmic-text under the hood)
    let painter_width = painter.width();
    let painter_height = painter.height();
    assert!(painter_width > 0.0);
    assert!(painter_height > 0.0);

    // Step 3: Paint to canvas
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::new(px(10.0), px(20.0)));

    // Step 4: Verify display list has the text command
    let display_list = canvas.finish();
    assert!(
        display_list.len() >= 1,
        "display list should contain text draw command"
    );
}

#[test]
fn full_pipeline_with_styled_text() {
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
    assert!(dl.len() >= 1);
}
