//! The widget harness runs the product frame transaction: every frame of
//! `lay_out` is the realm's `UiRealm::pump`, so what a realm provides on
//! screen — the root `MediaQuery`, the text-store commit gate, the owner
//! inbox, contained frame failures, the window's cursor — is what a harness
//! test sees.
//!
//! Each test fails against a harness that drives the pipeline itself instead
//! of pumping the realm.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::geometry::Size;
use flui_interaction::routing::FocusNode;
use flui_platform_api::text_store::{
    LockGrant, LockOutcome, LockTiming, TextStoreError, TextStoreRead, Utf16Offset,
};
use flui_rendering::hit_testing::CursorIcon;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_scheduler::SchedulerPhase;
use flui_testing::widgets::{ProbeSignals, SignalProbe, lay_out, tight};
use flui_testing::{A11yTree, Action, ActionRequest, TreeId};
use flui_view::prelude::*;
use flui_view::{RenderView, View};
use flui_widgets::{
    EditableText, GestureDetector, MediaQuery, MediaQueryData, MouseRegion, Semantics, SizedBox,
    Text, TextEditingController,
};

// ---------------------------------------------------------------------------
// The root MediaQuery
// ---------------------------------------------------------------------------

/// Records the `MediaQuery` each of its builds reads.
#[derive(Clone, StatelessView)]
struct MediaQueryReader {
    seen: Rc<RefCell<Vec<Option<MediaQueryData>>>>,
}

impl StatelessView for MediaQueryReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.seen.borrow_mut().push(MediaQuery::maybe_of(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

/// The realm wraps the root in the `MediaQuery` it publishes from its
/// window, so a widget under `lay_out` reads the surface it was laid out in.
///
/// Fails against a harness that mounts no realm: nothing above the root
/// publishes a `MediaQuery`, and the read is `None`.
#[test]
fn lay_out_publishes_the_realms_media_query() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let _laid = lay_out(
        MediaQueryReader {
            seen: Rc::clone(&seen),
        },
        tight(320.0, 240.0),
    );

    let data = seen
        .borrow()
        .last()
        .cloned()
        .expect("the reader built")
        .expect("the realm publishes a root MediaQuery");
    assert_eq!(data.size, Size::new(320.0, 240.0));
}

// ---------------------------------------------------------------------------
// Frames are text-store transactions
// ---------------------------------------------------------------------------

/// A focused field under the harness is the realm's IME client, and a
/// harness frame is its text-store transaction: a lock asked for inside the
/// frame is refused (sync) or queued (async), and the queued grant runs once
/// the pump returns, with the scheduler back in `Idle`.
///
/// Fails against a harness whose presentation owns no text input (the field
/// attaches nowhere, and there is no active store) or whose frame leaves the
/// commit gate open (the async grant runs inside the callback).
#[test]
fn harness_frames_are_text_store_transactions() {
    let controller = TextEditingController::with_text("abc");
    let focus_node = FocusNode::with_debug_label("transaction field");
    let mut laid = flui_testing::widgets::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    laid.enter_owner_scope(|| focus_node.request_focus());
    laid.tick();
    let store = laid
        .active_text_store()
        .expect("the focused field is the realm's active IME client");

    let sync_outcome = Rc::new(RefCell::new(None));
    let async_outcome = Rc::new(RefCell::new(None));
    let granted_in_phase = Rc::new(Cell::new(None));
    let scheduler = laid.scheduler().clone();
    let (sync_slot, async_slot, phase_slot) = (
        Rc::clone(&sync_outcome),
        Rc::clone(&async_outcome),
        Rc::clone(&granted_in_phase),
    );
    laid.local_post_frame_handle()
        .schedule_local(move |_timing| {
            *sync_slot.borrow_mut() = Some(store.request_lock(
                LockGrant::read(|_: &dyn TextStoreRead| {}),
                LockTiming::Sync,
            ));
            *async_slot.borrow_mut() = Some(store.request_lock(
                LockGrant::read(move |session: &dyn TextStoreRead| {
                    assert_eq!(session.document_len(), Utf16Offset::new(3));
                    phase_slot.set(Some(scheduler.phase()));
                }),
                LockTiming::Async,
            ));
        })
        .expect("the realm's post-frame lane is alive");
    laid.tick();

    assert_eq!(
        *sync_outcome.borrow(),
        Some(Err(TextStoreError::SyncLockUnavailable)),
        "a sync lock inside the frame is refused"
    );
    assert_eq!(
        *async_outcome.borrow(),
        Some(Ok(LockOutcome::Deferred)),
        "an async lock inside the frame is queued"
    );
    assert_eq!(
        granted_in_phase.get(),
        Some(SchedulerPhase::Idle),
        "the queued grant ran at the commit anchor, after the frame"
    );
}

// ---------------------------------------------------------------------------
// The owner inbox
// ---------------------------------------------------------------------------

fn labelled(detector: GestureDetector) -> Semantics {
    Semantics::new()
        .container(true)
        .child(detector.child(Text::new("Tap")))
}

fn tap_node(tree: &A11yTree) -> flui_testing::NodeId {
    tree.find_by_label("Tap")
        .unwrap_or_else(|error| panic!("one node labelled \"Tap\": {error}\n{}", tree.describe()))
        .id()
}

/// An assistive-technology action the platform adapter raises on its own
/// thread travels the realm's owner inbox, and the next harness pump applies
/// it (the pump's apply-commands step), which runs the tap and writes its
/// signal.
///
/// Fails against a harness with no owner inbox: no action listener is
/// registered on any window, so there is nothing to call.
#[test]
fn an_action_sent_off_thread_is_applied_by_the_next_harness_pump() {
    let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
        labelled(GestureDetector::new().on_tap(move |cx| count.update(cx, |n| *n += 1)))
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let node = tap_node(&laid.a11y_tree().expect("semantics is on"));
    let listener = laid
        .realm()
        .accessibility_action_listener()
        .expect("the realm registers an action listener on its window");

    std::thread::spawn(move || {
        listener(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: node,
            data: None,
        });
    })
    .join()
    .expect("the adapter thread ran");
    assert_eq!(probe.value(), Ok(0), "the request waits in the owner inbox");

    laid.tick();

    assert_eq!(probe.value(), Ok(1), "the pump applied the queued action");
}

// ---------------------------------------------------------------------------
// Contained frame failures
// ---------------------------------------------------------------------------

/// A 40 × 24 leaf whose paint panics while `armed` is set: a paint panic is
/// what the pipeline refuses the frame for.
#[derive(Debug)]
struct TripwireBox {
    armed: Arc<AtomicBool>,
}

impl flui_foundation::Diagnosticable for TripwireBox {}

impl RenderBox for TripwireBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(40.0, 24.0)
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {
        assert!(!self.armed.load(Ordering::SeqCst), "tripwire paint fired");
    }
}

#[derive(Clone)]
struct Tripwire {
    armed: Arc<AtomicBool>,
}

impl RenderView for Tripwire {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = TripwireBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> TripwireBox {
        TripwireBox {
            armed: Arc::clone(&self.armed),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut TripwireBox,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Tripwire {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

/// A mounted tripwire, armed and dirty for paint: the next frame fails.
fn armed_tripwire() -> (flui_testing::widgets::LaidOut, Arc<AtomicBool>) {
    let armed = Arc::new(AtomicBool::new(false));
    let laid = lay_out(
        Tripwire {
            armed: Arc::clone(&armed),
        },
        tight(40.0, 24.0),
    );
    armed.store(true, Ordering::SeqCst);
    laid.reassemble_render_tree();
    (laid, armed)
}

/// A frame failure the realm contains is raised by the harness after the
/// pump, with the realm's report: the phase and what failed.
///
/// Fails against a harness that returns from the pump as if nothing
/// happened: the realm reported the failure and kept going.
#[test]
fn a_frame_failure_under_the_harness_is_contained_reported_and_raised() {
    let (mut laid, _armed) = armed_tripwire();

    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the contained paint failure is raised");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed") && text.contains("paint"),
        "the raised panic carries the realm's report, got {text:?}"
    );
}

/// After a raised failure the harness keeps framing: once the cause is gone
/// the next pump paints, and the earlier failure is not raised again.
///
/// Fails against a harness that keeps the report and raises it on every
/// later pump.
#[test]
fn the_harness_makes_progress_after_a_contained_failure() {
    let (mut laid, armed) = armed_tripwire();
    let _ = catch_unwind(AssertUnwindSafe(|| laid.tick())).expect_err("the armed frame fails");
    let painted = laid.painted_frame_count();

    armed.store(false, Ordering::SeqCst);
    laid.tick();

    assert!(laid.did_paint_last_frame(), "the retried frame paints");
    assert_eq!(laid.painted_frame_count(), painted + 1);
}

/// When a post-frame callback panics in the same pump after the realm
/// contained a failure, the contained failure is what the harness raises.
///
/// Fails against a harness that lets the later unwind through (the
/// callback's text is raised) or loses the report when the pump unwinds.
#[test]
fn a_contained_frame_failure_stays_authoritative_over_a_later_post_frame_panic() {
    let (mut laid, _armed) = armed_tripwire();
    laid.local_post_frame_handle()
        .schedule_local(|_timing| panic!("post-frame callback panicked"))
        .expect("the realm's post-frame lane is alive");

    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the frame fails twice and raises once");

    let text = panic_text(&*raised);
    assert!(text.contains("frame pipeline failed"), "got {text:?}");
    assert!(
        !text.contains("post-frame callback panicked"),
        "got {text:?}"
    );
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

/// A hovered `MouseRegion`'s cursor reaches the realm's window, through the
/// presentation's mouse tracker.
///
/// Fails against a harness with no window: the cursor goes nowhere.
#[test]
fn mouse_region_cursor_reaches_the_window_through_the_realm() {
    let laid = lay_out(
        MouseRegion::new()
            .cursor(CursorIcon::Pointer)
            .child(SizedBox::new(40.0, 40.0)),
        tight(100.0, 100.0),
    );
    assert_eq!(laid.realm().window().cursor(), CursorIcon::Default);

    laid.dispatch_pointer_hover(20.0, 20.0);

    assert_eq!(laid.realm().window().cursor(), CursorIcon::Pointer);
}
