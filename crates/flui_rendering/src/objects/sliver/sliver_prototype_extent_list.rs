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
//! | Prototype measurement | Measure once, use for all children |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Measure prototype** (NOT IMPLEMENTED)
//!    - Layout first child or designated prototype
//!    - Extract main axis extent
//!    - Cache as prototype_extent
//!
//! 2. **Calculate visible range** (line 70-92 WORKS)
//!    - first_index = floor(scroll_offset / extent)
//!    - last_index = ceil((scroll_offset + remaining) / extent)
//!    - O(1) calculation using cached extent
//!
//! 3. **Layout visible children** (NOT IMPLEMENTED)
//!    - For each child in visible range
//!    - Position = index * prototype_extent
//!    - Layout child (assumed to match prototype extent)
//!
//! 4. **Return geometry** (line 95-140 WORKS)
//!    - scroll_extent = child_count * prototype_extent
//!    - paint_extent = visible portion
//!
//! # Paint Protocol (Intended)
//!
//! 1. **Paint visible children** (NOT IMPLEMENTED - line 187-196 TODO)
//!    - For each child in visible range
//!    - Offset = index * prototype_extent
//!    - Paint child at calculated position
//!
//! # Performance
//!
//! - **Prototype measurement**: O(1) - measure once at startup
//! - **Visible range**: O(1) - simple division/multiplication
//! - **Layout**: O(v) where v = visible children (intended)
//! - **Paint**: O(v) - only visible children (intended)
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
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Prototype NEVER measured** (line 153-154 TODO)
//!    - TODO comment says "Measure prototype if not yet measured"
//!    - Currently uses fallback 50.0 (line 157)
//!    - Defeats purpose of prototype approach!
//!
//! 2. **❌ Children NEVER laid out** (line 154 TODO)
//!    - TODO comment says "Layout visible children"
//!    - No layout_child() calls anywhere
//!    - Children sizes undefined
//!
//! 3. **❌ Paint NOT IMPLEMENTED** (line 187-196)
//!    - Returns empty Canvas
//!    - TODO comment on line 193
//!    - Children never painted
//!
//! 4. **✅ Geometry calculation CORRECT** (line 95-140, 150-185)
//!    - Uses cached prototype_extent correctly
//!    - Fallback to 50.0 when not measured
//!    - Accurate scroll_extent and paint_extent
//!
//! 5. **✅ visible_range() EXCELLENT** (line 70-92)
//!    - Correct O(1) calculation
//!    - Proper floor/ceil logic
//!    - Bounds checking for edge cases
//!
//! **This RenderObject is BROKEN - missing prototype measurement and child layout/paint!**
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
//! // Create list (prototype will be measured)
//! let mut list = RenderSliverPrototypeExtentList::new();
//! // WARNING: prototype never measured - uses fallback 50.0!
//!
//! // After prototype measurement (if implemented)
//! list.set_prototype_extent(72.5);  // Measured from prototype
//! // Now all children assume 72.5px height
//!
//! // Check visible range
//! let (first, last) = list.visible_range(&constraints, 1000);
//! // With scroll_offset=500, extent=50: first=10, last=22 (O(1)!)
//! ```

use flui_core::element::ElementTree;
use crate::core::{RuntimeArity, SliverSliverBoxPaintCtx, LegacySliverRender};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for lazy-loading list with prototype-measured extent.
///
/// Measures ONE prototype item, assumes ALL children have same extent. Combines O(1) calculations
/// of FixedExtentList with actual measurement flexibility. More accurate than hardcoded extent,
/// faster than measuring every child. Perfect for uniform-sized lists with unknown dimensions.
///
/// # Arity
///
/// `RuntimeArity::Variable` - Can have multiple children (0+).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
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
/// **BROKEN IMPLEMENTATION**:
/// - ❌ Prototype never measured (uses fallback 50.0)
/// - ❌ Children never laid out (defeats lazy loading purpose)
/// - ❌ Paint not implemented (empty Canvas)
/// - ✅ Geometry calculation correct (with fallback extent)
/// - ✅ visible_range() excellent (O(1) calculation)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPrototypeExtentList;
///
/// // Create list
/// let mut list = RenderSliverPrototypeExtentList::new();
/// // WARNING: prototype not measured - uses 50.0 fallback!
///
/// // Set extent manually (if measured elsewhere)
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
    /// Returns (first_visible_index, last_visible_index) inclusive
    pub fn visible_range(&self, constraints: &SliverConstraints, child_count: usize) -> (usize, usize) {
        let Some(extent) = self.prototype_extent else {
            return (0, 0);
        };

        if child_count == 0 || extent <= 0.0 {
            return (0, 0);
        }

        let scroll_offset = constraints.scroll_offset.max(0.0);
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate first visible item
        let first_index = (scroll_offset / extent).floor() as usize;
        let first_index = first_index.min(child_count.saturating_sub(1));

        // Calculate last visible item
        let last_offset = scroll_offset + remaining_extent;
        let last_index = (last_offset / extent).ceil() as usize;
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
        let Some(extent) = self.prototype_extent else {
            return SliverGeometry::default();
        };

        if children.is_empty() || extent <= 0.0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let child_count = children.len();
        let total_extent = extent * child_count as f32;

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

impl Default for RenderSliverPrototypeExtentList {
    fn default() -> Self {
        Self::new()
    }
}

impl LegacySliverRender for RenderSliverPrototypeExtentList {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // TODO: Measure prototype if not yet measured
        // TODO: Layout visible children using prototype extent

        // For now, use fixed extent for all children
        let prototype_extent = self.prototype_extent.unwrap_or(50.0);
        let child_count = ctx.children.as_slice().len();
        let total_extent = prototype_extent * child_count as f32;

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

    fn paint(&self, _ctx: &Sliver) -> Canvas {
        let canvas = Canvas::new();

        // Children are painted by viewport at their calculated positions
        // Each child is at prototype_extent * index from the start

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
    fn test_calculate_sliver_geometry_no_prototype() {
        let list = RenderSliverPrototypeExtentList::new();
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

        let geometry = list.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_empty() {
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

        let tree = ElementTree::new();
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
        let mut list = RenderSliverPrototypeExtentList::new();
        list.set_prototype_extent(50.0);

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
        let list = RenderSliverPrototypeExtentList::new();
        assert_eq!(list.arity(), RuntimeArity::Variable);
    }
}
