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
use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::RenderPadding;
use flui_rendering::{
    constraints::BoxConstraints,
    error::RenderError,
    protocol::{BoxProtocol, ProtocolGeometry},
    storage::RenderNode,
    testing::{FrameRun, Probe, RenderTester, box_node},
    traits::{HitTestOutcome, RenderObject},
};

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
        Ok(Size::new(40.0, 40.0))
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
        0.0, 200.0, 0.0, 200.0,
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
        Size::new(40.0, 40.0),
        "the recovered leaf lays out at its real size",
    );
    assert_eq!(
        run.box_geometry(root),
        Size::new(50.0, 50.0),
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
    assert_eq!(run.box_geometry(leaf), Size::new(40.0, 40.0));
    run.pump_idle_frames(2);
}

// ============================================================================
// Retriable failures poison only at the budget
// ============================================================================

// ============================================================================
// Failure at the dirty root itself
// ============================================================================

// ============================================================================
// Intrinsic-measurement poison — fixtures
// ============================================================================

// ============================================================================
// (a) Permanently-failing intrinsic probe poisons after the bound
// ============================================================================

// ============================================================================
// (b) Un-poison: fix the object + re-invalidate → intrinsic succeeds again
// ============================================================================

// ============================================================================
// (c) Transient intrinsic failure does not poison
// ============================================================================

// ============================================================================
// Public probe path (PipelineOwner::box_intrinsic_dimension)
// ============================================================================

// ============================================================================
// (d) A constraints change grants a poisoned node one fresh attempt
// ============================================================================

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
    let s1 = Size::new(30.0, 40.0);
    let s2 = Size::new(50.0, 60.0);

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
    .with_constraints(BoxConstraints::new(0.0, 200.0, 0.0, 200.0))
    .run_frame();
    let parent = run.id("parent");
    let leaf = run.id("leaf");

    // Pass 1: the leaf lays out cleanly and commits S1.
    assert_eq!(node(&run, leaf).geometry_box(), Some(s1));
    assert_eq!(run.box_geometry(parent), Size::new(40.0, 50.0));
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
    assert_eq!(run.box_geometry(parent), Size::new(10.0, 10.0));

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
    assert_eq!(run.box_geometry(parent), Size::new(40.0, 50.0));
    assert!(node(&run, parent).geometry_degraded());
    assert!(run.owner().is_layout_poisoned(leaf));
}
