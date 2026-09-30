//! TextLayout unit tests extracted from
//! `crates/flui-painting/src/text_layout/mod.rs` during the text-layout
//! module split.

use flui_painting::TextLayout;
use flui_painting::typography::{TextDirection, TextPosition};

pub(crate) fn test_text_layout_caret_position() {
    let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

    let start_offset = layout.get_offset_for_caret(TextPosition::upstream(0));
    assert!(start_offset.dx >= 0.0);

    let mid_offset = layout.get_offset_for_caret(TextPosition::upstream(2));
    assert!(mid_offset.dx > start_offset.dx);

    let end_offset = layout.get_offset_for_caret(TextPosition::upstream(5));
    assert!(end_offset.dx >= mid_offset.dx);
}

/// A multi-space run is one segment: an offset inside it selects the whole
/// run, and an offset at either edge selects the adjacent word. Pinned
/// against a two-space run specifically, since a three-space run could not
/// distinguish "the whole run" from "a two-space sub-range".
pub(crate) fn get_word_boundary_two_space_run_boundary_matrix() {
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
