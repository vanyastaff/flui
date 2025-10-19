//! FluiApp - Main application structure
//!
//! This module provides the FluiApp struct, which manages the application lifecycle,
//! element tree, and rendering pipeline integration with egui.

use flui_core::{BoxConstraints, Offset, PipelineOwner, Size, Widget};
use flui_types::events::{PointerEvent, PointerEventData, PointerDeviceKind, PointerButton};

/// Performance statistics for debugging and optimization
#[derive(Debug, Default)]
pub(crate) struct FrameStats {
    /// Total number of frames rendered
    pub frame_count: u64,
    /// Number of frames where rebuild happened
    pub rebuild_count: u64,
    /// Number of frames where layout happened
    pub layout_count: u64,
    /// Number of frames where paint happened
    pub paint_count: u64,
}

impl FrameStats {
    /// Log statistics to console
    pub fn log(&self) {
        if self.frame_count % 60 == 0 && self.frame_count > 0 {
            tracing::info!(
                "Performance: {} frames | Rebuilds: {} ({:.1}%) | Layouts: {} ({:.1}%) | Paints: {} ({:.1}%)",
                self.frame_count,
                self.rebuild_count,
                (self.rebuild_count as f64 / self.frame_count as f64) * 100.0,
                self.layout_count,
                (self.layout_count as f64 / self.frame_count as f64) * 100.0,
                self.paint_count,
                (self.paint_count as f64 / self.frame_count as f64) * 100.0,
            );
        }
    }
}

/// FluiApp - Main application structure
///
/// Manages the Flui application lifecycle, including:
/// - Element tree management via PipelineOwner
/// - Three-phase rendering: Build → Layout → Paint
/// - Integration with eframe/egui for window management
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::run_app;
///
/// run_app(Box::new(MyRootWidget))?;
/// ```
pub struct FluiApp {
    /// Pipeline owner that manages the rendering pipeline
    pipeline: PipelineOwner,

    /// Performance statistics
    stats: FrameStats,

    /// Last known window size for change detection
    last_size: Option<Size>,
}

impl FluiApp {
    /// Create a new FluiApp with a root widget
    ///
    /// # Parameters
    ///
    /// - `root_widget`: The root widget of the application
    pub fn new(root_widget: Box<dyn Widget>) -> Self {
        let mut pipeline = PipelineOwner::new();
        pipeline.mount_root(root_widget);

        Self {
            pipeline,
            stats: FrameStats::default(),
            last_size: None,
        }
    }

    /// Get a reference to the pipeline owner
    #[allow(dead_code)]
    pub fn pipeline(&self) -> &PipelineOwner {
        &self.pipeline
    }

    /// Check if window size changed significantly
    ///
    /// Ignores sub-pixel changes to avoid unnecessary layouts
    fn size_changed(&self, new_size: Size) -> bool {
        self.last_size.map_or(true, |last| {
            (last.width - new_size.width).abs() > 1.0
                || (last.height - new_size.height).abs() > 1.0
        })
    }

    /// Process pointer events from egui
    ///
    /// Converts egui pointer events to Flui PointerEvents and dispatches them
    /// through hit testing.
    fn process_pointer_events(&mut self, ui: &egui::Ui) {
        let input = ui.input(|i| i.clone());

        // Get pointer position
        if let Some(pointer_pos) = input.pointer.latest_pos() {
            let position = Offset::new(
                pointer_pos.x - ui.min_rect().min.x,
                pointer_pos.y - ui.min_rect().min.y,
            );

            // Check for pointer down events (button click)
            if input.pointer.primary_clicked() {
                let event_data = PointerEventData::new(position, PointerDeviceKind::Mouse)
                    .with_button(PointerButton::Primary);
                let event = PointerEvent::Down(event_data);
                tracing::info!("🖱️ Pointer Down at {:?}", position);

                let result = self.pipeline.dispatch_pointer_event(event.clone());

                // Dispatch to gesture handlers if hit
                if result.is_hit() {
                    flui_widgets::gestures::dispatch_gesture_event(&event);
                }
            }

            // Check for pointer up events (button release)
            if input.pointer.primary_released() {
                let event_data = PointerEventData::new(position, PointerDeviceKind::Mouse)
                    .with_button(PointerButton::Primary);
                let event = PointerEvent::Up(event_data);
                tracing::info!("🖱️ Pointer Up at {:?}", position);

                let result = self.pipeline.dispatch_pointer_event(event.clone());

                // Dispatch to gesture handlers if hit
                if result.is_hit() {
                    flui_widgets::gestures::dispatch_gesture_event(&event);
                }
            }

            // Check for pointer move events (only if pointer is over widget)
            if input.pointer.is_moving() {
                let event_data = PointerEventData::new(position, PointerDeviceKind::Mouse);
                let event = PointerEvent::Move(event_data);
                self.pipeline.dispatch_pointer_event(event);
            }
        }
    }

    /// Update the application for one frame
    ///
    /// Three-phase rendering pipeline:
    /// 1. **Build**: Rebuild dirty elements
    /// 2. **Layout**: Calculate sizes and positions (only if needed)
    /// 3. **Paint**: Render to screen (every frame for egui)
    ///
    /// # Parameters
    ///
    /// - `ctx`: egui Context for rendering
    /// - `ui`: egui Ui for getting available space
    pub fn update(&mut self, ctx: &egui::Context, ui: &egui::Ui) {
        self.stats.frame_count += 1;

        // ===== Phase 1: Build =====
        let dirty_count = self.pipeline.tree().read().dirty_element_count();
        if dirty_count > 0 {
            self.stats.rebuild_count += 1;
            tracing::debug!("Frame {}: Rebuilding {} dirty elements", self.stats.frame_count, dirty_count);
            self.pipeline.flush_build();
        }

        // ===== Phase 2: Layout =====
        let current_size = Size::new(ui.available_size().x, ui.available_size().y);
        let needs_layout = dirty_count > 0 || self.size_changed(current_size);

        if needs_layout {
            let constraints = BoxConstraints::tight(current_size);
            if self.pipeline.flush_layout(constraints).is_some() {
                self.stats.layout_count += 1;
                self.last_size = Some(current_size);
            }
        }

        // ===== Phase 2.5: Pointer Events =====
        // Process pointer events after layout but before paint
        self.process_pointer_events(ui);

        // ===== Phase 3: Paint =====
        // Note: egui clears screen every frame, so we must paint every frame
        let painter = ctx.layer_painter(egui::LayerId::background());
        let offset = Offset::new(ui.min_rect().min.x, ui.min_rect().min.y);
        self.pipeline.flush_paint(&painter, offset);
        self.stats.paint_count += 1;

        // Log performance stats periodically
        self.stats.log();

        // Request repaint if there are dirty elements waiting
        if self.pipeline.tree().read().has_dirty_elements() {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for FluiApp {
    /// Update the app for one frame
    ///
    /// Called by eframe once per frame.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            FluiApp::update(self, ctx, ui);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::{BuildContext, StatelessWidget};
    use flui_widgets::prelude::Text;

    #[derive(Debug, Clone)]
    struct TestWidget;

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
            Box::new(Text::new("Test"))
        }
    }

    #[test]
    fn test_flui_app_creation() {
        let app = FluiApp::new(Box::new(TestWidget));

        // Should have mounted root element
        let tree_guard = app.pipeline().tree().read();
        assert!(tree_guard.element_count() >= 1);
    }

    #[test]
    fn test_frame_stats_logging() {
        let mut stats = FrameStats::default();

        // Should not panic
        stats.log();

        stats.frame_count = 60;
        stats.rebuild_count = 1;
        stats.layout_count = 1;
        stats.paint_count = 60;

        // Should log at 60 frames
        stats.log();
    }
}
