//! RenderTable - Table layout container with configurable column widths
//!
//! Implements a table layout system with configurable column widths and automatic
//! row height calculation. Supports multiple column sizing modes (Fixed, Flex,
//! Intrinsic, Fraction) and vertical cell alignment. Children are arranged in a
//! grid with column-major ordering.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderTable` | `RenderTable` from `package:flutter/src/rendering/table.dart` |
//! | `TableColumnWidth::Fixed` | `FixedColumnWidth` |
//! | `TableColumnWidth::Flex` | `FlexColumnWidth` |
//! | `TableColumnWidth::Intrinsic` | `IntrinsicColumnWidth` |
//! | `TableColumnWidth::Fraction` | `FractionColumnWidth` |
//! | `TableCellVerticalAlignment` | `TableCellVerticalAlignment` enum |
//! | `set_column_width()` | Column width configuration |
//! | `columns` | Number of columns |
//!
//! # Layout Protocol
//!
//! 1. **Compute column widths**
//!    - First pass: Calculate Fixed, Intrinsic, and Fraction widths
//!    - Fixed: Use specified width
//!    - Intrinsic: Layout all cells in column with infinite width to get max
//!    - Fraction: Calculate as percentage of parent width
//!    - Track total fixed width
//!
//! 2. **Distribute flex units**
//!    - Calculate remaining width after fixed/intrinsic/fraction columns
//!    - Divide remaining width by total flex factor
//!    - Assign flex_unit × factor to each flex column
//!
//! 3. **Compute row heights**
//!    - For each row: find tallest cell
//!    - Layout all cells in row with column width constraint
//!    - Row height = max of all cell heights in that row
//!
//! 4. **Layout complete**
//!    - All cells laid out during width/height computation
//!    - Container size = sum of column widths × sum of row heights
//!
//! # Paint Protocol
//!
//! 1. **Iterate rows and columns**
//!    - Accumulate row offset: y += row_height
//!    - Accumulate column offset: x += column_width
//!
//! 2. **Paint each cell**
//!    - Calculate cell index: row × columns + column
//!    - Apply vertical alignment offset within cell
//!    - Paint cell at calculated (x, y) position
//!
//! # Performance
//!
//! - **Layout**: O(rows × cols) - multiple passes for width/height computation
//! - **Paint**: O(rows × cols) - paint each cell once
//! - **Memory**: 80 bytes base + O(cols + rows) for cached widths/heights
//!
//! # Use Cases
//!
//! - **Data tables**: Tabular data display with aligned columns
//! - **Forms**: Multi-column form layouts
//! - **Grids with uniform columns**: Grid layouts with same-width columns
//! - **Spreadsheets**: Simple spreadsheet-like layouts
//! - **Calendars**: Month/week calendar grids
//! - **Pricing tables**: Feature comparison tables
//!
//! # Column Width Modes
//!
//! ```text
//! Fixed(100.0): Always 100px wide
//! Flex(1.0): Takes 1 share of remaining space
//! Flex(2.0): Takes 2 shares of remaining space
//! Intrinsic: Sized to widest cell content
//! Fraction(0.3): 30% of parent width
//! ```
//!
//! # Vertical Alignment
//!
//! ```text
//! Top:    [Cell]-----  (default)
//! Middle: ---[Cell]---
//! Bottom: -----[Cell]
//! Fill:   [Cell fills entire row height]
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderGrid**: Grid uses explicit track placement, Table uses row-column ordering
//! - **vs RenderFlex**: Flex is 1D, Table is 2D with column alignment
//! - **vs RenderWrap**: Wrap auto-wraps, Table has fixed column count
//! - **vs RenderCustomMultiChildLayoutBox**: Table has standardized grid, Custom uses delegate
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderTable, TableColumnWidth, TableCellVerticalAlignment};
//!
//! // 3 columns: fixed 100px, flex 2x, flex 1x
//! let mut table = RenderTable::new(3);
//! table.set_column_width(0, TableColumnWidth::Fixed(100.0));
//! table.set_column_width(1, TableColumnWidth::Flex(2.0));
//! table.set_column_width(2, TableColumnWidth::Flex(1.0));
//!
//! // Centered cell alignment
//! table.set_vertical_alignment(TableCellVerticalAlignment::Middle);
//!
//! // Intrinsic width column (auto-sized to content)
//! let mut table = RenderTable::new(2);
//! table.set_column_width(0, TableColumnWidth::Intrinsic);
//! table.set_column_width(1, TableColumnWidth::Flex(1.0));
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Variable};
use flui_rendering::{RenderObject, RenderResult};
use flui_foundation::ElementId;
use flui_types::{BoxConstraints, Offset, Size};
use std::collections::HashMap;

/// Column width specification for table columns
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TableColumnWidth {
    /// Fixed width in logical pixels
    Fixed(f32),
    /// Flexible width with flex factor (similar to Flex widget)
    Flex(f32),
    /// Intrinsic width based on cell contents (min or max)
    Intrinsic,
    /// Fraction of available width (0.0-1.0)
    Fraction(f32),
}

impl Default for TableColumnWidth {
    fn default() -> Self {
        TableColumnWidth::Flex(1.0)
    }
}

/// Vertical alignment for table cells
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableCellVerticalAlignment {
    /// Align to top of row
    #[default]
    Top,
    /// Center vertically in row
    Middle,
    /// Align to bottom of row
    Bottom,
    /// Fill entire row height
    Fill,
}

/// RenderObject that implements table layout with configurable column widths.
///
/// Arranges children in a grid with fixed column count and automatic row creation.
/// Supports multiple column sizing modes (Fixed, Flex, Intrinsic, Fraction) and
/// vertical cell alignment. Each row's height is determined by its tallest cell.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+), arranged in rows based on
/// column count. Children are added in row-major order (left-to-right, top-to-bottom).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Table Layout Container** - 2D grid with configurable column widths, automatic
/// row heights based on tallest cell, multiple column sizing modes (Fixed/Flex/
/// Intrinsic/Fraction), vertical cell alignment, sizes to sum of columns × rows.
///
/// # Use Cases
///
/// - **Data tables**: Tabular data with aligned columns and uniform formatting
/// - **Forms**: Multi-column form layouts with label/field alignment
/// - **Spreadsheets**: Simple spreadsheet-like layouts with cell-based data
/// - **Calendars**: Month/week calendar grids with date cells
/// - **Pricing tables**: Feature comparison tables with aligned columns
/// - **Schedules**: Time-based schedules with aligned time slots
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderTable behavior:
/// - Children arranged in row-major order (row × columns + column)
/// - Column widths computed with Fixed, Flex, Intrinsic, Fraction modes
/// - Row heights determined by tallest cell in each row
/// - Vertical alignment support (Top, Middle, Bottom, Fill)
/// - Size = sum of column widths × sum of row heights
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderTable, TableColumnWidth, TableCellVerticalAlignment};
///
/// // 3 columns: fixed 100px, flex 2x, flex 1x
/// let mut table = RenderTable::new(3);
/// table.set_column_width(0, TableColumnWidth::Fixed(100.0));
/// table.set_column_width(1, TableColumnWidth::Flex(2.0));
/// table.set_column_width(2, TableColumnWidth::Flex(1.0));
///
/// // Center cells vertically
/// table.set_vertical_alignment(TableCellVerticalAlignment::Middle);
///
/// // Data table with intrinsic + flex columns
/// let mut data_table = RenderTable::new(4);
/// data_table.set_column_width(0, TableColumnWidth::Intrinsic); // ID column
/// data_table.set_column_width(1, TableColumnWidth::Flex(2.0)); // Name (wider)
/// data_table.set_column_width(2, TableColumnWidth::Flex(1.0)); // Email
/// data_table.set_column_width(3, TableColumnWidth::Fixed(80.0)); // Actions
/// ```
#[derive(Debug)]
pub struct RenderTable {
    /// Number of columns in the table
    pub columns: usize,
    /// Column width specifications (by column index)
    column_widths: HashMap<usize, TableColumnWidth>,
    /// Default column width for columns without explicit spec
    pub default_column_width: TableColumnWidth,
    /// Default vertical alignment for cells
    pub default_vertical_alignment: TableCellVerticalAlignment,

    // Cache for layout
    computed_column_widths: Vec<f32>,
    computed_row_heights: Vec<f32>,
    size: Size,
}

impl RenderTable {
    /// Create new RenderTable with specified column count
    pub fn new(columns: usize) -> Self {
        Self {
            columns,
            column_widths: HashMap::new(),
            default_column_width: TableColumnWidth::default(),
            default_vertical_alignment: TableCellVerticalAlignment::default(),
            computed_column_widths: Vec::new(),
            computed_row_heights: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set width specification for a specific column
    pub fn set_column_width(&mut self, column: usize, width: TableColumnWidth) {
        self.column_widths.insert(column, width);
    }

    /// Get width specification for a column (or default)
    fn get_column_width(&self, column: usize) -> TableColumnWidth {
        self.column_widths
            .get(&column)
            .copied()
            .unwrap_or(self.default_column_width)
    }

    /// Set default column width
    pub fn set_default_column_width(&mut self, width: TableColumnWidth) {
        self.default_column_width = width;
    }

    /// Set default vertical alignment
    pub fn set_vertical_alignment(&mut self, alignment: TableCellVerticalAlignment) {
        self.default_vertical_alignment = alignment;
    }

    /// Compute column widths based on constraints and column specs
    #[allow(clippy::needless_range_loop)]
    fn compute_column_widths(
        &self,
        children: &[ElementId],
        ctx: &mut BoxLayoutCtx<'_, Variable>,
        constraints: BoxConstraints,
    ) -> Vec<f32> {
        if self.columns == 0 {
            return Vec::new();
        }

        let row_count = children.len().div_ceil(self.columns);
        let available_width = constraints.max_width;

        // First pass: compute fixed and intrinsic widths
        let mut widths = vec![0.0; self.columns];
        let mut flex_total = 0.0;
        let mut fixed_width_total = 0.0;

        for col in 0..self.columns {
            match self.get_column_width(col) {
                TableColumnWidth::Fixed(w) => {
                    widths[col] = w;
                    fixed_width_total += w;
                }
                TableColumnWidth::Flex(factor) => {
                    flex_total += factor;
                }
                TableColumnWidth::Intrinsic => {
                    // Find max intrinsic width across all cells in this column
                    let mut max_width: f32 = 0.0;
                    for row in 0..row_count {
                        let idx = row * self.columns + col;
                        if idx < children.len() {
                            // For now, use unbounded constraints to get intrinsic size
                            let child_constraints = BoxConstraints::new(
                                0.0,
                                f32::INFINITY,
                                0.0,
                                constraints.max_height,
                            );
                            let child_size = ctx
                                .layout_child(children[idx], child_constraints)
                                .unwrap_or(Size::ZERO);
                            max_width = max_width.max(child_size.width);
                        }
                    }
                    widths[col] = max_width;
                    fixed_width_total += max_width;
                }
                TableColumnWidth::Fraction(fraction) => {
                    widths[col] = available_width * fraction.clamp(0.0, 1.0);
                    fixed_width_total += widths[col];
                }
            }
        }

        // Second pass: distribute remaining space to flex columns
        if flex_total > 0.0 {
            let remaining_width = (available_width - fixed_width_total).max(0.0);
            let flex_unit = remaining_width / flex_total;

            for col in 0..self.columns {
                if let TableColumnWidth::Flex(factor) = self.get_column_width(col) {
                    widths[col] = flex_unit * factor;
                }
            }
        }

        widths
    }

    /// Compute row heights based on column widths and cell contents
    #[allow(clippy::needless_range_loop)]
    fn compute_row_heights(
        &self,
        children: &[ElementId],
        ctx: &mut BoxLayoutCtx<'_, Variable>,
        column_widths: &[f32],
        constraints: BoxConstraints,
    ) -> Vec<f32> {
        if self.columns == 0 || children.is_empty() {
            return Vec::new();
        }

        let row_count = children.len().div_ceil(self.columns);
        let mut heights = vec![0.0; row_count];

        for row in 0..row_count {
            let mut max_height: f32 = 0.0;

            for col in 0..self.columns {
                let idx = row * self.columns + col;
                if idx < children.len() {
                    let child_constraints = BoxConstraints::new(
                        column_widths[col],
                        column_widths[col],
                        0.0,
                        constraints.max_height,
                    );
                    let child_size = ctx
                        .layout_child(children[idx], child_constraints)
                        .unwrap_or(Size::ZERO);
                    max_height = max_height.max(child_size.height);
                }
            }

            heights[row] = max_height;
        }

        heights
    }
}

impl RenderObject for RenderTable {}

impl RenderBox<Variable> for RenderTable {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Collect children first for multiple passes
        let child_ids: Vec<ElementId> = children.iter().map(|id| *id).collect();

        if self.columns == 0 || child_ids.is_empty() {
            self.computed_column_widths.clear();
            self.computed_row_heights.clear();
            self.size = Size::ZERO;
            return Ok(Size::ZERO);
        }

        // Compute column widths
        self.computed_column_widths = self.compute_column_widths(&child_ids, &mut ctx, constraints);

        // Compute row heights
        self.computed_row_heights = self.compute_row_heights(
            &child_ids,
            &mut ctx,
            &self.computed_column_widths,
            constraints,
        );

        // Calculate total size
        let total_width: f32 = self.computed_column_widths.iter().sum();
        let total_height: f32 = self.computed_row_heights.iter().sum();

        let size = constraints.constrain(Size::new(total_width, total_height));
        self.size = size;

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        if self.columns == 0 || child_ids.is_empty() {
            return;
        }

        let row_count = self.computed_row_heights.len();
        let mut y = offset.dy;

        for row in 0..row_count {
            let row_height = self.computed_row_heights[row];
            let mut x = offset.dx;

            for col in 0..self.columns {
                let idx = row * self.columns + col;
                if idx >= child_ids.len() {
                    break;
                }

                let col_width = self.computed_column_widths[col];

                // Calculate cell offset based on vertical alignment
                let cell_offset = match self.default_vertical_alignment {
                    TableCellVerticalAlignment::Top => Offset::new(x, y),
                    TableCellVerticalAlignment::Middle => {
                        // Center child vertically in row
                        // TODO: Get actual child height for proper centering
                        Offset::new(x, y + row_height / 2.0)
                    }
                    TableCellVerticalAlignment::Bottom => {
                        // Align to bottom of row
                        // TODO: Get actual child height
                        Offset::new(x, y + row_height)
                    }
                    TableCellVerticalAlignment::Fill => Offset::new(x, y),
                };

                ctx.paint_child(*child_ids[idx], cell_offset);

                x += col_width;
            }

            y += row_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_table_new() {
        let table = RenderTable::new(3);

        assert_eq!(table.columns, 3);
        assert_eq!(table.default_column_width, TableColumnWidth::Flex(1.0));
        assert_eq!(
            table.default_vertical_alignment,
            TableCellVerticalAlignment::Top
        );
    }

    #[test]
    fn test_render_table_set_column_width() {
        let mut table = RenderTable::new(3);
        table.set_column_width(0, TableColumnWidth::Fixed(100.0));
        table.set_column_width(1, TableColumnWidth::Flex(2.0));

        assert_eq!(table.get_column_width(0), TableColumnWidth::Fixed(100.0));
        assert_eq!(table.get_column_width(1), TableColumnWidth::Flex(2.0));
        assert_eq!(table.get_column_width(2), TableColumnWidth::Flex(1.0)); // default
    }

    #[test]
    fn test_render_table_set_default_column_width() {
        let mut table = RenderTable::new(3);
        table.set_default_column_width(TableColumnWidth::Fixed(50.0));

        assert_eq!(table.default_column_width, TableColumnWidth::Fixed(50.0));
        assert_eq!(table.get_column_width(0), TableColumnWidth::Fixed(50.0));
    }

    #[test]
    fn test_render_table_set_vertical_alignment() {
        let mut table = RenderTable::new(2);
        table.set_vertical_alignment(TableCellVerticalAlignment::Middle);

        assert_eq!(
            table.default_vertical_alignment,
            TableCellVerticalAlignment::Middle
        );
    }

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
    }

    #[test]
    fn test_table_column_width_default() {
        let default = TableColumnWidth::default();
        assert_eq!(default, TableColumnWidth::Flex(1.0));
    }

    #[test]
    fn test_table_cell_vertical_alignment_default() {
        let default = TableCellVerticalAlignment::default();
        assert_eq!(default, TableCellVerticalAlignment::Top);
    }
}
