//! RenderSliverToBoxAdapter - Adapts box widget to sliver protocol

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adapts a box widget to the sliver protocol
///
/// Allows inserting regular box widgets (like Container, Padding, etc.)
/// into a sliver context (like CustomScrollView). The box widget is given
/// loose constraints based on the sliver's available space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverToBoxAdapter;
///
/// // Wrap a box widget to use it in a sliver scroll view
/// let adapter = RenderSliverToBoxAdapter::new();
/// // Child would be something like RenderContainer, RenderPadding, etc.
/// ```
#[derive(Debug)]
pub struct RenderSliverToBoxAdapter {
    // Layout cache
    child_size: Size,
    sliver_geometry: SliverGeometry,
}

impl RenderSliverToBoxAdapter {
    /// Create new sliver to box adapter
    pub fn new() -> Self {
        Self {
            child_size: Size::ZERO,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Convert sliver constraints to box constraints for child
    fn child_constraints(&self, sliver_constraints: &SliverConstraints) -> BoxConstraints {
        // The child can be as wide as the cross axis extent
        // and as tall as the remaining paint extent (or unlimited)
        let max_width = sliver_constraints.cross_axis_extent;
        let max_height = if sliver_constraints.has_infinite_paint_extent() {
            f32::INFINITY
        } else {
            sliver_constraints.remaining_paint_extent
        };

        BoxConstraints {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    /// Calculate sliver geometry from child size
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_size: Size,
    ) -> SliverGeometry {
        // The main axis extent of the child
        let child_extent = match constraints.axis_direction.axis() {
            Axis::Vertical => child_size.height,
            Axis::Horizontal => child_size.width,
        };

        // Calculate scroll extent and paint extent
        let scroll_extent = child_extent;
        let scroll_offset = constraints.scroll_offset;

        // Determine how much is actually painted
        let paint_extent = if scroll_offset >= scroll_extent {
            // Child is completely scrolled off
            0.0
        } else if scroll_offset + constraints.remaining_paint_extent >= scroll_extent {
            // Child is completely visible
            (scroll_extent - scroll_offset).max(0.0)
        } else {
            // Child is partially visible
            constraints.remaining_paint_extent
        };

        let paint_extent = paint_extent.min(constraints.remaining_paint_extent);

        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
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

impl Default for RenderSliverToBoxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for RenderSliverToBoxAdapter {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // For now, use box constraints
        // In real implementation, this would receive SliverConstraints from viewport
        let constraints = ctx.constraints;

        // Store the size (would be from child in real impl)
        self.child_size = Size::new(
            constraints.max_width.min(constraints.min_width),
            constraints.max_height.min(constraints.min_height),
        );

        self.child_size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // Child painting happens here
        // Would paint the box child with scroll offset applied

        // TODO: Paint child at adjusted offset based on scroll position

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child (the box widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_to_box_adapter_new() {
        let adapter = RenderSliverToBoxAdapter::new();

        assert_eq!(adapter.child_size, Size::ZERO);
    }

    #[test]
    fn test_render_sliver_to_box_adapter_default() {
        let adapter = RenderSliverToBoxAdapter::default();

        assert_eq!(adapter.child_size, Size::ZERO);
    }

    #[test]
    fn test_child_constraints_finite() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
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

        let box_constraints = adapter.child_constraints(&sliver_constraints);

        assert_eq!(box_constraints.min_width, 0.0);
        assert_eq!(box_constraints.max_width, 400.0);
        assert_eq!(box_constraints.min_height, 0.0);
        assert_eq!(box_constraints.max_height, 600.0);
    }

    #[test]
    fn test_child_constraints_infinite() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: f32::INFINITY,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let box_constraints = adapter.child_constraints(&sliver_constraints);

        assert_eq!(box_constraints.max_width, 400.0);
        assert!(box_constraints.max_height.is_infinite());
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_visible() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
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
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is 200px tall, fully visible
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 50.0, // Scrolled 50px
            remaining_paint_extent: 100.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is 200px tall, but only 100px viewport remains
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 100.0); // Clipped to remaining extent
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.5); // 100/200
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Scrolled past child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is scrolled completely off
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_horizontal() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::LeftToRight,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 600.0,
            cross_axis_direction: AxisDirection::TopToBottom,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(300.0, 600.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Horizontal scroll uses width as extent
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_zero_child() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
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

        let child_size = Size::ZERO;
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let adapter = RenderSliverToBoxAdapter::new();
        assert_eq!(adapter.arity(), Arity::Exact(1));
    }
}
