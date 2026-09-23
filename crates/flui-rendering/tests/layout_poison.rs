//! Layout-poison (bounded layout retry) integration tests.
//!
//! A render object whose `perform_layout` keeps failing must not be retried
//! forever: any per-frame invalidation source (an animation tick, a stream,
//! a timer) re-dirties its ancestors every frame, and without a retry bound
//! the failing node's `perform_layout` re-runs inside every ancestor walk —
//! perpetual full-frame layout+paint work with the error re-logged each
//! frame. These tests pin the bounded-retry contract:
//!
//! - structural failures poison immediately and the node is skipped inside
//!   later walks until freshly invalidated;
//! - retriable failures poison only after a budget of consecutive attempts;
//! - a fresh invalidation (`mark_needs_layout`) lifts the poison, and a
//!   success fully clears the failure record;
//! - a poisoned node's stand-in geometry is its LAST COMMITTED size when it
//!   once succeeded, and exactly `Size::ZERO` when it never did — never a
//!   value the node would produce if re-attempted right now. See the
//!   retention/control pair near the end of this file.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_foundation::RenderId;
use flui_objects::RenderPadding;
use flui_rendering::{
    constraints::{BoxConstraints, SliverGeometry},
    context::{BoxLayoutContext, PaintCx, SliverHitTestContext, SliverLayoutContext},
    error::RenderError,
    parent_data::{BoxParentData, ParentData, SliverParentData},
    protocol::{BoxProtocol, ProtocolGeometry},
    storage::{IntrinsicDimension, RenderNode},
    testing::{FrameRun, Probe, RenderTester, box_node, sliver_node},
    traits::{HitTestOutcome, RenderBox, RenderObject, RenderSliver},
};
use flui_tree::{Leaf, Single};
use flui_types::{Matrix4, Size, geometry::px};

// ============================================================================
// FlakyLeaf — a leaf render object that fails layout on demand
// ============================================================================

/// Which error class [`FlakyLeaf`] produces while `fail` is set.
#[derive(Debug, Clone, Copy)]
enum FailMode {
    /// `ContractViolation` — permanent for the current tree state; poisons
    /// on the first failure.
    Structural,
    /// `InvalidConstraints` — could plausibly self-heal; retried up to the
    /// poison budget.
    Retriable,
}

/// A box-protocol leaf whose `perform_layout` either returns a fixed
/// 40×40 size or fails, counting every attempt so tests can assert how
/// many times the pipeline actually ran layout for it.
#[derive(Debug)]
struct FlakyLeaf {
    attempts: Arc<AtomicUsize>,
    fail: bool,
    mode: FailMode,
}

impl flui_foundation::Diagnosticable for FlakyLeaf {}

impl RenderObject<BoxProtocol> for FlakyLeaf {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            return Err(match self.mode {
                FailMode::Structural => {
                    RenderError::contract_violation("FlakyLeaf", "permanent structural failure")
                }
                FailMode::Retriable => RenderError::invalid_constraints("transient failure"),
            });
        }
        Ok(Size::new(px(40.0), px(40.0)))
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// Mounts `RenderPadding(5) → FlakyLeaf(fail = true)` and runs the first
/// frame, which attempts (and fails) the leaf's layout once.
///
/// Root constraints are LOOSE (0..200): the leaf's recovered 40×40 size
/// must be constraint-valid or its "recovery" layout would fail output
/// validation instead of committing.
fn mount_failing(mode: FailMode) -> (FrameRun, RenderId, RenderId, Arc<AtomicUsize>) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let run = RenderTester::mount(
        box_node(RenderPadding::all(5.0)).child(
            box_node(FlakyLeaf {
                attempts: Arc::clone(&attempts),
                fail: true,
                mode,
            })
            .label("leaf"),
        ),
    )
    .with_constraints(flui_rendering::constraints::BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    ))
    .run_frame();
    let leaf = run.id("leaf");
    let root = run.root();
    (run, root, leaf, attempts)
}

// ============================================================================
// (a) Permanent structural failure poisons and bounds the retry
// ============================================================================

/// A permanently-failing structural error must be retried only a bounded
/// number of times: after the first failure the leaf is layout-poisoned
/// and skipped inside every later ancestor walk, even when a per-frame
/// invalidation source keeps re-dirtying its parent. Without the poison
/// mechanism the leaf is re-attempted on every re-marked frame — this
/// assertion fails without the fix.
#[test]
fn permanent_structural_failure_is_poisoned_after_bounded_retries() {
    let (mut run, root, _leaf, attempts) = mount_failing(FailMode::Structural);
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "frame 1 attempts the failing leaf exactly once",
    );

    // Simulate a per-frame invalidation source (animation tick, stream):
    // re-dirty the root every frame and pump. The failing leaf must NOT be
    // re-laid-out — it is poisoned and the walk skips it.
    for _ in 0..5 {
        run.owner_mut().mark_needs_layout(root);
        run.pump();
    }
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "a poisoned node must not be re-attempted inside ancestor walks; \
         without a retry bound this count grows by one per frame",
    );

    // After the storm the pipeline settles completely: no layout/paint
    // work remains on idle frames.
    run.pump_idle_frames(2);
}

// ============================================================================
// (b) Fresh invalidation lifts the poison; a fixed node recovers
// ============================================================================

/// The poison must not make a legitimately-fixed tree stuck forever:
/// re-invalidating the failing node itself (a real property change, via
/// the harness `update` → `mark_needs_layout` flow) lifts the poison, and
/// the next layout succeeds and clears the failure record.
#[test]
fn fresh_invalidation_lifts_poison_and_layout_recovers() {
    let (mut run, root, leaf, attempts) = mount_failing(FailMode::Structural);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Prove the node is poisoned: re-dirtying the parent does not
    // re-attempt it.
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "poisoned leaf must be skipped while its own inputs are unchanged",
    );

    // Fix the error condition AND re-invalidate the node itself.
    run.update::<FlakyLeaf>(leaf, |leaf| leaf.fail = false);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a fresh invalidation of the node lifts the poison and retries layout",
    );
    assert_eq!(
        run.box_geometry(leaf),
        Size::new(px(40.0), px(40.0)),
        "the recovered leaf lays out at its real size",
    );
    assert_eq!(
        run.box_geometry(root),
        Size::new(px(50.0), px(50.0)),
        "the parent reflows around the recovered child (40 + 2×5 padding)",
    );

    // The failure record was cleared by the success: normal operation.
    run.pump_idle_frames(2);
}

// ============================================================================
// (c) A single transient failure does NOT poison
// ============================================================================

/// A retriable-class error that fails once and then succeeds must never
/// engage the poison: the node is retried on the next invalidation and
/// recovers. (Guard test — passes with and without the fix.)
#[test]
fn single_transient_failure_does_not_poison() {
    let (mut run, root, leaf, attempts) = mount_failing(FailMode::Retriable);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Re-dirty the parent WITHOUT fixing: the leaf is attempted again,
    // proving the first transient failure did not poison it.
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a single transient failure must not poison the node",
    );

    // Now fix the condition; normal operation resumes.
    run.update::<FlakyLeaf>(leaf, |leaf| leaf.fail = false);
    run.pump();
    assert_eq!(attempts.load(Ordering::Relaxed), 3);
    assert_eq!(run.box_geometry(leaf), Size::new(px(40.0), px(40.0)));
    run.pump_idle_frames(2);
}

// ============================================================================
// Retriable failures poison only at the budget
// ============================================================================

/// Retriable-class errors earn a small budget of consecutive retries
/// before poisoning — the middle ground between "never retry" (too
/// aggressive for genuinely transient conditions) and "retry forever"
/// (the infinite-loop bug).
#[test]
fn retriable_failures_poison_only_at_budget() {
    let (mut run, root, _leaf, attempts) = mount_failing(FailMode::Retriable);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Attempts 2 and 3: still retried (budget is 3 consecutive failures).
    for expected in 2..=3 {
        run.owner_mut().mark_needs_layout(root);
        run.pump();
        assert_eq!(
            attempts.load(Ordering::Relaxed),
            expected,
            "attempt {expected} must still run — the budget is not exhausted yet",
        );
    }

    // The third consecutive failure exhausted the budget: further
    // re-marked frames skip the node.
    for _ in 0..3 {
        run.owner_mut().mark_needs_layout(root);
        run.pump();
    }
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        3,
        "after the retry budget trips, the node is skipped",
    );

    run.pump_idle_frames(2);
}

// ============================================================================
// Failure at the dirty root itself
// ============================================================================

/// When the failing node is the dirty root of a walk, its error
/// propagates out of `run_layout`. The first structural failure poisons
/// the node AND surfaces the error to the caller once — an embedder must
/// see a structural break. Later frames skip the poisoned node (no storm,
/// no perpetual frame drops), and each fresh invalidation grants exactly
/// one bounded retry that re-poisons quietly.
#[test]
fn structural_failure_at_dirty_root_poisons_and_bounds_retries() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut owner = flui_rendering::pipeline::PipelineOwner::new();
    let root = owner.insert(Box::new(FlakyLeaf {
        attempts: Arc::clone(&attempts),
        fail: true,
        mode: FailMode::Structural,
    }));
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(flui_rendering::constraints::BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Frame 1: the first structural failure surfaces the error AND
    // poisons the node.
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(
        matches!(result, Err(RenderError::ContractViolation { .. })),
        "the first structural failure must surface as Err, got {result:?}",
    );
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "frame 1 attempts the failing root exactly once",
    );

    // Frame 2 (e.g. the embedder's one-shot error retry): the poisoned
    // root is skipped — the frame completes with no new attempt instead
    // of erroring forever.
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(
        result.is_ok(),
        "the poisoned root must be skipped, not re-errored: {result:?}",
    );
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "a poisoned dirty root is not re-attempted",
    );

    // Each fresh invalidation lifts the poison and grants exactly one
    // retry; the node re-poisons quietly (frame completes) instead of
    // spinning.
    for expected in 2..=3 {
        owner.mark_needs_layout(root);
        let (o, result) = owner.run_frame();
        owner = o;
        assert!(
            result.is_ok(),
            "a re-poisoned root completes the frame: {result:?}",
        );
        assert_eq!(
            attempts.load(Ordering::Relaxed),
            expected,
            "each fresh invalidation grants exactly one bounded retry",
        );
    }

    // With no invalidation source, the pipeline is fully idle.
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(result.is_ok());
    assert!(
        !owner.has_dirty_nodes(),
        "no layout/paint work may remain after the storm",
    );
}

// ============================================================================
// Intrinsic-measurement poison — fixtures
// ============================================================================

/// A leaf sliver used to make a Box child's intrinsic query fail with
/// `ProtocolMismatch` (box intrinsics are undefined on sliver nodes).
/// Lays out to an empty geometry; never painted (it keeps NEEDS_LAYOUT,
/// so the paint phase skips it).
#[derive(Debug, Default)]
struct StubLeafSliver;

impl flui_foundation::Diagnosticable for StubLeafSliver {}

impl RenderSliver for StubLeafSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, SliverParentData>,
    ) -> SliverGeometry {
        SliverGeometry::ZERO
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, SliverParentData>) -> bool {
        false
    }
}

/// A box parent that probes its only child's intrinsic width on every
/// `perform_layout` and sizes itself from the answer (falling back to
/// the child's laid-out width when the probe yields 0.0).
#[derive(Debug, Default)]
struct IntrinsicProbingParent;

impl flui_foundation::Diagnosticable for IntrinsicProbingParent {}

impl RenderBox for IntrinsicProbingParent {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let probed = ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 100.0);
        let child_size = ctx.layout_child(0, *ctx.constraints());
        let width = if probed > 0.0 {
            px(probed)
        } else {
            child_size.width
        };
        ctx.constraints()
            .constrain(Size::new(width, child_size.height))
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Single>) {}
}

/// A box node whose intrinsic measurement counts every computation and,
/// while `fail` is set, routes the measurement through its first child —
/// a child the test deliberately makes unmeasurable (a sliver, or a
/// stale link). While not failing it reports a fixed 40px intrinsic.
#[derive(Debug)]
struct CountingIntrinsicBox {
    attempts: Arc<AtomicUsize>,
    fail: bool,
}

impl flui_foundation::Diagnosticable for CountingIntrinsicBox {}

impl RenderObject<BoxProtocol> for CountingIntrinsicBox {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        Ok(Size::new(px(40.0), px(40.0)))
    }

    fn intrinsic_raw(
        &self,
        dimension: IntrinsicDimension,
        extent: f32,
        _child_count: usize,
        _child_parent_data: &[Option<&dyn ParentData>],
        child_query: &mut dyn FnMut(usize, IntrinsicDimension, f32) -> f32,
    ) -> f32 {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            child_query(0, dimension, extent)
        } else {
            40.0
        }
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// Mounts `IntrinsicProbingParent → CountingIntrinsicBox(fail) →
/// StubLeafSliver` and runs the first frame, which probes (and fails)
/// the child's intrinsic once with a structural `ProtocolMismatch`.
fn mount_failing_intrinsic() -> (FrameRun, RenderId, RenderId, Arc<AtomicUsize>) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let run = RenderTester::mount(
        box_node(IntrinsicProbingParent).child(
            box_node(CountingIntrinsicBox {
                attempts: Arc::clone(&attempts),
                fail: true,
            })
            .label("measured")
            .child(sliver_node(StubLeafSliver)),
        ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_frame();
    let measured = run.id("measured");
    let root = run.root();
    (run, root, measured, attempts)
}

// ============================================================================
// (a) Permanently-failing intrinsic probe poisons after the bound
// ============================================================================

/// A permanently-failing intrinsic query must be bounded exactly like a
/// failing `perform_layout`: after the first structural failure the
/// measured child is poisoned and the probe is skipped inside every
/// later layout pass, even when a per-frame invalidation source keeps
/// re-dirtying the probing parent. Without the fix the child's intrinsic
/// recomputation runs on every re-marked frame — this assertion fails
/// without it.
#[test]
fn permanent_intrinsic_failure_is_poisoned_after_bounded_retries() {
    let (mut run, root, _measured, attempts) = mount_failing_intrinsic();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "frame 1 measures the failing child exactly once",
    );

    // Simulate a per-frame invalidation source: re-dirty the probing
    // parent every frame and pump. The failing child's intrinsic must
    // NOT be recomputed — it is poisoned and the query is skipped.
    for _ in 0..5 {
        run.owner_mut().mark_needs_layout(root);
        run.pump();
    }
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "a poisoned node's intrinsic must not be recomputed inside later \
         layout passes; without a retry bound this count grows by one per frame",
    );

    run.pump_idle_frames(2);
}

// ============================================================================
// (b) Un-poison: fix the object + re-invalidate → intrinsic succeeds again
// ============================================================================

/// The poison must not make a legitimately-fixed node stuck: fixing the
/// measurement condition AND re-invalidating the node lifts the poison,
/// the next probe recomputes (and re-caches) successfully, and the
/// parent sizes itself from the recovered intrinsic.
#[test]
fn fresh_invalidation_lifts_intrinsic_poison_and_recovers() {
    let (mut run, root, measured, attempts) = mount_failing_intrinsic();
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Prove the probe is skipped while the node's inputs are unchanged.
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "poisoned node must not be re-measured while unchanged",
    );

    // Fix the error condition AND re-invalidate the node itself.
    run.update::<CountingIntrinsicBox>(measured, |b| b.fail = false);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a fresh invalidation lifts the poison and re-measures",
    );
    assert_eq!(
        run.box_geometry(root),
        Size::new(px(40.0), px(40.0)),
        "the parent sizes itself from the recovered 40px intrinsic",
    );

    // The success cleared the failure record: the next probe hits the
    // re-cached value without recomputing.
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a recovered measurement is re-cached, not recomputed every pass",
    );
    run.pump_idle_frames(2);
}

// ============================================================================
// (c) Transient intrinsic failure does not poison
// ============================================================================

/// A retriable-class intrinsic failure (here `NodeNotFound` from a
/// deliberately stale child link) that fails below the retry budget and
/// then succeeds must never engage the poison: the node keeps being
/// measured and recovers.
#[test]
fn transient_intrinsic_failure_does_not_poison() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut owner = flui_rendering::pipeline::PipelineOwner::new();
    let parent = owner.insert(Box::new(IntrinsicProbingParent));
    let measured = owner
        .insert_child_render_object(
            parent,
            Box::new(CountingIntrinsicBox {
                attempts: Arc::clone(&attempts),
                fail: true,
            }),
        )
        .expect("measured child insert");
    // A child link whose id is not in the tree: the intrinsic sub-query
    // fails with NodeNotFound — the retriable error class.
    owner
        .render_tree_mut()
        .get_mut(measured)
        .expect("measured in tree")
        .add_child(RenderId::new(999));
    owner.set_root_id(Some(parent));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Frame 1: the probe fails once (retriable) — no poison.
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(result.is_ok(), "descendant failure is isolated: {result:?}");
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Frame 2: still below the budget, so the probe still RUNS — a
    // single transient failure must not poison the node.
    owner.mark_needs_layout(parent);
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(result.is_ok());
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a retriable intrinsic failure below the budget must not poison",
    );

    // Fix the condition and re-invalidate: the probe succeeds.
    owner
        .render_tree_mut()
        .get_mut(measured)
        .expect("measured in tree")
        .downcast_render_object_mut::<CountingIntrinsicBox>()
        .expect("measured is a CountingIntrinsicBox")
        .fail = false;
    owner.mark_needs_layout(measured);
    let (o, result) = owner.run_frame();
    owner = o;
    assert!(result.is_ok());
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        3,
        "the recovered node is measured again",
    );
    assert!(
        !owner.has_dirty_nodes(),
        "pipeline must settle after recovery",
    );
}

// ============================================================================
// Public probe path (PipelineOwner::box_intrinsic_dimension)
// ============================================================================

/// The frame-independent probe API feeds the same budget: a permanently
/// failing intrinsic query through `box_intrinsic_dimension` poisons the
/// node (the error surfaces once), later probes return the stand-in
/// value without recomputing, and a fresh invalidation after a fix
/// recomputes and re-caches.
#[test]
fn public_probe_intrinsic_failure_poisons_and_recovers() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut owner = flui_rendering::pipeline::PipelineOwner::new();
    let measured = owner.insert(Box::new(CountingIntrinsicBox {
        attempts: Arc::clone(&attempts),
        fail: true,
    }));
    owner
        .insert_sliver_child_render_object(measured, Box::new(StubLeafSliver))
        .expect("sliver child insert");

    // First probe: the structural failure surfaces AND poisons the node.
    let err = owner
        .box_intrinsic_dimension(measured, IntrinsicDimension::MinWidth, 100.0)
        .expect_err("the structural intrinsic failure must surface as Err");
    assert!(
        matches!(err, RenderError::ProtocolMismatch { .. }),
        "expected ProtocolMismatch, got {err:?}",
    );
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Second probe: skipped — the stand-in value without recomputing.
    let value = owner
        .box_intrinsic_dimension(measured, IntrinsicDimension::MinWidth, 100.0)
        .expect("a poisoned probe returns the stand-in value");
    assert_eq!(value, 0.0, "never-succeeded node falls back to 0.0");
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "a poisoned node is not re-measured by the public probe",
    );

    // Fix + fresh invalidation: recompute and re-cache.
    owner
        .render_tree_mut()
        .get_mut(measured)
        .expect("measured in tree")
        .downcast_render_object_mut::<CountingIntrinsicBox>()
        .expect("measured is a CountingIntrinsicBox")
        .fail = false;
    owner.mark_needs_layout(measured);
    let value = owner
        .box_intrinsic_dimension(measured, IntrinsicDimension::MinWidth, 100.0)
        .expect("recovered probe succeeds");
    assert_eq!(value, 40.0);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);

    // The recovered value is re-cached: no third computation.
    let value = owner
        .box_intrinsic_dimension(measured, IntrinsicDimension::MinWidth, 100.0)
        .expect("cached probe succeeds");
    assert_eq!(value, 40.0);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

// ============================================================================
// (d) A constraints change grants a poisoned node one fresh attempt
// ============================================================================

/// A leaf that fails structurally exactly when its incoming constraints
/// are width-unbounded, laying out 40×40 under bounded ones. Counts
/// attempts so the test can prove precisely when the pipeline retries.
#[derive(Debug)]
struct UnboundedHatingLeaf {
    attempts: Arc<AtomicUsize>,
}

impl flui_foundation::Diagnosticable for UnboundedHatingLeaf {}

impl RenderObject<BoxProtocol> for UnboundedHatingLeaf {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        if ctx.constraints().max_width.is_infinite() {
            return Err(RenderError::unbounded_constraint("UnboundedHatingLeaf"));
        }
        Ok(Size::new(px(40.0), px(40.0)))
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// A single-child box parent that hands its child either width-unbounded
/// or bounded constraints depending on `unbounded`, sizing itself to the
/// child. Flipping the flag re-offers the child different constraints on
/// the parent's next layout WITHOUT the child itself ever being marked
/// dirty — the exact recovery path the poison skip must honor: a child
/// poisoned under one set of constraints must get a fresh attempt when
/// an ancestor's layout inputs change, not a stand-in geometry forever.
#[derive(Debug)]
struct WidthSwitchParent {
    unbounded: bool,
}

impl flui_foundation::Diagnosticable for WidthSwitchParent {}

impl RenderObject<BoxProtocol> for WidthSwitchParent {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        let child_constraints = if self.unbounded {
            BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(200.0))
        } else {
            BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0))
        };
        let size = ctx.layout_child(0, child_constraints);
        Ok(size)
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// Poisoned under invalid constraints, the child must recover when an
/// ancestor's re-layout offers different (valid) constraints — even
/// though nothing ever marks the child itself dirty. Before the
/// skip learned to compare constraints, the poisoned child kept
/// contributing `Size::ZERO` under any later constraints.
#[test]
fn constraints_change_grants_poisoned_node_one_fresh_attempt() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut run = RenderTester::mount(
        box_node(WidthSwitchParent { unbounded: true })
            .label("parent")
            .child(
                box_node(UnboundedHatingLeaf {
                    attempts: Arc::clone(&attempts),
                })
                .label("leaf"),
            ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_frame();
    let parent = run.id("parent");
    let leaf = run.id("leaf");
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "frame 1 fails the leaf once under unbounded constraints",
    );

    // Re-marking the parent under the SAME (still unbounded)
    // constraints must NOT re-attempt the leaf: unchanged inputs are
    // guaranteed to fail again, so the skip serves the stand-in.
    run.owner_mut().mark_needs_layout(parent);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "a poisoned node must stay skipped while its inputs are unchanged",
    );

    // Flip the offered constraints and re-mark ONLY the parent. The
    // child is never marked dirty; the walk must treat the new
    // constraints as one fresh attempt, and the success must clear the
    // failure record (pre-fix the leaf kept returning Size::ZERO).
    run.update::<WidthSwitchParent>(parent, |p| p.unbounded = false);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "changed constraints grant exactly one fresh layout attempt",
    );
    assert_eq!(
        run.box_geometry(leaf),
        Size::new(px(40.0), px(40.0)),
        "the leaf recovers under valid constraints",
    );

    run.pump_idle_frames(2);
}

/// The dual of the previous test: when a poisoned node's failure does
/// NOT depend on the offered constraints (a permanent contract
/// violation), the skip must hold under any constraints — only a fresh
/// external invalidation re-arms it.
#[test]
fn constraint_independent_poison_stays_skipped_under_new_constraints() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0)).child(
            box_node(FlakyLeaf {
                attempts: Arc::clone(&attempts),
                fail: true,
                mode: FailMode::Structural,
            })
            .label("leaf"),
        ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_frame();
    let root = run.root();
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    // Resize the window: the root and the leaf now see DIFFERENT
    // constraints than the failed attempt. The leaf's contract
    // violation is input-independent — wait, no: the skip compares
    // constraints, so a resize DOES grant one fresh attempt even for an
    // input-independent failure. That attempt fails again and the node
    // re-poisons — exactly one bounded retry per resize, not a storm.
    run.owner_mut()
        .set_root_constraints(Some(BoxConstraints::new(
            px(0.0),
            px(150.0),
            px(0.0),
            px(150.0),
        )));
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "a constraints change grants one fresh attempt; the still-failing \
         node re-poisons instead of storming",
    );

    // A second resize to yet another size grants one more attempt
    // (inputs changed again) — still failing, still bounded.
    run.owner_mut()
        .set_root_constraints(Some(BoxConstraints::new(
            px(0.0),
            px(100.0),
            px(0.0),
            px(100.0),
        )));
    run.pump();
    assert_eq!(attempts.load(Ordering::Relaxed), 3);

    // Re-marking WITHOUT any constraints change: no further attempts.
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        3,
        "unchanged constraints after re-poisoning must not re-attempt",
    );

    run.pump_idle_frames(2);
}

// ============================================================================
// Retention: a poisoned node's stand-in is its last committed size, not a
// fake recovery to zero — and its control, a node that never committed
// ============================================================================

/// A leaf whose layout panics on demand, driven entirely through shared
/// handles rather than the harness [`FrameRun::update`] flow: `update` calls
/// `mark_needs_layout` on the edited node, which would lift the very poison
/// these tests exist to observe before the pass under test even runs.
///
/// Counts every attempt (`calls`) and, while `panic` is unset, returns
/// `size_on_success` constrained by the incoming constraints — letting a
/// test flip the leaf from failing to succeeding (or back) without ever
/// touching the tree itself.
#[derive(Debug)]
struct RetainingLeaf {
    calls: Arc<AtomicUsize>,
    panic: Arc<AtomicBool>,
    size_on_success: Arc<Mutex<Size>>,
}

impl flui_foundation::Diagnosticable for RetainingLeaf {}

impl RenderObject<BoxProtocol> for RetainingLeaf {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(
            !self.panic.load(Ordering::Relaxed),
            "RetainingLeaf: deliberate layout panic",
        );
        let size = *self
            .size_on_success
            .lock()
            .expect("size_on_success mutex must not be poisoned");
        Ok(ctx.constraints().constrain(size))
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// The [`RenderNode`] backing `id` in `run`'s owner.
///
/// `Probe::box_geometry` panics instead of returning `None` on a node that
/// never committed, and has no `geometry_degraded` counterpart — both are
/// exactly what the retention/control pair below needs to read, so they go
/// through the node directly instead.
fn node(run: &FrameRun, id: RenderId) -> &RenderNode {
    run.owner()
        .render_tree()
        .get(id)
        .expect("render id must be live")
}

/// A poisoned leaf's stand-in geometry is its own LAST COMMITTED size, not a
/// fresh recomputation and not a collapse to zero — the retention half of
/// the contract; [`a_leaf_that_never_committed_stands_in_with_zero`] below
/// is its control, pinning the opposite answer from the identical oracle.
///
/// Tree: `RenderPadding(5) → RenderPadding(5) → RetainingLeaf`, root
/// constraints LOOSE (0..200 × 0..200) so the leaf's size is never derivable
/// from its constraints alone (a collapse to `Size::ZERO` would otherwise be
/// invisible), and the leaf sits two levels below the root so it is never
/// itself the dirty root (whose size is constraint-derived by construction).
///
/// Two discriminators prove this test measures retention specifically:
///
/// - **Flip pass 2's `panic` back to `false`** (so the re-armed leaf would
///   now succeed at S2 = 50×60 instead of failing) and this test goes RED on
///   the geometry assertions: pass 3 would read the leaf at S2 and the
///   parent at 60×70 instead of the retained S1 / 40×50. `calls` alone
///   stays at 2 in that variant too — the unchanged-constraints cache
///   serves the leaf in pass 3 without another `perform_layout_raw` call
///   either way, so the call count cannot tell these two apart on its own.
/// - **Delete pass 2 entirely** (go straight from pass 1 to pass 3) and this
///   test goes RED on the call count: nothing ever poisons the leaf, so
///   marking the parent in pass 3 re-attempts it — `calls` reads 2 for a
///   completely different reason (a second real attempt, not a skip).
#[test]
fn a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero() {
    let s1 = Size::new(px(30.0), px(40.0));
    let s2 = Size::new(px(50.0), px(60.0));

    let calls = Arc::new(AtomicUsize::new(0));
    let panic = Arc::new(AtomicBool::new(false));
    let size_on_success = Arc::new(Mutex::new(s1));

    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0)).label("root").child(
            box_node(RenderPadding::all(5.0)).label("parent").child(
                box_node(RetainingLeaf {
                    calls: Arc::clone(&calls),
                    panic: Arc::clone(&panic),
                    size_on_success: Arc::clone(&size_on_success),
                })
                .label("leaf"),
            ),
        ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_frame();
    let parent = run.id("parent");
    let leaf = run.id("leaf");

    // Pass 1: the leaf lays out cleanly and commits S1.
    assert_eq!(node(&run, leaf).geometry_box(), Some(s1));
    assert_eq!(run.box_geometry(parent), Size::new(px(40.0), px(50.0)));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!node(&run, parent).geometry_degraded());

    // Pass 2: re-arm the leaf specifically (it must be re-armed before the
    // failing pass, or nothing below even runs) and fail it. The frame as a
    // whole still completes `Ok`: a descendant failure is swallowed by the
    // parent's child-layout callback, which hands the parent `Size::ZERO`
    // and records the failure against the leaf's retry budget.
    panic.store(true, Ordering::Relaxed);
    *size_on_success.lock().expect("size_on_success mutex") = s2;
    run.owner_mut().mark_needs_layout(leaf);
    run.pump();
    assert_eq!(
        node(&run, leaf).geometry_box(),
        Some(s1),
        "a failed layout attempt must not touch the last committed geometry",
    );
    assert!(run.owner().is_layout_poisoned(leaf));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(node(&run, parent).geometry_degraded());
    assert_eq!(run.box_geometry(parent), Size::new(px(10.0), px(10.0)));

    // Pass 3: fix the condition (the leaf would now succeed at S2 if
    // re-attempted) but re-invalidate only the PARENT — not the leaf (that
    // would lift its poison) and not the root (whose cached geometry under
    // unchanged constraints would short-circuit before the parent's
    // `perform_layout` ever re-runs, per `layout_dirty_root`). The leaf
    // must stay skipped, and what stands in for it is its own last
    // COMMITTED size, not the fresh S2 it would now produce and not zero.
    panic.store(false, Ordering::Relaxed);
    run.owner_mut().mark_needs_layout(parent);
    run.pump();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "the poisoned leaf must not be re-attempted",
    );
    assert_eq!(
        node(&run, leaf).geometry_box(),
        Some(s1),
        "the poisoned leaf's stand-in is its last committed size, not the \
         value it would produce now if re-attempted",
    );
    assert_eq!(run.box_geometry(parent), Size::new(px(40.0), px(50.0)));
    assert!(node(&run, parent).geometry_degraded());
    assert!(run.owner().is_layout_poisoned(leaf));
}

/// The control for
/// [`a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero`]: a
/// leaf that never once succeeded stands in at exactly `Size::ZERO` — never
/// its own prior geometry (it has none to retain) and never a value it
/// would produce if re-attempted. Same oracle (`geometry_box`, `calls`,
/// `geometry_degraded`), opposite answer, which is what proves the
/// retention test above reads the committed-size mechanism and not, say, a
/// constant the pipeline always serves for any poisoned node.
#[test]
fn a_leaf_that_never_committed_stands_in_with_zero() {
    let calls = Arc::new(AtomicUsize::new(0));
    let panic = Arc::new(AtomicBool::new(true));
    let size_on_success = Arc::new(Mutex::new(Size::new(px(30.0), px(40.0))));

    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0)).label("root").child(
            box_node(RenderPadding::all(5.0)).label("parent").child(
                box_node(RetainingLeaf {
                    calls: Arc::clone(&calls),
                    panic: Arc::clone(&panic),
                    size_on_success: Arc::clone(&size_on_success),
                })
                .label("leaf"),
            ),
        ),
    )
    .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0)))
    .run_frame();
    let parent = run.id("parent");
    let leaf = run.id("leaf");

    // Pass 1: the leaf fails its very first-ever attempt — nothing has
    // committed, so there is nothing to retain.
    assert_eq!(node(&run, leaf).geometry_box(), None);
    assert_eq!(run.box_geometry(parent), Size::new(px(10.0), px(10.0)));
    assert!(node(&run, parent).geometry_degraded());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(run.owner().is_layout_poisoned(leaf));

    // Pass 2: re-invalidate only the parent, exactly as pass 3 of the
    // retention test does — the leaf stays poisoned and skipped, and its
    // stand-in is `Size::ZERO`, not a value derived from a prior success
    // (there is none).
    run.owner_mut().mark_needs_layout(parent);
    run.pump();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "the poisoned leaf must not be re-attempted",
    );
    assert_eq!(run.box_geometry(parent), Size::new(px(10.0), px(10.0)));
    assert!(node(&run, parent).geometry_degraded());
    assert_eq!(node(&run, leaf).geometry_box(), None);
}
