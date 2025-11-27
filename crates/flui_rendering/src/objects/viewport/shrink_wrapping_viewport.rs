//! RenderShrinkWrappingViewport - Viewport that sizes to content
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderShrinkWrappingViewport-class.html>
//!
//! # Overview
//!
//! Unlike `RenderViewport` which takes up all available space along the main axis,
//! `RenderShrinkWrappingViewport` sizes itself to the total extent of its sliver
//! children. This is useful for nested scrollable areas or scrollables within
//! non-scrollable contexts.
//!
//! # When to Use
//!
//! - ListView inside a Column/Row
//! - Nested scroll views
//! - Scrollable content with unknown/variable size
//!
//! # Limitations
//!
//! - Cannot scroll in the reverse direction (no negative scroll offsets)
//! - Performance may be worse than RenderViewport for large lists

use crate::core::arity::Variable;
use crate::core::contexts::{HitTestContext, LayoutContext, PaintContext};
use crate::core::protocol::BoxProtocol;
use crate::core::render_box::RenderBox;
use crate::core::render_tree::{HitTestTree, LayoutTree, PaintTree};
use crate::core::ElementId;
use flui_interaction::HitTestResult;
use flui_types::constraints::{GrowthDirection, ScrollDirection};
use flui_types::layout::{Axis, AxisDirection};
use flui_types::{Offset, Rect, Size, SliverConstraints, SliverGeometry};

use super::render_viewport::ClipBehavior;

/// Viewport that sizes itself to its content
///
/// This viewport computes its main axis extent based on the total scroll
/// extent of its sliver children, rather than expanding to fill available space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::viewport::RenderShrinkWrappingViewport;
/// use flui_types::layout::AxisDirection;
///
/// // Create a shrink-wrapping vertical scroll viewport
/// let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
/// ```
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    /// Cross axis direction
    pub cross_axis_direction: AxisDirection,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Whether to clip content to viewport bounds
    pub clip_behavior: ClipBehavior,

    // Layout cache
    size: Size,
    sliver_geometries: Vec<SliverLayoutData>,
    has_visual_overflow: bool,

    // Computed extents
    max_scroll_extent: f32,
    shrink_wrap_extent: f32,
}

/// Layout data for each sliver child
#[derive(Debug, Clone, Default)]
struct SliverLayoutData {
    /// The sliver's geometry result
    pub geometry: SliverGeometry,
    /// Paint offset relative to viewport
    pub paint_offset: Offset,
}

impl RenderShrinkWrappingViewport {
    /// Create new shrink-wrapping viewport
    ///
    /// # Arguments
    /// * `axis_direction` - Direction of scrolling axis
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self {
            axis_direction,
            cross_axis_direction: match axis_direction.axis() {
                Axis::Vertical => AxisDirection::LeftToRight,
                Axis::Horizontal => AxisDirection::TopToBottom,
            },
            scroll_offset: 0.0,
            clip_behavior: ClipBehavior::HardEdge,
            size: Size::ZERO,
            sliver_geometries: Vec::new(),
            has_visual_overflow: false,
            max_scroll_extent: 0.0,
            shrink_wrap_extent: 0.0,
        }
    }

    /// Set scroll offset
    pub fn set_scroll_offset(&mut self, offset: f32) {
        // Shrink-wrapping viewports don't support negative scroll offsets
        self.scroll_offset = offset.max(0.0);
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, behavior: ClipBehavior) {
        self.clip_behavior = behavior;
    }

    /// Get the axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Get the current size
    pub fn size(&self) -> Size {
        self.size
    }

    /// Check if viewport has visual overflow
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    /// Get maximum scroll extent
    pub fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    /// Get the shrink-wrapped extent (total content size)
    pub fn shrink_wrap_extent(&self) -> f32 {
        self.shrink_wrap_extent
    }

    /// Get cross axis extent from size
    #[allow(dead_code)]
    fn cross_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Vertical => self.size.width,
            Axis::Horizontal => self.size.height,
        }
    }

    /// Get paint offset for sliver based on layout position
    fn compute_paint_offset(&self, layout_offset: f32, geometry: &SliverGeometry) -> Offset {
        let main_axis_offset = layout_offset - self.scroll_offset;

        match self.axis() {
            Axis::Vertical => Offset::new(0.0, main_axis_offset + geometry.paint_origin),
            Axis::Horizontal => Offset::new(main_axis_offset + geometry.paint_origin, 0.0),
        }
    }
}

impl RenderBox<Variable> for RenderShrinkWrappingViewport {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        // Get cross axis extent from constraints
        let cross_axis_extent = match self.axis() {
            Axis::Vertical => ctx.constraints.max_width,
            Axis::Horizontal => ctx.constraints.max_height,
        };

        // Get sliver children
        let children: Vec<ElementId> = ctx
            .children
            .iter()
            .map(|id| ElementId::new(id.get()))
            .collect();

        if children.is_empty() {
            self.size = match self.axis() {
                Axis::Vertical => Size::new(cross_axis_extent, 0.0),
                Axis::Horizontal => Size::new(0.0, cross_axis_extent),
            };
            self.max_scroll_extent = 0.0;
            self.shrink_wrap_extent = 0.0;
            self.has_visual_overflow = false;
            return self.size;
        }

        self.sliver_geometries.clear();
        self.sliver_geometries.reserve(children.len());

        // For shrink-wrapping, we use infinite remaining paint extent
        // to let slivers report their full extent
        let mut total_scroll_extent = 0.0f32;
        let mut current_scroll_offset = self.scroll_offset;
        let mut preceding_scroll_extent = 0.0f32;
        let mut has_visual_overflow = false;

        for &child_id in &children {
            let sliver_scroll_offset = current_scroll_offset.max(0.0);

            let constraints = SliverConstraints {
                axis_direction: self.axis_direction,
                growth_direction: GrowthDirection::Forward,
                user_scroll_direction: ScrollDirection::Idle,
                scroll_offset: sliver_scroll_offset,
                preceding_scroll_extent,
                overlap: 0.0,
                remaining_paint_extent: f32::INFINITY,
                cross_axis_extent,
                cross_axis_direction: self.cross_axis_direction,
                viewport_main_axis_extent: f32::INFINITY,
                remaining_cache_extent: f32::INFINITY,
                cache_origin: 0.0,
            };

            let geometry = ctx
                .tree_mut()
                .perform_sliver_layout(child_id, constraints)
                .unwrap_or_default();

            // Calculate paint offset
            let layout_offset = total_scroll_extent;
            let paint_offset = self.compute_paint_offset(layout_offset, &geometry);

            // Store layout data
            self.sliver_geometries.push(SliverLayoutData {
                geometry,
                paint_offset,
            });

            // Update tracking values
            total_scroll_extent += geometry.scroll_extent;
            has_visual_overflow = has_visual_overflow || geometry.has_visual_overflow;
            current_scroll_offset -= geometry.scroll_extent;
            preceding_scroll_extent += geometry.scroll_extent;
        }

        // The shrink-wrap extent is the total scroll extent
        self.shrink_wrap_extent = total_scroll_extent;

        // Compute the viewport's main axis extent
        // It should be the smaller of: total content or max constraint
        let main_axis_extent = match self.axis() {
            Axis::Vertical => total_scroll_extent.min(ctx.constraints.max_height),
            Axis::Horizontal => total_scroll_extent.min(ctx.constraints.max_width),
        };

        // Calculate max scroll extent
        self.max_scroll_extent = (total_scroll_extent - main_axis_extent).max(0.0);
        self.has_visual_overflow = has_visual_overflow;

        // Compute final size
        self.size = match self.axis() {
            Axis::Vertical => Size::new(cross_axis_extent, main_axis_extent),
            Axis::Horizontal => Size::new(main_axis_extent, cross_axis_extent),
        };

        // Update paint offsets now that we know the viewport size
        // (needed for correct clipping)
        // Collect layout offset and paint_origin for each sliver
        let updates: Vec<(f32, f32)> = {
            let mut layout_offset = 0.0f32;
            self.sliver_geometries
                .iter()
                .map(|data| {
                    let offset = layout_offset;
                    layout_offset += data.geometry.scroll_extent;
                    (offset, data.geometry.paint_origin)
                })
                .collect()
        };

        // Then update paint offsets
        let axis = self.axis();
        let scroll_offset = self.scroll_offset;
        for (i, (layout_offset, paint_origin)) in updates.iter().enumerate() {
            if let Some(data) = self.sliver_geometries.get_mut(i) {
                let main_axis_offset = layout_offset - scroll_offset + paint_origin;
                data.paint_offset = match axis {
                    Axis::Vertical => Offset::new(0.0, main_axis_offset),
                    Axis::Horizontal => Offset::new(main_axis_offset, 0.0),
                };
            }
        }

        self.size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        // Apply clipping if needed using chaining API
        let needs_clip = self.clip_behavior != ClipBehavior::None;
        if needs_clip {
            let clip_rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);
            ctx.canvas().saved().clipped_rect(clip_rect);
        }

        // Collect children to avoid borrow issues
        let children: Vec<_> = ctx.children.iter().collect();

        // Paint each sliver at its computed offset
        for (i, child_id) in children.iter().enumerate() {
            if let Some(layout_data) = self.sliver_geometries.get(i) {
                if layout_data.geometry.visible {
                    let paint_offset = ctx.offset + layout_data.paint_offset;
                    ctx.paint_child(*child_id, paint_offset);
                }
            }
        }

        // Restore clipping
        if needs_clip {
            ctx.canvas().restored();
        }
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Variable, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Check if position is within viewport bounds
        if !ctx.contains(ctx.position) {
            return false;
        }

        // Collect children to allow reverse iteration
        let children: Vec<_> = ctx.children.iter().collect();

        // Hit test slivers in reverse paint order (last painted = on top)
        for (i, child_id) in children.iter().enumerate().rev() {
            if let Some(layout_data) = self.sliver_geometries.get(i) {
                if layout_data.geometry.visible {
                    let local_position = ctx.position - layout_data.paint_offset;

                    // Check if position is within sliver's paint extent
                    let in_sliver = match self.axis() {
                        Axis::Vertical => {
                            local_position.dy >= 0.0
                                && local_position.dy < layout_data.geometry.paint_extent
                        }
                        Axis::Horizontal => {
                            local_position.dx >= 0.0
                                && local_position.dx < layout_data.geometry.paint_extent
                        }
                    };

                    if in_sliver {
                        ctx.add_to_result(result);
                        return true;
                    }
                }
            }

            // Suppress unused variable warning
            let _ = child_id;
        }

        false
    }

    fn hit_test_self(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }
}

impl Default for RenderShrinkWrappingViewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shrink_wrapping_viewport_new() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.clip_behavior, ClipBehavior::HardEdge);
    }

    #[test]
    fn test_shrink_wrapping_viewport_default() {
        let viewport = RenderShrinkWrappingViewport::default();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
    }

    #[test]
    fn test_set_scroll_offset_clamps_negative() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);

        viewport.set_scroll_offset(-100.0);
        assert_eq!(viewport.scroll_offset, 0.0);

        viewport.set_scroll_offset(100.0);
        assert_eq!(viewport.scroll_offset, 100.0);
    }

    #[test]
    fn test_set_clip_behavior() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        viewport.set_clip_behavior(ClipBehavior::AntiAlias);

        assert_eq!(viewport.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_axis_vertical() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);

        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::BottomToTop);
        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_axis_horizontal() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.axis(), Axis::Horizontal);

        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::RightToLeft);
        assert_eq!(viewport.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_compute_paint_offset_vertical() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);
        viewport.scroll_offset = 50.0;

        let geometry = SliverGeometry {
            paint_extent: 100.0,
            paint_origin: 0.0,
            ..Default::default()
        };

        let offset = viewport.compute_paint_offset(100.0, &geometry);

        // layout_offset (100) - scroll_offset (50) = 50
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 50.0);
    }

    #[test]
    fn test_compute_paint_offset_horizontal() {
        let mut viewport = RenderShrinkWrappingViewport::new(AxisDirection::LeftToRight);
        viewport.scroll_offset = 30.0;

        let geometry = SliverGeometry {
            paint_extent: 100.0,
            paint_origin: 0.0,
            ..Default::default()
        };

        let offset = viewport.compute_paint_offset(80.0, &geometry);

        // layout_offset (80) - scroll_offset (30) = 50
        assert_eq!(offset.dx, 50.0);
        assert_eq!(offset.dy, 0.0);
    }

    #[test]
    fn test_initial_extents() {
        let viewport = RenderShrinkWrappingViewport::new(AxisDirection::TopToBottom);

        assert_eq!(viewport.max_scroll_extent(), 0.0);
        assert_eq!(viewport.shrink_wrap_extent(), 0.0);
        assert!(!viewport.has_visual_overflow());
    }
}
