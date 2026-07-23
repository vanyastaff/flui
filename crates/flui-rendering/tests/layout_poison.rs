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
//!   success fully clears the failure record.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::RenderId;
use flui_objects::RenderPadding;
use flui_rendering::{
    constraints::{BoxConstraints, SliverGeometry},
    context::{BoxLayoutContext, PaintCx, SliverHitTestContext, SliverLayoutContext},
    error::RenderError,
    parent_data::{BoxParentData, ParentData, SliverParentData},
    protocol::{BoxProtocol, ProtocolGeometry},
    storage::IntrinsicDimension,
    testing::{FrameRun, Probe, RenderTester, box_node, sliver_node},
    traits::{HitTestOutcome, RenderBox, RenderObject, RenderSliver},
};
use flui_tree::{Leaf, Single};
use flui_types::{Size, geometry::px};

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
        _hit_child: &mut (
                 dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
        ) -> bool
                     + Send
                     + Sync
             ),
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
        child_query: &mut (dyn FnMut(usize, IntrinsicDimension, f32) -> f32 + Send + Sync),
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
        _hit_child: &mut (
                 dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
        ) -> bool
                     + Send
                     + Sync
             ),
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
