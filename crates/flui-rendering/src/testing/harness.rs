//! The symmetric render-object test harness.
//!
//! [`RenderTester`] is a small config (a [`TreeNode`] spec plus root
//! constraints) that builds a real [`PipelineOwner`] and drives it to one
//! of two depths, chosen by symmetric verbs that mirror the pipeline's own
//! methods:
//!
//! - [`run_layout`](RenderTester::run_layout) -> [`LayoutRun`]: stops at the
//!   `Layout` phase, for inspecting committed geometry/offsets without a
//!   full frame (Box and Sliver alike);
//! - [`run_frame`](RenderTester::run_frame) -> [`FrameRun`]: a full frame
//!   (layout -> compositing -> paint), adding layer-tree structure, picture
//!   bounds, dirty-state checks, and multi-frame helpers ([`pump`](FrameRun::pump),
//!   [`pump_frames`](FrameRun::pump_frames), [`advance_layout`](FrameRun::advance_layout),
//!   [`advance_paint`](FrameRun::advance_paint), [`simulate`](FrameRun::simulate)).
//!
//! Both run results implement [`Probe`], so Box and Sliver are inspected
//! through the identical surface. They are two types only because the
//! typestate forbids one live struct from holding both a `<Layout>` and an
//! `<Idle>` owner.

use flui_foundation::RenderId;
use flui_layer::LayerTree;
use flui_types::{Rect, Size, geometry::px};

use crate::{
    constraints::BoxConstraints,
    error::RenderError,
    pipeline::{Compositing, Idle, Layout, PaintPhase, PipelineOwner, Semantics},
    storage::RenderNode,
    testing::{
        inspect::{self, Probe},
        report::FrameReport,
        tree::{self, RenderLabelRegistry, TreeNode},
    },
};

/// Default root constraints when a test does not specify any: a loose
/// `0..=800 x 0..=600` box, large enough that most trees lay out at their
/// natural size.
fn default_constraints() -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0))
}

/// Depth of `id` in the render tree (root = 0), for paint-dirty enqueue.
fn node_depth<P: crate::pipeline::PipelinePhase>(owner: &PipelineOwner<P>, id: RenderId) -> usize {
    owner.render_tree().depth(id).unwrap_or(0) as usize
}

/// Marks `id` for repaint: compositing bits (layer structure may change when
/// paint effects like opacity cross the fully-opaque threshold) then paint.
fn mark_needs_paint<P: crate::pipeline::PipelinePhase>(owner: &mut PipelineOwner<P>, id: RenderId) {
    let depth = node_depth(owner, id);
    owner.add_node_needing_compositing_bits_update(id, depth);
    owner.add_node_needing_paint(id, depth);
}

/// Downcasts the render object at `id` to `T` and runs `edit`.
///
/// Dispatches on the node's Box/Sliver protocol; `T` must match the concrete
/// type stored at `id`.
fn edit_object<T: 'static, P: crate::pipeline::PipelinePhase>(
    owner: &mut PipelineOwner<P>,
    id: RenderId,
    edit: impl FnOnce(&mut T),
) {
    let node = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("update: render id must be live");
    match node {
        RenderNode::Box(entry) => edit(
            entry
                .render_object_mut()
                .as_any_mut()
                .downcast_mut::<T>()
                .expect("update: render object is not of the requested type"),
        ),
        RenderNode::Sliver(entry) => edit(
            entry
                .render_object_mut()
                .as_any_mut()
                .downcast_mut::<T>()
                .expect("update: render object is not of the requested type"),
        ),
    }
}

/// A configured-but-not-yet-run render-object test.
///
/// Build it with [`mount`](RenderTester::mount), optionally set the root
/// constraints, then pick a run depth.
pub struct RenderTester {
    spec: TreeNode,
    constraints: Option<BoxConstraints>,
    semantics_enabled: bool,
}

impl std::fmt::Debug for RenderTester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderTester")
            .field("spec", &self.spec)
            .field("constraints", &self.constraints)
            .field("semantics_enabled", &self.semantics_enabled)
            .finish()
    }
}

impl RenderTester {
    /// Configures a test from a [`TreeNode`] spec (the root must be a Box
    /// node — see [`crate::testing::tree::mount`]).
    #[must_use]
    pub fn mount(spec: TreeNode) -> Self {
        Self {
            spec,
            constraints: None,
            semantics_enabled: false,
        }
    }

    /// Sets the root constraints applied on the first layout pass.
    #[must_use]
    pub fn with_constraints(mut self, constraints: BoxConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    /// Sets tight root constraints forcing the root to exactly `size`.
    #[must_use]
    pub fn with_size(mut self, size: Size) -> Self {
        self.constraints = Some(BoxConstraints::tight(size));
        self
    }

    /// Enables semantics on the built [`PipelineOwner`] before any phase
    /// runs (ADR-0014 D1: lazily creates a `SemanticsOwner`).
    ///
    /// Without this, `run_semantics` (and therefore [`Self::run_to_semantics`]
    /// / [`Self::run_frame`]'s semantics phase) is a no-op — semantics stays
    /// disabled by default so every other harness test is unaffected.
    #[must_use]
    pub fn with_semantics_enabled(mut self) -> Self {
        self.semantics_enabled = true;
        self
    }

    /// Builds the owner, mounts the spec, and seeds the root + constraints.
    fn build(self) -> (PipelineOwner<Idle>, RenderId, RenderLabelRegistry) {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(&mut owner, self.spec);
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(self.constraints.unwrap_or_else(default_constraints)));
        if self.semantics_enabled {
            owner.set_semantics_enabled(true);
        }
        (owner, root_id, registry)
    }

    /// Drives the tree through layout only, returning a [`LayoutRun`].
    #[must_use]
    pub fn run_layout(self) -> LayoutRun {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner
            .run_layout()
            .expect("run_layout must succeed for a well-formed test tree");
        LayoutRun {
            owner,
            root_id,
            registry,
        }
    }

    /// Drives the tree through a full frame, returning a [`FrameRun`].
    #[must_use]
    pub fn run_frame(self) -> FrameRun {
        let (owner, root_id, registry) = self.build();
        let (owner, result) = owner.run_frame();
        let layer_tree = result.expect("run_frame must succeed for a well-formed test tree");
        FrameRun {
            owner,
            root_id,
            registry,
            layer_tree,
        }
    }

    /// Drives the tree through layout only, returning `Err` on any
    /// [`RenderError`] (e.g. a panicking [`perform_layout_raw`]) instead of
    /// panicking the test process.
    ///
    /// [`perform_layout_raw`]: crate::protocol::RenderObject::perform_layout_raw
    pub fn try_run_layout(self) -> Result<LayoutRun, RenderError> {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner.run_layout()?;
        Ok(LayoutRun {
            owner,
            root_id,
            registry,
        })
    }

    /// Drives the tree through a full frame, returning `Err` on any
    /// [`RenderError`] (e.g. a panicking [`paint_raw`]) instead of panicking
    /// the test process.
    ///
    /// [`paint_raw`]: crate::protocol::RenderObject::paint_raw
    pub fn try_run_frame(self) -> Result<FrameRun, RenderError> {
        let (owner, root_id, registry) = self.build();
        let (owner, result) = owner.run_frame();
        let layer_tree = result?;
        Ok(FrameRun {
            owner,
            root_id,
            registry,
            layer_tree,
        })
    }

    /// Drives the tree through layout and asserts it fails, returning the
    /// [`RenderError`].
    ///
    /// Panics if layout succeeds — the invariant being tested is that this
    /// tree must produce a layout error.
    #[must_use]
    pub fn expect_layout_error(self) -> RenderError {
        self.try_run_layout()
            .expect_err("tree was expected to fail layout but layout succeeded")
    }
}

/// The result of a [`RenderTester::run_layout`]: a pipeline parked in the
/// `Layout` phase with committed geometry/offsets ready to inspect.
#[derive(Debug)]
pub struct LayoutRun {
    owner: PipelineOwner<Layout>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
}

impl LayoutRun {
    /// The root node's id.
    #[must_use]
    pub fn root(&self) -> RenderId {
        self.root_id
    }

    /// The underlying pipeline owner (escape hatch for advanced inspection).
    #[must_use]
    pub fn owner(&self) -> &PipelineOwner<Layout> {
        &self.owner
    }

    /// Mutable access to the underlying pipeline owner.
    pub fn owner_mut(&mut self) -> &mut PipelineOwner<Layout> {
        &mut self.owner
    }

    /// Downcasts the render object at `id` to `T`, runs `edit`, and marks the
    /// node layout-dirty. Pair with [`relayout`](LayoutRun::relayout) to re-run
    /// layout and observe the new geometry — the layout-only analog of
    /// [`FrameRun::update`] + [`FrameRun::pump`].
    ///
    /// Works for Box and Sliver nodes; `T` must match the concrete type at `id`.
    ///
    /// Panics if the id is stale or is not a `T`.
    pub fn update<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_object(&mut self.owner, id, edit);
        self.owner.mark_needs_layout(id);
    }

    /// Downcasts the render object at `id` to `T`, runs `edit`, and marks the
    /// node paint-dirty (opacity, color, transform, … — no layout pass).
    ///
    /// Pair with a full [`FrameRun`] if the change must be painted; layout-only
    /// runs do not execute the paint phase.
    ///
    /// Panics if the id is stale or is not a `T`.
    pub fn update_paint<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_object(&mut self.owner, id, edit);
        mark_needs_paint(&mut self.owner, id);
    }

    /// Marks `id` paint-dirty without mutating its render object.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        mark_needs_paint(&mut self.owner, id);
    }

    /// Re-runs the layout phase, committing fresh geometry/offsets for any
    /// nodes marked dirty since the previous pass.
    pub fn relayout(&mut self) {
        self.owner
            .run_layout()
            .expect("relayout must succeed for a well-formed test tree");
    }
}

impl Probe for LayoutRun {
    type Phase = Layout;

    fn pipeline(&self) -> &PipelineOwner<Layout> {
        &self.owner
    }

    fn registry(&self) -> &RenderLabelRegistry {
        &self.registry
    }
}

/// The result of a [`RenderTester::run_frame`]: a pipeline returned to
/// `Idle` plus the layer tree the frame produced.
#[derive(Debug)]
pub struct FrameRun {
    owner: PipelineOwner<Idle>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
    layer_tree: Option<LayerTree>,
}

impl FrameRun {
    /// The root node's id.
    #[must_use]
    pub fn root(&self) -> RenderId {
        self.root_id
    }

    /// Whether the most recent frame produced a layer tree.
    #[must_use]
    pub fn painted(&self) -> bool {
        self.layer_tree.is_some()
    }

    /// Whether the pipeline has no dirty nodes left after the frame.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.owner.has_dirty_nodes()
    }

    /// The layer tree from the most recent frame, if any.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.layer_tree.as_ref()
    }

    /// The composited layer kinds in pre-order (empty if nothing painted).
    #[must_use]
    pub fn structure(&self) -> Vec<&'static str> {
        self.layer_tree
            .as_ref()
            .map(inspect::layer_structure)
            .unwrap_or_default()
    }

    /// The bounds of the first picture layer, if any.
    #[must_use]
    pub fn picture_bounds(&self) -> Option<Rect> {
        self.layer_tree
            .as_ref()
            .and_then(inspect::first_picture_bounds)
    }

    /// A [`FrameReport`] snapshot of the most recent frame.
    #[must_use]
    pub fn report(&self) -> FrameReport {
        FrameReport {
            painted: self.painted(),
            structure: self
                .layer_tree
                .as_ref()
                .map(inspect::layer_structure_with_depth)
                .unwrap_or_default(),
            picture_bounds: self.picture_bounds(),
            dirty: self.owner.has_dirty_nodes(),
        }
    }

    /// Runs another frame (after mutating the tree via
    /// [`owner_mut`](FrameRun::owner_mut)) and returns its report.
    ///
    /// A frame with no dirty work produces no layer tree, mirroring the
    /// production idle-frame behavior.
    pub fn pump(&mut self) -> FrameReport {
        let owner = std::mem::take(&mut self.owner);
        let (owner, result) = owner.run_frame();
        self.owner = owner;
        self.layer_tree = result.expect("pump frame must succeed for a well-formed test tree");
        self.report()
    }

    /// Runs `count` consecutive frames with no mutations between them.
    ///
    /// Returns one [`FrameReport`] per frame. Idle frames (no dirty work)
    /// produce `painted: false`, mirroring production.
    pub fn pump_frames(&mut self, count: usize) -> Vec<FrameReport> {
        (0..count).map(|_| self.pump()).collect()
    }

    /// Runs `count` idle frames and panics if any frame paints or leaves the
    /// pipeline dirty — the strict helper for "skip N settled frames" checks.
    pub fn pump_idle_frames(&mut self, count: usize) {
        for (i, report) in self.pump_frames(count).into_iter().enumerate() {
            assert!(
                !report.painted,
                "idle frame {i} must not paint (no dirty work)",
            );
            assert!(
                !report.dirty,
                "idle frame {i} must leave the pipeline clean",
            );
        }
    }

    /// Layout mutation plus one frame — shorthand for
    /// [`update`](FrameRun::update) + [`pump`](FrameRun::pump).
    pub fn advance_layout<T: 'static>(
        &mut self,
        id: RenderId,
        edit: impl FnOnce(&mut T),
    ) -> FrameReport {
        self.update(id, edit);
        self.pump()
    }

    /// Paint-only mutation plus one frame — shorthand for
    /// [`update_paint`](FrameRun::update_paint) + [`pump`](FrameRun::pump).
    pub fn advance_paint<T: 'static>(
        &mut self,
        id: RenderId,
        edit: impl FnOnce(&mut T),
    ) -> FrameReport {
        self.update_paint(id, edit);
        self.pump()
    }

    /// Simulates a multi-frame animation on deterministic simulated time.
    ///
    /// For each `t` in `ticks`, calls `on_tick(t, self)` so the caller can
    /// advance a controller and mutate the tree (`update`, `update_paint`, …),
    /// then pumps exactly one frame. Returns one report per tick.
    ///
    /// ```
    /// # use flui_rendering::testing::{RenderTester, Probe, box_node};
    /// # use flui_rendering::prelude::*;
    /// # use flui_tree::Leaf;
    /// # use flui_types::{Size, geometry::px};
    /// # #[derive(Debug, Default)]
    /// # struct FixedBox(f32);
    /// # impl flui_foundation::Diagnosticable for FixedBox {}
    /// # impl RenderBox for FixedBox {
    /// #     type Arity = Leaf;
    /// #     type ParentData = BoxParentData;
    /// #     fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
    /// #         Size::new(px(self.0), px(self.0))
    /// #     }
    /// #     fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    /// # }
    /// let mut run = RenderTester::mount(box_node(FixedBox(40.0)).label("root"))
    ///     .run_frame();
    /// let root = run.id("root");
    /// run.simulate([0.0, 0.5, 1.0], |t, run| {
    ///     let side = 40.0 + 20.0 * t as f32;
    ///     run.update::<FixedBox>(root, |b| b.0 = side);
    /// });
    /// assert_eq!(
    ///     run.box_geometry(root),
    ///     Size::new(px(60.0), px(60.0)),
    /// );
    /// ```
    pub fn simulate<I, F>(&mut self, ticks: I, mut on_tick: F) -> Vec<FrameReport>
    where
        I: IntoIterator<Item = f64>,
        F: FnMut(f64, &mut Self),
    {
        let mut reports = Vec::new();
        for t in ticks {
            on_tick(t, self);
            reports.push(self.pump());
        }
        reports
    }

    /// Alpha of the first opacity layer in the most recent frame, if any.
    #[must_use]
    pub fn opacity_alpha(&self) -> Option<f32> {
        self.layer_tree
            .as_ref()
            .and_then(inspect::first_opacity_alpha)
    }

    /// Whether the most recent frame's layer tree contains a picture layer.
    #[must_use]
    pub fn has_picture_layer(&self) -> bool {
        self.layer_tree
            .as_ref()
            .is_some_and(inspect::has_picture_layer)
    }

    /// The underlying pipeline owner (escape hatch for advanced inspection).
    #[must_use]
    pub fn owner(&self) -> &PipelineOwner<Idle> {
        &self.owner
    }

    /// Mutable access to the underlying pipeline owner — mutate the tree
    /// here between [`pump`](FrameRun::pump) calls.
    pub fn owner_mut(&mut self) -> &mut PipelineOwner<Idle> {
        &mut self.owner
    }

    /// Downcasts the render object at `id` to `T`, runs `edit`, and marks the
    /// node layout-dirty — the multi-frame mutate-then-[`pump`](FrameRun::pump)
    /// flow in one call.
    ///
    /// Works for Box and Sliver nodes; `T` must match the concrete type at `id`.
    ///
    /// Panics if the id is stale or is not a `T`.
    ///
    /// ```
    /// # use flui_rendering::testing::{RenderTester, Probe, box_node};
    /// # use flui_rendering::prelude::*;
    /// # use flui_tree::Leaf;
    /// # use flui_types::{Size, geometry::px};
    /// # #[derive(Debug, Default)]
    /// # struct FixedBox(f32);
    /// # impl flui_foundation::Diagnosticable for FixedBox {}
    /// # impl RenderBox for FixedBox {
    /// #     type Arity = Leaf;
    /// #     type ParentData = BoxParentData;
    /// #     fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
    /// #         Size::new(px(self.0), px(self.0))
    /// #     }
    /// #     fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    /// # }
    /// let mut run = RenderTester::mount(box_node(FixedBox(40.0)).label("root"))
    ///     .run_frame();
    /// let root = run.root();
    /// run.update::<FixedBox>(root, |b| b.0 = 60.0);
    /// run.pump();
    /// assert_eq!(
    ///     run.box_geometry(root),
    ///     Size::new(px(60.0), px(60.0)),
    /// );
    /// ```
    pub fn update<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_object(&mut self.owner, id, edit);
        self.owner.mark_needs_layout(id);
    }

    /// Downcasts the render object at `id` to `T`, runs `edit`, and marks the
    /// node paint-dirty. Pair with [`pump`](FrameRun::pump).
    ///
    /// Panics if the id is stale or is not a `T`.
    pub fn update_paint<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_object(&mut self.owner, id, edit);
        mark_needs_paint(&mut self.owner, id);
    }

    /// Marks `id` paint-dirty without mutating its render object.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        mark_needs_paint(&mut self.owner, id);
    }

    /// Serializes the most recent frame's layer tree to a stable indented
    /// text form, or returns `"<no layer tree>"` when nothing was painted.
    ///
    /// Use with `insta::assert_snapshot!` to pin the layer structure over
    /// time. The format is stable across runs (2-decimal floats, insertion-order
    /// children, no hash iteration).
    #[must_use]
    pub fn snapshot(&self) -> String {
        super::snapshot::snapshot_tree(self.layer_tree.as_ref())
    }

    /// Serializes the subtree at the layer boundary for `node`, or returns
    /// `"<no layer tree>"` when nothing was painted.
    ///
    /// Falls back to the full tree until a `RenderId → LayerId` mapping is
    /// available; see [`super::snapshot::snapshot_subtree`] for details.
    #[must_use]
    pub fn snapshot_of(&self, node: RenderId) -> String {
        super::snapshot::snapshot_subtree(self.layer_tree.as_ref(), node)
    }

    /// Returns every [`DrawCommandSummary`] reachable from the most recent
    /// frame's layer tree in pre-order, or an empty `Vec` when nothing was
    /// painted.
    ///
    /// [`DrawCommandSummary`]: super::snapshot::DrawCommandSummary
    #[must_use]
    pub fn display_commands(&self) -> Vec<super::snapshot::DrawCommandSummary> {
        super::snapshot::commands_of(self.layer_tree.as_ref())
    }

    /// Panics unless at least one painted command satisfies `pred`.
    ///
    /// The panic message includes the full snapshot so it is immediately clear
    /// what was actually painted. Unlike Flutter's `paints..something()` this
    /// assertion never passes silently when `pred` never matches.
    pub fn assert_paints_any(&self, pred: impl Fn(&super::snapshot::DrawCommandSummary) -> bool) {
        super::snapshot::assert_any(self.layer_tree.as_ref(), pred);
    }
}

impl Probe for FrameRun {
    type Phase = Idle;

    fn pipeline(&self) -> &PipelineOwner<Idle> {
        &self.owner
    }

    fn registry(&self) -> &RenderLabelRegistry {
        &self.registry
    }
}

// ============================================================================
// Phase-tagged intermediate run handles (Task 5)
// ============================================================================

impl RenderTester {
    /// Drives the tree through layout → compositing → paint, then stops,
    /// returning a [`PaintRun`] parked in the `PaintPhase`.
    ///
    /// Use this when a test needs only the painted [`LayerTree`] and does not
    /// require a full round-trip back to `Idle`. The snapshot and predicate
    /// helpers on [`PaintRun`] are identical to those on [`FrameRun`], so
    /// tests can be promoted to `run_frame` without changing their assertions.
    ///
    /// `LayoutRun` deliberately has no `snapshot` method — that method lives
    /// exclusively on paint-phase handles. The compile-time proof:
    ///
    /// ```compile_fail
    /// # use flui_rendering::testing::{box_node, RenderTester};
    /// # use flui_rendering::prelude::*;
    /// # use flui_tree::Leaf;
    /// # use flui_types::{Size, geometry::px};
    /// # #[derive(Debug, Default)]
    /// # struct FixedBox;
    /// # impl flui_foundation::Diagnosticable for FixedBox {}
    /// # impl RenderBox for FixedBox {
    /// #     type Arity = Leaf;
    /// #     type ParentData = BoxParentData;
    /// #     fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size { Size::ZERO }
    /// #     fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    /// # }
    /// let run = RenderTester::mount(box_node(FixedBox)).run_layout();
    /// let _ = run.snapshot(); // error: no method `snapshot` found for `LayoutRun`
    /// ```
    #[must_use]
    pub fn run_to_paint(self) -> PaintRun {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner
            .run_layout()
            .expect("run_layout must succeed for a well-formed test tree");
        let mut owner = owner.into_compositing();
        owner
            .run_compositing()
            .expect("run_compositing must succeed for a well-formed test tree");
        let mut owner = owner.into_paint();
        owner
            .run_paint()
            .expect("run_paint must succeed for a well-formed test tree");
        // take_layer_tree() is on impl<Phase: PipelinePhase> PipelineOwner<Phase>
        // so it is reachable here on PipelineOwner<PaintPhase>.
        let layer_tree = owner.take_layer_tree();
        PaintRun {
            owner,
            root_id,
            registry,
            layer_tree,
        }
    }

    /// Drives the tree through layout → compositing, then stops, returning a
    /// [`CompositingRun`] parked in the `Compositing` phase.
    ///
    /// The compositing pass updates each node's compositing-bits flags but
    /// produces no layer tree. Use [`run_to_paint`](Self::run_to_paint) or
    /// [`run_frame`](Self::run_frame) when you need the painted output.
    #[must_use]
    pub fn run_to_compositing(self) -> CompositingRun {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner
            .run_layout()
            .expect("run_layout must succeed for a well-formed test tree");
        let mut owner = owner.into_compositing();
        owner
            .run_compositing()
            .expect("run_compositing must succeed for a well-formed test tree");
        CompositingRun {
            owner,
            root_id,
            registry,
        }
    }

    /// Drives the tree through all four phases (layout → compositing → paint →
    /// semantics), then stops, returning a [`SemanticsRun`] parked in the
    /// `Semantics` phase.
    ///
    /// The semantics pass builds a `SemanticsOwner` only when the caller has
    /// installed one and enabled semantics on the pipeline. Raw owner access
    /// via [`Probe::pipeline`] is the inspection surface.
    #[must_use]
    pub fn run_to_semantics(self) -> SemanticsRun {
        let (owner, root_id, registry) = self.build();
        let mut owner = owner.into_layout();
        owner
            .run_layout()
            .expect("run_layout must succeed for a well-formed test tree");
        let mut owner = owner.into_compositing();
        owner
            .run_compositing()
            .expect("run_compositing must succeed for a well-formed test tree");
        let mut owner = owner.into_paint();
        owner
            .run_paint()
            .expect("run_paint must succeed for a well-formed test tree");
        let mut owner = owner.into_semantics();
        owner
            .run_semantics()
            .expect("run_semantics must succeed for a well-formed test tree");
        SemanticsRun {
            owner,
            root_id,
            registry,
        }
    }
}

// ============================================================================
// PaintRun
// ============================================================================

/// The result of [`RenderTester::run_to_paint`]: a pipeline parked in the
/// `PaintPhase` with the painted [`LayerTree`] ready for snapshot assertions.
///
/// Snapshot and predicate helpers delegate to the shared free functions in
/// [`super::snapshot`], keeping the implementation DRY with [`FrameRun`].
///
/// The compile-time proof that `LayoutRun` has no `snapshot` method lives on
/// [`RenderTester::run_to_paint`].
#[derive(Debug)]
pub struct PaintRun {
    owner: PipelineOwner<PaintPhase>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
    /// The layer tree produced by `run_paint`, extracted with `take_layer_tree`
    /// immediately after the paint pass so the owner can be stored without
    /// holding a borrow.
    layer_tree: Option<LayerTree>,
}

impl PaintRun {
    /// The root node's id.
    #[must_use]
    pub fn root(&self) -> RenderId {
        self.root_id
    }

    /// The layer tree produced by the paint pass, if anything was painted.
    ///
    /// Returns `None` only when no root is set or the root has no paint work —
    /// the common case for a well-formed test tree is `Some`.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.layer_tree.as_ref()
    }

    /// Serializes the painted layer tree to a stable indented text form, or
    /// returns `"<no layer tree>"` when nothing was painted.
    ///
    /// Use with `insta::assert_snapshot!` to pin layer structure over time.
    #[must_use]
    pub fn snapshot(&self) -> String {
        super::snapshot::snapshot_tree(self.layer_tree.as_ref())
    }

    /// Serializes the subtree at the layer boundary for `node`.
    ///
    /// Falls back to the full tree until a `RenderId → LayerId` mapping is
    /// available; see [`super::snapshot::snapshot_subtree`] for details.
    #[must_use]
    pub fn snapshot_of(&self, node: RenderId) -> String {
        super::snapshot::snapshot_subtree(self.layer_tree.as_ref(), node)
    }

    /// Returns every [`DrawCommandSummary`] reachable from the painted layer
    /// tree in pre-order, or an empty `Vec` when nothing was painted.
    ///
    /// [`DrawCommandSummary`]: super::snapshot::DrawCommandSummary
    #[must_use]
    pub fn display_commands(&self) -> Vec<super::snapshot::DrawCommandSummary> {
        super::snapshot::commands_of(self.layer_tree.as_ref())
    }

    /// Panics unless at least one painted command satisfies `pred`.
    ///
    /// The panic message includes the full snapshot so the failure is
    /// self-describing.
    pub fn assert_paints_any(&self, pred: impl Fn(&super::snapshot::DrawCommandSummary) -> bool) {
        super::snapshot::assert_any(self.layer_tree.as_ref(), pred);
    }
}

impl Probe for PaintRun {
    type Phase = PaintPhase;

    fn pipeline(&self) -> &PipelineOwner<PaintPhase> {
        &self.owner
    }

    fn registry(&self) -> &RenderLabelRegistry {
        &self.registry
    }
}

// ============================================================================
// CompositingRun
// ============================================================================

/// The result of [`RenderTester::run_to_compositing`]: a pipeline parked in
/// the `Compositing` phase after compositing-bits have been updated but before
/// any paint or layer-tree work.
///
/// No snapshot helpers are provided because the compositing pass produces no
/// layer tree. Use [`run_to_paint`](RenderTester::run_to_paint) or
/// [`run_frame`](RenderTester::run_frame) when you need painted output.
#[derive(Debug)]
pub struct CompositingRun {
    owner: PipelineOwner<Compositing>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
}

impl CompositingRun {
    /// The root node's id.
    #[must_use]
    pub fn root(&self) -> RenderId {
        self.root_id
    }
}

impl Probe for CompositingRun {
    type Phase = Compositing;

    fn pipeline(&self) -> &PipelineOwner<Compositing> {
        &self.owner
    }

    fn registry(&self) -> &RenderLabelRegistry {
        &self.registry
    }
}

// ============================================================================
// SemanticsRun
// ============================================================================

/// The result of [`RenderTester::run_to_semantics`]: a pipeline parked in the
/// `Semantics` phase after all four pipeline phases have executed.
///
/// Semantics inspection goes through raw owner access via [`Probe::pipeline`].
#[derive(Debug)]
pub struct SemanticsRun {
    owner: PipelineOwner<Semantics>,
    root_id: RenderId,
    registry: RenderLabelRegistry,
}

impl SemanticsRun {
    /// The root node's id.
    #[must_use]
    pub fn root(&self) -> RenderId {
        self.root_id
    }

    /// The assembled semantics owner, if semantics was enabled for this run
    /// (see [`RenderTester::with_semantics_enabled`]).
    ///
    /// `None` when semantics was never enabled — `run_semantics` is then a
    /// no-op and no `SemanticsOwner` was ever lazily created (ADR-0014 D1).
    #[must_use]
    pub fn semantics_owner(&self) -> Option<&crate::semantics::SemanticsOwner> {
        self.owner.semantics_owner()
    }
}

impl Probe for SemanticsRun {
    type Phase = Semantics;

    fn pipeline(&self) -> &PipelineOwner<Semantics> {
        &self.owner
    }

    fn registry(&self) -> &RenderLabelRegistry {
        &self.registry
    }
}

// `has_overflow` moved to `flui-objects/tests/helpers.rs` (flui-objects extraction
// plan §2): it downcasts to concrete objects (RenderFittedBox/Stack/Viewport)
// which now live in flui-objects, not flui-rendering. Moved verbatim so
// `tests/harness_snapshot.rs` (also moved) keeps using it from its new home.
