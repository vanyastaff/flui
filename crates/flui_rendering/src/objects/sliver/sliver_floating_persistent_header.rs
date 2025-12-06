//! RenderSliverFloatingPersistentHeader - Scroll-direction-aware floating header
//!
//! Implements Flutter's floating persistent header pattern where the header appears immediately
//! when scrolling in reverse direction (scroll up), even if content hasn't scrolled far enough
//! to naturally reveal it. Unlike pinned headers that stick, floating headers can fully scroll
//! offscreen when scrolling forward.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverFloatingPersistentHeader` | `RenderSliverFloatingPersistentHeader` from `package:flutter/src/rendering/sliver_persistent_header.dart` |
//! | `extent` property | Fixed extent for header height |
//! | `snap` property | `snapConfiguration` for full/hidden only |
//! | Floating logic | Scroll direction tracking + immediate appearance |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Track scroll direction** (NOT IMPLEMENTED)
//!    - Monitor scroll_offset changes to detect up/down
//!    - Store previous scroll offset for comparison
//!
//! 2. **Calculate visibility based on direction**
//!    - **Scroll down**: Header shrinks normally, can scroll offscreen
//!    - **Scroll up**: Header appears immediately at full extent
//!    - **Snap mode**: Transition only between 0 and full extent
//!
//! 3. **Layout child with calculated extent**
//!    - Create BoxConstraints based on current visibility
//!    - Layout child with header content
//!
//! 4. **Return geometry**
//!    - scroll_extent: Fixed header extent
//!    - paint_extent: Current visible extent
//!    - layout_extent: Affects following slivers
//!
//! # Paint Protocol
//!
//! 1. **Check visibility**
//!    - Only paint if geometry.visible
//!
//! 2. **Paint child**
//!    - Paint header content at current offset
//!
//! # Performance
//!
//! - **Layout**: O(1) geometry + O(child) when implemented
//! - **Paint**: O(child) when visible
//! - **Memory**: 8 bytes (f32 + bool) + 48 bytes (SliverGeometry) = 56 bytes
//! - **Scroll tracking**: Requires storing previous offset (when implemented)
//!
//! # Use Cases
//!
//! - **App bars**: Material Design floating app bars
//! - **Search bars**: Hide on scroll down, appear on scroll up
//! - **Toolbars**: Contextual toolbars that respond to scroll
//! - **Navigation**: Quick-access navigation that hides/shows
//! - **Action bars**: Floating action context on scroll
//!
//! # Floating vs Pinned Behavior
//!
//! ```text
//! PINNED HEADER:
//! scroll=0:    [████████] (full)
//! scroll=50:   [███] (shrunk to minimum)
//! scroll=100+: [███] (STAYS at minimum)
//!
//! FLOATING HEADER (intended):
//! scroll=0:    [████████] (full)
//! scroll down: [        ] (hides completely)
//! scroll UP:   [████████] (APPEARS immediately!)
//!
//! CURRENT (simplified):
//! scroll=0:    [████████] (full)
//! scroll=50:   [████] (shrinks)
//! scroll=80+:  [        ] (hidden - no floating back)
//! ```
//!
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Child is NEVER laid out** (line 124-128)
//!    - No calls to `layout_child()` anywhere
//!    - Child size is undefined
//!    - Only geometry calculation, no actual layout
//!
//! 2. **❌ No scroll direction tracking** (line 79 comment)
//!    - Current implementation doesn't track scroll direction
//!    - Cannot detect "scroll up" to trigger floating behavior
//!    - Comment admits: "Since we don't have scroll direction here"
//!    - Floating behavior is thus IMPOSSIBLE without direction tracking
//!
//! 3. **❌ snap flag NOT USED** (line 35, 49, 55, 60)
//!    - Flag exists and can be set
//!    - Never checked in `calculate_sliver_geometry()`
//!    - Dead code - has no effect on behavior
//!
//! 4. **⚠️ Simplified geometry** (line 74-114)
//!    - Behaves like non-floating header (just shrinks with scroll)
//!    - Doesn't implement actual floating behavior
//!    - Missing the key "appear on scroll up" feature
//!
//! **This RenderObject is BROKEN - missing child layout, no floating behavior, unused snap flag!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPersistentHeader**: Floating is specialized, Persistent is generic
//! - **vs SliverPinnedPersistentHeader**: Pinned stays visible, Floating can hide
//! - **vs SliverAppBar**: AppBar is configurable, FloatingHeader is always floating (intended)
//! - **vs BoxAppBar**: FloatingHeader is sliver protocol, BoxAppBar is static box
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverFloatingPersistentHeader;
//!
//! // Basic floating header (Material Design app bar height)
//! let floating = RenderSliverFloatingPersistentHeader::new(56.0);
//! // Note: Won't actually float without direction tracking!
//!
//! // With snap behavior (full or hidden only)
//! let snap = RenderSliverFloatingPersistentHeader::new(80.0)
//!     .with_snap();
//! // Note: snap flag currently has no effect!
//!
//! // Custom height
//! let custom = RenderSliverFloatingPersistentHeader::new(120.0);
//! ```

use crate::core::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a scroll-direction-aware floating header.
///
/// Intended to implement floating behavior where the header appears immediately when
/// scrolling in reverse direction (up), even if content hasn't scrolled far enough
/// to naturally reveal it. The header can scroll completely offscreen when scrolling
/// forward (down), but "floats" back into view on any upward scroll.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child (header content).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Scroll-Responsive Floating Header** - Intended to track scroll direction
/// and adjust visibility accordingly. On scroll down: hides. On scroll up: appears
/// immediately at full extent. Currently simplified to just shrink/hide behavior.
///
/// # Use Cases
///
/// - **Material app bars**: Standard Material Design floating behavior
/// - **Search bars**: Hide when browsing, appear when searching
/// - **Contextual toolbars**: Show on scroll up for quick actions
/// - **Navigation bars**: Hide to maximize content, reveal on demand
/// - **Action headers**: Floating action context that responds to scroll
///
/// # Flutter Compliance
///
/// **INCOMPLETE IMPLEMENTATION**:
/// - ❌ No scroll direction tracking
/// - ❌ Child never laid out
/// - ❌ snap flag not used
/// - ⚠️ Behaves like normal header, not floating
///
/// # Key Missing Features
///
/// | Feature | Status | Impact |
/// |---------|--------|--------|
/// | Scroll direction tracking | ❌ Missing | Cannot detect "scroll up" |
/// | Floating on scroll up | ❌ Missing | Core behavior not implemented |
/// | Child layout | ❌ Missing | Header content not sized |
/// | Snap to full/hidden | ❌ Not used | Flag has no effect |
///
/// # Current Behavior vs Intended
///
/// **Current (Simplified):**
/// - scroll_offset < extent: Partially visible, shrinks normally
/// - scroll_offset >= extent: Hidden
/// - No difference between scroll up/down
///
/// **Intended (Flutter):**
/// - Scroll down: Header hides completely
/// - Scroll up: Header appears at full extent immediately
/// - Snap mode: Only full or hidden, no partial states
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFloatingPersistentHeader;
///
/// // Material Design floating app bar
/// let app_bar = RenderSliverFloatingPersistentHeader::new(56.0);
/// // WARNING: Won't actually float - missing direction tracking!
///
/// // With snap (no partial visibility)
/// let snap_bar = RenderSliverFloatingPersistentHeader::new(80.0)
///     .with_snap();
/// // WARNING: snap has no effect in current implementation!
/// ```
#[derive(Debug)]
pub struct RenderSliverFloatingPersistentHeader {
    /// Height/extent of the header
    pub extent: f32,
    /// Whether to snap the header (show fully or hide fully)
    pub snap: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFloatingPersistentHeader {
    /// Create new floating persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header in pixels
    pub fn new(extent: f32) -> Self {
        Self {
            extent,
            snap: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set snap behavior
    pub fn set_snap(&mut self, snap: bool) {
        self.snap = snap;
    }

    /// Create with snap behavior
    pub fn with_snap(mut self) -> Self {
        self.snap = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for floating behavior
    ///
    /// Floating headers appear immediately on reverse scroll but can
    /// scroll completely off-screen when scrolling forward.
    fn calculate_sliver_geometry(&self, constraints: &SliverConstraints) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // For floating headers, visibility depends on scroll direction
        // Since we don't have scroll direction here, we use a simplified model:
        // - If scroll offset is 0, header is fully visible
        // - If scroll offset >= extent, header can scroll off
        // - In between, header is partially visible

        let visible_extent = if scroll_offset < self.extent {
            // Header is in view
            (self.extent - scroll_offset).max(0.0)
        } else {
            // Header has scrolled off (but can float back)
            // For floating, we don't pin it, so it's truly off
            0.0
        };

        let paint_extent = visible_extent.min(remaining_extent);

        SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: self.extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if self.extent > 0.0 {
                (paint_extent / self.extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverFloatingPersistentHeader {
    fn default() -> Self {
        Self::new(56.0) // Material Design standard app bar height
    }
}

impl RenderObject for RenderSliverFloatingPersistentHeader {}

impl RenderSliver<Single> for RenderSliverFloatingPersistentHeader {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();

        // Calculate visible extent for child layout
        let scroll_offset = ctx.constraints.scroll_offset;
        let visible_extent = if scroll_offset < self.extent {
            (self.extent - scroll_offset).max(0.0)
        } else {
            0.0
        };

        // Layout child with box constraints (height = visible_extent or extent)
        // We layout with full extent even if partially visible so child maintains size
        let layout_extent = if visible_extent > 0.0 { self.extent } else { 0.0 };

        let box_constraints = BoxConstraints::new(
            0.0,
            ctx.constraints.cross_axis_extent,
            layout_extent,
            layout_extent,
        );

        ctx.tree_mut().perform_layout(child_id, box_constraints)?;

        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&ctx.constraints);
        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();

            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
                *ctx.canvas = child_canvas;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_floating_persistent_header_new() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

        assert_eq!(header.extent, 80.0);
        assert!(!header.snap);
    }

    #[test]
    fn test_render_sliver_floating_persistent_header_default() {
        let header = RenderSliverFloatingPersistentHeader::default();

        assert_eq!(header.extent, 56.0);
        assert!(!header.snap);
    }

    #[test]
    fn test_set_snap() {
        let mut header = RenderSliverFloatingPersistentHeader::new(80.0);
        header.set_snap(true);

        assert!(header.snap);
    }

    #[test]
    fn test_with_snap() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0).with_snap();

        assert!(header.snap);
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_visible() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

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

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be fully visible
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 80.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 40.0, // Scrolled 40px
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be half visible (80 - 40 = 40px)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 40.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.5);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled past header
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be scrolled off (floating, not pinned)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.0);
    }
}
