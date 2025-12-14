//! RenderSliverFillViewport - children fill entire viewport.
//!
//! Each child is sized to fill the entire viewport in the main axis.

use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::parent_data::SliverMultiBoxAdaptorParentData;
use crate::pipeline::PaintingContext;

/// A sliver where each child fills the viewport.
///
/// Used for page-style scrolling where each page takes up the full viewport.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::basic::RenderSliverFillViewport;
///
/// let fill = RenderSliverFillViewport::new(1.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverFillViewport {
    /// The fraction of the viewport each child should fill.
    viewport_fraction: f32,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child count and their parent data.
    children: Vec<(Size, SliverMultiBoxAdaptorParentData)>,
}

impl RenderSliverFillViewport {
    /// Creates a new fill viewport sliver.
    ///
    /// `viewport_fraction` is the fraction of the viewport each child fills.
    /// Typically 1.0 for full-page scrolling.
    pub fn new(viewport_fraction: f32) -> Self {
        debug_assert!(
            viewport_fraction > 0.0,
            "viewport_fraction must be positive"
        );
        Self {
            viewport_fraction,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            children: Vec::new(),
        }
    }

    /// Returns the viewport fraction.
    pub fn viewport_fraction(&self) -> f32 {
        self.viewport_fraction
    }

    /// Sets the viewport fraction.
    pub fn set_viewport_fraction(&mut self, fraction: f32) {
        debug_assert!(fraction > 0.0, "viewport_fraction must be positive");
        if (self.viewport_fraction - fraction).abs() > f32::EPSILON {
            self.viewport_fraction = fraction;
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

    /// Returns the extent each child should have.
    pub fn item_extent(&self, constraints: &SliverConstraints) -> f32 {
        constraints.viewport_main_axis_extent * self.viewport_fraction
    }

    /// Returns box constraints for a child.
    pub fn constraints_for_child(&self, constraints: &SliverConstraints) -> BoxConstraints {
        let item_extent = self.item_extent(constraints);

        match constraints.axis {
            flui_types::layout::Axis::Vertical => {
                BoxConstraints::tight(Size::new(constraints.cross_axis_extent, item_extent))
            }
            flui_types::layout::Axis::Horizontal => {
                BoxConstraints::tight(Size::new(item_extent, constraints.cross_axis_extent))
            }
        }
    }

    /// Performs layout with the given child count.
    pub fn perform_layout(
        &mut self,
        constraints: SliverConstraints,
        child_count: usize,
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.children.clear();

        if child_count == 0 {
            self.geometry = SliverGeometry::zero();
            return self.geometry;
        }

        let item_extent = self.item_extent(&constraints);
        let total_extent = item_extent * child_count as f32;

        // Calculate visible range
        let first_index = (constraints.scroll_offset / item_extent).floor() as usize;
        let target_end_index = ((constraints.scroll_offset + constraints.remaining_paint_extent)
            / item_extent)
            .ceil() as usize;
        let last_index = target_end_index.min(child_count);

        // Layout visible children
        for index in first_index..last_index {
            let layout_offset = index as f32 * item_extent;
            let mut parent_data = SliverMultiBoxAdaptorParentData::new(index);
            parent_data.layout_offset = Some(layout_offset);

            // Child size is fixed
            let child_size = match constraints.axis {
                flui_types::layout::Axis::Vertical => {
                    Size::new(constraints.cross_axis_extent, item_extent)
                }
                flui_types::layout::Axis::Horizontal => {
                    Size::new(item_extent, constraints.cross_axis_extent)
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
        let item_extent = self.item_extent(&self.constraints);
        let layout_offset = index as f32 * item_extent;
        let paint_offset = layout_offset - self.constraints.scroll_offset;

        match self.constraints.axis {
            flui_types::layout::Axis::Vertical => Offset::new(0.0, paint_offset),
            flui_types::layout::Axis::Horizontal => Offset::new(paint_offset, 0.0),
        }
    }

    /// Returns the index of the child at the given scroll offset.
    pub fn index_at_offset(&self, main_axis_offset: f32) -> Option<usize> {
        let item_extent = self.item_extent(&self.constraints);
        if item_extent <= 0.0 {
            return None;
        }

        let scroll_offset = main_axis_offset + self.constraints.scroll_offset;
        let index = (scroll_offset / item_extent).floor() as usize;

        if index < self.children.len() {
            Some(index)
        } else {
            None
        }
    }

    /// Paints this sliver.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let _ = (context, offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::{Axis, AxisDirection};

    fn make_constraints(scroll_offset: f32, remaining: f32, viewport: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            scroll_offset,
            remaining,
            viewport,
            400.0,
        )
    }

    #[test]
    fn test_fill_viewport_new() {
        let fill = RenderSliverFillViewport::new(1.0);
        assert_eq!(fill.viewport_fraction(), 1.0);
    }

    #[test]
    fn test_fill_viewport_item_extent() {
        let fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(0.0, 600.0, 600.0);

        assert_eq!(fill.item_extent(&constraints), 600.0);
    }

    #[test]
    fn test_fill_viewport_half_fraction() {
        let fill = RenderSliverFillViewport::new(0.5);
        let constraints = make_constraints(0.0, 600.0, 600.0);

        assert_eq!(fill.item_extent(&constraints), 300.0);
    }

    #[test]
    fn test_fill_viewport_layout() {
        let mut fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(0.0, 600.0, 600.0);

        let geometry = fill.perform_layout(constraints, 3);

        // 3 children * 600px each = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        assert_eq!(geometry.paint_extent, 600.0); // Viewport size
    }

    #[test]
    fn test_fill_viewport_scrolled() {
        let mut fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(600.0, 600.0, 600.0);

        let geometry = fill.perform_layout(constraints, 3);

        // Scrolled past first child
        assert_eq!(geometry.scroll_extent, 1800.0);
        assert_eq!(geometry.paint_extent, 600.0);
    }

    #[test]
    fn test_fill_viewport_constraints_for_child() {
        let fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(0.0, 600.0, 600.0);

        let child_constraints = fill.constraints_for_child(&constraints);

        assert_eq!(child_constraints.min_width, 400.0);
        assert_eq!(child_constraints.max_width, 400.0);
        assert_eq!(child_constraints.min_height, 600.0);
        assert_eq!(child_constraints.max_height, 600.0);
    }

    #[test]
    fn test_fill_viewport_paint_offset() {
        let mut fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(300.0, 600.0, 600.0);

        fill.perform_layout(constraints, 3);

        let offset = fill.paint_offset_for_child(1);
        // Child 1 at 600px, scrolled 300px = 300px paint offset
        assert_eq!(offset.dy, 300.0);
    }

    #[test]
    fn test_fill_viewport_empty() {
        let mut fill = RenderSliverFillViewport::new(1.0);
        let constraints = make_constraints(0.0, 600.0, 600.0);

        let geometry = fill.perform_layout(constraints, 0);

        assert!(geometry.is_empty());
    }
}
