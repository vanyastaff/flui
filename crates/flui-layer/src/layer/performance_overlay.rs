//! Performance overlay layer - displays performance statistics
//!
//! This module provides the PerformanceOverlayLayer for displaying
//! performance metrics like frame timings, raster cache, and memory usage.

use flui_types::geometry::Rect;

/// Options for what to display in the performance overlay.
///
/// These flags can be combined to show multiple types of performance
/// information simultaneously.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PerformanceOverlayOption(u32);

impl PerformanceOverlayOption {
    /// Display the frame time and FPS for the raster thread.
    pub const DISPLAY_RASTER_STATISTICS: Self = Self(1 << 0);

    /// Display a histogram of raster thread frame times.
    pub const VISUALIZE_RASTER_STATISTICS: Self = Self(1 << 1);

    /// Display the frame time and FPS for the UI thread.
    pub const DISPLAY_ENGINE_STATISTICS: Self = Self(1 << 2);

    /// Display a histogram of UI thread frame times.
    pub const VISUALIZE_ENGINE_STATISTICS: Self = Self(1 << 3);

    /// No options enabled.
    pub const NONE: Self = Self(0);

    /// All options enabled.
    pub const ALL: Self = Self(0b1111);

    /// Creates an empty options set.
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates options with all flags set.
    #[inline]
    pub const fn all() -> Self {
        Self::ALL
    }

    /// Returns the combined mask as an integer.
    #[inline]
    pub const fn as_mask(self) -> u32 {
        self.0
    }

    /// Creates options from a raw mask.
    #[inline]
    pub const fn from_mask(mask: u32) -> Self {
        Self(mask)
    }

    /// Returns true if this set is empty.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Combines two option sets (bitwise OR).
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns true if self contains all flags in other.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns true if raster statistics should be displayed.
    #[inline]
    pub const fn displays_raster_statistics(self) -> bool {
        self.contains(Self::DISPLAY_RASTER_STATISTICS)
    }

    /// Returns true if raster statistics should be visualized as histogram.
    #[inline]
    pub const fn visualizes_raster_statistics(self) -> bool {
        self.contains(Self::VISUALIZE_RASTER_STATISTICS)
    }

    /// Returns true if engine statistics should be displayed.
    #[inline]
    pub const fn displays_engine_statistics(self) -> bool {
        self.contains(Self::DISPLAY_ENGINE_STATISTICS)
    }

    /// Returns true if engine statistics should be visualized as histogram.
    #[inline]
    pub const fn visualizes_engine_statistics(self) -> bool {
        self.contains(Self::VISUALIZE_ENGINE_STATISTICS)
    }
}

impl std::ops::BitOr for PerformanceOverlayOption {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for PerformanceOverlayOption {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// A layer that displays a performance overlay.
///
/// The performance overlay displays timing and memory statistics
/// to help diagnose performance issues.
///
/// # Example
///
/// ```rust
/// use flui_layer::{PerformanceOverlayLayer, PerformanceOverlayOption};
/// use flui_types::geometry::Rect;
///
/// let overlay = PerformanceOverlayLayer::new(
///     Rect::from_xywh(10.0, 10.0, 200.0, 100.0),
///     PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
///         | PerformanceOverlayOption::VISUALIZE_RASTER_STATISTICS,
/// );
///
/// assert!(overlay.options().displays_raster_statistics());
/// ```
#[derive(Debug, Clone)]
pub struct PerformanceOverlayLayer {
    /// The rectangle where the overlay is displayed.
    overlay_rect: Rect,

    /// The options mask controlling what is displayed.
    options: PerformanceOverlayOption,

    /// Whether this layer needs to be re-added to scene.
    needs_add_to_scene: bool,
}

impl PerformanceOverlayLayer {
    /// Creates a new performance overlay layer.
    ///
    /// # Arguments
    ///
    /// * `overlay_rect` - The rectangle in the layer's coordinate system where
    ///   the overlay should be displayed.
    /// * `options` - The options controlling what statistics to display.
    #[inline]
    pub fn new(overlay_rect: Rect, options: PerformanceOverlayOption) -> Self {
        Self {
            overlay_rect,
            options,
            needs_add_to_scene: true,
        }
    }

    /// Creates a performance overlay showing all raster statistics.
    #[inline]
    pub fn raster_stats(overlay_rect: Rect) -> Self {
        Self::new(
            overlay_rect,
            PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
                | PerformanceOverlayOption::VISUALIZE_RASTER_STATISTICS,
        )
    }

    /// Creates a performance overlay showing all engine statistics.
    #[inline]
    pub fn engine_stats(overlay_rect: Rect) -> Self {
        Self::new(
            overlay_rect,
            PerformanceOverlayOption::DISPLAY_ENGINE_STATISTICS
                | PerformanceOverlayOption::VISUALIZE_ENGINE_STATISTICS,
        )
    }

    /// Creates a performance overlay showing all available statistics.
    #[inline]
    pub fn all_stats(overlay_rect: Rect) -> Self {
        Self::new(overlay_rect, PerformanceOverlayOption::all())
    }

    /// Returns the overlay rectangle.
    #[inline]
    pub fn overlay_rect(&self) -> Rect {
        self.overlay_rect
    }

    /// Sets the overlay rectangle.
    ///
    /// This marks the layer as needing to be re-added to the scene.
    #[inline]
    pub fn set_overlay_rect(&mut self, rect: Rect) {
        if self.overlay_rect != rect {
            self.overlay_rect = rect;
            self.needs_add_to_scene = true;
        }
    }

    /// Returns the options mask.
    #[inline]
    pub fn options(&self) -> PerformanceOverlayOption {
        self.options
    }

    /// Returns the options as a raw mask.
    #[inline]
    pub fn options_mask(&self) -> u32 {
        self.options.as_mask()
    }

    /// Sets the options.
    ///
    /// This marks the layer as needing to be re-added to the scene.
    #[inline]
    pub fn set_options(&mut self, options: PerformanceOverlayOption) {
        if self.options != options {
            self.options = options;
            self.needs_add_to_scene = true;
        }
    }

    /// Returns the bounds of this layer.
    #[inline]
    pub fn bounds(&self) -> Rect {
        self.overlay_rect
    }

    /// Returns true if this layer needs to be re-added to the scene.
    #[inline]
    pub fn needs_add_to_scene(&self) -> bool {
        self.needs_add_to_scene
    }

    /// Marks this layer as needing to be re-added to the scene.
    #[inline]
    pub fn mark_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = true;
    }

    /// Clears the needs_add_to_scene flag.
    #[inline]
    pub fn clear_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = false;
    }
}

impl Default for PerformanceOverlayLayer {
    fn default() -> Self {
        Self::new(Rect::ZERO, PerformanceOverlayOption::empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_overlay_option_flags() {
        let options = PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
            | PerformanceOverlayOption::VISUALIZE_RASTER_STATISTICS;

        assert!(options.displays_raster_statistics());
        assert!(options.visualizes_raster_statistics());
        assert!(!options.displays_engine_statistics());
        assert!(!options.visualizes_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_new() {
        let rect = Rect::from_xywh(10.0, 10.0, 200.0, 100.0);
        let options = PerformanceOverlayOption::all();
        let layer = PerformanceOverlayLayer::new(rect, options);

        assert_eq!(layer.overlay_rect(), rect);
        assert_eq!(layer.options(), options);
        assert!(layer.needs_add_to_scene());
    }

    #[test]
    fn test_performance_overlay_layer_raster_stats() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let layer = PerformanceOverlayLayer::raster_stats(rect);

        assert!(layer.options().displays_raster_statistics());
        assert!(layer.options().visualizes_raster_statistics());
        assert!(!layer.options().displays_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_engine_stats() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let layer = PerformanceOverlayLayer::engine_stats(rect);

        assert!(!layer.options().displays_raster_statistics());
        assert!(layer.options().displays_engine_statistics());
        assert!(layer.options().visualizes_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_all_stats() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let layer = PerformanceOverlayLayer::all_stats(rect);

        assert!(layer.options().displays_raster_statistics());
        assert!(layer.options().visualizes_raster_statistics());
        assert!(layer.options().displays_engine_statistics());
        assert!(layer.options().visualizes_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_set_rect() {
        let rect1 = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let rect2 = Rect::from_xywh(10.0, 10.0, 200.0, 100.0);

        let mut layer = PerformanceOverlayLayer::new(rect1, PerformanceOverlayOption::empty());
        layer.clear_needs_add_to_scene();
        assert!(!layer.needs_add_to_scene());

        layer.set_overlay_rect(rect2);
        assert!(layer.needs_add_to_scene());
        assert_eq!(layer.overlay_rect(), rect2);
    }

    #[test]
    fn test_performance_overlay_layer_set_options() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let mut layer = PerformanceOverlayLayer::new(rect, PerformanceOverlayOption::empty());
        layer.clear_needs_add_to_scene();

        layer.set_options(PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS);
        assert!(layer.needs_add_to_scene());
        assert!(layer.options().displays_raster_statistics());
    }

    #[test]
    fn test_options_mask() {
        let options = PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
            | PerformanceOverlayOption::DISPLAY_ENGINE_STATISTICS;

        let mask = options.as_mask();
        let restored = PerformanceOverlayOption::from_mask(mask);

        assert_eq!(options, restored);
    }

    #[test]
    fn test_empty_options() {
        let options = PerformanceOverlayOption::empty();
        assert!(options.is_empty());
        assert!(!options.displays_raster_statistics());
        assert!(!options.displays_engine_statistics());
    }
}
