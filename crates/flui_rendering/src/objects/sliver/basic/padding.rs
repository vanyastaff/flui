//! RenderSliverPadding - adds padding around a sliver child.
//!
//! Insets its sliver child by the given padding on each side.

use flui_types::{EdgeInsets, Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that insets another sliver by the given padding.
///
/// Unlike box padding, sliver padding only applies to the main axis edges
/// (before/after the content in the scroll direction) and the cross axis.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::basic::RenderSliverPadding;
/// use flui_types::EdgeInsets;
///
/// let padding = RenderSliverPadding::new(EdgeInsets::all(16.0));
/// ```
#[derive(Debug)]
pub struct RenderSliverPadding {
    /// The amount of padding.
    padding: EdgeInsets,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Resolved padding for main axis (before).
    before_padding: f32,

    /// Resolved padding for main axis (after).
    after_padding: f32,
}

impl RenderSliverPadding {
    /// Creates a new sliver padding with the given edge insets.
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            before_padding: 0.0,
            after_padding: 0.0,
        }
    }

    /// Returns the current padding.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Sets the padding.
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        if self.padding != padding {
            self.padding = padding;
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

    /// Resolves padding based on scroll direction.
    fn resolve_padding(&mut self, constraints: &SliverConstraints) {
        use flui_types::layout::AxisDirection;

        match constraints.axis_direction {
            AxisDirection::TopToBottom => {
                self.before_padding = self.padding.top;
                self.after_padding = self.padding.bottom;
            }
            AxisDirection::BottomToTop => {
                self.before_padding = self.padding.bottom;
                self.after_padding = self.padding.top;
            }
            AxisDirection::LeftToRight => {
                self.before_padding = self.padding.left;
                self.after_padding = self.padding.right;
            }
            AxisDirection::RightToLeft => {
                self.before_padding = self.padding.right;
                self.after_padding = self.padding.left;
            }
        }
    }

    /// Returns constraints for the child sliver.
    pub fn constraints_for_child(&self, constraints: SliverConstraints) -> SliverConstraints {
        let cross_axis_padding = match constraints.axis_direction {
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => self.padding.horizontal_total(),
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => self.padding.vertical_total(),
        };

        SliverConstraints {
            scroll_offset: (constraints.scroll_offset - self.before_padding).max(0.0),
            remaining_paint_extent: (constraints.remaining_paint_extent - self.before_padding)
                .max(0.0),
            cross_axis_extent: (constraints.cross_axis_extent - cross_axis_padding).max(0.0),
            ..constraints
        }
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.constraints = constraints;
        self.resolve_padding(&constraints);

        // Without child, just account for padding
        let total_padding = self.before_padding + self.after_padding;
        let paint_extent =
            constraints.clamp_paint_extent((total_padding - constraints.scroll_offset).max(0.0));

        self.geometry = SliverGeometry::new(total_padding, paint_extent, 0.0)
            .with_max_paint_extent(total_padding);

        self.geometry
    }

    /// Performs layout with child geometry.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.resolve_padding(&constraints);

        let before_padding_paint_extent = self
            .calculate_before_padding_paint_extent(constraints.scroll_offset, self.before_padding);

        let child_scroll_extent = child_geometry.scroll_extent;
        let child_paint_extent = child_geometry.paint_extent;

        let after_padding_paint_extent = self.calculate_after_padding_paint_extent(
            constraints.remaining_paint_extent - before_padding_paint_extent - child_paint_extent,
        );

        let total_scroll_extent = self.before_padding + child_scroll_extent + self.after_padding;

        let paint_extent = constraints.clamp_paint_extent(
            before_padding_paint_extent + child_paint_extent + after_padding_paint_extent,
        );

        self.geometry = SliverGeometry::new(total_scroll_extent, paint_extent, 0.0)
            .with_max_paint_extent(total_scroll_extent)
            .with_layout_extent(paint_extent);

        self.geometry
    }

    fn calculate_before_padding_paint_extent(
        &self,
        scroll_offset: f32,
        before_padding: f32,
    ) -> f32 {
        (before_padding - scroll_offset).clamp(0.0, before_padding)
    }

    fn calculate_after_padding_paint_extent(&self, remaining: f32) -> f32 {
        remaining.clamp(0.0, self.after_padding)
    }

    /// Returns the offset for the child.
    pub fn child_offset(&self) -> Offset {
        let main_axis_offset = self.calculate_before_padding_paint_extent(
            self.constraints.scroll_offset,
            self.before_padding,
        );

        let cross_axis_padding = match self.constraints.axis_direction {
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => self.padding.left,
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => self.padding.top,
        };

        match self.constraints.axis_direction {
            flui_types::layout::AxisDirection::TopToBottom => {
                Offset::new(cross_axis_padding, main_axis_offset)
            }
            flui_types::layout::AxisDirection::BottomToTop => {
                Offset::new(cross_axis_padding, -main_axis_offset)
            }
            flui_types::layout::AxisDirection::LeftToRight => {
                Offset::new(main_axis_offset, cross_axis_padding)
            }
            flui_types::layout::AxisDirection::RightToLeft => {
                Offset::new(-main_axis_offset, cross_axis_padding)
            }
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
    fn test_padding_new() {
        let padding = RenderSliverPadding::new(EdgeInsets::all(16.0));
        assert_eq!(padding.padding(), EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_layout_no_child() {
        // symmetric(horizontal, vertical) - for vertical scroll, we need vertical padding (top/bottom)
        let mut padding = RenderSliverPadding::new(EdgeInsets::symmetric(0.0, 20.0));
        let constraints = make_constraints(0.0, 400.0);

        let geometry = padding.perform_layout(constraints);

        // Total padding is 40 (top + bottom)
        assert_eq!(geometry.scroll_extent, 40.0);
        assert_eq!(geometry.paint_extent, 40.0);
    }

    #[test]
    fn test_padding_layout_with_child() {
        // symmetric(horizontal, vertical) - for vertical scroll, we need vertical padding (top/bottom)
        let mut padding = RenderSliverPadding::new(EdgeInsets::symmetric(0.0, 20.0));
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = padding.perform_layout_with_child(constraints, child_geometry);

        // 20 + 100 + 20 = 140
        assert_eq!(geometry.scroll_extent, 140.0);
    }

    #[test]
    fn test_padding_constraints_for_child() {
        let mut padding = RenderSliverPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = make_constraints(0.0, 400.0);

        // Initialize before/after padding
        padding.resolve_padding(&constraints);

        let child_constraints = padding.constraints_for_child(constraints);

        // Cross axis padding is left + right = 40
        assert_eq!(child_constraints.cross_axis_extent, 360.0);
    }

    #[test]
    fn test_padding_scrolled() {
        // symmetric(horizontal, vertical) - for vertical scroll, we need vertical padding (top/bottom)
        let mut padding = RenderSliverPadding::new(EdgeInsets::symmetric(0.0, 20.0));
        let constraints = make_constraints(10.0, 400.0);

        let geometry = padding.perform_layout(constraints);

        // Scrolled 10px into the 20px before padding
        // Paint extent should be reduced
        assert_eq!(geometry.scroll_extent, 40.0);
        assert_eq!(geometry.paint_extent, 30.0); // 40 - 10
    }
}
