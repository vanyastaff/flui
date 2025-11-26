//! RenderSliverFillRemaining - Fills remaining viewport space

use crate::core::{RuntimeArity, SliverLayoutContext, SliverPaintContext, LegacySliverRender};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that fills the remaining space in the viewport
///
/// Unlike RenderSliverFillViewport which sizes children to the viewport,
/// this sliver expands its single child to fill whatever space remains
/// after previous slivers have been laid out.
///
/// # Use Cases
///
/// - Footer content that sticks to bottom when content is short
/// - Expanding content that fills available space
/// - Centering content in remaining space
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFillRemaining;
///
/// // Child will expand to fill all remaining viewport space
/// let fill_remaining = RenderSliverFillRemaining::new();
/// ```
#[derive(Debug)]
pub struct RenderSliverFillRemaining {
    /// Whether to fill overscroll (space beyond content)
    pub has_scrolled_body: bool,
    /// Minimum child extent
    pub fill_overscroll: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFillRemaining {
    /// Create new sliver fill remaining
    pub fn new() -> Self {
        Self {
            has_scrolled_body: false,
            fill_overscroll: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set whether there's scrolled content before this sliver
    pub fn set_has_scrolled_body(&mut self, has_scrolled: bool) {
        self.has_scrolled_body = has_scrolled;
    }

    /// Set whether to fill overscroll area
    pub fn set_fill_overscroll(&mut self, fill: bool) {
        self.fill_overscroll = fill;
    }

    /// Create with overscroll filling enabled
    pub fn with_fill_overscroll(mut self) -> Self {
        self.fill_overscroll = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_size: Size,
    ) -> SliverGeometry {
        let remaining_extent = constraints.remaining_paint_extent;
        let scroll_offset = constraints.scroll_offset;

        // The child's main axis extent
        let child_extent = match constraints.axis_direction.axis() {
            Axis::Vertical => child_size.height,
            Axis::Horizontal => child_size.width,
        };

        // Determine the extent this sliver should report
        let extent = if self.has_scrolled_body {
            // If there's content before us that was scrolled, we take up
            // the remaining space exactly
            remaining_extent.max(child_extent)
        } else {
            // If we're at the top (no scrolled content), we might expand
            // to fill the viewport
            child_extent.max(remaining_extent)
        };

        // Calculate scroll extent
        let scroll_extent = if self.fill_overscroll {
            // Fill any overscroll area
            extent
        } else {
            // Only our actual child size
            child_extent
        };

        // Paint extent is what's actually visible
        let paint_extent = if scroll_offset >= scroll_extent {
            // Completely scrolled off
            0.0
        } else if scroll_offset + remaining_extent >= scroll_extent {
            // Fully visible
            (scroll_extent - scroll_offset).max(0.0)
        } else {
            // Partially visible
            remaining_extent
        };

        let paint_extent = paint_extent.min(remaining_extent);

        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if scroll_extent > 0.0 {
                (paint_extent / scroll_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverFillRemaining {
    fn default() -> Self {
        Self::new()
    }
}

impl LegacySliverRender for RenderSliverFillRemaining {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Layout child with box constraints based on remaining viewport space
        let child_size = if let Some(child_id) = ctx.children.try_single() {
            let remaining_extent = constraints.remaining_paint_extent;
            let box_constraints = BoxConstraints::new(
                0.0,
                constraints.cross_axis_extent,
                0.0,
                remaining_extent,
            );
            ctx.tree.layout_child(child_id, box_constraints)
        } else {
            Size::ZERO
        };

        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(constraints, child_size);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Paint child if present and visible
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                return ctx.tree.paint_child(child_id, ctx.offset);
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_fill_remaining_new() {
        let fill = RenderSliverFillRemaining::new();

        assert!(!fill.has_scrolled_body);
        assert!(!fill.fill_overscroll);
    }

    #[test]
    fn test_render_sliver_fill_remaining_default() {
        let fill = RenderSliverFillRemaining::default();

        assert!(!fill.has_scrolled_body);
        assert!(!fill.fill_overscroll);
    }

    #[test]
    fn test_set_has_scrolled_body() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_has_scrolled_body(true);

        assert!(fill.has_scrolled_body);
    }

    #[test]
    fn test_set_fill_overscroll() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_fill_overscroll(true);

        assert!(fill.fill_overscroll);
    }

    #[test]
    fn test_with_fill_overscroll() {
        let fill = RenderSliverFillRemaining::new().with_fill_overscroll();

        assert!(fill.fill_overscroll);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_scrolled_body() {
        let fill = RenderSliverFillRemaining::new();

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

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 200px, but we expand to fill remaining 600px
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.max_paint_extent, 600.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_with_scrolled_body() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_has_scrolled_body(true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 200.0, // Only 200px left
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 100.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // We expand to fill the remaining 200px
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_child_larger_than_remaining() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 200.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 300.0); // Child is bigger
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 300px, larger than remaining 200px
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 200.0); // Clipped to remaining
    }

    #[test]
    fn test_calculate_sliver_geometry_with_fill_overscroll() {
        let fill = RenderSliverFillRemaining::new().with_fill_overscroll();

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

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // With fill_overscroll, we report the expanded extent
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 500.0, // Scrolled past child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Scrolled past the child
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Partially scrolled
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 500.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 500px, scrolled 100px, showing 300px
        assert_eq!(geometry.scroll_extent, 500.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let fill = RenderSliverFillRemaining::new();
        assert_eq!(fill.arity(), RuntimeArity::Exact(1));
    }
}
