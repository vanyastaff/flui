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
/// // Composite with automatic profiling
/// compositor.composite(&scene, &mut painter);
///
/// // Get profiler stats
/// if let Some(stats) = compositor.frame_stats() {
///     println!("Frame time: {:.2}ms", stats.total_time_ms());
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

    /// Composite a scene with automatic profiling
    ///
    /// This wraps the standard composite() call with profiling markers
    /// for the Paint phase.
    ///
    /// # Arguments
    /// * `scene` - The scene to composite
    /// * `painter` - The painter to render with
    pub fn composite(&mut self, scene: &crate::Scene, painter: &mut dyn crate::Painter) {
        let profiler = self.profiler.lock();

        // Begin frame
        profiler.begin_frame();

        // Profile the paint phase
        {
            let _guard = profiler.profile_phase(flui_devtools::profiler::FramePhase::Paint);
            drop(profiler); // Release lock before calling composite
            self.compositor.composite(scene, painter);
        }

        // Note: Frame is ended externally by calling end_frame()
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
        _profiler: &Profiler,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        use crate::layer::Layer;
        use crate::{Paint, PictureLayer};
        use flui_types::Rect;

        // Create a picture layer for overlay
        let mut picture = PictureLayer::new();

        // Position in pixels
        let x = viewport_size.width * self.position.0;
        let y = viewport_size.height * self.position.1;

        // Background box
        let bg_rect = Rect::from_xywh(x, y, 200.0, 100.0);
        let bg_paint = Paint {
            color: [0.0, 0.0, 0.0, 0.7], // Semi-transparent black
            ..Default::default()
        };
        picture.draw_rect(bg_rect, bg_paint);

        // TODO: Add text rendering for FPS, frame time, etc.
        // This requires text support in PictureLayer or Painter

        // For now, just paint the background
        painter.save();
        picture.paint(painter);
        painter.restore();
    }
}

#[cfg(feature = "devtools")]
impl Default for PerformanceOverlay {
    fn default() -> Self {
        Self::new()
    }
}
