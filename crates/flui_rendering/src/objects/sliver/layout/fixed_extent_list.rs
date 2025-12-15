//! RenderSliverFixedExtentList - scrollable list with fixed item extent.
//!
//! More efficient than variable extent lists because item positions
//! can be calculated mathematically without laying out children.

use flui_types::{Offset, Size};

use crate::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};

use crate::parent_data::SliverMultiBoxAdaptorParentData;
use crate::pipeline::PaintingContext;

/// A sliver that places children in a linear list with fixed extent.
///
/// All children have the same extent in the main axis, allowing for
/// efficient scrolling and layout calculation.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::layout::RenderSliverFixedExtentList;
///
/// let list = RenderSliverFixedExtentList::new(50.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverFixedExtentList {
    /// The extent of each item in the main axis.
    item_extent: f32,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Visible children with their parent data.
    children: Vec<(Size, SliverMultiBoxAdaptorParentData)>,

    /// Total number of items.
    item_count: usize,
}

impl RenderSliverFixedExtentList {
    /// Creates a new fixed extent list with the given item extent.
    pub fn new(item_extent: f32) -> Self {
        debug_assert!(item_extent > 0.0, "item_extent must be positive");
        Self {
            item_extent,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            children: Vec::new(),
            item_count: 0,
        }
    }

    /// Returns the item extent.
    pub fn item_extent(&self) -> f32 {
        self.item_extent
    }

    /// Sets the item extent.
    pub fn set_item_extent(&mut self, extent: f32) {
        debug_assert!(extent > 0.0, "item_extent must be positive");
        if (self.item_extent - extent).abs() > f32::EPSILON {
            self.item_extent = extent;
            // mark_needs_layout
        }
    }

    /// Returns the current geometry.
    pub fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    /// Returns the current constraints.
    pub fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    /// Returns the index of the first visible child.
    pub fn first_visible_index(&self, constraints: &SliverConstraints) -> usize {
        (constraints.scroll_offset / self.item_extent).floor() as usize
    }

    /// Returns the index of the last visible child.
    pub fn last_visible_index(&self, constraints: &SliverConstraints, item_count: usize) -> usize {
        let target = constraints.scroll_offset + constraints.remaining_paint_extent;
        let index = (target / self.item_extent).ceil() as usize;
        index.min(item_count)
    }

    /// Returns box constraints for a child.
    pub fn constraints_for_child(&self, constraints: &SliverConstraints) -> BoxConstraints {
        match constraints.axis() {
            flui_types::layout::Axis::Vertical => {
                BoxConstraints::tight(Size::new(constraints.cross_axis_extent, self.item_extent))
            }
            flui_types::layout::Axis::Horizontal => {
                BoxConstraints::tight(Size::new(self.item_extent, constraints.cross_axis_extent))
            }
        }
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
            return self.geometry;
        }

        let total_extent = self.item_extent * item_count as f32;
        let first_index = self.first_visible_index(&constraints);
        let last_index = self.last_visible_index(&constraints, item_count);

        // Layout visible children
        for index in first_index..last_index {
            let layout_offset = index as f32 * self.item_extent;
            let mut parent_data = SliverMultiBoxAdaptorParentData::new(index);
            parent_data.layout_offset = Some(layout_offset);

            let child_size = match constraints.axis() {
                flui_types::layout::Axis::Vertical => {
                    Size::new(constraints.cross_axis_extent, self.item_extent)
                }
                flui_types::layout::Axis::Horizontal => {
                    Size::new(self.item_extent, constraints.cross_axis_extent)
                }
            };

            self.children.push((child_size, parent_data));
        }

        let paint_extent = self.calculate_paint_extent(total_extent, &constraints);

        self.geometry = SliverGeometry::new(total_extent, paint_extent, 0.0)
            .with_max_paint_extent(total_extent);

        self.geometry
    }

    fn calculate_paint_extent(&self, total_extent: f32, constraints: &SliverConstraints) -> f32 {
        let visible_extent = total_extent - constraints.scroll_offset;
        visible_extent.clamp(0.0, constraints.remaining_paint_extent)
    }

    /// Returns the paint offset for a child at the given index.
    pub fn paint_offset_for_child(&self, index: usize) -> Offset {
        let layout_offset = index as f32 * self.item_extent;
        let paint_offset = layout_offset - self.constraints.scroll_offset;

        match self.constraints.axis() {
            flui_types::layout::Axis::Vertical => Offset::new(0.0, paint_offset),
            flui_types::layout::Axis::Horizontal => Offset::new(paint_offset, 0.0),
        }
    }

    /// Returns the index of the item at the given scroll offset.
    pub fn index_at_offset(&self, main_axis_offset: f32) -> Option<usize> {
        let scroll_offset = main_axis_offset + self.constraints.scroll_offset;
        if scroll_offset < 0.0 {
            return None;
        }

        let index = (scroll_offset / self.item_extent).floor() as usize;
        if index < self.item_count {
            Some(index)
        } else {
            None
        }
    }

    /// Returns the visible children.
    pub fn visible_children(&self) -> &[(Size, SliverMultiBoxAdaptorParentData)] {
        &self.children
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

        self.index_at_offset(main_axis_position)
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
    fn test_fixed_extent_list_new() {
        let list = RenderSliverFixedExtentList::new(50.0);
        assert_eq!(list.item_extent(), 50.0);
    }

    #[test]
    fn test_fixed_extent_list_layout() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(0.0, 300.0);

        let geometry = list.perform_layout(constraints, 20);

        // 20 items * 50px = 1000px total
        assert_eq!(geometry.scroll_extent, 1000.0);
        assert_eq!(geometry.paint_extent, 300.0);
    }

    #[test]
    fn test_fixed_extent_list_visible_range() {
        let list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(100.0, 200.0);

        let first = list.first_visible_index(&constraints);
        let last = list.last_visible_index(&constraints, 20);

        // Scrolled 100px (2 items), viewport 200px (4 items)
        assert_eq!(first, 2);
        assert_eq!(last, 6);
    }

    #[test]
    fn test_fixed_extent_list_constraints_for_child() {
        let list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(0.0, 300.0);

        let child_constraints = list.constraints_for_child(&constraints);

        assert_eq!(child_constraints.min_width, 400.0);
        assert_eq!(child_constraints.max_width, 400.0);
        assert_eq!(child_constraints.min_height, 50.0);
        assert_eq!(child_constraints.max_height, 50.0);
    }

    #[test]
    fn test_fixed_extent_list_paint_offset() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(75.0, 300.0);

        list.perform_layout(constraints, 20);

        // Item 2 at 100px, scrolled 75px = 25px paint offset
        let offset = list.paint_offset_for_child(2);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_fixed_extent_list_index_at_offset() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(100.0, 300.0);

        list.perform_layout(constraints, 20);

        // Position 25 + scroll 100 = 125px, that's item 2 (floor(125/50))
        assert_eq!(list.index_at_offset(25.0), Some(2));
    }

    #[test]
    fn test_fixed_extent_list_empty() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(0.0, 300.0);

        let geometry = list.perform_layout(constraints, 0);

        assert!(geometry.is_empty());
    }

    #[test]
    fn test_fixed_extent_list_hit_test() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        let constraints = make_constraints(0.0, 300.0);

        list.perform_layout(constraints, 20);

        assert_eq!(list.hit_test(75.0, 200.0), Some(1));
        assert_eq!(list.hit_test(75.0, 500.0), None); // Outside cross axis
    }
}
