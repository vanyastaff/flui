//! Generic hit test entry infrastructure for box and sliver rendering
//!
//! This module provides a unified hit testing system that works for both
//! box-based rendering (BoxConstraints → Size) and sliver-based rendering
//! (SliverConstraints → SliverGeometry).

use flui_types::{Offset, Size, SliverGeometry};
use std::fmt::Debug;

/// Base trait for all hit test entries
///
/// This trait allows generic handling of hit test results across
/// different rendering protocols (box, sliver, etc.).
pub trait HitTestEntryTrait: Debug + Clone + Send + Sync {
    /// Local position where hit occurred
    fn local_position(&self) -> Offset;

    /// Check if this entry represents a valid hit within bounds
    ///
    /// Returns true if the hit position is within the element's valid hit region.
    /// This may return false for hits outside visible bounds or scrolled off-screen.
    fn is_valid_hit(&self) -> bool {
        true // Default: all hits are valid
    }
}

/// Hit test entry for box rendering
///
/// Used for standard box-based layout (BoxConstraints → Size).
/// Stores the hit position and the size of the box that was hit.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// Local position in box coordinates
    pub local_position: Offset,

    /// Size of the box that was hit
    pub size: Size,
}

impl BoxHitTestEntry {
    /// Create a new box hit test entry
    pub fn new(local_position: Offset, size: Size) -> Self {
        Self {
            local_position,
            size,
        }
    }
}

impl HitTestEntryTrait for BoxHitTestEntry {
    fn local_position(&self) -> Offset {
        self.local_position
    }

    fn is_valid_hit(&self) -> bool {
        // Check if hit is within box bounds
        self.local_position.dx >= 0.0
            && self.local_position.dy >= 0.0
            && self.local_position.dx <= self.size.width
            && self.local_position.dy <= self.size.height
    }
}

/// Hit test entry for sliver rendering
///
/// Used for sliver-based scrollable layout (SliverConstraints → SliverGeometry).
/// Stores additional information specific to slivers like scroll offset and
/// main axis position for viewport-aware hit testing.
#[derive(Debug, Clone)]
pub struct SliverHitTestEntry {
    /// Local position in sliver coordinates
    pub local_position: Offset,

    /// Sliver geometry at the time of hit
    ///
    /// Contains information about scroll extent, paint extent, and visibility.
    pub geometry: SliverGeometry,

    /// Scroll offset when hit occurred
    ///
    /// The scroll position of the viewport at the time this sliver was hit.
    pub scroll_offset: f32,

    /// Position along main axis (scroll direction)
    ///
    /// Distance from the leading edge of the viewport along the scroll axis.
    /// Used to determine if the hit is in the visible region.
    pub main_axis_position: f32,
}

impl SliverHitTestEntry {
    /// Create a new sliver hit test entry
    pub fn new(
        local_position: Offset,
        geometry: SliverGeometry,
        scroll_offset: f32,
        main_axis_position: f32,
    ) -> Self {
        Self {
            local_position,
            geometry,
            scroll_offset,
            main_axis_position,
        }
    }

    /// Check if hit is in visible region (not scrolled off-screen)
    ///
    /// Returns true if the hit position is within the painted portion of the sliver
    /// that's currently visible in the viewport.
    pub fn is_visible(&self) -> bool {
        self.main_axis_position >= 0.0 && self.main_axis_position < self.geometry.paint_extent
    }

    /// Check if hit is in cache extent (includes off-screen buffer)
    ///
    /// Returns true if the hit is within the cache extent, which includes
    /// the visible region plus an off-screen buffer for smooth scrolling.
    pub fn is_in_cache_extent(&self) -> bool {
        self.main_axis_position >= -self.geometry.cache_extent
            && self.main_axis_position < self.geometry.paint_extent + self.geometry.cache_extent
    }

    /// Get distance from leading edge of viewport
    ///
    /// Returns the absolute position of this hit relative to the start
    /// of the scrollable content (scroll_offset + main_axis_position).
    pub fn distance_from_viewport_edge(&self) -> f32 {
        self.scroll_offset + self.main_axis_position
    }

    /// Get cross-axis position
    ///
    /// Returns the position perpendicular to the scroll direction.
    /// For vertical scrolling, this is the X coordinate.
    /// For horizontal scrolling, this is the Y coordinate.
    pub fn cross_axis_position(&self) -> f32 {
        // Assuming vertical scroll (most common), cross-axis is X
        // TODO: This should check axis direction from constraints
        self.local_position.dx
    }
}

impl HitTestEntryTrait for SliverHitTestEntry {
    fn local_position(&self) -> Offset {
        self.local_position
    }

    fn is_valid_hit(&self) -> bool {
        // For slivers, a valid hit must be in the visible region
        self.is_visible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_hit_test_entry_within_bounds() {
        let entry = BoxHitTestEntry::new(Offset::new(50.0, 50.0), Size::new(100.0, 100.0));
        assert!(entry.is_valid_hit());
    }

    #[test]
    fn test_box_hit_test_entry_outside_bounds() {
        let entry = BoxHitTestEntry::new(Offset::new(150.0, 50.0), Size::new(100.0, 100.0));
        assert!(!entry.is_valid_hit());
    }

    #[test]
    fn test_sliver_hit_test_entry_visible() {
        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 600.0,
            cache_extent: 250.0,
            ..Default::default()
        };

        let entry = SliverHitTestEntry::new(Offset::new(50.0, 200.0), geometry, 100.0, 200.0);

        assert!(entry.is_visible());
        assert!(entry.is_in_cache_extent());
        assert_eq!(entry.distance_from_viewport_edge(), 300.0); // 100 + 200
    }

    #[test]
    fn test_sliver_hit_test_entry_not_visible() {
        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 600.0,
            cache_extent: 250.0,
            ..Default::default()
        };

        let entry = SliverHitTestEntry::new(Offset::new(50.0, 700.0), geometry, 100.0, 700.0);

        assert!(!entry.is_visible()); // Beyond paint_extent
        assert!(!entry.is_valid_hit()); // Not a valid hit
    }

    #[test]
    fn test_sliver_hit_test_entry_in_cache_but_not_visible() {
        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 600.0,
            cache_extent: 250.0,
            ..Default::default()
        };

        // Hit at 650 (beyond visible 600 but within cache 600+250=850)
        let entry = SliverHitTestEntry::new(Offset::new(50.0, 650.0), geometry, 100.0, 650.0);

        assert!(!entry.is_visible());
        assert!(entry.is_in_cache_extent());
    }
}
