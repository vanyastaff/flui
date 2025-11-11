//! Renderer binding - bridge to PipelineOwner
//!
//! RendererBinding coordinates the rendering pipeline phases:
//! build → layout → compositing → paint

use super::BindingBase;
use flui_core::pipeline::PipelineOwner;
use flui_engine::CanvasLayer;
use flui_types::{constraints::BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

/// Temporary Scene type until flui_engine exports it
///
/// TODO: Replace with flui_engine::Scene when available
pub struct Scene {
    size: Size,
    layer: Option<Box<CanvasLayer>>,
}

impl Scene {
    pub fn new(size: Size) -> Self {
        Self { size, layer: None }
    }

    pub fn add_canvas_layer(&mut self, layer: Option<Box<CanvasLayer>>) {
        self.layer = layer;
    }

    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    #[must_use]
    pub fn layer(&self) -> Option<&CanvasLayer> {
        self.layer.as_deref()
    }
}

/// Renderer binding - bridges to flui_rendering
///
/// # Architecture
///
/// ```text
/// RendererBinding
///   ├─ PipelineOwner (build, layout, paint phases)
///   └─ Scene (composited output)
/// ```
///
/// # Pipeline Phases
///
/// 1. **Build**: Rebuild dirty widgets → Element tree
/// 2. **Layout**: Compute sizes → RenderObject geometry
/// 3. **Compositing**: Mark layers needing repaint
/// 4. **Paint**: Generate drawing commands → Canvas
///
/// # Thread-Safety
///
/// Uses Arc<RwLock<PipelineOwner>> for thread-safe pipeline access.
pub struct RendererBinding {
    /// Pipeline owner for coordinating build/layout/paint
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl RendererBinding {
    /// Create a new RendererBinding
    pub fn new() -> Self {
        Self {
            pipeline_owner: Arc::new(RwLock::new(PipelineOwner::new())),
        }
    }

    /// Draw frame - flush pipeline and generate scene
    ///
    /// Executes all rendering pipeline phases in order:
    ///
    /// 1. Flush build (rebuild dirty widgets)
    /// 2. Flush layout (compute sizes)
    /// 3. Flush paint (generate canvases)
    /// 4. Create scene
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints (viewport size)
    ///
    /// # Returns
    ///
    /// Scene containing all painted canvases ready for GPU rendering
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    /// let scene = binding.draw_frame(constraints);
    /// // Render scene to wgpu...
    /// ```
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        let mut pipeline = self.pipeline_owner.write();

        tracing::trace!("Starting draw frame");

        // 1. Flush build (rebuild dirty widgets)
        pipeline.flush_build();

        // 2. Flush layout (compute sizes)
        let size = pipeline
            .flush_layout(constraints)
            .ok()
            .flatten()
            .unwrap_or_else(|| Size::new(constraints.max_width, constraints.max_height));

        tracing::trace!(size = ?size, "Layout complete");

        // 3. Flush paint (generate canvases)
        let layer = pipeline.flush_paint().ok().flatten();

        // 4. Create scene
        let mut scene = Scene::new(size);
        scene.add_canvas_layer(layer);

        tracing::trace!("Draw frame complete");
        scene
    }

    /// Request layout for a specific render object
    ///
    /// Marks the render object as needing layout in the next frame.
    ///
    /// # Parameters
    ///
    /// - `render_id`: ID of the render object to mark dirty
    pub fn request_layout(&self, render_id: flui_core::foundation::ElementId) {
        let mut pipeline = self.pipeline_owner.write();
        pipeline.request_layout(render_id);
    }

    /// Request paint for a specific render object
    ///
    /// Marks the render object as needing repaint in the next frame.
    ///
    /// # Parameters
    ///
    /// - `render_id`: ID of the render object to mark dirty
    pub fn request_paint(&self, render_id: flui_core::foundation::ElementId) {
        let mut pipeline = self.pipeline_owner.write();
        pipeline.request_paint(render_id);
    }

    /// Get shared reference to the pipeline owner
    ///
    /// Used by widgets and framework code to access the pipeline.
    #[must_use]
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
}

impl Default for RendererBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for RendererBinding {
    fn init(&mut self) {
        tracing::debug!("RendererBinding initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_binding_creation() {
        let binding = RendererBinding::new();
        let _pipeline = binding.pipeline_owner();
        // Should not panic
    }

    #[test]
    fn test_renderer_binding_init() {
        let mut binding = RendererBinding::new();
        binding.init();
        // Should not panic
    }

    #[test]
    fn test_draw_frame_empty() {
        let binding = RendererBinding::new();
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Should not panic even with empty pipeline
        let scene = binding.draw_frame(constraints);
        assert!(scene.size().width > 0.0);
    }
}
