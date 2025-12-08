//! RenderViewport - Scrollable viewport container for sliver children
//!
//! Core container that manages scrolling slivers (lists, grids, custom scrollables).
//! Converts scroll offset into SliverConstraints for children, manages layout of multiple
//! slivers in sequence, handles viewport clipping, and coordinates cache extent for smooth
//! scrolling. Essential building block for CustomScrollView, ListView, GridView.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderViewport` | `RenderViewport` from `package:flutter/src/rendering/viewport.dart` |
//! | `scroll_offset` | Current scroll position |
//! | `viewport_main_axis_extent` | Viewport size (height for vertical) |
//! | `cache_extent` | Buffer for prebuilding off-screen children |
//! | `calculate_sliver_constraints()` | Converts scroll offset to SliverConstraints |
//! | `layout_slivers()` | Sequential sliver layout with remaining extent |

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Variable};
use flui_rendering::{RenderObject, RenderResult};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for scrollable viewport containing sliver children.
///
/// Core container for sliver-based scrolling. Converts scroll offset into SliverConstraints,
/// layouts slivers sequentially with remaining extent tracking, manages viewport clipping,
/// and coordinates cache extent for smooth scrolling.
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
            cache_extent: 250.0,
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

impl RenderObject for RenderViewport {}

impl RenderBox<Variable> for RenderViewport {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
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

        // Layout sliver children
        self.sliver_geometries.clear();
        let mut remaining_paint_extent = self.viewport_main_axis_extent;
        let mut current_scroll_offset = self.scroll_offset;

        for child_id in ctx.children() {
            let sliver_constraints = self.calculate_sliver_constraints(
                remaining_paint_extent,
                current_scroll_offset,
            );

            // Layout sliver child using tree's perform_sliver_layout
            let geometry = ctx.tree_mut().perform_sliver_layout(child_id, sliver_constraints)?;

            self.sliver_geometries.push(geometry);

            remaining_paint_extent -= geometry.paint_extent;
            current_scroll_offset = (current_scroll_offset - geometry.scroll_extent).max(0.0);

            if remaining_paint_extent <= 0.0 {
                break;
            }
        }

        Ok(Size::new(width, height))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let mut canvas = Canvas::new();
        let mut paint_offset = 0.0;

        // Paint visible sliver children
        for (i, child_id) in ctx.children().enumerate() {
            if let Some(geometry) = self.sliver_geometries.get(i) {
                if geometry.paint_extent > 0.0 {
                    // Calculate child offset along main axis
                    let child_offset = match self.axis_direction.axis() {
                        Axis::Vertical => Offset::new(ctx.offset.dx, ctx.offset.dy + paint_offset),
                        Axis::Horizontal => Offset::new(ctx.offset.dx + paint_offset, ctx.offset.dy),
                    };

                    let child_canvas = ctx.tree().perform_paint(child_id, child_offset)
                        .unwrap_or_else(|_| Canvas::new());
                    canvas.append_canvas(child_canvas);
                }

                paint_offset += geometry.paint_extent;
            }
        }

        // TODO: Apply clipping based on clip_behavior

        *ctx.canvas = canvas;
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
}
