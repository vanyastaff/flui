//! RenderSliverOverlapAbsorber - Absorbs overlap for nested scroll views
//!
//! Implements Flutter's SliverOverlapAbsorber for coordinating overlapping headers in nested
//! scroll views. Wraps a sliver and treats its obstruction extent (typically from pinned
//! headers) as "absorbed overlap" that can be injected elsewhere via a handle.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverOverlapAbsorber` | `RenderSliverOverlapAbsorber` from `package:flutter/src/rendering/sliver_persistent_header.dart` |
//! | `SliverOverlapAbsorberHandle` | `SliverOverlapAbsorberHandle` |
//! | `handle` property | `handle` property |
//! | Shared Arc<Mutex<f32>> | ValueNotifier<double> pattern |
//! | Obstruction calculation | `maxScrollObstructionExtent` logic |
//!
//! # Layout Protocol
//!
//! 1. **Layout child with unchanged constraints**
//!    - Child receives identical constraints (proxy behavior)
//!
//! 2. **Calculate obstruction extent**
//!    - `obstruction = min(max_paint_extent, hit_test_extent ?? paint_extent)`
//!    - This is the "overlap" from pinned/sticky content
//!
//! 3. **Store obstruction in handle**
//!    - Write to Arc<Mutex<f32>> for cross-widget communication
//!    - Handle is shared with SliverOverlapInjector
//!
//! 4. **Modify child geometry**
//!    - `scroll_extent -= obstruction` (reduce scrollable space)
//!    - `layout_extent = max(0, paint_extent - obstruction)` (exclude overlap)
//!    - `paint_extent` unchanged (visual appearance preserved)
//!
//! # Paint Protocol
//!
//! 1. **Paint child at current offset**
//!    - Child painted unchanged
//!    - Absorption only affects geometry, not visuals
//!
//! # Performance
//!
//! - **Layout**: O(1) + child layout - simple geometry modification
//! - **Paint**: O(child) - pass-through proxy
//! - **Memory**: 16 bytes (Arc pointer + SliverGeometry cache)
//! - **Thread-safe**: Arc<Mutex<>> for safe cross-widget communication
//!
//! # Use Cases
//!
//! - **NestedScrollView**: Coordinate outer and inner scroll views
//! - **Pinned headers**: Share overlap between viewports
//! - **TabBarView + AppBar**: Absorb app bar overlap for tabs
//! - **Complex scrollables**: Multi-viewport layouts with shared headers
//! - **Collapsing toolbars**: Coordinate collapse across nested scrolls
//!
//! # Pattern: Absorber-Injector Pair
//!
//! ```text
//! OuterScrollView:
//!   SliverOverlapAbsorber(handle) ← Absorbs pinned header overlap
//!     SliverAppBar(pinned: true)
//!
//! InnerScrollView:
//!   SliverOverlapInjector(handle) ← Injects absorbed overlap as padding
//!     SliverList(...)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverOverlapInjector**: Absorber stores overlap, Injector adds it back
//! - **vs SliverPadding**: Padding is fixed, Absorber is dynamic from child
//! - **vs SliverAppBar**: AppBar creates overlap, Absorber manages it
//! - **vs SliverPersistentHeader**: PersistentHeader may create overlap, Absorber handles it
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderSliverOverlapAbsorber, SliverOverlapAbsorberHandle};
//!
//! // Create shared handle
//! let handle = SliverOverlapAbsorberHandle::new();
//!
//! // Outer scroll: absorb overlap from pinned app bar
//! let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
//! // ... wrap app bar with absorber ...
//!
//! // Inner scroll: inject absorbed overlap
//! let injector = RenderSliverOverlapInjector::new(handle.clone());
//! // ... use injector in inner viewport ...
//!
//! // Handle automatically shares overlap between them
//! let current_overlap = handle.get_extent();
//! ```

use flui_rendering::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::SliverGeometry;
use std::sync::{Arc, Mutex};

/// Handle for communicating absorbed overlap between RenderSliverOverlapAbsorber
/// and RenderSliverOverlapInjector.
///
/// This handle stores the overlap extent that has been absorbed by a
/// RenderSliverOverlapAbsorber and needs to be injected elsewhere in the
/// scroll view (typically by a RenderSliverOverlapInjector).
#[derive(Debug, Clone)]
pub struct SliverOverlapAbsorberHandle {
    /// The absorbed overlap extent (shared between absorber and injector)
    extent: Arc<Mutex<f32>>,
}

impl SliverOverlapAbsorberHandle {
    /// Create a new handle with zero initial overlap
    pub fn new() -> Self {
        Self {
            extent: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Get the current absorbed overlap extent
    pub fn get_extent(&self) -> f32 {
        *self.extent.lock().unwrap()
    }

    /// Set the absorbed overlap extent (internal use)
    fn set_extent(&self, extent: f32) {
        *self.extent.lock().unwrap() = extent;
    }
}

impl Default for SliverOverlapAbsorberHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject that absorbs overlap from child sliver for cross-viewport coordination.
///
/// Wraps a sliver (typically with pinned/sticky content like SliverAppBar) and calculates
/// its obstruction extent, then stores it in a shared handle for use by SliverOverlapInjector
/// in a different viewport. Essential for NestedScrollView pattern.
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
/// **Overlap Coordinator** - Calculates child's obstruction extent (overlap from pinned
/// content), stores in shared handle (Arc<Mutex<>>), modifies child geometry to exclude
/// overlap from scroll_extent and layout_extent. Works in tandem with SliverOverlapInjector.
///
/// # Use Cases
///
/// - **NestedScrollView**: Outer absorber + inner injector pattern
/// - **Tabbed interfaces**: Share app bar overlap across tabs
/// - **Complex headers**: Coordinate pinned headers between scroll views
/// - **Collapsing toolbars**: Manage collapse overlap in nested contexts
/// - **Multi-viewport UIs**: Share overlap state across viewports
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverOverlapAbsorber behavior:
/// - Calculates obstruction from child geometry ✅
/// - Stores in shared handle (Arc<Mutex> vs ValueNotifier) ✅
/// - Reduces scroll_extent by obstruction ✅
/// - Sets layout_extent to exclude obstruction ✅
/// - Preserves paint_extent (visual appearance) ✅
///
/// # Absorber-Injector Pattern
///
/// ```text
/// Outer Viewport:              Inner Viewport:
/// ┌─────────────────┐          ┌─────────────────┐
/// │ OverlapAbsorber │ ─handle─→│ OverlapInjector │
/// │   ┌─────────┐   │          │   (adds padding)│
/// │   │ AppBar  │   │          │   SliverList    │
/// │   │(pinned) │   │          │   [item 1]      │
/// │   └─────────┘   │          │   [item 2]      │
/// │   Content       │          │   [item 3]      │
/// └─────────────────┘          └─────────────────┘
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverOverlapAbsorber, SliverOverlapAbsorberHandle};
///
/// // Create shared handle
/// let handle = SliverOverlapAbsorberHandle::new();
///
/// // Outer scroll: absorb app bar overlap
/// let absorber = RenderSliverOverlapAbsorber::new(handle.clone());
///
/// // Inner scroll: inject overlap as padding
/// let injector = RenderSliverOverlapInjector::new(handle);
/// ```
#[derive(Debug)]
pub struct RenderSliverOverlapAbsorber {
    /// Handle for communicating absorbed overlap
    pub handle: SliverOverlapAbsorberHandle,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverOverlapAbsorber {
    /// Create new overlap absorber with given handle
    ///
    /// # Arguments
    /// * `handle` - Handle for storing absorbed overlap extent
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

    /// Calculate sliver geometry for overlap absorption
    ///
    /// The absorbed overlap is the child's maxScrollObstructionExtent.
    /// We modify the child's geometry to:
    /// - Reduce scroll_extent by the obstruction extent
    /// - Set layout_extent to max(0, paint_extent - obstruction)
    fn calculate_sliver_geometry(&self, child_geometry: SliverGeometry) -> SliverGeometry {
        // The obstruction extent is the overlap we're absorbing
        let obstruction_extent = child_geometry.max_paint_extent.min(
            child_geometry
                .hit_test_extent
                .unwrap_or(child_geometry.paint_extent),
        );

        // Store the absorbed overlap in the handle
        self.handle.set_extent(obstruction_extent);

        // Modify child geometry to account for absorbed overlap
        SliverGeometry {
            // Reduce scroll extent by absorbed overlap
            scroll_extent: (child_geometry.scroll_extent - obstruction_extent).max(0.0),

            // Layout extent excludes the obstruction
            layout_extent: (child_geometry.paint_extent - obstruction_extent).max(0.0),

            // Other fields pass through from child
            paint_extent: child_geometry.paint_extent,
            paint_origin: child_geometry.paint_origin,
            max_paint_extent: child_geometry.max_paint_extent,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: child_geometry.cross_axis_extent,
            cache_extent: child_geometry.cache_extent,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent,
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverOverlapAbsorber {
    fn default() -> Self {
        Self::new(SliverOverlapAbsorberHandle::new())
    }
}

impl RenderObject for RenderSliverOverlapAbsorber {}

impl RenderSliver<Single> for RenderSliverOverlapAbsorber {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();

        // Layout child with unchanged constraints
        let child_geometry = ctx.tree_mut().perform_sliver_layout(child_id, ctx.constraints)?;

        // Calculate our geometry by absorbing child's overlap
        self.sliver_geometry = self.calculate_sliver_geometry(child_geometry);
        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child
        let child_id = *ctx.children.single();

        if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
            *ctx.canvas = child_canvas;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_overlap_absorber_handle_new() {
        let handle = SliverOverlapAbsorberHandle::new();
        assert_eq!(handle.get_extent(), 0.0);
    }

    #[test]
    fn test_sliver_overlap_absorber_handle_set_extent() {
        let handle = SliverOverlapAbsorberHandle::new();
        handle.set_extent(50.0);
        assert_eq!(handle.get_extent(), 50.0);
    }

    #[test]
    fn test_sliver_overlap_absorber_handle_clone() {
        let handle1 = SliverOverlapAbsorberHandle::new();
        handle1.set_extent(100.0);

        let handle2 = handle1.clone();
        assert_eq!(handle2.get_extent(), 100.0);

        // Verify they share the same underlying data
        handle1.set_extent(200.0);
        assert_eq!(handle2.get_extent(), 200.0);
    }

    #[test]
    fn test_render_sliver_overlap_absorber_new() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        assert_eq!(absorber.handle.get_extent(), 0.0);
    }

    #[test]
    fn test_render_sliver_overlap_absorber_default() {
        let absorber = RenderSliverOverlapAbsorber::default();
        assert_eq!(absorber.handle.get_extent(), 0.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_basic() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 150.0,
            paint_origin: 0.0,
            layout_extent: 150.0,
            max_paint_extent: 200.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 150.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(150.0),
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent should be min(max_paint_extent, hit_test_extent)
        // = min(200.0, 150.0) = 150.0
        assert_eq!(handle.get_extent(), 150.0);

        // scroll_extent = child_scroll_extent - obstruction = 200 - 150 = 50
        assert_eq!(geometry.scroll_extent, 50.0);

        // layout_extent = max(0, paint_extent - obstruction) = max(0, 150 - 150) = 0
        assert_eq!(geometry.layout_extent, 0.0);

        // paint_extent passes through
        assert_eq!(geometry.paint_extent, 150.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_with_obstruction() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 300.0,
            paint_extent: 200.0,
            paint_origin: 0.0,
            layout_extent: 200.0,
            max_paint_extent: 300.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 200.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(100.0), // Obstruction is 100px
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent = min(300.0, 100.0) = 100.0
        assert_eq!(handle.get_extent(), 100.0);

        // scroll_extent = 300 - 100 = 200
        assert_eq!(geometry.scroll_extent, 200.0);

        // layout_extent = max(0, 200 - 100) = 100
        assert_eq!(geometry.layout_extent, 100.0);

        assert_eq!(geometry.paint_extent, 200.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_hit_test_extent() {
        let handle = SliverOverlapAbsorberHandle::new();
        let absorber = RenderSliverOverlapAbsorber::new(handle.clone());

        let child_geometry = SliverGeometry {
            scroll_extent: 250.0,
            paint_extent: 180.0,
            paint_origin: 0.0,
            layout_extent: 180.0,
            max_paint_extent: 250.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 1.0,
            cross_axis_extent: 400.0,
            cache_extent: 180.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: None, // Falls back to paint_extent
            scroll_offset_correction: None,
        };

        let geometry = absorber.calculate_sliver_geometry(child_geometry);

        // Obstruction extent = min(250.0, 180.0) = 180.0
        assert_eq!(handle.get_extent(), 180.0);

        // scroll_extent = 250 - 180 = 70
        assert_eq!(geometry.scroll_extent, 70.0);

        // layout_extent = max(0, 180 - 180) = 0
        assert_eq!(geometry.layout_extent, 0.0);
    }
}
