//! Text metrics types.

use crate::geometry::Rect;
use super::TextAffinity;

/// Position in text (offset and affinity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextPosition {
    /// Character offset.
    pub offset: usize,
    /// Affinity (upstream or downstream).
    pub affinity: TextAffinity,
}

impl TextPosition {
    /// Creates a new text position.
    #[inline]
    #[must_use]
    pub const fn new(offset: usize, affinity: TextAffinity) -> Self {
        Self { offset, affinity }
    }

    /// Creates a text position with upstream affinity.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextPosition, TextAffinity};
    ///
    /// let pos = TextPosition::upstream(5);
    /// assert_eq!(pos.offset, 5);
    /// assert_eq!(pos.affinity, TextAffinity::Upstream);
    /// ```
    #[inline]
    #[must_use]
    pub const fn upstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Upstream)
    }

    /// Creates a text position with downstream affinity.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextPosition, TextAffinity};
    ///
    /// let pos = TextPosition::downstream(10);
    /// assert_eq!(pos.offset, 10);
    /// assert_eq!(pos.affinity, TextAffinity::Downstream);
    /// ```
    #[inline]
    #[must_use]
    pub const fn downstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Downstream)
    }

    /// Returns the offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextPosition;
    ///
    /// let pos = TextPosition::upstream(42);
    /// assert_eq!(pos.offset(), 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the affinity.
    #[inline]
    #[must_use]
    pub const fn affinity(&self) -> TextAffinity {
        self.affinity
    }
}

impl Default for TextPosition {
    fn default() -> Self {
        Self::upstream(0)
    }
}

/// Range of text (start and end offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextRange {
    /// Start offset (inclusive).
    pub start: usize,
    /// End offset (exclusive).
    pub end: usize,
}

impl TextRange {
    /// Creates a new text range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range = TextRange::new(5, 10);
    /// assert_eq!(range.start, 5);
    /// assert_eq!(range.end, 10);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates an empty text range at the given offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range = TextRange::collapsed(5);
    /// assert_eq!(range.start, 5);
    /// assert_eq!(range.end, 5);
    /// assert!(range.is_collapsed());
    /// ```
    #[inline]
    #[must_use]
    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Returns the length of the range.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if the range is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Returns true if the range is collapsed (start == end).
    #[inline]
    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.start == self.end
    }

    /// Returns true if the range contains the given offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range = TextRange::new(5, 10);
    /// assert!(range.contains(7));
    /// assert!(!range.contains(10));
    /// ```
    #[inline]
    #[must_use]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Returns the intersection of two ranges, or None if they don't overlap.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range1 = TextRange::new(5, 10);
    /// let range2 = TextRange::new(8, 15);
    /// let intersection = range1.intersect(&range2).unwrap();
    /// assert_eq!(intersection.start, 8);
    /// assert_eq!(intersection.end, 10);
    /// ```
    #[inline]
    #[must_use]
    pub const fn intersect(&self, other: &TextRange) -> Option<TextRange> {
        let start = if self.start > other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end < other.end {
            self.end
        } else {
            other.end
        };

        if start < end {
            Some(TextRange::new(start, end))
        } else {
            None
        }
    }

    /// Returns the union of two ranges.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range1 = TextRange::new(5, 10);
    /// let range2 = TextRange::new(8, 15);
    /// let union = range1.union(&range2);
    /// assert_eq!(union.start, 5);
    /// assert_eq!(union.end, 15);
    /// ```
    #[inline]
    #[must_use]
    pub const fn union(&self, other: &TextRange) -> TextRange {
        let start = if self.start < other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end > other.end {
            self.end
        } else {
            other.end
        };
        TextRange::new(start, end)
    }

    /// Returns true if this range overlaps with another.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::TextRange;
    ///
    /// let range1 = TextRange::new(5, 10);
    /// let range2 = TextRange::new(8, 15);
    /// let range3 = TextRange::new(15, 20);
    /// assert!(range1.overlaps(&range2));
    /// assert!(!range1.overlaps(&range3));
    /// ```
    #[inline]
    #[must_use]
    pub const fn overlaps(&self, other: &TextRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Returns the start offset.
    #[inline]
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Returns the end offset.
    #[inline]
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }
}

impl Default for TextRange {
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// Text selection (base and extent positions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextSelection {
    /// Base position (where selection started).
    pub base: TextPosition,
    /// Extent position (where selection ended).
    pub extent: TextPosition,
}

impl TextSelection {
    /// Creates a new text selection.
    #[inline]
    #[must_use]
    pub const fn new(base: TextPosition, extent: TextPosition) -> Self {
        Self { base, extent }
    }

    /// Creates a collapsed selection at the given position.
    #[inline]
    #[must_use]
    pub const fn collapsed(position: TextPosition) -> Self {
        Self::new(position, position)
    }

    /// Creates a collapsed selection at the given offset.
    #[inline]
    #[must_use]
    pub const fn collapsed_at(offset: usize, affinity: TextAffinity) -> Self {
        let position = TextPosition::new(offset, affinity);
        Self::collapsed(position)
    }

    /// Returns true if the selection is collapsed.
    #[inline]
    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.base.offset == self.extent.offset
    }

    /// Returns the start offset of the selection (min of base and extent).
    #[inline]
    #[must_use]
    pub const fn start(&self) -> usize {
        if self.base.offset < self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the end offset of the selection (max of base and extent).
    #[inline]
    #[must_use]
    pub const fn end(&self) -> usize {
        if self.base.offset > self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the text range covered by this selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextSelection, TextPosition, TextAffinity};
    ///
    /// let selection = TextSelection::new(
    ///     TextPosition::upstream(5),
    ///     TextPosition::downstream(10)
    /// );
    /// let range = selection.range();
    /// assert_eq!(range.start, 5);
    /// assert_eq!(range.end, 10);
    /// ```
    #[inline]
    #[must_use]
    pub const fn range(&self) -> TextRange {
        TextRange::new(self.start(), self.end())
    }

    /// Returns true if the selection is reversed (base > extent).
    #[inline]
    #[must_use]
    pub const fn is_reversed(&self) -> bool {
        self.base.offset > self.extent.offset
    }

    /// Returns the length of the selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextSelection, TextPosition};
    ///
    /// let selection = TextSelection::new(
    ///     TextPosition::upstream(5),
    ///     TextPosition::downstream(10)
    /// );
    /// assert_eq!(selection.len(), 5);
    /// ```
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.range().len()
    }

    /// Returns true if the selection is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.is_collapsed()
    }

    /// Expands the selection to include the given range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextSelection, TextPosition, TextRange};
    ///
    /// let selection = TextSelection::collapsed_at(5, Default::default());
    /// let range = TextRange::new(3, 10);
    /// let expanded = selection.expand_to_range(&range);
    /// assert_eq!(expanded.start(), 3);
    /// assert_eq!(expanded.end(), 10);
    /// ```
    #[inline]
    #[must_use]
    pub const fn expand_to_range(&self, range: &TextRange) -> Self {
        let new_start = if self.start() < range.start {
            self.start()
        } else {
            range.start
        };
        let new_end = if self.end() > range.end {
            self.end()
        } else {
            range.end
        };
        Self::new(
            TextPosition::new(new_start, self.base.affinity),
            TextPosition::new(new_end, self.extent.affinity)
        )
    }

    /// Returns the base position.
    #[inline]
    #[must_use]
    pub const fn base(&self) -> TextPosition {
        self.base
    }

    /// Returns the extent position.
    #[inline]
    #[must_use]
    pub const fn extent(&self) -> TextPosition {
        self.extent
    }
}

impl Default for TextSelection {
    fn default() -> Self {
        Self::collapsed(TextPosition::default())
    }
}

/// Bounding box for a portion of text.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextBox {
    /// Bounding rectangle.
    pub rect: Rect,
    /// Text direction for this box.
    pub direction: super::TextDirection,
}

impl TextBox {
    /// Creates a new text box.
    #[inline]
    #[must_use]
    pub const fn new(rect: Rect, direction: super::TextDirection) -> Self {
        Self { rect, direction }
    }

    /// Returns the left edge of the box (depends on text direction).
    ///
    /// For LTR text, returns the left edge. For RTL text, returns the right edge.
    #[inline]
    #[must_use]
    pub fn start(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.left() as f64
        } else {
            self.rect.right() as f64
        }
    }

    /// Returns the right edge of the box (depends on text direction).
    ///
    /// For LTR text, returns the right edge. For RTL text, returns the left edge.
    #[inline]
    #[must_use]
    pub fn end(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.right() as f64
        } else {
            self.rect.left() as f64
        }
    }

    /// Returns the rect.
    #[inline]
    #[must_use]
    pub const fn rect(&self) -> &Rect {
        &self.rect
    }

    /// Returns the text direction.
    #[inline]
    #[must_use]
    pub const fn direction(&self) -> super::TextDirection {
        self.direction
    }

    /// Returns the width of the text box.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f64 {
        self.rect.width() as f64
    }

    /// Returns the height of the text box.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f64 {
        self.rect.height() as f64
    }
}

/// Information about a single glyph.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlyphInfo {
    /// Glyph ID in the font.
    pub glyph_id: u32,
    /// Unicode code point.
    pub code_point: char,
    /// Bounding rectangle.
    pub bounds: Rect,
    /// Advance width.
    pub advance: f64,
}

impl GlyphInfo {
    /// Creates a new glyph info.
    #[inline]
    #[must_use]
    pub fn new(glyph_id: u32, code_point: char, bounds: Rect, advance: f64) -> Self {
        Self {
            glyph_id,
            code_point,
            bounds,
            advance,
        }
    }

    /// Returns the glyph ID.
    #[inline]
    #[must_use]
    pub const fn glyph_id(&self) -> u32 {
        self.glyph_id
    }

    /// Returns the code point.
    #[inline]
    #[must_use]
    pub const fn code_point(&self) -> char {
        self.code_point
    }

    /// Returns the bounds.
    #[inline]
    #[must_use]
    pub const fn bounds(&self) -> &Rect {
        &self.bounds
    }

    /// Returns the advance width.
    #[inline]
    #[must_use]
    pub const fn advance(&self) -> f64 {
        self.advance
    }

    /// Returns the width of the glyph.
    #[inline]
    #[must_use]
    pub fn width(&self) -> f64 {
        self.bounds.width() as f64
    }

    /// Returns the height of the glyph.
    #[inline]
    #[must_use]
    pub fn height(&self) -> f64 {
        self.bounds.height() as f64
    }
}

/// Metrics for a line of text.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineMetrics {
    /// Whether this line ends with a hard break.
    pub hard_break: bool,
    /// Ascent (distance from baseline to top).
    pub ascent: f64,
    /// Descent (distance from baseline to bottom).
    pub descent: f64,
    /// Unscaled ascent.
    pub unscaled_ascent: f64,
    /// Height of the line.
    pub height: f64,
    /// Width of the line.
    pub width: f64,
    /// Left offset of the line.
    pub left: f64,
    /// Baseline offset from top.
    pub baseline: f64,
    /// Line number (0-indexed).
    pub line_number: usize,
    /// Start text index for this line.
    pub start_index: usize,
    /// End text index for this line (exclusive).
    pub end_index: usize,
    /// End text index excluding whitespace.
    pub end_excluding_whitespace: usize,
    /// End text index including newline.
    pub end_including_newline: usize,
}

impl LineMetrics {
    /// Creates a new line metrics.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        hard_break: bool,
        ascent: f64,
        descent: f64,
        unscaled_ascent: f64,
        height: f64,
        width: f64,
        left: f64,
        baseline: f64,
        line_number: usize,
        start_index: usize,
        end_index: usize,
        end_excluding_whitespace: usize,
        end_including_newline: usize,
    ) -> Self {
        Self {
            hard_break,
            ascent,
            descent,
            unscaled_ascent,
            height,
            width,
            left,
            baseline,
            line_number,
            start_index,
            end_index,
            end_excluding_whitespace,
            end_including_newline,
        }
    }

    /// Returns the length of the line (excluding newline).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    /// Returns true if the line is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the text range for this line.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::LineMetrics;
    ///
    /// let metrics = LineMetrics::new(
    ///     true, 10.0, 5.0, 10.0, 15.0, 100.0, 0.0, 10.0, 0, 0, 20, 18, 21,
    /// );
    /// let range = metrics.range();
    /// assert_eq!(range.start, 0);
    /// assert_eq!(range.end, 20);
    /// ```
    #[inline]
    #[must_use]
    pub fn range(&self) -> TextRange {
        TextRange::new(self.start_index, self.end_index)
    }

    /// Returns the top position of the line (baseline - ascent).
    #[inline]
    #[must_use]
    pub fn top(&self) -> f64 {
        self.baseline - self.ascent
    }

    /// Returns the bottom position of the line (baseline + descent).
    #[inline]
    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.baseline + self.descent
    }

    /// Returns the right edge of the line.
    #[inline]
    #[must_use]
    pub fn right(&self) -> f64 {
        self.left + self.width
    }

    /// Returns true if this line has a hard break.
    #[inline]
    #[must_use]
    pub const fn has_hard_break(&self) -> bool {
        self.hard_break
    }

    /// Returns the ascent.
    #[inline]
    #[must_use]
    pub const fn ascent(&self) -> f64 {
        self.ascent
    }

    /// Returns the descent.
    #[inline]
    #[must_use]
    pub const fn descent(&self) -> f64 {
        self.descent
    }

    /// Returns the total height (ascent + descent).
    #[inline]
    #[must_use]
    pub fn total_height(&self) -> f64 {
        self.ascent + self.descent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_position() {
        let pos = TextPosition::upstream(5);
        assert_eq!(pos.offset, 5);
        assert_eq!(pos.affinity, TextAffinity::Upstream);

        let pos = TextPosition::downstream(10);
        assert_eq!(pos.offset, 10);
        assert_eq!(pos.affinity, TextAffinity::Downstream);
    }

    #[test]
    fn test_text_position_default() {
        let pos = TextPosition::default();
        assert_eq!(pos.offset, 0);
        assert_eq!(pos.affinity, TextAffinity::Upstream);
    }

    #[test]
    fn test_text_range() {
        let range = TextRange::new(5, 10);
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 10);
        assert_eq!(range.len(), 5);
        assert!(!range.is_empty());
        assert!(!range.is_collapsed());
        assert!(!range.contains(4));
        assert!(range.contains(5));
        assert!(range.contains(9));
        assert!(!range.contains(10));
    }

    #[test]
    fn test_text_range_collapsed() {
        let range = TextRange::collapsed(5);
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 5);
        assert_eq!(range.len(), 0);
        assert!(range.is_empty());
        assert!(range.is_collapsed());
    }

    #[test]
    fn test_text_range_intersect() {
        let range1 = TextRange::new(5, 10);
        let range2 = TextRange::new(8, 15);
        let intersection = range1.intersect(&range2).unwrap();
        assert_eq!(intersection.start, 8);
        assert_eq!(intersection.end, 10);

        let range3 = TextRange::new(15, 20);
        assert!(range1.intersect(&range3).is_none());
    }

    #[test]
    fn test_text_selection() {
        let base = TextPosition::upstream(5);
        let extent = TextPosition::downstream(10);
        let selection = TextSelection::new(base, extent);

        assert_eq!(selection.start(), 5);
        assert_eq!(selection.end(), 10);
        assert!(!selection.is_collapsed());
        assert!(!selection.is_reversed());

        let range = selection.range();
        assert_eq!(range.start, 5);
        assert_eq!(range.end, 10);
    }

    #[test]
    fn test_text_selection_reversed() {
        let base = TextPosition::upstream(10);
        let extent = TextPosition::downstream(5);
        let selection = TextSelection::new(base, extent);

        assert_eq!(selection.start(), 5);
        assert_eq!(selection.end(), 10);
        assert!(selection.is_reversed());
    }

    #[test]
    fn test_text_selection_collapsed() {
        let pos = TextPosition::upstream(5);
        let selection = TextSelection::collapsed(pos);

        assert_eq!(selection.start(), 5);
        assert_eq!(selection.end(), 5);
        assert!(selection.is_collapsed());
    }

    #[test]
    fn test_text_box() {
        use super::super::TextDirection;
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let text_box = TextBox::new(rect, TextDirection::Ltr);

        assert_eq!(text_box.rect, rect);
        assert_eq!(text_box.direction, TextDirection::Ltr);
    }

    #[test]
    fn test_glyph_info() {
        let bounds = Rect::from_xywh(0.0, 0.0, 10.0, 20.0);
        let glyph = GlyphInfo::new(42, 'A', bounds, 12.0);

        assert_eq!(glyph.glyph_id, 42);
        assert_eq!(glyph.code_point, 'A');
        assert_eq!(glyph.bounds, bounds);
        assert_eq!(glyph.advance, 12.0);
    }

    #[test]
    fn test_line_metrics() {
        let metrics = LineMetrics::new(
            true, 10.0, 5.0, 10.0, 15.0, 100.0, 0.0, 10.0, 0, 0, 20, 18, 21,
        );

        assert!(metrics.hard_break);
        assert_eq!(metrics.ascent, 10.0);
        assert_eq!(metrics.descent, 5.0);
        assert_eq!(metrics.height, 15.0);
        assert_eq!(metrics.width, 100.0);
        assert_eq!(metrics.line_number, 0);
        assert_eq!(metrics.start_index, 0);
        assert_eq!(metrics.end_index, 20);
        assert_eq!(metrics.len(), 20);
        assert!(!metrics.is_empty());
    }
}
