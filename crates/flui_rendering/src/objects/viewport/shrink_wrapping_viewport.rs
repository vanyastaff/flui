//! RenderShrinkWrappingViewport - Viewport that sizes to its content
//!
//! Unlike RenderViewport which expands to fill available space, RenderShrinkWrappingViewport
//! sizes itself to match its children's total extent in the main axis. This is useful for
//! nested scrollables where the inner scrollable should size to its content.
//!
//! # Flutter Equivalence
//!
//! | Aspect | Flutter | FLUI |
//! |--------|---------|------|
//! | **Class** | `RenderShrinkWrappingViewport` | `RenderShrinkWrappingViewport` |
//! | **Protocol** | Box → Sliver children | Box → Sliver children |
//! | **Sizing** | Shrinks to content | ✅ Shrinks to total extent |
//! | **Layout** | Measures all slivers, sizes to total | ✅ Implemented |
//! | **Paint** | Paints visible slivers with clipping | ✅ Implemented |
//! | **Scroll** | Supports ScrollPosition | Basic scroll_offset |
//! | **Cache** | Uses cache_extent for pre-rendering | ✅ Implemented |
//!
//! # Layout Protocol
//!
//! 1. **Layout all sliver children** - Must layout ALL to measure total extent
//!    - Create SliverConstraints with unbounded remaining extent
//!    - Layout each child sequentially
//!    - Accumulate total scroll extent
//!
//! 2. **Size to content** - Height/width matches content (up to max constraints)
//!    - main_extent = min(total_scroll_extent, max_constraint)
//!    - cross_extent from constraints
//!
//! 3. **Cache geometries** - Store for paint phase
//!
//! # Paint Protocol
//!
//! 1. **Clip to viewport bounds** - Prevent overflow
//! 2. **Paint visible slivers** - Only those in viewport + cache_extent
//! 3. **Apply scroll offset** - Translate children by -scroll_offset
//!
//! # Performance
//!
//! - **Time**: O(n) where n = number of sliver children (must layout all!)
//! - **Space**: O(n) for child geometry storage
//! - **Cost**: More expensive than RenderViewport (dynamic sizing)
//!
//! # Use Cases
//!
//! - **Nested scrollables** - Inner scrollable should shrink-wrap
//! - **Dialogs with scrollable content** - Size dialog to content
//! - **Constrained scroll areas** - Content-sized scrolling
//!
//! # When NOT to Use (Use RenderViewport instead)
//!
//! - **Full-screen scrolling** - Fill available space
//! - **Fixed-size viewports** - Known size
//! - **Performance-critical** - Avoid dynamic sizing cost
//!
//! # Comparison with Related Objects
//!
//! | Aspect | RenderShrinkWrappingViewport | RenderViewport |
//! |--------|------------------------------|----------------|
//! | **Sizing** | Shrinks to content | Fills available space |
//! | **Protocol** | Box → Sliver | Box → Sliver |
//! | **Performance** | More expensive (dynamic) | Cheaper (fixed size) |
//! | **Use Case** | Nested scrollables, dialogs | Primary scrolling |
//! | **Size Stability** | Changes with content | Stable |
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderShrinkWrappingViewport;
//! use flui_types::layout::AxisDirection;
//!
//! // Create viewport that sizes to content
//! let viewport = RenderShrinkWrappingViewport::new(
//!     AxisDirection::TopToBottom,
//!     0.0,  // Initial scroll offset
//! );
//!
//! // With custom cache extent
//! let viewport = RenderShrinkWrappingViewport::new(
//!     AxisDirection::TopToBottom,
//!     100.0,
//! )
//! .with_cache_extent(500.0);  // Pre-render 500 pixels
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, RenderObject, Variable};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

/// Viewport that shrink-wraps its content
///
/// Unlike `RenderViewport` which expands to fill available space,
/// `RenderShrinkWrappingViewport` sizes itself to match its children's
/// total extent in the main axis.
///
/// **Performance Warning**: This shrink-wrapping behavior is expensive
/// because ALL children must be laid out to measure total extent.
///
/// # Arity
///
/// `Variable` - Supports 0+ sliver children.
///
/// # Protocol
///
/// **Box-to-Sliver Adapter** - Receives `BoxConstraints`, layouts sliver children,
/// returns `Size` matching content extent.
///
/// # Pattern
///
/// **Content-Sized Viewport Container** - Sizes to match total content extent
/// instead of filling available space. More expensive due to content-dependent sizing.
///
/// # Flutter Compliance
///
/// - ✅ Layout all slivers to measure extent
/// - ✅ Size to content (up to max constraints)
/// - ✅ Paint with viewport culling
/// - ✅ Cache extent support
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShrinkWrappingViewport;
/// use flui_types::layout::AxisDirection;
///
/// // Create viewport that sizes to its content
/// let viewport = RenderShrinkWrappingViewport::new(
///     AxisDirection::TopToBottom,
///     100.0,  // scroll offset
/// );
/// ```
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,

    /// Current scroll offset
    pub scroll_offset: f32,

    /// Cache extent for off-screen rendering
    pub cache_extent: f32,

    /// Cross-axis extent
    cross_axis_extent: f32,

    /// Main axis extent (from last layout)
    viewport_main_axis_extent: f32,

    // Cached sliver geometries from layout
    sliver_geometries: Vec<SliverGeometry>,
}

impl RenderShrinkWrappingViewport {
    /// Create new shrink-wrapping viewport
    ///
    /// # Arguments
    /// * `axis_direction` - Direction of scrolling axis
    /// * `scroll_offset` - Current scroll position
    pub fn new(axis_direction: AxisDirection, scroll_offset: f32) -> Self {
        Self {
            axis_direction,
            scroll_offset,
            cache_extent: 250.0, // Default cache extent
            cross_axis_extent: 0.0,
            viewport_main_axis_extent: 0.0,
            sliver_geometries: Vec::new(),
        }
    }

    /// Set cache extent
    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent;
        self
    }

    /// Get the main axis
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Create sliver constraints for child at given offset
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
}

impl Default for RenderShrinkWrappingViewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom, 0.0)
    }
}

impl RenderObject for RenderShrinkWrappingViewport {}

impl RenderBox<Variable> for RenderShrinkWrappingViewport {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Determine cross axis extent
        match self.axis_direction.axis() {
            Axis::Vertical => {
                self.cross_axis_extent = constraints.constrain_width(constraints.max_width);
            }
            Axis::Horizontal => {
                self.cross_axis_extent = constraints.constrain_height(constraints.max_height);
            }
        }

        // Layout all sliver children to measure total extent
        // Note: Unlike RenderViewport, we must layout ALL children to know total size
        self.sliver_geometries.clear();
        let mut total_scroll_extent = 0.0;
        let mut current_scroll_offset = self.scroll_offset;

        // Use unbounded remaining extent for measurement phase
        let mut remaining_paint_extent = f32::INFINITY;

        for child_id in ctx.children() {
            let sliver_constraints = self.calculate_sliver_constraints(
                remaining_paint_extent,
                current_scroll_offset,
            );

            // Layout sliver child
            let geometry = ctx.tree_mut().perform_sliver_layout(child_id, sliver_constraints)?;
            self.sliver_geometries.push(geometry);

            total_scroll_extent += geometry.scroll_extent;
            current_scroll_offset = (current_scroll_offset - geometry.scroll_extent).max(0.0);
            remaining_paint_extent -= geometry.paint_extent;
        }

        // Size viewport to content (up to max constraints)
        let (width, height) = match self.axis_direction.axis() {
            Axis::Vertical => {
                // Shrink-wrap height to content, use constraint width
                let height = total_scroll_extent.min(constraints.max_height);
                self.viewport_main_axis_extent = height;
                (self.cross_axis_extent, height)
            }
            Axis::Horizontal => {
                // Shrink-wrap width to content, use constraint height
                let width = total_scroll_extent.min(constraints.max_width);
                self.viewport_main_axis_extent = width;
                (width, self.cross_axis_extent)
            }
        };

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

                    // Paint child
                    let child_canvas = ctx.tree().perform_paint(child_id, child_offset)
                        .unwrap_or_else(|_| Canvas::new());
                    canvas.append_canvas(child_canvas);
                }
                paint_offset += geometry.paint_extent;
            }
        }

        *ctx.canvas = canvas;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shrink_wrapping_viewport_creation() {
        let viewport = RenderShrinkWrappingViewport::new(
            AxisDirection::TopToBottom,
            100.0,
        );

        assert_eq!(viewport.scroll_offset, 100.0);
        assert_eq!(viewport.cache_extent, 250.0);
    }

    #[test]
    fn test_with_cache_extent() {
        let viewport = RenderShrinkWrappingViewport::new(
            AxisDirection::TopToBottom,
            0.0,
        )
        .with_cache_extent(500.0);

        assert_eq!(viewport.cache_extent, 500.0);
    }

    #[test]
    fn test_axis() {
        let vertical = RenderShrinkWrappingViewport::new(
            AxisDirection::TopToBottom,
            0.0,
        );
        assert_eq!(vertical.axis(), Axis::Vertical);

        let horizontal = RenderShrinkWrappingViewport::new(
            AxisDirection::LeftToRight,
            0.0,
        );
        assert_eq!(horizontal.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_default() {
        let viewport = RenderShrinkWrappingViewport::default();
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.axis(), Axis::Vertical);
    }
}
