//! The rectangles UIKit draws a range's selection with and places its
//! handles by (`selectionRectsForRange:`): one per line the range crosses,
//! each with the way its line's text runs and whether it holds the range's
//! start or its end. Pure, over the field's read session, so it runs on every
//! host; the iOS responder hands UIKit what it answers.
//!
//! The store's contract is line-agnostic (ADR-0090): it answers a range's
//! bounding rectangle and the caret at an offset, and nothing about lines.
//! The lines come from the carets: a caret whose middle lies below the bottom
//! of a line's first caret sits on a later line. Carets only descend as the
//! offset grows in horizontal text, so a binary search finds where each line
//! ends in O(log n) caret reads, and a line's rectangle is the store's
//! rectangle of its part of the range. A caret at a line break is taken to
//! sit at the start of the later line, as the field's carets do.

use flui_foundation::TextDirection;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::text_store::utf16::{OffsetError, Utf16Offset, Utf16Range};
use flui_platform::text_store::{TextStoreError, TextStoreRead};

/// The most rectangles one answer holds: the lines from the last one on are
/// covered by the last rectangle, so one UIKit query reads a bounded number
/// of carets however many lines the range crosses.
pub(crate) const MAX_LINES: usize = 256;

/// One line's part of a range, as UIKit's `UITextSelectionRect` describes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SelectionRect {
    /// The line's part of the range, in window-root logical pixels; for an
    /// empty range, the caret.
    pub(crate) bounds: Bounds<f64>,
    /// The way text runs from the range's first scalar on the line: UIKit
    /// puts the start handle on that side of the rectangle that holds the
    /// start, and the end handle on the other side of the one that holds the
    /// end.
    pub(crate) direction: TextDirection,
    /// Whether the line holds the range's start.
    pub(crate) contains_start: bool,
    /// Whether the line holds the range's end.
    pub(crate) contains_end: bool,
}

impl SelectionRect {
    /// The edges UIKit stands the selection's handles on, as zero-wide
    /// rectangles as tall as their lines: the leading edge of the rectangle
    /// that holds the start and the trailing edge of the one that holds the
    /// end, leading being left for left-to-right text and right for
    /// right-to-left.
    pub(crate) fn handle_edges(rects: &[Self]) -> impl Iterator<Item = Bounds<f64>> + '_ {
        let edge = |rect: &Self, leading: bool| {
            let left = (rect.direction == TextDirection::Ltr) == leading;
            let x = if left {
                rect.bounds.left()
            } else {
                rect.bounds.right()
            };
            Bounds::new(
                Point::new(x, rect.bounds.top()),
                Size::new(0.0, rect.bounds.size.height),
            )
        };
        let start = rects.iter().find(|rect| rect.contains_start);
        let end = rects.iter().rev().find(|rect| rect.contains_end);
        start
            .map(|rect| edge(rect, true))
            .into_iter()
            .chain(end.map(|rect| edge(rect, false)))
    }
}

/// The rectangles of `range`, an offset range inside `session`'s document,
/// one per line in document order: one zero-wide rectangle, the caret, for
/// an empty range, and none when the field has no layout for it or the store
/// answers a rectangle that is not finite. An end that splits a surrogate
/// pair moves out to cover the whole scalar.
pub(crate) fn selection_rects(
    session: &dyn TextStoreRead,
    range: Utf16Range,
) -> Vec<SelectionRect> {
    match lines(session, range) {
        Ok(rects) if rects.iter().all(|rect| is_finite(rect.bounds)) => rects,
        Ok(_) => {
            tracing::debug!("the field answered a selection rectangle that is not finite");
            Vec::new()
        }
        Err(error) => {
            tracing::debug!(
                ?error,
                "the field has no selection rectangles for the range"
            );
            Vec::new()
        }
    }
}

fn lines(
    session: &dyn TextStoreRead,
    range: Utf16Range,
) -> Result<Vec<SelectionRect>, TextStoreError> {
    if range.is_empty() {
        let (at, bounds) = caret(session, range.start(), Toward::Start)?;
        let direction = if at < session.document_len() {
            scalar_direction(session, at, scalar_end(session, at)?)?
        } else if let Some(before) = at.get().checked_sub(1) {
            let (start, _) = caret(session, Utf16Offset::new(before), Toward::Start)?;
            scalar_direction(session, start, at)?
        } else {
            TextDirection::Ltr
        };
        return Ok(vec![SelectionRect {
            bounds,
            direction,
            contains_start: true,
            contains_end: true,
        }]);
    }
    let (first, _) = caret(session, range.start(), Toward::Start)?;
    let (last, _) = caret(session, range.end(), Toward::End)?;
    let mut rects = Vec::new();
    let mut start = first;
    loop {
        let (_, line) = caret(session, start, Toward::Start)?;
        let end = if rects.len() + 1 == MAX_LINES {
            last
        } else {
            line_end(session, start, line, last)?
        };
        // Carets that do not descend with the offset (a store outside the
        // contract) leave no line to step over: the rest is one rectangle.
        let end = if end > start { end } else { last };
        let part =
            Utf16Range::try_new(start, end).expect("BUG: a line's part ends after it starts");
        rects.push(SelectionRect {
            bounds: session.rect_for_range(part)?.bounds,
            direction: scalar_direction(session, start, scalar_end(session, start)?)?,
            contains_start: start == first,
            contains_end: end == last,
        });
        if end == last {
            return Ok(rects);
        }
        start = end;
    }
}

/// Which way an offset that splits a surrogate pair moves to reach a scalar
/// boundary.
#[derive(Clone, Copy)]
enum Toward {
    Start,
    End,
}

/// The caret at `at`, or at the scalar boundary `toward` it when `at` splits
/// a surrogate pair, and the boundary it is at.
fn caret(
    session: &dyn TextStoreRead,
    at: Utf16Offset,
    toward: Toward,
) -> Result<(Utf16Offset, Bounds<f64>), TextStoreError> {
    match session.rect_for_range(Utf16Range::collapsed(at)) {
        Ok(rect) => Ok((at, rect.bounds)),
        Err(TextStoreError::Offset(OffsetError::SplitsSurrogatePair(_))) => {
            // An offset inside a pair has a unit on each side of it.
            let at = Utf16Offset::new(match toward {
                Toward::Start => at.get().saturating_sub(1),
                Toward::End => at.get().saturating_add(1),
            });
            Ok((
                at,
                session.rect_for_range(Utf16Range::collapsed(at))?.bounds,
            ))
        }
        Err(error) => Err(error),
    }
}

/// The end of the scalar that starts at `at`, a boundary before the
/// document's end.
fn scalar_end(session: &dyn TextStoreRead, at: Utf16Offset) -> Result<Utf16Offset, TextStoreError> {
    let (end, _) = caret(
        session,
        Utf16Offset::new(at.get().saturating_add(1)),
        Toward::End,
    )?;
    Ok(end)
}

/// The first offset after `start`, up to `last`, whose caret sits on a later
/// line than `line`, the caret at `start`; `last` when every caret up to it
/// sits on that line.
fn line_end(
    session: &dyn TextStoreRead,
    start: Utf16Offset,
    line: Bounds<f64>,
    last: Utf16Offset,
) -> Result<Utf16Offset, TextStoreError> {
    let later = |bounds: Bounds<f64>| bounds.center().y > line.bottom();
    if !later(caret(session, last, Toward::End)?.1) {
        return Ok(last);
    }
    // The caret at `before` sits on the line, the one at `after` below it.
    // `after` is always a scalar boundary; `before` may lie inside a pair,
    // whose caret is the one at the pair's start.
    let (mut before, mut after) = (start.get(), last.get());
    while after - before > 1 {
        let middle = before + (after - before) / 2;
        let (boundary, bounds) = caret(session, Utf16Offset::new(middle), Toward::Start)?;
        if later(bounds) {
            after = boundary.get();
        } else {
            before = middle;
        }
    }
    Ok(Utf16Offset::new(after))
}

/// Which way text runs from the scalar `start..end`: right to left when the
/// caret at its start sits right of the middle of its box.
fn scalar_direction(
    session: &dyn TextStoreRead,
    start: Utf16Offset,
    end: Utf16Offset,
) -> Result<TextDirection, TextStoreError> {
    let scalar = Utf16Range::try_new(start, end).expect("BUG: a scalar ends after it starts");
    let (_, start_caret) = caret(session, start, Toward::Start)?;
    let scalar = session.rect_for_range(scalar)?.bounds;
    Ok(if start_caret.center().x > scalar.center().x {
        TextDirection::Rtl
    } else {
        TextDirection::Ltr
    })
}

fn is_finite(bounds: Bounds<f64>) -> bool {
    [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ]
    .iter()
    .all(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use flui_foundation::TextDirection;
    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::text_store::utf16::{self, Utf16Offset, Utf16Range};
    use flui_platform::text_store::{
        Composition, PointMode, RangeRect, Selection, TextStoreError, TextStoreRead,
    };

    use super::{SelectionRect, selection_rects};

    /// Each scalar's advance.
    const ADVANCE: f64 = 10.0;
    /// Each line's height.
    const LINE: f64 = 20.0;
    /// The width a right-to-left line runs from.
    const WIDTH: f64 = 100.0;

    /// A document laid out on lines that start at the given UTF-16 offsets,
    /// each running its way, one advance per scalar: the field's geometry as
    /// the store's contract gives it, a caret at a line's start on that line.
    struct LaidOutLines {
        text: String,
        lines: Vec<(usize, TextDirection)>,
    }

    impl LaidOutLines {
        fn new(text: &str, lines: &[(usize, TextDirection)]) -> Self {
            Self {
                text: text.to_owned(),
                lines: lines.to_vec(),
            }
        }

        /// The line `at` sits on, and the scalars between its start and `at`.
        fn place(&self, at: usize) -> (usize, TextDirection, usize) {
            let index = self
                .lines
                .iter()
                .rposition(|&(start, _)| start <= at)
                .expect("a test layout's first line starts at 0");
            let (start, direction) = self.lines[index];
            let prefix = utf16::byte_offset(&self.text, Utf16Offset::new(start))
                .expect("a line starts on a scalar boundary");
            let end = utf16::byte_offset(&self.text, Utf16Offset::new(at))
                .expect("a place is asked for at a scalar boundary");
            (index, direction, self.text[prefix..end].chars().count())
        }

        /// The edge `column` scalars into a line running `direction`.
        fn x(direction: TextDirection, column: usize) -> f64 {
            let advance = column as f64 * ADVANCE;
            match direction {
                TextDirection::Ltr => advance,
                TextDirection::Rtl => WIDTH - advance,
            }
        }

        fn top(line: usize) -> f64 {
            line as f64 * LINE
        }

        fn caret(&self, at: usize) -> Bounds<f64> {
            let (line, direction, column) = self.place(at);
            Bounds::new(
                Point::new(Self::x(direction, column), Self::top(line)),
                Size::new(0.0, LINE),
            )
        }

        /// The box of the scalar that starts at `at`.
        fn scalar_box(&self, at: usize) -> Bounds<f64> {
            let (line, direction, column) = self.place(at);
            let (a, b) = (Self::x(direction, column), Self::x(direction, column + 1));
            Bounds::from_corners(
                Point::new(a.min(b), Self::top(line)),
                Point::new(a.max(b), Self::top(line) + LINE),
            )
        }
    }

    impl TextStoreRead for LaidOutLines {
        fn document_len(&self) -> Utf16Offset {
            utf16::utf16_len(&self.text)
        }

        fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
            Ok(self.text[utf16::byte_range(&self.text, range)?].to_owned())
        }

        fn selection(&self) -> Selection {
            Selection::collapsed(Utf16Offset::ZERO)
        }

        fn composition(&self) -> Option<Composition> {
            None
        }

        /// The union of the range's scalar boxes, the caret for an empty one.
        fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
            let bytes = utf16::byte_range(&self.text, range)?;
            let bounds = if range.is_empty() {
                self.caret(range.start().get())
            } else {
                self.text[bytes.clone()]
                    .char_indices()
                    .map(|(byte, _)| {
                        let at = utf16::utf16_offset(&self.text, bytes.start + byte)
                            .expect("a scalar starts on a boundary");
                        self.scalar_box(at.get())
                    })
                    .reduce(|union, scalar| union.union(&scalar))
                    .expect("a non-empty range holds a scalar")
            };
            Ok(RangeRect {
                bounds,
                clipped: false,
            })
        }

        fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }

        fn index_at_point(
            &self,
            _point: Point<f64>,
            _mode: PointMode,
        ) -> Result<Utf16Offset, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
    }

    fn span(start: usize, end: usize) -> Utf16Range {
        Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end))
            .expect("a test span starts first")
    }

    fn rect(
        (x, y, width): (f64, f64, f64),
        direction: TextDirection,
        contains_start: bool,
        contains_end: bool,
    ) -> SelectionRect {
        SelectionRect {
            bounds: Bounds::new(Point::new(x, y), Size::new(width, LINE)),
            direction,
            contains_start,
            contains_end,
        }
    }

    #[test]
    fn a_range_gets_a_rectangle_for_each_line_it_crosses() {
        use TextDirection::{Ltr, Rtl};
        let rows: [(&str, LaidOutLines, Utf16Range, Vec<SelectionRect>); 6] = [
            (
                "one line",
                LaidOutLines::new("hello world", &[(0, Ltr)]),
                span(6, 11),
                vec![rect((60.0, 0.0, 50.0), Ltr, true, true)],
            ),
            (
                "three lines, the middle one whole",
                LaidOutLines::new("one two three", &[(0, Ltr), (4, Ltr), (8, Ltr)]),
                span(2, 10),
                vec![
                    rect((20.0, 0.0, 20.0), Ltr, true, false),
                    rect((0.0, LINE, 40.0), Ltr, false, false),
                    rect((0.0, 2.0 * LINE, 20.0), Ltr, false, true),
                ],
            ),
            (
                "a range ending where a line starts ends on the line before",
                LaidOutLines::new("one two", &[(0, Ltr), (4, Ltr)]),
                span(1, 4),
                vec![rect((10.0, 0.0, 30.0), Ltr, true, true)],
            ),
            (
                "a right-to-left line after a left-to-right one",
                LaidOutLines::new("abc def", &[(0, Ltr), (4, Rtl)]),
                span(2, 6),
                vec![
                    rect((20.0, 0.0, 20.0), Ltr, true, false),
                    rect((WIDTH - 20.0, LINE, 20.0), Rtl, false, true),
                ],
            ),
            (
                "an empty range is its caret",
                LaidOutLines::new("abc", &[(0, Ltr)]),
                span(2, 2),
                vec![rect((20.0, 0.0, 0.0), Ltr, true, true)],
            ),
            (
                "ends inside a surrogate pair cover the scalar",
                LaidOutLines::new("a😀b😀", &[(0, Ltr)]),
                span(2, 5),
                vec![rect((10.0, 0.0, 30.0), Ltr, true, true)],
            ),
        ];
        for (case, layout, range, expected) in rows {
            assert_eq!(selection_rects(&layout, range), expected, "{case}");
        }
    }

    #[test]
    fn the_handles_stand_on_the_starts_leading_and_the_ends_trailing_edge() {
        use TextDirection::{Ltr, Rtl};
        let edge = |x: f64, y: f64| Bounds::new(Point::new(x, y), Size::new(0.0, LINE));
        let rows: [(&str, LaidOutLines, Utf16Range, Vec<Bounds<f64>>); 3] = [
            (
                "one left-to-right line",
                LaidOutLines::new("hello world", &[(0, Ltr)]),
                span(6, 11),
                vec![edge(60.0, 0.0), edge(110.0, 0.0)],
            ),
            (
                "one right-to-left line",
                LaidOutLines::new("abc def", &[(0, Rtl)]),
                span(1, 3),
                vec![edge(WIDTH - 10.0, 0.0), edge(WIDTH - 30.0, 0.0)],
            ),
            (
                "from a left-to-right line into a right-to-left one",
                LaidOutLines::new("abc def", &[(0, Ltr), (4, Rtl)]),
                span(2, 6),
                vec![edge(20.0, 0.0), edge(WIDTH - 20.0, LINE)],
            ),
        ];
        for (case, layout, range, expected) in rows {
            let rects = selection_rects(&layout, range);
            let edges: Vec<Bounds<f64>> = SelectionRect::handle_edges(&rects).collect();
            assert_eq!(edges, expected, "{case}");
        }
    }
}
