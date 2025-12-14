//! RenderSliverToBoxAdapter - wraps a box widget in a sliver.
//!
//! Allows a single RenderBox child to be placed inside a scrollable area.

use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that contains a single box widget.
///
/// This is useful for including non-scrolling widgets (like headers or
/// buttons) in a scrollable list.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::basic::RenderSliverToBoxAdapter;
///
/// let adapter = RenderSliverToBoxAdapter::new();
/// ```
#[derive(Debug, Default)]
pub struct RenderSliverToBoxAdapter {
    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child size from last layout.
    child_size: Size,
}

impl RenderSliverToBoxAdapter {
    /// Creates a new sliver to box adapter.
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

    /// Returns box constraints for the child based on sliver constraints.
    pub fn constraints_for_child(&self, constraints: &SliverConstraints) -> BoxConstraints {
        // Child gets infinite extent in main axis, constrained in cross axis
        match constraints.axis {
            flui_types::layout::Axis::Vertical => {
                BoxConstraints::new(0.0, constraints.cross_axis_extent, 0.0, f32::INFINITY)
            }
            flui_types::layout::Axis::Horizontal => {
                BoxConstraints::new(0.0, f32::INFINITY, 0.0, constraints.cross_axis_extent)
            }
        }
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.constraints = constraints;
        self.child_size = Size::ZERO;
        self.geometry = SliverGeometry::zero();
        self.geometry
    }

    /// Performs layout with the child's size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: SliverConstraints,
        child_size: Size,
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.child_size = child_size;

        let child_extent = self.get_child_extent(child_size, &constraints);
        let paint_extent = self.calculate_paint_extent(child_extent, &constraints);
        let cache_extent = self.calculate_cache_extent(child_extent, &constraints);

        self.geometry = SliverGeometry::new(child_extent, paint_extent, 0.0)
            .with_max_paint_extent(child_extent)
            .with_cache_extent(cache_extent);

        self.geometry
    }

    /// Gets the main axis extent of the child.
    fn get_child_extent(&self, child_size: Size, constraints: &SliverConstraints) -> f32 {
        match constraints.axis {
            flui_types::layout::Axis::Vertical => child_size.height,
            flui_types::layout::Axis::Horizontal => child_size.width,
        }
    }

    /// Calculates paint extent based on scroll position.
    fn calculate_paint_extent(&self, child_extent: f32, constraints: &SliverConstraints) -> f32 {
        let visible_extent = child_extent - constraints.scroll_offset;
        visible_extent.clamp(0.0, constraints.remaining_paint_extent)
    }

    /// Calculates cache extent for keeping content rendered.
    fn calculate_cache_extent(&self, child_extent: f32, constraints: &SliverConstraints) -> f32 {
        let visible_extent = child_extent - constraints.scroll_offset;
        visible_extent.max(0.0)
    }

    /// Returns the offset for painting the child.
    pub fn child_paint_offset(&self) -> Offset {
        let scroll_offset = self
            .constraints
            .scroll_offset
            .min(self.get_child_extent(self.child_size, &self.constraints));

        match self.constraints.axis {
            flui_types::layout::Axis::Vertical => Offset::new(0.0, -scroll_offset),
            flui_types::layout::Axis::Horizontal => Offset::new(-scroll_offset, 0.0),
        }
    }

    /// Paints this sliver.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let _ = (context, offset);
        // In real implementation: paint child at offset + child_paint_offset
    }

    /// Hit tests this sliver at the given position.
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        let child_extent = self.get_child_extent(self.child_size, &self.constraints);
        let scroll_offset = self.constraints.scroll_offset;

        // Position in child coordinates
        let child_main_position = main_axis_position + scroll_offset;

        child_main_position >= 0.0
            && child_main_position < child_extent
            && cross_axis_position >= 0.0
            && cross_axis_position < self.constraints.cross_axis_extent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::{Axis, AxisDirection};

    fn make_constraints(scroll_offset: f32, remaining: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            scroll_offset,
            remaining,
            600.0,
            400.0,
        )
    }

    #[test]
    fn test_adapter_new() {
        let adapter = RenderSliverToBoxAdapter::new();
        assert_eq!(adapter.geometry().scroll_extent, 0.0);
    }

    #[test]
    fn test_adapter_layout_no_child() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(0.0, 400.0);

        let geometry = adapter.perform_layout(constraints);

        assert!(geometry.is_empty());
    }

    #[test]
    fn test_adapter_layout_with_child() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(0.0, 400.0);
        let child_size = Size::new(400.0, 100.0);

        let geometry = adapter.perform_layout_with_child(constraints, child_size);

        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_adapter_partially_scrolled() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(30.0, 400.0);
        let child_size = Size::new(400.0, 100.0);

        let geometry = adapter.perform_layout_with_child(constraints, child_size);

        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 70.0); // 100 - 30
    }

    #[test]
    fn test_adapter_fully_scrolled() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(150.0, 400.0);
        let child_size = Size::new(400.0, 100.0);

        let geometry = adapter.perform_layout_with_child(constraints, child_size);

        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 0.0); // Scrolled out of view
    }

    #[test]
    fn test_adapter_child_paint_offset() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(30.0, 400.0);
        let child_size = Size::new(400.0, 100.0);

        adapter.perform_layout_with_child(constraints, child_size);

        let offset = adapter.child_paint_offset();
        assert_eq!(offset.dy, -30.0); // Scrolled up by 30
    }

    #[test]
    fn test_adapter_constraints_for_child() {
        let adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(0.0, 400.0);

        let child_constraints = adapter.constraints_for_child(&constraints);

        assert_eq!(child_constraints.max_width, 400.0);
        assert_eq!(child_constraints.max_height, f32::INFINITY);
    }

    #[test]
    fn test_adapter_hit_test() {
        let mut adapter = RenderSliverToBoxAdapter::new();
        let constraints = make_constraints(0.0, 400.0);
        let child_size = Size::new(400.0, 100.0);

        adapter.perform_layout_with_child(constraints, child_size);

        assert!(adapter.hit_test(50.0, 200.0));
        assert!(!adapter.hit_test(150.0, 200.0)); // Beyond child extent
    }
}
