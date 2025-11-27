//! RenderSliverConstrainedCrossAxis - Constrains cross-axis extent for slivers

use crate::core::{LayoutContext, LayoutTree, RenderSliverProxy, Single, SliverProtocol};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that constrains the cross-axis extent of sliver content
///
/// While slivers primarily deal with main-axis scrolling, they also have
/// a cross-axis (width for vertical scrolling, height for horizontal).
/// This widget allows you to constrain that cross-axis extent.
///
/// # Use Cases
///
/// - Limiting list width in wide viewports
/// - Creating narrow centered scrollable content
/// - Responsive design that caps maximum width
/// - Implementing material design width constraints
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverConstrainedCrossAxis;
///
/// // Limit cross-axis extent to 600px max
/// let constrained = RenderSliverConstrainedCrossAxis::new(600.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverConstrainedCrossAxis {
    /// Maximum cross-axis extent
    pub max_extent: f32,
}

impl RenderSliverConstrainedCrossAxis {
    /// Create new sliver constrained cross axis
    ///
    /// # Arguments
    /// * `max_extent` - Maximum cross-axis extent
    pub fn new(max_extent: f32) -> Self {
        Self { max_extent }
    }

    /// Set maximum extent
    pub fn set_max_extent(&mut self, max_extent: f32) {
        self.max_extent = max_extent;
    }

    /// Calculate child constraints with limited cross-axis extent
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        let constrained_cross_axis = constraints.cross_axis_extent.min(self.max_extent);

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            growth_direction: constraints.growth_direction,
            user_scroll_direction: constraints.user_scroll_direction,
            scroll_offset: constraints.scroll_offset,
            preceding_scroll_extent: constraints.preceding_scroll_extent,
            overlap: constraints.overlap,
            remaining_paint_extent: constraints.remaining_paint_extent,
            cross_axis_extent: constrained_cross_axis,
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: constraints.remaining_cache_extent,
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        // Pass through child geometry, but with constrained cross-axis
        let constrained_cross_axis = constraints.cross_axis_extent.min(self.max_extent);

        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent,
            paint_extent: child_geometry.paint_extent,
            paint_origin: child_geometry.paint_origin,
            layout_extent: child_geometry.layout_extent,
            max_paint_extent: child_geometry.max_paint_extent,
            max_scroll_obstruction_extent: child_geometry.max_scroll_obstruction_extent,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: constrained_cross_axis,
            cache_extent: child_geometry.cache_extent,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent,
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverConstrainedCrossAxis {
    fn default() -> Self {
        Self::new(f32::INFINITY) // No constraint by default
    }
}

impl RenderSliverProxy for RenderSliverConstrainedCrossAxis {
    // Layout: custom implementation to constrain cross-axis
    fn proxy_layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Constrain cross-axis for child
        let child_constraints = self.child_constraints(&constraints);

        // Layout child with constrained cross-axis
        let child_geometry = ctx.layout_child(ctx.children.single(), child_constraints);

        // Calculate geometry with constrained cross-axis
        self.calculate_sliver_geometry(&constraints, child_geometry)
    }

    // Paint: use default proxy (child painted normally)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_constrained_cross_axis_new() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        assert_eq!(constrained.max_extent, 600.0);
    }

    #[test]
    fn test_render_sliver_constrained_cross_axis_default() {
        let constrained = RenderSliverConstrainedCrossAxis::default();

        assert_eq!(constrained.max_extent, f32::INFINITY);
    }

    #[test]
    fn test_set_max_extent() {
        let mut constrained = RenderSliverConstrainedCrossAxis::new(600.0);
        constrained.set_max_extent(800.0);

        assert_eq!(constrained.max_extent, 800.0);
    }

    #[test]
    fn test_child_constraints_within_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 400.0, // Less than max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should remain unchanged (400 < 600)
        assert_eq!(child_constraints.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_child_constraints_exceeds_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0, // Exceeds max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should be clamped to max
        assert_eq!(child_constraints.cross_axis_extent, 600.0);
    }

    #[test]
    fn test_child_constraints_exactly_at_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 600.0, // Exactly at max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should be exactly max
        assert_eq!(child_constraints.cross_axis_extent, 600.0);
    }

    #[test]
    fn test_calculate_sliver_geometry() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 500.0,
            layout_extent: 500.0,
            max_paint_extent: 500.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 1000.0, // Child reports unconstrained
            cache_extent: 500.0,
            has_visual_overflow: false,
            hit_test_extent: Some(500.0),
            scroll_offset_correction: None,
            max_scroll_obstruction_extent: 0.0,
        };

        let geometry = constrained.calculate_sliver_geometry(&constraints, child_geometry);

        // Cross-axis should be constrained
        assert_eq!(geometry.cross_axis_extent, 600.0);
        // Other properties pass through
        assert_eq!(geometry.scroll_extent, 500.0);
        assert_eq!(geometry.paint_extent, 500.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_constraint() {
        let constrained = RenderSliverConstrainedCrossAxis::new(f32::INFINITY);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 500.0,
            layout_extent: 500.0,
            max_paint_extent: 500.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 1000.0,
            cache_extent: 500.0,
            has_visual_overflow: false,
            hit_test_extent: Some(500.0),
            scroll_offset_correction: None,
            max_scroll_obstruction_extent: 0.0,
        };

        let geometry = constrained.calculate_sliver_geometry(&constraints, child_geometry);

        // Cross-axis should pass through unchanged
        assert_eq!(geometry.cross_axis_extent, 1000.0);
    }
}
