//! DevTools integration for flui_engine
//!
//! This module provides integration between flui_engine and flui_devtools,
//! enabling performance profiling and debugging features.

#[cfg(feature = "devtools")]
use flui_devtools::profiler::Profiler;

#[cfg(feature = "devtools")]
use std::sync::Arc;

#[cfg(feature = "devtools")]
use parking_lot::Mutex;

/// Compositor with integrated profiler
///
/// This wraps the standard Compositor and adds profiling capabilities
/// via flui_devtools::Profiler.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::devtools::ProfiledCompositor;
///
/// let mut compositor = ProfiledCompositor::new();
///
/// // In your render loop:
/// compositor.begin_frame();
/// compositor.composite(&scene, &mut painter);
///
/// if let Some(stats) = compositor.end_frame() {
///     println!("Frame time: {:.2}ms", stats.total_time_ms());
///     println!("FPS: {:.1}", compositor.fps());
/// }
/// ```
#[cfg(feature = "devtools")]
pub struct ProfiledCompositor {
    /// Inner compositor
    compositor: crate::Compositor,

    /// Profiler instance
    profiler: Arc<Mutex<Profiler>>,
}

#[cfg(feature = "devtools")]
impl ProfiledCompositor {
    /// Create a new profiled compositor
    pub fn new() -> Self {
        Self {
            compositor: crate::Compositor::new(),
            profiler: Arc::new(Mutex::new(Profiler::new())),
        }
    }

    /// Create a profiled compositor with custom options
    pub fn with_options(options: crate::CompositorOptions) -> Self {
        Self {
            compositor: crate::Compositor::with_options(options),
            profiler: Arc::new(Mutex::new(Profiler::new())),
        }
    }

    /// Get reference to the inner compositor
    pub fn compositor(&self) -> &crate::Compositor {
        &self.compositor
    }

    /// Get mutable reference to the inner compositor
    pub fn compositor_mut(&mut self) -> &mut crate::Compositor {
        &mut self.compositor
    }

    /// Get reference to the profiler
    pub fn profiler(&self) -> Arc<Mutex<Profiler>> {
        Arc::clone(&self.profiler)
    }

    /// Begin a new frame for profiling
    ///
    /// Call this at the start of each frame, before any rendering work.
    pub fn begin_frame(&self) {
        self.profiler.lock().begin_frame();
    }

    /// Composite a scene with automatic profiling
    ///
    /// This wraps the standard composite() call with profiling markers
    /// for the Paint phase.
    ///
    /// Note: You must call begin_frame() before calling this method.
    ///
    /// # Arguments
    /// * `scene` - The scene to composite
    /// * `painter` - The painter to render with
    pub fn composite(&mut self, scene: &crate::Scene, painter: &mut dyn crate::Painter) {
        // Profile the paint phase
        let _guard = {
            let profiler = self.profiler.lock();
            profiler.profile_phase(flui_devtools::profiler::FramePhase::Paint)
            // MutexGuard is dropped here, but PhaseGuard is returned and kept alive
        };

        // Composite (guard will be dropped automatically after this)
        self.compositor.composite(scene, painter);

        // PhaseGuard is dropped here, recording the paint phase duration
    }

    /// End the current frame and get statistics
    ///
    /// Call this after all phases (Build, Layout, Paint) are complete.
    ///
    /// # Returns
    /// Frame statistics including timing for all phases (if available)
    pub fn end_frame(&mut self) -> Option<flui_devtools::profiler::FrameStats> {
        let profiler = self.profiler.lock();
        profiler.end_frame();
        profiler.frame_stats()
    }

    /// Get frame statistics without ending the frame
    pub fn frame_stats(&self) -> Option<flui_devtools::profiler::FrameStats> {
        self.profiler.lock().frame_stats()
    }

    /// Get composition statistics from the inner compositor
    pub fn composition_stats(&self) -> &crate::CompositionStats {
        self.compositor.stats()
    }

    /// Set the viewport for culling
    pub fn set_viewport(&mut self, viewport: flui_types::Rect) {
        self.compositor.set_viewport(viewport);
    }

    /// Enable or disable layer culling
    pub fn set_culling_enabled(&mut self, enabled: bool) {
        self.compositor.set_culling_enabled(enabled);
    }

    /// Check if the last frame was janky (>16ms for 60fps)
    ///
    /// Returns true if the last completed frame took longer than the target frame time.
    pub fn is_janky(&self) -> bool {
        if let Some(stats) = self.profiler.lock().frame_stats() {
            stats.is_jank()
        } else {
            false
        }
    }

    /// Get current FPS based on recent frame history
    pub fn fps(&self) -> f64 {
        self.profiler.lock().average_fps()
    }
}

#[cfg(feature = "devtools")]
impl Default for ProfiledCompositor {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export devtools types when feature is enabled
#[cfg(feature = "devtools")]
pub use flui_devtools::profiler::{FramePhase, FrameStats};

/// Performance overlay for rendering profiling information
///
/// This can be used to draw an FPS counter and frame timing overlay on top of the rendered content.
#[cfg(feature = "devtools")]
pub struct PerformanceOverlay {
    /// Show FPS counter
    pub show_fps: bool,

    /// Show frame time graph
    pub show_frame_time: bool,

    /// Show jank indicators
    pub show_jank: bool,

    /// Overlay position (0.0-1.0, relative to viewport)
    pub position: (f32, f32),
}

#[cfg(feature = "devtools")]
impl PerformanceOverlay {
    /// Create a new performance overlay with default settings
    pub fn new() -> Self {
        Self {
            show_fps: true,
            show_frame_time: true,
            show_jank: true,
            position: (0.02, 0.02), // Top-left corner
        }
    }

    /// Render the performance overlay
    ///
    /// This should be called after compositing the main scene, to draw the overlay on top.
    ///
    /// # Arguments
    /// * `profiler` - The profiler to get stats from
    /// * `painter` - The painter to draw with
    /// * `viewport_size` - The size of the viewport
    pub fn render(
        &self,
        profiler: &Profiler,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        use crate::Paint;
        use flui_types::{Point, Rect};

        // Get profiler stats
        let stats = profiler.frame_stats();
        let avg_fps = profiler.average_fps();
        let jank_pct = profiler.jank_percentage();

        // Position in pixels
        let x = viewport_size.width * self.position.0;
        let y = viewport_size.height * self.position.1;

        // Background dimensions
        let width = 180.0;
        let height = if stats.is_some() { 90.0 } else { 40.0 };

        // Draw background
        let bg_rect = Rect::from_xywh(x, y, width, height);
        let bg_paint = Paint {
            color: [0.0, 0.0, 0.0, 0.75], // Semi-transparent black
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Text paint (white)
        let text_paint = Paint {
            color: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        };

        let mut current_y = y + 15.0;
        let line_height = 18.0;
        let padding_x = x + 10.0;
        let font_size = 13.0;

        // Draw FPS
        if self.show_fps {
            let fps_text = format!("FPS: {:.1}", avg_fps);
            painter.text(&fps_text, Point::new(padding_x, current_y), font_size, &text_paint);
            current_y += line_height;
        }

        // Draw frame stats if available
        if let Some(stats) = stats {
            if self.show_frame_time {
                let time_text = format!("Frame: {:.2}ms", stats.total_time_ms());
                painter.text(&time_text, Point::new(padding_x, current_y), font_size, &text_paint);
                current_y += line_height;

                // Draw paint phase time if available
                if let Some(paint_phase) = stats.phase(FramePhase::Paint) {
                    let paint_text = format!("Paint: {:.2}ms", paint_phase.duration_ms());
                    painter.text(&paint_text, Point::new(padding_x, current_y), font_size, &text_paint);
                    current_y += line_height;
                }
            }

            // Draw jank indicator
            if self.show_jank {
                if stats.is_jank() {
                    let jank_paint = Paint {
                        color: [1.0, 0.3, 0.3, 1.0], // Red for jank
                        ..Default::default()
                    };
                    painter.text("⚠ JANK", Point::new(padding_x, current_y), font_size, &jank_paint);
                } else {
                    let jank_text = format!("Jank: {:.1}%", jank_pct);
                    painter.text(&jank_text, Point::new(padding_x, current_y), font_size, &text_paint);
                }
            }
        }

        painter.restore();
    }
}

#[cfg(feature = "devtools")]
impl Default for PerformanceOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Frame timeline graph - visual representation of frame history
///
/// Renders a mini-graph showing frame times over the last N frames,
/// making it easy to spot performance issues at a glance.
#[cfg(feature = "devtools")]
pub struct FrameTimelineGraph {
    /// Graph position (0.0-1.0, relative to viewport)
    pub position: (f32, f32),

    /// Graph dimensions in pixels
    pub width: f32,
    pub height: f32,

    /// Target frame time in ms (for 60fps = 16.67ms)
    pub target_frame_time_ms: f32,

    /// Maximum frames to display
    pub max_frames: usize,
}

#[cfg(feature = "devtools")]
impl FrameTimelineGraph {
    /// Create a new frame timeline graph
    pub fn new() -> Self {
        Self {
            position: (0.02, 0.85),
            width: 200.0,
            height: 60.0,
            target_frame_time_ms: 16.67, // 60 FPS
            max_frames: 60,
        }
    }

    /// Render the timeline graph
    pub fn render(
        &self,
        profiler: &Profiler,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        use crate::Paint;
        use flui_types::{Point, Rect};

        // Get frame history
        let history = profiler.frame_history();
        if history.is_empty() {
            return;
        }

        // Position in pixels
        let x = viewport_size.width * self.position.0;
        let y = viewport_size.height * self.position.1;

        // Draw background
        let bg_rect = Rect::from_xywh(x, y, self.width, self.height);
        let bg_paint = Paint {
            color: [0.0, 0.0, 0.0, 0.75],
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Draw target line (60fps = 16.67ms)
        let target_y = y + self.height - (self.target_frame_time_ms / 33.33 * self.height);
        let target_paint = Paint {
            color: [0.4, 0.4, 0.4, 0.8], // Gray line
            stroke_width: 1.0,
            ..Default::default()
        };
        painter.line(
            Point::new(x, target_y),
            Point::new(x + self.width, target_y),
            &target_paint,
        );

        // Draw frame time bars
        let frames_to_show = history.len().min(self.max_frames);
        let bar_width = self.width / frames_to_show as f32;

        for (i, stats) in history.iter().rev().take(frames_to_show).enumerate() {
            let frame_time_ms = stats.total_time_ms() as f32;

            // Clamp to reasonable max (33ms = ~30fps)
            let clamped_time = frame_time_ms.min(33.33);
            let bar_height = clamped_time / 33.33 * self.height;

            let bar_x = x + i as f32 * bar_width;
            let bar_y = y + self.height - bar_height;

            // Color based on performance (green = good, yellow = ok, red = bad)
            let color = if stats.is_jank() {
                [1.0, 0.3, 0.3, 0.9] // Red for jank
            } else if frame_time_ms > self.target_frame_time_ms {
                [1.0, 0.8, 0.2, 0.9] // Yellow for slight slowdown
            } else {
                [0.2, 0.8, 0.4, 0.9] // Green for good
            };

            let bar_paint = Paint {
                color,
                ..Default::default()
            };

            let bar_rect = Rect::from_xywh(bar_x, bar_y, bar_width - 1.0, bar_height);
            painter.rect(bar_rect, &bar_paint);
        }

        // Draw label
        let text_paint = Paint {
            color: [0.8, 0.8, 0.8, 1.0],
            ..Default::default()
        };
        painter.text("Frame Time", Point::new(x + 5.0, y + 12.0), 10.0, &text_paint);

        painter.restore();
    }
}

#[cfg(feature = "devtools")]
impl Default for FrameTimelineGraph {
    fn default() -> Self {
        Self::new()
    }
}
