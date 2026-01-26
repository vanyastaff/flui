//! Performance overlay layer - displays performance statistics
//!
//! This module provides the PerformanceOverlayLayer for displaying
//! performance metrics like frame timings, raster cache, and memory usage.

use flui_types::geometry::{Pixels, Rect};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Performance statistics for frame timing
///
/// Tracks frame times for calculating FPS and frame time statistics.
/// Uses a ring buffer to store recent frame times.
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    /// Recent frame durations (ring buffer)
    frame_times: VecDeque<Duration>,
    /// Maximum number of frames to track
    max_samples: usize,
    /// Last frame timestamp
    last_frame: Option<Instant>,
    /// Total frames rendered
    total_frames: u64,
}

impl Default for PerformanceStats {
    fn default() -> Self {
        Self::new(120) // 2 seconds at 60fps
    }
}

impl PerformanceStats {
    /// Create new performance stats with specified sample count
    pub fn new(max_samples: usize) -> Self {
        Self {
            frame_times: VecDeque::with_capacity(max_samples),
            max_samples,
            last_frame: None,
            total_frames: 0,
        }
    }

    /// Record a new frame
    pub fn record_frame(&mut self) {
        let now = Instant::now();

        if let Some(last) = self.last_frame {
            let duration = now.duration_since(last);

            if self.frame_times.len() >= self.max_samples {
                self.frame_times.pop_front();
            }
            self.frame_times.push_back(duration);
        }

        self.last_frame = Some(now);
        self.total_frames += 1;
    }

    /// Get current FPS (frames per second)
    pub fn fps(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }

        let total: Duration = self.frame_times.iter().sum();
        let avg_frame_time = total.as_secs_f32() / self.frame_times.len() as f32;

        if avg_frame_time > 0.0 {
            1.0 / avg_frame_time
        } else {
            0.0
        }
    }

    /// Get average frame time in milliseconds
    pub fn avg_frame_time_ms(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }

        let total: Duration = self.frame_times.iter().sum();
        total.as_secs_f32() * 1000.0 / self.frame_times.len() as f32
    }

    /// Get minimum frame time in milliseconds
    pub fn min_frame_time_ms(&self) -> f32 {
        self.frame_times
            .iter()
            .min()
            .map(|d| d.as_secs_f32() * 1000.0)
            .unwrap_or(0.0)
    }

    /// Get maximum frame time in milliseconds
    pub fn max_frame_time_ms(&self) -> f32 {
        self.frame_times
            .iter()
            .max()
            .map(|d| d.as_secs_f32() * 1000.0)
            .unwrap_or(0.0)
    }

    /// Get total frames rendered
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get frame times for histogram visualization
    pub fn frame_times_ms(&self) -> impl Iterator<Item = f32> + '_ {
        self.frame_times.iter().map(|d| d.as_secs_f32() * 1000.0)
    }

    /// Get number of recorded samples
    pub fn sample_count(&self) -> usize {
        self.frame_times.len()
    }

    /// Reset all statistics
    pub fn reset(&mut self) {
        self.frame_times.clear();
        self.last_frame = None;
        self.total_frames = 0;
    }
}

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
    overlay_rect: Rect<Pixels>,

    /// The options mask controlling what is displayed.
    options: PerformanceOverlayOption,

    /// Whether this layer needs to be re-added to scene.
    needs_add_to_scene: bool,

    /// Cached FPS value for rendering
    cached_fps: f32,

    /// Cached average frame time in ms
    cached_frame_time_ms: f32,

    /// Cached total frame count
    cached_total_frames: u64,
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
    pub fn new(overlay_rect: Rect<Pixels>, options: PerformanceOverlayOption) -> Self {
        Self {
            overlay_rect,
            options,
            needs_add_to_scene: true,
            cached_fps: 0.0,
            cached_frame_time_ms: 0.0,
            cached_total_frames: 0,
        }
    }

    /// Update the overlay with current performance statistics
    pub fn update_stats(&mut self, stats: &PerformanceStats) {
        self.cached_fps = stats.fps();
        self.cached_frame_time_ms = stats.avg_frame_time_ms();
        self.cached_total_frames = stats.total_frames();
        self.needs_add_to_scene = true;
    }

    /// Get cached FPS value
    #[inline]
    pub fn fps(&self) -> f32 {
        self.cached_fps
    }

    /// Get cached frame time in milliseconds
    #[inline]
    pub fn frame_time_ms(&self) -> f32 {
        self.cached_frame_time_ms
    }

    /// Get cached total frame count
    #[inline]
    pub fn total_frames(&self) -> u64 {
        self.cached_total_frames
    }

    /// Creates a performance overlay showing all raster statistics.
    #[inline]
    pub fn raster_stats(overlay_rect: Rect<Pixels>) -> Self {
        Self::new(
            overlay_rect,
            PerformanceOverlayOption::DISPLAY_RASTER_STATISTICS
                | PerformanceOverlayOption::VISUALIZE_RASTER_STATISTICS,
        )
    }

    /// Creates a performance overlay showing all engine statistics.
    #[inline]
    pub fn engine_stats(overlay_rect: Rect<Pixels>) -> Self {
        Self::new(
            overlay_rect,
            PerformanceOverlayOption::DISPLAY_ENGINE_STATISTICS
                | PerformanceOverlayOption::VISUALIZE_ENGINE_STATISTICS,
        )
    }

    /// Creates a performance overlay showing all available statistics.
    #[inline]
    pub fn all_stats(overlay_rect: Rect<Pixels>) -> Self {
        Self::new(overlay_rect, PerformanceOverlayOption::all())
    }

    /// Returns the overlay rectangle.
    #[inline]
    pub fn overlay_rect(&self) -> Rect<Pixels> {
        self.overlay_rect
    }

    /// Sets the overlay rectangle.
    ///
    /// This marks the layer as needing to be re-added to the scene.
    #[inline]
    pub fn set_overlay_rect(&mut self, rect: Rect<Pixels>) {
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
    pub fn bounds(&self) -> Rect<Pixels> {
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
        Self {
            overlay_rect: Rect::ZERO,
            options: PerformanceOverlayOption::empty(),
            needs_add_to_scene: true,
            cached_fps: 0.0,
            cached_frame_time_ms: 0.0,
            cached_total_frames: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

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
        let rect = Rect::from_xywh(px(10.0), px(10.0), px(200.0), px(100.0));
        let options = PerformanceOverlayOption::all();
        let layer = PerformanceOverlayLayer::new(rect, options);

        assert_eq!(layer.overlay_rect(), rect);
        assert_eq!(layer.options(), options);
        assert!(layer.needs_add_to_scene());
    }

    #[test]
    fn test_performance_overlay_layer_raster_stats() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0));
        let layer = PerformanceOverlayLayer::raster_stats(rect);

        assert!(layer.options().displays_raster_statistics());
        assert!(layer.options().visualizes_raster_statistics());
        assert!(!layer.options().displays_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_engine_stats() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0));
        let layer = PerformanceOverlayLayer::engine_stats(rect);

        assert!(!layer.options().displays_raster_statistics());
        assert!(layer.options().displays_engine_statistics());
        assert!(layer.options().visualizes_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_all_stats() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0));
        let layer = PerformanceOverlayLayer::all_stats(rect);

        assert!(layer.options().displays_raster_statistics());
        assert!(layer.options().visualizes_raster_statistics());
        assert!(layer.options().displays_engine_statistics());
        assert!(layer.options().visualizes_engine_statistics());
    }

    #[test]
    fn test_performance_overlay_layer_set_rect() {
        let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0));
        let rect2 = Rect::from_xywh(px(10.0), px(10.0), px(200.0), px(100.0));

        let mut layer = PerformanceOverlayLayer::new(rect1, PerformanceOverlayOption::empty());
        layer.clear_needs_add_to_scene();
        assert!(!layer.needs_add_to_scene());

        layer.set_overlay_rect(rect2);
        assert!(layer.needs_add_to_scene());
        assert_eq!(layer.overlay_rect(), rect2);
    }

    #[test]
    fn test_performance_overlay_layer_set_options() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(50.0));
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
