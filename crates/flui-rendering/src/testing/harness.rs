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
    pipeline::{Idle, Layout, PipelineOwner},
    testing::{
        inspect::{self, Probe},
        report::FrameReport,
        tree::{self, IdRegistry, TreeNode},
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

/// Downcasts the Box render object at `id` to `T` and runs `edit`.
fn edit_box_object<T: 'static, P: crate::pipeline::PipelinePhase>(
    owner: &mut PipelineOwner<P>,
    id: RenderId,
    edit: impl FnOnce(&mut T),
) {
    let entry = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("update: render id must be live")
        .as_box_mut()
        .expect("update: node must be a Box render object");
    let object = entry
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<T>()
        .expect("update: render object is not of the requested type");
    edit(object);
}

/// A configured-but-not-yet-run render-object test.
///
/// Build it with [`mount`](RenderTester::mount), optionally set the root
/// constraints, then pick a run depth.
pub struct RenderTester {
    spec: TreeNode,
    constraints: Option<BoxConstraints>,
}

impl RenderTester {
    /// Configures a test from a [`TreeNode`] spec (the root must be a Box
    /// node — see [`crate::testing::tree::mount`]).
    #[must_use]
    pub fn mount(spec: TreeNode) -> Self {
        Self {
            spec,
            constraints: None,
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

    /// Builds the owner, mounts the spec, and seeds the root + constraints.
    fn build(self) -> (PipelineOwner<Idle>, RenderId, IdRegistry) {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(&mut owner, self.spec);
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(self.constraints.unwrap_or_else(default_constraints)));
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
}

/// The result of a [`RenderTester::run_layout`]: a pipeline parked in the
/// `Layout` phase with committed geometry/offsets ready to inspect.
pub struct LayoutRun {
    owner: PipelineOwner<Layout>,
    root_id: RenderId,
    registry: IdRegistry,
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

    /// Downcasts the Box render object at `id` to `T`, runs `edit`, and marks
    /// the node layout-dirty. Pair with [`relayout`](LayoutRun::relayout) to
    /// re-run layout and observe the new geometry — the layout-only analog of
    /// [`FrameRun::update`] + [`FrameRun::pump`].
    ///
    /// Panics if the id is stale, is not a Box node, or is not a `T`.
    pub fn update<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_box_object(&mut self.owner, id, edit);
        self.owner.mark_needs_layout(id);
    }

    /// Downcasts the Box render object at `id` to `T`, runs `edit`, and marks
    /// the node paint-dirty (opacity, color, transform, … — no layout pass).
    ///
    /// Pair with a full [`FrameRun`] if the change must be painted; layout-only
    /// runs do not execute the paint phase.
    ///
    /// Panics if the id is stale, is not a Box node, or is not a `T`.
    pub fn update_paint<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_box_object(&mut self.owner, id, edit);
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

    fn registry(&self) -> &IdRegistry {
        &self.registry
    }
}

/// The result of a [`RenderTester::run_frame`]: a pipeline returned to
/// `Idle` plus the layer tree the frame produced.
pub struct FrameRun {
    owner: PipelineOwner<Idle>,
    root_id: RenderId,
    registry: IdRegistry,
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
    /// # use flui_rendering::objects::{RenderColoredBox, RenderPadding};
    /// # use flui_rendering::testing::{RenderTester, Probe, box_node};
    /// # use flui_types::{EdgeInsets, Offset, geometry::px};
    /// let mut run = RenderTester::mount(
    ///     box_node(RenderPadding::all(5.0))
    ///         .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    /// )
    /// .run_frame();
    /// let child = run.id("child");
    /// let pad = run.root();
    /// run.simulate([0.0, 0.5, 1.0], |t, run| {
    ///     let padding = 5.0 + 50.0 * t as f32;
    ///     run.update::<RenderPadding>(pad, |p| {
    ///         p.set_padding(EdgeInsets::all(px(padding)));
    ///     });
    /// });
    /// assert_eq!(run.offset(child), Offset::new(px(55.0), px(55.0)));
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

    /// Downcasts the Box render object at `id` to `T`, runs `edit`, and marks
    /// the node layout-dirty — the multi-frame mutate-then-[`pump`](FrameRun::pump)
    /// flow in one call, collapsing the
    /// `get_mut → as_box_mut → render_object_mut → as_any_mut → downcast_mut`
    /// dance.
    ///
    /// Panics if the id is stale, is not a Box node, or is not a `T`.
    ///
    /// ```
    /// # use flui_rendering::objects::{RenderColoredBox, RenderPadding};
    /// # use flui_rendering::testing::{RenderTester, Probe, box_node};
    /// # use flui_types::{EdgeInsets, Offset, geometry::px};
    /// let mut run = RenderTester::mount(
    ///     box_node(RenderPadding::all(5.0))
    ///         .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    /// )
    /// .run_frame();
    /// let pad = run.root();
    /// run.update::<RenderPadding>(pad, |p| p.set_padding(EdgeInsets::all(px(20.0))));
    /// run.pump();
    /// assert_eq!(run.offset(run.id("child")), Offset::new(px(20.0), px(20.0)));
    /// ```
    pub fn update<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_box_object(&mut self.owner, id, edit);
        self.owner.mark_needs_layout(id);
    }

    /// Downcasts the Box render object at `id` to `T`, runs `edit`, and marks
    /// the node paint-dirty. Pair with [`pump`](FrameRun::pump).
    ///
    /// Panics if the id is stale, is not a Box node, or is not a `T`.
    pub fn update_paint<T: 'static>(&mut self, id: RenderId, edit: impl FnOnce(&mut T)) {
        edit_box_object(&mut self.owner, id, edit);
        mark_needs_paint(&mut self.owner, id);
    }

    /// Marks `id` paint-dirty without mutating its render object.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        mark_needs_paint(&mut self.owner, id);
    }
}

impl Probe for FrameRun {
    type Phase = Idle;

    fn pipeline(&self) -> &PipelineOwner<Idle> {
        &self.owner
    }

    fn registry(&self) -> &IdRegistry {
        &self.registry
    }
}
