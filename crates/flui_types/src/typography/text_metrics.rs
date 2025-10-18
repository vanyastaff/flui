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
    pub const fn new(offset: usize, affinity: TextAffinity) -> Self {
        Self { offset, affinity }
    }

    /// Creates a text position with upstream affinity.
    pub const fn upstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Upstream)
    }

    /// Creates a text position with downstream affinity.
    pub const fn downstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Downstream)
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
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates an empty text range at the given offset.
    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Returns the length of the range.
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if the range is empty.
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Returns true if the range is collapsed (start == end).
    pub const fn is_collapsed(&self) -> bool {
        self.start == self.end
    }

    /// Returns true if the range contains the given offset.
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Returns the intersection of two ranges, or None if they don't overlap.
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
    pub const fn new(base: TextPosition, extent: TextPosition) -> Self {
        Self { base, extent }
    }

    /// Creates a collapsed selection at the given position.
    pub const fn collapsed(position: TextPosition) -> Self {
        Self::new(position, position)
    }

    /// Creates a collapsed selection at the given offset.
    pub const fn collapsed_at(offset: usize, affinity: TextAffinity) -> Self {
        let position = TextPosition::new(offset, affinity);
        Self::collapsed(position)
    }

    /// Returns true if the selection is collapsed.
    pub const fn is_collapsed(&self) -> bool {
        self.base.offset == self.extent.offset
    }

    /// Returns the start offset of the selection (min of base and extent).
    pub const fn start(&self) -> usize {
        if self.base.offset < self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the end offset of the selection (max of base and extent).
    pub const fn end(&self) -> usize {
        if self.base.offset > self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the text range covered by this selection.
    pub const fn range(&self) -> TextRange {
        TextRange::new(self.start(), self.end())
    }

    /// Returns true if the selection is reversed (base > extent).
    pub const fn is_reversed(&self) -> bool {
        self.base.offset > self.extent.offset
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
    pub const fn new(rect: Rect, direction: super::TextDirection) -> Self {
        Self { rect, direction }
    }

    /// Returns the left edge of the box (depends on text direction).
    pub fn start(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.left() as f64
        } else {
            self.rect.right() as f64
        }
    }

    /// Returns the right edge of the box (depends on text direction).
    pub fn end(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.right() as f64
        } else {
            self.rect.left() as f64
        }
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
    pub fn new(glyph_id: u32, code_point: char, bounds: Rect, advance: f64) -> Self {
        Self {
            glyph_id,
            code_point,
            bounds,
            advance,
        }
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
    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    /// Returns true if the line is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
