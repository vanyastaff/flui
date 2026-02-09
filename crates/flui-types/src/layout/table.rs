//! Table layout types
//!
//! Types for configuring table column widths and cell alignment.
//! Based on Flutter's Table widget API.

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
    #[inline]
    fn default() -> Self {
        TableColumnWidth::Flex(1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TableCellVerticalAlignment {
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
