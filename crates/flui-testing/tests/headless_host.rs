//! [`HeadlessHost`]'s failure contract: a frame failure the UI runtime contains
//! is raised after the pump, the first failure of a pump stays authoritative,
//! and the UI runtime keeps producing frames once the cause is gone.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_testing::widgets::{LaidOut, lay_out, tight};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::{
    AppLifecycleState, BuildContext, CloseReason, IntoView, LifecycleContext, RenderView,
    StatefulView, View, ViewState,
};

/// A 40 × 24 leaf whose paint panics while `armed` is set. A paint panic is
/// what the pipeline refuses the frame for (`RenderError::Poisoned`); a
/// layout panic it contains below the root with stand-in geometry.
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

/// A UI runtime with a tripwire root, armed as asked, before its first frame.
fn tripwire_ui_runtime(armed: bool) -> (HeadlessHost, Arc<AtomicBool>) {
    let armed = Arc::new(AtomicBool::new(armed));
    let ui_runtime = HeadlessHost::new(HeadlessWindow::new(40, 24));
    ui_runtime
        .attach(&Tripwire {
            armed: Arc::clone(&armed),
        })
        .expect("a fresh ui_runtime has no root yet");
    (ui_runtime, armed)
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

/// A paint panic the UI runtime contains as a dropped frame is raised after the
/// pump, with the pipeline's report.
///
/// Fails against a driver that returns the pump's outcome as is: the UI runtime
/// contains the panic, the pump returns normally and nothing is raised.
#[test]
fn a_contained_frame_failure_is_raised_after_the_pump() {
    let (mut ui_runtime, _armed) = tripwire_ui_runtime(true);

    let raised = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(Duration::ZERO)))
        .expect_err("the contained paint panic must be raised");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed") && text.contains("paint"),
        "the raised panic carries the ui_runtime's pipeline report, got {text:?}"
    );
    assert_eq!(
        ui_runtime.sink().submits(),
        0,
        "a dropped frame submits nothing"
    );
}

/// Once the cause is gone, the next pump paints: the raise left the UI runtime
/// able to make progress, and the failure is not raised again.
///
/// Fails against a driver that keeps a raised report and raises it on every
/// later pump, or that leaves the UI runtime unable to frame after the raise.
#[test]
fn the_ui_runtime_makes_progress_after_a_raised_failure() {
    let (mut ui_runtime, armed) = tripwire_ui_runtime(true);
    let _ = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(Duration::ZERO)))
        .expect_err("the armed tripwire fails the first frame");

    armed.store(false, Ordering::SeqCst);
    let outcome = ui_runtime.pump(Duration::ZERO);

    assert!(outcome.presented(), "the retried frame presents");
    assert_eq!(ui_runtime.sink().submits(), 1);
    assert!(
        ui_runtime.sink().layer_tree().is_some(),
        "the sink keeps the scene the retried frame composited"
    );
}

/// Schedule a post-frame callback that panics, through the UI runtime's own
/// owner-local lane.
fn schedule_post_frame_panic(ui_runtime: &HeadlessHost) {
    ui_runtime
        .post_frame_handle()
        .schedule(|_timing| panic!("post-frame callback panicked"))
        .expect("the ui_runtime's post-frame lane is alive");
}

/// After a pump that unwound, the next pump runs: it returns, and the
/// post-frame lane the unwind went through still runs a callback. A frame is
/// requested first, so the frame latch the unwind left behind must let it
/// through.
fn assert_progress_after_unwind(ui_runtime: &mut HeadlessHost) -> flui_runtime::pump::FrameOutcome {
    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_callback = Arc::clone(&ran);
    ui_runtime
        .post_frame_handle()
        .schedule(move |_timing| ran_in_callback.store(true, Ordering::SeqCst))
        .expect("the post-frame lane survives the unwind");
    ui_runtime.request_frame();

    let outcome = ui_runtime.pump(Duration::ZERO);

    assert!(
        ran.load(Ordering::SeqCst),
        "the frame after the unwind runs its post-frame callbacks"
    );
    outcome
}

/// A panic that unwinds out of the pump after the UI runtime contained a failure
/// in the same pump does not replace it: the contained failure is raised.
/// Once the cause is gone, the UI runtime frames again.
///
/// Fails against a driver that resumes the later unwind (the post-frame
/// callback's text would be raised) or that loses the report when the pump
/// unwinds.
#[test]
fn a_contained_failure_stays_authoritative_over_a_later_unwind() {
    let (mut ui_runtime, armed) = tripwire_ui_runtime(true);
    schedule_post_frame_panic(&ui_runtime);

    let raised = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(Duration::ZERO)))
        .expect_err("the pump fails twice and raises once");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed"),
        "the first failure of the pump is raised, got {text:?}"
    );
    assert!(
        !text.contains("post-frame callback panicked"),
        "the later unwind must not replace the first failure, got {text:?}"
    );

    armed.store(false, Ordering::SeqCst);
    let outcome = assert_progress_after_unwind(&mut ui_runtime);
    assert!(
        outcome.presented(),
        "the tripwire, still waiting for paint, presents once disarmed"
    );
    assert_eq!(ui_runtime.sink().submits(), 1);
}

/// With nothing contained, a panic that unwinds out of the pump is raised as
/// itself, and the UI runtime frames again afterwards: the unwind left its frame
/// latch and post-frame lane usable.
///
/// Fails against a driver that swallows an unwind it has no report for.
#[test]
fn an_uncontained_unwind_is_raised_as_itself() {
    let (mut ui_runtime, _armed) = tripwire_ui_runtime(false);
    let _ = ui_runtime.pump(Duration::ZERO);
    schedule_post_frame_panic(&ui_runtime);
    ui_runtime.request_frame();

    let raised = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(Duration::ZERO)))
        .expect_err("the post-frame panic unwinds out of the pump");

    assert_eq!(panic_text(&*raised), "post-frame callback panicked");
    let _outcome = assert_progress_after_unwind(&mut ui_runtime);
}

/// Counts the Detached deliveries its presentation makes, and asks for the
/// close again from inside the first one, as an application closing its
/// window from a lifecycle observer does.
#[derive(Clone)]
struct ClosesAgainOnDetach {
    detached: Rc<Cell<usize>>,
    tree: Rc<RefCell<Weak<LaidOut>>>,
}

struct ClosesAgainOnDetachState {
    view: ClosesAgainOnDetach,
    observation: Option<flui_view::LifecycleSubscription>,
}

impl StatefulView for ClosesAgainOnDetach {
    type State = ClosesAgainOnDetachState;

    fn create_state(&self) -> Self::State {
        ClosesAgainOnDetachState {
            view: self.clone(),
            observation: None,
        }
    }
}

impl View for ClosesAgainOnDetach {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<ClosesAgainOnDetach> for ClosesAgainOnDetachState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let ClosesAgainOnDetach { detached, tree } = self.view.clone();
        let (_, observation) = cx
            .lifecycle_handle()
            .expect("a ui_runtime presentation has a lifecycle")
            .subscribe(move |state| {
                if state == AppLifecycleState::Detached {
                    detached.set(detached.get() + 1);
                    if let Some(tree) = tree.borrow().upgrade() {
                        tree.request_close(CloseReason::User);
                    }
                }
            })
            .expect("the presentation is open");
        self.observation = Some(observation);
    }

    fn build(&self, _view: &ClosesAgainOnDetach, _cx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// One close is delivered once: a close requested again from inside the
/// Detached observer, and the close the UI runtime repeats when it drops, deliver
/// nothing a second time.
///
/// Holds before the shared close delivery exists, because the lifecycle
/// source commits Detached only once; it stays as the guard that the
/// delivery's latch keeps it so.
#[test]
fn close_delivery_is_idempotent() {
    let detached = Rc::new(Cell::new(0));
    let slot = Rc::new(RefCell::new(Weak::new()));
    let tree = Rc::new(lay_out(
        ClosesAgainOnDetach {
            detached: Rc::clone(&detached),
            tree: Rc::clone(&slot),
        },
        tight(10.0, 10.0),
    ));
    *slot.borrow_mut() = Rc::downgrade(&tree);

    tree.request_close(CloseReason::User);
    tree.request_close(CloseReason::Program);
    drop(tree);

    assert_eq!(
        detached.get(),
        1,
        "the presentation is told it is detached exactly once"
    );
}

#[test]
fn secondary_window_preserves_its_text_store_backend() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(100, 100));
    let enabled = host.open_window(HeadlessWindow::new(100, 100).with_text_store_host());
    let absent = host.open_window(HeadlessWindow::new(100, 100));
    assert!(host.text_store_host_on(host.primary_window()).is_none());
    let node = flui_interaction::routing::FocusNode::new();
    host.attach_to(
        enabled,
        &flui_widgets::EditableText::new(flui_widgets::TextEditingController::new(), node.clone()),
    )
    .expect("secondary root");
    let _ = host.pump(Duration::ZERO);
    let _ = node.request_focus();
    let _ = host.pump(Duration::ZERO);
    assert!(
        host.text_store_host_on(enabled)
            .expect("backend")
            .focused_store()
            .is_some(),
        "the secondary field reaches the pull host"
    );
    assert!(host.text_store_host_on(absent).is_none());
}
