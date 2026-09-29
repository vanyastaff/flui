//! `PerformanceOverlayLayer` — the frame-statistics readout the backend draws
//! over the scene.
//!
//! The layer carries the numbers to draw and where; it does not measure them.
//! The presentation that owns the frame clock samples fps / frame time /
//! frame count and hands them in through [`PerformanceOverlayLayer::update_stats`],
//! so no clock and no history ring lives in the compositor vocabulary.

use flui_foundation::geometry::Rect;

bitflags::bitflags! {
    /// Which readouts the overlay shows.
    ///
    /// The backend receives the set as-is; today it draws every readout
    /// regardless (`flui-app`'s config documents the option set as having no
    /// observable effect yet).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PerformanceOverlayOption: u32 {
        /// Frame time and FPS for the raster thread.
        const DISPLAY_RASTER_STATISTICS = 1 << 0;
        /// A histogram of raster-thread frame times.
        const VISUALIZE_RASTER_STATISTICS = 1 << 1;
        /// Frame time and FPS for the UI thread.
        const DISPLAY_ENGINE_STATISTICS = 1 << 2;
        /// A histogram of UI-thread frame times.
        const VISUALIZE_ENGINE_STATISTICS = 1 << 3;
    }
}

/// Frame statistics drawn at a fixed rectangle over the scene.
#[derive(Debug, Clone)]
pub struct PerformanceOverlayLayer {
    bounds: Rect<f64>,
    options: PerformanceOverlayOption,
    fps: f64,
    frame_time_ms: f64,
    total_frames: u64,
    /// One extra line of runtime diagnostics (present/input percentiles,
    /// dropped-frame counts) the presentation formats each frame.
    diagnostic_line: Option<String>,
}

impl PerformanceOverlayLayer {
    /// The top-left placement the presentation uses by default.
    #[must_use]
    pub fn default_bounds() -> Rect<f64> {
        Rect::from_ltwh(8.0, 8.0, 480.0, 58.0)
    }

    /// An overlay showing `options` inside `bounds`, with no samples yet.
    #[inline]
    pub fn new(bounds: Rect<f64>, options: PerformanceOverlayOption) -> Self {
        Self {
            bounds,
            options,
            fps: 0.0,
            frame_time_ms: 0.0,
            total_frames: 0,
            diagnostic_line: None,
        }
    }

    /// An overlay with every readout enabled.
    #[inline]
    pub fn all_stats(bounds: Rect<f64>) -> Self {
        Self::new(bounds, PerformanceOverlayOption::all())
    }

    /// Replaces the sampled numbers the overlay draws.
    pub fn update_stats(&mut self, fps: f64, frame_time_ms: f64, total_frames: u64) {
        self.fps = fps;
        self.frame_time_ms = frame_time_ms;
        self.total_frames = total_frames;
    }

    /// Frames per second, as last sampled.
    #[inline]
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Average frame time in milliseconds, as last sampled.
    #[inline]
    pub fn frame_time_ms(&self) -> f64 {
        self.frame_time_ms
    }

    /// Frames rendered so far, as last sampled.
    #[inline]
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// See the field doc on `diagnostic_line`.
    #[inline]
    pub fn diagnostic_line(&self) -> Option<&str> {
        self.diagnostic_line.as_deref()
    }

    /// Sets (or clears) the extra diagnostics line.
    pub fn set_diagnostic_line(&mut self, line: Option<String>) {
        self.diagnostic_line = line;
    }

    /// Which readouts to show.
    #[inline]
    pub fn options(&self) -> PerformanceOverlayOption {
        self.options
    }

    /// Where the overlay is drawn.
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
        self.bounds
    }
}
