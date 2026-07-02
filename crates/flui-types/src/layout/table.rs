//! Table layout types
//!
//! Types for configuring table column widths and cell alignment.
//! Based on Flutter's Table widget API.

/// How one table column's width is decided.
///
/// Mirrors Flutter's `TableColumnWidth` hierarchy (`rendering/table.dart`).
/// The leaf variants (`Fixed`/`Flex`/`Intrinsic`/`Fraction`) are cheap value
/// specs; [`Max`](Self::Max)/[`Min`](Self::Min) are *combinators* that wrap
/// two other specs — which is why this enum owns them behind [`Box`] and is
/// therefore [`Clone`] but not `Copy` (a recursive type cannot be `Copy`).
#[derive(Debug, Clone, PartialEq)]
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

    /// The larger of two column-width specs, evaluated independently.
    ///
    /// For "10% of the container width or 100px, whichever is bigger", use
    /// `TableColumnWidth::max(Fixed(100.0), Fraction(0.1))`. Both `a` and `b`
    /// are evaluated (so if either is expensive, so is this). Flutter parity:
    /// `MaxColumnWidth` (`rendering/table.dart:235`).
    Max(Box<TableColumnWidth>, Box<TableColumnWidth>),

    /// The smaller of two column-width specs, evaluated independently.
    ///
    /// For "10% of the container width but never bigger than 100px", use
    /// `TableColumnWidth::min(Fixed(100.0), Fraction(0.1))`. Both `a` and `b`
    /// are evaluated. Flutter parity: `MinColumnWidth`
    /// (`rendering/table.dart:287`).
    Min(Box<TableColumnWidth>, Box<TableColumnWidth>),
}

impl TableColumnWidth {
    /// The larger of `a` and `b` (see [`TableColumnWidth::Max`]).
    #[must_use]
    pub fn max(a: TableColumnWidth, b: TableColumnWidth) -> Self {
        TableColumnWidth::Max(Box::new(a), Box::new(b))
    }

    /// The smaller of `a` and `b` (see [`TableColumnWidth::Min`]).
    #[must_use]
    pub fn min(a: TableColumnWidth, b: TableColumnWidth) -> Self {
        TableColumnWidth::Min(Box::new(a), Box::new(b))
    }
}

impl Default for TableColumnWidth {
    #[inline]
    fn default() -> Self {
        TableColumnWidth::Flex(1.0)
    }
}

/// Where a `Table`/`RenderTable` cell should be placed vertically within its
/// row's resolved height.
///
/// The canonical home for this type — `flui_rendering::parent_data::table_text::TableCellParentData`
/// re-points at this definition rather than keeping an independent copy.
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
}
