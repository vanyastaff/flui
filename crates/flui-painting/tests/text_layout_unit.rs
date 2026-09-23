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
///
/// The exact boundary is pinned, not just "not the whole line": without a
/// `cjdict`-style dictionary (which this crate does not have — see
/// `flui-widgets/ARCHITECTURE.md`'s Mapping decision), the rule-based
/// default segments Han per character, so `"日本語のテスト"` ("Japanese
/// test") splits after the FIRST character `日` (3 UTF-8 bytes) rather
/// than staying one word — a known limitation, asserted here so a future
/// dictionary integration has a failing test to flip, not a silently
/// stale comment.
#[test]
fn get_word_boundary_splits_cjk_per_character_not_per_word() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let text = "日本語のテスト"; // "Japanese test" -- no ASCII whitespace anywhere.
    let layout = TextLayout::new(text, None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let pos = TextPosition::new(0, TextAffinity::Downstream);
    let word = layout.get_word_boundary(pos);

    assert_eq!(word.start, 0);
    assert_eq!(
        word.end, 3,
        "日 alone (3 UTF-8 bytes), not the whole word \"日本語\""
    );
}

/// The boundary tie-break matrix `get_word_boundary`'s own doc names: a
/// WORD segment wins over an adjacent WHITESPACE one at an exact
/// boundary, regardless of which side of the offset it is on.
#[test]
fn get_word_boundary_prefers_the_word_side_of_a_boundary_over_the_whitespace_side() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new("foo bar", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let word_at = |offset: usize| {
        layout.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream))
    };

    // Offset 0: always the first segment.
    assert_eq!((word_at(0).start, word_at(0).end), (0, 3), "\"foo\"");
    // Offset 3: the boundary between "foo" (word) and the space
    // (whitespace) -- the word wins, even though the space is what
    // FOLLOWS this offset.
    assert_eq!((word_at(3).start, word_at(3).end), (0, 3), "\"foo\"");
    // Offset 4: the boundary between the space (whitespace) and "bar"
    // (word) -- the word wins again, even though it FOLLOWS this offset
    // rather than precedes it. This is the case a plain
    // prefer-the-preceding-segment rule gets wrong.
    assert_eq!((word_at(4).start, word_at(4).end), (4, 7), "\"bar\"");
    // Offset 7 (end of buffer): the only candidate is "bar".
    assert_eq!((word_at(7).start, word_at(7).end), (4, 7), "\"bar\"");
}

/// At a boundary between TWO non-whitespace segments, the FOLLOWING one
/// wins (downstream affinity) -- the case
/// [`get_word_boundary_prefers_the_word_side_of_a_boundary_over_the_whitespace_side`]
/// doesn't cover, since one whitespace-vs-word rule cannot answer a
/// punctuation-vs-word or CJK-character-vs-CJK-character boundary. This
/// is the practical bug this test guards: without it, a double-tap
/// landing exactly on a CJK character's leading edge (a real, common
/// case for scripts with no dictionary segmentation, see the
/// per-character-CJK test) selects the character BEFORE it instead.
#[test]
fn get_word_boundary_prefers_the_following_segment_between_two_word_segments() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let word_at = |layout: &TextLayout, offset: usize| {
        layout.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream))
    };

    let paren_foo = TextLayout::new("(foo", None, 14.0, None, None, TextDirection::Ltr);
    let _ = paren_foo.metrics();
    let word = word_at(&paren_foo, 1);
    assert_eq!(
        (word.start, word.end),
        (1, 4),
        "\"(\"/\"foo\" boundary: \"foo\" wins, not \"(\""
    );

    let cjk = TextLayout::new("日本語", None, 14.0, None, None, TextDirection::Ltr);
    let _ = cjk.metrics();
    let word = word_at(&cjk, 3);
    assert_eq!(
        (word.start, word.end),
        (3, 6),
        "日/本 boundary: 本 wins, not 日 -- a double-tap landing on 本 must select 本"
    );
}

/// A word-vs-punctuation boundary is still a word-vs-word case for this
/// tie-break (punctuation is simply not whitespace): both sides of
/// `"foo, bar"`'s `,` land on the comma, for different reasons depending
/// on which boundary is probed.
#[test]
fn get_word_boundary_word_vs_punctuation_boundary() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let layout = TextLayout::new("foo, bar", None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let word_at = |offset: usize| {
        layout.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream))
    };

    // "foo"/"," boundary: both non-whitespace, the FOLLOWING one (",")
    // wins.
    let word = word_at(3);
    assert_eq!((word.start, word.end), (3, 4), "\",\"");
    // ","/" " boundary: "," is not whitespace, " " is -- "," wins as the
    // non-whitespace side, same rule as the plain word-vs-whitespace
    // matrix, just landing on the SAME range as the case above by
    // coincidence of this string.
    let word = word_at(4);
    assert_eq!((word.start, word.end), (3, 4), "\",\"");
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
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

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

/// Leading and trailing whitespace at the buffer's own edges: `0` always
/// answers with the first segment regardless of whether it is
/// whitespace (unaffected by the word-vs-whitespace tie-break, which
/// only applies to an INTERIOR boundary between two segments); the very
/// end of a buffer that ends in whitespace answers with that trailing
/// run, since there is no following segment to prefer instead.
#[test]
fn get_word_boundary_at_the_buffers_own_leading_and_trailing_whitespace() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let leading = TextLayout::new("  foo", None, 14.0, None, None, TextDirection::Ltr);
    let _ = leading.metrics();
    let word = leading.get_word_boundary(TextPosition::new(0, TextAffinity::Downstream));
    assert_eq!(
        (word.start, word.end),
        (0, 2),
        "offset 0 is always the first segment, even when it is whitespace"
    );

    let trailing = TextLayout::new("foo ", None, 14.0, None, None, TextDirection::Ltr);
    let _ = trailing.metrics();
    let word = trailing.get_word_boundary(TextPosition::new(4, TextAffinity::Downstream));
    assert_eq!(
        (word.start, word.end),
        (3, 4),
        "the end of the buffer with no following segment answers with the \
         trailing whitespace run itself"
    );
}

/// A position strictly inside a ZWJ family emoji's grapheme cluster must
/// resolve to a segment that CONTAINS the whole cluster, never split it —
/// the painting-layer counterpart of the `flui-widgets::controller`
/// grapheme-boundary guarantees, exercised here through the word-boundary
/// query a double-tap actually calls.
#[test]
fn get_word_boundary_never_splits_inside_a_zwj_emoji_cluster() {
    use flui_painting::TextLayout;
    use flui_types::typography::{TextAffinity, TextDirection, TextPosition};

    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}"; // family emoji, one grapheme
    let text = format!("hi {family} bye");
    let layout = TextLayout::new(&text, None, 14.0, None, None, TextDirection::Ltr);
    let _ = layout.metrics();

    let cluster_start = "hi ".len();
    let cluster_end = cluster_start + family.len();

    // Probe every byte strictly inside the cluster, not just its edges.
    for offset in (cluster_start + 1)..cluster_end {
        if !text.is_char_boundary(offset) {
            continue;
        }
        let word = layout.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream));
        assert!(
            word.start <= cluster_start && word.end >= cluster_end,
            "offset {offset} inside the cluster must resolve to a segment covering the whole \
             cluster [{cluster_start}, {cluster_end}), got [{}, {})",
            word.start,
            word.end
        );
    }
}
