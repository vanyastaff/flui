//! Pipeline binding - manages rendering pipeline via PipelineOwner
//!
//! PipelineBinding provides the high-level interface for managing the rendering pipeline,
//! attaching the root widget, and coordinating build/layout/paint phases.

use super::BindingBase;
use flui_core::{pipeline::PipelineOwner, view::View};
use parking_lot::RwLock;
use std::sync::Arc;

/// Pipeline binding - manages rendering pipeline and root widget
///
/// # Architecture
///
/// ```text
/// PipelineBinding
///   └─ PipelineOwner (shared with RendererBinding)
///       ├─ ElementTree
///       ├─ BuildPipeline
///       ├─ LayoutPipeline
///       └─ PaintPipeline
/// ```
///
/// # Responsibilities
///
/// - Attach root widget to pipeline (via `attach_root_widget()`)
/// - Flush rebuild queue during frame cycle (via `handle_build_frame()`)
/// - Provide access to PipelineOwner for other bindings
///
/// # Design Note
///
/// PipelineOwner is the single source of truth for all tree management,
/// following Flutter's architecture pattern.
pub struct PipelineBinding {
    /// Pipeline owner (shared with RendererBinding)
    ///
    /// PipelineOwner manages:
    /// - Element tree (via Arc<RwLock<ElementTree>>)
    /// - Build/layout/paint coordination
    /// - Rebuild queue (for signals)
    /// - Root element tracking
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl PipelineBinding {
    /// Create a new PipelineBinding with a shared PipelineOwner
    ///
    /// The PipelineOwner should be shared with RendererBinding for coordinated rendering.
    pub fn new(pipeline_owner: Arc<RwLock<PipelineOwner>>) -> Self {
        Self { pipeline_owner }
    }

    /// Attach root widget
    ///
    /// Converts the View to an Element and sets it as the pipeline root.
    /// PipelineOwner automatically schedules it for initial build.
    ///
    /// This method delegates to `PipelineOwner::attach()` which handles
    /// all the BuildContext setup and View → Element conversion.
    ///
    /// # Parameters
    ///
    /// - `widget`: The root widget (typically MaterialApp or similar)
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached. The PipelineOwner only supports
    /// one root element at a time.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// binding.attach_root_widget(MyApp);
    /// ```
    pub fn attach_root_widget<V>(&self, widget: V)
    where
        V: View + Clone + Send + Sync + 'static,
    {
        // Delegate to PipelineOwner::attach()
        // This keeps all the View → Element conversion logic in flui-core
        let mut pipeline = self.pipeline_owner.write();
        pipeline.attach(widget);
    }

    /// Get shared reference to the pipeline owner
    ///
    /// This is the main access point for other bindings to interact with the pipeline.
    /// All pipeline operations should go through PipelineOwner directly for better
    /// separation of concerns.
    ///
    /// # Design Note
    ///
    /// PipelineBinding is a thin wrapper around PipelineOwner. Its only responsibility
    /// is to integrate PipelineOwner into the binding architecture. All actual pipeline
    /// operations (attach_root, flush_rebuild_queue, build_frame, etc.) should be called
    /// directly on PipelineOwner.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Good: Direct access to PipelineOwner
    /// let pipeline = binding.pipeline_owner();
    /// pipeline.write().attach(MyApp);
    /// pipeline.write().flush_rebuild_queue();
    ///
    /// // Bad: Creating wrapper methods in PipelineBinding
    /// // binding.attach_root_widget(MyApp);  // Avoid this pattern
    /// ```
    #[must_use]
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
}

impl BindingBase for PipelineBinding {
    fn init(&mut self) {
        tracing::debug!("PipelineBinding initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_binding_creation() {
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = PipelineBinding::new(pipeline_owner.clone());

        // Access pipeline directly
        let pipeline = binding.pipeline_owner();
        assert!(pipeline.read().root_element_id().is_none());
    }

    #[test]
    fn test_pipeline_owner_access() {
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = PipelineBinding::new(pipeline_owner.clone());

        // PipelineBinding should provide access to the same PipelineOwner
        let accessed_pipeline = binding.pipeline_owner();
        assert!(Arc::ptr_eq(&pipeline_owner, &accessed_pipeline));
    }
}
