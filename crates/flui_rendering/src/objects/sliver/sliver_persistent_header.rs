//! RenderSliverPersistentHeader - Generic persistent header with configurable pinning/floating
//!
//! Base implementation for persistent headers that can optionally stick during scrolling.
//! Unlike specialized variants (SliverPinnedPersistentHeader, SliverFloatingPersistentHeader),
//! this is the generic header that supports configuration of both behaviors through flags.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverPersistentHeader` | `RenderSliverPersistentHeader` from `package:flutter/src/rendering/sliver_persistent_header.dart` |
//! | `pinned` flag | `pinned` parameter for sticky behavior |
//! | `floating` flag | `floating` parameter for reverse-scroll appearance |
//! | Pinned geometry | Sticks once scroll_offset >= extent |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate header visibility**
//!    - If pinned && scrolled past: Always visible (sticks)
//!    - If floating && scrolling up: Appears immediately (NOT IMPLEMENTED)
//!    - Otherwise: Normal scroll-away behavior
//!
//! 2. **Layout child with BoxConstraints** (NOT IMPLEMENTED)
//!    - Convert SliverConstraints to BoxConstraints
//!    - Layout single child with header content
//!
//! 3. **Return geometry**
//!    - scroll_extent: Fixed header extent
//!    - paint_extent: Current visible extent
//!    - layout_extent: Affects following slivers (pinned adds space)
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
//! - **Memory**: 9 bytes (f32 + 2×bool) + 48 bytes (SliverGeometry) = 57 bytes
//!
//! # Use Cases
//!
//! - **Section headers**: Sticky category headers in lists
//! - **Table headers**: Persistent column headers
//! - **Date separators**: Timeline headers in messaging
//! - **Group dividers**: Shopping cart section headers
//! - **Context headers**: Document outline navigation
//!
//! # Pinned vs Floating vs Both
//!
//! ```text
//! PINNED ONLY (pinned=true, floating=false):
//! scroll=0:    [████████] (full)
//! scroll=50:   [███] (shrinking)
//! scroll=80+:  [███] (STICKS at minimum)
//!
//! FLOATING ONLY (pinned=false, floating=true) - INTENDED:
//! scroll down: [        ] (hides)
//! scroll UP:   [████████] (APPEARS immediately)
//!
//! BOTH (pinned=true, floating=true) - INTENDED:
//! scroll down: [███] (sticks at minimum)
//! scroll UP:   [████████] (expands to full)
//!
//! CURRENT IMPLEMENTATION:
//! floating flag has NO EFFECT - only pinned works
//! ```
//!
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Child is NEVER laid out** (line 138-142)
//!    - No calls to `layout_child()` anywhere
//!    - Child size is undefined
//!    - Only geometry calculation, no actual layout
//!
//! 2. **❌ floating flag NOT USED** (line 40, 67-69, 72-75)
//!    - Flag can be set via `set_floating()` and `with_floating()`
//!    - Never checked in `calculate_sliver_geometry()`
//!    - Dead code - has no effect on behavior
//!    - Floating behavior requires scroll direction tracking (missing)
//!
//! 3. **✅ Pinned mode works correctly** (line 91-105, 108-113)
//!    - Correctly sticks header once scroll_offset >= extent
//!    - layout_extent properly affects following slivers
//!
//! 4. **⚠️ Simplified implementation** (line 83-134)
//!    - Only supports pinned behavior
//!    - Missing floating logic for reverse scroll detection
//!    - Generic header should support both modes
//!
//! **This RenderObject is BROKEN - missing child layout, floating flag unused!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPinnedPersistentHeader**: Generic has flags, Pinned is always pinned
//! - **vs SliverFloatingPersistentHeader**: Generic has flags, Floating is always floating
//! - **vs SliverAppBar**: AppBar has UI (title, actions), PersistentHeader is generic
//! - **vs SliverPadding**: PersistentHeader sticks, Padding just adds space
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverPersistentHeader;
//!
//! // Pinned header (sticks once scrolled past)
//! let pinned = RenderSliverPersistentHeader::new(56.0, true);
//! // Works correctly!
//!
//! // Floating header (appears on scroll up)
//! let floating = RenderSliverPersistentHeader::new(56.0, false)
//!     .with_floating();
//! // WARNING: floating flag has no effect - needs implementation!
//!
//! // Both pinned and floating
//! let both = RenderSliverPersistentHeader::new(56.0, true)
//!     .with_floating();
//! // WARNING: only pinned works, floating is ignored!
//! ```

use crate::core::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a generic persistent header with configurable behavior.
///
/// Base implementation for persistent headers that can optionally stick (pinned) or
/// float back on reverse scroll (floating). This is the configurable generic header,
/// unlike the specialized SliverPinnedPersistentHeader and SliverFloatingPersistentHeader
/// which hardcode their behavior.
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
/// **Configurable Persistent Header** - Generic header with runtime-configurable pinned/floating
/// flags. Intended to support both behaviors, but currently only pinned mode works. Floating
/// mode requires scroll direction tracking which is not implemented.
///
/// # Use Cases
///
/// - **Section headers**: Sticky category headers in lists
/// - **Table headers**: Persistent column headers during scroll
/// - **Date separators**: Timeline headers in chat/messaging
/// - **Group dividers**: Shopping cart section separators
/// - **Context headers**: Document outline navigation headers
///
/// # Flutter Compliance
///
/// **PARTIALLY IMPLEMENTED**:
/// - ✅ Pinned mode works correctly
/// - ❌ Floating mode has no effect (flag unused)
/// - ❌ Child never laid out
/// - ⚠️ Simplified to only support pinned behavior
///
/// # Implementation Status
///
/// | Feature | Status | Notes |
/// |---------|--------|-------|
/// | Pinned geometry | ✅ Complete | Correctly sticks once scrolled past |
/// | Floating geometry | ❌ Missing | Flag exists but never checked |
/// | Child layout | ❌ Missing | No layout_child() calls |
/// | Paint | ✅ Works | Paints child when visible |
/// | Scroll direction tracking | ❌ Missing | Required for floating mode |
///
/// # Current Behavior vs Intended
///
/// **Current (Pinned Only):**
/// - `pinned=true`: Header sticks once scroll_offset >= extent ✅
/// - `floating=true`: Flag has no effect ❌
/// - Child: Never laid out ❌
///
/// **Intended (Flutter):**
/// - `pinned=true`: Header sticks once scrolled past
/// - `floating=true`: Header appears on scroll up
/// - Both: Header sticks AND floats back to full size
/// - Child: Laid out with BoxConstraints
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPersistentHeader;
///
/// // Pinned header (works correctly)
/// let pinned = RenderSliverPersistentHeader::new(56.0, true);
///
/// // Floating header (flag ignored!)
/// let floating = RenderSliverPersistentHeader::new(50.0, false)
///     .with_floating();
/// // WARNING: floating has no effect in current implementation!
/// ```
#[derive(Debug)]
pub struct RenderSliverPersistentHeader {
    /// Height of the header
    pub extent: f32,
    /// Whether header is pinned (stays visible)
    pub pinned: bool,
    /// Whether header floats (reappears on scroll up)
    pub floating: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPersistentHeader {
    /// Create new persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header
    /// * `pinned` - Whether to pin header once visible
    pub fn new(extent: f32, pinned: bool) -> Self {
        Self {
            extent,
            pinned,
            floating: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set whether header is pinned
    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    /// Set whether header is floating
    pub fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
    }

    /// Create with floating behavior
    pub fn with_floating(mut self) -> Self {
        self.floating = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
    ) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate how much of the header is visible
        let paint_extent = if self.pinned {
            // Pinned: Always visible once reached
            if scroll_offset >= self.extent {
                // Header has been reached, now it sticks
                self.extent.min(remaining_extent)
            } else {
                // Not yet reached the header
                let visible = (self.extent - scroll_offset).max(0.0);
                visible.min(remaining_extent)
            }
        } else {
            // Not pinned: Regular scrolling behavior
            let visible = (self.extent - scroll_offset).max(0.0);
            visible.min(remaining_extent)
        };

        // Layout extent is what affects following slivers
        let layout_extent = if self.pinned && scroll_offset >= self.extent {
            // When pinned and past scroll offset, we take up space
            self.extent.min(remaining_extent)
        } else {
            paint_extent
        };

        SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
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

impl RenderObject for RenderSliverPersistentHeader {}

impl RenderSliver<Single> for RenderSliverPersistentHeader {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let child_id = *ctx.children.single();

        // Layout child with box constraints (height = extent)
        let box_constraints = BoxConstraints::new(
            0.0,
            ctx.constraints.cross_axis_extent,
            self.extent,
            self.extent,
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
    fn test_render_sliver_persistent_header_new_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        assert_eq!(header.extent, 50.0);
        assert!(header.pinned);
        assert!(!header.floating);
    }

    #[test]
    fn test_render_sliver_persistent_header_new_not_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        assert_eq!(header.extent, 50.0);
        assert!(!header.pinned);
        assert!(!header.floating);
    }

    #[test]
    fn test_set_pinned() {
        let mut header = RenderSliverPersistentHeader::new(50.0, false);
        header.set_pinned(true);

        assert!(header.pinned);
    }

    #[test]
    fn test_set_floating() {
        let mut header = RenderSliverPersistentHeader::new(50.0, true);
        header.set_floating(true);

        assert!(header.floating);
    }

    #[test]
    fn test_with_floating() {
        let header = RenderSliverPersistentHeader::new(50.0, true).with_floating();

        assert!(header.pinned);
        assert!(header.floating);
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

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

        // Full header visible
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 50.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 25.0, // Scrolled halfway
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Half visible
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 25.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past_not_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 60.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Not visible when not pinned
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 60.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Still visible when pinned!
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 50.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_pinned_before_reached() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 25.0, // Before fully scrolled
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Partially visible, not yet pinned
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 25.0); // 50 - 25
        assert!(geometry.visible);
    }
}
