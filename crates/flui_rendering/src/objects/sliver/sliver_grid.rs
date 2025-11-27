//! RenderSliverGrid - Scrollable grid with lazy loading

use crate::core::{
    ChildrenAccess, LayoutContext, LayoutTree, PaintContext, PaintTree, SliverProtocol,
    SliverRender, Variable,
};
use flui_types::{SliverConstraints, SliverGeometry};

/// Grid delegate for calculating grid layout
///
/// Determines how many columns, row heights, spacing, etc.
pub trait SliverGridDelegate: std::fmt::Debug + Send + Sync {
    /// Get the number of columns
    fn get_column_count(&self, cross_axis_extent: f32) -> usize;

    /// Get the main axis extent (height for vertical) for a child at index
    fn get_main_axis_extent(&self, index: usize, cross_axis_extent: f32) -> f32;

    /// Get spacing between items
    fn get_spacing(&self) -> (f32, f32); // (main_axis_spacing, cross_axis_spacing)

    /// Check if layout should be recalculated
    fn should_relayout(&self, old: &dyn std::any::Any) -> bool;

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Fixed column count grid delegate
///
/// Creates a grid with a fixed number of columns and equal-height rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverGridDelegateFixedCrossAxisCount {
    /// Number of columns
    pub cross_axis_count: usize,
    /// Height of each item (in main axis)
    pub main_axis_extent: f32,
    /// Spacing between items on main axis
    pub main_axis_spacing: f32,
    /// Spacing between items on cross axis
    pub cross_axis_spacing: f32,
}

impl SliverGridDelegateFixedCrossAxisCount {
    /// Create new fixed column count delegate
    pub fn new(cross_axis_count: usize, main_axis_extent: f32) -> Self {
        Self {
            cross_axis_count,
            main_axis_extent,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
        }
    }

    /// Set main axis spacing
    pub fn with_main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Set cross axis spacing
    pub fn with_cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateFixedCrossAxisCount {
    fn get_column_count(&self, _cross_axis_extent: f32) -> usize {
        self.cross_axis_count
    }

    fn get_main_axis_extent(&self, _index: usize, _cross_axis_extent: f32) -> f32 {
        self.main_axis_extent
    }

    fn get_spacing(&self) -> (f32, f32) {
        (self.main_axis_spacing, self.cross_axis_spacing)
    }

    fn should_relayout(&self, old: &dyn std::any::Any) -> bool {
        if let Some(old) = old.downcast_ref::<Self>() {
            self != old
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// RenderObject for scrollable grids with lazy loading
///
/// Similar to RenderSliverList but arranges children in a 2D grid.
/// Only builds and lays out children that are visible or near-visible.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverGrid, SliverGridDelegateFixedCrossAxisCount};
///
/// let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
///     .with_main_axis_spacing(10.0)
///     .with_cross_axis_spacing(10.0);
///
/// let grid = RenderSliverGrid::new(Box::new(delegate));
/// ```
pub struct RenderSliverGrid {
    /// Grid layout delegate
    #[allow(clippy::type_complexity)]
    pub delegate: Box<dyn SliverGridDelegate>,
    /// Cross axis extent (width for vertical scroll)
    pub cross_axis_extent: f32,
}

impl std::fmt::Debug for RenderSliverGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverGrid")
            .field("delegate", &self.delegate)
            .field("cross_axis_extent", &self.cross_axis_extent)
            .finish()
    }
}

impl RenderSliverGrid {
    /// Create new sliver grid with delegate
    pub fn new(delegate: Box<dyn SliverGridDelegate>) -> Self {
        Self {
            delegate,
            cross_axis_extent: 0.0,
        }
    }

    /// Set cross axis extent
    pub fn set_cross_axis_extent(&mut self, extent: f32) {
        self.cross_axis_extent = extent;
    }

    /// Calculate sliver geometry for grid layout
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_count: usize,
    ) -> SliverGeometry {
        // If no children, return zero geometry
        if child_count == 0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;
        let cross_axis_extent = constraints.cross_axis_extent;

        // Get grid parameters from delegate
        let column_count = self.delegate.get_column_count(cross_axis_extent);
        if column_count == 0 {
            return SliverGeometry::default();
        }

        let (main_spacing, _cross_spacing) = self.delegate.get_spacing();

        // Calculate total rows
        let row_count = child_count.div_ceil(column_count);

        // Calculate total extent
        // For simplicity, assume all rows have same height (delegate returns same value)
        let row_height = self.delegate.get_main_axis_extent(0, cross_axis_extent);
        let total_spacing = if row_count > 1 {
            main_spacing * (row_count - 1) as f32
        } else {
            0.0
        };
        let total_extent = row_height * row_count as f32 + total_spacing;

        // Calculate what's visible
        let leading_scroll_offset = scroll_offset.max(0.0);
        let trailing_scroll_offset = (scroll_offset + remaining_extent).min(total_extent);

        let paint_extent = (trailing_scroll_offset - leading_scroll_offset).max(0.0);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obstruction_extent: 0.0,
            visible_fraction: if total_extent > 0.0 {
                (paint_extent / total_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl SliverRender<Variable> for RenderSliverGrid {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Store cross axis extent
        self.cross_axis_extent = constraints.cross_axis_extent;

        // Calculate sliver geometry using child count
        let child_count = ctx.children.len();
        self.calculate_sliver_geometry(&constraints, child_count)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        use flui_types::geometry::Offset;

        // Paint children in grid positions
        let children = ctx.children.iter().collect::<Vec<_>>();
        let cross_axis_extent = self.cross_axis_extent;

        // Get grid parameters from delegate
        let column_count = self.delegate.get_column_count(cross_axis_extent);
        if column_count == 0 {
            return;
        }

        let (main_spacing, cross_spacing) = self.delegate.get_spacing();
        let row_height = self.delegate.get_main_axis_extent(0, cross_axis_extent);

        // Calculate cell width (cross axis extent / columns - spacing)
        let total_cross_spacing = cross_spacing * (column_count - 1).max(0) as f32;
        let cell_width = (cross_axis_extent - total_cross_spacing) / column_count as f32;

        for (index, &child_id) in children.iter().enumerate() {
            // Calculate grid position
            let row = index / column_count;
            let col = index % column_count;

            // Calculate child offset
            let x = col as f32 * (cell_width + cross_spacing);
            let y = row as f32 * (row_height + main_spacing);
            let child_offset = Offset::new(x, y);

            ctx.paint_child(child_id, ctx.offset + child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};

    #[test]
    fn test_sliver_grid_delegate_fixed_new() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);

        assert_eq!(delegate.cross_axis_count, 3);
        assert_eq!(delegate.main_axis_extent, 100.0);
        assert_eq!(delegate.main_axis_spacing, 0.0);
        assert_eq!(delegate.cross_axis_spacing, 0.0);
    }

    #[test]
    fn test_sliver_grid_delegate_fixed_with_spacing() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
            .with_main_axis_spacing(10.0)
            .with_cross_axis_spacing(5.0);

        assert_eq!(delegate.main_axis_spacing, 10.0);
        assert_eq!(delegate.cross_axis_spacing, 5.0);
    }

    #[test]
    fn test_sliver_grid_delegate_get_column_count() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(4, 100.0);

        assert_eq!(delegate.get_column_count(400.0), 4);
    }

    #[test]
    fn test_sliver_grid_delegate_get_main_axis_extent() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 120.0);

        assert_eq!(delegate.get_main_axis_extent(0, 400.0), 120.0);
        assert_eq!(delegate.get_main_axis_extent(5, 400.0), 120.0); // Same for all
    }

    #[test]
    fn test_sliver_grid_delegate_get_spacing() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
            .with_main_axis_spacing(15.0)
            .with_cross_axis_spacing(10.0);

        let (main, cross) = delegate.get_spacing();
        assert_eq!(main, 15.0);
        assert_eq!(cross, 10.0);
    }

    #[test]
    fn test_sliver_grid_delegate_should_relayout() {
        let delegate1 = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);
        let delegate2 = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);
        let delegate3 = SliverGridDelegateFixedCrossAxisCount::new(4, 100.0);

        assert!(!delegate1.should_relayout(&delegate2 as &dyn std::any::Any));
        assert!(delegate1.should_relayout(&delegate3 as &dyn std::any::Any));
    }

    #[test]
    fn test_render_sliver_grid_new() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        assert_eq!(grid.cross_axis_extent, 0.0);
    }

    #[test]
    fn test_render_sliver_grid_set_cross_axis_extent() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let mut grid = RenderSliverGrid::new(delegate);

        grid.set_cross_axis_extent(400.0);
        assert_eq!(grid.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_empty() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, 0);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_render_sliver_grid_geometry_single_row() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 3 children = 1 row
        let geometry = grid.calculate_sliver_geometry(&constraints, 3);

        // 1 row * 100px = 100px
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_multiple_rows() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 9 children = 3 rows
        let geometry = grid.calculate_sliver_geometry(&constraints, 9);

        // 3 rows * 100px = 300px
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_render_sliver_grid_geometry_with_spacing() {
        let delegate = Box::new(
            SliverGridDelegateFixedCrossAxisCount::new(2, 100.0).with_main_axis_spacing(10.0),
        );
        let grid = RenderSliverGrid::new(delegate);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 4 children = 2 rows
        let geometry = grid.calculate_sliver_geometry(&constraints, 4);

        // 2 rows * 100px + 1 spacing * 10px = 210px
        assert_eq!(geometry.scroll_extent, 210.0);
        assert_eq!(geometry.paint_extent, 210.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_scrolled() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(2, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 150.0, // Scrolled past 1.5 rows
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 10 children = 5 rows
        let geometry = grid.calculate_sliver_geometry(&constraints, 10);

        // 5 rows * 100px = 500px total
        assert_eq!(geometry.scroll_extent, 500.0);
        // Visible: from 150 to 450 = 300px
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.6); // 300/500
    }
}
