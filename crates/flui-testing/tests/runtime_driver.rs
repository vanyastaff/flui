//! The widget harness runs the product frame transaction: every frame of
//! `lay_out` is the UI runtime's `UiRuntime::pump`, so what a UI runtime provides on
//! screen — the root `MediaQuery`, the text-store commit gate, the owner
//! inbox, contained frame failures, the window's cursor — is what a harness
//! test sees.
//!
//! Each test fails against a harness that drives the pipeline itself instead
//! of pumping the UI runtime.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::geometry::Size;
use flui_interaction::routing::FocusNode;
use flui_painting::styling::Color;
use flui_platform_api::text_store::{
    LockGrant, LockOutcome, LockTiming, TextStoreError, TextStoreRead, Utf16Offset,
};
use flui_rendering::hit_testing::CursorIcon;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_scheduler::SchedulerPhase;
use flui_testing::widgets::{ProbeSignals, SignalProbe, lay_out, loose, tight};
use flui_testing::{A11yTree, Action, ActionRequest, TreeId};
use flui_view::prelude::*;
use flui_view::{RenderView, View};
use flui_widgets::{
    ColoredBox, EditableText, GestureDetector, MediaQuery, MediaQueryData, MouseRegion, Padding,
    Semantics, SizedBox, Text, TextEditingController,
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

/// The UI runtime wraps the root in the `MediaQuery` it publishes from its
/// window, so a widget under `lay_out` reads the surface it was laid out in.
///
/// Fails against a harness that mounts no ui_runtime: nothing above the root
/// publishes a `MediaQuery`, and the read is `None`.
#[test]
fn lay_out_publishes_the_ui_runtimes_media_query() {
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
        .expect("the ui_runtime publishes a root MediaQuery");
    assert_eq!(data.size, Size::new(320.0, 240.0));
}

// ---------------------------------------------------------------------------
// Frames are text-store transactions
// ---------------------------------------------------------------------------

/// A focused field under the harness is the UI runtime's IME client, and a
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
    let _ = laid.enter_owner_scope(|| focus_node.request_focus());
    laid.tick();
    let store = laid
        .active_text_store()
        .expect("the focused field is the ui_runtime's active IME client");

    let sync_outcome = Rc::new(RefCell::new(None));
    let async_outcome = Rc::new(RefCell::new(None));
    let granted_in_phase = Rc::new(Cell::new(None));
    let scheduler = laid.scheduler().clone();
    let (sync_slot, async_slot, phase_slot) = (
        Rc::clone(&sync_outcome),
        Rc::clone(&async_outcome),
        Rc::clone(&granted_in_phase),
    );
    laid.post_frame_handle()
        .schedule(move |_timing| {
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
        .expect("the ui_runtime's post-frame lane is alive");
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

/// A pump that unwinds out of a post-frame callback skips its commit anchor,
/// so the async grant that callback queued stays queued; the next pump runs
/// it at its own anchor, back in `Idle`.
///
/// Fails against a harness whose unwind drops the queued grant (it never
/// runs) or leaves the commit gate open (the grant runs inside the unwinding
/// frame, or the next pump runs it inside its frame).
#[test]
fn a_grant_queued_before_an_unwind_runs_at_the_next_pumps_anchor() {
    let controller = TextEditingController::with_text("abc");
    let focus_node = FocusNode::with_debug_label("unwind field");
    let mut laid = flui_testing::widgets::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    let _ = laid.enter_owner_scope(|| focus_node.request_focus());
    laid.tick();
    let store = laid
        .active_text_store()
        .expect("the focused field is the ui_runtime's active IME client");

    let outcome = Rc::new(RefCell::new(None));
    let granted_in_phase = Rc::new(Cell::new(None));
    let scheduler = laid.scheduler().clone();
    let (outcome_slot, phase_slot) = (Rc::clone(&outcome), Rc::clone(&granted_in_phase));
    laid.post_frame_handle()
        .schedule(move |_timing| {
            *outcome_slot.borrow_mut() = Some(store.request_lock(
                LockGrant::read(move |_: &dyn TextStoreRead| {
                    phase_slot.set(Some(scheduler.phase()));
                }),
                LockTiming::Async,
            ));
            panic!("post-frame callback panicked");
        })
        .expect("the ui_runtime's post-frame lane is alive");

    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the post-frame panic unwinds out of the pump");
    assert_eq!(panic_text(&*raised), "post-frame callback panicked");
    assert_eq!(
        *outcome.borrow(),
        Some(Ok(LockOutcome::Deferred)),
        "the lock was asked for inside the frame"
    );
    assert_eq!(
        granted_in_phase.get(),
        None,
        "the unwound pump skipped its commit anchor"
    );

    laid.tick();

    assert_eq!(
        granted_in_phase.get(),
        Some(SchedulerPhase::Idle),
        "the next pump ran the queued grant at its anchor"
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
/// thread travels the UI runtime's owner inbox, and the next harness pump applies
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
        .accessibility_action_listener()
        .expect("the ui_runtime registers an action listener on its window");

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

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 24.0))
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

/// A frame failure the UI runtime contains is raised by the harness after the
/// pump, with the UI runtime's report: the phase and what failed.
///
/// Fails against a harness that returns from the pump as if nothing
/// happened: the UI runtime reported the failure and kept going.
#[test]
fn a_frame_failure_under_the_harness_is_contained_reported_and_raised() {
    let (mut laid, _armed) = armed_tripwire();

    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the contained paint failure is raised");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed") && text.contains("paint"),
        "the raised panic carries the ui_runtime's report, got {text:?}"
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

/// When a post-frame callback panics in the same pump after the UI runtime
/// contained a failure, the contained failure is what the harness raises.
///
/// Fails against a harness that lets the later unwind through (the
/// callback's text is raised) or loses the report when the pump unwinds.
#[test]
fn a_contained_frame_failure_stays_authoritative_over_a_later_post_frame_panic() {
    let (mut laid, armed) = armed_tripwire();
    let invocations = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&invocations);
    laid.scheduler()
        .add_post_frame_callback(Box::new(move |_timing| {
            called.fetch_add(1, Ordering::SeqCst);
            panic!("post-frame callback panicked");
        }));

    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the frame fails twice and raises once");

    let text = panic_text(&*raised);
    assert!(text.contains("frame pipeline failed"), "got {text:?}");
    assert!(
        !text.contains("post-frame callback panicked"),
        "got {text:?}"
    );
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        1,
        "the unscoped host callback competes in this same pump"
    );
    let painted = laid.painted_frame_count();
    armed.store(false, Ordering::SeqCst);
    laid.tick();
    assert!(
        laid.did_paint_last_frame(),
        "repair presents after both failures"
    );
    assert_eq!(laid.painted_frame_count(), painted + 1);
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
}

/// A post-frame panic that unwinds out of a harness pump leaves the harness
/// able to paint: the next frame with a dirty render tree paints it.
///
/// Fails against a harness whose unwind leaves the UI runtime's frame latch or
/// pipeline stuck: the later frame paints nothing.
#[test]
fn the_harness_paints_after_a_post_frame_unwind() {
    let (mut laid, armed) = armed_tripwire();
    armed.store(false, Ordering::SeqCst);
    laid.post_frame_handle()
        .schedule(|_timing| panic!("post-frame callback panicked"))
        .expect("the ui_runtime's post-frame lane is alive");
    let raised = catch_unwind(AssertUnwindSafe(|| laid.tick()))
        .expect_err("the post-frame panic unwinds out of the pump");
    assert_eq!(panic_text(&*raised), "post-frame callback panicked");
    let painted = laid.painted_frame_count();

    laid.reassemble_render_tree();
    laid.tick();

    assert!(
        laid.did_paint_last_frame(),
        "the frame after the unwind paints"
    );
    assert_eq!(laid.painted_frame_count(), painted + 1);
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

/// A hovered `MouseRegion`'s cursor reaches the UI runtime's window, through the
/// presentation's mouse tracker.
///
/// Fails against a harness with no window: the cursor goes nowhere.
#[test]
fn mouse_region_cursor_reaches_the_window_through_the_ui_runtime() {
    let laid = lay_out(
        MouseRegion::new()
            .cursor(CursorIcon::Pointer)
            .child(SizedBox::new(40.0, 40.0)),
        tight(100.0, 100.0),
    );
    assert_eq!(laid.cursor(), CursorIcon::Default);

    laid.dispatch_pointer_hover(20.0, 20.0);

    assert_eq!(laid.cursor(), CursorIcon::Pointer);
}

#[derive(Clone, StatefulView)]
struct ReplacingRenderRoot {
    mode: Rc<Cell<u8>>,
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
}

struct ReplacingRenderRootState {
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
}

impl StatefulView for ReplacingRenderRoot {
    type State = ReplacingRenderRootState;

    fn create_state(&self) -> Self::State {
        ReplacingRenderRootState {
            rebuild: Rc::clone(&self.rebuild),
        }
    }
}

impl ViewState<ReplacingRenderRoot> for ReplacingRenderRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &ReplacingRenderRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        assert_ne!(view.mode.get(), 1, "induced root build failure");
        if view.mode.get() == 2 {
            Padding::all(5.0).child(SizedBox::new(40.0, 30.0)).boxed()
        } else {
            SizedBox::new(20.0, 10.0).boxed()
        }
    }
}

pub fn logical_render_root_tracks_replacement_and_build_recovery() {
    let mode = Rc::new(Cell::new(0));
    let rebuild = Rc::new(RefCell::new(None));
    let mut laid = lay_out(
        ReplacingRenderRoot {
            mode: Rc::clone(&mode),
            rebuild: Rc::clone(&rebuild),
        },
        loose(100.0),
    );
    let initial = laid.root();
    assert_eq!(laid.size(initial), Size::new(20.0, 10.0));
    for next in [2, 1, 0] {
        mode.set(next);
        rebuild
            .borrow()
            .as_ref()
            .expect("root registered its rebuild handle")
            .schedule(RebuildReason::StateChange);
        laid.tick();
        let root = laid.root();
        assert_eq!(root, laid.current_root());
        match next {
            2 => {
                assert_eq!(root, laid.find_by_render_type("RenderPadding"));
                assert_eq!(laid.size(root), Size::new(50.0, 40.0));
                assert_eq!(laid.try_size(initial), None);
            }
            1 => {
                assert_eq!(root, laid.find_by_render_type("RenderErrorBox"));
                assert!(laid.try_size(root).is_some());
            }
            _ => {
                assert_eq!(laid.find_all_by_render_type("RenderErrorBox"), []);
                assert_eq!(root, laid.find_by_render_type("RenderConstrainedBox"));
                assert_eq!(laid.size(root), Size::new(20.0, 10.0));
            }
        }
    }
}

pub fn a_zero_capacity_performance_window_retains_no_frame_samples() {
    let mut disabled = flui_runtime::performance_stats::PerformanceStats::new(0);
    let mut enabled = flui_runtime::performance_stats::PerformanceStats::new(1);
    disabled.record_frame();
    enabled.record_frame();
    // The API samples the platform clock, rather than accepting a test clock.
    // A nonzero interval distinguishes disabled retention from a zero-duration frame.
    std::thread::sleep(std::time::Duration::from_millis(2));
    disabled.record_frame();
    enabled.record_frame();
    assert_eq!(disabled.avg_frame_time_ms(), 0.0);
    assert_eq!(disabled.fps(), 0.0);
    assert!(enabled.avg_frame_time_ms() > 0.0);
    assert!(enabled.fps() > 0.0);
    disabled.record_frame();
    assert_eq!(disabled.avg_frame_time_ms(), 0.0);
}

/// A reused probe follows the new owning UI runtime rather than its released signal.
pub fn a_signal_probe_reads_and_writes_its_current_mount_after_remount() {
    let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
        GestureDetector::new()
            .on_tap(move |cx| count.update(cx, |value| *value += 1))
            .child(ColoredBox::new(Color::WHITE))
    });
    let mut first = lay_out(probe.view(), tight(100.0, 100.0));
    first.dispatch_pointer_down(50.0, 50.0);
    first.dispatch_pointer_up(50.0, 50.0);
    first.tick();
    assert_eq!(probe.value(), Ok(1));
    drop(first);
    let mut current = lay_out(probe.view(), tight(100.0, 100.0));
    assert_eq!(probe.value(), Ok(0), "the new mount owns a new live signal");
    current.dispatch_pointer_down(50.0, 50.0);
    current.dispatch_pointer_up(50.0, 50.0);
    current.tick();
    assert_eq!(probe.value(), Ok(1));
}
