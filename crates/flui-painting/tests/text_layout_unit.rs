//! TextLayout unit tests extracted from
//! `crates/flui-painting/src/text_layout/mod.rs` during the text-layout
//! module split.

use flui_painting::TextLayout;
use flui_painting::typography::{TextDirection, TextPosition};

#[test]
fn test_text_layout_caret_position() {
    let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

    let start_offset = layout.get_offset_for_caret(TextPosition::upstream(0));
    assert!(start_offset.dx >= 0.0);

    let mid_offset = layout.get_offset_for_caret(TextPosition::upstream(2));
    assert!(mid_offset.dx > start_offset.dx);

    let end_offset = layout.get_offset_for_caret(TextPosition::upstream(5));
    assert!(end_offset.dx >= mid_offset.dx);
}

/// A multi-space run is still one segment regardless of where inside or
/// at which edge of it `offset` falls — the whitespace-run test
/// ([`get_word_boundary_selects_a_whole_whitespace_run`]) already covers
/// the interior; this pins both edges too, against a two-space run
/// specifically (the three-space one that test already used could not
/// distinguish "the whole run" from "a two-space sub-range").
#[test]
fn get_word_boundary_two_space_run_boundary_matrix() {
    use flui_painting::TextLayout;
    use flui_painting::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new("foo  bar", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let word_at = |offset: usize| {
        layout.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream))
    };

    assert_eq!((word_at(3).start, word_at(3).end), (0, 3), "\"foo\"");
    assert_eq!(
        (word_at(4).start, word_at(4).end),
        (3, 5),
        "inside the gap: the whole two-space run"
    );
    assert_eq!((word_at(5).start, word_at(5).end), (5, 8), "\"bar\"");
}
