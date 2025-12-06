//! RenderSliverPinnedPersistentHeader - Always-visible collapsible header
//!
//! Specialized persistent header that sticks to the top/leading edge once scrolled into view.
//! Supports collapsing from a maximum extent down to a minimum extent as content scrolls,
//! but never scrolls off-screen once pinned. This is the specialized variant - unlike the
//! generic RenderSliverPersistentHeader, pinning is always enabled (not configurable).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverPinnedPersistentHeader` | `RenderSliverPersistentHeader` with `pinned: true` from `package:flutter/src/rendering/sliver_persistent_header.dart` |
//! | `min_extent` | Minimum height when fully collapsed |
//! | `max_extent` | Maximum height when fully expanded |
//! | Collapsing logic | scroll_extent = max - min, layout_extent = min |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate collapse extent**
//!    - scrolled_extent = scroll_offset.min(max_extent - min_extent)
//!    - current_extent = max_extent - scrolled_extent
//!    - Shrinks from max to min as we scroll
//!
//! 2. **Layout child with BoxConstraints** (NOT IMPLEMENTED)
//!    - Convert current_extent to BoxConstraints
//!    - Layout single child with header content
//!
//! 3. **Return geometry with pinning**
//!    - scroll_extent: max_extent - min_extent (only collapsible part)
//!    - paint_extent: current_extent.min(remaining)
//!    - layout_extent: min_extent (THIS makes it pinned!)
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
//! - **Memory**: 8 bytes (2×f32) + 48 bytes (SliverGeometry) = 56 bytes
//!
//! # Use Cases
//!
//! - **Material app bars**: Collapsing app bars that shrink on scroll
//! - **Section headers**: Sticky category headers at minimum size
//! - **Table headers**: Column headers that collapse but stay visible
//! - **Search bars**: Search UI that shrinks but never hides
//! - **Navigation headers**: Toolbars that minimize to icon-only mode
//!
//! # Collapsible Pinned Behavior
//!
//! ```text
//! With min_extent=40, max_extent=120:
//!
//! scroll=0:    [████████████] (120px - full expanded)
//!              layout_extent=40, paint_extent=120
//!
//! scroll=40:   [████████] (80px - collapsing)
//!              layout_extent=40, paint_extent=80
//!
//! scroll=80:   [████] (40px - fully collapsed, PINNED)
//!              layout_extent=40, paint_extent=40
//!
//! scroll=100+: [████] (40px - STAYS at minimum!)
//!              layout_extent=40, paint_extent=40
//!
//! KEY INSIGHT: layout_extent ALWAYS = min_extent
//! This reserves min_extent space at top, making it "pinned"
//! ```
//!
//! # Fixed vs Collapsible Mode
//!
//! ```rust,ignore
//! // Fixed extent (min == max) - no collapsing
//! let fixed = RenderSliverPinnedPersistentHeader::new(56.0);
//! // scroll_extent = 0 (nothing to collapse)
//! // Always 56px visible
//!
//! // Collapsible (min < max) - shrinks as we scroll
//! let collapsible = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);
//! // scroll_extent = 80 (collapsible range)
//! // Shrinks from 120px to 40px, then stays pinned
//! ```
//!
//! # ⚠️ IMPLEMENTATION ISSUE
//!
//! This implementation has **ONE CRITICAL MISSING FEATURE**:
//!
//! 1. **❌ Child is NEVER laid out** (line 141-145)
//!    - No calls to `layout_child()` anywhere
//!    - Child size is undefined
//!    - Only geometry calculation, no actual layout
//!
//! 2. **✅ Geometry calculation EXCELLENT** (line 94-131)
//!    - Correctly implements collapsible pinned behavior
//!    - scroll_extent = max - min (only collapsible part)
//!    - layout_extent = min (THIS is the pinning mechanism!)
//!    - Sophisticated and correct implementation
//!
//! 3. **✅ Paint works correctly**
//!    - Paints child when visible using paint_child()
//!
//! **This is one of the BEST implementations - only missing child layout!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPersistentHeader**: Pinned is always pinned, Persistent is configurable
//! - **vs SliverFloatingPersistentHeader**: Pinned stays visible, Floating can hide
//! - **vs SliverAppBar**: AppBar has UI (title, actions), PinnedHeader is generic container
//! - **vs SliverPadding**: PinnedHeader sticks and collapses, Padding just adds space
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverPinnedPersistentHeader;
//!
//! // Fixed-height pinned header (Material Design standard)
//! let fixed = RenderSliverPinnedPersistentHeader::new(56.0);
//! // Always 56px, never collapses or scrolls away
//!
//! // Collapsible toolbar (like Material expanded app bar)
//! let toolbar = RenderSliverPinnedPersistentHeader::with_extents(48.0, 200.0);
//! // Starts at 200px, collapses to 48px, stays pinned
//!
//! // Tall hero header that minimizes
//! let hero = RenderSliverPinnedPersistentHeader::with_extents(60.0, 300.0);
//! // Large hero image that shrinks to small header
//! ```

use crate::core::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a pinned persistent header that collapses but never hides.
///
/// Specialized persistent header that always stays visible once scrolled into view.
/// Supports collapsing from `max_extent` down to `min_extent` as content scrolls,
/// but never scrolls off-screen. The key mechanism is that `layout_extent` is always
/// set to `min_extent`, which reserves that space at the top of the viewport.
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
/// **Collapsible Pinned Header** - Always-visible header that shrinks from max to min
/// extent as we scroll, then stays pinned at minimum size. Uses clever geometry with
/// `layout_extent = min_extent` to reserve space at viewport top.
///
/// # Use Cases
///
/// - **Material app bars**: Collapsing app bars (Material Design pattern)
/// - **Section headers**: Sticky category headers at minimum size
/// - **Table headers**: Column headers that collapse but stay visible
/// - **Search bars**: Search UI that shrinks but never disappears
/// - **Navigation toolbars**: Toolbars that minimize to icon-only mode
///
/// # Flutter Compliance
///
/// **ALMOST COMPLETE** - Excellent geometry implementation:
/// - ✅ Collapsible pinned behavior correct
/// - ✅ Geometry calculation sophisticated and accurate
/// - ✅ Paint works correctly
/// - ❌ Child never laid out (only missing feature)
///
/// # Implementation Quality
///
/// | Feature | Status | Quality |
/// |---------|--------|---------|
/// | Collapse logic | ✅ Complete | **EXCELLENT** - correctly shrinks max→min |
/// | Pinning mechanism | ✅ Complete | **EXCELLENT** - layout_extent = min |
/// | Fixed vs collapsible | ✅ Complete | Supports both (min==max or min<max) |
/// | Child layout | ❌ Missing | No layout_child() calls |
/// | Paint | ✅ Works | Correctly paints when visible |
///
/// # Geometry Behavior
///
/// **Key Insight**: `layout_extent = min_extent` (always) makes it "pinned"
///
/// - `scroll_extent = max_extent - min_extent` (only collapsible part scrolls)
/// - `paint_extent = (max_extent - scrolled).min(remaining)` (shrinks as we scroll)
/// - `layout_extent = min_extent` (reserves space at top - THIS is the pinning!)
///
/// **Fixed mode** (min == max):
/// - `scroll_extent = 0` (nothing to collapse)
/// - `paint_extent = extent` (always same size)
/// - `layout_extent = extent` (always takes same space)
///
/// **Collapsible mode** (min < max):
/// - Starts at max_extent when scroll_offset = 0
/// - Shrinks linearly as we scroll
/// - Reaches min_extent when scroll_offset >= (max - min)
/// - Stays pinned at min_extent forever after
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPinnedPersistentHeader;
///
/// // Fixed-height (Material Design standard)
/// let fixed = RenderSliverPinnedPersistentHeader::new(56.0);
/// // Always 56px, never changes
///
/// // Collapsible Material app bar
/// let app_bar = RenderSliverPinnedPersistentHeader::with_extents(56.0, 200.0);
/// // Starts at 200px (with hero image), collapses to 56px toolbar
/// ```
#[derive(Debug)]
pub struct RenderSliverPinnedPersistentHeader {
    /// Minimum extent (height when pinned)
    pub min_extent: f32,
    /// Maximum extent (height when fully expanded)
    pub max_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPinnedPersistentHeader {
    /// Create new pinned persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header (both min and max)
    pub fn new(extent: f32) -> Self {
        Self {
            min_extent: extent,
            max_extent: extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Create with separate min and max extents
    ///
    /// This allows the header to collapse/expand as it scrolls.
    ///
    /// # Arguments
    /// * `min_extent` - Minimum height when pinned
    /// * `max_extent` - Maximum height when fully expanded
    pub fn with_extents(min_extent: f32, max_extent: f32) -> Self {
        Self {
            min_extent,
            max_extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set minimum extent
    pub fn set_min_extent(&mut self, extent: f32) {
        self.min_extent = extent;
    }

    /// Set maximum extent
    pub fn set_max_extent(&mut self, extent: f32) {
        self.max_extent = extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for pinned behavior
    ///
    /// Pinned headers:
    /// - Scroll normally until they reach the top
    /// - Then stick to the top (at min_extent) as content scrolls underneath
    /// - Never scroll off-screen once pinned
    fn calculate_sliver_geometry(&self, constraints: &SliverConstraints) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate how much of the header is scrolled past
        let scrolled_extent = scroll_offset.min(self.max_extent - self.min_extent);

        // Current extent shrinks as we scroll (from max to min)
        let current_extent = self.max_extent - scrolled_extent;

        // Paint extent is what's actually visible
        let paint_extent = current_extent.min(remaining_extent);

        // Layout extent for pinned header is always min_extent
        // This means it always takes up min_extent space at the top
        let layout_extent = self.min_extent.min(remaining_extent);

        SliverGeometry {
            // Scroll extent is the collapsible part (max - min)
            scroll_extent: self.max_extent - self.min_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: self.max_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if self.max_extent > 0.0 {
                (paint_extent / self.max_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.max_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverPinnedPersistentHeader {
    fn default() -> Self {
        Self::new(56.0) // Material Design standard app bar height
    }
}

impl RenderObject for RenderSliverPinnedPersistentHeader {}

impl RenderSliver<Single> for RenderSliverPinnedPersistentHeader {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();

        // Calculate current extent (shrinks from max to min as we scroll)
        let scroll_offset = ctx.constraints.scroll_offset;
        let scrolled_extent = scroll_offset.min(self.max_extent - self.min_extent);
        let current_extent = self.max_extent - scrolled_extent;

        // Layout child with box constraints (height = current_extent)
        let box_constraints = BoxConstraints::new(
            0.0,
            ctx.constraints.cross_axis_extent,
            current_extent,
            current_extent,
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
    fn test_render_sliver_pinned_persistent_header_new() {
        let header = RenderSliverPinnedPersistentHeader::new(60.0);

        assert_eq!(header.min_extent, 60.0);
        assert_eq!(header.max_extent, 60.0);
    }

    #[test]
    fn test_render_sliver_pinned_persistent_header_with_extents() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        assert_eq!(header.min_extent, 40.0);
        assert_eq!(header.max_extent, 120.0);
    }

    #[test]
    fn test_render_sliver_pinned_persistent_header_default() {
        let header = RenderSliverPinnedPersistentHeader::default();

        assert_eq!(header.min_extent, 56.0);
        assert_eq!(header.max_extent, 56.0);
    }

    #[test]
    fn test_set_min_extent() {
        let mut header = RenderSliverPinnedPersistentHeader::new(60.0);
        header.set_min_extent(30.0);

        assert_eq!(header.min_extent, 30.0);
    }

    #[test]
    fn test_set_max_extent() {
        let mut header = RenderSliverPinnedPersistentHeader::new(60.0);
        header.set_max_extent(100.0);

        assert_eq!(header.max_extent, 100.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

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

        // Header should be at max extent
        assert_eq!(geometry.scroll_extent, 80.0); // max - min = 120 - 40
        assert_eq!(geometry.paint_extent, 120.0); // Full max extent
        assert_eq!(geometry.layout_extent, 40.0); // Always min_extent for pinned
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

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

        // Header should be collapsing (120 - 40 = 80px)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 80.0); // Partially collapsed
        assert_eq!(geometry.layout_extent, 40.0); // Still min_extent
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_collapsed() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled past collapsible part
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be pinned at min extent
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 40.0); // Collapsed to min
        assert_eq!(geometry.layout_extent, 40.0); // At min_extent
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_fixed_extent() {
        let header = RenderSliverPinnedPersistentHeader::new(60.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Fixed extent header (min == max)
        assert_eq!(geometry.scroll_extent, 0.0); // No collapsible part
        assert_eq!(geometry.paint_extent, 60.0);
        assert_eq!(geometry.layout_extent, 60.0);
        assert!(geometry.visible);
    }
}
