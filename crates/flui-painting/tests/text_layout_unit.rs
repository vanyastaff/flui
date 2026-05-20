//! TextLayout unit tests extracted from
//! `crates/flui-painting/src/text_layout/mod.rs` during Mythos chain U8.

#![cfg(feature = "text")]

use flui_painting::{
    LineInfo, TextLayout, detect_text_direction, measure_inline_span, measure_text,
};
use flui_types::{
    geometry::{Offset, px},
    typography::{TextDirection, TextPosition, TextRange},
};

#[test]
fn test_measure_simple_text() {
    let result = measure_text("Hello, World!", None, 14.0, None, None);

    assert!(result.width > 0.0);
    assert!(result.height > 0.0);
    assert_eq!(result.line_count, 1);
}

#[test]
fn test_measure_multiline_text() {
    let result = measure_text("Line 1\nLine 2\nLine 3", None, 14.0, None, None);

    assert!(result.height > 0.0);
    assert_eq!(result.line_count, 3);
}

#[test]
fn test_measure_with_width_constraint() {
    let text = "This is a very long line of text that should wrap when constrained";

    let unconstrained = measure_text(text, None, 14.0, None, None);
    let constrained = measure_text(text, None, 14.0, Some(100.0), None);

    assert!(constrained.line_count >= unconstrained.line_count);
    assert!(constrained.max_line_width <= 100.0 + 1.0);
}

#[test]
fn test_measure_empty_text() {
    let result = measure_text("", None, 14.0, None, None);

    assert_eq!(result.line_count, 1);
    assert!(result.height > 0.0);
}

#[test]
fn test_text_layout_creation() {
    let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

    let metrics = layout.metrics();
    assert!(metrics.width > 0.0);
    assert!(metrics.height > 0.0);
    assert_eq!(metrics.line_count, 1);
}

#[test]
fn test_text_layout_caret_position() {
    let mut layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

    let start_offset = layout.get_offset_for_caret(TextPosition::upstream(0));
    assert!(start_offset.dx >= px(0.0));

    let mid_offset = layout.get_offset_for_caret(TextPosition::upstream(2));
    assert!(mid_offset.dx > start_offset.dx);

    let end_offset = layout.get_offset_for_caret(TextPosition::upstream(5));
    assert!(end_offset.dx >= mid_offset.dx);
}

#[test]
fn test_text_layout_hit_test() {
    let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

    let pos = layout.get_position_for_offset(Offset::new(px(0.0), px(5.0)));
    assert_eq!(pos.offset, 0);

    let pos = layout.get_position_for_offset(Offset::new(px(1000.0), px(5.0)));
    assert!(pos.offset <= 5);
}

#[test]
fn test_text_layout_line_metrics() {
    let layout = TextLayout::new("Line 1\nLine 2", None, 14.0, None, None, TextDirection::Ltr);

    let metrics = layout.get_line_metrics();
    assert_eq!(metrics.len(), 2);

    assert_eq!(metrics[0].line_number, 0);
    assert!(metrics[0].width > 0.0);

    assert_eq!(metrics[1].line_number, 1);
}

#[test]
fn test_text_layout_selection_boxes() {
    let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

    let boxes = layout.get_boxes_for_range(TextRange::new(1, 5));
    assert!(!boxes.is_empty());

    let first_box = &boxes[0];
    assert!(first_box.rect.width() > px(0.0));
    assert!(first_box.rect.height() > px(0.0));
}

#[test]
fn test_text_layout_word_boundary() {
    let layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);

    let boundary = layout.get_word_boundary(TextPosition::upstream(2));
    assert!(boundary.start <= 2);
    assert!(boundary.end >= 2);
}

#[test]
fn test_detect_text_direction_ltr() {
    assert_eq!(detect_text_direction("Hello"), Some(TextDirection::Ltr));
    assert_eq!(detect_text_direction("Привет"), Some(TextDirection::Ltr));
    assert_eq!(detect_text_direction("日本語"), Some(TextDirection::Ltr));
}

#[test]
fn test_detect_text_direction_rtl() {
    assert_eq!(detect_text_direction("مرحبا"), Some(TextDirection::Rtl));
    assert_eq!(detect_text_direction("שלום"), Some(TextDirection::Rtl));
}

#[test]
fn test_detect_text_direction_neutral() {
    assert_eq!(detect_text_direction("123"), None);
    assert_eq!(detect_text_direction("   "), None);
    assert_eq!(detect_text_direction("!@#$%"), None);
}

#[test]
fn test_detect_text_direction_mixed() {
    assert_eq!(detect_text_direction("123 Hello"), Some(TextDirection::Ltr));
    assert_eq!(detect_text_direction("123 مرحبا"), Some(TextDirection::Rtl));
}

#[test]
fn test_line_info_ltr() {
    let layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);

    let info = layout.get_line_info();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].line_number, 0);
    assert!(!info[0].is_rtl);
    assert_eq!(info[0].direction(), TextDirection::Ltr);
}

#[test]
fn test_line_info_rtl() {
    let layout = TextLayout::new("مرحبا بالعالم", None, 14.0, None, None, TextDirection::Rtl);

    let info = layout.get_line_info();
    assert_eq!(info.len(), 1);
}

#[test]
fn test_has_rtl_content() {
    let ltr_layout = TextLayout::new("Hello World", None, 14.0, None, None, TextDirection::Ltr);
    assert!(!ltr_layout.has_rtl_content());
}

#[test]
fn test_line_info_methods() {
    let info = LineInfo {
        line_number: 0,
        is_rtl: true,
        width: 100.0,
        height: 20.0,
        top: 0.0,
        start_index: 0,
        end_index: 10,
    };

    assert_eq!(info.direction(), TextDirection::Rtl);
    assert!((info.bottom() - 20.0).abs() < f32::EPSILON);
    assert_eq!(info.len(), 10);
    assert!(!info.is_empty());
}

#[test]
fn test_line_info_empty() {
    let info = LineInfo {
        line_number: 0,
        is_rtl: false,
        width: 0.0,
        height: 20.0,
        top: 0.0,
        start_index: 5,
        end_index: 5,
    };

    assert!(info.is_empty());
    assert_eq!(info.len(), 0);
}

#[test]
fn test_measure_inline_span_smoke() {
    use flui_types::typography::{InlineSpan, TextSpan};

    let span: InlineSpan = TextSpan::new("Hello").into();
    let result = measure_inline_span(&span, 14.0, None, 1.0);
    assert!(result.width > 0.0);
    assert_eq!(result.line_count, 1);
}
