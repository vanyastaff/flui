//! Table layout types
//!
//! Types for configuring table column widths and cell alignment.
//! Based on Flutter's Table widget API.

/// Column width specification for table columns.
///
/// Determines how columns in a table are sized. Each variant represents
/// a different sizing strategy.
///
/// # Examples
///
/// ```
/// use flui_types::layout::TableColumnWidth;
///
/// // Fixed width of 100 pixels
/// let fixed = TableColumnWidth::Fixed(100.0);
///
/// // Flexible width with flex factor 2
/// let flex = TableColumnWidth::Flex(2.0);
///
/// // 30% of available width
/// let fraction = TableColumnWidth::Fraction(0.3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TableColumnWidth {
    /// Fixed width in logical pixels.
    ///
    /// The column will always be exactly this width regardless of content.
    Fixed(f32),

    /// Flexible width with flex factor.
    ///
    /// Similar to `Flex` widget - distributes remaining space proportionally.
    /// A column with `Flex(2.0)` will be twice as wide as one with `Flex(1.0)`.
    Flex(f32),

    /// Intrinsic width based on cell contents.
    ///
    /// The column will be sized to fit the widest cell content.
    /// This requires an additional layout pass to measure content.
    Intrinsic,

    /// Fraction of available width (0.0-1.0).
    ///
    /// For example, `Fraction(0.25)` means 25% of the table's available width.
    /// Values are clamped to the 0.0-1.0 range.
    Fraction(f32),
}

impl Default for TableColumnWidth {
    fn default() -> Self {
        TableColumnWidth::Flex(1.0)
    }
}

/// Vertical alignment for table cells.
///
/// Determines how cell content is aligned vertically within the row height.
///
/// # Examples
///
/// ```
/// use flui_types::layout::TableCellVerticalAlignment;
///
/// let alignment = TableCellVerticalAlignment::Middle;
/// assert_ne!(alignment, TableCellVerticalAlignment::Top);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TableCellVerticalAlignment {
    /// Align content to the top of the row.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_column_width_variants() {
        assert_eq!(
            TableColumnWidth::Fixed(100.0),
            TableColumnWidth::Fixed(100.0)
        );
        assert_ne!(
            TableColumnWidth::Fixed(100.0),
            TableColumnWidth::Fixed(200.0)
        );
        assert_eq!(TableColumnWidth::Flex(1.0), TableColumnWidth::Flex(1.0));
        assert_eq!(TableColumnWidth::Intrinsic, TableColumnWidth::Intrinsic);
        assert_eq!(
            TableColumnWidth::Fraction(0.5),
            TableColumnWidth::Fraction(0.5)
        );
    }

    #[test]
    fn test_table_column_width_default() {
        let default = TableColumnWidth::default();
        assert_eq!(default, TableColumnWidth::Flex(1.0));
    }

    #[test]
    fn test_table_cell_vertical_alignment_variants() {
        assert_eq!(
            TableCellVerticalAlignment::Top,
            TableCellVerticalAlignment::Top
        );
        assert_ne!(
            TableCellVerticalAlignment::Top,
            TableCellVerticalAlignment::Middle
        );
        assert_eq!(
            TableCellVerticalAlignment::Bottom,
            TableCellVerticalAlignment::Bottom
        );
        assert_eq!(
            TableCellVerticalAlignment::Fill,
            TableCellVerticalAlignment::Fill
        );
        assert_eq!(
            TableCellVerticalAlignment::Baseline,
            TableCellVerticalAlignment::Baseline
        );
    }

    #[test]
    fn test_table_cell_vertical_alignment_default() {
        let default = TableCellVerticalAlignment::default();
        assert_eq!(default, TableCellVerticalAlignment::Top);
    }
}
