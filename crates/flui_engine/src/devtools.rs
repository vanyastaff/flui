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

    /// Background opacity (0.0-1.0)
    pub bg_opacity: f32,

    /// Text opacity (0.0-1.0)
    pub text_opacity: f32,
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
            bg_opacity: 0.7,
            text_opacity: 1.0,
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
        use flui_types::{Color, Point, Rect};

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
            color: Color::from_rgba_f32_array([0.0, 0.0, 0.0, self.bg_opacity]), // Semi-transparent black with configurable opacity
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Text paint (white) - with separate text opacity
        let text_paint = Paint {
            color: Color::from_rgba_f32_array([1.0, 1.0, 1.0, self.text_opacity]),
            ..Default::default()
        };

        let mut current_y = y + 15.0;
        let line_height = 18.0;
        let padding_x = x + 10.0;
        let font_size = 13.0;

        // Draw FPS
        if self.show_fps {
            let fps_text = format!("FPS: {:.1}", avg_fps);
            painter.text(
                &fps_text,
                Point::new(padding_x, current_y),
                font_size,
                &text_paint,
            );
            current_y += line_height;
        }

        // Draw frame stats if available
        if let Some(stats) = stats {
            if self.show_frame_time {
                let time_text = format!("Frame: {:.2}ms", stats.total_time_ms());
                painter.text(
                    &time_text,
                    Point::new(padding_x, current_y),
                    font_size,
                    &text_paint,
                );
                current_y += line_height;

                // Draw paint phase time if available
                if let Some(paint_phase) = stats.phase(FramePhase::Paint) {
                    let paint_text = format!("Paint: {:.2}ms", paint_phase.duration_ms());
                    painter.text(
                        &paint_text,
                        Point::new(padding_x, current_y),
                        font_size,
                        &text_paint,
                    );
                    current_y += line_height;
                }
            }

            // Draw jank indicator
            if self.show_jank {
                if stats.is_jank() {
                    let jank_paint = Paint {
                        color: Color::from_rgba_f32_array([1.0, 0.3, 0.3, self.text_opacity]), // Red for jank with text opacity
                        ..Default::default()
                    };
                    painter.text(
                        "⚠ JANK",
                        Point::new(padding_x, current_y),
                        font_size,
                        &jank_paint,
                    );
                } else {
                    let jank_text = format!("Jank: {:.1}%", jank_pct);
                    painter.text(
                        &jank_text,
                        Point::new(padding_x, current_y),
                        font_size,
                        &text_paint,
                    );
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

    /// Background opacity (0.0-1.0)
    pub bg_opacity: f32,

    /// Text opacity (0.0-1.0)
    pub text_opacity: f32,
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
            bg_opacity: 0.7,
            text_opacity: 1.0,
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
        use flui_types::{Color, Point, Rect};

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
            color: Color::from_rgba_f32_array([0.0, 0.0, 0.0, self.bg_opacity]),
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Draw target line (60fps = 16.67ms)
        let target_y = y + self.height - (self.target_frame_time_ms / 33.33 * self.height);
        let target_paint = Paint {
            color: Color::from_rgba_f32_array([0.4, 0.4, 0.4, 0.5]), // Gray line
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
                Color::from_rgba_f32_array([1.0, 0.3, 0.3, 0.85]) // Red for jank
            } else if frame_time_ms > self.target_frame_time_ms {
                Color::from_rgba_f32_array([1.0, 0.8, 0.2, 0.85]) // Yellow for slight slowdown
            } else {
                Color::from_rgba_f32_array([0.2, 0.8, 0.4, 0.85]) // Green for good
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
            color: Color::from_rgba_f32_array([0.8, 0.8, 0.8, self.text_opacity]),
            ..Default::default()
        };
        painter.text(
            "Frame Time",
            Point::new(x + 5.0, y + 12.0),
            10.0,
            &text_paint,
        );

        painter.restore();
    }
}

#[cfg(feature = "devtools")]
impl Default for FrameTimelineGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory usage graph - visual representation of memory consumption
///
/// Renders a line graph showing memory usage over time,
/// making it easy to spot memory leaks and usage patterns.
#[cfg(all(feature = "devtools", feature = "memory-profiler"))]
pub struct MemoryGraph {
    /// Graph position (0.0-1.0, relative to viewport)
    pub position: (f32, f32),

    /// Graph dimensions in pixels
    pub width: f32,
    pub height: f32,

    /// Maximum memory to display (in MB)
    pub max_memory_mb: f32,

    /// Background opacity (0.0-1.0)
    pub bg_opacity: f32,

    /// Text opacity (0.0-1.0)
    pub text_opacity: f32,
}

#[cfg(all(feature = "devtools", feature = "memory-profiler"))]
impl MemoryGraph {
    /// Create a new memory graph
    pub fn new() -> Self {
        Self {
            position: (0.75, 0.02),
            width: 200.0,
            height: 100.0,
            max_memory_mb: 100.0, // 100MB max
            bg_opacity: 0.7,
            text_opacity: 1.0,
        }
    }

    /// Render the memory graph
    pub fn render(
        &self,
        memory_profiler: &flui_devtools::memory::MemoryProfiler,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        use crate::Paint;
        use flui_types::{Color, Point, Rect};

        // Get memory history
        let history = memory_profiler.history();
        if history.is_empty() {
            return;
        }

        // Position in pixels
        let x = viewport_size.width * self.position.0;
        let y = viewport_size.height * self.position.1;

        // Draw background
        let bg_rect = Rect::from_xywh(x, y, self.width, self.height);
        let bg_paint = Paint {
            color: Color::from_rgba_f32_array([0.0, 0.0, 0.0, self.bg_opacity]),
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Draw memory line
        if history.len() > 1 {
            let line_paint = Paint {
                color: Color::from_rgba_f32_array([0.3, 0.7, 1.0, 0.9]), // Blue line
                stroke_width: 2.0,
                ..Default::default()
            };

            let point_width = self.width / (history.len() - 1) as f32;

            for i in 0..history.len() - 1 {
                let mem1 = history[i].total_mb() as f32;
                let mem2 = history[i + 1].total_mb() as f32;

                // Normalize to graph height
                let y1 =
                    y + self.height - (mem1 / self.max_memory_mb * self.height).min(self.height);
                let y2 =
                    y + self.height - (mem2 / self.max_memory_mb * self.height).min(self.height);

                let x1 = x + i as f32 * point_width;
                let x2 = x + (i + 1) as f32 * point_width;

                painter.line(Point::new(x1, y1), Point::new(x2, y2), &line_paint);
            }
        }

        // Draw current memory value
        let text_paint = Paint {
            color: Color::from_rgba_f32_array([0.9, 0.9, 0.9, self.text_opacity]),
            ..Default::default()
        };

        let current_mb = memory_profiler.current_stats().total_mb();
        let mem_text = format!("Mem: {:.1} MB", current_mb);
        painter.text(&mem_text, Point::new(x + 5.0, y + 15.0), 11.0, &text_paint);

        // Show peak memory
        if let Some(peak) = memory_profiler.peak_memory() {
            let peak_text = format!("Peak: {:.1} MB", peak.total_mb());
            painter.text(&peak_text, Point::new(x + 5.0, y + 30.0), 10.0, &text_paint);
        }

        // Show average
        let avg_text = format!("Avg: {:.1} MB", memory_profiler.average_memory_mb());
        painter.text(&avg_text, Point::new(x + 5.0, y + 45.0), 10.0, &text_paint);

        // Leak warning
        if memory_profiler.is_leaking() {
            let warning_paint = Paint {
                color: Color::from_rgba_f32_array([1.0, 0.3, 0.3, self.text_opacity]),
                ..Default::default()
            };
            painter.text(
                "⚠ LEAK?",
                Point::new(x + 5.0, y + 60.0),
                11.0,
                &warning_paint,
            );
        }

        painter.restore();
    }
}

#[cfg(all(feature = "devtools", feature = "memory-profiler"))]
impl Default for MemoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Corner position for overlay anchoring
#[cfg(feature = "devtools")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCorner {
    /// Top-left corner
    TopLeft,
    /// Top-right corner
    TopRight,
    /// Bottom-left corner
    BottomLeft,
    /// Bottom-right corner
    BottomRight,
}

/// Unified DevTools overlay - single compact panel with all metrics
///
/// This combines FPS, frame timing, and memory info into one compact panel,
/// similar to professional tools like RenderDoc, PIX, Nsight Graphics, etc.
#[cfg(feature = "devtools")]
pub struct UnifiedDevToolsOverlay {
    /// Corner to anchor the overlay to
    pub corner: OverlayCorner,

    /// Background opacity (0.0-1.0)
    pub bg_opacity: f32,

    /// Text opacity (0.0-1.0)
    pub text_opacity: f32,

    /// Show FPS and frame time
    pub show_performance: bool,

    /// Show frame timeline graph
    pub show_timeline: bool,

    /// Show memory stats
    #[cfg(feature = "memory-profiler")]
    pub show_memory: bool,

    /// Width of the overlay
    pub width: f32,
}

#[cfg(feature = "devtools")]
impl UnifiedDevToolsOverlay {
    /// Create a new unified overlay (defaults to top-left corner)
    pub fn new() -> Self {
        Self {
            corner: OverlayCorner::TopLeft,
            bg_opacity: 0.75,
            text_opacity: 1.0,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
            width: 320.0,
        }
    }

    /// Set the corner to anchor the overlay to
    pub fn set_corner(&mut self, corner: OverlayCorner) {
        self.corner = corner;
    }

    /// Render the unified overlay
    pub fn render(
        &self,
        profiler: &flui_devtools::profiler::Profiler,
        #[cfg(feature = "memory-profiler")] memory_profiler: Option<
            &flui_devtools::memory::MemoryProfiler,
        >,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        use crate::Paint;
        use flui_types::{Color, Point, Rect};

        // Calculate total height needed first (for bottom corners)
        let mut total_height = 10.0; // Top padding
        let line_height = 16.0;
        let section_spacing = 8.0;

        // Performance section
        if self.show_performance {
            total_height += line_height * 3.0 + section_spacing; // FPS, Frame time, Jank
        }

        // Timeline section
        if self.show_timeline {
            total_height += 70.0 + section_spacing; // Graph height
        }

        // Memory section
        #[cfg(feature = "memory-profiler")]
        if self.show_memory && memory_profiler.is_some() {
            total_height += line_height * 3.0 + section_spacing; // Current, Peak, Avg
        }

        total_height += 10.0; // Bottom padding

        // Calculate position based on corner
        let margin = 8.0;
        let (x, mut y) = match self.corner {
            OverlayCorner::TopLeft => (margin, margin),
            OverlayCorner::TopRight => (viewport_size.width - self.width - margin, margin),
            OverlayCorner::BottomLeft => (margin, viewport_size.height - total_height - margin),
            OverlayCorner::BottomRight => (
                viewport_size.width - self.width - margin,
                viewport_size.height - total_height - margin,
            ),
        };

        // Draw background
        let bg_rect = Rect::from_xywh(x, y, self.width, total_height);
        let bg_paint = Paint {
            color: Color::from_rgba_f32_array([0.0, 0.0, 0.0, self.bg_opacity]),
            ..Default::default()
        };
        painter.save();
        painter.rect(bg_rect, &bg_paint);

        // Text paint
        let text_paint = Paint {
            color: Color::from_rgba_f32_array([1.0, 1.0, 1.0, self.text_opacity]),
            ..Default::default()
        };

        let padding_x = x + 10.0;
        y += 12.0;

        // === Performance Section ===
        if self.show_performance {
            let stats = profiler.frame_stats();
            let avg_fps = profiler.average_fps();
            let jank_pct = profiler.jank_percentage();

            // Title
            let title_paint = Paint {
                color: Color::from_rgba_f32_array([0.7, 0.9, 1.0, self.text_opacity]), // Light blue
                ..Default::default()
            };
            painter.text(
                "⚡ PERFORMANCE",
                Point::new(padding_x, y),
                12.0,
                &title_paint,
            );
            y += line_height + 2.0;

            // FPS
            let fps_text = format!("FPS: {:.1}", avg_fps);
            painter.text(
                &fps_text,
                Point::new(padding_x + 15.0, y),
                11.0,
                &text_paint,
            );
            y += line_height;

            // Frame time
            if let Some(stats) = stats {
                let time_text = format!("Frame: {:.2}ms", stats.total_time_ms());
                painter.text(
                    &time_text,
                    Point::new(padding_x + 15.0, y),
                    11.0,
                    &text_paint,
                );
                y += line_height;

                // Jank
                if stats.is_jank() {
                    let jank_paint = Paint {
                        color: Color::from_rgba_f32_array([1.0, 0.3, 0.3, self.text_opacity]),
                        ..Default::default()
                    };
                    painter.text("⚠ JANK", Point::new(padding_x + 15.0, y), 11.0, &jank_paint);
                } else {
                    let jank_text = format!("Jank: {:.1}%", jank_pct);
                    painter.text(
                        &jank_text,
                        Point::new(padding_x + 15.0, y),
                        11.0,
                        &text_paint,
                    );
                }
                y += line_height;
            }

            y += section_spacing;
        }

        // === Timeline Graph Section ===
        if self.show_timeline {
            let title_paint = Paint {
                color: Color::from_rgba_f32_array([0.7, 1.0, 0.8, self.text_opacity]), // Light green
                ..Default::default()
            };
            painter.text("📊 TIMELINE", Point::new(padding_x, y), 12.0, &title_paint);
            y += line_height + 2.0;

            // Draw mini timeline graph
            let graph_width = self.width - 20.0;
            let graph_height = 50.0;
            let graph_x = padding_x;
            let graph_y = y;

            // Draw graph background
            let graph_bg_rect = Rect::from_xywh(graph_x, graph_y, graph_width, graph_height);
            let graph_bg_paint = Paint {
                color: Color::from_rgba_f32_array([0.1, 0.1, 0.1, 0.5]),
                ..Default::default()
            };
            painter.rect(graph_bg_rect, &graph_bg_paint);

            // Draw target line (60fps)
            let target_y = graph_y + graph_height - (16.67 / 33.33 * graph_height);
            let target_paint = Paint {
                color: Color::from_rgba_f32_array([0.4, 0.4, 0.4, 0.5]),
                stroke_width: 1.0,
                ..Default::default()
            };
            painter.line(
                Point::new(graph_x, target_y),
                Point::new(graph_x + graph_width, target_y),
                &target_paint,
            );

            // Draw frame bars
            let history = profiler.frame_history();
            if !history.is_empty() {
                let max_frames = 60;
                let frames_to_show = history.len().min(max_frames);
                let bar_width = graph_width / frames_to_show as f32;

                for (i, stats) in history.iter().rev().take(frames_to_show).enumerate() {
                    let frame_time_ms = stats.total_time_ms() as f32;
                    let clamped_time = frame_time_ms.min(33.33);
                    let bar_height = clamped_time / 33.33 * graph_height;

                    let bar_x = graph_x + i as f32 * bar_width;
                    let bar_y = graph_y + graph_height - bar_height;

                    let color = if stats.is_jank() {
                        Color::from_rgba_f32_array([1.0, 0.3, 0.3, 0.85])
                    } else if frame_time_ms > 16.67 {
                        Color::from_rgba_f32_array([1.0, 0.8, 0.2, 0.85])
                    } else {
                        Color::from_rgba_f32_array([0.2, 0.8, 0.4, 0.85])
                    };

                    let bar_paint = Paint {
                        color,
                        ..Default::default()
                    };

                    let bar_rect = Rect::from_xywh(bar_x, bar_y, bar_width - 1.0, bar_height);
                    painter.rect(bar_rect, &bar_paint);
                }
            }

            y += graph_height + section_spacing;
        }

        // === Memory Section ===
        #[cfg(feature = "memory-profiler")]
        if self.show_memory {
            if let Some(mem_profiler) = memory_profiler {
                let title_paint = Paint {
                    color: Color::from_rgba_f32_array([1.0, 0.8, 0.6, self.text_opacity]), // Light orange
                    ..Default::default()
                };
                painter.text("💾 MEMORY", Point::new(padding_x, y), 12.0, &title_paint);
                y += line_height + 2.0;

                let current_mb = mem_profiler.current_stats().total_mb();
                let mem_text = format!("Current: {:.1} MB", current_mb);
                painter.text(
                    &mem_text,
                    Point::new(padding_x + 15.0, y),
                    11.0,
                    &text_paint,
                );
                y += line_height;

                if let Some(peak) = mem_profiler.peak_memory() {
                    let peak_text = format!("Peak: {:.1} MB", peak.total_mb());
                    painter.text(
                        &peak_text,
                        Point::new(padding_x + 15.0, y),
                        11.0,
                        &text_paint,
                    );
                    y += line_height;
                }

                let avg_text = format!("Avg: {:.1} MB", mem_profiler.average_memory_mb());
                painter.text(
                    &avg_text,
                    Point::new(padding_x + 15.0, y),
                    11.0,
                    &text_paint,
                );
                y += line_height;

                // Leak warning
                if mem_profiler.is_leaking() {
                    let warning_paint = Paint {
                        color: Color::from_rgba_f32_array([1.0, 0.3, 0.3, self.text_opacity]),
                        ..Default::default()
                    };
                    painter.text(
                        "⚠ LEAK DETECTED",
                        Point::new(padding_x + 15.0, y),
                        11.0,
                        &warning_paint,
                    );
                }
            }
        }

        painter.restore();
    }
}

#[cfg(feature = "devtools")]
impl Default for UnifiedDevToolsOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout manager for DevTools overlays
///
/// Organizes the positioning of multiple DevTools components to avoid overlap
/// and provide a clean, organized display.
#[cfg(feature = "devtools")]
pub struct DevToolsLayout {
    /// Performance overlay configuration
    pub performance_overlay: PerformanceOverlay,

    /// Frame timeline graph configuration
    pub timeline_graph: FrameTimelineGraph,

    /// Memory graph configuration (if memory-profiler feature is enabled)
    #[cfg(feature = "memory-profiler")]
    pub memory_graph: MemoryGraph,

    /// Global opacity for all overlays (0.0-1.0)
    pub global_opacity: f32,

    /// Show/hide performance overlay
    pub show_performance: bool,

    /// Show/hide timeline graph
    pub show_timeline: bool,

    /// Show/hide memory graph
    #[cfg(feature = "memory-profiler")]
    pub show_memory: bool,
}

#[cfg(feature = "devtools")]
impl DevToolsLayout {
    /// Create a new DevTools layout with default "compact" preset
    pub fn new() -> Self {
        Self::compact()
    }

    /// Compact layout - all overlays visible, non-overlapping
    /// Performance at top-left, Memory at top-right, Timeline at bottom-left
    pub fn compact() -> Self {
        let mut perf = PerformanceOverlay::new();
        perf.position = (0.01, 0.01); // Top-left with small margin

        let mut timeline = FrameTimelineGraph::new();
        timeline.position = (0.01, 0.85); // Bottom-left
        timeline.width = 250.0;
        timeline.height = 70.0;

        #[cfg(feature = "memory-profiler")]
        let mut memory = MemoryGraph::new();
        #[cfg(feature = "memory-profiler")]
        {
            memory.position = (0.72, 0.01); // Top-right
            memory.width = 260.0;
            memory.height = 110.0;
        }

        Self {
            performance_overlay: perf,
            timeline_graph: timeline,
            #[cfg(feature = "memory-profiler")]
            memory_graph: memory,
            global_opacity: 0.7,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
        }
    }

    /// Minimal layout - only FPS counter
    pub fn minimal() -> Self {
        let mut layout = Self::compact();
        layout.show_timeline = false;
        #[cfg(feature = "memory-profiler")]
        {
            layout.show_memory = false;
        }
        layout
    }

    /// Detailed layout - larger overlays stacked vertically on left side
    pub fn detailed() -> Self {
        let mut perf = PerformanceOverlay::new();
        perf.position = (0.01, 0.01);

        let mut timeline = FrameTimelineGraph::new();
        timeline.position = (0.01, 0.18);
        timeline.width = 320.0;
        timeline.height = 90.0;

        #[cfg(feature = "memory-profiler")]
        let mut memory = MemoryGraph::new();
        #[cfg(feature = "memory-profiler")]
        {
            memory.position = (0.01, 0.38);
            memory.width = 320.0;
            memory.height = 140.0;
        }

        Self {
            performance_overlay: perf,
            timeline_graph: timeline,
            #[cfg(feature = "memory-profiler")]
            memory_graph: memory,
            global_opacity: 0.7,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
        }
    }

    /// Bottom bar layout - all overlays in a horizontal bar at the bottom
    pub fn bottom_bar() -> Self {
        let mut perf = PerformanceOverlay::new();
        perf.position = (0.01, 0.88); // Bottom-left

        let mut timeline = FrameTimelineGraph::new();
        timeline.position = (0.25, 0.88); // Bottom-center-left
        timeline.width = 200.0;
        timeline.height = 70.0;

        #[cfg(feature = "memory-profiler")]
        let mut memory = MemoryGraph::new();
        #[cfg(feature = "memory-profiler")]
        {
            memory.position = (0.51, 0.88); // Bottom-center-right
            memory.width = 220.0;
            memory.height = 70.0;
        }

        Self {
            performance_overlay: perf,
            timeline_graph: timeline,
            #[cfg(feature = "memory-profiler")]
            memory_graph: memory,
            global_opacity: 0.7,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
        }
    }

    /// Right side layout - all overlays stacked vertically on right side
    pub fn right_side() -> Self {
        let mut perf = PerformanceOverlay::new();
        perf.position = (0.77, 0.01); // Top-right

        let mut timeline = FrameTimelineGraph::new();
        timeline.position = (0.73, 0.18); // Mid-right
        timeline.width = 260.0;
        timeline.height = 80.0;

        #[cfg(feature = "memory-profiler")]
        let mut memory = MemoryGraph::new();
        #[cfg(feature = "memory-profiler")]
        {
            memory.position = (0.73, 0.38); // Lower-right
            memory.width = 260.0;
            memory.height = 120.0;
        }

        Self {
            performance_overlay: perf,
            timeline_graph: timeline,
            #[cfg(feature = "memory-profiler")]
            memory_graph: memory,
            global_opacity: 0.7,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
        }
    }

    /// Corners layout - overlays positioned in different corners
    /// FPS in top-left, Memory in top-right, Timeline in bottom-left
    pub fn corners() -> Self {
        let mut perf = PerformanceOverlay::new();
        perf.position = (0.01, 0.01); // Top-left

        let mut timeline = FrameTimelineGraph::new();
        timeline.position = (0.01, 0.88); // Bottom-left
        timeline.width = 220.0;
        timeline.height = 65.0;

        #[cfg(feature = "memory-profiler")]
        let mut memory = MemoryGraph::new();
        #[cfg(feature = "memory-profiler")]
        {
            memory.position = (0.75, 0.01); // Top-right
            memory.width = 240.0;
            memory.height = 100.0;
        }

        Self {
            performance_overlay: perf,
            timeline_graph: timeline,
            #[cfg(feature = "memory-profiler")]
            memory_graph: memory,
            global_opacity: 0.7,
            show_performance: true,
            show_timeline: true,
            #[cfg(feature = "memory-profiler")]
            show_memory: true,
        }
    }

    /// Set global background opacity for all overlays
    pub fn set_opacity(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        self.global_opacity = opacity;
        self.performance_overlay.bg_opacity = opacity;
        self.timeline_graph.bg_opacity = opacity;
        #[cfg(feature = "memory-profiler")]
        {
            self.memory_graph.bg_opacity = opacity;
        }
    }

    /// Set text opacity for all overlays
    pub fn set_text_opacity(&mut self, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        self.performance_overlay.text_opacity = opacity;
        self.timeline_graph.text_opacity = opacity;
        #[cfg(feature = "memory-profiler")]
        {
            self.memory_graph.text_opacity = opacity;
        }
    }

    /// Render all enabled overlays
    pub fn render(
        &self,
        profiler: &flui_devtools::profiler::Profiler,
        #[cfg(feature = "memory-profiler")] memory_profiler: Option<
            &flui_devtools::memory::MemoryProfiler,
        >,
        painter: &mut dyn crate::Painter,
        viewport_size: flui_types::Size,
    ) {
        if self.show_performance {
            self.performance_overlay
                .render(profiler, painter, viewport_size);
        }

        if self.show_timeline {
            self.timeline_graph.render(profiler, painter, viewport_size);
        }

        #[cfg(feature = "memory-profiler")]
        if self.show_memory {
            if let Some(mem_profiler) = memory_profiler {
                self.memory_graph
                    .render(mem_profiler, painter, viewport_size);
            }
        }
    }
}

#[cfg(feature = "devtools")]
impl Default for DevToolsLayout {
    fn default() -> Self {
        Self::new()
    }
}
