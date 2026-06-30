//! Self-contained rendering pipeline for hot-reload plugins.
//!
//! `PluginPipeline` runs the full three-tree architecture (View → Element →
//! Render) inside a plugin, producing a [`Scene`] that can be passed back to
//! the host via the `app_plugin!` macro.
//!
//! This is intentionally independent of `AppBinding` — the plugin owns its own
//! `WidgetsBinding` and `PipelineOwner`, avoiding singleton conflicts with the
//! host.

use std::sync::Arc;

use flui_layer::Scene;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::{Size, geometry::px};
use flui_view::{
    ElementBase, RootRenderElement, RootRenderView, StatelessView, View, WidgetsBinding,
    element::ElementKind,
};
use parking_lot::RwLock;

/// Log messages via Android logcat (or stderr on other platforms).
///
/// The tracing subscriber from the host doesn't propagate into dlopen'd
/// plugins, so we use `android_log_sys` directly on Android and `eprintln`
/// elsewhere.
#[allow(unused_variables)]
fn log(msg: &str) {
    #[cfg(target_os = "android")]
    {
        let tag = c"PluginPipeline";
        let msg_c = std::ffi::CString::new(msg).unwrap_or_default();
        #[allow(unsafe_code)]
        unsafe {
            android_log_sys::__android_log_write(
                android_log_sys::LogPriority::INFO as i32,
                tag.as_ptr(),
                msg_c.as_ptr(),
            );
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        eprintln!("[PluginPipeline] {msg}");
    }
}

/// A self-contained rendering pipeline for use inside hot-reload plugins.
///
/// Encapsulates `WidgetsBinding` (element tree) and `PipelineOwner` (render
/// tree), mounts a root widget, and produces `Scene` objects on each
/// `draw_frame()` call.
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
    /// Kept alive so the element tree isn't dropped while the pipeline is
    /// active. The element tree holds references into the render tree (via
    /// RenderId).
    #[allow(dead_code)]
    root_element: Option<ElementKind>,
}

impl PluginPipeline {
    /// Mount a root widget and create the rendering pipeline.
    ///
    /// This mirrors the `mount_root()` logic in `flui-app`'s runner,
    /// but uses a standalone `WidgetsBinding` instead of the global
    /// `AppBinding`.
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
            .element_mut()
            .downcast_mut::<RootRenderElement<V>>()
        {
            rre.set_pipeline_owner(Arc::clone(&pipeline_owner));
        } else {
            log("ERROR: failed to downcast to RootRenderElement");
        }

        // Mount the element tree (creates RenderViewObject, inserts into
        // RenderTree). Plan §U8: thread the ElementOwner split-borrow
        // handle through the recursive mount so descendant lifecycle calls
        // can register `GlobalKey`s / schedule rebuilds.
        widgets.with_build_owner_mut(|build_owner| {
            root_element.mount(None, 0, &mut build_owner.element_owner_mut());
        });

        // Diagnostic: verify pipeline state after mount
        {
            let owner = pipeline_owner.read();
            let has_root = owner.root_id().is_some();
            let tree_len = owner.render_tree().len();
            log(&format!(
                "mount complete: root_id={has_root}, render_tree_len={tree_len}, size={width}x{height}"
            ));
        }

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
    /// 2. **Layout / Compositing / Paint / Semantics** — Via the
    ///    typestate-driven `PipelineOwner::run_frame` (Mythos Step 7).
    /// 3. **Scene** — Extract `LayerTree` and create `Scene`
    pub fn draw_frame(&mut self, width: f32, height: f32) -> Scene {
        // Phase 1: Build (rebuild dirty elements)
        if self.widgets.has_pending_builds() {
            self.widgets.draw_frame();
        }

        // Phase 2: Run the full frame through the typestate-driven
        // pipeline. Force-mark the root dirty first so we always produce
        // a fresh LayerTree -- unlike AppBinding (which skips frames when
        // nothing is dirty and the previous frame is still on-screen),
        // the plugin must return a Scene every time it's called: the
        // host expects a new opaque pointer.
        //
        // Mythos Step 12 (2026-05-20): `run_frame` returns
        // `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)`. On
        // error we log and emit an empty Scene so the host's opaque
        // pointer stays valid.
        let layer_tree = {
            let mut guard = self.pipeline_owner.write();
            if let Some(root_id) = guard.root_id() {
                guard.add_node_needing_paint(root_id, 0);
            } else {
                log("draw_frame: WARNING — no root_id in pipeline");
            }
            let owner = std::mem::take(&mut *guard);
            let (owner, result) = owner.run_frame();
            *guard = owner;
            match result {
                Ok(layer_tree) => layer_tree,
                Err(e) => {
                    log(&format!("draw_frame: pipeline failed: {e:?}"));
                    None
                }
            }
        };

        // Phase 3: Extract Scene from LayerTree
        let size = Size::new(px(width), px(height));

        if let Some(layer_tree) = layer_tree {
            let root = layer_tree.root();
            Scene::new(size, layer_tree, root, 1)
        } else {
            log("draw_frame: no LayerTree produced after force-repaint");
            let tree = flui_layer::LayerTree::new();
            Scene::new(size, tree, None, 1)
        }
    }
}
