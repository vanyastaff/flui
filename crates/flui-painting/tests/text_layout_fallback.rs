//! Fallback `TextLayout` coverage (no `text` feature).
//!
//! Mirrors the cosmic-text test surface from `text_layout_unit.rs`
//! but exercises the stub implementation in
//! `src/text_layout/fallback.rs`. Runs only when the `text` feature
//! is NOT enabled, so the integration test binary participates in
//! `cargo test --no-default-features -p flui-painting` runs.

#![cfg(not(feature = "text"))]

use flui_painting::{TextLayout, detect_text_direction, measure_inline_span, measure_text};
use flui_types::{
    geometry::Offset,
    typography::{TextDirection, TextPosition, TextRange},
};

#[test]
fn fallback_text_layout_metrics_are_estimated() {
    let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

    let metrics = layout.metrics();

    // Fallback estimates width as `chars * font_size * 0.5`.
    let expected_width = "Hello, World!".chars().count() as f32 * 14.0 * 0.5;
    assert!(
        (metrics.width - expected_width).abs() < f32::EPSILON,
        "fallback width should match `chars * font_size * 0.5`; got {}, expected {}",
        metrics.width,
        expected_width
    );
    assert!(metrics.height > 0.0);
    assert_eq!(metrics.line_count, 1);
    // Default line_height is `font_size * 1.2`; alphabetic baseline
    // is `line_height * 0.8`.
    let expected_baseline = 14.0 * 1.2 * 0.8;
    assert!(
        (metrics.alphabetic_baseline - expected_baseline).abs() < 1e-3,
        "fallback alphabetic_baseline mismatch: got {}, expected {}",
        metrics.alphabetic_baseline,
        expected_baseline
    );
}

#[test]
fn fallback_get_offset_for_caret_steps_by_char_width() {
    let mut layout = TextLayout::new("abcdef", None, 10.0, None, None, TextDirection::Ltr);

    let position = TextPosition::downstream(3);
    let offset = layout.get_offset_for_caret(position);

    // 3 chars at avg-char-width = font_size * 0.5 = 5.0 → x = 15.0.
    assert!(
        (offset.dx.0 - 15.0).abs() < f32::EPSILON,
        "fallback caret should be at x=15.0; got x={}",
        offset.dx.0
    );
    assert!((offset.dy.0 - 0.0).abs() < f32::EPSILON);
}

#[test]
fn fallback_get_position_for_offset_round_trips_char_index() {
    let layout = TextLayout::new("abcdef", None, 10.0, None, None, TextDirection::Ltr);

    // x=25 with avg-char-width=5 → char_index=5.
    let pos = layout.get_position_for_offset(Offset::new(
        flui_types::geometry::Pixels(25.0),
        flui_types::geometry::Pixels(0.0),
    ));
    assert_eq!(pos.offset, 5);
}

#[test]
fn fallback_line_metrics_one_per_line() {
    let layout = TextLayout::new(
        "long text that should wrap several times across the constrained width",
        None,
        14.0,
        Some(60.0),
        None,
        TextDirection::Ltr,
    );

    let line_count = layout.metrics().line_count;
    let line_metrics = layout.get_line_metrics();

    assert_eq!(line_metrics.len(), line_count);
    for (i, line) in line_metrics.iter().enumerate() {
        // Each line's start/end indices are stub-zero in the fallback,
        // but the synthesized line numbering must still progress
        // monotonically.
        assert_eq!(line.line_number, i);
        assert!(line.height > 0.0);
    }
}

#[test]
fn fallback_get_boxes_for_range_returns_single_box() {
    let layout = TextLayout::new("abcdef", None, 10.0, None, None, TextDirection::Ltr);

    let boxes = layout.get_boxes_for_range(TextRange::new(1, 4));
    assert_eq!(
        boxes.len(),
        1,
        "fallback emits exactly one TextBox for the requested range"
    );
}

#[test]
fn fallback_word_boundary_splits_on_space() {
    let layout = TextLayout::new("hello world", None, 10.0, None, None, TextDirection::Ltr);

    let range = layout.get_word_boundary(TextPosition::downstream(2));
    assert_eq!(range.start, 0);
    assert_eq!(range.end, 5);
}

#[test]
fn fallback_detect_text_direction_is_always_ltr() {
    assert_eq!(detect_text_direction("hello"), Some(TextDirection::Ltr));
    // Even with an Arabic string, the fallback can't detect RTL.
    assert_eq!(detect_text_direction("مرحبا"), Some(TextDirection::Ltr));
}

#[test]
fn fallback_measure_text_estimates_lines_from_constraint() {
    let unconstrained = measure_text("aaaaaaaaaaaaaaaaaaaa", None, 10.0, None, None);
    let constrained = measure_text("aaaaaaaaaaaaaaaaaaaa", None, 10.0, Some(50.0), None);

    assert_eq!(unconstrained.line_count, 1);
    assert!(constrained.line_count >= unconstrained.line_count);
}

#[test]
fn fallback_measure_inline_span_proxies_to_measure_text() {
    use flui_types::typography::{InlineSpan, TextSpan};

    let span = InlineSpan::new(TextSpan::new("abcdef"));
    let result = measure_inline_span(&span, 14.0, None, 1.0);

    let expected = measure_text("abcdef", None, 14.0, None, None);
    assert!((result.width - expected.width).abs() < f32::EPSILON);
}
