//! Integration tests for Element lifecycle.
//!
//! Tests the lifecycle states and transitions: Initial → Active ⇄ Inactive →
//! Defunct

use std::future::Future;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::task::{Context, Waker};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::__runtime::{CloseGuardSource, LifecycleEvent, OwnerNotify, Undeliverable};
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, RenderView, StatefulView, View, ViewExt,
    ViewState,
};
use flui_view::{CloseChanged, CloseGuard, CloseHold, CloseReason, PendingClose, StaleClose};

// ============================================================================
// Test Views with lifecycle tracking
// ============================================================================

/// A true tree leaf — renders directly, with no further `build()` in the
/// chain. `TrackingView` above deliberately can't serve this role: its own
/// `build()` returns `self.clone().boxed()`, an intentionally self-
/// referential fixture (never driven through `build_scope` by the tests
/// that use it) that would recurse forever if it were.
#[derive(Clone)]
struct LeafView;

impl RenderView for LeafView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct LifecycleTrackingView {
    disposed: Arc<AtomicBool>,
    activated: Arc<AtomicUsize>,
    deactivated: Arc<AtomicUsize>,
}

struct LifecycleTrackingState {
    disposed: Arc<AtomicBool>,
    activated: Arc<AtomicUsize>,
    deactivated: Arc<AtomicUsize>,
}

impl StatefulView for LifecycleTrackingView {
    type State = LifecycleTrackingState;

    fn create_state(&self) -> Self::State {
        LifecycleTrackingState {
            disposed: self.disposed.clone(),
            activated: self.activated.clone(),
            deactivated: self.deactivated.clone(),
        }
    }
}

impl ViewState<LifecycleTrackingView> for LifecycleTrackingState {
    fn build(&self, _view: &LifecycleTrackingView, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafView.boxed()
    }

    fn activate(&mut self) {
        self.activated.fetch_add(1, Ordering::SeqCst);
    }

    fn deactivate(&mut self) {
        self.deactivated.fetch_add(1, Ordering::SeqCst);
    }

    fn dispose(&mut self) {
        self.disposed.store(true, Ordering::SeqCst);
    }
}

impl View for LifecycleTrackingView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

// ============================================================================
// Lifecycle State Tests
// ============================================================================

// ============================================================================
// Element Lifecycle Tests
// ============================================================================

// ============================================================================
// StatefulElement Lifecycle Callbacks
// ============================================================================

pub(crate) fn test_stateful_element_multiple_deactivate_activate_cycles() {
    let activated = Arc::new(AtomicUsize::new(0));
    let deactivated = Arc::new(AtomicUsize::new(0));
    let view = LifecycleTrackingView {
        disposed: Arc::new(AtomicBool::new(false)),
        activated: activated.clone(),
        deactivated: deactivated.clone(),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());
    // Drive the first build so `init_state` runs before the first
    // `deactivate`/`activate`: `StatefulBehavior::on_activate` is gated on a
    // completed `init_state`, so a mounted but never-built element never runs
    // its `activate` callback.
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    // First cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    // Second cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    // Third cycle
    tree.deactivate(root_id, &mut owner.element_owner_mut());
    tree.activate(root_id, &mut owner.element_owner_mut());

    assert_eq!(activated.load(Ordering::SeqCst), 3);
    assert_eq!(deactivated.load(Ordering::SeqCst), 3);
}

// ============================================================================
// ElementTree Lifecycle Tests
// ============================================================================

// ============================================================================
// Depth Tests
// ============================================================================

// ============================================================================
// Slot Tests
// ============================================================================

// ============================================================================
// Parent Tracking Tests
// ============================================================================

// ============================================================================
// Lifecycle Memory Layout Tests
// ============================================================================

// ============================================================================
// Thread Safety Tests
// ============================================================================

// ============================================================================
// Characterization Tests for `ElementOwner` threading
// ============================================================================
//
// These tests pin the observable mount/unmount/update invariants that the
// signature-threading refactor must preserve. They were authored BEFORE the
// `&mut ElementOwner` parameter was threaded through `ElementBase` so that any
// regression in the lifecycle FSM during the rewire is caught immediately.
//
// Invariants asserted:
// - After `mount(parent, slot)` the element transitions Initial → Active.
// - After `unmount()` the element transitions to Defunct.
// - A child mounted via `ElementTree::insert` is stored at the recorded
//   parent + slot + (parent_depth + 1).

// ============================================================================
// Close guard
// ============================================================================

// A hold is taken and released on the owner thread, where the work's
// completion is observed; none of the guard's values leaves it.
static_assertions::assert_not_impl_any!(CloseGuard: Send, Sync);
static_assertions::assert_not_impl_any!(CloseHold: Send, Sync);
static_assertions::assert_not_impl_any!(CloseChanged: Send, Sync);
// The owner-notify channel is what an IO thread signals through.
static_assertions::assert_impl_all!(OwnerNotify: Send, Sync, Clone);

const EVERY_REASON: [CloseReason; 3] = [
    CloseReason::User,
    CloseReason::Program,
    CloseReason::SessionEnd,
];

/// The events a guard signalled its owner with.
type Signals = Arc<Mutex<Vec<LifecycleEvent>>>;

/// A source for a desktop presentation, whose platform lets the application
/// refuse every reason, recording the events it signals; `delivered` says
/// whether the owner can still be reached.
fn desktop_close_guard(delivered: bool) -> (CloseGuardSource, Signals) {
    let signals = Signals::default();
    let log = Arc::clone(&signals);
    let notify = OwnerNotify::new(move |event| {
        log.lock().expect("signal log").push(event);
        if delivered {
            Ok(())
        } else {
            Err(Undeliverable)
        }
    });
    (CloseGuardSource::new(&EVERY_REASON, notify), signals)
}

fn due_signals(signals: &Signals) -> usize {
    signals
        .lock()
        .expect("signal log")
        .iter()
        .filter(|event| **event == LifecycleEvent::CloseDue)
        .count()
}

/// A close refused by holds becomes due once, when the last hold goes: the
/// owner is signalled once and its settle carries it out once, without the
/// handler being asked again.
#[test]
#[ignore = "contract: a released hold makes the recorded close due once"]
fn releasing_the_last_hold_queues_one_close() {
    let (source, signals) = desktop_close_guard(true);
    let guard = source.guard();
    let saving = guard.hold();
    let second = guard.hold();

    let refused = source.refuses(CloseReason::User);
    drop(saving);
    let due_while_held = due_signals(&signals);
    drop(second);

    assert_eq!(
        due_signals(&signals),
        1,
        "releasing the last hold signals the owner exactly once"
    );
    assert_eq!(due_while_held, 0, "a hold is still held, so nothing is due");
    assert!(refused, "a hold refuses the user's close");
    assert_eq!(
        source.settle().close,
        Some(CloseReason::User),
        "the close is due"
    );
    assert_eq!(source.settle().close, None, "and carried out once");
}

/// A repeated request for the same reason records nothing more: resolving
/// the one record leaves none.
#[test]
#[ignore = "contract: a repeated request records one close"]
fn a_repeated_request_records_one_close() {
    let (source, _signals) = desktop_close_guard(true);
    let guard = source.guard();
    let _saving = guard.hold();

    let refusals = [
        source.refuses(CloseReason::User),
        source.refuses(CloseReason::User),
    ];
    let first = guard.pending().map(PendingClose::stay_open);

    assert_eq!(first, Some(Ok(())), "one close is recorded and resolved");
    assert!(
        guard.pending().is_none(),
        "the repeated request recorded no second close"
    );
    assert_eq!(refusals, [true, true], "both requests are refused");
}

/// What each kind of hold refuses, reason by reason: a hold lets the session
/// end through, a decision refuses every reason the platform lets the
/// application refuse.
#[test]
#[ignore = "contract: a hold refuses user and program closes, a decision every vetoable one"]
fn a_hold_and_a_decision_refuse_by_reason() {
    fn refusals(take: fn(&CloseGuard) -> CloseHold) -> Vec<(CloseReason, bool)> {
        EVERY_REASON
            .iter()
            .map(|reason| {
                let (source, _signals) = desktop_close_guard(true);
                let _hold = take(&source.guard());
                (*reason, source.refuses(*reason))
            })
            .collect()
    }

    assert_eq!(
        refusals(CloseGuard::hold),
        [
            (CloseReason::User, true),
            (CloseReason::Program, true),
            (CloseReason::SessionEnd, false),
        ],
        "a hold covers work that finishes by itself"
    );
    assert_eq!(
        refusals(CloseGuard::require_decision),
        [
            (CloseReason::User, true),
            (CloseReason::Program, true),
            (CloseReason::SessionEnd, true),
        ],
        "a decision refuses every reason the platform lets the application refuse"
    );
}

/// A close recorded under a decision is carried out once the decision hold
/// is released, as a retry that succeeded releases it; resolved with
/// `stay_open` it is not, and the resolved record is stale afterwards.
#[test]
#[ignore = "contract: the recorded close runs when the last hold goes, unless kept open"]
fn a_recorded_close_runs_when_the_last_hold_goes_unless_kept_open() {
    let (retried, _signals) = desktop_close_guard(true);
    let decision = retried.guard().require_decision();
    let _ = retried.refuses(CloseReason::User);
    drop(decision);
    assert_eq!(
        retried.settle().close,
        Some(CloseReason::User),
        "a successful retry releases the decision and the close runs"
    );

    let (kept, _signals) = desktop_close_guard(true);
    let guard = kept.guard();
    let decision = guard.require_decision();
    let _ = kept.refuses(CloseReason::User);
    let stale = guard.pending();
    let resolved = guard.pending().map(PendingClose::stay_open);
    drop(decision);
    assert_eq!(resolved, Some(Ok(())), "the user chose to stay");
    assert_eq!(
        kept.settle().close,
        None,
        "a close kept open is not carried out"
    );
    assert_eq!(
        stale.map(PendingClose::discard_and_close),
        Some(Err(StaleClose)),
        "a resolved record is stale"
    );
}

/// A cancelled log-off withdraws its own recorded close and nothing else:
/// the user's close, recorded earlier, still waits for a decision.
#[test]
#[ignore = "contract: a withdrawn session end leaves the user's close recorded"]
fn a_cancelled_logoff_withdraws_only_the_session_entry() {
    let (source, signals) = desktop_close_guard(true);
    let guard = source.guard();
    let _unsaved = guard.require_decision();

    let user_refused = source.refuses(CloseReason::User);
    let session_refused = source.refuses(CloseReason::SessionEnd);
    source.withdraw(CloseReason::SessionEnd);

    assert_eq!(
        guard.pending().map(|pending| pending.reason()),
        Some(CloseReason::User),
        "the user's close stays recorded after the session end is withdrawn"
    );
    assert!(
        user_refused && session_refused,
        "a decision hold refuses every reason"
    );
    assert_eq!(due_signals(&signals), 0, "nothing became due");
}

/// A due close whose signal could not reach the owner is not lost: the
/// owner's next settle still finds it.
#[test]
#[ignore = "contract: an undelivered signal keeps the due close"]
fn an_undelivered_signal_keeps_the_due_close() {
    let (source, signals) = desktop_close_guard(false);
    let saving = source.guard().hold();
    let _ = source.refuses(CloseReason::User);
    drop(saving);

    assert_eq!(
        source.settle().close,
        Some(CloseReason::User),
        "the due close survives a failed signal"
    );
    assert_eq!(due_signals(&signals), 1, "the owner was signalled once");
}

/// A waker that counts how often it is woken, so a test can tell a wake on
/// the owner's turn from one inside the change.
#[derive(Default)]
struct CountingWaker(AtomicUsize);

impl std::task::Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl CountingWaker {
    fn wakes(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// The owner wakes what a settle returned, as its containment boundary does.
fn wake_on_the_owner_turn(wakers: Vec<Waker>) {
    for waker in wakers {
        waker.wake();
    }
}

/// A future taken before a close is recorded wakes neither in `refuses` nor
/// in `settle`, and once when the owner wakes what `settle` returned.
fn a_change_recorded_under_a_hold_wakes_on_the_owner_turn() {
    let (source, signals) = desktop_close_guard(true);
    let guard = source.guard();
    let _saving = guard.hold();
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut cx = Context::from_waker(&waker);
    let mut changed = std::pin::pin!(guard.changed());
    let pending_before = changed.as_mut().poll(&mut cx).is_pending();

    let _ = source.refuses(CloseReason::User);
    let after_refuses = counter.wakes();
    let settled = source.settle();
    let after_settle = counter.wakes();
    wake_on_the_owner_turn(settled.wake);

    assert_eq!(
        counter.wakes(),
        1,
        "the owner wakes the waiter once, from what settle returned"
    );
    assert_eq!(after_refuses, 0, "nothing wakes inside the change");
    assert_eq!(after_settle, 0, "settle wakes nothing itself");
    assert!(pending_before && changed.as_mut().poll(&mut cx).is_ready());
    assert!(
        signals
            .lock()
            .expect("signal log")
            .contains(&LifecycleEvent::CloseChanged),
        "the change signalled the owner"
    );
}

/// A future taken after a close was recorded, across the release of the
/// last hold: the release wakes nothing, settle returns the due close with
/// the waker, and the waker wakes once the owner wakes it.
fn a_change_taken_across_the_release_wakes_on_the_owner_turn() {
    let (source, _signals) = desktop_close_guard(true);
    let guard = source.guard();
    let saving = guard.hold();
    let _ = source.refuses(CloseReason::User);
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut cx = Context::from_waker(&waker);
    let mut changed = std::pin::pin!(guard.changed());
    let _ = changed.as_mut().poll(&mut cx);

    drop(saving);
    let after_release = counter.wakes();
    let settled = source.settle();
    let due = settled.close;
    let after_settle = counter.wakes();
    wake_on_the_owner_turn(settled.wake);

    assert_eq!(
        counter.wakes(),
        1,
        "the owner wakes the waiter once, from what settle returned"
    );
    assert_eq!(due, Some(CloseReason::User), "the close is due");
    assert_eq!(after_release, 0, "releasing the hold wakes nothing");
    assert_eq!(after_settle, 0, "settle wakes nothing itself");
}

/// A change wakes `changed()` on the owner's turn, never inside the change
/// and never inside `settle`.
#[test]
#[ignore = "contract: a close change wakes waiters on the owner's turn"]
fn a_close_change_wakes_on_the_owner_turn() {
    crate::run_table(
        "a_close_change_wakes_on_the_owner_turn",
        &[
            (
                "a_change_recorded_under_a_hold_wakes_on_the_owner_turn",
                a_change_recorded_under_a_hold_wakes_on_the_owner_turn as fn(),
            ),
            (
                "a_change_taken_across_the_release_wakes_on_the_owner_turn",
                a_change_taken_across_the_release_wakes_on_the_owner_turn as fn(),
            ),
        ],
    );
}

/// Discarding a recorded close makes it due: the owner is signalled, its
/// settle returns the reason, and the record is gone.
#[test]
#[ignore = "contract: discarding a recorded close makes it due"]
fn discarding_a_recorded_close_makes_it_due() {
    let (source, signals) = desktop_close_guard(true);
    let guard = source.guard();
    let _unsaved = guard.require_decision();
    let _ = source.refuses(CloseReason::User);

    let discarded = guard.pending().map(PendingClose::discard_and_close);

    assert_eq!(discarded, Some(Ok(())), "the recorded close is discarded");
    assert_eq!(due_signals(&signals), 1, "the owner is signalled once");
    assert_eq!(
        source.settle().close,
        Some(CloseReason::User),
        "settle returns the discarded close for the owner to carry out"
    );
    assert!(guard.pending().is_none(), "the record is gone");
}
