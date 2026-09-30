//! Grapheme and word boundaries over ICU4X, the segmenters Parley marks its
//! clusters with (flui-painting `ARCHITECTURE.md`, mapping decision 15).
//!
//! Both segmenters are borrowed from compiled data in a `const` block, so
//! there is no `static` and nothing to build per call. The word segmenter is
//! the one for non-complex scripts: no dictionary or LSTM data, so CJK and
//! Thai text segments per character, as it did before.

use icu_segmenter::options::WordBreakInvariantOptions;
use icu_segmenter::{
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, WordSegmenter,
    WordSegmenterBorrowed,
};

fn graphemes() -> GraphemeClusterSegmenterBorrowed<'static> {
    const { GraphemeClusterSegmenter::new() }
}

fn words() -> WordSegmenterBorrowed<'static> {
    const { WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default()) }
}

/// The grapheme boundaries around `offset` in `text`: `(before, after)`,
/// equal when `offset` is itself a boundary. `offset` is clamped to the text.
pub(crate) fn grapheme_bounds(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let mut before = 0;
    for boundary in graphemes().segment_str(text) {
        if boundary == offset {
            return (offset, offset);
        }
        if boundary > offset {
            return (before, boundary);
        }
        before = boundary;
    }
    (before, text.len())
}

/// `text`'s word segments in order: `(start, end, all whitespace)`.
pub(crate) fn word_segments(text: &str) -> Vec<(usize, usize, bool)> {
    let boundaries: Vec<usize> = words().segment_str(text).collect();
    boundaries
        .windows(2)
        .map(|pair| {
            let (start, end) = (pair[0], pair[1]);
            (
                start,
                end,
                text[start..end].chars().all(char::is_whitespace),
            )
        })
        .collect()
}

/// The word segment at `offset` in `text`, as `(start, end)`.
///
/// `offset` is clamped to the text and snapped down to a char boundary. `0`
/// answers the first segment. Where `offset` sits between two segments:
///
/// - one side whitespace, the other not: the non-whitespace side, whichever
///   side it is on, so a caret right after a word (`"café "` at the space)
///   answers the word just typed and one right before a word (`"foo bar"`
///   at `b`) answers the word about to be typed into;
/// - both sides non-whitespace (`"(foo"` at `(`/`f`, or two CJK characters,
///   each its own segment without a dictionary): the following segment, so
///   a double-tap on the second character of a CJK run selects it.
pub(crate) fn word_at(text: &str, offset: usize) -> (usize, usize) {
    let total = text.len();
    if text.is_empty() {
        return (0, 0);
    }
    let mut offset = offset.min(total);
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let segments = word_segments(text);
    let Some(&(first_start, first_end, _)) = segments.first() else {
        return (total, total);
    };
    if offset == 0 {
        return (first_start, first_end);
    }
    let preceding = segments.iter().find(|&&(_, end, _)| end == offset).copied();
    let following = segments
        .iter()
        .find(|&&(start, _, _)| start == offset)
        .copied();
    match (preceding, following) {
        (Some(_), Some((start, end, false))) => (start, end),
        (Some((start, end, _)), _) => (start, end),
        (None, Some((start, end, _))) => (start, end),
        (None, None) => segments
            .iter()
            .find(|&&(start, end, _)| start < offset && offset < end)
            .map_or((total, total), |&(start, end, _)| (start, end)),
    }
}
