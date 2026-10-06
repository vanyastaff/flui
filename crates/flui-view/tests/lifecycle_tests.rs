//! Integration tests for Element lifecycle.
//!
//! Tests the lifecycle states and transitions: Initial → Active ⇄ Inactive →
//! Defunct

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::__runtime::CloseGuardSource;
use flui_view::CloseReason;
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, RenderView, StatefulView, View, ViewExt,
    ViewState,
};

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

// The guard is handed to IO completions and its change future awaited off the
// owner thread, so both cross threads whatever state they come to hold.
static_assertions::assert_impl_all!(flui_view::CloseGuard: Send, Sync, Clone);
static_assertions::assert_impl_all!(flui_view::CloseChanged: Send, std::future::Future<Output = ()>);

/// A source for a desktop presentation, whose platform lets the application
/// refuse every reason, counting the closes it queues on the owner.
fn desktop_close_guard() -> (CloseGuardSource, Arc<AtomicUsize>) {
    let queued = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&queued);
    let source = CloseGuardSource::new(
        &[
            CloseReason::User,
            CloseReason::Program,
            CloseReason::SessionEnd,
        ],
        move || {
            count.fetch_add(1, Ordering::SeqCst);
        },
    );
    (source, queued)
}

/// A close refused by holds is carried out once, when the last hold goes,
/// without asking again; a repeated request for the same reason adds nothing.
#[test]
#[ignore = "contract: a released hold carries out the recorded close once"]
fn releasing_the_last_hold_queues_one_close() {
    let (source, queued) = desktop_close_guard();
    let guard = source.guard();
    let saving = guard.hold();
    let second = guard.hold();

    let refused = source.refuses(CloseReason::User);
    let refused_again = source.refuses(CloseReason::User);
    drop(saving);
    assert_eq!(
        queued.load(Ordering::SeqCst),
        0,
        "a hold is still held, so nothing is queued"
    );
    drop(second);
    assert_eq!(
        queued.load(Ordering::SeqCst),
        1,
        "releasing the last hold queues exactly one close operation"
    );
    assert!(
        refused && refused_again,
        "a hold refuses the user's close, and a repeated request too"
    );
}

/// A cancelled log-off withdraws its own recorded close and nothing else:
/// the user's close, recorded earlier, still waits for a decision.
#[test]
#[ignore = "contract: a withdrawn session end leaves the user's close recorded"]
fn a_cancelled_logoff_withdraws_only_the_session_entry() {
    let (source, queued) = desktop_close_guard();
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
    assert_eq!(queued.load(Ordering::SeqCst), 0, "nothing was carried out");
}
