//! FluiApp - Main application structure
//!
//! This module provides the FluiApp struct, which manages the application lifecycle,
//! element tree, and rendering pipeline integration with egui.

use flui_core::{Offset, Size};
use flui_core::pipeline::PipelineOwner;
use flui_core::view::{AnyView, BuildContext};
use flui_core::foundation::ElementId;
use flui_engine::Painter;
use flui_types::BoxConstraints;

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
        if self.frame_count.is_multiple_of(60) && self.frame_count > 0 {
            println!(
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
/// use flui_core::view::View;
///
/// #[derive(Clone)]
/// struct MyApp;
///
/// impl View for MyApp {
///     type State = ();
///     type Element = ComponentElement;
///
///     fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
///         // Build your UI here
///         todo!()
///     }
/// }
///
/// run_app(Box::new(MyApp))?;
/// ```
pub struct FluiApp {
    /// Pipeline owner that manages the rendering pipeline
    pipeline: PipelineOwner,

    /// Root view (type-erased)
    root_view: Box<dyn AnyView>,

    /// Root element ID
    root_id: Option<ElementId>,

    /// Performance statistics
    stats: FrameStats,

    /// Last known window size for change detection
    last_size: Option<Size>,

    /// Whether the root has been initially built
    root_built: bool,
}

impl FluiApp {
    /// Create a new FluiApp with a root view
    ///
    /// # Parameters
    ///
    /// - `root_view`: The root view of the application (type-erased via AnyView)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_app::FluiApp;
    ///
    /// let app = FluiApp::new(Box::new(MyRootView));
    /// ```
    #[allow(deprecated)]
    pub fn new(root_view: Box<dyn AnyView>) -> Self {
        let pipeline = PipelineOwner::new();

        Self {
            pipeline,
            root_view,
            root_id: None,
            stats: FrameStats::default(),
            last_size: None,
            root_built: false,
        }
    }

    /// Get a reference to the pipeline owner
    #[allow(dead_code)]
    pub fn pipeline(&self) -> &PipelineOwner {
        &self.pipeline
    }

    /// Build the root element on first frame
    fn ensure_root_built(&mut self) {
        if self.root_built {
            return;
        }

        // Build root element from view
        // Use a temporary ID (will be replaced by actual ID after insertion)
        // Note: This temporary ID is only used for BuildContext creation
        // The actual root will get a proper ID from pipeline.set_root()
        let tree = self.pipeline.tree().clone();
        let temp_id = ElementId::new(1);
        let ctx = BuildContext::new(tree, temp_id);

        // Build root element using thread-local BuildContext
        let root_element = flui_core::view::with_build_context(&ctx, || {
            self.root_view.build_any()
        });

        // Mount and insert root element using pipeline.set_root()
        // This properly initializes the root and schedules it for build
        let root_id = self.pipeline.set_root(root_element);

        // Mark root for layout and paint
        println!("[DEBUG] Requesting layout and paint for root: {:?}", root_id);
        self.pipeline.request_layout(root_id);
        self.pipeline.request_paint(root_id);
        println!("[DEBUG] After request_layout/paint");

        self.root_id = Some(root_id);
        self.root_built = true;

        println!("[DEBUG] Root element built with ID: {:?}", root_id);
    }

    /// Check if window size changed significantly
    ///
    /// Ignores sub-pixel changes to avoid unnecessary layouts
    fn size_changed(&self, new_size: Size) -> bool {
        self.last_size.is_none_or(|last| {
            (last.width - new_size.width).abs() > 1.0 || (last.height - new_size.height).abs() > 1.0
        })
    }

    /// Process pointer events from egui
    ///
    /// Converts egui pointer events to Flui PointerEvents and dispatches them
    /// through hit testing.
    ///
    /// TODO: Re-implement pointer event handling when the API is ready
    fn process_pointer_events(&mut self, _ui: &egui::Ui) {
        // Pointer event dispatching is temporarily disabled
        // until the PipelineOwner API is updated
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

        // Ensure root is built
        self.ensure_root_built();

        // ===== Phase 1: Build =====
        // Keep flushing build until tree is fully built (no more dirty elements)
        let mut iterations = 0;
        loop {
            let dirty_count = self.pipeline.dirty_count();

            if dirty_count == 0 {
                break;
            }

            self.stats.rebuild_count += 1;
            self.pipeline.flush_build();

            iterations += 1;

            // Safety check: prevent infinite loops
            if iterations > 100 {
                tracing::warn!("Build loop exceeded 100 iterations, breaking");
                break;
            }
        }

        // DEBUG: Check element tree size
        #[cfg(debug_assertions)]
        if self.stats.frame_count <= 3 {
            let tree = self.pipeline.tree();
            let tree_guard = tree.read();
            let element_count = tree_guard.len();
            println!("[DEBUG] After build: ElementTree has {} elements", element_count);
        }

        // ===== Phase 2: Layout =====
        let current_size = Size::new(ui.available_size().x, ui.available_size().y);
        let size_changed = self.size_changed(current_size);
        // Need layout if size changed OR if there were any rebuilds
        let needs_layout = size_changed || iterations > 0;

        if self.stats.frame_count <= 3 {
            println!("[DEBUG] Frame {}: size_changed={}, iterations={}, needs_layout={}, size={:?}",
                self.stats.frame_count, size_changed, iterations, needs_layout, current_size);
        }

        // TEMP DEBUG: Always log when size changes
        if size_changed {
            println!("[RESIZE] Frame {}: Window resized to {:?}, triggering layout",
                self.stats.frame_count, current_size);
        }

        if needs_layout {
            let constraints = BoxConstraints::tight(current_size);
            match self.pipeline.flush_layout(constraints) {
                Ok(Some(_size)) => {
                    self.stats.layout_count += 1;
                    self.last_size = Some(current_size);

                    // IMPORTANT: Request paint after layout completes
                    // When window resizes, layout changes positions/sizes but doesn't trigger paint
                    // We must explicitly request paint to redraw the scene
                    if let Some(root_id) = self.root_id {
                        self.pipeline.request_paint(root_id);
                    }

                    if self.stats.frame_count <= 3 {
                        println!("[DEBUG] Layout succeeded: {:?}", _size);
                    }
                }
                Ok(None) => {
                    if self.stats.frame_count <= 3 {
                        println!("[DEBUG] Layout returned None (no root?)");
                    }
                }
                Err(e) => {
                    if self.stats.frame_count <= 3 {
                        println!("[DEBUG] Layout error: {:?}", e);
                    }
                }
            }
        }

        // ===== Phase 2.5: Pointer Events =====
        self.process_pointer_events(ui);

        // ===== Phase 3: Paint =====
        // Note: egui clears screen every frame, so we must paint every frame

        // DIRECT TEST: Draw text directly to egui to verify it works
        #[cfg(debug_assertions)]
        {
            let painter = ui.painter();
            let color = egui::Color32::from_rgb(255, 0, 0);  // RED
            let pos = egui::pos2(100.0, 100.0);
            let font_id = egui::FontId::proportional(48.0);
            let galley = painter.layout_no_wrap("DIRECT EGUI TEST".to_string(), font_id, color);
            let text_shape = egui::epaint::TextShape::new(pos, galley, color);
            painter.add(egui::Shape::Text(text_shape));
            tracing::debug!("Paint: Added DIRECT egui text at (100, 100)");
        }

        if let Ok(Some(layer)) = self.pipeline.flush_paint() {
            self.stats.paint_count += 1;

            #[cfg(debug_assertions)]
            tracing::debug!("Paint: Received layer from flush_paint, painting to screen");

            // Composite layer to screen using EguiPainter
            let egui_painter = ui.painter();
            let mut painter = flui_engine::EguiPainter::new(egui_painter);
            let offset = Offset::new(ui.min_rect().min.x, ui.min_rect().min.y);

            #[cfg(debug_assertions)]
            tracing::debug!("Paint: Calling layer.paint() with offset={:?}", offset);

            painter.save();
            painter.translate(offset);
            layer.paint(&mut painter);
            painter.restore();

            #[cfg(debug_assertions)]
            tracing::debug!("Paint: layer.paint() completed");
        } else {
            #[cfg(debug_assertions)]
            tracing::debug!("Paint: flush_paint returned None");
        }

        // Log performance stats periodically
        self.stats.log();

        // Request repaint for the next frame
        ctx.request_repaint();
    }
}

impl eframe::App for FluiApp {
    /// Update the app for one frame
    ///
    /// Called by eframe once per frame.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE) // Remove default padding/margin
            .show(ctx, |ui| {
                FluiApp::update(self, ctx, ui);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
