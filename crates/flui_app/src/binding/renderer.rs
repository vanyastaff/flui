//! Renderer binding - bridge to PipelineOwner
//!
//! RendererBinding coordinates the rendering pipeline phases:
//! build → layout → compositing → paint

use super::BindingBase;
use flui_core::pipeline::Pipeline;
use flui_engine::Scene;
use flui_types::{constraints::BoxConstraints, Size};
use std::sync::Arc;

/// Renderer binding - bridges to Pipeline
///
/// # Architecture
///
/// ```text
/// RendererBinding
///   ├─ Pipeline (trait object - build, layout, paint phases)
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
/// Uses `Arc<dyn Pipeline>` for trait-based abstraction.
/// This enables dependency injection, mocking for tests, and alternative implementations.
pub struct RendererBinding {
    /// Pipeline abstraction for coordinating build/layout/paint
    pipeline: Arc<dyn Pipeline>,
}

impl RendererBinding {
    /// Create a new RendererBinding with a Pipeline implementation
    ///
    /// # Parameters
    ///
    /// - `pipeline`: Any implementation of the Pipeline trait
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::pipeline::{Pipeline, PipelineOwner};
    /// use parking_lot::RwLock;
    /// use std::sync::Arc;
    ///
    /// // Production: Use real PipelineOwner
    /// let owner = Arc::new(RwLock::new(PipelineOwner::new()));
    /// let renderer = RendererBinding::new(owner);
    ///
    /// // Testing: Use mock
    /// let mock: Arc<dyn Pipeline> = Arc::new(MockPipeline::new());
    /// let renderer = RendererBinding::new(mock);
    /// ```
    pub fn new<P>(pipeline: P) -> Self
    where
        P: Pipeline + 'static,
    {
        Self {
            pipeline: Arc::new(pipeline),
        }
    }

    /// Create from Arc<dyn Pipeline> directly
    ///
    /// Useful when you already have an Arc-wrapped pipeline.
    pub fn from_arc(pipeline: Arc<dyn Pipeline>) -> Self {
        Self { pipeline }
    }

    /// Draw frame - execute complete rendering pipeline
    ///
    /// Executes all three rendering phases in order:
    ///
    /// 1. **Build**: Rebuild dirty widgets (flush_build)
    /// 2. **Layout**: Compute sizes and positions (flush_layout)
    /// 3. **Paint**: Generate CanvasLayer (flush_paint)
    ///
    /// Uses PipelineOwner::build_frame() which handles all phases atomically.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints (typically tight constraints matching window size)
    ///
    /// # Returns
    ///
    /// Scene containing the CanvasLayer ready for GPU rendering.
    /// Returns an empty scene if the pipeline is empty or errors occurred.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    /// let scene = renderer.draw_frame(constraints);
    /// if let Some(layer) = scene.root_layer() {
    ///     wgpu_renderer.render(layer.as_ref(), &view, &mut encoder)?;
    /// }
    /// ```
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        tracing::trace!("Starting draw frame");

        // Execute complete pipeline: rebuild queue → build → layout → paint
        // Pipeline::build_frame handles all phases atomically
        let layer = match self.pipeline.build_frame(constraints) {
            Ok(layer_opt) => {
                if layer_opt.is_none() {
                    tracing::warn!("Pipeline returned None (empty tree or no root)");
                }
                layer_opt
            }
            Err(e) => {
                tracing::error!(error = ?e, "Pipeline build_frame failed");
                None
            }
        };

        // Extract size from pipeline or use constraints as fallback
        let size = self
            .pipeline
            .root_element_id()
            .and_then(|root_id| {
                let tree = self.pipeline.tree();
                let tree_guard = tree.read();
                tree_guard.render_state(root_id).and_then(|state| state.size())
            })
            .unwrap_or_else(|| Size::new(constraints.max_width, constraints.max_height));

        // Create scene using new flui_engine::Scene API
        let scene = if let Some(layer) = layer {
            // Wrap layer in Arc for zero-copy sharing with hit testing
            Scene::with_layer(size, Arc::new(*layer), 0)
        } else {
            Scene::new(size)
        };

        tracing::trace!(
            size = ?size,
            has_content = scene.has_content(),
            "Draw frame complete"
        );
        scene
    }

    /// Get shared reference to the pipeline
    ///
    /// Returns the Pipeline trait object. This allows framework code to
    /// access the pipeline without depending on concrete PipelineOwner.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let pipeline = renderer.pipeline();
    /// pipeline.request_layout(element_id);
    /// ```
    #[must_use]
    pub fn pipeline(&self) -> Arc<dyn Pipeline> {
        self.pipeline.clone()
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
    use flui_core::pipeline::PipelineOwner;
    use parking_lot::RwLock;

    #[test]
    fn test_renderer_binding_creation() {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = RendererBinding::new(pipeline);
        let _pipeline = binding.pipeline();
        // Should not panic
    }

    #[test]
    fn test_renderer_binding_init() {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let mut binding = RendererBinding::new(pipeline);
        binding.init();
        // Should not panic
    }

    #[test]
    fn test_draw_frame_empty() {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = RendererBinding::new(pipeline);
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Should not panic even with empty pipeline
        let scene = binding.draw_frame(constraints);
        assert!(scene.size().width > 0.0);
        assert!(!scene.has_content()); // No root element, so no content
    }
}
