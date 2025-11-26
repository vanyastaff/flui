//! RenderSliverEdgeInsetsPadding - EdgeInsets-based padding for slivers

use crate::core::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds EdgeInsets padding to sliver content
///
/// Similar to RenderSliverPadding but specifically designed for EdgeInsets.
/// This is a specialized, optimized version for the common case of uniform
/// or asymmetric rectangular padding.
///
/// # Difference from RenderSliverPadding
///
/// - RenderSliverPadding: Generic padding (can be any value)
/// - RenderSliverEdgeInsetsPadding: Specifically EdgeInsets (left/top/right/bottom)
///
/// # Use Cases
///
/// - Adding margin around list content
/// - Creating breathing room in scrollable content
/// - Implementing Material Design spacing
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverEdgeInsetsPadding;
/// use flui_types::EdgeInsets;
///
/// // Symmetric padding
/// let padding = RenderSliverEdgeInsetsPadding::new(
///     EdgeInsets::symmetric(16.0, 8.0), // horizontal, vertical
/// );
///
/// // Asymmetric padding
/// let padding = RenderSliverEdgeInsetsPadding::new(
///     EdgeInsets::new(20.0, 10.0, 20.0, 16.0), // left, top, right, bottom
/// );
/// ```
#[derive(Debug)]
pub struct RenderSliverEdgeInsetsPadding {
    /// Edge insets padding
    pub padding: EdgeInsets,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverEdgeInsetsPadding {
    /// Create new sliver edge insets padding
    ///
    /// # Arguments
    /// * `padding` - EdgeInsets padding values
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate main axis padding
    fn main_axis_padding(&self, axis: Axis) -> (f32, f32) {
        match axis {
            Axis::Vertical => (self.padding.top, self.padding.bottom),
            Axis::Horizontal => (self.padding.left, self.padding.right),
        }
    }

    /// Calculate cross axis padding
    fn cross_axis_padding(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Vertical => self.padding.horizontal_total(),
            Axis::Horizontal => self.padding.vertical_total(),
        }
    }

    /// Calculate child constraints with padding removed
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        let (leading_padding, trailing_padding) = self.main_axis_padding(constraints.axis_direction.axis());
        let cross_padding = self.cross_axis_padding(constraints.axis_direction.axis());

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            grow_direction_reversed: constraints.grow_direction_reversed,
            scroll_offset: (constraints.scroll_offset - leading_padding).max(0.0),
            remaining_paint_extent: (constraints.remaining_paint_extent - leading_padding - trailing_padding).max(0.0),
            cross_axis_extent: (constraints.cross_axis_extent - cross_padding).max(0.0),
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: (constraints.remaining_cache_extent - leading_padding - trailing_padding).max(0.0),
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        let (leading_padding, trailing_padding) = self.main_axis_padding(constraints.axis_direction.axis());
        let total_padding = leading_padding + trailing_padding;

        // Add padding to child's geometry
        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent + total_padding,
            paint_extent: (child_geometry.paint_extent + leading_padding + trailing_padding)
                .min(constraints.remaining_paint_extent),
            paint_origin: child_geometry.paint_origin,
            layout_extent: (child_geometry.layout_extent + leading_padding + trailing_padding)
                .min(constraints.remaining_paint_extent),
            max_paint_extent: child_geometry.max_paint_extent + total_padding,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: child_geometry.cache_extent + leading_padding + trailing_padding,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent.map(|e| e + leading_padding + trailing_padding),
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverEdgeInsetsPadding {
    fn default() -> Self {
        Self::new(EdgeInsets::ZERO)
    }
}

impl LegacySliverRender for RenderSliverEdgeInsetsPadding {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Adjust constraints for child
        let child_constraints = self.child_constraints(constraints);

        // Layout child
        let child_geometry = if let Some(child_id) = ctx.children.try_single() {
            ctx.tree.layout_sliver_child(child_id, child_constraints)
        } else {
            SliverGeometry::default()
        };

        // Calculate and cache geometry with padding
        self.sliver_geometry = self.calculate_sliver_geometry(constraints, child_geometry);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Paint child if present and visible
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                // Paint child with padding offset
                let padding_offset = Offset::new(self.padding.left, self.padding.top);
                return ctx.tree.paint_child(child_id, ctx.offset + padding_offset);
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_edge_insets_padding_new() {
        let padding = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        assert_eq!(sliver.padding, padding);
    }

    #[test]
    fn test_render_sliver_edge_insets_padding_default() {
        let sliver = RenderSliverEdgeInsetsPadding::default();

        assert_eq!(sliver.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_set_padding() {
        let mut sliver = RenderSliverEdgeInsetsPadding::new(EdgeInsets::ZERO);
        let new_padding = EdgeInsets::new(5.0, 10.0, 5.0, 15.0);
        sliver.set_padding(new_padding);

        assert_eq!(sliver.padding, new_padding);
    }

    #[test]
    fn test_main_axis_padding_vertical() {
        let padding = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let (leading, trailing) = sliver.main_axis_padding(Axis::Vertical);
        assert_eq!(leading, 20.0);  // top
        assert_eq!(trailing, 30.0); // bottom
    }

    #[test]
    fn test_main_axis_padding_horizontal() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let (leading, trailing) = sliver.main_axis_padding(Axis::Horizontal);
        assert_eq!(leading, 10.0);  // left
        assert_eq!(trailing, 15.0); // right
    }

    #[test]
    fn test_cross_axis_padding_vertical() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let cross = sliver.cross_axis_padding(Axis::Vertical);
        assert_eq!(cross, 25.0); // left + right = 10 + 15
    }

    #[test]
    fn test_cross_axis_padding_horizontal() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let cross = sliver.cross_axis_padding(Axis::Horizontal);
        assert_eq!(cross, 50.0); // top + bottom = 20 + 30
    }

    #[test]
    fn test_child_constraints() {
        let padding = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = sliver.child_constraints(&constraints);

        // Scroll offset adjusted by leading padding
        assert_eq!(child_constraints.scroll_offset, 60.0); // 100 - 40
        // Remaining paint extent reduced by total padding
        assert_eq!(child_constraints.remaining_paint_extent, 540.0); // 600 - 40 - 20
        // Cross axis unchanged (no horizontal padding)
        assert_eq!(child_constraints.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_calculate_sliver_geometry() {
        let padding = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        // Simulate child geometry
        let child_geometry = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 200.0,
            layout_extent: 200.0,
            max_paint_extent: 200.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 400.0,
            cache_extent: 200.0,
            has_visual_overflow: false,
            hit_test_extent: Some(200.0),
            scroll_offset_correction: None,
            max_scroll_obsolescence: 0.0,
        };

        let geometry = sliver.calculate_sliver_geometry(&constraints, child_geometry);

        // Scroll extent includes padding
        assert_eq!(geometry.scroll_extent, 260.0); // 200 + 40 + 20
        // Paint extent includes padding
        assert_eq!(geometry.paint_extent, 260.0); // 200 + 40 + 20
    }

    #[test]
    fn test_arity_is_single_child() {
        let sliver = RenderSliverEdgeInsetsPadding::new(EdgeInsets::ZERO);
        assert_eq!(sliver.arity(), RuntimeArity::Exact(1));
    }
}
