//! RenderTable - Table layout with configurable column widths
//!
//! A table where the columns and rows are sized to fit the contents of the cells.
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderTable-class.html>

use crate::core::{BoxProtocol, LayoutContext, LayoutTree, PaintContext, RenderBox, Variable};
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

/// RenderObject that implements table layout
///
/// Arranges children in a grid with configurable column widths and row heights.
/// Each row can have different height based on its tallest cell.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderTable, TableColumnWidth};
///
/// // 3 columns: fixed 100px, flex 2x, flex 1x
/// let mut table = RenderTable::new(3);
/// table.set_column_width(0, TableColumnWidth::Fixed(100.0));
/// table.set_column_width(1, TableColumnWidth::Flex(2.0));
/// table.set_column_width(2, TableColumnWidth::Flex(1.0));
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
    /// Cached child sizes from layout (indexed by child order)
    child_sizes: Vec<Size>,
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
            child_sizes: Vec::new(),
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
    fn compute_column_widths<T>(
        &self,
        children: &[ElementId],
        ctx: &mut LayoutContext<'_, T, Variable, BoxProtocol>,
        constraints: BoxConstraints,
    ) -> Vec<f32>
    where
        T: LayoutTree,
    {
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
                            let child_size = ctx.layout_child(children[idx], child_constraints);
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

    /// Compute row heights and cache child sizes
    ///
    /// Returns (row_heights, child_sizes) tuple.
    /// child_sizes is indexed by child order (row * columns + col).
    #[allow(clippy::needless_range_loop)]
    fn compute_row_heights_and_sizes<T>(
        &self,
        children: &[ElementId],
        ctx: &mut LayoutContext<'_, T, Variable, BoxProtocol>,
        column_widths: &[f32],
        constraints: BoxConstraints,
    ) -> (Vec<f32>, Vec<Size>)
    where
        T: LayoutTree,
    {
        if self.columns == 0 || children.is_empty() {
            return (Vec::new(), Vec::new());
        }

        let row_count = children.len().div_ceil(self.columns);
        let mut heights = vec![0.0; row_count];
        let mut sizes = vec![Size::ZERO; children.len()];

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
                    let child_size = ctx.layout_child(children[idx], child_constraints);
                    sizes[idx] = child_size;
                    max_height = max_height.max(child_size.height);
                }
            }

            heights[row] = max_height;
        }

        (heights, sizes)
    }
}

impl RenderBox<Variable> for RenderTable {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Collect children first for multiple passes
        let child_ids: Vec<_> = children.iter().collect();

        if self.columns == 0 || child_ids.is_empty() {
            self.computed_column_widths.clear();
            self.computed_row_heights.clear();
            self.child_sizes.clear();
            self.size = Size::ZERO;
            return Size::ZERO;
        }

        // Compute column widths
        self.computed_column_widths = self.compute_column_widths(&child_ids, &mut ctx, constraints);

        // Compute row heights and cache child sizes
        let (row_heights, child_sizes) = self.compute_row_heights_and_sizes(
            &child_ids,
            &mut ctx,
            &self.computed_column_widths,
            constraints,
        );
        self.computed_row_heights = row_heights;
        self.child_sizes = child_sizes;

        // Calculate total size
        let total_width: f32 = self.computed_column_widths.iter().sum();
        let total_height: f32 = self.computed_row_heights.iter().sum();

        let size = constraints.constrain(Size::new(total_width, total_height));
        self.size = size;

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: crate::core::PaintTree,
    {
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
                let child_size = self.child_sizes.get(idx).copied().unwrap_or(Size::ZERO);

                // Calculate cell offset based on vertical alignment
                let cell_offset = match self.default_vertical_alignment {
                    TableCellVerticalAlignment::Top => Offset::new(x, y),
                    TableCellVerticalAlignment::Middle => {
                        // Center child vertically in row
                        let vertical_offset = (row_height - child_size.height) / 2.0;
                        Offset::new(x, y + vertical_offset)
                    }
                    TableCellVerticalAlignment::Bottom => {
                        // Align to bottom of row
                        let vertical_offset = row_height - child_size.height;
                        Offset::new(x, y + vertical_offset)
                    }
                    TableCellVerticalAlignment::Fill => Offset::new(x, y),
                };

                ctx.paint_child(child_ids[idx], cell_offset);

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
