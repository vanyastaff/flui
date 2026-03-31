//! Damage region tracking for incremental rendering.
//!
//! This module provides [`DamageTracker`], which accumulates dirty rectangles
//! within a frame so the renderer can skip repainting unchanged regions.

use flui_types::geometry::{Pixels, Rect};

/// Accumulates damage regions for incremental rendering.
///
/// Tracks dirty rectangles within a frame. The renderer can query
/// the unified damage rect to determine what needs repainting.
///
/// # Usage
/// ```ignore
/// let mut tracker = DamageTracker::new();
/// // First frame always does full repaint
/// assert!(tracker.needs_full_repaint());
///
/// tracker.reset(); // Start new frame
/// tracker.mark_dirty(some_rect);
/// if let Some(damage) = tracker.damage_rect() {
///     // Render only the damaged region
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DamageTracker {
    regions: Vec<Rect<Pixels>>,
    full_repaint: bool,
}

impl DamageTracker {
    /// Creates a new tracker with `full_repaint: true` (first frame always full).
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            full_repaint: true,
        }
    }

    /// Adds a dirty region to the tracker.
    #[inline]
    pub fn mark_dirty(&mut self, rect: Rect<Pixels>) {
        self.regions.push(rect);
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

    /// Returns the bounding box of all dirty regions.
    ///
    /// - Returns `None` if a full repaint is needed (caller should repaint everything).
    /// - Returns `Rect::ZERO` if no regions are marked and no full repaint is needed
    ///   (meaning nothing to paint).
    /// - Otherwise returns the union of all dirty regions.
    #[must_use]
    pub fn damage_rect(&self) -> Option<Rect<Pixels>> {
        if self.full_repaint {
            return None;
        }

        let mut iter = self.regions.iter();
        let first = match iter.next() {
            Some(r) => r,
            None => return Some(Rect::ZERO),
        };

        let mut result = *first;
        for r in iter {
            result = result.union(r);
        }
        Some(result)
    }

    /// Returns `true` if any damage exists (regions or full repaint).
    #[inline]
    #[must_use]
    pub fn has_damage(&self) -> bool {
        self.full_repaint || !self.regions.is_empty()
    }

    /// Clears regions and resets `full_repaint` to false. Call at frame start.
    #[inline]
    pub fn reset(&mut self) {
        self.regions.clear();
        self.full_repaint = false;
    }

    /// Returns the number of dirty regions (for debugging).
    #[inline]
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
}

impl Default for DamageTracker {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
