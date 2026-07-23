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
use flui_view::{StatelessView, View, WidgetsBinding};
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
    #[cfg(test)]
    frame_boundary_probe: Option<Box<dyn FnMut() + Send>>,
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
        let pipeline = Self::mount_with_boundary(&root, width, height, |_| {});
        // Preserve the established by-value API contract: mounting consumes
        // the root configuration after cloning it into the element tree.
        drop(root);
        pipeline
    }

    fn mount_with_boundary<V>(
        root: &V,
        width: f32,
        height: f32,
        mount_boundary: impl FnOnce(&WidgetsBinding),
    ) -> Self
    where
        V: View + StatelessView + Clone + Send + Sync + 'static,
    {
        let widgets = WidgetsBinding::new();
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

        // Connect WidgetsBinding to PipelineOwner
        widgets.set_pipeline_owner(Arc::clone(&pipeline_owner));

        widgets.with_global_key_registry(|| {
            widgets
                .attach_root_widget_with_size(root, width, height)
                .expect("BUG: fresh plugin binding must accept its root widget");
            // `attach_root_widget_with_size` has released its binding lock,
            // while mount's registry activation still encloses this legal
            // lifecycle boundary. The no-op production call is monomorphized
            // away; tests inject an observer through this shared path.
            mount_boundary(&widgets);
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
            #[cfg(test)]
            frame_boundary_probe: None,
        }
    }

    /// Execute the full rendering pipeline and produce a Scene.
    ///
    /// Runs all four phases:
    /// 1. **Build** — Rebuild dirty elements (calls user's `build()` methods)
    /// 2. **Layout / Compositing / Paint / Semantics** — Via the
    ///    typestate-driven `PipelineOwner::run_frame`.
    /// 3. **Scene** — Extract `LayerTree` and create `Scene`
    pub fn draw_frame(&mut self, width: f32, height: f32) -> Scene {
        let widgets = &self.widgets;
        let pipeline_owner = &self.pipeline_owner;
        widgets.with_global_key_registry(|| {
            // Phase 1: Build (rebuild dirty elements)
            if self.widgets.has_pending_builds() {
                self.widgets.draw_frame();
            }

            // Legal frame boundary: widget build has released its binding
            // write lock, while the plugin realm's GlobalKey activation is
            // still in dynamic scope. Tests observe the production entrypoint
            // here without querying GlobalKey from inside `build()`.
            #[cfg(test)]
            if let Some(probe) = self.frame_boundary_probe.as_mut() {
                probe();
            }

            // Phase 2: Run the full frame through the typestate-driven
            // pipeline. Force-mark the root dirty first so we always produce
            // a fresh LayerTree -- unlike AppBinding (which skips frames when
            // nothing is dirty and the previous frame is still on-screen),
            // the plugin must return a Scene every time it's called: the
            // host expects a new opaque pointer.
            //
            // `run_frame` returns
            // `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)`. On
            // error we log and emit an empty Scene so the host's opaque
            // pointer stays valid.
            let layer_tree = {
                let mut guard = pipeline_owner.write();
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
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use flui_foundation::ViewKey;
    use flui_view::{BuildContext, GlobalKey, IntoView};

    use super::*;
    use flui_view::element::ElementKind;

    #[derive(Clone)]
    struct ProbeRoot {
        key: GlobalKey<()>,
    }

    impl StatelessView for ProbeRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            flui_view::ErrorView::new("plugin probe")
        }
    }

    impl View for ProbeRoot {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    #[test]
    fn mount_and_draw_frame_activate_plugin_registry_at_frame_boundary() {
        let key = GlobalKey::<()>::new();
        let root = ProbeRoot { key: key.clone() };
        let mount_observed = Arc::new(AtomicBool::new(false));
        let mount_observed_in_probe = Arc::clone(&mount_observed);
        let mount_probe_key = GlobalKey::<()>::new();
        let key_at_mount = mount_probe_key.clone();
        let mut pipeline =
            PluginPipeline::mount_with_boundary(&root, 320.0, 240.0, move |widgets| {
                // The user root is built on the first draw, so mount uses a
                // dedicated registry entry installed only after attach released
                // its binding lock. This observes the real mount activation
                // without manually activating TLS or exporting a test API.
                widgets.with_build_owner_mut(|owner| {
                    owner.register_global_key(
                        key_at_mount.id(),
                        flui_foundation::ElementId::new(73),
                    );
                });
                mount_observed_in_probe
                    .store(key_at_mount.current_element().is_some(), Ordering::Relaxed);
            });
        assert!(
            mount_observed.load(Ordering::Relaxed),
            "real mount path must keep plugin registry active after attach lock release"
        );
        assert_eq!(
            mount_probe_key.current_element(),
            None,
            "mount scope must not leak"
        );

        let observed = Arc::new(AtomicBool::new(false));
        let observed_in_probe = Arc::clone(&observed);
        let key_in_probe = key.clone();
        pipeline.frame_boundary_probe = Some(Box::new(move || {
            observed_in_probe.store(key_in_probe.current_element().is_some(), Ordering::Relaxed);
        }));

        let _scene = pipeline.draw_frame(320.0, 240.0);
        assert!(
            observed.load(Ordering::Relaxed),
            "real draw_frame must keep plugin registry active after build lock release"
        );
        assert_eq!(key.current_element(), None, "plugin scope must not leak");
    }
}
