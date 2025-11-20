//! RenderGrid - CSS Grid-inspired layout

// TODO: Migrate to Render<A>
// use flui_core::render::{RuntimeArity, LayoutContext, PaintContext, LegacyRender};
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, Size};
use std::collections::HashMap;

/// Track size specification for grid rows/columns
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridTrackSize {
    /// Fixed size in logical pixels
    Fixed(f32),
    /// Flexible size with flex factor (fr units)
    Flex(f32),
    /// Intrinsic size based on content
    Auto,
    /// Minimum size constraint
    MinContent,
    /// Maximum size constraint
    MaxContent,
}

impl Default for GridTrackSize {
    fn default() -> Self {
        GridTrackSize::Flex(1.0)
    }
}

/// Grid item placement specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPlacement {
    /// Column start (0-based index)
    pub column_start: usize,
    /// Number of columns to span
    pub column_span: usize,
    /// Row start (0-based index)
    pub row_start: usize,
    /// Number of rows to span
    pub row_span: usize,
}

impl GridPlacement {
    /// Create new grid placement
    pub fn new(column_start: usize, row_start: usize) -> Self {
        Self {
            column_start,
            column_span: 1,
            row_start,
            row_span: 1,
        }
    }

    /// Create with spans
    pub fn with_span(column_start: usize, column_span: usize, row_start: usize, row_span: usize) -> Self {
        Self {
            column_start,
            column_span: column_span.max(1),
            row_start,
            row_span: row_span.max(1),
        }
    }

    /// Set column span
    pub fn column_span(mut self, span: usize) -> Self {
        self.column_span = span.max(1);
        self
    }

    /// Set row span
    pub fn row_span(mut self, span: usize) -> Self {
        self.row_span = span.max(1);
        self
    }
}

impl Default for GridPlacement {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

/// RenderObject that implements CSS Grid-inspired layout
///
/// Arranges children in a grid with configurable row and column sizing.
/// Supports flexible (fr), fixed (px), and automatic (auto) track sizing.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderGrid, GridTrackSize};
///
/// // 3 columns: 1fr, 2fr, 100px
/// // 2 rows: auto, 1fr
/// let mut grid = RenderGrid::new(
///     vec![GridTrackSize::Flex(1.0), GridTrackSize::Flex(2.0), GridTrackSize::Fixed(100.0)],
///     vec![GridTrackSize::Auto, GridTrackSize::Flex(1.0)],
/// );
/// ```
#[derive(Debug)]
pub struct RenderGrid {
    /// Column track sizes
    pub column_sizes: Vec<GridTrackSize>,
    /// Row track sizes
    pub row_sizes: Vec<GridTrackSize>,
    /// Gap between columns
    pub column_gap: f32,
    /// Gap between rows
    pub row_gap: f32,
    /// Item placements (by child index)
    placements: HashMap<usize, GridPlacement>,

    // Cache for layout
    computed_column_widths: Vec<f32>,
    computed_row_heights: Vec<f32>,
    size: Size,
}

impl RenderGrid {
    /// Create new RenderGrid with column and row sizes
    pub fn new(column_sizes: Vec<GridTrackSize>, row_sizes: Vec<GridTrackSize>) -> Self {
        Self {
            column_sizes,
            row_sizes,
            column_gap: 0.0,
            row_gap: 0.0,
            placements: HashMap::new(),
            computed_column_widths: Vec::new(),
            computed_row_heights: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set column gap
    pub fn with_column_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap.max(0.0);
        self
    }

    /// Set row gap
    pub fn with_row_gap(mut self, gap: f32) -> Self {
        self.row_gap = gap.max(0.0);
        self
    }

    /// Set both gaps
    pub fn with_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap.max(0.0);
        self.row_gap = gap.max(0.0);
        self
    }

    /// Set placement for a specific child
    pub fn set_placement(&mut self, child_index: usize, placement: GridPlacement) {
        self.placements.insert(child_index, placement);
    }

    /// Get placement for a child (or auto-placement)
    fn get_placement(&self, child_index: usize) -> GridPlacement {
        self.placements
            .get(&child_index)
            .copied()
            .unwrap_or_else(|| {
                // Auto-placement: fill grid left-to-right, top-to-bottom
                let cols = self.column_sizes.len().max(1);
                let row = child_index / cols;
                let col = child_index % cols;
                GridPlacement::new(col, row)
            })
    }

    /// Compute column widths based on track sizes
    fn compute_column_widths(
        &self,
        children: &[flui_core::element::ElementId],
        tree: &flui_core::element::ElementTree,
        constraints: BoxConstraints,
    ) -> Vec<f32> {
        if self.column_sizes.is_empty() {
            return Vec::new();
        }

        let total_gap = self.column_gap * (self.column_sizes.len() - 1) as f32;
        let available_width = (constraints.max_width - total_gap).max(0.0);

        let mut widths = vec![0.0; self.column_sizes.len()];
        let mut flex_total = 0.0;
        let mut fixed_width_total = 0.0;

        // First pass: compute fixed and auto widths
        for (col, size) in self.column_sizes.iter().enumerate() {
            match size {
                GridTrackSize::Fixed(w) => {
                    widths[col] = *w;
                    fixed_width_total += *w;
                }
                GridTrackSize::Flex(factor) => {
                    flex_total += factor;
                }
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                    // Find max width of items in this column
                    let mut max_width: f32 = 0.0;
                    for (idx, &child_id) in children.iter().enumerate() {
                        let placement = self.get_placement(idx);
                        if placement.column_start == col && placement.column_span == 1 {
                            let child_constraints = BoxConstraints::new(
                                0.0,
                                f32::INFINITY,
                                0.0,
                                constraints.max_height,
                            );
                            let child_size = tree.layout_child(child_id, child_constraints);
                            max_width = max_width.max(child_size.width);
                        }
                    }
                    widths[col] = max_width;
                    fixed_width_total += max_width;
                }
            }
        }

        // Second pass: distribute remaining width to flex tracks
        if flex_total > 0.0 {
            let remaining_width = (available_width - fixed_width_total).max(0.0);
            let flex_unit = remaining_width / flex_total;

            for (col, size) in self.column_sizes.iter().enumerate() {
                if let GridTrackSize::Flex(factor) = size {
                    widths[col] = flex_unit * factor;
                }
            }
        }

        widths
    }

    /// Compute row heights based on track sizes
    fn compute_row_heights(
        &self,
        children: &[flui_core::element::ElementId],
        tree: &flui_core::element::ElementTree,
        column_widths: &[f32],
        constraints: BoxConstraints,
    ) -> Vec<f32> {
        if self.row_sizes.is_empty() {
            return Vec::new();
        }

        let total_gap = self.row_gap * (self.row_sizes.len() - 1) as f32;
        let available_height = (constraints.max_height - total_gap).max(0.0);

        let mut heights = vec![0.0; self.row_sizes.len()];
        let mut flex_total = 0.0;
        let mut fixed_height_total = 0.0;

        // First pass: compute fixed and auto heights
        for (row, size) in self.row_sizes.iter().enumerate() {
            match size {
                GridTrackSize::Fixed(h) => {
                    heights[row] = *h;
                    fixed_height_total += *h;
                }
                GridTrackSize::Flex(factor) => {
                    flex_total += factor;
                }
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                    // Find max height of items in this row
                    let mut max_height: f32 = 0.0;
                    for (idx, &child_id) in children.iter().enumerate() {
                        let placement = self.get_placement(idx);
                        if placement.row_start == row && placement.row_span == 1 {
                            let col = placement.column_start;
                            let child_width = if col < column_widths.len() {
                                column_widths[col]
                            } else {
                                0.0
                            };

                            let child_constraints = BoxConstraints::new(
                                child_width,
                                child_width,
                                0.0,
                                constraints.max_height,
                            );
                            let child_size = tree.layout_child(child_id, child_constraints);
                            max_height = max_height.max(child_size.height);
                        }
                    }
                    heights[row] = max_height;
                    fixed_height_total += max_height;
                }
            }
        }

        // Second pass: distribute remaining height to flex tracks
        if flex_total > 0.0 {
            let remaining_height = (available_height - fixed_height_total).max(0.0);
            let flex_unit = remaining_height / flex_total;

            for (row, size) in self.row_sizes.iter().enumerate() {
                if let GridTrackSize::Flex(factor) = size {
                    heights[row] = flex_unit * factor;
                }
            }
        }

        heights
    }
}

impl LegacyRender for RenderGrid {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let children = ctx.children.multi();
        let constraints = ctx.constraints;

        if self.column_sizes.is_empty() || self.row_sizes.is_empty() || children.is_empty() {
            self.computed_column_widths.clear();
            self.computed_row_heights.clear();
            self.size = Size::ZERO;
            return Size::ZERO;
        }

        // Compute track sizes
        self.computed_column_widths = self.compute_column_widths(children, tree, constraints);
        self.computed_row_heights =
            self.compute_row_heights(children, tree, &self.computed_column_widths, constraints);

        // Layout each child in its grid cell
        for (idx, &child_id) in children.iter().enumerate() {
            let placement = self.get_placement(idx);

            // Calculate child constraints from spanned tracks
            let mut child_width = 0.0;
            for i in 0..placement.column_span {
                let col = placement.column_start + i;
                if col < self.computed_column_widths.len() {
                    child_width += self.computed_column_widths[col];
                    if i > 0 {
                        child_width += self.column_gap;
                    }
                }
            }

            let mut child_height = 0.0;
            for i in 0..placement.row_span {
                let row = placement.row_start + i;
                if row < self.computed_row_heights.len() {
                    child_height += self.computed_row_heights[row];
                    if i > 0 {
                        child_height += self.row_gap;
                    }
                }
            }

            let child_constraints = BoxConstraints::new(
                child_width,
                child_width,
                child_height,
                child_height,
            );
            tree.layout_child(child_id, child_constraints);
        }

        // Calculate total size
        let total_width: f32 = self.computed_column_widths.iter().sum();
        let total_height: f32 = self.computed_row_heights.iter().sum();
        let gap_width = self.column_gap * (self.computed_column_widths.len().saturating_sub(1)) as f32;
        let gap_height = self.row_gap * (self.computed_row_heights.len().saturating_sub(1)) as f32;

        let size = constraints.constrain(Size::new(
            total_width + gap_width,
            total_height + gap_height,
        ));
        self.size = size;

        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let children = ctx.children.multi();
        let offset = ctx.offset;

        let mut canvas = Canvas::new();

        if self.column_sizes.is_empty() || self.row_sizes.is_empty() || children.is_empty() {
            return canvas;
        }

        // Paint each child at its grid position
        for (idx, &child_id) in children.iter().enumerate() {
            let placement = self.get_placement(idx);

            // Calculate child offset
            let mut x = offset.dx;
            for i in 0..placement.column_start {
                if i < self.computed_column_widths.len() {
                    x += self.computed_column_widths[i] + self.column_gap;
                }
            }

            let mut y = offset.dy;
            for i in 0..placement.row_start {
                if i < self.computed_row_heights.len() {
                    y += self.computed_row_heights[i] + self.row_gap;
                }
            }

            let child_offset = Offset::new(x, y);
            let child_canvas = tree.paint_child(child_id, child_offset);
            canvas.append_canvas(child_canvas);
        }

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_grid_new() {
        let grid = RenderGrid::new(
            vec![GridTrackSize::Flex(1.0), GridTrackSize::Fixed(100.0)],
            vec![GridTrackSize::Auto, GridTrackSize::Flex(1.0)],
        );

        assert_eq!(grid.column_sizes.len(), 2);
        assert_eq!(grid.row_sizes.len(), 2);
        assert_eq!(grid.column_gap, 0.0);
        assert_eq!(grid.row_gap, 0.0);
    }

    #[test]
    fn test_render_grid_with_gaps() {
        let grid = RenderGrid::new(
            vec![GridTrackSize::Flex(1.0)],
            vec![GridTrackSize::Flex(1.0)],
        )
        .with_column_gap(10.0)
        .with_row_gap(20.0);

        assert_eq!(grid.column_gap, 10.0);
        assert_eq!(grid.row_gap, 20.0);
    }

    #[test]
    fn test_render_grid_with_gap() {
        let grid = RenderGrid::new(
            vec![GridTrackSize::Flex(1.0)],
            vec![GridTrackSize::Flex(1.0)],
        )
        .with_gap(15.0);

        assert_eq!(grid.column_gap, 15.0);
        assert_eq!(grid.row_gap, 15.0);
    }

    #[test]
    fn test_grid_placement_new() {
        let placement = GridPlacement::new(1, 2);

        assert_eq!(placement.column_start, 1);
        assert_eq!(placement.column_span, 1);
        assert_eq!(placement.row_start, 2);
        assert_eq!(placement.row_span, 1);
    }

    #[test]
    fn test_grid_placement_with_span() {
        let placement = GridPlacement::with_span(0, 2, 1, 3);

        assert_eq!(placement.column_start, 0);
        assert_eq!(placement.column_span, 2);
        assert_eq!(placement.row_start, 1);
        assert_eq!(placement.row_span, 3);
    }

    #[test]
    fn test_grid_placement_span_methods() {
        let placement = GridPlacement::new(0, 0)
            .column_span(3)
            .row_span(2);

        assert_eq!(placement.column_span, 3);
        assert_eq!(placement.row_span, 2);
    }

    #[test]
    fn test_grid_placement_zero_span_clamped() {
        let placement = GridPlacement::with_span(0, 0, 0, 0);

        assert_eq!(placement.column_span, 1); // Min 1
        assert_eq!(placement.row_span, 1); // Min 1
    }

    #[test]
    fn test_grid_track_size_variants() {
        assert_eq!(GridTrackSize::Fixed(100.0), GridTrackSize::Fixed(100.0));
        assert_ne!(GridTrackSize::Fixed(100.0), GridTrackSize::Fixed(200.0));
        assert_eq!(GridTrackSize::Flex(1.0), GridTrackSize::Flex(1.0));
        assert_eq!(GridTrackSize::Auto, GridTrackSize::Auto);
        assert_eq!(GridTrackSize::MinContent, GridTrackSize::MinContent);
        assert_eq!(GridTrackSize::MaxContent, GridTrackSize::MaxContent);
    }

    #[test]
    fn test_grid_track_size_default() {
        let default = GridTrackSize::default();
        assert_eq!(default, GridTrackSize::Flex(1.0));
    }

    #[test]
    fn test_grid_placement_default() {
        let default = GridPlacement::default();
        assert_eq!(default.column_start, 0);
        assert_eq!(default.row_start, 0);
        assert_eq!(default.column_span, 1);
        assert_eq!(default.row_span, 1);
    }

    #[test]
    fn test_arity_is_variable() {
        let grid = RenderGrid::new(
            vec![GridTrackSize::Flex(1.0)],
            vec![GridTrackSize::Flex(1.0)],
        );

        assert_eq!(grid.arity(), RuntimeArity::Variable);
    }
}
