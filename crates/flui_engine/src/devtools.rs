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
