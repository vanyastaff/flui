//! RenderViewport - Container for sliver content with scrolling

use flui_core::element::ElementTree;
use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that provides a viewport for sliver content
///
/// A viewport is the visible portion of scrollable content. It manages:
/// - Converting scroll offset into sliver constraints
/// - Laying out sliver children with appropriate constraints
/// - Clipping content to viewport bounds
/// - Cache extent for smooth scrolling
///
/// # Coordinate System
///
/// - scroll_offset: 0.0 means top of content visible
/// - Positive scroll_offset scrolls content upward (downward scroll gesture)
/// - viewport_main_axis_extent: Height (vertical) or width (horizontal) of viewport
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderViewport;
/// use flui_types::layout::AxisDirection;
///
/// // Vertical scrolling viewport
/// let viewport = RenderViewport::new(
///     AxisDirection::TopToBottom,
///     600.0,  // viewport height
///     100.0,  // scroll offset
/// );
/// ```
#[derive(Debug)]
pub struct RenderViewport {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    /// Main axis extent (height for vertical, width for horizontal)
    pub viewport_main_axis_extent: f32,
    /// Cross axis extent
    pub cross_axis_extent: f32,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Cache extent for off-screen rendering
    pub cache_extent: f32,
    /// Whether to clip content to viewport bounds
    pub clip_behavior: ClipBehavior,

    // Layout cache
    sliver_geometries: Vec<SliverGeometry>,
}

/// Clipping behavior for viewport
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    /// Clip content to viewport bounds
    HardEdge,
    /// Clip with anti-aliasing
    AntiAlias,
    /// Clip with anti-aliasing and handle edge bleeding
    AntiAliasWithSaveLayer,
}

impl RenderViewport {
    /// Create new viewport
    ///
    /// # Arguments
    /// * `axis_direction` - Direction of scrolling axis
    /// * `viewport_main_axis_extent` - Size of viewport on main axis
    /// * `scroll_offset` - Current scroll position
    pub fn new(
        axis_direction: AxisDirection,
        viewport_main_axis_extent: f32,
        scroll_offset: f32,
    ) -> Self {
        Self {
            axis_direction,
            viewport_main_axis_extent,
            cross_axis_extent: 0.0,
            scroll_offset,
            cache_extent: 250.0, // Default cache extent
            clip_behavior: ClipBehavior::HardEdge,
            sliver_geometries: Vec::new(),
        }
    }

    /// Set scroll offset
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_offset = offset;
    }

    /// Set viewport extent
    pub fn set_viewport_extent(&mut self, extent: f32) {
        self.viewport_main_axis_extent = extent;
    }

    /// Set cache extent
    pub fn set_cache_extent(&mut self, extent: f32) {
        self.cache_extent = extent;
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, behavior: ClipBehavior) {
        self.clip_behavior = behavior;
    }

    /// Get the axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Calculate sliver constraints for children
    fn calculate_sliver_constraints(
        &self,
        remaining_paint_extent: f32,
        scroll_offset: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            axis_direction: self.axis_direction,
            grow_direction_reversed: false,
            scroll_offset,
            remaining_paint_extent,
            cross_axis_extent: self.cross_axis_extent,
            cross_axis_direction: match self.axis_direction.axis() {
                Axis::Vertical => AxisDirection::LeftToRight,
                Axis::Horizontal => AxisDirection::TopToBottom,
            },
            viewport_main_axis_extent: self.viewport_main_axis_extent,
            remaining_cache_extent: self.cache_extent,
            cache_origin: 0.0,
        }
    }

    /// Layout sliver children
    fn layout_slivers(
        &mut self,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) {
        self.sliver_geometries.clear();

        let mut remaining_paint_extent = self.viewport_main_axis_extent;
        let mut current_scroll_offset = self.scroll_offset;

        for _child_id in children {
            let constraints = self.calculate_sliver_constraints(
                remaining_paint_extent,
                current_scroll_offset,
            );

            // In real implementation:
            // 1. Set sliver constraints on child
            // 2. Call child.layout()
            // 3. Get child's SliverGeometry
            // 4. Update remaining_paint_extent and current_scroll_offset

            // For now, create placeholder geometry
            let geometry = SliverGeometry {
                scroll_extent: 100.0,
                paint_extent: remaining_paint_extent.min(100.0),
                layout_extent: remaining_paint_extent.min(100.0),
                max_paint_extent: 100.0,
                visible: remaining_paint_extent > 0.0,
                visible_fraction: 1.0,
                paint_origin: 0.0,
                cross_axis_extent: constraints.cross_axis_extent,
                cache_extent: remaining_paint_extent.min(100.0),
                has_visual_overflow: false,
                hit_test_extent: Some(remaining_paint_extent.min(100.0)),
                scroll_offset_correction: None,
                max_scroll_obsolescence: 0.0,
            };

            self.sliver_geometries.push(geometry);

            remaining_paint_extent -= geometry.paint_extent;
            current_scroll_offset = (current_scroll_offset - geometry.scroll_extent).max(0.0);

            if remaining_paint_extent <= 0.0 {
                break;
            }
        }
    }

    /// Get geometry for child at index
    pub fn geometry_at(&self, index: usize) -> Option<&SliverGeometry> {
        self.sliver_geometries.get(index)
    }
}

impl Default for RenderViewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom, 600.0, 0.0)
    }
}

impl Render for RenderViewport {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // Viewport takes up the space given by box constraints
        let width = constraints.max_width;
        let height = constraints.max_height;

        // Determine cross axis extent based on axis direction
        match self.axis_direction.axis() {
            Axis::Vertical => {
                self.cross_axis_extent = width;
                self.viewport_main_axis_extent = height;
            }
            Axis::Horizontal => {
                self.cross_axis_extent = height;
                self.viewport_main_axis_extent = width;
            }
        }

        // TODO: Layout sliver children
        // self.layout_slivers(tree, children);

        Size::new(width, height)
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // TODO: Apply clipping based on clip_behavior
        // TODO: Paint sliver children at their calculated positions

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Multiple sliver children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_viewport_new() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.viewport_main_axis_extent, 600.0);
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.cache_extent, 250.0);
        assert_eq!(viewport.clip_behavior, ClipBehavior::HardEdge);
    }

    #[test]
    fn test_render_viewport_default() {
        let viewport = RenderViewport::default();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.viewport_main_axis_extent, 600.0);
        assert_eq!(viewport.scroll_offset, 0.0);
    }

    #[test]
    fn test_set_scroll_offset() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_scroll_offset(100.0);

        assert_eq!(viewport.scroll_offset, 100.0);
    }

    #[test]
    fn test_set_viewport_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_viewport_extent(800.0);

        assert_eq!(viewport.viewport_main_axis_extent, 800.0);
    }

    #[test]
    fn test_set_cache_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_cache_extent(500.0);

        assert_eq!(viewport.cache_extent, 500.0);
    }

    #[test]
    fn test_set_clip_behavior() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_clip_behavior(ClipBehavior::AntiAlias);

        assert_eq!(viewport.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_axis_vertical() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_axis_horizontal() {
        let viewport = RenderViewport::new(AxisDirection::LeftToRight, 600.0, 0.0);

        assert_eq!(viewport.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_calculate_sliver_constraints_vertical() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 100.0);
        let constraints = viewport.calculate_sliver_constraints(500.0, 100.0);

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.scroll_offset, 100.0);
        assert_eq!(constraints.remaining_paint_extent, 500.0);
        assert_eq!(constraints.viewport_main_axis_extent, 600.0);
        assert_eq!(constraints.cross_axis_direction, AxisDirection::LeftToRight);
    }

    #[test]
    fn test_calculate_sliver_constraints_horizontal() {
        let viewport = RenderViewport::new(AxisDirection::LeftToRight, 800.0, 50.0);
        let constraints = viewport.calculate_sliver_constraints(700.0, 50.0);

        assert_eq!(constraints.axis_direction, AxisDirection::LeftToRight);
        assert_eq!(constraints.scroll_offset, 50.0);
        assert_eq!(constraints.remaining_paint_extent, 700.0);
        assert_eq!(constraints.viewport_main_axis_extent, 800.0);
        assert_eq!(constraints.cross_axis_direction, AxisDirection::TopToBottom);
    }

    #[test]
    fn test_layout_slivers_single_child() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

        viewport.layout_slivers(&tree, &children);

        assert_eq!(viewport.sliver_geometries.len(), 1);
        let geometry = viewport.geometry_at(0).unwrap();
        assert_eq!(geometry.scroll_extent, 100.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_layout_slivers_multiple_children() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        viewport.layout_slivers(&tree, &children);

        // With placeholder geometry, all children should be laid out
        assert_eq!(viewport.sliver_geometries.len(), 3);
    }

    #[test]
    fn test_geometry_at_valid_index() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

        viewport.layout_slivers(&tree, &children);

        assert!(viewport.geometry_at(0).is_some());
    }

    #[test]
    fn test_geometry_at_invalid_index() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert!(viewport.geometry_at(0).is_none());
    }

    #[test]
    fn test_arity_is_variable() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        assert_eq!(viewport.arity(), Arity::Variable);
    }
}
