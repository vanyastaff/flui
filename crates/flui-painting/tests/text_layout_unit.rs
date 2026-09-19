//! TextLayout unit tests extracted from
//! `crates/flui-painting/src/text_layout/mod.rs` during the text-layout
//! module split.

use flui_painting::TextLayout;
use flui_types::{
    geometry::{Offset, px},
    typography::{TextDirection, TextPosition, TextRange},
};

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
    let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

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

/// Regression test for the `get_word_boundary` O(n²) ASCII-degenerate
/// bug (Copilot PR #80 comment #3273541280).
///
/// The previous implementation pushed every `glyph.start` into a
/// `Vec<usize>` then called `char_positions.contains(&i)` for each `i`
/// in a left/right expansion loop. For ASCII text every byte index
/// matched a glyph start, so the function returned `(0, line_len)` —
/// the entire line — for every cursor position, instead of the
/// non-whitespace run around the cursor.
#[test]
fn get_word_boundary_returns_word_not_whole_line() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new(
        "the quick brown fox",
        None,
        14.0,
        None,
        None,
        TextDirection::Ltr,
    );
    let _ = layout.metrics(); // ensure shaped.

    // Cursor inside "quick" (byte offset 6 = inside 'q-u-i-c-k').
    let pos = TextPosition::new(6, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert_eq!(word.start, 4, "word should start at 'q' (byte 4)");
    assert_eq!(word.end, 9, "word should end after 'k' (byte 9)");
}

/// Confirms `get_word_boundary` does not split inside multi-byte
/// UTF-8 codepoints.
#[test]
fn get_word_boundary_handles_non_ascii() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    // "café" — 'é' is 2 bytes (0xC3 0xA9). Total len = 5 bytes.
    let layout = TextLayout::new("café world", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    // Cursor right after "café" (byte 5).
    let pos = TextPosition::new(5, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert_eq!(word.start, 0, "word should start at 'c' (byte 0)");
    assert_eq!(word.end, 5, "word should end after 'é' (byte 5)");
}
