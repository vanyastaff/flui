//! TextPainter unit tests extracted from
//! `crates/flui-painting/src/text_painter/mod.rs` during the text-painter
//! module split.

use flui_painting::{DEFAULT_FONT_SIZE, TextBaseline, TextPainter};
use flui_types::{
    geometry::{Offset, px},
    typography::{TextAlign, TextDirection, TextPosition, TextSpan},
};

#[test]
fn test_text_painter_new() {
    let painter = TextPainter::new();
    assert!(painter.text().is_none());
    assert_eq!(painter.text_align(), TextAlign::Start);
    assert!(painter.text_direction().is_none());
}

#[test]
fn test_text_painter_builder() {
    let painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_align(TextAlign::Center);

    assert!(painter.text().is_some());
    assert_eq!(painter.text_align(), TextAlign::Center);
    assert_eq!(painter.text_direction(), Some(TextDirection::Ltr));
}

#[test]
fn test_text_painter_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    assert!(painter.did_layout());
    assert!(painter.width() > 0.0);
    assert!(painter.height() > 0.0);
}

#[test]
fn test_text_painter_setters_invalidate_layout() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);
    assert!(painter.did_layout());

    // Alignment is a paint offset over the shaped lines (shaped/paint
    // split) — the layout cache survives the change.
    painter.set_text_align(TextAlign::Center);
    assert!(painter.did_layout());

    // Layout-affecting setters still drop the cache.
    painter.set_max_lines(Some(1));
    assert!(!painter.did_layout());
}

#[test]
fn test_text_painter_max_lines() {
    let painter = TextPainter::new().with_max_lines(Some(3));

    assert_eq!(painter.max_lines(), Some(3));
}

#[test]
fn test_text_baseline() {
    assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
}

#[test]
fn test_default_font_size() {
    assert!((DEFAULT_FONT_SIZE - 14.0).abs() < f32::EPSILON);
}

#[test]
fn test_get_offset_for_caret() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    let start = painter.get_offset_for_caret(TextPosition::upstream(0));
    let mid = painter.get_offset_for_caret(TextPosition::upstream(5));
    let end = painter.get_offset_for_caret(TextPosition::upstream(13));

    assert!(start.dx >= px(0.0));
    assert!(mid.dx > start.dx);
    assert!(end.dx > mid.dx);
}

#[test]
fn test_get_position_for_offset() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    let pos = painter.get_position_for_offset(Offset::new(px(0.0), px(5.0)));
    assert_eq!(pos.offset, 0);

    let pos = painter.get_position_for_offset(Offset::new(px(1000.0), px(5.0)));
    assert!(pos.offset <= 5);
}

#[test]
fn test_get_line_metrics() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Line 1\nLine 2"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    let metrics = painter.get_line_metrics();
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0].line_number, 0);
    assert_eq!(metrics[1].line_number, 1);
}

#[test]
fn test_get_boxes_for_selection() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello, World!"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    let boxes = painter.get_boxes_for_selection(1, 5);
    assert!(!boxes.is_empty());

    let first_box = &boxes[0];
    assert!(first_box.rect.width() > px(0.0));
    assert!(first_box.rect.height() > px(0.0));
}

#[test]
fn test_get_word_boundary() {
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("Hello World"))
        .with_text_direction(TextDirection::Ltr);

    painter.layout(0.0, 200.0);

    let boundary = painter.get_word_boundary(TextPosition::upstream(2));
    assert!(boundary.start <= 2);
    assert!(boundary.end >= 2);
}
