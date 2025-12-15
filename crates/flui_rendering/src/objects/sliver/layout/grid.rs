//! RenderSliverGrid - scrollable grid layout.
//!
//! Arranges children in a 2D grid with scrollable main axis.

use flui_types::{Offset, Size};

use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};

use crate::delegates::{SliverGridDelegate, SliverGridLayout};
use crate::parent_data::SliverGridParentData;
use crate::pipeline::PaintingContext;

/// A sliver that places children in a 2D grid layout.
///
/// Uses a [`SliverGridDelegate`] to control the grid layout.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::layout::RenderSliverGrid;
/// use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
///
/// let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(3)
///     .with_main_axis_spacing(8.0)
///     .with_cross_axis_spacing(8.0);
/// let grid = RenderSliverGrid::new(Box::new(delegate));
/// ```
#[derive(Debug)]
pub struct RenderSliverGrid {
    /// The delegate that controls grid layout.
    delegate: Box<dyn SliverGridDelegate>,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Cached layout from last layout.
    cached_layout: Option<SliverGridLayout>,

    /// Laid out children with their sizes and parent data.
    children: Vec<(Size, SliverGridParentData)>,

    /// Total number of items.
    item_count: usize,
}

impl RenderSliverGrid {
    /// Creates a new grid sliver with the given delegate.
    pub fn new(delegate: Box<dyn SliverGridDelegate>) -> Self {
        Self {
            delegate,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            cached_layout: None,
            children: Vec::new(),
            item_count: 0,
        }
    }

    /// Returns a reference to the grid delegate.
    pub fn delegate(&self) -> &dyn SliverGridDelegate {
        self.delegate.as_ref()
    }

    /// Sets a new grid delegate.
    pub fn set_delegate(&mut self, delegate: Box<dyn SliverGridDelegate>) {
        self.delegate = delegate;
        self.cached_layout = None;
        // mark_needs_layout
    }

    /// Returns the current geometry.
    pub fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    /// Returns the current constraints.
    pub fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    /// Returns the visible children.
    pub fn visible_children(&self) -> &[(Size, SliverGridParentData)] {
        &self.children
    }

    /// Returns box constraints for a child at the given index.
    pub fn constraints_for_child(
        &self,
        constraints: &SliverConstraints,
        _index: usize,
    ) -> BoxConstraints {
        let layout = self.delegate.get_layout(*constraints);
        let tile_size = Size::new(
            layout.child_cross_axis_extent,
            layout.child_main_axis_extent,
        );

        BoxConstraints::tight(tile_size)
    }

    /// Performs layout with the given item count.
    pub fn perform_layout(
        &mut self,
        constraints: SliverConstraints,
        item_count: usize,
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.item_count = item_count;
        self.children.clear();

        if item_count == 0 {
            self.geometry = SliverGeometry::zero();
            self.cached_layout = None;
            return self.geometry;
        }

        let layout = self.delegate.get_layout(constraints);
        self.cached_layout = Some(layout);

        let min_index = layout.get_min_child_index_for_scroll_offset(constraints.scroll_offset);
        let max_index = layout.get_max_child_index_for_scroll_offset(
            constraints.scroll_offset + constraints.remaining_paint_extent,
        );

        let first_index = min_index.min(item_count);
        let last_index = (max_index + 1).min(item_count);

        // Layout visible children
        for index in first_index..last_index {
            let scroll_offset = layout.get_scroll_offset_of_child(index);
            let cross_axis_offset =
                layout.get_cross_axis_offset_of_child(index, constraints.cross_axis_extent);

            let tile_size = Size::new(
                layout.child_cross_axis_extent,
                layout.child_main_axis_extent,
            );

            let mut parent_data = SliverGridParentData::default();
            parent_data.index = Some(index);
            parent_data.layout_offset = Some(scroll_offset);
            parent_data.cross_axis_offset = Some(cross_axis_offset);

            self.children.push((tile_size, parent_data));
        }

        let total_extent = self.compute_max_scroll_offset(&layout, item_count);
        let paint_extent = self.calculate_paint_extent(total_extent, &constraints);

        self.geometry = SliverGeometry::new(total_extent, paint_extent, 0.0)
            .with_max_paint_extent(total_extent);

        self.geometry
    }

    fn compute_max_scroll_offset(&self, layout: &SliverGridLayout, item_count: usize) -> f32 {
        if item_count == 0 || layout.cross_axis_count == 0 {
            return 0.0;
        }

        let row_count = (item_count + layout.cross_axis_count - 1) / layout.cross_axis_count;
        row_count as f32 * layout.main_axis_stride
            - (layout.main_axis_stride - layout.child_main_axis_extent)
    }

    fn calculate_paint_extent(&self, total_extent: f32, constraints: &SliverConstraints) -> f32 {
        let visible_extent = total_extent - constraints.scroll_offset;
        visible_extent.clamp(0.0, constraints.remaining_paint_extent)
    }

    /// Returns the paint offset for a child with the given layout offset and cross axis offset.
    pub fn paint_offset_for_child(&self, layout_offset: f32, cross_axis_offset: f32) -> Offset {
        let main_offset = layout_offset - self.constraints.scroll_offset;

        match self.constraints.axis() {
            flui_types::layout::Axis::Vertical => Offset::new(cross_axis_offset, main_offset),
            flui_types::layout::Axis::Horizontal => Offset::new(main_offset, cross_axis_offset),
        }
    }

    /// Paints this sliver.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let _ = (context, offset);
    }

    /// Hit tests this sliver.
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> Option<usize> {
        if cross_axis_position < 0.0 || cross_axis_position >= self.constraints.cross_axis_extent {
            return None;
        }

        let scroll_offset = main_axis_position + self.constraints.scroll_offset;

        for (size, parent_data) in &self.children {
            if let (Some(layout_offset), Some(cross_offset)) =
                (parent_data.layout_offset, parent_data.cross_axis_offset)
            {
                let child_main_extent = match self.constraints.axis() {
                    flui_types::layout::Axis::Vertical => size.height,
                    flui_types::layout::Axis::Horizontal => size.width,
                };
                let child_cross_extent = match self.constraints.axis() {
                    flui_types::layout::Axis::Vertical => size.width,
                    flui_types::layout::Axis::Horizontal => size.height,
                };

                if scroll_offset >= layout_offset
                    && scroll_offset < layout_offset + child_main_extent
                    && cross_axis_position >= cross_offset
                    && cross_axis_position < cross_offset + child_cross_extent
                {
                    return parent_data.index;
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegates::SliverGridDelegateWithFixedCrossAxisCount;

    fn make_constraints(scroll_offset: f32, remaining: f32, cross_axis: f32) -> SliverConstraints {
        SliverConstraints {
            scroll_offset,
            remaining_paint_extent: remaining,
            viewport_main_axis_extent: 600.0,
            cross_axis_extent: cross_axis,
            ..Default::default()
        }
    }

    fn make_delegate() -> Box<dyn SliverGridDelegate> {
        Box::new(
            SliverGridDelegateWithFixedCrossAxisCount::new(3)
                .with_main_axis_spacing(8.0)
                .with_cross_axis_spacing(8.0),
        )
    }

    #[test]
    fn test_grid_new() {
        let grid = RenderSliverGrid::new(make_delegate());
        assert_eq!(grid.geometry().scroll_extent, 0.0);
    }

    #[test]
    fn test_grid_layout() {
        let mut grid = RenderSliverGrid::new(make_delegate());
        // 400px cross axis, 3 columns, 8px spacing
        // Tile width = (400 - 2*8) / 3 = 128px
        // Tile height = 128px (aspect ratio 1.0)
        let constraints = make_constraints(0.0, 400.0, 400.0);

        let geometry = grid.perform_layout(constraints, 9);

        // 9 items in 3 columns = 3 rows
        // Total extent = 3 * 128 + 2 * 8 = 400
        assert!(geometry.scroll_extent > 0.0);
    }

    #[test]
    fn test_grid_visible_children() {
        let mut grid = RenderSliverGrid::new(make_delegate());
        let constraints = make_constraints(0.0, 200.0, 400.0);

        grid.perform_layout(constraints, 12);

        // Should have some visible children
        assert!(!grid.visible_children().is_empty());
    }

    #[test]
    fn test_grid_empty() {
        let mut grid = RenderSliverGrid::new(make_delegate());
        let constraints = make_constraints(0.0, 400.0, 400.0);

        let geometry = grid.perform_layout(constraints, 0);

        assert!(geometry.is_empty());
    }

    #[test]
    fn test_grid_constraints_for_child() {
        let grid = RenderSliverGrid::new(make_delegate());
        let constraints = make_constraints(0.0, 400.0, 400.0);

        let child_constraints = grid.constraints_for_child(&constraints, 0);

        // Should be tight constraints for the tile size
        assert_eq!(child_constraints.min_width, child_constraints.max_width);
        assert_eq!(child_constraints.min_height, child_constraints.max_height);
    }

    #[test]
    fn test_grid_paint_offset() {
        let mut grid = RenderSliverGrid::new(make_delegate());
        let constraints = make_constraints(50.0, 400.0, 400.0);

        grid.perform_layout(constraints, 9);

        // Layout offset 100, scroll 50 = 50 paint offset
        let offset = grid.paint_offset_for_child(100.0, 128.0);
        assert_eq!(offset.dy, 50.0);
        assert_eq!(offset.dx, 128.0);
    }
}
