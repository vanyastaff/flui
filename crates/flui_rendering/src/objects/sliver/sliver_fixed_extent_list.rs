//! RenderSliverFixedExtentList - Optimized list with uniform fixed-size items
//!
//! Implements Flutter's SliverFixedExtentList pattern for lists where all children have the same
//! main-axis extent. This uniformity enables O(1) calculations for item positions, visible range,
//! and scroll extent, making it significantly faster than variable-size lists.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverFixedExtentList` | `RenderSliverFixedExtentList` from `package:flutter/src/rendering/sliver_fixed_extent_list.dart` |
//! | `item_extent` property | `itemExtent` property |
//! | `visible_range()` method | Visible item calculation logic |
//! | O(1) position calculation | Flutter's optimization for fixed sizes |
//! | Layout visible children | ✅ Implemented |
//!
//! # Layout Protocol
//!
//! 1. **Calculate visible range** - O(1)
//!    - `first_index = floor(scroll_offset / item_extent)`
//!    - `last_index = ceil((scroll_offset + remaining_paint_extent) / item_extent)`
//!    - Only these children need to be laid out (viewport culling)
//!
//! 2. **Layout visible children**
//!    - For each child in visible range:
//!    - Create BoxConstraints: `width = cross_axis_extent, height = item_extent` (tight constraints)
//!    - Layout child with fixed constraints
//!    - Position at `index * item_extent`
//!
//! 3. **Calculate total geometry**
//!    - scroll_extent: `item_extent * child_count` (O(1))
//!    - paint_extent: visible portion from scroll_offset
//!    - No iteration needed for extent calculation
//!
//! # Paint Protocol
//!
//! 1. **Calculate visible range** - O(1)
//!    - Same as layout visible range
//!
//! 2. **Paint visible children**
//!    - For each child in visible range:
//!    - Calculate offset: `main_axis_offset = index * item_extent - scroll_offset`
//!    - Paint child at calculated position
//!
//! # Performance
//!
//! - **Layout**: O(1) geometry + O(visible) child layout
//! - **Paint**: O(visible) - only visible children painted
//! - **Memory**: 4 bytes (f32 item_extent) + 48 bytes (SliverGeometry) = 52 bytes
//! - **Scroll performance**: O(1) jump to any scroll position
//! - **Viewport culling**: Automatic optimization - offscreen children skipped
//!
//! # Use Cases
//!
//! - **Uniform lists**: Contact lists, message threads with fixed row heights
//! - **Grid layouts**: Fixed-size grid cells (with SliverGrid)
//! - **Performance-critical scrolling**: Large lists with predictable item sizes
//! - **Infinite scroll**: Known item heights enable efficient scrollbar sizing
//! - **Material Design**: List items with standard 56dp height
//! - **Timeline views**: Event timelines with fixed time slot heights
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverList**: FixedExtent is O(1) calculations, List is O(n) for variable sizes
//! - **vs SliverPrototypeExtentList**: FixedExtent has explicit size, Prototype uses first child
//! - **vs SliverFillViewport**: FillViewport sizes to viewport, FixedExtent has arbitrary size
//! - **vs ListView.builder (widget)**: ListView uses FixedExtentList internally when itemExtent provided
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverFixedExtentList;
//!
//! // Material Design list items (56dp height)
//! let material_list = RenderSliverFixedExtentList::new(56.0);
//!
//! // Fixed-height contact list
//! let contact_list = RenderSliverFixedExtentList::new(72.0);
//!
//! // Timeline with hourly slots
//! let timeline = RenderSliverFixedExtentList::new(60.0); // 60px per hour
//! ```

use crate::core::{RenderObject, RenderSliver, SliverLayoutContext, SliverPaintContext, Variable};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, SliverConstraints, SliverGeometry};

/// RenderObject for lists where all items have the same fixed extent.
///
/// Highly optimized version of SliverList for the common case where all children have uniform
/// size on the main axis. The fixed extent enables O(1) calculations for item positions,
/// visible range determination, and scroll extent computation, eliminating the need for
/// iterative measurement or size caching.
///
/// # Arity
///
/// `Variable` - Supports multiple box children (N ≥ 0).
///
/// # Protocol
///
/// **Sliver-to-Box Adapter** - Uses `SliverConstraints`, but children use **BoxConstraints**
/// and return **Size** (not sliver protocol).
///
/// # Pattern
///
/// **Fixed-Size Multi-Child Layout with O(1) Optimization** - Known item size enables
/// instant position calculation without iteration. Viewport culling automatically skips
/// offscreen children. Total extent is simply `item_extent * count`.
///
/// # Use Cases
///
/// - **Uniform lists**: Contact lists, email threads with predictable row heights
/// - **Material Design lists**: Standard 56dp list item height
/// - **Performance optimization**: Large lists where iteration is too expensive
/// - **Timeline layouts**: Fixed time slots (e.g., calendar hours)
/// - **Grid cells**: Fixed-size grid items (combined with SliverGrid)
/// - **Infinite scroll**: Predictable item sizes enable accurate scrollbar thumb sizing
///
/// # Flutter Compliance
///
/// - ✅ Geometry calculation O(1)
/// - ✅ Visible range calculation O(1)
/// - ✅ Child layout with tight constraints
/// - ✅ Paint with viewport culling
///
/// # Performance Benefits
///
/// | Operation | Fixed Extent | Variable Size List |
/// |-----------|--------------|-------------------|
/// | Find visible range | O(1) | O(log n) or O(n) |
/// | Calculate scroll extent | O(1) | O(n) |
/// | Jump to position | O(1) | O(n) |
/// | Memory overhead | None | O(n) size cache |
/// | Viewport culling | Automatic | Requires iteration |
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFixedExtentList;
///
/// // Material Design list (56dp items)
/// let list = RenderSliverFixedExtentList::new(56.0);
///
/// // Performance comparison:
/// // Fixed extent: O(1) to find item 1000
/// // Variable size: O(1000) to calculate positions
/// ```
#[derive(Debug)]
pub struct RenderSliverFixedExtentList {
    /// Fixed extent (height for vertical) for each item
    pub item_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFixedExtentList {
    /// Create new fixed extent list
    ///
    /// # Arguments
    /// * `item_extent` - Fixed size for each item on the main axis
    pub fn new(item_extent: f32) -> Self {
        Self {
            item_extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set item extent
    pub fn set_item_extent(&mut self, extent: f32) {
        self.item_extent = extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate which items are visible
    ///
    /// Returns (first_visible_index, last_visible_index) where last is exclusive
    pub fn visible_range(&self, constraints: &SliverConstraints, child_count: usize) -> (usize, usize) {
        if child_count == 0 || self.item_extent <= 0.0 {
            return (0, 0);
        }

        let scroll_offset = constraints.scroll_offset.max(0.0);
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate first visible item (O(1))
        let first_index = (scroll_offset / self.item_extent).floor() as usize;
        let first_index = first_index.min(child_count.saturating_sub(1));

        // Calculate last visible item (O(1))
        let last_offset = scroll_offset + remaining_extent;
        let last_index = (last_offset / self.item_extent).ceil() as usize;
        let last_index = last_index.min(child_count);

        (first_index, last_index)
    }
}

impl Default for RenderSliverFixedExtentList {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl RenderObject for RenderSliverFixedExtentList {}

impl RenderSliver<Variable> for RenderSliverFixedExtentList {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;

        // Get children
        let children: Vec<_> = ctx.children().collect();
        let child_count = children.len();

        if child_count == 0 || self.item_extent <= 0.0 {
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        }

        // Calculate total extent (O(1))
        let total_extent = self.item_extent * child_count as f32;

        // Calculate visible range (O(1))
        let (first_visible, last_visible) = self.visible_range(&constraints, child_count);

        // Create tight BoxConstraints for children
        let child_constraints = BoxConstraints::new(
            0.0,
            constraints.cross_axis_extent,
            self.item_extent,
            self.item_extent,
        );

        // Layout only visible children for efficiency
        for i in first_visible..last_visible {
            if let Some(child_id) = children.get(i) {
                // Layout child with fixed extent
                ctx.tree_mut().perform_layout(*child_id, child_constraints)?;

                // Position child at index * item_extent
                let child_offset = Offset::new(0.0, i as f32 * self.item_extent);
                ctx.set_child_offset(*child_id, child_offset);
            }
        }

        // Calculate sliver geometry
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let leading_scroll_offset = scroll_offset.max(0.0);
        let trailing_scroll_offset = (scroll_offset + remaining_extent).min(total_extent);
        let paint_extent = (trailing_scroll_offset - leading_scroll_offset).max(0.0);

        self.sliver_geometry = SliverGeometry {
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
        };

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
        let mut canvas = Canvas::new();

        // Calculate visible range for paint
        let children: Vec<_> = ctx.children().collect();
        let child_count = children.len();

        if child_count == 0 || self.item_extent <= 0.0 {
            *ctx.canvas = canvas;
            return;
        }

        // Use cached geometry for scroll offset
        let scroll_offset = ctx.geometry.scroll_extent - ctx.geometry.paint_extent;

        // Calculate visible range (O(1))
        let first_visible = (scroll_offset / self.item_extent).floor() as usize;
        let first_visible = first_visible.min(child_count.saturating_sub(1));

        let last_offset = scroll_offset + ctx.geometry.paint_extent;
        let last_visible = (last_offset / self.item_extent).ceil() as usize;
        let last_visible = last_visible.min(child_count);

        // Paint only visible children
        for i in first_visible..last_visible {
            if let Some(child_id) = children.get(i) {
                // Calculate child offset (O(1) since we know the index)
                let child_offset_y = i as f32 * self.item_extent - scroll_offset;
                let child_offset = Offset::new(ctx.offset.dx, ctx.offset.dy + child_offset_y);

                // Paint child
                if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, child_offset) {
                    canvas.append_canvas(child_canvas);
                }
            }
        }

        *ctx.canvas = canvas;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_fixed_extent_list_new() {
        let list = RenderSliverFixedExtentList::new(50.0);

        assert_eq!(list.item_extent, 50.0);
    }

    #[test]
    fn test_set_item_extent() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        list.set_item_extent(100.0);

        assert_eq!(list.item_extent, 100.0);
    }

    #[test]
    fn test_visible_range_empty() {
        let list = RenderSliverFixedExtentList::new(50.0);

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

        let (first, last) = list.visible_range(&constraints, 0);
        assert_eq!(first, 0);
        assert_eq!(last, 0);
    }

    #[test]
    fn test_visible_range_all_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);

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

        // 10 items * 50px = 500px (all fit in 600px viewport)
        let (first, last) = list.visible_range(&constraints, 10);
        assert_eq!(first, 0);
        assert_eq!(last, 10); // Last index is exclusive
    }

    #[test]
    fn test_visible_range_partial() {
        let list = RenderSliverFixedExtentList::new(50.0);

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

        // 20 items * 50px = 1000px (only first 600px visible = 12 items)
        let (first, last) = list.visible_range(&constraints, 20);
        assert_eq!(first, 0);
        assert_eq!(last, 12); // ceil(600/50) = 12
    }

    #[test]
    fn test_visible_range_scrolled() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled past 2 items
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let (first, last) = list.visible_range(&constraints, 20);
        assert_eq!(first, 2); // floor(100/50) = 2
        assert_eq!(last, 14); // ceil((100+600)/50) = 14
    }

    #[test]
    fn test_visible_range_scrolled_to_end() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 500.0, // Scrolled to last items
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        // 15 items * 50px = 750px total
        let (first, last) = list.visible_range(&constraints, 15);
        assert_eq!(first, 10); // floor(500/50) = 10
        assert_eq!(last, 15); // Capped at child count
    }

    #[test]
    fn test_default() {
        let list = RenderSliverFixedExtentList::default();
        assert_eq!(list.item_extent, 0.0);
    }
}
