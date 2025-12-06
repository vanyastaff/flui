//! RenderSliverGrid - Lazy-loading scrollable grid with viewport culling
//!
//! Implements Flutter's sliver grid protocol for efficient 2D grid layouts in scrollable
//! viewports. Uses lazy child building and viewport culling to handle large grids (1000s
//! of items) efficiently. Combines row/column layout with sliver scroll-awareness for
//! photo grids, product catalogs, and tile-based interfaces.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverGrid` | `RenderSliverGrid` from `package:flutter/src/rendering/sliver_grid.dart` |
//! | `SliverGridDelegate` | `SliverGridDelegate` trait |
//! | `SliverGridDelegateFixedCrossAxisCount` | `SliverGridDelegateWithFixedCrossAxisCount` |
//! | `get_column_count()` | `getLayout().crossAxisCount` |
//! | `get_main_axis_extent()` | Tile extent calculation |
//! | `get_spacing()` | `mainAxisSpacing`, `crossAxisSpacing` |
//!
//! # Layout Protocol
//!
//! 1. **Calculate grid dimensions**: Delegate determines columns, item size, spacing
//! 2. **Calculate visible range**: Which rows/cells are in viewport + cache
//! 3. **Lazy child building**: Build only visible + cached cells via delegate
//! 4. **Grid layout**: Position children in grid with spacing
//! 5. **SliverGeometry**: Return scroll extent, paint extent, visible status
//!
//! # Paint Protocol
//!
//! - **Viewport culling**: Only paint visible cells (rows in viewport)
//! - **Grid positioning**: Paint cells at grid coordinates with spacing
//!
//! # Performance
//!
//! - **Layout**: O(v) where v = visible cells - only layouts visible items
//! - **Paint**: O(v) - only paints visible cells
//! - **Memory**: O(v + c) where c = cached cells
//!
//! # Use Cases
//!
//! - **Photo grids**: Instagram-style photo galleries
//! - **Product catalogs**: E-commerce product grids
//! - **App launchers**: Icon grids with lazy loading
//! - **Tile interfaces**: Dashboard tiles, menu grids
//! - **Image galleries**: Large image collections
//! - **Calendar grids**: Month/year calendar views
//!
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Children are NEVER laid out** (line 313-323)
//!    - No calls to `layout_child()` anywhere
//!    - Child sizes are undefined
//!    - Only geometry calculation, no actual grid layout
//!
//! 2. **❌ Paint not implemented** (line 325-335)
//!    - Returns empty canvas
//!    - TODO comment: "Implement actual child painting with grid layout"
//!    - Children are never painted in grid positions
//!
//! 3. **✅ Geometry calculation CORRECT**
//!    - Row/column math is sound
//!    - Spacing correctly applied
//!    - Scroll extent accurately computed
//!
//! 4. **✅ Delegate pattern well implemented**
//!    - SliverGridDelegate trait is clean
//!    - FixedCrossAxisCount implementation works
//!    - Extensible for other grid types
//!
//! **This RenderObject is BROKEN - geometry only, no layout or paint of grid cells!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverList**: Grid is 2D layout, List is 1D (single column)
//! - **vs SliverFixedExtentList**: Grid has columns, FixedExtent is uniform 1D
//! - **vs SliverMultiBoxAdaptor**: Grid is specialized, MultiBoxAdaptor is generic
//! - **vs GridView (widget)**: GridView uses SliverGrid internally
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderSliverGrid, SliverGridDelegateFixedCrossAxisCount};
//!
//! // 3-column photo grid with 100px tall items
//! let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
//!     .with_main_axis_spacing(8.0)
//!     .with_cross_axis_spacing(8.0));
//! let grid = RenderSliverGrid::new(delegate);
//! // Note: Won't render due to missing layout/paint!
//!
//! // Product catalog grid (4 columns, no spacing)
//! let catalog_delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(4, 150.0));
//! let catalog = RenderSliverGrid::new(catalog_delegate);
//! ```

use crate::core::{RenderObject, RenderSliver, Variable, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry, BoxConstraints, Offset, Axis};

/// Grid delegate for calculating grid layout
///
/// Determines how many columns, row heights, spacing, etc.
pub trait SliverGridDelegate: std::fmt::Debug + Send + Sync {
    /// Get the number of columns
    fn get_column_count(&self, cross_axis_extent: f32) -> usize;

    /// Get the main axis extent (height for vertical) for a child at index
    fn get_main_axis_extent(&self, index: usize, cross_axis_extent: f32) -> f32;

    /// Get spacing between items
    fn get_spacing(&self) -> (f32, f32); // (main_axis_spacing, cross_axis_spacing)

    /// Check if layout should be recalculated
    fn should_relayout(&self, old: &dyn std::any::Any) -> bool;

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Fixed column count grid delegate
///
/// Creates a grid with a fixed number of columns and equal-height rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverGridDelegateFixedCrossAxisCount {
    /// Number of columns
    pub cross_axis_count: usize,
    /// Height of each item (in main axis)
    pub main_axis_extent: f32,
    /// Spacing between items on main axis
    pub main_axis_spacing: f32,
    /// Spacing between items on cross axis
    pub cross_axis_spacing: f32,
}

impl SliverGridDelegateFixedCrossAxisCount {
    /// Create new fixed column count delegate
    pub fn new(cross_axis_count: usize, main_axis_extent: f32) -> Self {
        Self {
            cross_axis_count,
            main_axis_extent,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
        }
    }

    /// Set main axis spacing
    pub fn with_main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Set cross axis spacing
    pub fn with_cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateFixedCrossAxisCount {
    fn get_column_count(&self, _cross_axis_extent: f32) -> usize {
        self.cross_axis_count
    }

    fn get_main_axis_extent(&self, _index: usize, _cross_axis_extent: f32) -> f32 {
        self.main_axis_extent
    }

    fn get_spacing(&self) -> (f32, f32) {
        (self.main_axis_spacing, self.cross_axis_spacing)
    }

    fn should_relayout(&self, old: &dyn std::any::Any) -> bool {
        if let Some(old) = old.downcast_ref::<Self>() {
            self != old
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// RenderObject for lazy-loading scrollable grids with viewport culling.
///
/// 2D grid layout with sliver scroll-awareness. Uses delegate pattern for grid
/// configuration (columns, item size, spacing). Only builds and layouts cells
/// that are visible or in cache extent, enabling efficient scrolling through
/// thousands of grid items.
///
/// # Arity
///
/// `RuntimeArity` (Variable) - Variable number of children, but only visible +
/// cached cells are built and laid out.
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Lazy Loading 2D Grid Viewport** - Delegate-based grid configuration, viewport
/// culling (only visible rows/cells painted), lazy child building, fixed or variable
/// cell sizing, scroll-aware layout with SliverGeometry.
///
/// # Use Cases
///
/// - **Photo grids**: Instagram/Pinterest style image grids
/// - **Product catalogs**: E-commerce grids with lazy loading
/// - **App launchers**: Icon grids with many apps
/// - **Tile dashboards**: Dashboard with tile-based layout
/// - **Image galleries**: Large collections with efficient scrolling
/// - **Calendar grids**: Month views with many cells
///
/// # Flutter Compliance
///
/// **PARTIALLY IMPLEMENTED**:
/// - ✅ Delegate pattern (SliverGridDelegate trait)
/// - ✅ Geometry calculation with rows, spacing
/// - ✅ Spacing support (main and cross axis)
/// - ❌ Child layout not implemented
/// - ❌ Paint not implemented
/// - ❌ Lazy building not implemented (would need child layout)
/// - ❌ Viewport culling not implemented (would need paint)
///
/// # Implementation Status
///
/// | Feature | Status | Notes |
/// |---------|--------|-------|
/// | Delegate pattern | ✅ Complete | SliverGridDelegate trait works |
/// | Geometry calculation | ✅ Complete | Row math, spacing correct |
/// | Fixed column count | ✅ Complete | FixedCrossAxisCount delegate |
/// | Child layout | ❌ Missing | No layout_child() calls |
/// | Grid positioning | ❌ Missing | Would need child layout |
/// | Child paint | ❌ Missing | Empty canvas returned |
/// | Viewport culling | ❌ Missing | Would need paint implementation |
/// | Lazy building | ❌ Missing | All children created, not lazy |
///
/// **Critical Missing:** Child layout and paint - the core rendering functionality!
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverGrid, SliverGridDelegateFixedCrossAxisCount};
///
/// // 3-column grid, 100px tall items, 10px spacing
/// let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
///     .with_main_axis_spacing(10.0)
///     .with_cross_axis_spacing(10.0);
/// let grid = RenderSliverGrid::new(Box::new(delegate));
/// // WARNING: Geometry correct but children won't render!
///
/// // Photo grid with uniform cells
/// let photo_grid_delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 120.0);
/// let photo_grid = RenderSliverGrid::new(Box::new(photo_grid_delegate));
/// // WARNING: Has bugs - no child layout or paint!
/// ```
pub struct RenderSliverGrid {
    /// Grid layout delegate
    #[allow(clippy::type_complexity)]
    pub delegate: Box<dyn SliverGridDelegate>,
    /// Cross axis extent (width for vertical scroll)
    pub cross_axis_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl std::fmt::Debug for RenderSliverGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverGrid")
            .field("delegate", &self.delegate)
            .field("cross_axis_extent", &self.cross_axis_extent)
            .field("sliver_geometry", &self.sliver_geometry)
            .finish()
    }
}

impl RenderSliverGrid {
    /// Create new sliver grid with delegate
    pub fn new(delegate: Box<dyn SliverGridDelegate>) -> Self {
        Self {
            delegate,
            cross_axis_extent: 0.0,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set cross axis extent
    pub fn set_cross_axis_extent(&mut self, extent: f32) {
        self.cross_axis_extent = extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

}

impl RenderObject for RenderSliverGrid {}

impl RenderSliver<Variable> for RenderSliverGrid {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Variable>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;
        let children: Vec<_> = ctx.children().collect();

        // Store cross axis extent
        self.cross_axis_extent = constraints.cross_axis_extent;

        // If no children, return zero geometry
        if children.is_empty() {
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;
        let cross_axis_extent = constraints.cross_axis_extent;

        // Get grid parameters from delegate
        let column_count = self.delegate.get_column_count(cross_axis_extent);
        if column_count == 0 {
            self.sliver_geometry = SliverGeometry::default();
            return Ok(self.sliver_geometry);
        }

        let (main_spacing, cross_spacing) = self.delegate.get_spacing();

        // Calculate grid dimensions
        let child_count = children.len();
        let row_count = (child_count + column_count - 1) / column_count; // Ceiling division
        let row_height = self.delegate.get_main_axis_extent(0, cross_axis_extent);

        // Calculate column width
        let total_cross_spacing = if column_count > 1 {
            cross_spacing * (column_count - 1) as f32
        } else {
            0.0
        };
        let column_width = (cross_axis_extent - total_cross_spacing) / column_count as f32;

        // Calculate total extent
        let total_row_spacing = if row_count > 1 {
            main_spacing * (row_count - 1) as f32
        } else {
            0.0
        };
        let total_extent = row_height * row_count as f32 + total_row_spacing;

        // Calculate visible row range (viewport culling)
        let first_visible_row = (scroll_offset / (row_height + main_spacing)).floor() as usize;
        let last_visible_row = ((scroll_offset + remaining_extent) / (row_height + main_spacing)).ceil() as usize;
        let last_visible_row = last_visible_row.min(row_count);

        // Layout visible children in grid
        let box_constraints = BoxConstraints::new(
            0.0,
            column_width,
            row_height,
            row_height,
        );

        for (i, &child_id) in children.iter().enumerate() {
            let row = i / column_count;
            let col = i % column_count;

            // Only layout visible children (viewport culling)
            if row >= first_visible_row && row < last_visible_row {
                ctx.tree_mut().perform_layout(child_id, box_constraints)?;

                // Calculate child position
                let x = col as f32 * (column_width + cross_spacing);
                let y = row as f32 * (row_height + main_spacing) - scroll_offset;

                let child_offset = match constraints.axis_direction.axis() {
                    Axis::Vertical => Offset::new(x, y),
                    Axis::Horizontal => Offset::new(y, x),
                };

                ctx.set_child_offset(child_id, child_offset);
            }
        }

        // Calculate what's visible
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
        if !self.sliver_geometry.visible {
            return;
        }

        let children: Vec<_> = ctx.children().collect();

        // Paint visible children
        for &child_id in &children {
            if let Some(child_offset) = ctx.get_child_offset(child_id) {
                if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset + child_offset) {
                    ctx.canvas.append_canvas(child_canvas);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_grid_delegate_fixed_new() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);

        assert_eq!(delegate.cross_axis_count, 3);
        assert_eq!(delegate.main_axis_extent, 100.0);
        assert_eq!(delegate.main_axis_spacing, 0.0);
        assert_eq!(delegate.cross_axis_spacing, 0.0);
    }

    #[test]
    fn test_sliver_grid_delegate_fixed_with_spacing() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
            .with_main_axis_spacing(10.0)
            .with_cross_axis_spacing(5.0);

        assert_eq!(delegate.main_axis_spacing, 10.0);
        assert_eq!(delegate.cross_axis_spacing, 5.0);
    }

    #[test]
    fn test_sliver_grid_delegate_get_column_count() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(4, 100.0);

        assert_eq!(delegate.get_column_count(400.0), 4);
    }

    #[test]
    fn test_sliver_grid_delegate_get_main_axis_extent() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 120.0);

        assert_eq!(delegate.get_main_axis_extent(0, 400.0), 120.0);
        assert_eq!(delegate.get_main_axis_extent(5, 400.0), 120.0); // Same for all
    }

    #[test]
    fn test_sliver_grid_delegate_get_spacing() {
        let delegate = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0)
            .with_main_axis_spacing(15.0)
            .with_cross_axis_spacing(10.0);

        let (main, cross) = delegate.get_spacing();
        assert_eq!(main, 15.0);
        assert_eq!(cross, 10.0);
    }

    #[test]
    fn test_sliver_grid_delegate_should_relayout() {
        let delegate1 = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);
        let delegate2 = SliverGridDelegateFixedCrossAxisCount::new(3, 100.0);
        let delegate3 = SliverGridDelegateFixedCrossAxisCount::new(4, 100.0);

        assert!(!delegate1.should_relayout(&delegate2 as &dyn std::any::Any));
        assert!(delegate1.should_relayout(&delegate3 as &dyn std::any::Any));
    }

    #[test]
    fn test_render_sliver_grid_new() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        assert_eq!(grid.cross_axis_extent, 0.0);
    }

    #[test]
    fn test_render_sliver_grid_set_cross_axis_extent() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let mut grid = RenderSliverGrid::new(delegate);

        grid.set_cross_axis_extent(400.0);
        assert_eq!(grid.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_empty() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();
        let children = vec![];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_render_sliver_grid_geometry_single_row() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 3 children = 1 row
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 1 row * 100px = 100px
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_multiple_rows() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 9 children = 3 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
            flui_core::element::ElementId::new(5),
            flui_core::element::ElementId::new(6),
            flui_core::element::ElementId::new(7),
            flui_core::element::ElementId::new(8),
            flui_core::element::ElementId::new(9),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 3 rows * 100px = 300px
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_render_sliver_grid_geometry_with_spacing() {
        let delegate = Box::new(
            SliverGridDelegateFixedCrossAxisCount::new(2, 100.0).with_main_axis_spacing(10.0),
        );
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 4 children = 2 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 rows * 100px + 1 spacing * 10px = 210px
        assert_eq!(geometry.scroll_extent, 210.0);
        assert_eq!(geometry.paint_extent, 210.0);
    }

    #[test]
    fn test_render_sliver_grid_geometry_scrolled() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(2, 100.0));
        let grid = RenderSliverGrid::new(delegate);
        let tree = ElementTree::new();

        // 10 children = 5 rows
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
            flui_core::element::ElementId::new(4),
            flui_core::element::ElementId::new(5),
            flui_core::element::ElementId::new(6),
            flui_core::element::ElementId::new(7),
            flui_core::element::ElementId::new(8),
            flui_core::element::ElementId::new(9),
            flui_core::element::ElementId::new(10),
        ];

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 150.0, // Scrolled past 1.5 rows
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = grid.calculate_sliver_geometry(&constraints, &tree, &children);

        // 5 rows * 100px = 500px total
        assert_eq!(geometry.scroll_extent, 500.0);
        // Visible: from 150 to 450 = 300px
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.6); // 300/500
    }

    #[test]
    fn test_arity_is_variable() {
        let delegate = Box::new(SliverGridDelegateFixedCrossAxisCount::new(3, 100.0));
        let grid = RenderSliverGrid::new(delegate);

        assert_eq!(grid.arity(), RuntimeArity::Variable);
    }
}
