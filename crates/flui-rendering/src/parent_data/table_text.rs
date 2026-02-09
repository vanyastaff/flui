//! Specialized parent data types - Table and Text layouts.

use flui_types::Offset;
use std::hash::{Hash, Hasher};

use super::base::ParentData;

use super::container_mixin::ContainerParentDataMixin;
use flui_foundation::RenderId;

// ============================================================================
// TABLE CELL PARENT DATA
// ============================================================================

/// Parent data for table cell children.
///
/// Extends `BoxParentData` with table-specific cell positioning and alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCellParentData {
    /// Offset from parent (table's top-left corner).
    pub offset: Offset,

    /// Column index (0-based).
    pub x: usize,

    /// Row index (0-based).
    pub y: usize,

    /// Vertical alignment within the cell.
    pub vertical_alignment: TableCellVerticalAlignment,
}

/// Vertical alignment options for table cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableCellVerticalAlignment {
    /// Align to top of cell.
    Top,

    /// Center vertically in cell.
    Middle,

    /// Align to bottom of cell.
    Bottom,

    /// Align to baseline (for text cells).
    Baseline,

    /// Fill entire cell height.
    Fill,
}

impl TableCellParentData {
    /// Create with cell position and alignment.
    pub const fn new(x: usize, y: usize, vertical_alignment: TableCellVerticalAlignment) -> Self {
        Self {
            offset: Offset::ZERO,
            x,
            y,
            vertical_alignment,
        }
    }

    /// Create at cell (0, 0) with top alignment.
    pub const fn zero() -> Self {
        Self::new(0, 0, TableCellVerticalAlignment::Top)
    }

    /// Builder: set cell position.
    pub const fn at_cell(mut self, x: usize, y: usize) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Builder: set vertical alignment.
    pub const fn with_alignment(mut self, alignment: TableCellVerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }

    /// Builder: set offset.
    pub const fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Check if this is the first cell.
    #[inline]
    pub const fn is_first_cell(&self) -> bool {
        self.x == 0 && self.y == 0
    }

    /// Get cell position as tuple.
    #[inline]
    pub const fn cell_position(&self) -> (usize, usize) {
        (self.x, self.y)
    }
}

impl Default for TableCellParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for TableCellParentData {}

impl Hash for TableCellParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.x.hash(state);
        self.y.hash(state);
        self.vertical_alignment.hash(state);
    }
}

// ============================================================================
// TEXT PARENT DATA
// ============================================================================

/// Parent data for inline text spans in rich text.
///
/// Combines container functionality (for inline spans) with text range information.
#[derive(Debug, Clone, PartialEq)]
pub struct TextParentData {
    /// Offset from paragraph origin.
    pub offset: Offset,

    /// Container mixin for sibling text spans.
    pub container: ContainerParentDataMixin<RenderId>,

    /// Range of text covered by this span (start, end indices).
    ///
    /// `None` if span doesn't represent a text range (e.g., inline widget).
    pub span: Option<TextRange>,
}

/// Range of text in a paragraph (start and end character indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRange {
    /// Start character index (inclusive).
    pub start: usize,

    /// End character index (exclusive).
    pub end: usize,
}

impl TextRange {
    /// Create text range.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Get length of range.
    #[inline]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Check if range is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Check if range contains index.
    #[inline]
    pub const fn contains(&self, index: usize) -> bool {
        index >= self.start && index < self.end
    }
}

impl TextParentData {
    /// Create with optional text range.
    pub const fn new(span: Option<TextRange>) -> Self {
        Self {
            offset: Offset::ZERO,
            container: ContainerParentDataMixin::new(),
            span,
        }
    }

    /// Create at origin with no text range.
    pub const fn zero() -> Self {
        Self::new(None)
    }

    /// Create with text range.
    pub const fn with_range(start: usize, end: usize) -> Self {
        Self::new(Some(TextRange::new(start, end)))
    }

    /// Builder: set text range.
    pub fn with_span(mut self, span: TextRange) -> Self {
        self.span = Some(span);
        self
    }

    /// Builder: set offset.
    pub const fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Check if span has text range.
    #[inline]
    pub const fn has_span(&self) -> bool {
        self.span.is_some()
    }

    /// Get span length if present.
    #[inline]
    pub fn span_length(&self) -> Option<usize> {
        self.span.as_ref().map(|s| s.len())
    }
}

impl Default for TextParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for TextParentData {}

impl Hash for TextParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
        self.container.hash(state);
        self.span.hash(state);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_cell_parent_data() {
        let data = TableCellParentData::zero()
            .at_cell(2, 3)
            .with_alignment(TableCellVerticalAlignment::Middle);

        assert_eq!(data.x, 2);
        assert_eq!(data.y, 3);
        assert_eq!(data.vertical_alignment, TableCellVerticalAlignment::Middle);
        assert_eq!(data.cell_position(), (2, 3));
        assert!(!data.is_first_cell());
    }

    #[test]
    fn test_table_cell_first() {
        let data = TableCellParentData::zero();
        assert!(data.is_first_cell());
    }

    #[test]
    fn test_text_range() {
        let range = TextRange::new(5, 10);

        assert_eq!(range.len(), 5);
        assert!(!range.is_empty());
        assert!(range.contains(5));
        assert!(range.contains(9));
        assert!(!range.contains(10));
    }

    #[test]
    fn test_text_parent_data() {
        let data = TextParentData::with_range(0, 10);

        assert!(data.has_span());
        assert_eq!(data.span_length(), Some(10));
    }

    #[test]
    fn test_text_parent_data_no_span() {
        let data = TextParentData::zero();

        assert!(!data.has_span());
        assert_eq!(data.span_length(), None);
    }
}
