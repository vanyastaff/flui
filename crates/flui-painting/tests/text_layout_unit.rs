//! TextLayout unit tests extracted from
//! `crates/flui-painting/src/text_layout/mod.rs` during the text-layout
//! module split.

use flui_foundation::geometry::Offset;
use flui_painting::TextLayout;
use flui_painting::typography::{TextDirection, TextPosition, TextRange};

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

#[test]
fn test_text_layout_hit_test() {
    let layout = TextLayout::new("Hello", None, 14.0, None, None, TextDirection::Ltr);

    let pos = layout.get_position_for_offset(Offset::new(0.0, 5.0));
    assert_eq!(pos.offset, 0);

    let pos = layout.get_position_for_offset(Offset::new(1000.0, 5.0));
    assert!(pos.offset <= 5);
}

#[test]
fn test_text_layout_selection_boxes() {
    let layout = TextLayout::new("Hello, World!", None, 14.0, None, None, TextDirection::Ltr);

    let boxes = layout.get_boxes_for_range(TextRange::new(1, 5));
    assert!(!boxes.is_empty());

    let first_box = &boxes[0];
    assert!(first_box.rect.width() > 0.0);
    assert!(first_box.rect.height() > 0.0);
}

/// `"don't"` is ONE word under UAX #29's `MidLetter` rule (a straight
/// apostrophe between letters does not break) — the case an
/// ASCII-whitespace scan would also pass (no whitespace to split on
/// either), so this specifically exercises the segmentation crate's own
/// rule, not just whitespace-skipping.
#[test]
fn get_word_boundary_does_not_split_on_an_apostrophe() {
    use flui_painting::TextLayout;
    use flui_painting::typography::{TextAffinity, TextDirection, TextPosition};

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
    use flui_painting::typography::{TextAffinity, TextDirection, TextPosition};

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

/// A position strictly inside a ZWJ family emoji's grapheme cluster must
/// resolve to a segment that CONTAINS the whole cluster, never split it —
/// the painting-layer counterpart of the `flui-widgets::controller`
/// grapheme-boundary guarantees, exercised here through the word-boundary
/// query a double-tap actually calls.
#[test]
fn get_word_boundary_never_splits_inside_a_zwj_emoji_cluster() {
    use flui_painting::TextLayout;
    use flui_painting::typography::{TextAffinity, TextDirection, TextPosition};

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
