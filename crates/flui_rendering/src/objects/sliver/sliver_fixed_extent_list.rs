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
//! | Layout pass | Should layout visible children only |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate visible range** - O(1)
//!    - `first_index = floor(scroll_offset / item_extent)`
//!    - `last_index = ceil((scroll_offset + remaining_paint_extent) / item_extent)`
//!    - Only these children need to be laid out (viewport culling)
//!
//! 2. **Layout visible children**
//!    - For each child in visible range:
//!    - Create BoxConstraints: `width = cross_axis_extent, height = item_extent`
//!    - Layout child with fixed constraints
//!    - Position at `index * item_extent`
//!
//! 3. **Calculate total geometry**
//!    - scroll_extent: `item_extent * child_count` (O(1))
//!    - paint_extent: visible portion from scroll_offset
//!    - No iteration needed for extent calculation
//!
//! # Paint Protocol (Intended)
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
//! - **Layout**: O(1) geometry + O(visible) child layout (when implemented)
//! - **Paint**: O(visible) - only visible children painted (when implemented)
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
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR BUGS AND INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ BUG: Geometry not cached** (line 136)
//!    - `layout()` returns `calculate_sliver_geometry()` result directly
//!    - Does NOT assign to `self.sliver_geometry` first!
//!    - `self.geometry()` always returns default (zero) geometry
//!    - Should be: `self.sliver_geometry = ...; self.sliver_geometry`
//!
//! 2. **❌ Children are NEVER laid out** (line 131-137)
//!    - No calls to `layout_child()` anywhere
//!    - Child sizes are undefined
//!    - Only geometry calculation, no actual layout
//!
//! 3. **❌ Paint not implemented** (line 139-148)
//!    - Returns empty canvas
//!    - TODO comment: "Paint visible children at their positions"
//!    - Children are never painted
//!
//! 4. **✅ Geometry calculation correct**
//!    - `calculate_sliver_geometry()` logic is sound
//!    - `visible_range()` utility method works correctly
//!    - Just not being used properly due to bugs above
//!
//! **This RenderObject is BROKEN - bug in geometry caching, no layout or paint!**
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
//! // Note: Won't render due to bugs! Needs fixes.
//!
//! // Fixed-height contact list
//! let contact_list = RenderSliverFixedExtentList::new(72.0);
//!
//! // Timeline with hourly slots
//! let timeline = RenderSliverFixedExtentList::new(60.0); // 60px per hour
//! ```

use flui_core::element::ElementTree;
use crate::core::{RuntimeArity, SliverSliverBoxPaintCtx, LegacySliverRender};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for lists where all items have the same fixed extent.
///
/// Highly optimized version of SliverList for the common case where all children have uniform
/// size on the main axis. The fixed extent enables O(1) calculations for item positions,
/// visible range determination, and scroll extent computation, eliminating the need for
/// iterative measurement or size caching.
///
/// # Arity
///
/// `RuntimeArity::Variable` - Supports multiple box children (N ≥ 0).
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
/// **INCOMPLETE + BUGGY IMPLEMENTATION**:
/// - ❌ Geometry not cached (layout bug)
/// - ❌ Child layout not implemented
/// - ❌ Paint not implemented
/// - ✅ Geometry calculation logic correct
/// - ✅ Visible range calculation correct
///
/// # Performance Benefits (When Implemented)
///
/// | Operation | Fixed Extent | Variable Size List |
/// |-----------|--------------|-------------------|
/// | Find visible range | O(1) | O(log n) or O(n) |
/// | Calculate scroll extent | O(1) | O(n) |
/// | Jump to position | O(1) | O(n) |
/// | Memory overhead | None | O(n) size cache |
/// | Viewport culling | Automatic | Requires iteration |
///
/// # Implementation Status
///
/// **Current State (BROKEN):**
/// - ⚠️ BUG: `layout()` doesn't cache geometry
/// - ❌ Children never laid out
/// - ❌ Paint returns empty canvas
/// - ✅ `visible_range()` utility works correctly
/// - ✅ `calculate_sliver_geometry()` logic sound
///
/// **Missing from Flutter:**
/// - Layout visible children with fixed BoxConstraints
/// - Paint children at calculated positions
/// - Cache calculated geometry properly
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFixedExtentList;
///
/// // Material Design list (56dp items)
/// let list = RenderSliverFixedExtentList::new(56.0);
/// // Warning: Has bugs - won't render correctly!
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
    /// Returns (first_visible_index, last_visible_index) inclusive
    pub fn visible_range(&self, constraints: &SliverConstraints, child_count: usize) -> (usize, usize) {
        if child_count == 0 || self.item_extent <= 0.0 {
            return (0, 0);
        }

        let scroll_offset = constraints.scroll_offset.max(0.0);
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate first visible item
        let first_index = (scroll_offset / self.item_extent).floor() as usize;
        let first_index = first_index.min(child_count.saturating_sub(1));

        // Calculate last visible item
        let last_offset = scroll_offset + remaining_extent;
        let last_index = (last_offset / self.item_extent).ceil() as usize;
        let last_index = last_index.min(child_count);

        (first_index, last_index)
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) -> SliverGeometry {
        if children.is_empty() || self.item_extent <= 0.0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let child_count = children.len();
        let total_extent = self.item_extent * child_count as f32;

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

impl LegacySliverRender for RenderSliverFixedExtentList {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Calculate and cache sliver geometry
        let children_slice = ctx.children.as_slice();
        self.calculate_sliver_geometry(constraints, ctx.tree, children_slice)
    }

    fn paint(&self, _ctx: &Sliver) -> Canvas {
        let canvas = Canvas::new();

        // Children are painted by viewport at their calculated positions
        // Each child is at item_extent * index from the start

        // TODO: Paint visible children at their positions

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
    fn test_calculate_sliver_geometry_empty() {
        let list = RenderSliverFixedExtentList::new(50.0);
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

        let geometry = list.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_all_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);
        let tree = ElementTree::new();

        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
            flui_core::element::ElementId::new(5),
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

        let geometry = list.calculate_sliver_geometry(&constraints, &tree, &children);

        // 5 items * 50px = 250px
        assert_eq!(geometry.scroll_extent, 250.0);
        assert_eq!(geometry.paint_extent, 250.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);
        let tree = ElementTree::new();

        // 20 items
        let children: Vec<_> = (1..=20)
            .map(|i| flui_core::element::ElementId::new(i))
            .collect();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 300.0, // Only 300px visible
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = list.calculate_sliver_geometry(&constraints, &tree, &children);

        // 20 items * 50px = 1000px total
        assert_eq!(geometry.scroll_extent, 1000.0);
        // Only 300px visible
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.3);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled() {
        let list = RenderSliverFixedExtentList::new(50.0);
        let tree = ElementTree::new();

        let children: Vec<_> = (1..=20)
            .map(|i| flui_core::element::ElementId::new(i))
            .collect();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 200.0, // Scrolled 200px
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = list.calculate_sliver_geometry(&constraints, &tree, &children);

        // 20 items * 50px = 1000px total
        assert_eq!(geometry.scroll_extent, 1000.0);
        // From 200 to 500 = 300px visible
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_variable() {
        let list = RenderSliverFixedExtentList::new(50.0);
        assert_eq!(list.arity(), RuntimeArity::Variable);
    }
}
