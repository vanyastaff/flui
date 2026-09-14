//! The canonical headless bootstrap: mount a root [`View`], drive its first
//! frame, and hand the result to a tree-bound [`HeadlessBinding`].
//!
//! [`HeadlessBinding::with_tree`] deliberately does *no* bootstrap — it takes
//! owners that are already mounted, rooted, and laid out. Getting them into
//! that state is an eight-step sequence whose ordering is load-bearing at
//! nearly every step, and it used to be hand-rolled once per harness — eight
//! copies across flui-widgets, the facade examples, and the facade tests. They
//! drifted, in ways a green test suite could not see:
//!
//! - the `flui` golden-screenshot suite bootstrapped with a bare
//!   `PipelineOwner::run_frame` instead of the layout↔build fixpoint, so any
//!   build-during-layout content (a `SliverAppBar`'s delegate child, a
//!   persistent-header body) was captured unbuilt — while `examples/screenshot.rs`,
//!   derived from the same code, carried a comment warning about exactly that;
//! - no copy ran the lazy-sliver service pass, so the bootstrap frame was not
//!   the same frame [`HeadlessBinding::pump_frame`] runs.
//!
//! [`HeadlessBinding::mount_root`] is that sequence, owned once. Its contract is
//! the one property the copies kept losing: **the bootstrap frame is the same
//! frame `pump_frame` runs** — same fixpoint helper, same service pass, same
//! owner scope — so a tree that settles on frame one headlessly settles on
//! frame one on screen.
//!
//! What stays with the caller is what genuinely differs per harness: which
//! presentation scopes wrap the root (`FocusRoot`, `VsyncScope`,
//! `GestureArenaScope` all live in `flui-widgets`, which this crate must never
//! depend on), which extra capabilities the `BuildOwner` carries, and what the
//! root constraints are.
//!
//! # Example
//!
//! ```rust,ignore
//! let mut binding = HeadlessBinding::new();
//! let mounted = binding.mount_root(
//!     &GestureArenaScope::new(binding.arena().clone(), FocusRoot::new(my_widget)),
//!     MountOwners::fresh(),
//!     MountOptions::tight(800.0, 600.0),
//! );
//! // `mounted.root_element` is the RootRenderView; `content_element` is the
//! // GestureArenaScope the caller passed in.
//! assert!(mounted.painted);
//! ```

use flui_foundation::RenderId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementId, ElementTree, RootRenderView, View};

use crate::HeadlessBinding;

/// Seed size for the headless [`RootRenderView`] when mount constraints are
/// unbounded on an axis — matches the production
/// `WidgetsBinding` default root-view seed (800×600).
const DEFAULT_ROOT_VIEW_SIZE: (f32, f32) = (800.0, 600.0);

/// Logical width × height seeded into [`RootRenderView`] from mount
/// constraints' biggest size, falling back per-axis when unbounded.
fn root_view_size(constraints: &BoxConstraints) -> (f32, f32) {
    let biggest = constraints.biggest();
    let width = if biggest.width.is_finite() {
        biggest.width.0
    } else {
        DEFAULT_ROOT_VIEW_SIZE.0
    };
    let height = if biggest.height.is_finite() {
        biggest.height.0
    } else {
        DEFAULT_ROOT_VIEW_SIZE.1
    };
    (width, height)
}

/// The three owners a bootstrap consumes.
///
/// Supply them yourself when the harness must configure one first — a
/// `BuildOwner` carrying an extra platform capability, or a [`PipelineCell`]
/// the caller keeps a clone of for geometry reads. [`MountOwners::fresh`]
/// covers the common case.
#[derive(Debug)]
pub struct MountOwners {
    /// Owns the dirty heap and the external build inbox.
    pub build_owner: BuildOwner,
    /// The element tree the root is mounted into.
    pub tree: ElementTree,
    /// The **shared** render owner. Clone it before handing it over if the
    /// caller needs its own handle; the element tree holds a clone too.
    pub pipeline_owner: PipelineCell,
}

impl MountOwners {
    /// A fresh, unconfigured owner triple.
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            build_owner: BuildOwner::new(),
            tree: ElementTree::new(),
            pipeline_owner: PipelineCell::new(PipelineOwner::new()),
        }
    }

    /// A fresh triple over a caller-supplied [`PipelineCell`], for a harness
    /// that keeps its own clone of the same shared owner.
    #[must_use]
    pub fn with_pipeline_owner(pipeline_owner: PipelineCell) -> Self {
        Self {
            build_owner: BuildOwner::new(),
            tree: ElementTree::new(),
            pipeline_owner,
        }
    }
}

impl Default for MountOwners {
    fn default() -> Self {
        Self::fresh()
    }
}

/// Which build capabilities the bootstrap installs on the `BuildOwner` before
/// the mount build pass.
///
/// Installation must happen *before* the mount, not after: a
/// `ViewState::init_state` runs inside that first `build_scope` and already
/// asks for these handles. A `FutureBuilder`/`StreamBuilder` that subscribes
/// there would silently never poll if the async driver arrived late.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildCapabilities {
    /// The full set — async driver, post-frame, owner-local post-frame, and
    /// interaction dispatch. What every ordinary harness wants.
    #[default]
    Installed,
    /// Only the async driver.
    ///
    /// Withholds the post-frame and interaction handles so a test can assert
    /// how code behaves when `BuildContext::post_frame_handle()` returns
    /// `None` — a real, reachable configuration for an embedder that drives
    /// frames itself. The async driver still goes in: withholding it too
    /// would change *which* capability the test is about, since the mount
    /// `build_scope` depends on it.
    AsyncDriverOnly,
}

/// Root constraints and capability policy for a bootstrap.
#[derive(Debug, Clone, Copy)]
pub struct MountOptions {
    /// Constraints installed on the pipeline root before the first frame.
    /// Setting them is what marks the root dirty for that frame's layout.
    pub constraints: BoxConstraints,
    /// Which capabilities the `BuildOwner` carries into the mount pass.
    pub capabilities: BuildCapabilities,
}

impl MountOptions {
    /// Bootstrap under `constraints`, with the full capability set.
    #[must_use]
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            capabilities: BuildCapabilities::Installed,
        }
    }

    /// Bootstrap under constraints forcing exactly `width` × `height` logical
    /// pixels — the surface-sized root a screenshot or a golden wants.
    #[must_use]
    pub fn tight(width: f32, height: f32) -> Self {
        Self::new(BoxConstraints::tight(Size::new(px(width), px(height))))
    }

    /// Bootstrap under loose constraints from zero up to `max` × `max`.
    #[must_use]
    pub fn loose(max: f32) -> Self {
        Self::new(BoxConstraints::loose(Size::new(px(max), px(max))))
    }

    /// Override the capability policy (see [`BuildCapabilities`]).
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: BuildCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// What a bootstrap discovered and produced.
///
/// The owners themselves are now inside the binding; reach them through
/// [`HeadlessBinding::build_owner_mut`] / [`HeadlessBinding::tree_mut`], and
/// the shared render owner through the [`PipelineCell`] the caller passed in.
#[derive(Debug, Clone)]
pub struct Mounted {
    /// The element-tree root — always a [`RootRenderView`] / `RootRenderElement`.
    ///
    /// A root swap ([`HeadlessBinding::swap_root_view`]) targets this id and
    /// must pass another [`RootRenderView`] of the same concrete child type.
    pub root_element: ElementId,
    /// The caller's view element — the single child of [`root_element`](Self::root_element)
    /// after the first build. Geometry and recovery probes that talk about
    /// "the mounted widget" want this, not the `RootRenderView` wrapper.
    pub content_element: ElementId,
    /// Pipeline render root — the `RenderView` installed by `RootRenderElement`
    /// as [`PipelineOwner::root_id`](flui_rendering::PipelineOwner::root_id).
    ///
    /// See [`logical_render_root`](Self::logical_render_root) for the caller's
    /// own render node below it.
    pub render_root: RenderId,
    /// Children of [`render_root`](Self::render_root), in pipeline order.
    ///
    /// Exposed rather than resolved because what counts as "the caller's own
    /// root" is presentation policy owned by the widget layer, not by this
    /// crate (e.g. a harness may insert an `Align` loosener under the
    /// `RenderView` before the caller's first render object). Prefer
    /// [`logical_render_root`](Self::logical_render_root) for the common
    /// single-child-of-`RenderView` case.
    pub render_root_children: Vec<RenderId>,
    /// Logical size seeded into the [`RootRenderView`] at bootstrap — pass the
    /// same pair when swapping the root so the `RenderView` configuration
    /// stays stable.
    pub root_view_size: (f32, f32),
    /// Whether the bootstrap frame committed a layer tree. Read it through
    /// [`HeadlessBinding::layer_tree`].
    pub painted: bool,
}

impl Mounted {
    /// The immediate child of the pipeline
    /// [`RenderView`](flui_rendering::view::RenderView), or the `RenderView`
    /// itself when it has none yet (a recovered `ErrorView` is render-less).
    ///
    /// Widget harnesses that insert an `Align` loosener under the `RenderView`
    /// then take that Align's child as the caller's root — see
    /// `flui_widgets::testing::lay_out`.
    ///
    /// # Panics
    ///
    /// If the `RenderView` has more than one child.
    #[must_use]
    pub fn logical_render_root(&self) -> RenderId {
        assert!(
            self.render_root_children.len() <= 1,
            "logical_render_root expects the RootRenderView's RenderView to wrap at most one \
             child, but the mounted render root has {} children; read \
             render_root_children directly for other shapes",
            self.render_root_children.len(),
        );
        self.render_root_children
            .first()
            .copied()
            .unwrap_or(self.render_root)
    }
}

impl HeadlessBinding {
    /// Mount `root`, drive its first frame, and bind the result to this
    /// binding — the canonical headless bootstrap.
    ///
    /// In order, and every step's position is load-bearing:
    ///
    /// 1. install the build capabilities named by
    ///    [`MountOptions::capabilities`] — **before** the mount, because
    ///    `init_state` asks for them during it;
    /// 2. wrap `root` in [`RootRenderView`] (same production shape as
    ///    [`flui_view::WidgetsBinding::attach_root_widget`]) and mount it
    ///    inside [`enter_owner_scope`](Self::enter_owner_scope), so
    ///    `RootRenderElement` installs `PipelineOwner.root_id` and lifecycle
    ///    callbacks see the same active interaction lane they see during
    ///    [`pump_frame`](Self::pump_frame);
    /// 3. schedule and run the initial build pass, reconciling and mounting the
    ///    whole subtree's render objects;
    /// 4. **verify** the single parentless render node equals the already-
    ///    installed pipeline root (the scan does not invent `root_id`);
    /// 5. install root constraints from [`MountOptions::constraints`];
    /// 6. run the layout↔build fixpoint — the same
    ///    `run_frame_with_layout_builders` helper `pump_frame`'s pipeline step
    ///    and the live `draw_frame` use, never a bare
    ///    `PipelineOwner::run_frame`, which does not service
    ///    build-during-layout content;
    /// 7. run the lazy-sliver service pass, as every pumped frame's pipeline
    ///    does;
    /// 8. bind the owners under the requested capability policy, retaining the
    ///    committed layer tree.
    ///
    /// # What the bootstrap is NOT
    ///
    /// It is a **pipeline step, not a whole scheduler frame.**
    /// [`pump_frame`](Self::pump_frame) wraps the same pipeline in
    /// [`flui_scheduler::UpdateScheduler::drive_frame_with_lane`], which adds begin-frame
    /// work (transient callbacks, microtasks, the async-driver poll), the
    /// stationary-device re-hit-test, and `end_frame`'s post-frame callbacks.
    /// None of those run here. A post-frame callback scheduled from
    /// `init_state`, and an async task queued there, both run on the first
    /// [`pump_frame`](Self::pump_frame) instead.
    ///
    /// That is deliberate, and it is the production shape: `attach_root_widget`
    /// mounts and lays out, and the first `draw_frame` comes after. Folding a
    /// whole frame into the mount would make a harness *less* faithful — a
    /// `StreamBuilder` would deliver an already-queued event before any test
    /// could observe the `Waiting` state Flutter guarantees.
    ///
    /// # Panics
    ///
    /// If the mounted subtree does not produce exactly one parentless render
    /// node equal to `PipelineOwner.root_id`, or if the bootstrap frame fails.
    /// Both are regressions in code under test, and both are loud on purpose:
    /// a harness that swallowed them would report a tree that never rendered
    /// as a passing test.
    pub fn mount_root<V: View + Clone + 'static>(
        &mut self,
        root: &V,
        owners: MountOwners,
        options: MountOptions,
    ) -> Mounted {
        let MountOwners {
            mut build_owner,
            mut tree,
            pipeline_owner,
        } = owners;

        match options.capabilities {
            BuildCapabilities::Installed => {
                self.install_build_capabilities(&mut build_owner);
                // Before the mount, not after the bind: a `ViewState::init_state`
                // running in the `build_scope` below is exactly where the
                // capability is supposed to be acquired.
                self.install_hit_test_capability(&mut build_owner, &pipeline_owner);
            }
            BuildCapabilities::AsyncDriverOnly => {
                build_owner.set_async_driver(self.scheduler().async_driver().clone());
            }
        }

        let view_size = root_view_size(&options.constraints);
        let root_render_view = RootRenderView::new(root.clone(), view_size.0, view_size.1);

        let root_element = self.enter_owner_scope(|| {
            let root_element = tree.mount_root_with_pipeline_owner(
                &root_render_view,
                Some(pipeline_owner.clone()),
                &mut build_owner.element_owner_mut(),
            );
            // Reconcile and mount the whole subtree: children's render objects
            // attach under the RenderView during this pass.
            build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
            build_owner.build_scope(&mut tree);
            root_element
        });

        let content_element = {
            let kids = tree
                .get(root_element)
                .expect("BUG: RootRenderView element must remain after mount build")
                .child_ids();
            assert_eq!(
                kids.len(),
                1,
                "RootRenderView must reconcile exactly one content child after the \
                 bootstrap build; got {}",
                kids.len(),
            );
            kids[0]
        };

        let (render_root, render_root_children) = pipeline_owner.with(|owner| {
            let installed = owner.root_id().expect(
                "BUG: RootRenderElement must install PipelineOwner.root_id during mount",
            );
            let render_tree = owner.render_tree();
            let mut roots = render_tree
                .iter()
                .map(|(id, _)| id)
                .filter(|id| render_tree.parent(*id).is_none());
            let discovered = roots
                .next()
                .expect("the mounted subtree must have a render root");
            assert!(
                roots.next().is_none(),
                "expected exactly one parentless render node after mount",
            );
            assert_eq!(
                discovered, installed,
                "parentless render node must equal PipelineOwner.root_id — \
                 HeadlessBinding must not invent a pipeline root by scanning",
            );
            (installed, render_tree.children(installed).to_vec())
        });

        pipeline_owner.with_mut(|owner| {
            // RootRenderElement already set root_id; only constraints remain.
            // Fresh root constraints mark the root dirty for the frame below.
            owner.set_root_constraints(Some(options.constraints));
        });

        // The PIPELINE step, deliberately not a whole scheduler frame — see
        // this method's docs for exactly what that excludes and why. It mirrors
        // production, where `attach_root_widget` mounts and lays out and the
        // first `draw_frame` comes after.
        let committed_layer_tree = self.enter_owner_scope(|| {
            let layer_tree = build_owner
                .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
                .expect("the bootstrap frame must succeed");
            // Same trailing step every pumped frame's pipeline runs: the
            // safety net for a frame that hit the fixpoint's pass bound (the
            // loop itself services lazy-sliver requests between passes). A
            // no-op on a converged frame; without it the bootstrap's pipeline
            // step would not be the one `pump_frame` runs.
            build_owner.service_child_requests(&mut tree, &pipeline_owner);
            layer_tree
        });

        let painted = committed_layer_tree.is_some();
        // Through the capability-aware door: the public `bind_tree*` installs
        // the full set unconditionally, which would undo an `AsyncDriverOnly`
        // request the moment the bootstrap finished.
        self.bind_tree_with_capabilities(
            build_owner,
            tree,
            pipeline_owner,
            committed_layer_tree,
            options.capabilities,
        );

        Mounted {
            root_element,
            content_element,
            render_root,
            render_root_children,
            root_view_size: view_size,
            painted,
        }
    }
}
