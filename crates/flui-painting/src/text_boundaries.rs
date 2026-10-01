//! Grapheme and word boundaries over ICU4X, the segmenters Parley marks its
//! clusters with (ADR-0092 §6; `ARCHITECTURE.md`, mapping decision 15).
//!
//! Hit-testing and carets snap to these boundaries, and an editor that steps
//! its caret through them moves the way the painted text is clustered:
//! one user-perceived character (an extended grapheme cluster, UAX #29) at a
//! time, and one word segment (UAX #29 word boundaries) at a time.
//!
//! Offsets are UTF-8 byte offsets into the text; every boundary is a `char`
//! boundary. ICU4X segments forward only, so a query near an offset starts
//! at the beginning of the line that holds it, just past the last LF before
//! it: a break after LF is mandatory for graphemes (GB4) and words (WB3a), so
//! nothing before it changes the answer, and a query costs the length of its
//! line, not of the text.
//!
//! Both segmenters are borrowed from compiled data in a `const` block, so
//! there is no `static` and nothing to build per call. The word segmenter is
//! the one for non-complex scripts: no dictionary or LSTM data, so CJK and
//! Thai text segments per character.

use std::ops::Range;

use icu_segmenter::options::WordBreakInvariantOptions;
use icu_segmenter::{
    GraphemeClusterSegmenter, GraphemeClusterSegmenterBorrowed, WordSegmenter,
    WordSegmenterBorrowed,
};

fn grapheme_segmenter() -> GraphemeClusterSegmenterBorrowed<'static> {
    const { GraphemeClusterSegmenter::new() }
}

fn word_segmenter() -> WordSegmenterBorrowed<'static> {
    const { WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default()) }
}

/// The start of the line that holds byte `at`: just past the last LF before
/// it, or `0`. Always a grapheme and a word boundary.
fn line_start(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |lf| lf + 1)
}

/// The grapheme boundaries of `text` from `from`, a boundary, as offsets
/// into `text`.
fn grapheme_boundaries_from(text: &str, from: usize) -> impl Iterator<Item = usize> + '_ {
    grapheme_segmenter()
        .segment_str(&text[from..])
        .map(move |boundary| from + boundary)
}

/// The grapheme boundary before `offset`: the start of the extended grapheme
/// cluster that ends at, or holds, `offset`. `0` at the start.
///
/// `offset` is clamped to the text and need not be a boundary, or even a
/// `char` boundary: inside a cluster it answers that cluster's start.
#[must_use]
pub fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == 0 {
        return 0;
    }
    let from = line_start(text, offset - 1);
    grapheme_boundaries_from(text, from)
        .take_while(|&boundary| boundary < offset)
        .last()
        .unwrap_or(from)
}

/// The grapheme boundary after `offset`: the end of the extended grapheme
/// cluster that starts at, or holds, `offset`. The text's length at its end.
///
/// `offset` is clamped to the text and need not be a boundary, or even a
/// `char` boundary: inside a cluster it answers that cluster's end.
#[must_use]
pub fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if offset == text.len() {
        return offset;
    }
    grapheme_boundaries_from(text, line_start(text, offset))
        .find(|&boundary| boundary > offset)
        .unwrap_or(text.len())
}

/// Whether `offset` is an extended grapheme cluster boundary of `text`: its
/// start, its end, or between two clusters. An offset past the end is not.
#[must_use]
pub fn is_grapheme_boundary(text: &str, offset: usize) -> bool {
    if offset == 0 || offset == text.len() {
        return true;
    }
    offset < text.len()
        && grapheme_boundaries_from(text, line_start(text, offset))
            .find(|&boundary| boundary >= offset)
            == Some(offset)
}

/// The byte range of each extended grapheme cluster of `text`, in order.
pub fn graphemes(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    pairs(grapheme_segmenter().segment_str(text))
}

/// One UAX #29 word segment: a word, a run of whitespace, or a punctuation
/// character, as [`word_segments`] yields them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WordSegment {
    /// Byte offset of the segment's start.
    pub start: usize,
    /// Byte offset just past the segment's end.
    pub end: usize,
    /// Whether every character of the segment is whitespace (a line break
    /// included).
    pub whitespace: bool,
}

/// `text`'s UAX #29 word segments, in order, covering the whole text.
///
/// A run of horizontal whitespace is one segment, and so is each line
/// break; a word stays one segment across an apostrophe (`don't`) and from
/// letters into digits (`foo123`).
pub fn word_segments(text: &str) -> impl Iterator<Item = WordSegment> + '_ {
    word_segments_at(text, 0)
}

/// `text`'s word segments from the start of the line that holds byte `at`
/// (just past the last LF before it, or `0`): the same segments
/// [`word_segments`] yields from there on.
///
/// For a query near `at` that should cost the length of its line, not of the
/// text: the segments before that line cannot change these.
pub fn word_segments_from(text: &str, at: usize) -> impl Iterator<Item = WordSegment> + '_ {
    word_segments_at(text, line_start(text, at))
}

fn word_segments_at(text: &str, from: usize) -> impl Iterator<Item = WordSegment> + '_ {
    pairs(word_segmenter().segment_str(&text[from..])).map(move |range| {
        let (start, end) = (from + range.start, from + range.end);
        WordSegment {
            start,
            end,
            whitespace: text[start..end].chars().all(char::is_whitespace),
        }
    })
}

/// Consecutive boundaries as ranges: `[0, 3, 5]` gives `0..3` and `3..5`.
fn pairs(mut boundaries: impl Iterator<Item = usize>) -> impl Iterator<Item = Range<usize>> {
    let mut start = boundaries.next();
    std::iter::from_fn(move || {
        let end = boundaries.next()?;
        let range = start?..end;
        start = Some(end);
        Some(range)
    })
}

/// The grapheme boundaries around `offset` in `text`: `(before, after)`,
/// equal when `offset` is itself a boundary. `offset` is clamped to the text.
pub(crate) fn grapheme_bounds(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    if is_grapheme_boundary(text, offset) {
        (offset, offset)
    } else {
        (
            previous_grapheme_boundary(text, offset),
            next_grapheme_boundary(text, offset),
        )
    }
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
    let mut offset = offset.min(total);
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    if offset == 0 {
        return word_segments(text)
            .next()
            .map_or((total, total), |segment| (segment.start, segment.end));
    }
    let (mut preceding, mut following, mut holding) = (None, None, None);
    for segment in word_segments_from(text, offset - 1) {
        if segment.start > offset {
            break;
        }
        if segment.end == offset {
            preceding = Some(segment);
        } else if segment.start == offset {
            following = Some(segment);
        } else if segment.start < offset && offset < segment.end {
            holding = Some(segment);
        }
    }
    let chosen = match (preceding, following) {
        (Some(_), Some(after)) if !after.whitespace => Some(after),
        (Some(before), _) => Some(before),
        (None, Some(after)) => Some(after),
        (None, None) => holding,
    };
    chosen.map_or((total, total), |segment| (segment.start, segment.end))
}
