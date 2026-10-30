//! [`HeadlessHost`]'s failure contract: a frame failure the UI runtime contains
//! is raised after the pump, the first failure of a pump stays authoritative,
//! and the UI runtime keeps producing frames once the cause is gone.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Once the cause is gone, the pump at the retry deadline paints: the raise left the UI runtime
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
    let outcome = ui_runtime
        .pump_next_frame()
        .expect("the dropped frame armed its retry");

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

/// An incomplete presentation retains its completion callbacks. Repair makes
/// them eligible; their later failure belongs to that repaired pump, and does
/// not replay the original pipeline failure or prevent subsequent completion.
#[test]
fn an_incomplete_frame_retains_callbacks_until_repair_and_recovers_after_their_unwind() {
    let (mut ui_runtime, armed) = tripwire_ui_runtime(true);
    let invocations = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&invocations);
    ui_runtime
        .post_frame_handle()
        .schedule(move |_timing| {
            called.fetch_add(1, Ordering::SeqCst);
            panic!("post-frame callback panicked");
        })
        .expect("the presentation's completion lane is alive");

    let raised = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump(Duration::ZERO)))
        .expect_err("the incomplete pump raises its pipeline failure");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed"),
        "the incomplete pump's failure is raised, got {text:?}"
    );
    assert!(
        !text.contains("post-frame callback panicked"),
        "an incomplete presentation does not run completion callbacks, got {text:?}"
    );
    assert_eq!(invocations.load(Ordering::SeqCst), 0);
    assert_eq!(ui_runtime.sink().submits(), 0);

    armed.store(false, Ordering::SeqCst);
    let raised = catch_unwind(AssertUnwindSafe(|| ui_runtime.pump_next_frame()))
        .expect_err("repair delivers the retained callback's own failure");
    assert_eq!(panic_text(&*raised), "post-frame callback panicked");
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
    assert_eq!(ui_runtime.sink().submits(), 1);
    let _ = assert_progress_after_unwind(&mut ui_runtime);
    assert_eq!(invocations.load(Ordering::SeqCst), 1);
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
    let node = flui_interaction::focus::FocusNode::new();
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

/// A headless test's gestures follow the preset it names, never the machine
/// that runs it: a 9 px drift exceeds Android's 8 dp touch slop and stays
/// within iOS's 10 pt and the platform-neutral 18 px.
#[test]
fn gestures_follow_the_named_preset_on_every_host() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        GestureSettings,
        events::PointerKind,
        testing::input::{pointer_down, pointer_move, pointer_up},
    };
    use flui_platform::{PlatformInput, TargetPlatform};

    for (preset, taps) in [
        (Some(TargetPlatform::Android), 0),
        (Some(TargetPlatform::iOS), 1),
        (None, 1),
    ] {
        let mut window = HeadlessWindow::new(200, 200);
        if let Some(platform) = preset {
            window =
                window.with_gesture_preset(GestureSettings::preset(platform, PointerKind::Touch));
        }
        let mut host = HeadlessHost::new(window);
        let count = Rc::new(Cell::new(0));
        let output = Rc::clone(&count);
        host.attach(
            &flui_widgets::GestureDetector::new()
                .on_tap(move |_| output.set(output.get() + 1))
                .child(flui_widgets::ColoredBox::new(
                    flui_painting::styling::Color::RED,
                )),
        )
        .expect("attach");
        let _ = host.pump(Duration::from_millis(16));
        let start = Offset::new(40.0, 40.0);
        let drifted = Offset::new(49.0, 40.0);
        for event in [
            pointer_down(start, PointerKind::Touch).expect("down"),
            pointer_move(drifted, PointerKind::Touch).expect("move"),
            pointer_up(drifted, PointerKind::Touch).expect("up"),
        ] {
            host.dispatch(PlatformInput::Pointer(event));
        }
        assert_eq!(count.get(), taps, "preset {preset:?}");
    }
}

/// One presentation, two kinds of pointer: each contact is admitted and
/// hit-tested with its own kind's preset (ADR-0212 §4), never one profile for
/// the whole window. On Windows a finger gets the coarse row (44 px minimum
/// target, 18 px slop) and a mouse the fine one (24 px, 1 px).
#[test]
fn a_mouse_and_a_touch_get_their_own_preset_and_min_target() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        GestureProfiles, PointerId,
        events::{
            PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
        },
    };
    use flui_platform::{PlatformInput, TargetPlatform};

    let window = HeadlessWindow::new(200, 200)
        .with_gesture_preset(GestureProfiles::preset(TargetPlatform::Windows));
    let mut host = HeadlessHost::new(window);
    let count = Rc::new(Cell::new(0));
    let output = Rc::clone(&count);
    host.attach(&flui_widgets::Stack::new(flui_widgets::row![
        flui_widgets::SizedBox::new(200.0, 200.0),
        flui_widgets::Positioned::new(
            flui_widgets::HitSlop::new().child(
                flui_widgets::GestureDetector::new()
                    .on_tap(move |_| output.set(output.get() + 1))
                    .child(flui_widgets::SizedBox::new(20.0, 20.0).child(
                        flui_widgets::ColoredBox::new(flui_painting::styling::Color::RED),
                    )),
            )
        )
        .left(90.0)
        .top(90.0),
    ]))
    .expect("attach");
    let _ = host.pump(Duration::from_millis(16));

    let mut next_id = 0_u64;
    let mut contact = |kind: PointerKind, down: Offset<f64>, up: Offset<f64>| -> usize {
        next_id += 1;
        let id = PointerId::try_from(next_id).expect("nonzero pointer id");
        let before = count.get();
        for event in [
            make_down_event_for_id(id, down, kind).expect("down"),
            make_move_event_for_id(id, up, kind).expect("move"),
            make_up_event_for_id(id, up, kind).expect("up"),
        ] {
            host.dispatch(PlatformInput::Pointer(event));
        }
        let _ = host.pump(Duration::from_millis(16));
        count.get() - before
    };

    // 10 px left of the 20 px target: inside a 44 px minimum target (12 px a
    // side), outside a 24 px one (2 px a side).
    let beside = Offset::new(80.0, 100.0);
    assert_eq!(
        contact(PointerKind::Touch, beside, beside),
        1,
        "touch min target"
    );
    assert_eq!(
        contact(PointerKind::Mouse, beside, beside),
        0,
        "mouse min target"
    );
    // On the target, drifting 5 px before lifting: within a finger's slop,
    // past a mouse's.
    let (on, drifted) = (Offset::new(100.0, 100.0), Offset::new(105.0, 100.0));
    assert_eq!(contact(PointerKind::Touch, on, drifted), 1, "touch slop");
    assert_eq!(contact(PointerKind::Mouse, on, drifted), 0, "mouse slop");
}

/// The headless window hands its OS what a real host would: a region whose device edges do
/// not fit in `i32` is dropped, not saturated, and one that is not finite too.
#[test]
fn the_headless_window_drops_input_regions_no_host_could_send() {
    use flui_foundation::geometry::{DevicePoint, DeviceRect, Rect};
    use flui_platform::PlatformWindow as _;
    use flui_platform::input_regions::{DeviceInputRegion, InputRegion, InputRegionKind};

    const EXCLUSION: InputRegionKind = InputRegionKind::SystemGestureExclusion;
    let window = HeadlessWindow::new(40, 24);
    let beyond_i32 = f64::from(i32::MAX) + 10.0;
    window
        .set_input_regions(&[
            InputRegion::new(EXCLUSION, Rect::from_ltrb(0.0, 0.0, beyond_i32, 4.0)),
            InputRegion::new(EXCLUSION, Rect::from_ltrb(0.0, 0.0, f64::INFINITY, 4.0)),
            InputRegion::new(EXCLUSION, Rect::from_ltrb(1.0, 2.0, 3.0, 4.0)),
        ])
        .expect("the test host takes input regions");
    assert_eq!(
        window.input_region_updates(),
        vec![vec![DeviceInputRegion::new(
            EXCLUSION,
            DeviceRect::from_min_max(DevicePoint::new(1, 2), DevicePoint::new(3, 4)),
        )]]
    );
}

// ============================================================================
// Backing off a frame that keeps failing (R10, ADR-0258 §6)
// ============================================================================

/// A tripwire host, armed, whose window is on a display refreshing every
/// `period`.
fn tripwire_on_display(period: Duration) -> (HeadlessHost, Arc<AtomicBool>) {
    use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod, WindowDisplay};

    let (host, armed) = tripwire_ui_runtime(true);
    let display = WindowDisplay::new(
        DisplayRegistry::new()
            .mint(&DisplayKey::Injected(1))
            .expect("a fresh registry mints an id"),
        Some(RefreshPeriod::try_from(period).expect("a valid refresh period")),
    );
    host.set_window_display(host.primary_window(), Some(display));
    (host, armed)
}

/// Pump `host` once after `wait` and report whether the pump raised a dropped
/// frame: whether a frame was attempted and failed.
fn attempt_after(host: &mut HeadlessHost, wait: Duration) -> bool {
    catch_unwind(AssertUnwindSafe(|| host.pump(wait))).is_err()
}

/// With no input, sleep until each wake the UI runtime arms and pump, as a
/// host's loop does, until `span` has passed since the first failure.
/// Returns when each dropped frame was reported, on the host's clock.
fn failures_without_input(host: &mut HeadlessHost, span: Duration) -> Vec<Duration> {
    let mut reports = Vec::new();
    if attempt_after(host, Duration::ZERO) {
        reports.push(host.clock().elapsed());
    }
    let start = host.clock().elapsed();
    for _ in 0..500 {
        if host.clock().elapsed().saturating_sub(start) >= span {
            return reports;
        }
        let wait = host
            .time_to_next_frame()
            .expect("a frame that keeps failing keeps a retry armed");
        if attempt_after(host, wait) {
            reports.push(host.clock().elapsed());
        }
    }
    panic!("the wakes stopped advancing the clock: {reports:?}");
}

/// **A paint that panics on every attempt backs off but is never given up
/// on.** One row per refresh rate (60, 120 and 240 Hz), on the host's virtual
/// clock, with no input: every interval between two reports is at least one
/// refresh period, the intervals below
/// [`FAILURE_RETRY_CAP`](flui_runtime::frame_failure::FAILURE_RETRY_CAP) never
/// shrink, every interval once the cap is reached lies in
/// `[cap, cap + period)`, and ten caps' worth of time sees at least nine
/// reports.
///
/// Fails without the backoff: the frame is retried on every wake, so the
/// interval is a single period or none.
pub(crate) fn a_repeating_paint_panic_backs_off_but_keeps_retrying() {
    use flui_runtime::frame_failure::FAILURE_RETRY_CAP;

    for hz in [60_u64, 120, 240] {
        let period = Duration::from_nanos(1_000_000_000 / hz);
        let (mut host, _armed) = tripwire_on_display(period);
        let reports = failures_without_input(&mut host, FAILURE_RETRY_CAP * 16);
        let intervals: Vec<Duration> = reports
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0]))
            .collect();

        assert!(
            intervals.iter().all(|interval| *interval >= period),
            "{hz} Hz: an interval is shorter than one refresh period: {intervals:?}"
        );
        let capped_from = intervals
            .iter()
            .position(|interval| *interval >= FAILURE_RETRY_CAP)
            .unwrap_or_else(|| panic!("{hz} Hz: the backoff never reached its cap: {intervals:?}"));
        assert!(
            intervals[..capped_from]
                .windows(2)
                .all(|pair| pair[0] <= pair[1]),
            "{hz} Hz: an interval below the cap shrank: {intervals:?}"
        );
        assert!(
            intervals[capped_from..]
                .iter()
                .all(|interval| *interval < FAILURE_RETRY_CAP + period),
            "{hz} Hz: an interval past the cap left [cap, cap + period): {intervals:?}"
        );
        let capped_since = reports[capped_from];
        let in_ten_caps = reports
            .iter()
            .filter(|at| **at > capped_since && **at <= capped_since + FAILURE_RETRY_CAP * 10)
            .count();
        assert!(
            in_ten_caps >= 9,
            "{hz} Hz: {in_ten_caps} reports in ten caps' worth of time: {reports:?}"
        );
    }
}

/// **A single paint panic is redrawn without input.** The frame fails once,
/// its retry is armed one refresh period later, and the pump at that wake
/// paints; then nothing is left to wake for.
///
/// Fails without the retry deadline: a dropped frame leaves no wake, and a host
/// with no input never comes back to it.
pub(crate) fn a_single_paint_panic_redraws_without_input() {
    use flui_runtime::ui_runtime::DEFAULT_REFRESH_PERIOD;

    let (mut host, armed) = tripwire_ui_runtime(true);
    assert!(
        attempt_after(&mut host, Duration::ZERO),
        "the armed frame fails"
    );
    armed.store(false, Ordering::SeqCst);

    assert_eq!(
        host.time_to_next_frame(),
        Some(DEFAULT_REFRESH_PERIOD),
        "the first retry waits one refresh period"
    );
    let outcome = host
        .pump_next_frame()
        .expect("the dropped frame armed its retry");
    assert!(outcome.presented(), "the retry paints");
    assert_eq!(
        host.pump_until_settled(DEFAULT_REFRESH_PERIOD, 4),
        Ok(0),
        "nothing is left to wake for"
    );
}

/// **Pointer motion does not cut the backoff short; a press does, once.** With
/// the frame failing on every attempt, mouse moves across the window during
/// the wait attempt nothing. A button press tries at once; a second press in
/// the same wait does not.
///
/// Fails if motion or any input re-attempts the frame: a report arrives
/// before the deadline.
pub(crate) fn a_pointer_move_does_not_bypass_the_backoff() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::PointerId;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_platform::PlatformInput;

    let (mut host, _armed) = tripwire_ui_runtime(true);
    assert!(
        attempt_after(&mut host, Duration::ZERO),
        "the first frame fails"
    );
    // A second failure doubles the wait, leaving room for input inside it.
    let wait = host.time_to_next_frame().expect("a retry is armed");
    assert!(attempt_after(&mut host, wait), "the retry fails");
    let wait = host.time_to_next_frame().expect("a retry is armed");
    let step = wait / 8;

    let mouse = PointerId::try_from(1).expect("nonzero pointer id");
    for x in 1..=4 {
        let at = Offset::new(f64::from(x), 10.0);
        host.dispatch(PlatformInput::Pointer(
            make_move_event_for_id(mouse, at, PointerKind::Mouse).expect("move"),
        ));
        assert!(
            !attempt_after(&mut host, step),
            "a pointer move attempted the frame inside its backoff"
        );
    }

    let press = |host: &HeadlessHost| {
        let at = Offset::new(5.0, 10.0);
        host.dispatch(PlatformInput::Pointer(
            make_down_event_for_id(mouse, at, PointerKind::Mouse).expect("down"),
        ));
        host.dispatch(PlatformInput::Pointer(
            make_up_event_for_id(mouse, at, PointerKind::Mouse).expect("up"),
        ));
    };
    press(&host);
    assert!(
        attempt_after(&mut host, Duration::ZERO),
        "a press attempts the frame at once"
    );
    press(&host);
    assert!(
        !attempt_after(&mut host, Duration::ZERO),
        "a second press in the same wait does not"
    );
}

/// **A streak far past thirty-three failures keeps the one-second cap.** The
/// doubling is bounded before the shift, so a forty-failure streak waits
/// [`FAILURE_RETRY_CAP`](flui_runtime::frame_failure::FAILURE_RETRY_CAP), not
/// an overflowed shift's wait.
///
/// Fails with an unbounded shift: `T << 32` overflows and the wait wraps.
pub(crate) fn a_failure_streak_past_thirty_three_keeps_the_one_second_cap() {
    use flui_runtime::frame_failure::FAILURE_RETRY_CAP;

    let (mut host, _armed) = tripwire_ui_runtime(true);
    let mut failures = u32::from(attempt_after(&mut host, Duration::ZERO));
    for _ in 0..400 {
        if failures == 40 {
            break;
        }
        let wait = host.time_to_next_frame().expect("a retry is armed");
        if attempt_after(&mut host, wait) {
            failures += 1;
        }
    }
    assert_eq!(failures, 40, "every wake for the retry attempted the frame");
    assert_eq!(
        host.time_to_next_frame(),
        Some(FAILURE_RETRY_CAP),
        "the fortieth failure waits the cap"
    );
}
