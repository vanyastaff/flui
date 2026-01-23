//! Text metrics types.

use super::TextAffinity;
use crate::geometry::{Rect, Pixels};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextPosition {
    /// Character offset.
    pub offset: usize,
    /// Affinity (upstream or downstream).
    pub affinity: TextAffinity,
}

impl TextPosition {
    #[must_use]
    pub const fn new(offset: usize, affinity: TextAffinity) -> Self {
        Self { offset, affinity }
    }

    #[must_use]
    pub const fn upstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Upstream)
    }

    #[must_use]
    pub const fn downstream(offset: usize) -> Self {
        Self::new(offset, TextAffinity::Downstream)
    }

    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextRange {
    /// Start offset (inclusive).
    pub start: usize,
    /// End offset (exclusive).
    pub end: usize,
}

impl TextRange {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.start == self.end
    }

    #[must_use]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }

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

    #[must_use]
    pub const fn overlaps(&self, other: &TextRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextSelection {
    /// Base position (where selection started).
    pub base: TextPosition,
    /// Extent position (where selection ended).
    pub extent: TextPosition,
}

impl TextSelection {
    #[must_use]
    pub const fn new(base: TextPosition, extent: TextPosition) -> Self {
        Self { base, extent }
    }

    #[must_use]
    pub const fn collapsed(position: TextPosition) -> Self {
        Self::new(position, position)
    }

    #[must_use]
    pub const fn collapsed_at(offset: usize, affinity: TextAffinity) -> Self {
        let position = TextPosition::new(offset, affinity);
        Self::collapsed(position)
    }

    #[must_use]
    pub const fn is_collapsed(&self) -> bool {
        self.base.offset == self.extent.offset
    }

    #[must_use]
    pub const fn start(&self) -> usize {
        if self.base.offset < self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    #[must_use]
    pub const fn end(&self) -> usize {
        if self.base.offset > self.extent.offset {
            self.base.offset
        } else {
            self.extent.offset
        }
    }

    #[must_use]
    pub const fn range(&self) -> TextRange {
        TextRange::new(self.start(), self.end())
    }

    #[must_use]
    pub const fn is_reversed(&self) -> bool {
        self.base.offset > self.extent.offset
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.range().len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.is_collapsed()
    }

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
            TextPosition::new(new_end, self.extent.affinity),
        )
    }

    #[must_use]
    pub const fn base(&self) -> TextPosition {
        self.base
    }

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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextBox {
    /// Bounding rectangle.
    pub rect: Rect<Pixels>,
    /// Text direction for this box.
    pub direction: super::TextDirection,
}

impl TextBox {
    #[must_use]
    pub const fn new(rect: Rect<Pixels>, direction: super::TextDirection) -> Self {
        Self { rect, direction }
    }

    #[must_use]
    pub fn start(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.left().0 as f64
        } else {
            self.rect.right().0 as f64
        }
    }

    #[must_use]
    pub fn end(&self) -> f64 {
        if self.direction.is_ltr() {
            self.rect.right().0 as f64
        } else {
            self.rect.left().0 as f64
        }
    }

    #[must_use]
    pub const fn rect(&self) -> &Rect<Pixels> {
        &self.rect
    }

    #[must_use]
    pub const fn direction(&self) -> super::TextDirection {
        self.direction
    }

    #[must_use]
    pub fn width(&self) -> f64 {
        self.rect.width().0 as f64
    }

    #[must_use]
    pub fn height(&self) -> f64 {
        self.rect.height().0 as f64
    }
}

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
    #[must_use]
    pub fn new(glyph_id: u32, code_point: char, bounds: Rect<Pixels>, advance: f64) -> Self {
        Self {
            glyph_id,
            code_point,
            bounds,
            advance,
        }
    }

    #[must_use]
    pub const fn glyph_id(&self) -> u32 {
        self.glyph_id
    }

    #[must_use]
    pub const fn code_point(&self) -> char {
        self.code_point
    }

    #[must_use]
    pub const fn bounds(&self) -> &Rect<Pixels> {
        &self.bounds
    }

    #[must_use]
    pub const fn advance(&self) -> f64 {
        self.advance
    }

    #[must_use]
    pub fn width(&self) -> f64 {
        self.bounds.width().0 as f64
    }

    #[must_use]
    pub fn height(&self) -> f64 {
        self.bounds.height().0 as f64
    }
}

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

    #[must_use]
    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn range(&self) -> TextRange {
        TextRange::new(self.start_index, self.end_index)
    }

    #[must_use]
    pub fn top(&self) -> f64 {
        self.baseline - self.ascent
    }

    #[must_use]
    pub fn bottom(&self) -> f64 {
        self.baseline + self.descent
    }

    #[must_use]
    pub fn right(&self) -> f64 {
        self.left + self.width
    }

    #[must_use]
    pub const fn has_hard_break(&self) -> bool {
        self.hard_break
    }

    #[must_use]
    pub const fn ascent(&self) -> f64 {
        self.ascent
    }

    #[must_use]
    pub const fn descent(&self) -> f64 {
        self.descent
    }

    #[must_use]
    pub fn total_height(&self) -> f64 {
        self.ascent + self.descent
    }
}
