//! Self-contained rendering pipeline for hot-reload plugins.
//!
//! `PluginPipeline` runs the full three-tree architecture (View → Element →
//! Render) inside a plugin, producing a [`Scene`] that can be passed back to
//! the host via the `app_plugin!` macro.
//!
//! This is intentionally independent of the host's UI runtime — the plugin owns its
//! own `WidgetsBinding` and `PipelineOwner`, so it never shares mutable UI
//! state with the host's `UiRuntime`.
//!
//! Text is no exception. The pipeline measures through the
//! [`TextContextHandle`] it is mounted with, and `app_plugin!` passes a
//! [`TextContextHandle::standalone`] one: a context over the plugin image's
//! own font collection, holding the bundled faces. The host UI runtime's context
//! does not cross the `dlopen` boundary: `flui_app_build` has no parameter
//! that could carry it, and `abi_token` does not cover `TextContext`'s layout.
//! Faces the host app registers are not visible to the plugin (ARCHITECTURE.md,
//! "The plugin image is a UI runtime of its own for text").

#[cfg(test)]
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_layer::Scene;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner, TextContextHandle};
use flui_view::{__runtime::BindingRuntime as _, StatelessView, View, WidgetsBinding};

/// Log messages via Android logcat (or stderr on other platforms).
///
/// The tracing subscriber from the host doesn't propagate into dlopen'd
/// plugins, so we use `android_log_sys` directly on Android and `eprintln`
/// elsewhere.
fn log(msg: &str) {
    #[cfg(target_os = "android")]
    {
        let tag = c"PluginPipeline";
        let msg_c = std::ffi::CString::new(msg).unwrap_or_default();
        // SAFETY: `tag` and `msg_c` are NUL-terminated C strings that outlive
        // the call; liblog copies both before returning and retains neither.
        #[expect(unsafe_code)]
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
/// `draw_frame(width, height)` call.
///
/// # Usage
///
/// Created by the `app_plugin!` macro. Not intended for direct use.
///
/// # Lifecycle
///
/// 1. `mount()` — Creates pipeline, mounts root widget
/// 2. `draw_frame(width, height)` — Build → Layout → Paint → Scene (called
///    per frame, at that frame's surface size)
/// 3. Drop — Cleans up element and render trees
#[expect(missing_debug_implementations)]
pub struct PluginPipeline {
    widgets: WidgetsBinding,
    pipeline_owner: PipelineCell,
    #[cfg(test)]
    frame_boundary_probe: Option<Box<dyn FnMut() + Send>>,
}

impl PluginPipeline {
    /// Mount a root widget and create the rendering pipeline.
    ///
    /// This mirrors the `mount_root()` logic in `flui-app`'s runner,
    /// but uses a standalone `WidgetsBinding` instead of the host UI runtime's
    /// widget machinery. Every measurement the pipeline makes goes through
    /// `text`.
    pub fn mount<V>(root: V, width: f64, height: f64, text: TextContextHandle) -> Self
    where
        V: View + StatelessView + Clone + Send + Sync + 'static,
    {
        let pipeline = Self::mount_with_boundary(&root, width, height, text, |_| {});
        // Preserve the established by-value API contract: mounting consumes
        // the root configuration after cloning it into the element tree.
        drop(root);
        pipeline
    }

    fn mount_with_boundary<V>(
        root: &V,
        width: f64,
        height: f64,
        text: TextContextHandle,
        mount_boundary: impl FnOnce(&WidgetsBinding),
    ) -> Self
    where
        V: View + StatelessView + Clone + Send + Sync + 'static,
    {
        let widgets = WidgetsBinding::new();
        let pipeline_owner = PipelineCell::new(PipelineOwner::new(text));

        // Connect WidgetsBinding to PipelineOwner
        widgets.set_pipeline_owner(pipeline_owner.clone());

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
        pipeline_owner.with(|owner| {
            let has_root = owner.root_id().is_some();
            let tree_len = owner.render_tree().len();
            log(&format!(
                "mount complete: root_id={has_root}, render_tree_len={tree_len}, size={width}x{height}"
            ));
        });

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
    ///
    /// The root is laid out tight to `width` x `height`, the surface size of
    /// this frame: the host passes it on every call, so a resized surface is
    /// laid out at its new size on the next frame.
    pub fn draw_frame(&mut self, width: f64, height: f64) -> Scene {
        let widgets = &self.widgets;
        let pipeline_owner = &self.pipeline_owner;
        widgets.with_global_key_registry(|| {
            // Phase 1: Build (rebuild dirty elements)
            if self.widgets.has_pending_builds() {
                self.widgets.draw_frame();
            }

            // Legal frame boundary: widget build has released its binding
            // write lock, while the plugin ui_runtime's GlobalKey activation is
            // still in dynamic scope. Tests observe the production entrypoint
            // here without querying GlobalKey from inside `build()`.
            #[cfg(test)]
            if let Some(probe) = self.frame_boundary_probe.as_mut() {
                probe();
            }

            // Phase 2: Run the full frame through the typestate-driven
            // pipeline. Force-mark the root dirty first so we always produce
            // a fresh LayerTree -- unlike the host ui_runtime's frame loop (which
            // skips frames when nothing is dirty and the previous frame is
            // still on-screen), the plugin must return a Scene every time
            // it's called: the
            // host expects a new opaque pointer.
            //
            // `run_frame_in_place` keeps the owner in its slot on every
            // exit, a panic included, and returns the frame's
            // `Option<LayerTree>` or its error. On error we log and emit an
            // empty Scene so the host's opaque pointer stays valid.
            // A scene of its own each frame: the host draws the frame whole, so
            // nothing is retained across the plugin boundary, and the pipeline
            // repaints from the root into the new lineage. It drops after the
            // pipeline's borrow ends: an annotation's `Drop` is user code.
            let mut scene = flui_rendering::layer::SceneOwner::new();
            let mut released: Option<flui_rendering::layer::SceneVersion> = None;
            let layer_tree = pipeline_owner.with_mut(|guard| {
                if let Some(root_id) = guard.root_id() {
                    guard.mark_needs_paint(root_id);
                } else {
                    log("draw_frame: WARNING — no root_id in pipeline");
                }
                // The root lays out at this frame's surface size, as the host
                // ui_runtime's root does at its window's every frame. Without root
                // constraints the pipeline lays nothing out and paints an
                // unmeasured tree; constraints set once at mount would keep the
                // first size after a resize.
                guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(width, height))));
                let mut writer = scene.writer();
                let painted = match guard.run_frame_in_place(&mut writer) {
                    Ok(painted) => painted,
                    Err(e) => {
                        log(&format!("draw_frame: pipeline failed: {e:?}"));
                        false
                    }
                };
                if !painted {
                    return None;
                }
                // The scene's root slots the frame, as a presentation's does: a
                // node no slot reaches is unattached, and its leaders are
                // invisible to followers and to the pipeline's leader watches.
                if let Some(frame) = guard.scene_frame_node() {
                    let mut root = flui_rendering::layer::BoundaryContent::builder();
                    root.slot(frame);
                    let node = writer.current().root();
                    writer.push(flui_rendering::layer::SceneOp::Publish {
                        node,
                        content: std::sync::Arc::new(root.finish()),
                    });
                }
                // The key only tells a resize apart: whole pixels, clamped to
                // the range `u32` holds, a non-finite size as zero.
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to 0..=u32::MAX before the cast"
                )]
                let pixels = |extent: f64| {
                    if extent.is_finite() {
                        extent.ceil().clamp(0.0, f64::from(u32::MAX)) as u32
                    } else {
                        0
                    }
                };
                let surface = flui_rendering::layer::SurfaceKey {
                    size: (pixels(width), pixels(height)),
                    generation: flui_foundation::SurfaceGeneration::ZERO,
                };
                if let Err(error) = scene.apply(surface) {
                    log(&format!(
                        "draw_frame: BUG: the pipeline's patch was refused: {error}"
                    ));
                    return None;
                }
                released = guard.scene_applied(scene.current());
                guard
                    .scene_frame_node()
                    .and_then(|node| scene.current().flatten_node(node))
            });
            // The version the pipeline replaced drops here, outside its borrow.
            drop(released);

            // Phase 3: freeze the LayerTree into a Scene (an empty one when
            // the pipeline produced nothing).
            layer_tree.map_or_else(
                || {
                    log("draw_frame: no LayerTree produced after force-repaint");
                    Scene::default()
                },
                Scene::new,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use flui_foundation::ViewKey;
    use flui_view::{BuildContext, GlobalKey, IntoView, KeyLookup};

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
        fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
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
        let mut pipeline = PluginPipeline::mount_with_boundary(
            &root,
            320.0,
            240.0,
            TextContextHandle::standalone(),
            move |widgets| {
                // The user root is built on the first draw, so mount uses a
                // dedicated registry entry installed only after attach released
                // its binding lock. This observes the real mount activation
                // without manually activating TLS or exporting a test API.
                widgets.with_build_owner_mut(|owner| {
                    owner.register_global_key(&key_at_mount, flui_foundation::ElementId::new(73));
                });
                mount_observed_in_probe.store(
                    matches!(key_at_mount.current_element(), KeyLookup::Mounted(_)),
                    Ordering::Relaxed,
                );
            },
        );
        assert!(
            mount_observed.load(Ordering::Relaxed),
            "real mount path must keep plugin registry active after attach lock release"
        );
        assert_eq!(
            mount_probe_key.current_element(),
            KeyLookup::NoActiveRuntime,
            "mount scope must not leak"
        );

        let observed = Arc::new(AtomicBool::new(false));
        let observed_in_probe = Arc::clone(&observed);
        let key_in_probe = key.clone();
        pipeline.frame_boundary_probe = Some(Box::new(move || {
            observed_in_probe.store(
                matches!(key_in_probe.current_element(), KeyLookup::Mounted(_)),
                Ordering::Relaxed,
            );
        }));

        let _scene = pipeline.draw_frame(320.0, 240.0);
        assert!(
            observed.load(Ordering::Relaxed),
            "real draw_frame must keep plugin registry active after build lock release"
        );
        assert_eq!(
            key.current_element(),
            KeyLookup::NoActiveRuntime,
            "plugin scope must not leak"
        );
    }

    /// The plugin's scene root slots the frame, so a leader the frame
    /// publishes is attached: the pipeline's leader watches see it.
    ///
    /// Red-check: apply the patch without republishing the root (the watch
    /// reads no leader).
    #[test]
    fn a_plugin_frame_publishes_its_leaders_attached() {
        use flui_objects::{RenderLeaderLayer, RenderSizedBox};
        use flui_rendering::layer::LayerLink;
        use flui_rendering::testing::{box_node, tree};

        let root = ProbeRoot {
            key: GlobalKey::<()>::new(),
        };
        let mut pipeline =
            PluginPipeline::mount(root, 100.0, 100.0, TextContextHandle::standalone());
        let _built = pipeline.draw_frame(100.0, 100.0);
        let link = LayerLink::new();
        pipeline.pipeline_owner.with_mut(|owner| {
            let (leader, _) = tree::mount(
                owner,
                box_node(RenderLeaderLayer::new(link))
                    .child(box_node(RenderSizedBox::fixed(10.0, 20.0))),
            );
            owner.set_root_id(Some(leader));
        });
        let scheduler = flui_scheduler::UpdateScheduler::new();
        let frame = flui_scheduler::OwnerFrame::try_new(&scheduler)
            .expect("the scheduler has no live owner frame");
        let watch = pipeline
            .pipeline_owner
            .watch_leader(link, frame.post_frame_handle());

        let _scene = pipeline.draw_frame(100.0, 100.0);
        assert!(
            watch.geometry().is_some(),
            "the leader the plugin frame painted is attached"
        );
    }
}
