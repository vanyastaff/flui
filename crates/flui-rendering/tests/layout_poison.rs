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
    error::RenderError,
    protocol::{BoxProtocol, ProtocolGeometry},
    testing::{FrameRun, Probe, RenderTester, box_node},
    traits::{HitTestOutcome, RenderObject},
};
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
