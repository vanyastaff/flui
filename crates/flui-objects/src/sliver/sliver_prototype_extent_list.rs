//! RenderSliverPrototypeExtentList - Lazy list with prototype-measured extent
//!
//! List where all items assumed to have same size as measured prototype. Measures ONE prototype
//! item, caches its extent, uses that for ALL children. Combines benefits of FixedExtentList
//! (O(1) calculations) with flexibility (actual measurement). More accurate than hardcoded extent,
//! faster than measuring every item. Perfect for lists with consistent but unknown item sizes.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverPrototypeExtentList` | `RenderSliverPrototypeExtentList` from `package:flutter/src/rendering/sliver_prototype_extent_list.dart` |
//! | `prototype_extent` | Cached extent from prototype measurement |
//! | `visible_range()` | Calculate first/last visible indices (O(1)) |
//! | Prototype measurement | ✅ Measure first child, cache extent |
//!
//! # Layout Protocol
//!
//! 1. **Measure prototype** (if not already measured)
//!    - Layout first child with cross-axis constraints
//!    - Extract main axis extent
//!    - Cache as prototype_extent
//!
//! 2. **Calculate visible range** - O(1)
//!    - first_index = floor(scroll_offset / extent)
//!    - last_index = ceil((scroll_offset + remaining) / extent)
//!    - O(1) calculation using cached extent
//!
//! 3. **Layout visible children**
//!    - For each child in visible range
//!    - Position = index * prototype_extent
//!    - Layout child with tight constraints (prototype_extent)
//!
//! 4. **Return geometry**
//!    - scroll_extent = child_count * prototype_extent
//!    - paint_extent = visible portion
//!
//! # Paint Protocol
//!
//! 1. **Paint visible children**
//!    - For each child in visible range
//!    - Offset = index * prototype_extent - scroll_offset
//!    - Paint child at calculated position
//!
//! # Performance
//!
//! - **Prototype measurement**: O(1) - measure once at startup
//! - **Visible range**: O(1) - simple division/multiplication
//! - **Layout**: O(v) where v = visible children
//! - **Paint**: O(v) - only visible children
//! - **Memory**: 16 bytes (Option<f32> + SliverGeometry)
//!
//! # Use Cases
//!
//! - **Uniform unknown sizes**: Cards with consistent but dynamic content
//! - **Responsive lists**: Item size depends on screen width
//! - **Themed lists**: Item size depends on theme settings
//! - **Better than hardcoding**: Actual measurement vs magic numbers
//! - **Faster than variable**: O(1) vs O(n) for full measurement
//!
//! # vs FixedExtentList vs Variable
//!
//! ```text
//! FixedExtentList:
//!   extent = 50.0 (hardcoded) ← FAST but INFLEXIBLE
//!   Pro: No measurement needed
//!   Con: Breaks with theme/screen changes
//!
//! PrototypeExtentList:
//!   extent = measure(prototype) ← FAST and FLEXIBLE
//!   Pro: Adapts to actual content
//!   Con: Requires prototype measurement
//!
//! Variable extent list:
//!   extent[i] = measure(child[i]) ← SLOW but ACCURATE
//!   Pro: Each child can differ
//!   Con: Must measure all children
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverFixedExtentList**: Prototype measures, Fixed uses hardcoded value
//! - **vs SliverList**: Prototype O(1) extent, List measures each child
//! - **vs SliverGrid**: Prototype is 1D list, Grid is 2D
//! - **vs ListView (box)**: PrototypeExtentList is sliver, ListView is widget
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverPrototypeExtentList;
//!
//! // Create list (prototype will be measured on first layout)
//! let mut list = RenderSliverPrototypeExtentList::new();
//!
//! // After first layout, prototype is measured and cached
//! assert!(list.prototype_extent().is_some());
//!
//! // Or set extent manually
//! list.set_prototype_extent(72.5);
//!
//! // Check visible range
//! let (first, last) = list.visible_range(&constraints, 1000);
//! // With scroll_offset=500, extent=50: first=10, last=22 (O(1)!)
//! ```

use flui_rendering::{RenderObject, RenderSliver, SliverLayoutContext, SliverPaintContext, Variable};
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, SliverConstraints, SliverGeometry};

/// RenderObject for lazy-loading list with prototype-measured extent.
///
/// Measures ONE prototype item, assumes ALL children have same extent. Combines O(1) calculations
/// of FixedExtentList with actual measurement flexibility. More accurate than hardcoded extent,
/// faster than measuring every child. Perfect for uniform-sized lists with unknown dimensions.
///
/// # Arity
///
/// `Variable` - Can have multiple children (0+).
///
/// # Protocol
///
/// **Sliver-to-Box Adapter** - Uses `SliverConstraints`, but children use **BoxConstraints**
/// and return **Size** (not sliver protocol).
///
/// # Pattern
///
/// **Prototype-Based Uniform List** - Measure once (prototype), apply to all children,
/// O(1) visible range calculation, lazy loading with viewport culling.
///
/// # Use Cases
///
/// - **Responsive lists**: Item size depends on screen width
/// - **Themed lists**: Size depends on theme settings
/// - **Dynamic content**: Uniform cards with variable content
/// - **Better than hardcoding**: Actual measurement adapts to changes
/// - **Faster than variable**: O(1) extent vs measuring each child
///
/// # Flutter Compliance
///
/// - ✅ Prototype measurement from first child
/// - ✅ Geometry calculation O(1)
/// - ✅ Visible range calculation O(1)
/// - ✅ Child layout with tight constraints
/// - ✅ Paint with viewport culling
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPrototypeExtentList;
///
/// // Create list
/// let mut list = RenderSliverPrototypeExtentList::new();
/// // Prototype will be measured on first layout
///
/// // Or set extent manually (if measured elsewhere)
/// list.set_prototype_extent(72.5);
/// ```
#[derive(Debug)]
pub struct RenderSliverPrototypeExtentList {
    /// Cached extent from prototype measurement
    prototype_extent: Option<f32>,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPrototypeExtentList {
    /// Create new prototype extent list
    pub fn new() -> Self {
        Self {
            prototype_extent: None,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set prototype extent (after measuring prototype)
    pub fn set_prototype_extent(&mut self, extent: f32) {
        self.prototype_extent = Some(extent);
    }

    /// Get prototype extent if available
    pub fn prototype_extent(&self) -> Option<f32> {
        self.prototype_extent
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate which items are visible
    ///
    /// Returns (first_visible_index, last_visible_index) where last is exclusive
    pub fn visible_range(&self, constraints: &SliverConstraints, child_count: usize) -> (usize, usize) {
        let Some(extent) = self.prototype_extent else {
            return (0, 0);
        };

        if child_count == 0 || extent <= 0.0 {
            return (0, 0);
        }

        let scroll_offset = constraints.scroll_offset.max(0.0);
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate first visible item (O(1))
        let first_index = (scroll_offset / extent).floor() as usize;
        let first_index = first_index.min(child_count.saturating_sub(1));

        // Calculate last visible item (O(1))
        let last_offset = scroll_offset + remaining_extent;
        let last_index = (last_offset / extent).ceil() as usize;
        let last_index = last_index.min(child_count);

        (first_index, last_index)
    }
}

impl Default for RenderSliverPrototypeExtentList {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderSliverPrototypeExtentList {}

impl RenderSliver<Variable> for RenderSliverPrototypeExtentList {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;

        // Get children
        let children: Vec<_> = ctx.children().collect();
        let child_count = children.len();

        if child_count == 0 {
            self.sliver_geometry = SliverGeometry::default();
            self.prototype_extent = None;
            return Ok(self.sliver_geometry);
        }

        // Measure prototype if not yet measured
        if self.prototype_extent.is_none() {
            // Use first child as prototype
            if let Some(&prototype_id) = children.first() {
                // Layout prototype with loose constraints to measure its natural size
                let prototype_constraints = BoxConstraints::new(
                    0.0,
                    constraints.cross_axis_extent,
                    0.0,
                    f32::INFINITY,
                );

                let prototype_size = ctx.tree_mut().perform_layout(prototype_id, prototype_constraints)?;

                // Cache the main axis extent (assuming vertical for now)
                // TODO: Use axis_direction to determine which dimension
                self.prototype_extent = Some(prototype_size.height);
            }
        }

        let Some(extent) = self.prototype_extent else {
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        };

        if extent <= 0.0 {
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        }

        // Calculate total extent (O(1))
        let total_extent = extent * child_count as f32;

        // Calculate visible range (O(1))
        let (first_visible, last_visible) = self.visible_range(&constraints, child_count);

        // Create tight BoxConstraints for children
        let child_constraints = BoxConstraints::new(
            0.0,
            constraints.cross_axis_extent,
            extent,
            extent,
        );

        // Layout only visible children for efficiency
        for i in first_visible..last_visible {
            if let Some(child_id) = children.get(i) {
                // Layout child with prototype extent
                ctx.tree_mut().perform_layout(*child_id, child_constraints)?;

                // Position child at index * extent
                let child_offset = Offset::new(0.0, i as f32 * extent);
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

        let Some(extent) = self.prototype_extent else {
            *ctx.canvas = canvas;
            return;
        };

        if child_count == 0 || extent <= 0.0 {
            *ctx.canvas = canvas;
            return;
        }

        // Use cached geometry for scroll offset
        let scroll_offset = ctx.geometry.scroll_extent - ctx.geometry.paint_extent;

        // Calculate visible range (O(1))
        let first_visible = (scroll_offset / extent).floor() as usize;
        let first_visible = first_visible.min(child_count.saturating_sub(1));

        let last_offset = scroll_offset + ctx.geometry.paint_extent;
        let last_visible = (last_offset / extent).ceil() as usize;
        let last_visible = last_visible.min(child_count);

        // Paint only visible children
        for i in first_visible..last_visible {
            if let Some(child_id) = children.get(i) {
                // Calculate child offset (O(1) since we know the index)
                let child_offset_y = i as f32 * extent - scroll_offset;
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
    fn test_render_sliver_prototype_extent_list_new() {
        let list = RenderSliverPrototypeExtentList::new();

        assert!(list.prototype_extent().is_none());
    }

    #[test]
    fn test_render_sliver_prototype_extent_list_default() {
        let list = RenderSliverPrototypeExtentList::default();

        assert!(list.prototype_extent().is_none());
    }

    #[test]
    fn test_set_prototype_extent() {
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(75.0);

        assert_eq!(list.prototype_extent(), Some(75.0));
    }

    #[test]
    fn test_visible_range_no_prototype() {
        let list = RenderSliverPrototypeExtentList::new();

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

        let (first, last) = list.visible_range(&constraints, 10);
        assert_eq!(first, 0);
        assert_eq!(last, 0);
    }

    #[test]
    fn test_visible_range_empty() {
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
    fn test_default() {
        let list = RenderSliverPrototypeExtentList::default();
        assert!(list.prototype_extent().is_none());
    }
}
