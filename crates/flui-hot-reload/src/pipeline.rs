//! Self-contained rendering pipeline for hot-reload plugins.
//!
//! `PluginPipeline` runs the full three-tree architecture (View → Element → Render)
//! inside a plugin, producing a [`Scene`] that can be passed back to the host
//! via the `app_plugin!` macro.
//!
//! This is intentionally independent of `AppBinding` — the plugin owns its own
//! `WidgetsBinding` and `PipelineOwner`, avoiding singleton conflicts with the host.

use flui_layer::Scene;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::geometry::px;
use flui_types::Size;
use flui_view::{
    ElementBase, RootRenderElement, RootRenderView, StatelessView, View, WidgetsBinding,
};
use parking_lot::RwLock;
use std::sync::Arc;

/// A self-contained rendering pipeline for use inside hot-reload plugins.
///
/// Encapsulates `WidgetsBinding` (element tree) and `PipelineOwner` (render tree),
/// mounts a root widget, and produces `Scene` objects on each `draw_frame()` call.
///
/// # Usage
///
/// Created by the `app_plugin!` macro. Not intended for direct use.
///
/// # Lifecycle
///
/// 1. `mount()` — Creates pipeline, mounts root widget
/// 2. `draw_frame()` — Build → Layout → Paint → Scene (called per frame)
/// 3. Drop — Cleans up element and render trees
#[allow(missing_debug_implementations)]
pub struct PluginPipeline {
    widgets: WidgetsBinding,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    /// Kept alive so the element tree isn't dropped while the pipeline is active.
    /// The element tree holds references into the render tree (via RenderId).
    #[allow(dead_code)]
    root_element: Option<Box<dyn ElementBase>>,
}

impl PluginPipeline {
    /// Mount a root widget and create the rendering pipeline.
    ///
    /// This mirrors the `mount_root()` logic in `flui-app`'s runner,
    /// but uses a standalone `WidgetsBinding` instead of the global `AppBinding`.
    pub fn mount<V>(root: V, width: f32, height: f32) -> Self
    where
        V: View + StatelessView + Clone + Send + Sync + 'static,
    {
        let widgets = WidgetsBinding::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Connect WidgetsBinding to PipelineOwner
        widgets.set_pipeline_owner(Arc::clone(&pipeline_owner));

        // Wrap user widget in RootRenderView (same as runner.rs mount_root)
        let root_view = RootRenderView::new(root, width, height);
        let mut root_element = root_view.create_element();

        // Set PipelineOwner on the root render element before mounting
        if let Some(rre) = root_element
            .as_any_mut()
            .downcast_mut::<RootRenderElement<V>>()
        {
            rre.set_pipeline_owner(Arc::clone(&pipeline_owner));
        }

        // Mount the element tree (creates RenderViewObject, inserts into RenderTree)
        root_element.mount(None, 0);

        tracing::info!("PluginPipeline: root widget mounted");

        Self {
            widgets,
            pipeline_owner,
            root_element: Some(root_element),
        }
    }

    /// Execute the full rendering pipeline and produce a Scene.
    ///
    /// Runs all four phases:
    /// 1. **Build** — Rebuild dirty elements (calls user's `build()` methods)
    /// 2. **Layout** — Compute sizes via `flush_layout()`
    /// 3. **Paint** — Generate display lists via `flush_paint()`
    /// 4. **Scene** — Extract `LayerTree` and create `Scene`
    pub fn draw_frame(&mut self, width: f32, height: f32) -> Scene {
        // Phase 1: Build (rebuild dirty elements)
        // WidgetsBinding.draw_frame() processes elements marked dirty via
        // mark_needs_build(). On first frame after mount(), the root element
        // is already built by mount() → perform_build().
        if self.widgets.has_pending_builds() {
            self.widgets.draw_frame();
        }

        // Phase 2 & 3: Layout + Compositing + Paint
        {
            let mut pipeline = self.pipeline_owner.write();
            pipeline.flush_layout();
            pipeline.flush_compositing_bits();
            pipeline.flush_paint();
        }

        // Phase 4: Extract Scene from LayerTree
        let size = Size::new(px(width), px(height));
        let mut pipeline = self.pipeline_owner.write();

        if let Some(layer_tree) = pipeline.take_layer_tree() {
            let root = layer_tree.root();
            Scene::new(size, layer_tree, root, 1)
        } else {
            // No layer tree produced — return empty scene
            tracing::warn!("PluginPipeline: no LayerTree produced, returning empty scene");
            let tree = flui_layer::LayerTree::new();
            Scene::new(size, tree, None, 1)
        }
    }
}
