//! The canonical headless bootstrap: mount a root [`View`], drive its first
//! frame, and hand the result to a tree-bound [`HeadlessBinding`].
//!
//! [`HeadlessBinding::with_tree`] deliberately does *no* bootstrap — it takes
//! owners that are already mounted, rooted, and laid out. Getting them into
//! that state is an eight-step sequence whose ordering is load-bearing at
//! nearly every step, and it used to be hand-rolled once per harness. The
//! copies drifted, in ways a green test suite could not see:
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
//! assert!(mounted.painted);
//! ```

use flui_foundation::RenderId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{BuildOwner, ElementId, ElementTree, View};

use crate::HeadlessBinding;

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
    /// The mounted root element — the node a root swap targets.
    pub root_element: ElementId,
    /// The single parentless render node, installed as the pipeline root.
    ///
    /// When the caller wraps its widget in presentation scopes (a `FocusRoot`
    /// contributes a transparent traversal anchor), this is the *anchor*, not
    /// the caller's own root. See [`logical_render_root`](Self::logical_render_root).
    pub render_root: RenderId,
    /// Children of [`render_root`](Self::render_root), in pipeline order.
    ///
    /// Exposed rather than resolved because what counts as "the caller's own
    /// root" is presentation policy owned by the widget layer, not by this
    /// crate.
    pub render_root_children: Vec<RenderId>,
    /// Whether the bootstrap frame committed a layer tree. Read it through
    /// [`HeadlessBinding::layer_tree`].
    pub painted: bool,
}

impl Mounted {
    /// The caller's own render root, for the common single-anchor shape: the
    /// only child of [`render_root`](Self::render_root), or the anchor itself
    /// when it has none.
    ///
    /// A childless anchor is a real case, not a bug — a recovered `ErrorView`
    /// is render-less — so this reports the anchor rather than panicking, and
    /// a caller that must distinguish the two reads
    /// [`render_root_children`](Self::render_root_children) directly.
    ///
    /// # Panics
    ///
    /// If the anchor has more than one child: a presentation anchor wraps at
    /// most one logical root, so a second child means the caller mounted
    /// something other than the single-anchor shape this helper is for.
    #[must_use]
    pub fn logical_render_root(&self) -> RenderId {
        assert!(
            self.render_root_children.len() <= 1,
            "logical_render_root expects a presentation anchor wrapping at most one \
             logical render root, but the mounted render root has {} children; read \
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
    /// 2. mount `root` inside [`enter_owner_scope`](Self::enter_owner_scope),
    ///    so lifecycle callbacks see the same active interaction lane they see
    ///    during [`pump_frame`](Self::pump_frame);
    /// 3. schedule and run the initial build pass, reconciling and mounting the
    ///    whole subtree's render objects;
    /// 4. discover the single parentless render node;
    /// 5. install it as the pipeline root under
    ///    [`MountOptions::constraints`];
    /// 6. drive the layout↔build fixpoint frame — the same
    ///    `run_frame_with_layout_builders` helper `pump_frame` and the live
    ///    `draw_frame` use, never a bare `PipelineOwner::run_frame`, which
    ///    never services build-during-layout content;
    /// 7. run the lazy-sliver service pass, as every pumped frame does;
    /// 8. bind the owners, retaining the committed layer tree.
    ///
    /// # Panics
    ///
    /// If the mounted subtree does not produce exactly one parentless render
    /// node, or if the bootstrap frame fails. Both are regressions in code
    /// under test, and both are loud on purpose: a harness that swallowed them
    /// would report a tree that never rendered as a passing test.
    pub fn mount_root(
        &mut self,
        root: &dyn View,
        owners: MountOwners,
        options: MountOptions,
    ) -> Mounted {
        let MountOwners {
            mut build_owner,
            mut tree,
            pipeline_owner,
        } = owners;

        match options.capabilities {
            BuildCapabilities::Installed => self.install_build_capabilities(&mut build_owner),
            BuildCapabilities::AsyncDriverOnly => {
                build_owner.set_async_driver(self.scheduler().async_driver().clone());
            }
        }

        let root_element = self.enter_owner_scope(|| {
            let root_element = tree.mount_root_with_pipeline_owner(
                root,
                Some(pipeline_owner.clone()),
                &mut build_owner.element_owner_mut(),
            );
            // Reconcile and mount the whole subtree: children's render objects
            // attach to their parents during this pass.
            build_owner.schedule_build_for(root_element, 0, flui_view::RebuildReason::InitialMount);
            build_owner.build_scope(&mut tree);
            root_element
        });

        let (render_root, render_root_children) = pipeline_owner.with(|owner| {
            let render_tree = owner.render_tree();
            let mut roots = render_tree
                .iter()
                .map(|(id, _)| id)
                .filter(|id| render_tree.parent(*id).is_none());
            let root = roots
                .next()
                .expect("the mounted subtree must have a render root");
            assert!(
                roots.next().is_none(),
                "expected exactly one parentless render node after mount",
            );
            (root, render_tree.children(root).to_vec())
        });

        pipeline_owner.with_mut(|owner| {
            owner.set_root_id(Some(render_root));
            // Fresh root constraints mark the root dirty for the frame below.
            owner.set_root_constraints(Some(options.constraints));
        });

        let committed_layer_tree = self.enter_owner_scope(|| {
            let layer_tree = build_owner
                .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
                .expect("the bootstrap frame must succeed");
            // Same trailing step every pumped frame runs: drain the lazy-sliver
            // build requests and retain bands layout emitted. A no-op when no
            // lazy sliver is mounted; without it the bootstrap frame would not
            // be the frame `pump_frame` runs.
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
            render_root,
            render_root_children,
            painted,
        }
    }
}
