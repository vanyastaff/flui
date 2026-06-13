//! Damage region tracking for incremental rendering.
//!
//! This module provides [`DamageTracker`], which accumulates dirty rectangles
//! within a frame so the renderer can skip repainting unchanged regions.
//!
//! # Multi-rect strategy (Slint-inspired)
//!
//! The tracker holds at most [`MAX_DAMAGE_RECTS`] (3) regions. When a new
//! dirty rect would exceed the limit, the pair with the smallest union area
//! increase is merged first. This bounds GPU scissor passes to 3 while
//! keeping damage tight — a single full-screen union wastes GPU work on
//! unchanged regions.

use flui_types::geometry::{Pixels, Rect};

/// Maximum number of damage rects before merging. Slint uses 3 — enough
/// for typical UI patterns (toolbar + content + sidebar) without blowing
/// up scissor passes.
const MAX_DAMAGE_RECTS: usize = 3;

/// Accumulates damage regions for incremental rendering.
///
/// Tracks up to [`MAX_DAMAGE_RECTS`] dirty rectangles within a frame.
/// When the limit is exceeded, the pair with the smallest union area
/// increase is merged. The renderer can query [`damage_rects`] for the
/// individual rects (for multi-scissor passes) or [`damage_rect`] for
/// a single bounding rect.
///
/// # Usage
/// ```ignore
/// let mut tracker = DamageTracker::new();
/// // First frame always does full repaint
/// assert!(tracker.needs_full_repaint());
///
/// tracker.reset(); // Start new frame
/// tracker.mark_dirty(some_rect);
/// if let Some(rects) = tracker.damage_rects() {
///     // Render only the damaged regions (up to 3 scissor passes)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DamageTracker {
    /// Up to `MAX_DAMAGE_RECTS` damage regions.
    regions: [Rect<Pixels>; MAX_DAMAGE_RECTS],
    /// Number of valid entries in `regions`.
    count: usize,
    /// Whether a full repaint is needed (first frame, resize, etc.).
    full_repaint: bool,
}

impl DamageTracker {
    /// Creates a new tracker with `full_repaint: true` (first frame always full).
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            regions: [Rect::ZERO; MAX_DAMAGE_RECTS],
            count: 0,
            full_repaint: true,
        }
    }

    /// Adds a dirty region to the tracker.
    ///
    /// If the rect is zero-sized, it is ignored. If adding would exceed
    /// [`MAX_DAMAGE_RECTS`], the pair with the smallest union area increase
    /// is merged first.
    #[inline]
    pub fn mark_dirty(&mut self, rect: Rect<Pixels>) {
        // Zero-sized rects are no-ops.
        if rect.width().0 <= 0.0 || rect.height().0 <= 0.0 {
            return;
        }

        if self.count < MAX_DAMAGE_RECTS {
            self.regions[self.count] = rect;
            self.count += 1;
            return;
        }

        // Find the pair (i, j) whose union has the smallest area increase
        // over the sum of their individual areas. Merge them, then insert
        // the new rect in the freed slot.
        let mut best_i = 0;
        let mut best_j = 1;
        let mut best_increase = f32::MAX;

        for i in 0..MAX_DAMAGE_RECTS {
            for j in (i + 1)..MAX_DAMAGE_RECTS {
                let area_i = Self::area(&self.regions[i]);
                let area_j = Self::area(&self.regions[j]);
                let union = self.regions[i].union(&self.regions[j]);
                let increase = Self::area(&union) - area_i - area_j;
                if increase < best_increase {
                    best_increase = increase;
                    best_i = i;
                    best_j = j;
                }
            }
        }

        // Merge best_i and best_j into best_i, put new rect in best_j.
        self.regions[best_i] = self.regions[best_i].union(&self.regions[best_j]);
        self.regions[best_j] = rect;
    }

    /// Forces a full repaint (e.g., on window resize).
    #[inline]
    pub fn mark_full_repaint(&mut self) {
        self.full_repaint = true;
    }

    /// Returns whether a full repaint is needed.
    #[inline]
    #[must_use]
    pub fn needs_full_repaint(&self) -> bool {
        self.full_repaint
    }

    /// Returns the individual damage rects for multi-scissor passes.
    ///
    /// - Returns `None` if a full repaint is needed.
    /// - Returns `Some(&[])` if no regions are marked (nothing to paint).
    /// - Otherwise returns a slice of up to 3 rects.
    #[must_use]
    pub fn damage_rects(&self) -> Option<&[Rect<Pixels>]> {
        if self.full_repaint {
            return None;
        }
        Some(&self.regions[..self.count])
    }

    /// Returns a single bounding rect of all damage regions.
    ///
    /// - Returns `None` if a full repaint is needed.
    /// - Returns `Some(Rect::ZERO)` if no regions are marked.
    /// - Otherwise returns the union of all damage regions.
    #[must_use]
    pub fn damage_rect(&self) -> Option<Rect<Pixels>> {
        if self.full_repaint {
            return None;
        }

        if self.count == 0 {
            return Some(Rect::ZERO);
        }

        let mut result = self.regions[0];
        for i in 1..self.count {
            result = result.union(&self.regions[i]);
        }
        Some(result)
    }

    /// Returns `true` if any damage exists (regions or full repaint).
    #[inline]
    #[must_use]
    pub fn has_damage(&self) -> bool {
        self.full_repaint || self.count > 0
    }

    /// Clears regions and resets `full_repaint` to false. Call at frame start.
    #[inline]
    pub fn reset(&mut self) {
        self.count = 0;
        self.full_repaint = false;
    }

    /// Returns the number of dirty regions (for debugging).
    #[inline]
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.count
    }

    /// Computes the area of a rect.
    #[inline]
    fn area(rect: &Rect<Pixels>) -> f32 {
        rect.width().0 * rect.height().0
    }
}

impl Default for DamageTracker {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::{px, Rect};

    use super::*;

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect<Pixels> {
        Rect::from_ltrb(px(l), px(t), px(r), px(b))
    }

    #[test]
    fn new_tracker_needs_full_repaint() {
        let tracker = DamageTracker::new();
        assert!(tracker.needs_full_repaint());
        assert!(tracker.damage_rect().is_none());
        assert!(tracker.damage_rects().is_none());
    }

    #[test]
    fn reset_clears_full_repaint() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        assert!(!tracker.needs_full_repaint());
        assert_eq!(tracker.region_count(), 0);
        assert_eq!(tracker.damage_rect(), Some(Rect::ZERO));
    }

    #[test]
    fn single_damage_rect() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(10.0, 10.0, 50.0, 50.0));
        assert_eq!(tracker.region_count(), 1);
        assert_eq!(tracker.damage_rect(), Some(rect(10.0, 10.0, 50.0, 50.0)));
        assert_eq!(tracker.damage_rects().unwrap().len(), 1);
    }

    #[test]
    fn three_damage_rects_no_merge() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(0.0, 0.0, 10.0, 10.0));
        tracker.mark_dirty(rect(100.0, 100.0, 110.0, 110.0));
        tracker.mark_dirty(rect(200.0, 200.0, 210.0, 210.0));
        assert_eq!(tracker.region_count(), 3);
        assert_eq!(tracker.damage_rects().unwrap().len(), 3);
    }

    #[test]
    fn fourth_rect_merges_closest_pair() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        // Two close rects (small union increase) and one far rect.
        tracker.mark_dirty(rect(0.0, 0.0, 10.0, 10.0));   // area = 100
        tracker.mark_dirty(rect(5.0, 5.0, 15.0, 15.0));    // area = 100, close to first
        tracker.mark_dirty(rect(200.0, 200.0, 210.0, 210.0)); // area = 100, far
        // Fourth rect should merge the two close ones.
        tracker.mark_dirty(rect(300.0, 300.0, 310.0, 310.0));
        assert_eq!(tracker.region_count(), 3);
    }

    #[test]
    fn zero_size_rect_ignored() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(10.0, 10.0, 10.0, 50.0)); // zero width
        assert_eq!(tracker.region_count(), 0);
        tracker.mark_dirty(rect(10.0, 10.0, 50.0, 10.0)); // zero height
        assert_eq!(tracker.region_count(), 0);
    }

    #[test]
    fn mark_full_repaint_overrides() {
        let mut tracker = DamageTracker::new();
        tracker.reset();
        tracker.mark_dirty(rect(0.0, 0.0, 10.0, 10.0));
        tracker.mark_full_repaint();
        assert!(tracker.needs_full_repaint());
        assert!(tracker.damage_rect().is_none());
        assert!(tracker.damage_rects().is_none());
    }
}
