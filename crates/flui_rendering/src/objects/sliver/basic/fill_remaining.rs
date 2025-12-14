//! RenderSliverFillRemaining - fills remaining viewport space.
//!
//! A sliver that fills all remaining space in the viewport.

use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that fills the remaining space in the viewport.
///
/// This is useful for placing content at the bottom of a scrollable
/// area, or for creating a scrollable area that always fills the screen.
///
/// # Modes
///
/// - `has_scroll_body: false` - Child is sized to remaining space, no scrolling
/// - `has_scroll_body: true` - Child can be larger, remaining space is minimum
/// - `fill_overscroll: true` - Also fills overscroll area (iOS bounce)
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::basic::RenderSliverFillRemaining;
///
/// let fill = RenderSliverFillRemaining::new(false, false);
/// ```
#[derive(Debug)]
pub struct RenderSliverFillRemaining {
    /// Whether the child has its own scroll body.
    has_scroll_body: bool,

    /// Whether to fill overscroll area.
    fill_overscroll: bool,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child size from last layout.
    child_size: Size,
}

impl RenderSliverFillRemaining {
    /// Creates a new fill remaining sliver.
    pub fn new(has_scroll_body: bool, fill_overscroll: bool) -> Self {
        Self {
            has_scroll_body,
            fill_overscroll,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            child_size: Size::ZERO,
        }
    }

    /// Returns whether the child has its own scroll body.
    pub fn has_scroll_body(&self) -> bool {
        self.has_scroll_body
    }

    /// Sets whether the child has its own scroll body.
    pub fn set_has_scroll_body(&mut self, value: bool) {
        if self.has_scroll_body != value {
            self.has_scroll_body = value;
            // mark_needs_layout
        }
    }

    /// Returns whether to fill overscroll.
    pub fn fill_overscroll(&self) -> bool {
        self.fill_overscroll
    }

    /// Sets whether to fill overscroll.
    pub fn set_fill_overscroll(&mut self, value: bool) {
        if self.fill_overscroll != value {
            self.fill_overscroll = value;
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

    /// Returns box constraints for the child.
    pub fn constraints_for_child(&self, constraints: &SliverConstraints) -> BoxConstraints {
        let extent = constraints.remaining_paint_extent;

        match constraints.axis {
            flui_types::layout::Axis::Vertical => {
                if self.has_scroll_body {
                    BoxConstraints::new(0.0, constraints.cross_axis_extent, extent, f32::INFINITY)
                } else {
                    BoxConstraints::tight(Size::new(constraints.cross_axis_extent, extent))
                }
            }
            flui_types::layout::Axis::Horizontal => {
                if self.has_scroll_body {
                    BoxConstraints::new(extent, f32::INFINITY, 0.0, constraints.cross_axis_extent)
                } else {
                    BoxConstraints::tight(Size::new(extent, constraints.cross_axis_extent))
                }
            }
        }
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.constraints = constraints;
        self.child_size = Size::ZERO;

        let extent = constraints.remaining_paint_extent;

        self.geometry = SliverGeometry::new(extent, extent, 0.0).with_max_paint_extent(extent);

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

        let child_extent = match constraints.axis {
            flui_types::layout::Axis::Vertical => child_size.height,
            flui_types::layout::Axis::Horizontal => child_size.width,
        };

        let extent = if self.has_scroll_body {
            child_extent.max(constraints.remaining_paint_extent)
        } else {
            constraints.remaining_paint_extent
        };

        let paint_extent = if self.fill_overscroll && constraints.scroll_offset < 0.0 {
            // During overscroll (iOS bounce), extend to fill
            (extent - constraints.scroll_offset).min(constraints.remaining_paint_extent)
        } else {
            extent.min(constraints.remaining_paint_extent)
        };

        self.geometry = SliverGeometry::new(extent, paint_extent, 0.0)
            .with_max_paint_extent(extent)
            .with_layout_extent(extent);

        self.geometry
    }

    /// Returns the offset for painting the child.
    pub fn child_paint_offset(&self) -> Offset {
        if self.fill_overscroll && self.constraints.scroll_offset < 0.0 {
            // During overscroll, shift child down
            match self.constraints.axis {
                flui_types::layout::Axis::Vertical => {
                    Offset::new(0.0, -self.constraints.scroll_offset)
                }
                flui_types::layout::Axis::Horizontal => {
                    Offset::new(-self.constraints.scroll_offset, 0.0)
                }
            }
        } else {
            Offset::ZERO
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
    fn test_fill_remaining_new() {
        let fill = RenderSliverFillRemaining::new(false, false);
        assert!(!fill.has_scroll_body());
        assert!(!fill.fill_overscroll());
    }

    #[test]
    fn test_fill_remaining_no_child() {
        let mut fill = RenderSliverFillRemaining::new(false, false);
        let constraints = make_constraints(0.0, 200.0);

        let geometry = fill.perform_layout(constraints);

        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
    }

    #[test]
    fn test_fill_remaining_with_child() {
        let mut fill = RenderSliverFillRemaining::new(false, false);
        let constraints = make_constraints(0.0, 200.0);
        let child_size = Size::new(400.0, 100.0);

        let geometry = fill.perform_layout_with_child(constraints, child_size);

        // Without scroll body, uses remaining paint extent
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
    }

    #[test]
    fn test_fill_remaining_with_scroll_body() {
        let mut fill = RenderSliverFillRemaining::new(true, false);
        let constraints = make_constraints(0.0, 200.0);
        let child_size = Size::new(400.0, 300.0);

        let geometry = fill.perform_layout_with_child(constraints, child_size);

        // With scroll body, child can be larger
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 200.0); // Clamped to remaining
    }

    #[test]
    fn test_fill_remaining_constraints_for_child() {
        let fill = RenderSliverFillRemaining::new(false, false);
        let constraints = make_constraints(0.0, 200.0);

        let child_constraints = fill.constraints_for_child(&constraints);

        // Tight constraints without scroll body
        assert_eq!(child_constraints.min_width, 400.0);
        assert_eq!(child_constraints.max_width, 400.0);
        assert_eq!(child_constraints.min_height, 200.0);
        assert_eq!(child_constraints.max_height, 200.0);
    }

    #[test]
    fn test_fill_remaining_constraints_with_scroll_body() {
        let fill = RenderSliverFillRemaining::new(true, false);
        let constraints = make_constraints(0.0, 200.0);

        let child_constraints = fill.constraints_for_child(&constraints);

        // Loose in main axis with scroll body
        assert_eq!(child_constraints.min_height, 200.0);
        assert_eq!(child_constraints.max_height, f32::INFINITY);
    }

    #[test]
    fn test_fill_remaining_overscroll() {
        let mut fill = RenderSliverFillRemaining::new(false, true);
        // Negative scroll offset indicates overscroll
        let constraints = make_constraints(-50.0, 250.0);
        let child_size = Size::new(400.0, 200.0);

        let geometry = fill.perform_layout_with_child(constraints, child_size);

        // Should extend during overscroll
        assert_eq!(geometry.paint_extent, 250.0);
    }
}
