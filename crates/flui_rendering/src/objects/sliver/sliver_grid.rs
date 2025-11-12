//! RenderSliverGrid - Scrollable grid with lazy loading

use flui_core::element::ElementTree;
use flui_core::render::{Arity, SliverLayoutContext, SliverPaintContext, RenderSliver};
use flui_painting::Canvas;
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

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl std::fmt::Debug for RenderSliverGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverGrid")
            .field("delegate", &self.delegate)
            .field("cross_axis_extent", &self.cross_axis_extent)
            .field("sliver_geometry", &self.sliver_geometry)
            .finish()
    }
}

impl RenderSliverGrid {
    /// Create new sliver grid with delegate
    pub fn new(delegate: Box<dyn SliverGridDelegate>) -> Self {
        Self {
            delegate,
            cross_axis_extent: 0.0,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set cross axis extent
    pub fn set_cross_axis_extent(&mut self, extent: f32) {
        self.cross_axis_extent = extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for grid layout
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) -> SliverGeometry {
        // If no children, return zero geometry
        if children.is_empty() {
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
        let child_count = children.len();
        let row_count = (child_count + column_count - 1) / column_count; // Ceiling division

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
            max_scroll_obsolescence: 0.0,
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

impl RenderSliver for RenderSliverGrid {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Store cross axis extent
        self.cross_axis_extent = constraints.cross_axis_extent;

        // Calculate and cache sliver geometry
        let children_slice = ctx.children.as_slice();
        self.sliver_geometry = self.calculate_sliver_geometry(constraints, ctx.tree, children_slice);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        let canvas = Canvas::new();

        // Grid painting happens in viewport
        // Children are painted in grid positions based on scroll offset

        // TODO: Implement actual child painting with grid layout
        // This would calculate grid positions and paint visible children

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Multiple children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let tree = ElementTree::new();
        let children = vec![];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_render_sliver_grid_geometry_single_row() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 3 children = 1 row
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

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
        let tree = ElementTree::new();

        // 9 children = 3 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
            flui_core::element::ElementId::new(5),
            flui_core::element::ElementId::new(6),
            flui_core::element::ElementId::new(7),
            flui_core::element::ElementId::new(8),
            flui_core::element::ElementId::new(9),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

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
        let tree = ElementTree::new();

        // 4 children = 2 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 rows * 100px + 1 spacing * 10px = 210px
        assert_eq!(geometry.scroll_extent, 210.0);
        assert_eq!(geometry.paint_extent, 210.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_scrolled() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(2, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 10 children = 5 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
            flui_core::element::ElementId::new(5),
            flui_core::element::ElementId::new(6),
            flui_core::element::ElementId::new(7),
            flui_core::element::ElementId::new(8),
            flui_core::element::ElementId::new(9),
            flui_core::element::ElementId::new(10),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 150.0, // Scrolled past 1.5 rows
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 5 rows * 100px = 500px total
        assert_eq!(geometry.scroll_extent, 500.0);
        // Visible: from 150 to 450 = 300px
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.6); // 300/500
    }

    #[test]
    fn test_arity_is_variable() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        assert_eq!(grid.arity(), Arity::Variable);
    }
}
