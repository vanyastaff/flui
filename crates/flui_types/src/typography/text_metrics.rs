//! Text metrics types.

use super::TextAffinity;
use crate::geometry::{Pixels, Rect};

/// Position within text with directional affinity.
///
/// Represents a cursor position in text with information about which direction
/// the position "leans" toward (upstream/downstream). This is important for
/// handling positions at line breaks and bidirectional text.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextPosition {
    /// Character offset.
    pub offset: usize,
    /// Affinity (upstream or downstream).
    pub affinity: TextAffinity,
}

impl TextPosition {
    /// Creates a new text position.
    #[must_use]
    #[inline]
    pub const fn new(offset: usize, affinity: TextAffinity) -> Self {
        Self { offset, affinity }
    }

    /// Creates a position with upstream affinity.
    #[must_use]
    #[inline]
    pub const fn upstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Upstream)
    }

    /// Creates a position with downstream affinity.
    #[must_use]
    #[inline]
    pub const fn downstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Downstream)
    }

    /// Returns the character offset.
    #[must_use]
    #[inline]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the text affinity.
    #[must_use]
    #[inline]
    pub const fn affinity(&self) -> TextAffinity {
        self.affinity
    }
}

impl Default for TextPosition {
    #[inline]
    fn default() -> Self {
        Self::upstream(0)
    }
}

/// Range of text specified by start and end offsets.
///
/// Represents a contiguous span of text characters. Start is inclusive, end is exclusive.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextRange {
    /// Start offset (inclusive).
    pub start: usize,
    /// End offset (exclusive).
    pub end: usize,
}

impl TextRange {
    /// Creates a new text range.
    #[must_use]
    #[inline]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates a collapsed (zero-length) range at the given offset.
    #[must_use]
    #[inline]
    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Returns the length of the range.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if the range is empty (start >= end).
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Returns true if the range is collapsed (start == end).
    #[must_use]
    #[inline]
    pub const fn is_collapsed(&self) -> bool {
        self.start == self.end
    }

    /// Returns true if the range contains the given offset.
    #[must_use]
    #[inline]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Returns the intersection of two ranges, or None if they don't overlap.
    #[must_use]
    #[inline]
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

    /// Returns the union of two ranges (smallest range containing both).
    #[must_use]
    #[inline]
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
    #[must_use]
    #[inline]
    pub const fn overlaps(&self, other: &TextRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Returns the start offset.
    #[must_use]
    #[inline]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Returns the end offset.
    #[must_use]
    #[inline]
    pub const fn end(&self) -> usize {
        self.end
    }
}

impl Default for TextRange {
    #[inline]
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// Text selection with base and extent positions.
///
/// Represents a text selection with separate tracking of where it started (base)
/// and where it currently ends (extent). This allows for directional selections.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextSelection {
    /// Base position (where selection started).
    pub base: TextPosition,
    /// Extent position (where selection ended).
    pub extent: TextPosition,
}

impl TextSelection {
    /// Creates a new text selection.
    #[must_use]
    #[inline]
    pub const fn new(base: TextPosition, extent: TextPosition) -> Self {
        Self { base, extent }
    }

    /// Creates a collapsed selection at the given position.
    #[must_use]
    #[inline]
    pub const fn collapsed(position: TextPosition) -> Self {
        Self::new(position, position)
    }

    /// Creates a collapsed selection at the given offset and affinity.
    #[must_use]
    #[inline]
    pub const fn collapsed_at(offset: usize, affinity: TextAffinity) -> Self {
        let position = TextPosition::new(offset, affinity);
        Self::collapsed(position)
    }

    /// Returns true if the selection is collapsed (no range selected).
    #[must_use]
    #[inline]
    pub const fn is_collapsed(&self) -> bool {
        self.base.offset == self.extent.offset
    }

    /// Returns the start offset of the selection (minimum of base and extent).
    #[must_use]
    #[inline]
    pub const fn start(&self) -> usize {
        if self.base.offset < self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the end offset of the selection (maximum of base and extent).
    #[must_use]
    #[inline]
    pub const fn end(&self) -> usize {
        if self.base.offset > self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    /// Returns the selection as a text range.
    #[must_use]
    #[inline]
    pub const fn range(&self) -> TextRange {
        TextRange::new(self.start(), self.end())
    }

    /// Returns true if the selection is reversed (base > extent).
    #[must_use]
    #[inline]
    pub const fn is_reversed(&self) -> bool {
        self.base.offset > self.extent.offset
    }

    /// Returns the length of the selection.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.range().len()
    }

    /// Returns true if the selection is empty.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.is_collapsed()
    }

    /// Expands the selection to include the given range.
    #[must_use]
    #[inline]
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
            TextPosition::new(new_end, self.extent.affinity),
        )
    }

    /// Returns the base position.
    #[must_use]
    #[inline]
    pub const fn base(&self) -> TextPosition {
        self.base
    }

    /// Returns the extent position.
    #[must_use]
    #[inline]
    pub const fn extent(&self) -> TextPosition {
        self.extent
    }
}

impl Default for TextSelection {
    #[inline]
    fn default() -> Self {
        Self::collapsed(TextPosition::default())
    }
}

/// Bounding box for a portion of text.
///
/// Represents the rectangular bounds of text with directional information
/// for handling bidirectional text layout.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextBox {
    /// Bounding rectangle.
    pub rect: Rect<Pixels>,
    /// Text direction for this box.
    pub direction: super::TextDirection,
}

impl TextBox {
    /// Creates a new text box.
    #[must_use]
    #[inline]
    pub const fn new(rect: Rect<Pixels>, direction: super::TextDirection) -> Self {
        Self { rect, direction }
    }

    /// Returns the start edge (left for LTR, right for RTL).
    #[must_use]
    #[inline]
    pub fn start(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.left().0 as f64
        } else {
            self.rect.right().0 as f64
        }
    }

    /// Returns the end edge (right for LTR, left for RTL).
    #[must_use]
    #[inline]
    pub fn end(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.right().0 as f64
        } else {
            self.rect.left().0 as f64
        }
    }

    /// Returns the bounding rectangle.
    #[must_use]
    #[inline]
    pub const fn rect(&self) -> &Rect<Pixels> {
        &self.rect
    }

    /// Returns the text direction.
    #[must_use]
    #[inline]
    pub const fn direction(&self) -> super::TextDirection {
        self.direction
    }

    /// Returns the width of the text box.
    #[must_use]
    #[inline]
    pub fn width(&self) -> f64 {
        self.rect.width().0 as f64
    }

    /// Returns the height of the text box.
    #[must_use]
    #[inline]
    pub fn height(&self) -> f64 {
        self.rect.height().0 as f64
    }
}

/// Information about a single glyph.
///
/// Contains rendering information for a single glyph including its ID,
/// Unicode code point, bounding box, and advance width.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlyphInfo {
    /// Glyph ID in the font.
    pub glyph_id: u32,
    /// Unicode code point.
    pub code_point: char,
    /// Bounding rectangle.
    pub bounds: Rect<Pixels>,
    /// Advance width.
    pub advance: f64,
}

impl GlyphInfo {
    /// Creates new glyph info.
    #[must_use]
    #[inline]
    pub fn new(glyph_id: u32, code_point: char, bounds: Rect<Pixels>, advance: f64) -> Self {
        Self {
            glyph_id,
            code_point,
            bounds,
            advance,
        }
    }

    /// Returns the glyph ID.
    #[must_use]
    #[inline]
    pub const fn glyph_id(&self) -> u32 {
        self.glyph_id
    }

    /// Returns the Unicode code point.
    #[must_use]
    #[inline]
    pub const fn code_point(&self) -> char {
        self.code_point
    }

    /// Returns the glyph bounds.
    #[must_use]
    #[inline]
    pub const fn bounds(&self) -> &Rect<Pixels> {
        &self.bounds
    }

    /// Returns the advance width.
    #[must_use]
    #[inline]
    pub const fn advance(&self) -> f64 {
        self.advance
    }

    /// Returns the glyph width.
    #[must_use]
    #[inline]
    pub fn width(&self) -> f64 {
        self.bounds.width().0 as f64
    }

    /// Returns the glyph height.
    #[must_use]
    #[inline]
    pub fn height(&self) -> f64 {
        self.bounds.height().0 as f64
    }
}

/// Metrics for a single line of text.
///
/// Contains comprehensive layout information for a text line including
/// dimensions, baseline, and text range information.
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
    /// Creates new line metrics.
    #[must_use]
    #[inline]
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

    /// Returns the number of characters in the line.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    /// Returns true if the line is empty.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the text range for this line.
    #[must_use]
    #[inline]
    pub fn range(&self) -> TextRange {
        TextRange::new(self.start_index, self.end_index)
    }

    /// Returns the top coordinate of the line.
    #[must_use]
    #[inline]
    pub fn top(&self) -> f64 {
        self.baseline - self.ascent
    }

    /// Returns the bottom coordinate of the line.
    #[must_use]
    #[inline]
    pub fn bottom(&self) -> f64 {
        self.baseline + self.descent
    }

    /// Returns the right edge of the line.
    #[must_use]
    #[inline]
    pub fn right(&self) -> f64 {
        self.left + self.width
    }

    /// Returns true if the line ends with a hard break.
    #[must_use]
    #[inline]
    pub const fn has_hard_break(&self) -> bool {
        self.hard_break
    }

    /// Returns the ascent value.
    #[must_use]
    #[inline]
    pub const fn ascent(&self) -> f64 {
        self.ascent
    }

    /// Returns the descent value.
    #[must_use]
    #[inline]
    pub const fn descent(&self) -> f64 {
        self.descent
    }

    /// Returns the total height (ascent + descent).
    #[must_use]
    #[inline]
    pub fn total_height(&self) -> f64 {
        self.ascent + self.descent
    }
}
