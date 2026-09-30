//! Specialized parent data types - Table and Text layouts.

use std::hash::{Hash, Hasher};

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, canonical_bits_f64};

// `TextRange` used to be declared here as well; flui-painting's typography owns
// the concept and its copy is a strict superset. Imported privately, not re-exported: every
// other consumer in the workspace already reaches for
// `flui_painting::typography::TextRange` directly, so a `pub use` here would be
// public surface with no consumer.
use flui_painting::typography::TextRange;

use super::{base::ParentData, container_mixin::ContainerParentDataMixin};

/// Where a `Table`/`RenderTable` cell should be placed vertically within its
/// row's resolved height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TableCellVerticalAlignment {
    /// Align to the top of the row.
    #[default]
    Top,

    /// Center content vertically within the row.
    Middle,

    /// Align content to the bottom of the row.
    Bottom,

    /// Stretch content to fill the entire row height.
    Fill,

    /// Align content based on text baseline.
    ///
    /// Useful when mixing text of different sizes in a row.
    Baseline,

    /// Size the cell to the row's tallest cell.
    ///
    /// The difference from [`Fill`](Self::Fill) is which pass the cell takes
    /// part in. An `IntrinsicHeight` cell is measured first, so its own
    /// content contributes to how tall the row becomes, and is then stretched
    /// to that height. A `Fill` cell is not measured at all — it only
    /// stretches — so a row whose cells are all `Fill` has zero height, while
    /// a row whose cells are all `IntrinsicHeight` is as tall as its tallest
    /// cell and every cell in it ends up that tall.
    IntrinsicHeight,
}

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

    /// Column index (0-based), as of the cell's last layout.
    pub x: usize,

    /// Row index (0-based), as of the cell's last layout.
    pub y: usize,

    /// Vertical alignment within the cell.
    ///
    /// `None` defers to `RenderTable::default_vertical_alignment` — Flutter
    /// parity: `TableCellParentData.verticalAlignment` is `TableCellVerticalAlignment?`
    /// (`table.dart:20`), not a value that forces `Top` on every unset cell.
    pub vertical_alignment: Option<TableCellVerticalAlignment>,
}

impl TableCellParentData {
    /// Create with cell position and an explicit vertical alignment.
    pub const fn new(x: usize, y: usize, vertical_alignment: TableCellVerticalAlignment) -> Self {
        Self {
            offset: Offset::ZERO,
            x,
            y,
            vertical_alignment: Some(vertical_alignment),
        }
    }

    /// Create at cell (0, 0) with no explicit alignment (defers to the
    /// table's `default_vertical_alignment`).
    pub const fn zero() -> Self {
        Self {
            offset: Offset::ZERO,
            x: 0,
            y: 0,
            vertical_alignment: None,
        }
    }

    /// Builder: set cell position.
    pub const fn at_cell(mut self, x: usize, y: usize) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Builder: set an explicit vertical alignment, overriding the table's default.
    pub const fn with_alignment(mut self, alignment: TableCellVerticalAlignment) -> Self {
        self.vertical_alignment = Some(alignment);
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
        canonical_bits_f64(self.offset.dx).hash(state);
        canonical_bits_f64(self.offset.dy).hash(state);
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
/// Combines container functionality (for inline spans) with text range
/// information.
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
        self.span.as_ref().map(TextRange::len)
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
        canonical_bits_f64(self.offset.dx).hash(state);
        canonical_bits_f64(self.offset.dy).hash(state);
        self.container.hash(state);
        self.span.hash(state);
    }
}
