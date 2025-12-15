//! RenderSliverList - scrollable list with variable height items.
//!
//! Unlike fixed extent lists, this handles items with varying heights.

use flui_types::{Offset, Size};

use crate::constraints::{SliverConstraints, SliverGeometry};

use crate::parent_data::SliverMultiBoxAdaptorParentData;
use crate::pipeline::PaintingContext;

/// A sliver that places children in a linear list with variable extent.
///
/// Children can have different sizes in the main axis. This is less
/// efficient than fixed extent lists because layout must be performed
/// to determine item positions.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::layout::RenderSliverList;
///
/// let list = RenderSliverList::new();
/// ```
#[derive(Debug, Default)]
pub struct RenderSliverList {
    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Laid out children with their sizes and parent data.
    children: Vec<(Size, SliverMultiBoxAdaptorParentData)>,

    /// Total scroll extent (sum of all children extents).
    total_extent: f32,
}

impl RenderSliverList {
    /// Creates a new variable extent list.
    pub fn new() -> Self {
        Self::default()
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
    pub fn visible_children(&self) -> &[(Size, SliverMultiBoxAdaptorParentData)] {
        &self.children
    }

    /// Performs layout with the given child sizes.
    ///
    /// `child_sizes` should contain the size of each child in layout order.
    pub fn perform_layout(
        &mut self,
        constraints: SliverConstraints,
        child_sizes: &[Size],
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.children.clear();

        if child_sizes.is_empty() {
            self.total_extent = 0.0;
            self.geometry = SliverGeometry::zero();
            return self.geometry;
        }

        // Calculate total extent and find visible range
        let mut layout_offset = 0.0;
        let mut first_visible_index = None;
        let scroll_end = constraints.scroll_offset + constraints.remaining_paint_extent;

        for (index, size) in child_sizes.iter().enumerate() {
            let child_extent = self.get_child_extent(*size, &constraints);
            let child_end = layout_offset + child_extent;

            // Check if child is visible
            let is_visible = child_end > constraints.scroll_offset && layout_offset < scroll_end;

            if is_visible {
                if first_visible_index.is_none() {
                    first_visible_index = Some(index);
                }

                let mut parent_data = SliverMultiBoxAdaptorParentData::new(index);
                parent_data.layout_offset = Some(layout_offset);
                self.children.push((*size, parent_data));
            } else if first_visible_index.is_some() {
                // We've passed the visible region
                break;
            }

            layout_offset = child_end;
        }

        // Continue to calculate total extent for remaining children
        for size in child_sizes
            .iter()
            .skip(self.children.len() + first_visible_index.unwrap_or(0))
        {
            layout_offset += self.get_child_extent(*size, &constraints);
        }

        self.total_extent = layout_offset;

        let paint_extent = self.calculate_paint_extent(self.total_extent, &constraints);

        self.geometry = SliverGeometry::new(self.total_extent, paint_extent, 0.0)
            .with_max_paint_extent(self.total_extent);

        self.geometry
    }

    fn get_child_extent(&self, size: Size, constraints: &SliverConstraints) -> f32 {
        match constraints.axis() {
            flui_types::layout::Axis::Vertical => size.height,
            flui_types::layout::Axis::Horizontal => size.width,
        }
    }

    fn calculate_paint_extent(&self, total_extent: f32, constraints: &SliverConstraints) -> f32 {
        let visible_extent = total_extent - constraints.scroll_offset;
        visible_extent.clamp(0.0, constraints.remaining_paint_extent)
    }

    /// Returns the paint offset for a child with the given layout offset.
    pub fn paint_offset_for_child(&self, layout_offset: f32) -> Offset {
        let paint_offset = layout_offset - self.constraints.scroll_offset;

        match self.constraints.axis() {
            flui_types::layout::Axis::Vertical => Offset::new(0.0, paint_offset),
            flui_types::layout::Axis::Horizontal => Offset::new(paint_offset, 0.0),
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

        let target_offset = main_axis_position + self.constraints.scroll_offset;

        for (size, parent_data) in &self.children {
            if let Some(layout_offset) = parent_data.layout_offset {
                let child_extent = self.get_child_extent(*size, &self.constraints);
                if target_offset >= layout_offset && target_offset < layout_offset + child_extent {
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

    fn make_constraints(scroll_offset: f32, remaining: f32) -> SliverConstraints {
        SliverConstraints {
            scroll_offset,
            remaining_paint_extent: remaining,
            viewport_main_axis_extent: 600.0,
            cross_axis_extent: 400.0,
            ..Default::default()
        }
    }

    #[test]
    fn test_list_new() {
        let list = RenderSliverList::new();
        assert_eq!(list.geometry().scroll_extent, 0.0);
    }

    #[test]
    fn test_list_layout_uniform() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(0.0, 300.0);
        let child_sizes: Vec<Size> = (0..10).map(|_| Size::new(400.0, 50.0)).collect();

        let geometry = list.perform_layout(constraints, &child_sizes);

        // 10 items * 50px = 500px
        assert_eq!(geometry.scroll_extent, 500.0);
        assert_eq!(geometry.paint_extent, 300.0);
    }

    #[test]
    fn test_list_layout_variable() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(0.0, 300.0);
        let child_sizes = vec![
            Size::new(400.0, 100.0),
            Size::new(400.0, 50.0),
            Size::new(400.0, 75.0),
            Size::new(400.0, 200.0),
        ];

        let geometry = list.perform_layout(constraints, &child_sizes);

        // 100 + 50 + 75 + 200 = 425px
        assert_eq!(geometry.scroll_extent, 425.0);
    }

    #[test]
    fn test_list_visible_children() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(100.0, 150.0);
        let child_sizes: Vec<Size> = (0..10).map(|_| Size::new(400.0, 50.0)).collect();

        list.perform_layout(constraints, &child_sizes);

        // Scrolled 100px (2 items), viewport 150px (3 items)
        // Items at layout offsets 100, 150, 200 are visible (scroll_end = 250)
        // Item at 250 starts exactly at scroll_end, so not visible
        let visible = list.visible_children();
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn test_list_paint_offset() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(75.0, 300.0);
        let child_sizes: Vec<Size> = (0..10).map(|_| Size::new(400.0, 50.0)).collect();

        list.perform_layout(constraints, &child_sizes);

        // Child at layout offset 100, scroll offset 75 = 25 paint offset
        let offset = list.paint_offset_for_child(100.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_list_empty() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(0.0, 300.0);

        let geometry = list.perform_layout(constraints, &[]);

        assert!(geometry.is_empty());
    }

    #[test]
    fn test_list_hit_test() {
        let mut list = RenderSliverList::new();
        let constraints = make_constraints(0.0, 300.0);
        let child_sizes: Vec<Size> = (0..10).map(|_| Size::new(400.0, 50.0)).collect();

        list.perform_layout(constraints, &child_sizes);

        // Position 75 is in item 1 (50-100)
        assert_eq!(list.hit_test(75.0, 200.0), Some(1));

        // Outside cross axis
        assert_eq!(list.hit_test(75.0, 500.0), None);
    }
}
