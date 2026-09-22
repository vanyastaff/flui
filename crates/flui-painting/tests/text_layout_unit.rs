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

/// A run of plain whitespace is one UAX #29 segment, not a sequence of
/// one-byte gaps — a double-tap landing inside a multi-space run selects
/// the whole run, matching what most editors do.
#[test]
fn get_word_boundary_selects_a_whole_whitespace_run() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new("foo   bar", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    // Cursor in the middle of the three-space gap (byte 4).
    let pos = TextPosition::new(4, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert_eq!(word.start, 3, "the whole gap, not one space");
    assert_eq!(word.end, 6, "the whole gap, not one space");
}

/// `"don't"` is ONE word under UAX #29's `MidLetter` rule (a straight
/// apostrophe between letters does not break) — the case an
/// ASCII-whitespace scan would also pass (no whitespace to split on
/// either), so this specifically exercises the segmentation crate's own
/// rule, not just whitespace-skipping.
#[test]
fn get_word_boundary_does_not_split_on_an_apostrophe() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new("don't stop", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let pos = TextPosition::new(3, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert_eq!(word.start, 0);
    assert_eq!(word.end, 5, "the whole word \"don't\", apostrophe included");
}

/// UAX #29 word segmentation finds an internal boundary in CJK text even
/// though it contains no ASCII whitespace at all — the case the previous
/// ASCII-whitespace-run implementation could not pass by construction:
/// with nothing to split on, it would return the entire line as one
/// "word" (the same degenerate shape
/// `get_word_boundary_returns_word_not_whole_line` guards against, here
/// triggered by script rather than by its glyph-count bug).
#[test]
fn get_word_boundary_finds_an_internal_boundary_in_cjk_text_with_no_whitespace() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let text = "日本語のテスト"; // "Japanese test" -- no ASCII whitespace anywhere.
    let layout = TextLayout::new(text, None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let pos = TextPosition::new(0, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert!(
        word.end < text.len(),
        "a script-aware segmenter must stop before the end of the line, not return the whole \
         line the way the old ASCII-whitespace scan did"
    );
}
