//! RenderSliverFillViewport - Sliver where each child fills the viewport


use crate::core::{RuntimeArity, SliverLayoutContext, SliverPaintContext, LegacySliverRender};
use flui_painting::Canvas;
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

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFillViewport {
    /// Create new sliver fill viewport
    ///
    /// # Arguments
    /// * `viewport_fraction` - Fraction of viewport each child occupies (1.0 = full viewport)
    pub fn new(viewport_fraction: f32) -> Self {
        Self {
            viewport_fraction,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set viewport fraction
    pub fn set_viewport_fraction(&mut self, fraction: f32) {
        self.viewport_fraction = fraction;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) -> SliverGeometry {
        if children.is_empty() {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;
        let viewport_extent = constraints.viewport_main_axis_extent;

        // Each child takes up viewport_fraction * viewport_extent
        let child_extent = viewport_extent * self.viewport_fraction;

        // Total extent is child_extent * number of children
        let child_count = children.len();
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
            max_scroll_obsolescence: 0.0,
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

impl LegacySliverRender for RenderSliverFillViewport {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Each child fills the viewport based on viewport_fraction
        let child_extent = constraints.viewport_main_axis_extent * self.viewport_fraction;
        let child_count = ctx.children.as_slice().len();
        let total_extent = child_extent * child_count as f32;

        // Calculate visible portion
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let paint_extent = (total_extent - scroll_offset).max(0.0).min(remaining_extent);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obsolescence: 0.0,
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

    fn paint(&self, _ctx: &SliverPaintContext) -> Canvas {
        let canvas = Canvas::new();

        // Children are painted by viewport
        // Each child is painted at its calculated position

        // TODO: Paint visible children at their viewport-filling positions

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable // Multiple children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

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
        let tree = ElementTree::new();
        let children = vec![];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_single_child_full_viewport() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 1 child * 600px = 600px
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_multiple_children() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

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
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 children * (600 * 0.5) = 600px total
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 600.0, // Scrolled past first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 3 children * 600px = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        // From offset 600 to 1200 = 600px (second child fully visible)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Halfway through first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

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
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 2000.0, // Scrolled past all children
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 children * 600px = 1200px total
        assert_eq!(geometry.scroll_extent, 1200.0);
        // Nothing visible
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_arity_is_variable() {
        let viewport = RenderSliverFillViewport::new(1.0);
        assert_eq!(viewport.arity(), RuntimeArity::Variable);
    }
}
