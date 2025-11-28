//! RenderSliverPadding - Adds padding around sliver content

use crate::core::{
    FullRenderTree,
    LayoutContext, LayoutTree, PaintContext, PaintTree, Single, SliverProtocol, SliverRender,
};
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds padding around sliver content
///
/// Insets the child sliver by the specified padding amounts. This is
/// the sliver equivalent of RenderPadding for boxes.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPadding;
/// use flui_types::EdgeInsets;
///
/// let padding = RenderSliverPadding::new(
///     EdgeInsets::symmetric(16.0, 8.0)
/// );
/// ```
#[derive(Debug)]
pub struct RenderSliverPadding {
    /// Padding to apply
    pub padding: EdgeInsets,
}

impl RenderSliverPadding {
    /// Create new sliver padding
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    /// Create with all sides equal
    pub fn all(amount: f32) -> Self {
        Self::new(EdgeInsets::all(amount))
    }

    /// Create with symmetric padding
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(horizontal, vertical))
    }

    /// Create with individual sides
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(EdgeInsets::new(left, top, right, bottom))
    }

    /// Set padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    /// Calculate adjusted sliver constraints for child
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        // Adjust constraints to account for padding
        let main_axis_padding = match constraints.axis_direction.axis() {
            Axis::Vertical => self.padding.vertical_total(),
            Axis::Horizontal => self.padding.horizontal_total(),
        };

        let cross_axis_padding = match constraints.axis_direction.axis() {
            Axis::Vertical => self.padding.horizontal_total(),
            Axis::Horizontal => self.padding.vertical_total(),
        };

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            growth_direction: constraints.growth_direction,
            user_scroll_direction: constraints.user_scroll_direction,
            scroll_offset: (constraints.scroll_offset - main_axis_padding).max(0.0),
            preceding_scroll_extent: constraints.preceding_scroll_extent,
            overlap: constraints.overlap,
            remaining_paint_extent: (constraints.remaining_paint_extent - main_axis_padding)
                .max(0.0),
            cross_axis_extent: (constraints.cross_axis_extent - cross_axis_padding).max(0.0),
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: (constraints.remaining_cache_extent - main_axis_padding)
                .max(0.0),
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child geometry
    fn child_to_parent_geometry(&self, child_geometry: SliverGeometry) -> SliverGeometry {
        let main_axis_padding = self.padding.vertical_total(); // Assuming vertical for now

        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent + main_axis_padding,
            paint_extent: child_geometry.paint_extent + main_axis_padding,
            paint_origin: child_geometry.paint_origin,
            layout_extent: child_geometry.layout_extent + main_axis_padding,
            max_paint_extent: child_geometry.max_paint_extent + main_axis_padding,
            max_scroll_obstruction_extent: child_geometry.max_scroll_obstruction_extent,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: child_geometry.cross_axis_extent + self.padding.horizontal_total(),
            cache_extent: child_geometry.cache_extent + main_axis_padding,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry
                .hit_test_extent
                .map(|extent| extent + main_axis_padding),
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl SliverRender<Single> for RenderSliverPadding {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Adjust constraints for padding
        let child_constraints = self.child_constraints(&constraints);

        // Layout child
        let child_geometry = ctx.layout_child(ctx.children.single(), child_constraints);

        // Add padding to geometry
        self.child_to_parent_geometry(child_geometry)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Paint child with padding offset
        let padding_offset = Offset::new(self.padding.left, self.padding.top);
        ctx.paint_child(ctx.children.single(), padding_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};

    #[test]
    fn test_render_sliver_padding_new() {
        let padding = RenderSliverPadding::new(EdgeInsets::all(10.0));

        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_sliver_padding_all() {
        let padding = RenderSliverPadding::all(15.0);

        assert_eq!(padding.padding, EdgeInsets::all(15.0));
    }

    #[test]
    fn test_render_sliver_padding_symmetric() {
        let padding = RenderSliverPadding::symmetric(20.0, 10.0);

        assert_eq!(padding.padding, EdgeInsets::symmetric(20.0, 10.0));
    }

    #[test]
    fn test_render_sliver_padding_only() {
        let padding = RenderSliverPadding::only(5.0, 10.0, 15.0, 20.0);

        assert_eq!(padding.padding, EdgeInsets::new(5.0, 10.0, 15.0, 20.0));
    }

    #[test]
    fn test_set_padding() {
        let mut padding = RenderSliverPadding::all(10.0);
        padding.set_padding(EdgeInsets::all(20.0));

        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_child_constraints_vertical() {
        let padding = RenderSliverPadding::symmetric(10.0, 20.0); // h, v

        let parent_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 100.0,
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

        let child_constraints = padding.child_constraints(&parent_constraints);

        // Vertical padding = 40 (20 top + 20 bottom)
        assert_eq!(child_constraints.scroll_offset, 60.0); // 100 - 40
        assert_eq!(child_constraints.remaining_paint_extent, 560.0); // 600 - 40
        assert_eq!(child_constraints.remaining_cache_extent, 960.0); // 1000 - 40

        // Horizontal padding = 20 (10 left + 10 right)
        assert_eq!(child_constraints.cross_axis_extent, 380.0); // 400 - 20
    }

    #[test]
    fn test_child_constraints_clamped_to_zero() {
        let padding = RenderSliverPadding::all(1000.0); // Huge padding

        let parent_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 50.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 100.0,
            cross_axis_extent: 100.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 200.0,
            cache_origin: 0.0,
        ..SliverConstraints::default()
        };

        let child_constraints = padding.child_constraints(&parent_constraints);

        // Should be clamped to 0, not negative
        assert_eq!(child_constraints.scroll_offset, 0.0);
        assert_eq!(child_constraints.remaining_paint_extent, 0.0);
        assert_eq!(child_constraints.cross_axis_extent, 0.0);
        assert_eq!(child_constraints.remaining_cache_extent, 0.0);
    }

    #[test]
    fn test_child_to_parent_geometry() {
        let padding = RenderSliverPadding::symmetric(10.0, 20.0); // h=20, v=40

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 300.0,
            paint_origin: 0.0,
            layout_extent: 300.0,
            max_paint_extent: 500.0,
            max_scroll_obstruction_extent: 0.0,
            visible_fraction: 0.6,
            cross_axis_extent: 360.0,
            cache_extent: 300.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(300.0),
            scroll_offset_correction: None,
        };

        let parent_geometry = padding.child_to_parent_geometry(child_geometry);

        // Vertical padding = 40
        assert_eq!(parent_geometry.scroll_extent, 540.0); // 500 + 40
        assert_eq!(parent_geometry.paint_extent, 340.0); // 300 + 40
        assert_eq!(parent_geometry.layout_extent, 340.0);
        assert_eq!(parent_geometry.max_paint_extent, 540.0);

        // Horizontal padding = 20
        assert_eq!(parent_geometry.cross_axis_extent, 380.0); // 360 + 20

        // Cache extent
        assert_eq!(parent_geometry.cache_extent, 340.0);

        // Hit test extent
        assert_eq!(parent_geometry.hit_test_extent, Some(340.0));

        // Other properties preserved
        assert_eq!(parent_geometry.visible_fraction, 0.6);
        assert!(parent_geometry.visible);
    }

    #[test]
    fn test_child_to_parent_geometry_zero_child() {
        let padding = RenderSliverPadding::all(10.0);

        let child_geometry = SliverGeometry::default();
        let parent_geometry = padding.child_to_parent_geometry(child_geometry);

        // Even with zero child, padding adds extent
        assert_eq!(parent_geometry.scroll_extent, 20.0); // 0 + 20 (top+bottom)
        assert_eq!(parent_geometry.paint_extent, 20.0);
    }
}
