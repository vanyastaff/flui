//! Scroll metrics types
//!
//! This module provides types that describe the scroll position and extents
//! of scrollable areas.

use crate::layout::Axis;

/// Scroll metrics with a fixed item extent
///
/// Similar to Flutter's `FixedExtentMetrics`. Describes the scroll position
/// in a scrollable area where all items have the same extent.
///
/// # Examples
///
/// ```
/// use flui_types::constraints::FixedExtentMetrics;
/// use flui_types::layout::Axis;
///
/// let metrics = FixedExtentMetrics::new(
///     0.0,        // min_scroll_extent
///     1000.0,     // max_scroll_extent
///     100.0,      // pixels (current scroll offset)
///     800.0,      // viewport_dimension
///     Axis::Vertical,
///     50.0,       // item_extent
/// );
///
/// assert_eq!(metrics.item_index(), 2);
/// assert_eq!(metrics.item_extent, 50.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FixedExtentMetrics {
    /// The minimum in-range value for `pixels`
    ///
    /// The actual `pixels` value might be outside this range if the scroll
    /// position is out of bounds.
    pub min_scroll_extent: f32,

    /// The maximum in-range value for `pixels`
    ///
    /// The actual `pixels` value might be outside this range if the scroll
    /// position is out of bounds.
    pub max_scroll_extent: f32,

    /// The current scroll offset
    ///
    /// This is the distance from the start of the scrollable area to the
    /// leading edge of the viewport.
    pub pixels: f32,

    /// The extent of the viewport along the scroll axis
    pub viewport_dimension: f32,

    /// The axis along which the viewport scrolls
    pub axis: Axis,

    /// The fixed extent of each item
    ///
    /// All items in the scrollable area have this extent along the main axis.
    pub item_extent: f32,
}

impl FixedExtentMetrics {
    /// Creates new fixed extent metrics
    #[inline]
    #[must_use]
    pub fn new(
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

    /// Returns the current item index based on the scroll position
    ///
    /// This is calculated as `floor(pixels / item_extent)`.
    #[inline]
    #[must_use]
    pub fn item_index(&self) -> usize {
        if self.item_extent > 0.0 {
            (self.pixels / self.item_extent).floor().max(0.0) as usize
        } else {
            0
        }
    }

    /// Returns the fractional part of the current scroll position
    ///
    /// This is the offset within the current item, in the range [0.0, 1.0).
    #[inline]
    #[must_use]
    pub fn item_offset_fraction(&self) -> f32 {
        if self.item_extent > 0.0 {
            (self.pixels % self.item_extent) / self.item_extent
        } else {
            0.0
        }
    }

    /// Returns how many items are fully or partially visible in the viewport
    #[inline]
    #[must_use]
    pub fn visible_item_count(&self) -> usize {
        if self.item_extent > 0.0 {
            (self.viewport_dimension / self.item_extent).ceil() as usize + 1
        } else {
            0
        }
    }

    /// Returns the total number of items that can be scrolled through
    #[inline]
    #[must_use]
    pub fn total_item_count(&self) -> usize {
        if self.item_extent > 0.0 {
            ((self.max_scroll_extent - self.min_scroll_extent) / self.item_extent).ceil() as usize
        } else {
            0
        }
    }

    /// Returns whether the scroll position is at the start
    #[inline]
    #[must_use]
    pub fn at_edge_start(&self) -> bool {
        self.pixels <= self.min_scroll_extent
    }

    /// Returns whether the scroll position is at the end
    #[inline]
    #[must_use]
    pub fn at_edge_end(&self) -> bool {
        self.pixels >= self.max_scroll_extent
    }

    /// Returns whether the scroll position is out of bounds
    #[inline]
    #[must_use]
    pub fn out_of_range(&self) -> bool {
        self.pixels < self.min_scroll_extent || self.pixels > self.max_scroll_extent
    }

    /// Returns the amount of overscroll at the start
    #[inline]
    #[must_use]
    pub fn overscroll_start(&self) -> f32 {
        if self.pixels < self.min_scroll_extent {
            self.min_scroll_extent - self.pixels
        } else {
            0.0
        }
    }

    /// Returns the amount of overscroll at the end
    #[inline]
    #[must_use]
    pub fn overscroll_end(&self) -> f32 {
        if self.pixels > self.max_scroll_extent {
            self.pixels - self.max_scroll_extent
        } else {
            0.0
        }
    }

    /// Returns the total scrollable extent
    #[inline]
    #[must_use]
    pub fn extent(&self) -> f32 {
        self.max_scroll_extent - self.min_scroll_extent
    }

    // ===== Helper methods for layout and rendering =====

    /// Returns the pixel offset of a specific item
    #[inline]
    #[must_use]
    pub fn item_offset(&self, index: usize) -> f32 {
        self.min_scroll_extent + (index as f32 * self.item_extent)
    }

    /// Returns the range of visible item indices (start, end)
    #[inline]
    #[must_use]
    pub fn visible_item_range(&self) -> (usize, usize) {
        let start = self.item_index();
        let end = start + self.visible_item_count();
        (start, end.min(self.total_item_count()))
    }

    /// Clamp a scroll position to valid bounds
    #[inline]
    #[must_use]
    pub fn clamp_pixels(&self, pixels: f32) -> f32 {
        pixels.clamp(self.min_scroll_extent, self.max_scroll_extent)
    }

    /// Snap to the nearest item boundary
    #[inline]
    #[must_use]
    pub fn snap_to_item(&self) -> f32 {
        if self.item_extent > 0.0 {
            (self.pixels / self.item_extent).round() * self.item_extent
        } else {
            self.pixels
        }
    }

    /// Returns whether this metrics represents infinite scrolling (no bounds)
    #[inline]
    #[must_use]
    pub fn is_infinite(&self) -> bool {
        self.max_scroll_extent.is_infinite()
    }
}

/// Fixed scroll metrics
///
/// Similar to Flutter's `FixedScrollMetrics`. Describes the scroll position
/// in a scrollable area with fixed boundaries.
///
/// # Examples
///
/// ```
/// use flui_types::constraints::FixedScrollMetrics;
/// use flui_types::layout::Axis;
///
/// let metrics = FixedScrollMetrics::new(
///     0.0,        // min_scroll_extent
///     1000.0,     // max_scroll_extent
///     100.0,      // pixels (current scroll offset)
///     800.0,      // viewport_dimension
///     Axis::Vertical,
/// );
///
/// assert_eq!(metrics.pixels, 100.0);
/// assert!(!metrics.at_edge_start());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FixedScrollMetrics {
    /// The minimum in-range value for `pixels`
    pub min_scroll_extent: f32,

    /// The maximum in-range value for `pixels`
    pub max_scroll_extent: f32,

    /// The current scroll offset in pixels
    pub pixels: f32,

    /// The extent of the viewport along the scroll axis
    pub viewport_dimension: f32,

    /// The axis along which the viewport scrolls
    pub axis: Axis,
}

impl FixedScrollMetrics {
    /// Creates new fixed scroll metrics
    #[inline]
    #[must_use]
    pub fn new(
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

    /// Returns the total scrollable extent
    #[inline]
    #[must_use]
    pub fn extent(&self) -> f32 {
        self.max_scroll_extent - self.min_scroll_extent
    }

    /// Returns whether the scroll position is at the start
    #[inline]
    #[must_use]
    pub fn at_edge_start(&self) -> bool {
        self.pixels <= self.min_scroll_extent
    }

    /// Returns whether the scroll position is at the end
    #[inline]
    #[must_use]
    pub fn at_edge_end(&self) -> bool {
        self.pixels >= self.max_scroll_extent
    }

    /// Returns whether the scroll position is out of bounds
    #[inline]
    #[must_use]
    pub fn out_of_range(&self) -> bool {
        self.pixels < self.min_scroll_extent || self.pixels > self.max_scroll_extent
    }

    /// Returns the amount of overscroll at the start
    #[inline]
    #[must_use]
    pub fn overscroll_start(&self) -> f32 {
        if self.pixels < self.min_scroll_extent {
            self.min_scroll_extent - self.pixels
        } else {
            0.0
        }
    }

    /// Returns the amount of overscroll at the end
    #[inline]
    #[must_use]
    pub fn overscroll_end(&self) -> f32 {
        if self.pixels > self.max_scroll_extent {
            self.pixels - self.max_scroll_extent
        } else {
            0.0
        }
    }

    /// Returns the fraction of the scrollable extent that is currently scrolled
    ///
    /// Returns a value between 0.0 (at start) and 1.0 (at end).
    /// Returns 0.0 if the extent is zero.
    #[inline]
    #[must_use]
    pub fn scroll_fraction(&self) -> f32 {
        let extent = self.extent();
        if extent > 0.0 {
            ((self.pixels - self.min_scroll_extent) / extent).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Returns the number of viewport-sized pages in the scrollable area
    #[inline]
    #[must_use]
    pub fn page_count(&self) -> f32 {
        if self.viewport_dimension > 0.0 {
            self.extent() / self.viewport_dimension
        } else {
            0.0
        }
    }

    /// Returns the current page index (0-based)
    ///
    /// This is calculated based on the viewport dimension.
    #[inline]
    #[must_use]
    pub fn current_page(&self) -> usize {
        if self.viewport_dimension > 0.0 {
            ((self.pixels - self.min_scroll_extent) / self.viewport_dimension)
                .floor()
                .max(0.0) as usize
        } else {
            0
        }
    }

    /// Returns the amount of content visible beyond the trailing edge
    #[inline]
    #[must_use]
    pub fn trailing_content(&self) -> f32 {
        (self.max_scroll_extent - self.pixels).max(0.0)
    }

    /// Returns the amount of content before the leading edge
    #[inline]
    #[must_use]
    pub fn leading_content(&self) -> f32 {
        (self.pixels - self.min_scroll_extent).max(0.0)
    }

    // ===== Helper methods for layout and rendering =====

    /// Clamp a scroll position to valid bounds
    #[inline]
    #[must_use]
    pub fn clamp_pixels(&self, pixels: f32) -> f32 {
        pixels.clamp(self.min_scroll_extent, self.max_scroll_extent)
    }

    /// Returns whether this metrics represents infinite scrolling (no bounds)
    #[inline]
    #[must_use]
    pub fn is_infinite(&self) -> bool {
        self.max_scroll_extent.is_infinite()
    }

    /// Returns whether the scroll is within valid bounds
    #[inline]
    #[must_use]
    pub fn is_in_range(&self) -> bool {
        !self.out_of_range()
    }

    /// Scroll to a specific fraction (0.0 = start, 1.0 = end)
    #[inline]
    #[must_use]
    pub fn pixels_from_fraction(&self, fraction: f32) -> f32 {
        self.min_scroll_extent + (self.extent() * fraction.clamp(0.0, 1.0))
    }

    /// Scroll to a specific page index
    #[inline]
    #[must_use]
    pub fn pixels_from_page(&self, page: usize) -> f32 {
        self.min_scroll_extent + (page as f32 * self.viewport_dimension)
    }

    /// Returns the total amount of overscroll (start + end)
    #[inline]
    #[must_use]
    pub fn total_overscroll(&self) -> f32 {
        self.overscroll_start() + self.overscroll_end()
    }

    /// Returns whether the viewport can scroll (content exceeds viewport)
    #[inline]
    #[must_use]
    pub fn can_scroll(&self) -> bool {
        self.extent() > self.viewport_dimension
    }

    /// Copy metrics with new pixel offset
    #[inline]
    #[must_use]
    pub fn with_pixels(&self, pixels: f32) -> Self {
        Self {
            pixels,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_extent_metrics_new() {
        let metrics = FixedExtentMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical, 50.0);

        assert_eq!(metrics.min_scroll_extent, 0.0);
        assert_eq!(metrics.max_scroll_extent, 1000.0);
        assert_eq!(metrics.pixels, 100.0);
        assert_eq!(metrics.viewport_dimension, 800.0);
        assert_eq!(metrics.axis, Axis::Vertical);
        assert_eq!(metrics.item_extent, 50.0);
    }

    #[test]
    fn test_fixed_extent_metrics_item_index() {
        let metrics = FixedExtentMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(metrics.item_index(), 2);

        let metrics2 = FixedExtentMetrics::new(0.0, 1000.0, 125.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(metrics2.item_index(), 2);

        let metrics3 = FixedExtentMetrics::new(0.0, 1000.0, 150.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(metrics3.item_index(), 3);
    }

    #[test]
    fn test_fixed_extent_metrics_visible_count() {
        let metrics = FixedExtentMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(metrics.visible_item_count(), 17); // ceil(800/50) + 1
    }

    #[test]
    fn test_fixed_extent_metrics_total_count() {
        let metrics = FixedExtentMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(metrics.total_item_count(), 20); // ceil(1000/50)
    }

    #[test]
    fn test_fixed_extent_metrics_edges() {
        let at_start = FixedExtentMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical, 50.0);
        assert!(at_start.at_edge_start());
        assert!(!at_start.at_edge_end());

        let at_end = FixedExtentMetrics::new(0.0, 1000.0, 1000.0, 800.0, Axis::Vertical, 50.0);
        assert!(!at_end.at_edge_start());
        assert!(at_end.at_edge_end());

        let in_middle = FixedExtentMetrics::new(0.0, 1000.0, 500.0, 800.0, Axis::Vertical, 50.0);
        assert!(!in_middle.at_edge_start());
        assert!(!in_middle.at_edge_end());
    }

    #[test]
    fn test_fixed_extent_metrics_overscroll() {
        let overscroll_start =
            FixedExtentMetrics::new(0.0, 1000.0, -50.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(overscroll_start.overscroll_start(), 50.0);
        assert_eq!(overscroll_start.overscroll_end(), 0.0);
        assert!(overscroll_start.out_of_range());

        let overscroll_end =
            FixedExtentMetrics::new(0.0, 1000.0, 1050.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(overscroll_end.overscroll_start(), 0.0);
        assert_eq!(overscroll_end.overscroll_end(), 50.0);
        assert!(overscroll_end.out_of_range());

        let in_range = FixedExtentMetrics::new(0.0, 1000.0, 500.0, 800.0, Axis::Vertical, 50.0);
        assert_eq!(in_range.overscroll_start(), 0.0);
        assert_eq!(in_range.overscroll_end(), 0.0);
        assert!(!in_range.out_of_range());
    }

    #[test]
    fn test_fixed_scroll_metrics_new() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical);

        assert_eq!(metrics.min_scroll_extent, 0.0);
        assert_eq!(metrics.max_scroll_extent, 1000.0);
        assert_eq!(metrics.pixels, 100.0);
        assert_eq!(metrics.viewport_dimension, 800.0);
        assert_eq!(metrics.axis, Axis::Vertical);
    }

    #[test]
    fn test_fixed_scroll_metrics_extent() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, 100.0, 800.0, Axis::Vertical);
        assert_eq!(metrics.extent(), 1000.0);

        let metrics2 = FixedScrollMetrics::new(100.0, 900.0, 200.0, 800.0, Axis::Vertical);
        assert_eq!(metrics2.extent(), 800.0);
    }

    #[test]
    fn test_fixed_scroll_metrics_edges() {
        let at_start = FixedScrollMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical);
        assert!(at_start.at_edge_start());
        assert!(!at_start.at_edge_end());

        let at_end = FixedScrollMetrics::new(0.0, 1000.0, 1000.0, 800.0, Axis::Vertical);
        assert!(!at_end.at_edge_start());
        assert!(at_end.at_edge_end());
    }

    #[test]
    fn test_fixed_scroll_metrics_scroll_fraction() {
        let at_start = FixedScrollMetrics::new(0.0, 1000.0, 0.0, 800.0, Axis::Vertical);
        assert_eq!(at_start.scroll_fraction(), 0.0);

        let at_middle = FixedScrollMetrics::new(0.0, 1000.0, 500.0, 800.0, Axis::Vertical);
        assert_eq!(at_middle.scroll_fraction(), 0.5);

        let at_end = FixedScrollMetrics::new(0.0, 1000.0, 1000.0, 800.0, Axis::Vertical);
        assert_eq!(at_end.scroll_fraction(), 1.0);
    }

    #[test]
    fn test_fixed_scroll_metrics_page_count() {
        let metrics = FixedScrollMetrics::new(0.0, 1600.0, 0.0, 800.0, Axis::Vertical);
        assert_eq!(metrics.page_count(), 2.0);

        let metrics2 = FixedScrollMetrics::new(0.0, 1000.0, 0.0, 400.0, Axis::Vertical);
        assert_eq!(metrics2.page_count(), 2.5);
    }

    #[test]
    fn test_fixed_scroll_metrics_current_page() {
        let page0 = FixedScrollMetrics::new(0.0, 1600.0, 0.0, 800.0, Axis::Vertical);
        assert_eq!(page0.current_page(), 0);

        let page1 = FixedScrollMetrics::new(0.0, 1600.0, 800.0, 800.0, Axis::Vertical);
        assert_eq!(page1.current_page(), 1);

        let page1_partial = FixedScrollMetrics::new(0.0, 1600.0, 850.0, 800.0, Axis::Vertical);
        assert_eq!(page1_partial.current_page(), 1);
    }

    #[test]
    fn test_fixed_scroll_metrics_overscroll() {
        let overscroll_start = FixedScrollMetrics::new(0.0, 1000.0, -50.0, 800.0, Axis::Vertical);
        assert_eq!(overscroll_start.overscroll_start(), 50.0);
        assert_eq!(overscroll_start.overscroll_end(), 0.0);

        let overscroll_end = FixedScrollMetrics::new(0.0, 1000.0, 1050.0, 800.0, Axis::Vertical);
        assert_eq!(overscroll_end.overscroll_start(), 0.0);
        assert_eq!(overscroll_end.overscroll_end(), 50.0);
    }

    #[test]
    fn test_fixed_scroll_metrics_content() {
        let metrics = FixedScrollMetrics::new(0.0, 1000.0, 200.0, 800.0, Axis::Vertical);
        assert_eq!(metrics.leading_content(), 200.0);
        assert_eq!(metrics.trailing_content(), 800.0);
    }
}
