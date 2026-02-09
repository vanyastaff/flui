//! Scroll metrics for tracking scroll state and position.
//!
//! Provides types for representing scroll position, bounds, and progress tracking
//! in scrollable viewports.

use flui_types::layout::Axis;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Common scroll metrics interface.
///
/// Provides shared functionality for tracking scroll position, bounds,
/// and progress in scrollable content.
pub trait ScrollMetrics {
    /// Minimum scroll extent (usually 0.0).
    fn min_scroll_extent(&self) -> f32;

    /// Maximum scroll extent (content size - viewport size).
    fn max_scroll_extent(&self) -> f32;

    /// Current scroll offset in pixels.
    fn pixels(&self) -> f32;

    /// Size of the viewport in the scroll axis.
    fn viewport_dimension(&self) -> f32;

    /// Scroll axis direction.
    fn axis(&self) -> Axis;

    // Derived queries with default implementations

    /// Returns whether scroll is at minimum extent.
    #[inline]
    fn at_start(&self) -> bool {
        self.pixels() <= self.min_scroll_extent()
    }

    /// Returns whether scroll is at maximum extent.
    #[inline]
    fn at_end(&self) -> bool {
        self.pixels() >= self.max_scroll_extent()
    }

    /// Returns whether scroll is within bounds.
    #[inline]
    fn in_bounds(&self) -> bool {
        self.pixels() >= self.min_scroll_extent() && self.pixels() <= self.max_scroll_extent()
    }

    /// Returns whether scroll is out of bounds.
    #[inline]
    fn out_of_bounds(&self) -> bool {
        !self.in_bounds()
    }

    /// Returns the out-of-bounds distance (0.0 if in bounds).
    #[inline]
    fn out_of_bounds_distance(&self) -> f32 {
        if self.pixels() < self.min_scroll_extent() {
            self.min_scroll_extent() - self.pixels()
        } else if self.pixels() > self.max_scroll_extent() {
            self.pixels() - self.max_scroll_extent()
        } else {
            0.0
        }
    }

    /// Returns scroll progress as a fraction (0.0 to 1.0).
    ///
    /// Returns 0.0 if there's no scrollable range.
    #[inline]
    fn scroll_progress(&self) -> f32 {
        let range = self.max_scroll_extent() - self.min_scroll_extent();
        if range > 0.0 {
            ((self.pixels() - self.min_scroll_extent()) / range).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Returns the total scrollable extent.
    #[inline]
    fn scroll_extent(&self) -> f32 {
        self.max_scroll_extent() - self.min_scroll_extent()
    }

    /// Returns the amount of content before the viewport.
    #[inline]
    fn before_viewport(&self) -> f32 {
        (self.pixels() - self.min_scroll_extent()).max(0.0)
    }

    /// Returns the amount of content after the viewport.
    #[inline]
    fn after_viewport(&self) -> f32 {
        (self.max_scroll_extent() - self.pixels() - self.viewport_dimension()).max(0.0)
    }
}

// ============================================================================
// FIXED SCROLL METRICS
// ============================================================================

/// Basic scroll metrics with fixed boundaries.
///
/// Tracks scroll position within defined min/max bounds without
/// any assumptions about content structure.
#[derive(Clone, Copy, PartialEq)]
pub struct FixedScrollMetrics {
    /// Minimum scroll offset (typically 0.0 or negative for overscroll).
    pub min_scroll_extent: f32,
    /// Maximum scroll offset (content size minus viewport size).
    pub max_scroll_extent: f32,
    /// Current scroll position in logical pixels.
    pub pixels: f32,
    /// Size of the viewport along the scroll axis.
    pub viewport_dimension: f32,
    /// The axis along which scrolling occurs.
    pub axis: Axis,
}

impl FixedScrollMetrics {
    /// Creates new fixed scroll metrics.
    #[inline]
    #[must_use]
    pub const fn new(
        min_scroll_extent: f32,
        max_scroll_extent: f32,
        pixels: f32,
        viewport_dimension: f32,
        axis: Axis,
    ) -> Self {
        Self {
            min_scroll_extent,
            max_scroll_extent,
            pixels,
            viewport_dimension,
            axis,
        }
    }

    /// Updates scroll position.
    #[inline]
    #[must_use]
    pub const fn with_pixels(mut self, pixels: f32) -> Self {
        self.pixels = pixels;
        self
    }

    /// Updates viewport dimension.
    #[inline]
    #[must_use]
    pub const fn with_viewport_dimension(mut self, dimension: f32) -> Self {
        self.viewport_dimension = dimension;
        self
    }
}

impl ScrollMetrics for FixedScrollMetrics {
    #[inline]
    fn min_scroll_extent(&self) -> f32 {
        self.min_scroll_extent
    }

    #[inline]
    fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    #[inline]
    fn pixels(&self) -> f32 {
        self.pixels
    }

    #[inline]
    fn viewport_dimension(&self) -> f32 {
        self.viewport_dimension
    }

    #[inline]
    fn axis(&self) -> Axis {
        self.axis
    }
}

impl Hash for FixedScrollMetrics {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min_scroll_extent.to_bits().hash(state);
        self.max_scroll_extent.to_bits().hash(state);
        self.pixels.to_bits().hash(state);
        self.viewport_dimension.to_bits().hash(state);
        self.axis.hash(state);
    }
}

impl Eq for FixedScrollMetrics {}

impl fmt::Debug for FixedScrollMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixedScrollMetrics")
            .field("pixels", &self.pixels)
            .field(
                "range",
                &format!("{}..{}", self.min_scroll_extent, self.max_scroll_extent),
            )
            .field(
                "progress",
                &format!("{:.1}%", self.scroll_progress() * 100.0),
            )
            .finish()
    }
}

// ============================================================================
// FIXED EXTENT METRICS
// ============================================================================

/// Scroll metrics with uniform item sizing.
///
/// Extends basic scroll tracking with item-based calculations,
/// assuming all items have the same extent.
#[derive(Clone, Copy, PartialEq)]
pub struct FixedExtentMetrics {
    /// Minimum scroll offset (typically 0.0 or negative for overscroll).
    pub min_scroll_extent: f32,
    /// Maximum scroll offset (content size minus viewport size).
    pub max_scroll_extent: f32,
    /// Current scroll position in logical pixels.
    pub pixels: f32,
    /// Size of the viewport along the scroll axis.
    pub viewport_dimension: f32,
    /// The axis along which scrolling occurs.
    pub axis: Axis,
    /// Size of each item along the scroll axis (all items have same size).
    pub item_extent: f32,
}

impl FixedExtentMetrics {
    /// Creates new fixed extent metrics.
    #[inline]
    #[must_use]
    pub const fn new(
        min_scroll_extent: f32,
        max_scroll_extent: f32,
        pixels: f32,
        viewport_dimension: f32,
        axis: Axis,
        item_extent: f32,
    ) -> Self {
        Self {
            min_scroll_extent,
            max_scroll_extent,
            pixels,
            viewport_dimension,
            axis,
            item_extent,
        }
    }

    /// Updates scroll position.
    #[inline]
    #[must_use]
    pub const fn with_pixels(mut self, pixels: f32) -> Self {
        self.pixels = pixels;
        self
    }

    /// Updates viewport dimension.
    #[inline]
    #[must_use]
    pub const fn with_viewport_dimension(mut self, dimension: f32) -> Self {
        self.viewport_dimension = dimension;
        self
    }

    // Item-specific calculations

    /// Returns the current item index based on scroll position.
    ///
    /// Calculates which item is at the leading edge of the viewport.
    #[inline]
    #[must_use]
    pub fn item_index(&self) -> usize {
        if self.item_extent > 0.0 {
            (self.pixels / self.item_extent).floor().max(0.0) as usize
        } else {
            0
        }
    }

    /// Returns the fractional offset within the current item.
    ///
    /// Value is in [0.0, item_extent).
    #[inline]
    #[must_use]
    pub fn item_offset(&self) -> f32 {
        if self.item_extent > 0.0 {
            self.pixels % self.item_extent
        } else {
            0.0
        }
    }

    /// Returns the total number of items in the scroll extent.
    #[inline]
    #[must_use]
    pub fn total_items(&self) -> usize {
        if self.item_extent > 0.0 {
            ((self.max_scroll_extent - self.min_scroll_extent) / self.item_extent).ceil() as usize
        } else {
            0
        }
    }

    /// Returns the number of items visible in the viewport.
    ///
    /// May include partially visible items.
    #[inline]
    #[must_use]
    pub fn visible_items(&self) -> usize {
        if self.item_extent > 0.0 {
            (self.viewport_dimension / self.item_extent).ceil() as usize
        } else {
            0
        }
    }
}

impl ScrollMetrics for FixedExtentMetrics {
    #[inline]
    fn min_scroll_extent(&self) -> f32 {
        self.min_scroll_extent
    }

    #[inline]
    fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    #[inline]
    fn pixels(&self) -> f32 {
        self.pixels
    }

    #[inline]
    fn viewport_dimension(&self) -> f32 {
        self.viewport_dimension
    }

    #[inline]
    fn axis(&self) -> Axis {
        self.axis
    }
}

impl Hash for FixedExtentMetrics {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min_scroll_extent.to_bits().hash(state);
        self.max_scroll_extent.to_bits().hash(state);
        self.pixels.to_bits().hash(state);
        self.viewport_dimension.to_bits().hash(state);
        self.axis.hash(state);
        self.item_extent.to_bits().hash(state);
    }
}

impl Eq for FixedExtentMetrics {}

impl fmt::Debug for FixedExtentMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixedExtentMetrics")
            .field("pixels", &self.pixels)
            .field("item", &format!("#{}", self.item_index()))
            .field(
                "progress",
                &format!("{:.1}%", self.scroll_progress() * 100.0),
            )
            .finish()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_scroll_metrics_trait() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, 500.0, 800.0, Axis::Vertical);

        assert_eq!(metrics.scroll_progress(), 0.5);
        assert!(!metrics.at_start());
        assert!(!metrics.at_end());
        assert!(metrics.in_bounds());
        assert_eq!(metrics.scroll_extent(), 1000.0);
    }

    #[test]
    fn test_bounds_checking() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, -50.0, 800.0, Axis::Vertical);

        assert!(metrics.out_of_bounds());
        assert_eq!(metrics.out_of_bounds_distance(), 50.0);

        let in_bounds = metrics.with_pixels(500.0);
        assert!(in_bounds.in_bounds());
        assert_eq!(in_bounds.out_of_bounds_distance(), 0.0);
    }

    #[test]
    fn test_fixed_extent_items() {
        let metrics = FixedExtentMetrics::new(0.0, 1000.0, 125.0, 800.0, Axis::Vertical, 50.0);

        assert_eq!(metrics.item_index(), 2);
        assert_eq!(metrics.item_offset(), 25.0);
        assert_eq!(metrics.total_items(), 20);
        assert_eq!(metrics.visible_items(), 16);
    }

    #[test]
    fn test_hash_equality() {
        let m1 = FixedScrollMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical);
        let m2 = FixedScrollMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical);
        let m3 = FixedScrollMetrics::new(0.0, 1000.0, 200.0, 800.0, Axis::Vertical);

        assert_eq!(m1, m2);
        assert_ne!(m1, m3);

        let mut set = HashSet::new();
        set.insert(m1);
        assert!(set.contains(&m2));
        assert!(!set.contains(&m3));
    }

    #[test]
    fn test_builder_pattern() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical)
            .with_pixels(500.0)
            .with_viewport_dimension(600.0);

        assert_eq!(metrics.pixels, 500.0);
        assert_eq!(metrics.viewport_dimension, 600.0);
    }
}
