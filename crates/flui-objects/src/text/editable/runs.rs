//! The field's text as runs for assistive technology: one run per stretch of
//! one direction on one line, cut where AccessKit's limits require, with the
//! word starts the caret steps between and the selection.

use std::ops::Range;

use flui_foundation::geometry::{Point, Rect, Size};
use flui_painting::text_boundaries::word_starts;
use flui_painting::typography::{TextCharacter, TextDirection, TextLineRun};
use flui_rendering::semantics::{
    MAX_RUN_CHARACTERS, TextContent, TextRun, TextRunError, TextRunParts, TextRunSelection,
};

use super::RenderEditable;
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

impl RenderEditable {
    /// The laid-out text as runs, with the selection, or `None` before
    /// layout. A run the semantics model refuses is a bug in this
    /// description: it is logged and the field publishes its value alone.
    pub(super) fn semantics_text_content(&self) -> Option<TextContent> {
        if !self.painter.has_layout() {
            return None;
        }
        match self.describe_runs() {
            Ok(content) => Some(content),
            Err(error) => {
                tracing::error!(%error, "BUG: a text field's layout described a run the semantics model refuses");
                None
            }
        }
    }

    fn describe_runs(&self) -> Result<TextContent, TextRunError> {
        let text = self.plain_text.as_str();
        let words: Vec<usize> = word_starts(text).collect();
        let direction = self.painter.text_direction().unwrap_or(TextDirection::Ltr);
        let mut runs = Vec::new();
        for (line, layout) in self.painter.line_layouts().iter().enumerate() {
            if layout.runs.is_empty() {
                runs.push(self.empty_run(layout.start, line, direction)?);
            }
            for run in &layout.runs {
                let pieces = pieces(text, run);
                for chunk in pieces.chunks(MAX_RUN_CHARACTERS) {
                    runs.push(self.run(
                        text,
                        chunk,
                        line,
                        run.direction,
                        (layout.top, layout.bottom),
                        &words,
                    )?);
                }
            }
        }
        let content = TextContent::try_new(runs)?;
        match self.run_selection(&content) {
            Some(selection) => content.with_selection(selection),
            None => Ok(content),
        }
    }

    /// An empty run at `start`, where the caret on an empty line sits.
    fn empty_run(
        &self,
        start: usize,
        line: usize,
        direction: TextDirection,
    ) -> Result<TextRun, TextRunError> {
        let rect = self
            .local_rect_for_range(start..start)
            .unwrap_or_else(|| Rect::from_origin_size(Point::ZERO, Size::ZERO));
        TextRun::try_new(TextRunParts::new(
            "",
            start,
            line,
            semantics_text_direction(direction),
            rect,
        ))
    }

    /// The run of `pieces`, contiguous and in logical order, on `line`.
    fn run(
        &self,
        text: &str,
        pieces: &[Piece],
        line: usize,
        direction: TextDirection,
        (top, bottom): (f64, f64),
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
        let rect = Rect::from_ltrb(left - self.scroll_x, top, right - self.scroll_x, bottom);
        let mut parts = TextRunParts::new(
            &text[start..end],
            start,
            line,
            semantics_text_direction(direction),
            rect,
        );
        for (index, piece) in pieces.iter().enumerate() {
            parts.character_lengths.push(
                u8::try_from(piece.range.len())
                    .expect("BUG: pieces are cut to at most u8::MAX bytes"),
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

    /// The selection, focus at the caret, as positions in `content`: `None`
    /// when an end falls inside a published character, which the
    /// controller's grapheme snapping rules out.
    fn run_selection(&self, content: &TextContent) -> Option<TextRunSelection> {
        let caret = self.caret_byte_offset;
        let anchor = match &self.selection {
            Some(range) if range.end == caret => range.start,
            Some(range) => range.end,
            None => caret,
        };
        Some(TextRunSelection::new(
            content.position_at(anchor)?,
            content.position_at(caret)?,
        ))
    }
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
