//! Laid-out text as runs for assistive technology (ADR-0204 §2): one run per
//! stretch of one direction on one line, cut where AccessKit's limits
//! require, with the word starts the caret steps between. Built the same way
//! for a field and for selectable static text (ADR-0214 §2).

use std::ops::Range;

use flui_foundation::geometry::Rect;
use flui_painting::text_boundaries::word_starts;
use flui_painting::typography::{TextCharacter, TextDirection, TextLineLayout, TextLineRun};
use flui_rendering::semantics::{MAX_RUN_CHARACTERS, TextRun, TextRunError, TextRunParts};

use crate::text::paragraph::semantics_text_direction;

/// The most bytes one character of a run may hold: AccessKit counts a
/// character's length in a `u8`, so a longer cluster is published as pieces.
const MAX_CHARACTER_BYTES: usize = u8::MAX as usize;

/// One published character: its bytes of the text and its horizontal span.
struct Piece {
    range: Range<usize>,
    left: f64,
    right: f64,
}

/// `text`'s runs as `lines` lay it out in `direction`, shifted left by
/// `scroll_x`. An empty line publishes one empty run, bounded by
/// `empty_rect` of the line's start and the line itself, where the caret on
/// it sits. No run crosses a byte of `cuts` (sorted), such as a link's
/// edge.
pub(crate) fn text_runs(
    text: &str,
    lines: &[TextLineLayout],
    direction: TextDirection,
    scroll_x: f64,
    empty_rect: impl Fn(usize, &TextLineLayout) -> Rect,
    cuts: &[usize],
) -> Result<Vec<TextRun>, TextRunError> {
    let words: Vec<usize> = word_starts(text).collect();
    let mut runs = Vec::new();
    for (line, layout) in lines.iter().enumerate() {
        if layout.runs.is_empty() {
            runs.push(TextRun::try_new(TextRunParts::new(
                "",
                layout.start,
                line,
                semantics_text_direction(direction),
                empty_rect(layout.start, layout),
            ))?);
        }
        for run in &layout.runs {
            let pieces = pieces(text, run);
            for chunk in groups(&pieces, cuts) {
                runs.push(text_run(
                    text,
                    chunk,
                    line,
                    run.direction,
                    (layout.top, layout.bottom),
                    scroll_x,
                    &words,
                )?);
            }
        }
    }
    Ok(runs)
}

/// `pieces` cut into runs of at most [`MAX_RUN_CHARACTERS`] characters and
/// at every byte of `cuts` (sorted), so no run crosses one.
fn groups<'a>(pieces: &'a [Piece], cuts: &[usize]) -> Vec<&'a [Piece]> {
    let mut groups = Vec::new();
    let mut start = 0;
    for (index, piece) in pieces.iter().enumerate() {
        let full = index - start == MAX_RUN_CHARACTERS;
        let cut = index > start && cuts.binary_search(&piece.range.start).is_ok();
        if full || cut {
            groups.push(&pieces[start..index]);
            start = index;
        }
    }
    if start < pieces.len() {
        groups.push(&pieces[start..]);
    }
    groups
}
/// The run of `pieces`, contiguous and in logical order, on `line`.
fn text_run(
    text: &str,
    pieces: &[Piece],
    line: usize,
    direction: TextDirection,
    (top, bottom): (f64, f64),
    scroll_x: f64,
    words: &[usize],
) -> Result<TextRun, TextRunError> {
    let start = pieces.first().map_or(0, |piece| piece.range.start);
    let end = pieces.last().map_or(start, |piece| piece.range.end);
    let left = pieces
        .iter()
        .map(|piece| piece.left)
        .fold(f64::INFINITY, f64::min);
    let right = pieces
        .iter()
        .map(|piece| piece.right)
        .fold(f64::NEG_INFINITY, f64::max);
    let rect = Rect::from_ltrb(left - scroll_x, top, right - scroll_x, bottom);
    let mut parts = TextRunParts::new(
        &text[start..end],
        start,
        line,
        semantics_text_direction(direction),
        rect,
    );
    for (index, piece) in pieces.iter().enumerate() {
        parts.character_lengths.push(
            u8::try_from(piece.range.len()).expect("BUG: pieces are cut to at most u8::MAX bytes"),
        );
        let offset = if direction == TextDirection::Rtl {
            right - piece.right
        } else {
            piece.left - left
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "AccessKit stores character geometry as f32"
        )]
        {
            parts.character_positions.push(offset as f32);
            parts
                .character_widths
                .push((piece.right - piece.left) as f32);
        }
        if words.binary_search(&piece.range.start).is_ok() {
            parts
                .word_starts
                .push(u8::try_from(index).expect("BUG: a run holds at most 256 characters"));
        }
    }
    TextRun::try_new(parts)
}
/// `run`'s characters as published: a cluster longer than
/// [`MAX_CHARACTER_BYTES`] is cut at `char` boundaries into pieces that
/// share its span in proportion to their bytes.
fn pieces(text: &str, run: &TextLineRun) -> Vec<Piece> {
    let rtl = run.direction == TextDirection::Rtl;
    let mut pieces = Vec::with_capacity(run.characters.len());
    for character in &run.characters {
        let TextCharacter {
            range, left, right, ..
        } = character;
        let mut start = range.start;
        while start < range.end {
            let end = if range.end - start <= MAX_CHARACTER_BYTES {
                range.end
            } else {
                text.floor_char_boundary(start + MAX_CHARACTER_BYTES)
            };
            // The slice of the span in proportion to the bytes before it.
            #[expect(
                clippy::cast_precision_loss,
                reason = "byte counts within one cluster are small"
            )]
            let at = |byte: usize| (byte - range.start) as f64 / range.len() as f64;
            let width = right - left;
            let (from, to) = if rtl {
                (right - width * at(end), right - width * at(start))
            } else {
                (left + width * at(start), left + width * at(end))
            };
            pieces.push(Piece {
                range: start..end,
                left: from,
                right: to,
            });
            start = end;
        }
    }
    pieces
}
