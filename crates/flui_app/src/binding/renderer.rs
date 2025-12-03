//! Renderer binding - coordinates rendering pipeline phases
//!
//! RendererBinding executes the rendering pipeline phases:
//! build → layout → paint

use super::BindingBase;
use flui_core::pipeline::PipelineOwner;
use flui_engine::Scene;
use flui_types::{constraints::BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

/// Renderer binding - executes rendering pipeline
///
/// # Architecture
///
/// ```text
/// RendererBinding
///   └─ draw_frame(pipeline, constraints) → Scene
/// ```
///
/// # Pipeline Phases
///
/// 1. **Build**: Rebuild dirty widgets → Element tree
/// 2. **Layout**: Compute sizes → RenderObject geometry
/// 3. **Paint**: Generate drawing commands → CanvasLayer
///
/// # Design Note
///
/// RendererBinding no longer owns the pipeline. Instead, it receives
/// the pipeline as a parameter to `draw_frame()`. This eliminates
/// duplication and clarifies ownership semantics.
pub struct RendererBinding {
    // No fields needed - pipeline is passed as parameter
}

impl RendererBinding {
    /// Create a new RendererBinding
    pub fn new() -> Self {
        Self {}
    }

    /// Draw frame - execute complete rendering pipeline
    ///
    /// Executes all three rendering phases in order:
    ///
    /// 1. **Build**: Rebuild dirty widgets (flush_build)
    /// 2. **Layout**: Compute sizes and positions (flush_layout)
    /// 3. **Paint**: Generate CanvasLayer (flush_paint)
    ///
    /// # Parameters
    ///
    /// - `pipeline`: The PipelineOwner to use for rendering
    /// - `constraints`: Root layout constraints (typically window size)
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
    /// let scene = renderer.draw_frame(&pipeline_owner, constraints);
    /// if let Some(layer) = scene.root_layer() {
    ///     gpu_renderer.render(layer.as_ref())?;
    /// }
    /// ```
    pub fn draw_frame(
        &self,
        pipeline: &Arc<RwLock<PipelineOwner>>,
        constraints: BoxConstraints,
    ) -> Scene {
        tracing::trace!("Starting draw frame");

        // Get write lock for the entire frame
        let mut owner = pipeline.write();

        // Execute complete pipeline: build → layout → paint
        let layer = match owner.build_frame(constraints) {
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

        // Extract size from root element or use constraints as fallback
        let size = owner
            .root_element_id()
            .and_then(|root_id| {
                let tree = owner.tree();
                let tree_guard = tree.read();
                tree_guard
                    .get(root_id)
                    .and_then(|element| element.render_state())
                    .and_then(|state| state.downcast_ref::<flui_core::render::BoxRenderState>())
                    .map(|state| state.size())
                    .filter(|size| size.width > 0.0 && size.height > 0.0)
            })
            .unwrap_or_else(|| Size::new(constraints.max_width, constraints.max_height));

        // Create scene using flui_engine::Scene API
        let scene = if let Some(canvas) = layer {
            // Convert Canvas to Layer via CanvasLayer
            use flui_engine::layer::{CanvasLayer, Layer};
            let canvas_layer = CanvasLayer::from_canvas(canvas);
            let layer: Layer = canvas_layer.into();
            Scene::with_layer(size, Arc::new(layer), 0)
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
        // Should not panic
        drop(binding);
    }

    #[test]
    fn test_renderer_binding_init() {
        let mut binding = RendererBinding::new();
        binding.init();
        // Should not panic
    }

    #[test]
    fn test_draw_frame_empty() {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = RendererBinding::new();
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));

        // Should not panic even with empty pipeline
        let scene = binding.draw_frame(&pipeline, constraints);
        assert!(scene.size().width > 0.0);
        assert!(!scene.has_content()); // No root element, so no content
    }
}
