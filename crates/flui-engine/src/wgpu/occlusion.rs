//! Occlusion culling for opaque primitives.
//!
//! Tracks opaque regions to skip fully-occluded draw calls, reducing overdraw
//! in deep widget composition scenarios.
//!
//! # Approach
//!
//! Maintains a list of opaque axis-aligned rects. A layer is considered occluded
//! if any single opaque rect fully contains it. This handles the common case
//! (background fills, cards) without complex region algebra.
//!
//! # Performance
//!
//! O(n) per occlusion check where n = number of opaque rects.
//! Designed for < 100 opaque rects per frame (typical UI).

/// Tracks opaque regions to skip fully-occluded draw calls.
///
/// Simple approach: maintain a list of opaque axis-aligned rects.
/// A layer is occluded if any single opaque rect fully contains it.
/// This handles the common case (background fills, cards) without
/// complex region algebra.
///
/// # Performance
/// O(n) per occlusion check where n = number of opaque rects.
/// Designed for < 100 opaque rects per frame (typical UI).
pub struct OcclusionTracker {
    opaque_rects: Vec<[f32; 4]>, // [x, y, width, height]
}

impl OcclusionTracker {
    /// Creates an empty tracker with pre-allocated capacity.
    pub fn new() -> Self {
        Self {
            opaque_rects: Vec::with_capacity(32),
        }
    }

    /// Registers an opaque region.
    ///
    /// Opaque rects should be added in front-to-back order so that
    /// subsequent layers can be tested against all previously added rects.
    pub fn add_opaque(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.opaque_rects.push([x, y, w, h]);
    }

    /// Checks if a rect is fully contained by any single opaque rect.
    ///
    /// Returns `true` if the query rect is fully occluded and can be skipped.
    #[inline]
    pub fn is_occluded(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let qx2 = x + w;
        let qy2 = y + h;

        self.opaque_rects
            .iter()
            .any(|&[ox, oy, ow, oh]| ox <= x && oy <= y && (ox + ow) >= qx2 && (oy + oh) >= qy2)
    }

    /// Clears all tracked opaque rects for the next frame.
    pub fn reset(&mut self) {
        self.opaque_rects.clear();
    }

    /// Returns the number of tracked opaque rects (for debugging).
    pub fn opaque_count(&self) -> usize {
        self.opaque_rects.len()
    }
}

impl Default for OcclusionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tracker_not_occluded() {
        let tracker = OcclusionTracker::new();
        assert!(!tracker.is_occluded(10.0, 10.0, 50.0, 50.0));
    }

    #[test]
    fn test_fully_occluded() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(0.0, 0.0, 100.0, 100.0);
        assert!(tracker.is_occluded(10.0, 10.0, 50.0, 50.0));
    }

    #[test]
    fn test_partially_occluded_not_culled() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(0.0, 0.0, 100.0, 100.0);
        assert!(!tracker.is_occluded(50.0, 50.0, 100.0, 100.0)); // extends beyond
    }

    #[test]
    fn test_multiple_opaque_rects() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(0.0, 0.0, 50.0, 50.0);
        tracker.add_opaque(50.0, 0.0, 50.0, 50.0);
        // Small rect fully inside first opaque
        assert!(tracker.is_occluded(10.0, 10.0, 20.0, 20.0));
        // Rect spanning both — not fully contained by either single rect
        assert!(!tracker.is_occluded(25.0, 10.0, 50.0, 20.0));
    }

    #[test]
    fn test_reset_clears() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(0.0, 0.0, 100.0, 100.0);
        tracker.reset();
        assert!(!tracker.is_occluded(10.0, 10.0, 50.0, 50.0));
    }

    #[test]
    fn test_exact_bounds() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(10.0, 10.0, 50.0, 50.0);
        // Exactly same rect — should be occluded (>= comparison)
        assert!(tracker.is_occluded(10.0, 10.0, 50.0, 50.0));
    }

    #[test]
    fn test_zero_size_query() {
        let mut tracker = OcclusionTracker::new();
        tracker.add_opaque(0.0, 0.0, 100.0, 100.0);
        assert!(tracker.is_occluded(50.0, 50.0, 0.0, 0.0));
    }

    #[test]
    fn test_opaque_count() {
        let mut tracker = OcclusionTracker::new();
        assert_eq!(tracker.opaque_count(), 0);
        tracker.add_opaque(0.0, 0.0, 10.0, 10.0);
        tracker.add_opaque(20.0, 20.0, 10.0, 10.0);
        assert_eq!(tracker.opaque_count(), 2);
        tracker.reset();
        assert_eq!(tracker.opaque_count(), 0);
    }

    #[test]
    fn test_default_trait() {
        let tracker = OcclusionTracker::default();
        assert_eq!(tracker.opaque_count(), 0);
        assert!(!tracker.is_occluded(0.0, 0.0, 1.0, 1.0));
    }
}
