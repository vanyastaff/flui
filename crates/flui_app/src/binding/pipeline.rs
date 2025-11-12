//! Pipeline binding - manages rendering pipeline via PipelineOwner
//!
//! PipelineBinding provides the high-level interface for managing the rendering pipeline,
//! attaching the root widget, and coordinating build/layout/paint phases.

use super::BindingBase;
use flui_core::{
    foundation::ElementId,
    pipeline::PipelineOwner,
    view::{IntoElement, View},
};
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
        V: View + 'static,
    {
        use flui_core::view::{BuildContext, BuildContextGuard};

        tracing::info!("Attaching root widget");

        // Check if root already exists
        {
            let pipeline = self.pipeline_owner.read();
            if pipeline.root_element_id().is_some() {
                panic!("Root widget already attached. PipelineOwner only supports one root.");
            }
        }

        // Create a temporary BuildContext for root widget initialization
        // We use ElementId(0) as a placeholder since the root doesn't have a parent
        let tree = self.pipeline_owner.read().tree().clone();
        let ctx = BuildContext::new(tree.clone(), flui_core::ElementId::new(1));

        // Set up BuildContext guard and convert View to Element
        let element = {
            let _guard = BuildContextGuard::new(&ctx);
            widget.into_element()
        };

        // Set as pipeline root (automatically schedules initial build)
        let mut pipeline = self.pipeline_owner.write();
        let root_id = pipeline.set_root(element);
        drop(pipeline);

        tracing::info!(root_id = ?root_id, "Root widget attached to pipeline");
    }

    /// Detach root widget
    ///
    /// Removes the root widget from the pipeline and cleans up the tree.
    /// This is called when the app exits or when switching root widgets.
    ///
    /// **Note:** Currently, flui-core's PipelineOwner doesn't have a remove_root() method,
    /// so this is a TODO for future implementation.
    pub fn detach_root_widget(&self) {
        let pipeline = self.pipeline_owner.read();
        if let Some(root_id) = pipeline.root_element_id() {
            drop(pipeline);

            // TODO: PipelineOwner needs a remove_root() method
            // For now, we can clear the tree entirely
            let mut pipeline = self.pipeline_owner.write();
            pipeline.tree().write().clear();

            tracing::info!(root_id = ?root_id, "Root widget detached (tree cleared)");
        }
    }

    /// Handle build frame
    ///
    /// Called every frame by SchedulerBinding to process pending rebuilds.
    /// Flushes the rebuild queue (signal updates) and marks elements dirty.
    ///
    /// The actual build phase is triggered by RendererBinding during draw_frame().
    pub fn handle_build_frame(&self) {
        let mut pipeline = self.pipeline_owner.write();

        // Flush rebuild queue (processes signal updates)
        pipeline.flush_rebuild_queue();

        let dirty_count = pipeline.dirty_count();
        if dirty_count > 0 {
            tracing::trace!(dirty_count, "Build frame handled, elements marked dirty");
        }
    }

    /// Get shared reference to the pipeline owner
    ///
    /// Used by renderer and other framework components.
    #[must_use]
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }

    /// Get root element ID
    ///
    /// Returns None if no root widget is attached.
    #[must_use]
    pub fn root_element(&self) -> Option<ElementId> {
        self.pipeline_owner.read().root_element_id()
    }

    /// Check if a root widget is attached
    #[must_use]
    pub fn has_root(&self) -> bool {
        self.pipeline_owner.read().root_element_id().is_some()
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
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = PipelineBinding::new(pipeline);
        assert!(!binding.has_root());
        assert_eq!(binding.root_element(), None);
    }

    #[test]
    fn test_handle_build_frame_empty() {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let binding = PipelineBinding::new(pipeline);

        // Should not panic with no root
        binding.handle_build_frame();
    }
}
