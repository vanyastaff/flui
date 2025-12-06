//! RenderSliverCrossAxisGroup - Groups multiple slivers along cross axis
//!
//! Implements Flutter's SliverCrossAxisGroup pattern for arranging sliver children side-by-side
//! perpendicular to the scroll direction. In vertical scrolling, children are placed horizontally
//! (left-to-right). In horizontal scrolling, children are placed vertically (top-to-bottom).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverCrossAxisGroup` | `RenderSliverCrossAxisGroup` from `package:flutter/src/rendering/sliver_multi_box_adaptor.dart` |
//! | `child_cross_axis_extents` | Per-child cross-axis extent tracking |
//! | `child_cross_axis_offsets` | Cross-axis positioning |
//! | Equal distribution (current) | Flex factor distribution (Flutter full implementation) |
//! | Max scroll extent logic | Flutter's `maxScrollExtent` aggregation |
//!
//! # Layout Protocol
//!
//! 1. **Divide cross-axis space**
//!    - Currently: Equal distribution among all children
//!    - Flutter full: Flex factor-based distribution
//!    - `child_cross_extent = total_cross_extent / num_children`
//!
//! 2. **Layout each child with constrained cross-axis**
//!    - Create child constraints with adjusted cross_axis_extent
//!    - All other constraint fields pass through unchanged
//!    - Layout child and collect geometry
//!
//! 3. **Aggregate geometries**
//!    - scroll_extent: Maximum from all children (longest scrollable content)
//!    - paint_extent: Maximum from all children (tallest visible portion)
//!    - cache_extent: Sum of all children (total cached area)
//!    - visible: True if any child is visible
//!
//! 4. **Store child positions**
//!    - Track cross_axis_offsets for paint phase
//!    - Track cross_axis_extents for each child
//!
//! # Paint Protocol
//!
//! 1. **Paint each child at cross-axis offset**
//!    - Retrieve stored cross_axis_offset for child
//!    - Calculate child's paint offset based on scroll axis
//!    - Paint child and append to parent canvas
//!
//! 2. **⚠️ CURRENT LIMITATION**
//!    - Paint assumes vertical scrolling (cross-axis = horizontal)
//!    - Hardcodes offset calculation: `dx + cross_offset`
//!    - Needs axis direction detection for horizontal scroll support
//!
//! # Performance
//!
//! - **Layout**: O(N) - layout all children sequentially
//! - **Paint**: O(N) - paint all children
//! - **Memory**: 16 bytes (SliverGeometry) + 2*8N bytes (Vec<f32> offsets/extents)
//! - **Layout cost**: N × child_layout_cost (no parallelization)
//!
//! # Use Cases
//!
//! - **Multi-column scrolling**: Side-by-side lists in vertical scroll
//! - **Calendar columns**: Day columns in calendar week view
//! - **Split-screen content**: Multiple independent scrollable sections
//! - **Flexible sliver layouts**: Responsive multi-column designs
//! - **Side-by-side feeds**: Social media multi-feed layouts
//! - **Comparative views**: A/B comparison of scrollable content
//!
//! # Current Implementation Limitations
//!
//! 1. **⚠️ No flex factor support**: All children get equal cross-axis space (line 91)
//!    - Flutter's full implementation supports flex factors for proportional distribution
//!    - Current: `child_extent = total / count`
//!    - Needed: `child_extent = total * (flex / total_flex)`
//!
//! 2. **⚠️ Hardcoded vertical scroll assumption** (line 174)
//!    - Paint assumes vertical scrolling (cross-axis = horizontal)
//!    - Adds cross_offset to dx only
//!    - Needs axis direction check for horizontal scroll
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverMainAxisGroup**: CrossAxis arranges perpendicular, MainAxis arranges along scroll
//! - **vs SliverConstrainedCrossAxis**: ConstrainedCrossAxis limits single child, CrossAxisGroup splits among many
//! - **vs RenderFlex (box)**: CrossAxisGroup is sliver protocol, Flex is box protocol
//! - **vs SliverList**: List has variable main-axis children, CrossAxisGroup has cross-axis children
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverCrossAxisGroup;
//!
//! // Two side-by-side lists (50% width each)
//! let group = RenderSliverCrossAxisGroup::new();
//! // Add children: [SliverList, SliverList]
//! // Each gets 50% of cross-axis space automatically
//!
//! // Three-column layout (33.3% each)
//! let three_column = RenderSliverCrossAxisGroup::new();
//! // Add children: [SliverList, SliverList, SliverList]
//! // Each gets 33.3% of cross-axis space
//!
//! // Calendar week view (7 day columns)
//! let calendar_week = RenderSliverCrossAxisGroup::new();
//! // Add 7 children, each gets 1/7 of width
//! ```

use crate::core::{RenderObject, RenderSliver, Variable, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::{Offset, SliverConstraints, SliverGeometry};

/// RenderObject that groups multiple slivers along the cross axis.
///
/// Places sliver children side-by-side perpendicular to the scroll direction.
/// In vertical scrolling, children are arranged horizontally (left-to-right).
/// In horizontal scrolling, children are arranged vertically (top-to-bottom).
/// Each child receives a portion of the cross-axis space while sharing the same
/// main-axis scroll position.
///
/// # Arity
///
/// `RuntimeArity::Variable` - Supports multiple sliver children (N ≥ 0).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Cross-Axis Multi-Child Layout** - Divides cross-axis space among children,
/// lays them out in parallel at different cross-axis offsets, and aggregates
/// their geometries using max scroll extent and sum cache extent.
///
/// # Use Cases
///
/// - **Multi-column lists**: Side-by-side scrollable lists (e.g., comparison views)
/// - **Calendar week view**: 7 day columns scrolling vertically
/// - **Split-screen scrolling**: Multiple independent scrollable sections
/// - **Responsive layouts**: Adaptive column counts based on viewport width
/// - **Social feeds**: Multiple parallel feeds scrolling together
/// - **A/B comparison**: Side-by-side scrollable content comparison
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverCrossAxisGroup core behavior:
/// - Divides cross-axis space among children ✅
/// - Uses maximum scroll extent from all children ✅
/// - Paints children at cross-axis offsets ✅
/// - Sums cache extents from all children ✅
///
/// **Known Limitations:**
/// - ⚠️ No flex factor support (equal distribution only)
/// - ⚠️ Hardcodes vertical scroll assumption in paint
///
/// # Layout Behavior
///
/// | Children | Cross-Axis Distribution | Scroll Extent | Paint Extent |
/// |----------|-------------------------|---------------|--------------|
/// | 0 | N/A | 0.0 | 0.0 |
/// | 1 | 100% | child | child |
/// | 2 | 50% each | max(c1, c2) | max(c1, c2) |
/// | 3 | 33.3% each | max(c1, c2, c3) | max(c1, c2, c3) |
/// | N | 1/N each | max(all) | max(all) |
///
/// # Implementation Status
///
/// **Current Implementation:**
/// - ✅ Equal cross-axis distribution
/// - ✅ Max scroll extent aggregation
/// - ✅ Cross-axis offset tracking
/// - ⚠️ Hardcoded vertical scroll assumption (line 174)
///
/// **Missing from Flutter:**
/// - ❌ Flex factor support for proportional distribution
/// - ❌ Axis direction detection in paint
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverCrossAxisGroup;
///
/// // Two side-by-side lists (50% width each)
/// let group = RenderSliverCrossAxisGroup::new();
/// // Add children: [SliverList, SliverList]
/// // Vertical scroll with horizontal cross-axis arrangement
///
/// // Three-column layout
/// let three_cols = RenderSliverCrossAxisGroup::new();
/// // Add 3 children, each gets 33.3% of cross-axis space
/// ```
#[derive(Debug)]
pub struct RenderSliverCrossAxisGroup {
    /// Computed geometry from last layout
    sliver_geometry: SliverGeometry,

    /// Cross-axis offsets for each child (computed during layout)
    child_cross_axis_offsets: Vec<f32>,

    /// Cross-axis extents for each child
    child_cross_axis_extents: Vec<f32>,
}

impl RenderSliverCrossAxisGroup {
    /// Create new cross axis group
    pub fn new() -> Self {
        Self {
            sliver_geometry: SliverGeometry::default(),
            child_cross_axis_offsets: Vec::new(),
            child_cross_axis_extents: Vec::new(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate aggregate geometry from multiple children
    fn calculate_aggregate_geometry(
        &self,
        constraints: &SliverConstraints,
        max_scroll_extent: f32,
        max_paint_extent: f32,
        max_max_paint_extent: f32,
        total_cache_extent: f32,
        any_visible: bool,
    ) -> SliverGeometry {
        SliverGeometry {
            scroll_extent: max_scroll_extent,
            paint_extent: max_paint_extent.min(constraints.remaining_paint_extent),
            paint_origin: 0.0,
            layout_extent: max_paint_extent.min(constraints.remaining_paint_extent),
            max_paint_extent: max_max_paint_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if any_visible { 1.0 } else { 0.0 },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: total_cache_extent,
            visible: any_visible,
            has_visual_overflow: max_paint_extent > constraints.remaining_paint_extent,
            hit_test_extent: Some(max_paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverCrossAxisGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderSliverCrossAxisGroup {}

impl RenderSliver<Variable> for RenderSliverCrossAxisGroup {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;
        let children: Vec<_> = ctx.children().collect();

        if children.is_empty() {
            self.child_cross_axis_offsets.clear();
            self.child_cross_axis_extents.clear();
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        }

        // For simplicity, divide cross-axis space equally among children
        // A full implementation would support flex factors
        let child_cross_axis_extent = constraints.cross_axis_extent / children.len() as f32;

        self.child_cross_axis_offsets.clear();
        self.child_cross_axis_extents.clear();
        self.child_cross_axis_offsets.reserve(children.len());
        self.child_cross_axis_extents.reserve(children.len());

        let mut max_scroll_extent = 0.0f32;
        let mut max_paint_extent = 0.0f32;
        let mut max_max_paint_extent = 0.0f32;
        let mut total_cache_extent = 0.0;
        let mut any_visible = false;

        // Layout each child with its portion of cross-axis space
        for (i, &child_id) in children.iter().enumerate() {
            let cross_axis_offset = i as f32 * child_cross_axis_extent;

            // Create child constraints with adjusted cross-axis extent
            let child_constraints = SliverConstraints {
                cross_axis_extent: child_cross_axis_extent,
                ..constraints
            };

            // Layout child
            let child_geometry = ctx.tree_mut().perform_sliver_layout(child_id, child_constraints)?;

            // Store cross-axis position and extent
            self.child_cross_axis_offsets.push(cross_axis_offset);
            self.child_cross_axis_extents.push(child_cross_axis_extent);

            // Use maximum extents from all children
            max_scroll_extent = max_scroll_extent.max(child_geometry.scroll_extent);
            max_paint_extent = max_paint_extent.max(child_geometry.paint_extent);
            max_max_paint_extent = max_max_paint_extent.max(child_geometry.max_paint_extent);
            total_cache_extent += child_geometry.cache_extent;

            if child_geometry.visible {
                any_visible = true;
            }
        }

        // Calculate aggregate geometry
        self.sliver_geometry = self.calculate_aggregate_geometry(
            &constraints,
            max_scroll_extent,
            max_paint_extent,
            max_max_paint_extent,
            total_cache_extent,
            any_visible,
        );

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Variable>) {
        let mut canvas = Canvas::new();

        // Paint children with their respective cross-axis offsets
        for (i, child_id) in ctx.children().enumerate() {
            if i < self.child_cross_axis_offsets.len() {
                let cross_axis_offset = self.child_cross_axis_offsets[i];

                // Calculate child's offset based on cross axis
                // For simplicity, assume vertical scrolling (cross axis = horizontal)
                let child_offset = Offset::new(ctx.offset.dx + cross_axis_offset, ctx.offset.dy);

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

    #[test]
    fn test_sliver_cross_axis_group_creation() {
        let group = RenderSliverCrossAxisGroup::new();
        assert_eq!(group.child_cross_axis_offsets.len(), 0);
        assert_eq!(group.child_cross_axis_extents.len(), 0);
    }

    #[test]
    fn test_sliver_cross_axis_group_default() {
        let group = RenderSliverCrossAxisGroup::default();
        assert_eq!(group.child_cross_axis_offsets.len(), 0);
    }

    #[test]
    fn test_geometry_getter() {
        let group = RenderSliverCrossAxisGroup::new();
        let geometry = group.geometry();
        assert_eq!(geometry.scroll_extent, 0.0);
    }
}
