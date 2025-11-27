//! RenderSliverFillViewport - Sliver where each child fills the viewport

use crate::core::{ChildrenAccess, LayoutContext, LayoutTree, PaintContext, PaintTree, SliverProtocol, SliverRender, Variable};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject where each child fills the entire viewport
///
/// Each child is sized to exactly fill the viewport's main axis extent.
/// This is commonly used for page views, carousels, or full-screen slides.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFillViewport;
///
/// // Each child will be sized to fill the viewport
/// let viewport_filler = RenderSliverFillViewport::new(1.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverFillViewport {
    /// Fraction of viewport each child should occupy (typically 1.0)
    pub viewport_fraction: f32,

    // Layout cache (set during layout, used during paint)
    cached_child_extent: f32,
    cached_scroll_offset: f32,
    cached_viewport_extent: f32,
}

impl RenderSliverFillViewport {
    /// Create new sliver fill viewport
    ///
    /// # Arguments
    /// * `viewport_fraction` - Fraction of viewport each child occupies (1.0 = full viewport)
    pub fn new(viewport_fraction: f32) -> Self {
        Self {
            viewport_fraction,
            cached_child_extent: 0.0,
            cached_scroll_offset: 0.0,
            cached_viewport_extent: 0.0,
        }
    }

    /// Set viewport fraction
    pub fn set_viewport_fraction(&mut self, fraction: f32) {
        self.viewport_fraction = fraction;
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_count: usize,
    ) -> SliverGeometry {
        if child_count == 0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;
        let viewport_extent = constraints.viewport_main_axis_extent;

        // Each child takes up viewport_fraction * viewport_extent
        let child_extent = viewport_extent * self.viewport_fraction;

        // Total extent is child_extent * number of children
        let total_extent = child_extent * child_count as f32;

        // Calculate visible portion
        let leading_scroll_offset = scroll_offset.max(0.0);
        let trailing_scroll_offset = (scroll_offset + remaining_extent).min(total_extent);

        let paint_extent = (trailing_scroll_offset - leading_scroll_offset).max(0.0);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obstruction_extent: 0.0,
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

impl Default for RenderSliverFillViewport {
    fn default() -> Self {
        Self::new(1.0) // Default to filling entire viewport
    }
}

impl SliverRender<Variable> for RenderSliverFillViewport {
    fn layout<T>(
        &mut self,
        ctx: LayoutContext<'_, T, Variable, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;
        let child_count = ctx.children.len();

        // Cache values for use during paint
        self.cached_viewport_extent = constraints.viewport_main_axis_extent;
        self.cached_scroll_offset = constraints.scroll_offset;
        self.cached_child_extent = constraints.viewport_main_axis_extent * self.viewport_fraction;

        // Calculate geometry using viewport fraction
        // In full implementation, each child would be laid out to fill
        // viewport_fraction * viewport_main_axis_extent
        self.calculate_sliver_geometry(&constraints, child_count)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        use flui_types::geometry::Offset;

        // Use cached values from layout
        let viewport_extent = self.cached_viewport_extent;
        let scroll_offset = self.cached_scroll_offset;
        let child_extent = self.cached_child_extent;

        // Paint visible children at their viewport-filling positions
        let children = ctx.children.iter().collect::<Vec<_>>();

        for (index, &child_id) in children.iter().enumerate() {
            // Calculate child's scroll position
            let child_scroll_position = index as f32 * child_extent;

            // Check if child is visible in viewport
            let child_trailing_edge = child_scroll_position + child_extent;

            if child_trailing_edge > scroll_offset &&
               child_scroll_position < scroll_offset + viewport_extent {
                // Child is at least partially visible
                // Calculate paint offset relative to scroll position
                let child_offset = Offset::new(0.0, child_scroll_position - scroll_offset);
                ctx.paint_child(child_id, ctx.offset + child_offset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};

    #[test]
    fn test_render_sliver_fill_viewport_new() {
        let viewport = RenderSliverFillViewport::new(1.0);

        assert_eq!(viewport.viewport_fraction, 1.0);
    }

    #[test]
    fn test_render_sliver_fill_viewport_default() {
        let viewport = RenderSliverFillViewport::default();

        assert_eq!(viewport.viewport_fraction, 1.0);
    }

    #[test]
    fn test_set_viewport_fraction() {
        let mut viewport = RenderSliverFillViewport::new(1.0);
        viewport.set_viewport_fraction(0.5);

        assert_eq!(viewport.viewport_fraction, 0.5);
    }

    #[test]
    fn test_calculate_sliver_geometry_empty() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 0);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_single_child_full_viewport() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 1);

        // 1 child * 600px = 600px
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_multiple_children() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 3);

        // 3 children * 600px = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        // Only 600px visible (first child)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert!((geometry.visible_fraction - 0.333).abs() < 0.01); // 600/1800 ≈ 0.33
    }

    #[test]
    fn test_calculate_sliver_geometry_half_viewport() {
        let viewport = RenderSliverFillViewport::new(0.5);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 2);

        // 2 children * (600 * 0.5) = 600px total
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 600.0, // Scrolled past first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 3);

        // 3 children * 600px = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        // From offset 600 to 1200 = 600px (second child fully visible)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 300.0, // Halfway through first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 2);

        // 2 children * 600px = 1200px total
        assert_eq!(geometry.scroll_extent, 1200.0);
        // From offset 300 to 900 = 600px
        // (half of first child + half of second child)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let viewport = RenderSliverFillViewport::new(1.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 2000.0, // Scrolled past all children
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, 2);

        // 2 children * 600px = 1200px total
        assert_eq!(geometry.scroll_extent, 1200.0);
        // Nothing visible
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }
}
