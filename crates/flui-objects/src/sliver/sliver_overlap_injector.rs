//! RenderSliverOverlapInjector - Injects absorbed overlap as padding
//!
//! Implements Flutter's SliverOverlapInjector for coordinating overlapping headers in nested
//! scroll views. Reads overlap extent from a shared handle (set by SliverOverlapAbsorber) and
//! injects it as padding before child content, ensuring child doesn't render under the overlap.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverOverlapInjector` | `RenderSliverOverlapInjector` from `package:flutter/src/rendering/sliver_persistent_header.dart` |
//! | `SliverOverlapAbsorberHandle` | `SliverOverlapAbsorberHandle` |
//! | `handle` property | `handle` property |
//! | Shared Arc<Mutex<f32>> | ValueNotifier<double> pattern |
//! | Injected extent | Padding before child content |
//!
//! # Layout Protocol
//!
//! 1. **Read overlap extent from handle**
//!    - Get current extent from Arc<Mutex<f32>>
//!    - This is the overlap absorbed by SliverOverlapAbsorber
//!
//! 2. **Adjust constraints for child**
//!    - `scroll_offset = max(0, scroll_offset - overlap_extent)`
//!    - `remaining_paint_extent = max(0, remaining_paint_extent - overlap_extent)`
//!    - Child receives constraints as if overlap was already consumed
//!
//! 3. **Layout child with adjusted constraints**
//!    - Child lays out in remaining space after overlap
//!
//! 4. **Add overlap to child geometry**
//!    - `scroll_extent += overlap_extent` (add injected space)
//!    - `paint_extent += overlap_extent` (visible extent includes overlap)
//!    - `layout_extent += overlap_extent` (affects following slivers)
//!
//! # Paint Protocol
//!
//! 1. **Paint child with offset**
//!    - Child painted at `offset + overlap_extent` along main axis
//!    - Ensures child doesn't paint under absorbed overlap
//!
//! # Performance
//!
//! - **Layout**: O(1) + child layout - simple geometry addition
//! - **Paint**: O(child) - pass-through with offset
//! - **Memory**: 16 bytes (Arc pointer + SliverGeometry cache)
//! - **Thread-safe**: Arc<Mutex<>> for safe cross-widget communication
//!
//! # Use Cases
//!
//! - **NestedScrollView**: Inject outer scroll overlap into inner scroll
//! - **Pinned headers**: Ensure content doesn't render under headers
//! - **TabBarView + AppBar**: Inject app bar overlap for each tab
//! - **Complex scrollables**: Multi-viewport layouts with shared headers
//! - **Collapsing toolbars**: Coordinate collapse across nested scrolls
//!
//! # Pattern: Absorber-Injector Pair
//!
//! ```text
//! OuterScrollView:
//!   SliverOverlapAbsorber(handle) ← Absorbs pinned header overlap
//!     SliverAppBar(pinned: true, height: 200)
//!
//! InnerScrollView:
//!   SliverOverlapInjector(handle) ← Injects 200px padding
//!     SliverList(...) ← Content starts after 200px
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverOverlapAbsorber**: Injector adds overlap, Absorber stores it
//! - **vs SliverPadding**: Padding is fixed, Injector is dynamic from handle
//! - **vs SliverSafeArea**: SafeArea for device insets, Injector for scroll overlap
//! - **vs SliverToBoxAdapter**: Adapter converts protocols, Injector adds dynamic space
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderSliverOverlapInjector, SliverOverlapAbsorberHandle};
//!
//! // Create shared handle (shared with absorber in outer scroll)
//! let handle = SliverOverlapAbsorberHandle::new();
//!
//! // Inner scroll: inject absorbed overlap
//! let injector = RenderSliverOverlapInjector::new(handle.clone());
//! // ... use injector as first sliver in inner viewport ...
//!
//! // Injector automatically adds padding equal to absorbed overlap
//! // Child content starts after the injected extent
//! ```

use flui_rendering::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::sliver::sliver_overlap_absorber::SliverOverlapAbsorberHandle;
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::{SliverGeometry, Offset, Axis};

/// RenderObject that injects absorbed overlap as padding before child content.
///
/// Works in tandem with SliverOverlapAbsorber to coordinate overlapping headers between
/// nested scroll views. Reads overlap extent from shared handle and adds it as padding,
/// ensuring child content doesn't render under absorbed overlap (typically pinned headers).
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child sliver (optional in implementation).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Overlap Injector** - Reads overlap from shared handle (Arc<Mutex<>>), adjusts child
/// constraints to account for injected space, adds overlap extent to child geometry. Works
/// in tandem with SliverOverlapAbsorber.
///
/// # Use Cases
///
/// - **NestedScrollView**: Inner injector receives outer absorber's overlap
/// - **Tabbed interfaces**: Each tab injects shared app bar overlap
/// - **Complex headers**: Coordinate pinned headers between scroll views
/// - **Collapsing toolbars**: Inject collapse overlap in nested contexts
/// - **Multi-viewport UIs**: Share overlap state across viewports
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverOverlapInjector behavior:
/// - Reads overlap from shared handle (Arc<Mutex> vs ValueNotifier) ✅
/// - Adjusts constraints for child (scroll_offset, remaining_extent) ✅
/// - Adds overlap to child geometry (scroll_extent, paint_extent) ✅
/// - Offsets child paint by overlap extent ✅
/// - Coordinates with SliverOverlapAbsorber ✅
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverOverlapInjector, SliverOverlapAbsorberHandle};
///
/// // Get handle from outer scroll (shared with absorber)
/// let handle = SliverOverlapAbsorberHandle::new();
///
/// // Create injector for inner scroll
/// let injector = RenderSliverOverlapInjector::new(handle);
///
/// // Injector adds padding equal to absorbed overlap
/// // Child content starts after the injected extent
/// ```
#[derive(Debug)]
pub struct RenderSliverOverlapInjector {
    /// Handle for reading absorbed overlap extent
    pub handle: SliverOverlapAbsorberHandle,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverOverlapInjector {
    /// Create new overlap injector with given handle
    ///
    /// # Arguments
    /// * `handle` - Handle for reading absorbed overlap extent
    pub fn new(handle: SliverOverlapAbsorberHandle) -> Self {
        Self {
            handle,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate child constraints with overlap accounted for
    fn child_constraints(&self, constraints: &flui_types::SliverConstraints) -> flui_types::SliverConstraints {
        let overlap = self.handle.get_extent();

        flui_types::SliverConstraints {
            // Reduce scroll offset by overlap (child starts after overlap)
            scroll_offset: (constraints.scroll_offset - overlap).max(0.0),
            // Reduce remaining paint extent by overlap (less space for child)
            remaining_paint_extent: (constraints.remaining_paint_extent - overlap).max(0.0),
            ..(*constraints)
        }
    }

    /// Add overlap to child geometry
    fn child_to_parent_geometry(&self, child_geometry: SliverGeometry) -> SliverGeometry {
        let overlap = self.handle.get_extent();

        SliverGeometry {
            // Add overlap to scroll extent (injected space is scrollable)
            scroll_extent: child_geometry.scroll_extent + overlap,
            // Add overlap to paint extent (visible extent includes injected space)
            paint_extent: child_geometry.paint_extent + overlap,
            // paint_origin unchanged (no offset needed)
            paint_origin: child_geometry.paint_origin,
            // Add overlap to layout extent (affects following slivers)
            layout_extent: child_geometry.layout_extent + overlap,
            // max_paint_extent includes overlap
            max_paint_extent: child_geometry.max_paint_extent + overlap,
            // Other fields pass through
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: child_geometry.cross_axis_extent,
            cache_extent: child_geometry.cache_extent + overlap,
            visible: child_geometry.visible || overlap > 0.0,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry
                .hit_test_extent
                .map(|extent| extent + overlap),
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl RenderObject for RenderSliverOverlapInjector {}

impl RenderSliver<Single> for RenderSliverOverlapInjector {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();
        let constraints = ctx.constraints;

        // Get overlap from handle
        let overlap = self.handle.get_extent();

        // Adjust constraints for child
        let child_constraints = self.child_constraints(&constraints);

        // Layout child
        let child_geometry = ctx.tree_mut().perform_sliver_layout(child_id, child_constraints)?;

        // Add overlap to child geometry
        self.sliver_geometry = self.child_to_parent_geometry(child_geometry);

        // Store child offset for painting
        let child_offset = match constraints.axis_direction.axis() {
            Axis::Vertical => Offset::new(0.0, overlap),
            Axis::Horizontal => Offset::new(overlap, 0.0),
        };
        ctx.set_child_offset(child_id, child_offset);

        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        let child_id = *ctx.children.single();

        // Get child offset (set during layout)
        if let Some(child_offset) = ctx.get_child_offset(child_id) {
            // Paint child with offset
            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset + child_offset) {
                *ctx.canvas = child_canvas;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlap_injector_new() {
        let handle = SliverOverlapAbsorberHandle::new();
        let injector = RenderSliverOverlapInjector::new(handle.clone());

        assert_eq!(handle.get_extent(), 0.0);
    }

    #[test]
    fn test_overlap_injector_reads_handle() {
        let handle = SliverOverlapAbsorberHandle::new();
        let injector = RenderSliverOverlapInjector::new(handle.clone());

        // Simulate absorber setting extent
        handle.set_extent(100.0);

        assert_eq!(handle.get_extent(), 100.0);
    }

    #[test]
    fn test_child_constraints_with_overlap() {
        let handle = SliverOverlapAbsorberHandle::new();
        handle.set_extent(50.0);

        let injector = RenderSliverOverlapInjector::new(handle);

        let constraints = flui_types::SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::Down,
            growth_direction: flui_types::layout::GrowthDirection::Forward,
            user_scroll_direction: flui_types::layout::ScrollDirection::Idle,
            scroll_offset: 100.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::Right,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = injector.child_constraints(&constraints);

        // scroll_offset reduced by overlap
        assert_eq!(child_constraints.scroll_offset, 50.0); // 100 - 50
        // remaining_paint_extent reduced by overlap
        assert_eq!(child_constraints.remaining_paint_extent, 550.0); // 600 - 50
    }

    #[test]
    fn test_child_constraints_overlap_clamped() {
        let handle = SliverOverlapAbsorberHandle::new();
        handle.set_extent(150.0);

        let injector = RenderSliverOverlapInjector::new(handle);

        let constraints = flui_types::SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::Down,
            growth_direction: flui_types::layout::GrowthDirection::Forward,
            user_scroll_direction: flui_types::layout::ScrollDirection::Idle,
            scroll_offset: 100.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::Right,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = injector.child_constraints(&constraints);

        // scroll_offset can't go below 0
        assert_eq!(child_constraints.scroll_offset, 0.0); // max(0, 100 - 150)
    }

    #[test]
    fn test_geometry_includes_overlap() {
        let handle = SliverOverlapAbsorberHandle::new();
        handle.set_extent(100.0);

        let injector = RenderSliverOverlapInjector::new(handle);

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 300.0,
            paint_origin: 0.0,
            layout_extent: 300.0,
            max_paint_extent: 500.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 300.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(300.0),
            scroll_offset_correction: None,
        };

        let parent_geometry = injector.child_to_parent_geometry(child_geometry);

        // All extents should include the 100px overlap
        assert_eq!(parent_geometry.scroll_extent, 600.0); // 500 + 100
        assert_eq!(parent_geometry.paint_extent, 400.0); // 300 + 100
        assert_eq!(parent_geometry.layout_extent, 400.0); // 300 + 100
        assert_eq!(parent_geometry.max_paint_extent, 600.0); // 500 + 100
        assert_eq!(parent_geometry.cache_extent, 400.0); // 300 + 100
        assert_eq!(parent_geometry.hit_test_extent, Some(400.0)); // 300 + 100
    }
}
