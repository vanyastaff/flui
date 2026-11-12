//! A self-contained widget pipeline for a scene plugin (ADR-0311 §6).
//!
//! [`PluginPipeline`] runs the three trees (view, element, render) inside a
//! plugin image and produces a [`Scene`] per frame, which `flui-hot-reload`'s
//! `app_plugin!` hands back across the `dlopen` boundary. The plugin owns its
//! own `WidgetsBinding` and `PipelineOwner`, so it never shares mutable UI
//! state with the host's UI runtime.
//!
//! Text is no exception: the pipeline measures through the
//! [`TextContextHandle`] it is mounted with, and `app_plugin!` passes a
//! [`TextContextHandle::standalone`] one, a context over the plugin image's
//! own font collection. The host's context does not cross the boundary.
//!
//! The pipeline is here, not in the package, because mounting and framing a
//! binding goes through `flui_view::__runtime`, the composition roots' seam,
//! which is not SDK surface (ADR-0081 §4): the package names only this type.
//! It leaves with the dlopen path (ADR-0311 §7).

use std::fmt;
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_layer::Scene;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::layer::{BoundaryContent, SceneOp, SceneOwner, SceneVersion, SurfaceKey};
use flui_rendering::pipeline::{PipelineCell, PipelineOwner, TextContextHandle};
use flui_view::{__runtime::BindingRuntime as _, StatelessView, View, WidgetsBinding};

/// Why [`PluginPipeline::draw_frame`] produced no scene.
///
/// The plugin image has no tracing subscriber of its own, so the caller logs
/// it where its platform shows it and hands the host an empty scene.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginFrameError {
    /// The render pipeline refused the frame; the text is its error's.
    Pipeline(String),
    /// The scene refused the pipeline's patch: a defect in the pipeline.
    PatchRefused(String),
    /// The frame painted nothing: the pipeline has no root, or produced no
    /// layer tree.
    NothingPainted,
}

impl fmt::Display for PluginFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pipeline(error) => write!(f, "the plugin's pipeline failed: {error}"),
            Self::PatchRefused(error) => {
                write!(
                    f,
                    "BUG: the plugin's scene refused its pipeline's patch: {error}"
                )
            }
            Self::NothingPainted => f.write_str("the plugin's frame painted nothing"),
        }
    }
}

impl std::error::Error for PluginFrameError {}

/// A widget pipeline of its own inside a scene plugin.
///
/// Mount a root with [`mount`](Self::mount), then call
/// [`draw_frame`](Self::draw_frame) once per host frame at that frame's
/// surface size. Dropping it tears both trees down.
///
/// **Evolving** (ADR-0088 §4): a hook of the dlopen reload path, which
/// `flui-hot-reload` alone uses.
#[expect(
    missing_debug_implementations,
    reason = "the bindings it holds have no Debug of their own"
)]
pub struct PluginPipeline {
    widgets: WidgetsBinding,
    pipeline_owner: PipelineCell,
    #[cfg(test)]
    frame_boundary_probe: Option<Box<dyn FnMut()>>,
}

impl PluginPipeline {
    /// Mounts `root` on a binding of its own, laid out at `width` x `height`
    /// and measuring text through `text`.
    pub fn mount<V>(root: V, width: f64, height: f64, text: TextContextHandle) -> Self
    where
        V: View + StatelessView + Clone + 'static,
    {
        Self::mount_with_boundary(&root, width, height, text, |_| {})
    }

    fn mount_with_boundary<V>(
        root: &V,
        width: f64,
        height: f64,
        text: TextContextHandle,
        mount_boundary: impl FnOnce(&WidgetsBinding),
    ) -> Self
    where
        V: View + StatelessView + Clone + 'static,
    {
        let widgets = WidgetsBinding::new();
        let pipeline_owner = PipelineCell::new(PipelineOwner::new(text));
        widgets.set_pipeline_owner(pipeline_owner.clone());

        widgets.with_global_key_registry(|| {
            widgets
                .attach_root_widget_with_size(root, width, height)
                .expect("BUG: a fresh plugin binding accepts its root widget");
            // `attach_root_widget_with_size` has released its binding lock,
            // while the registry activation still encloses this boundary. The
            // production closure does nothing; tests observe it here.
            mount_boundary(&widgets);
        });

        Self {
            widgets,
            pipeline_owner,
            #[cfg(test)]
            frame_boundary_probe: None,
        }
    }

    /// Builds, lays out and paints a frame at `width` x `height` and returns
    /// its scene.
    ///
    /// The root is laid out tight to the size passed with each frame, so a
    /// resized surface is laid out at its new size on the next one. Every
    /// call repaints from the root into a scene of its own: the host draws
    /// the frame whole and nothing is retained across the plugin boundary.
    ///
    /// # Errors
    ///
    /// [`PluginFrameError`] when the frame produced no scene; the pipeline is
    /// still usable and the next call draws again.
    pub fn draw_frame(&mut self, width: f64, height: f64) -> Result<Scene, PluginFrameError> {
        let widgets = &self.widgets;
        let pipeline_owner = &self.pipeline_owner;
        widgets.with_global_key_registry(|| {
            if self.widgets.has_pending_builds() {
                self.widgets.draw_frame();
            }

            // A frame boundary: build has released its binding lock, while
            // the plugin's GlobalKey activation is still in scope.
            #[cfg(test)]
            if let Some(probe) = self.frame_boundary_probe.as_mut() {
                probe();
            }

            // A scene of its own each frame. It drops after the pipeline's
            // borrow ends, as does the version the pipeline replaces: an
            // annotation's `Drop` is user code.
            let mut scene = SceneOwner::new();
            let mut released: Option<SceneVersion> = None;
            let layer_tree = pipeline_owner.with_mut(|guard| {
                let Some(root_id) = guard.root_id() else {
                    return Err(PluginFrameError::NothingPainted);
                };
                guard.mark_needs_paint(root_id);
                guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(width, height))));
                let mut writer = scene.writer();
                let painted = guard
                    .run_frame_in_place(&mut writer)
                    .map_err(|error| PluginFrameError::Pipeline(format!("{error:?}")))?;
                if !painted {
                    return Err(PluginFrameError::NothingPainted);
                }
                // The scene's root slots the frame, as a presentation's does:
                // a node no slot reaches is unattached, and its leaders are
                // invisible to followers and to the pipeline's leader watches.
                if let Some(frame) = guard.scene_frame_node() {
                    let mut root = BoundaryContent::builder();
                    root.slot(frame);
                    let node = writer.current().root();
                    writer.push(SceneOp::Publish {
                        node,
                        content: Arc::new(root.finish()),
                    });
                }
                let surface = SurfaceKey {
                    size: (whole_pixels(width), whole_pixels(height)),
                    generation: flui_foundation::SurfaceGeneration::ZERO,
                };
                scene
                    .apply(surface)
                    .map_err(|error| PluginFrameError::PatchRefused(error.to_string()))?;
                released = guard.scene_applied(scene.current());
                Ok(guard
                    .scene_frame_node()
                    .and_then(|node| scene.current().flatten_node(node)))
            });
            drop(released);
            layer_tree?
                .map(Scene::new)
                .ok_or(PluginFrameError::NothingPainted)
        })
    }
}

/// A surface extent as whole pixels for the surface key, which only tells a
/// resize apart: clamped to what `u32` holds, a non-finite extent as zero.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u32::MAX before the cast"
)]
fn whole_pixels(extent: f64) -> u32 {
    if extent.is_finite() {
        extent.ceil().clamp(0.0, f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_foundation::ViewKey;
    use flui_view::element::ElementKind;
    use flui_view::{BuildContext, GlobalKey, IntoView, KeyLookup};

    use super::*;

    #[derive(Clone)]
    struct KeyedRoot {
        key: GlobalKey<()>,
    }

    impl StatelessView for KeyedRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            flui_view::ErrorView::new("plugin probe")
        }
    }

    impl View for KeyedRoot {
        fn create_element(self: Rc<Self>) -> ElementKind {
            ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    #[test]
    fn mount_and_draw_frame_activate_plugin_registry_at_frame_boundary() {
        let key = GlobalKey::<()>::new();
        let root = KeyedRoot { key: key.clone() };
        let mount_observed = Rc::new(Cell::new(false));
        let mount_observed_in_probe = Rc::clone(&mount_observed);
        let mount_probe_key = GlobalKey::<()>::new();
        let key_at_mount = mount_probe_key.clone();
        let mut pipeline = PluginPipeline::mount_with_boundary(
            &root,
            320.0,
            240.0,
            TextContextHandle::standalone(),
            move |widgets| {
                // The user root is built on the first draw, so mount uses a
                // registry entry of its own, installed after attach released
                // its binding lock.
                widgets.with_build_owner_mut(|owner| {
                    owner.register_global_key(&key_at_mount, flui_foundation::ElementId::new(73));
                });
                mount_observed_in_probe.set(matches!(
                    key_at_mount.current_element(),
                    KeyLookup::Mounted(_)
                ));
            },
        );
        assert!(
            mount_observed.get(),
            "the mount path keeps the plugin registry active after attach releases its lock"
        );
        assert_eq!(
            mount_probe_key.current_element(),
            KeyLookup::NoActiveRuntime,
            "the mount scope does not leak"
        );

        let observed = Rc::new(Cell::new(false));
        let observed_in_probe = Rc::clone(&observed);
        let key_in_probe = key.clone();
        pipeline.frame_boundary_probe = Some(Box::new(move || {
            observed_in_probe.set(matches!(
                key_in_probe.current_element(),
                KeyLookup::Mounted(_)
            ));
        }));

        let _scene = pipeline.draw_frame(320.0, 240.0);
        assert!(
            observed.get(),
            "draw_frame keeps the plugin registry active after build releases its lock"
        );
        assert_eq!(
            key.current_element(),
            KeyLookup::NoActiveRuntime,
            "the plugin scope does not leak"
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

        let root = KeyedRoot {
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

    #[test]
    fn whole_pixels_table() {
        let cases = [
            (0.0, 0),
            (99.2, 100),
            (-5.0, 0),
            (f64::NAN, 0),
            (f64::INFINITY, 0),
            (1e300, u32::MAX),
        ];
        for (extent, expected) in cases {
            assert_eq!(whole_pixels(extent), expected, "{extent}");
        }
    }
}
