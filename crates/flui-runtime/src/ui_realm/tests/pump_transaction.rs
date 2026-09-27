//! `UiRealm::pump`, the frame transaction (ADR-0083 §1): apply commands, begin
//! frame, draw frame, end frame, at the one timestamp the pump's clock
//! returns — and `pump_background`, the frames-disabled wake.
//!
//! Each test drives the pump itself, never a hand-assembled
//! `drive_frame_with_lane` around `draw_frame`, so each fails against a pump
//! that skips or reorders the phase it names.

use std::time::Duration;

use flui_animation::{Animation, AnimationController};
use flui_foundation::notifier::Listenable as _;
use flui_types::geometry::px;

use super::*;
use crate::testing::ManualClock;

/// A post-frame callback scheduled on the realm's owner-local lane runs once,
/// after the pipeline, and sees the layout this same pump committed.
///
/// Fails against a pump that skips end frame (no call), that drives the
/// pipeline after end frame (the callback sees no layout), or that ends the
/// frame without the realm's local lane (the lane is never drained).
#[test]
fn pump_post_frame_callback_observes_this_frames_committed_layout() {
    use flui_rendering::prelude::Leaf;
    use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, PaintCx, RenderBox};

    #[derive(Debug, Default)]
    struct FixedBox;
    impl flui_foundation::Diagnosticable for FixedBox {}
    impl RenderBox for FixedBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_types::Size {
            flui_types::Size::new(px(40.0), px(24.0))
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    }

    let mut realm = UiRealm::for_test();
    let pipeline = realm.pipeline_for_test();
    let root = pipeline.with_mut(|owner| {
        let root = owner.insert::<flui_rendering::protocol::BoxProtocol>(Box::new(FixedBox));
        owner.set_root_id(Some(root));
        root
    });
    assert_eq!(
        pipeline.with(|owner| owner.box_size(root)),
        None,
        "nothing is laid out before the first frame"
    );

    let observed = Arc::new(RwLock::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_cb = Arc::clone(&observed);
    let calls_cb = Arc::clone(&calls);
    let pipeline_cb = pipeline.clone();
    // `PipelineCell` is `!Send`, so the callback goes on the owner-local
    // lane, which only a pump that ends its frame with that lane drains.
    realm
        .widgets()
        .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
        .expect("owner-local post-frame handle installed by UiRealm::construct")
        .schedule_local(move |_timing| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            *observed_cb.write() = pipeline_cb.with(|owner| owner.box_size(root));
        })
        .expect("the realm's local post-frame lane outlives this call");

    // The pump lays the root out tight to the sink's surface (at the test
    // realm's device pixel ratio of 1), so the surface is the box's own size.
    let _ = realm.pump(
        &mut ManualClock::new(),
        &mut ScriptedSink::always_presents().with_size(40, 24),
    );

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the pump's end frame drains the realm's local post-frame lane exactly once"
    );
    assert_eq!(
        *observed.read(),
        Some(flui_types::Size::new(px(40.0), px(24.0))),
        "the post-frame callback must observe THIS pump's committed layout"
    );
}

/// A controller ticking on the realm's scheduler (a transient frame callback)
/// advances across two pumps.
///
/// Fails against a pump that skips begin frame: the transient callback never
/// runs and the value stays 0. It pins the begin-frame phase, not the frame
/// clock: `flui_scheduler::Ticker` measures elapsed time from the wall clock,
/// not from the frame's timestamp, so the test lets a little wall time pass
/// between the pumps rather than relying on the manual clock's advance.
#[test]
fn pump_advances_a_scheduler_ticker_between_two_pumps() {
    let mut realm = UiRealm::for_test();
    let controller = AnimationController::new(Duration::from_secs(10), realm.scheduler());
    controller.forward().expect("fresh controller forwards");
    let mut clock = ManualClock::new();
    let mut sink = ScriptedSink::always_presents();

    let _ = realm.pump(&mut clock, &mut sink);
    let first = controller.value();
    std::thread::sleep(Duration::from_millis(2));
    clock.advance(Duration::from_millis(16));
    let _ = realm.pump(&mut clock, &mut sink);
    let second = controller.value();

    assert!(
        second > first,
        "the second pump's begin frame must tick the controller forward \
         (first={first}, second={second})"
    );
    controller.dispose();
}

/// A controller registered with the realm's `Vsync` ticks at the frame
/// clock's timestamp: two pumps 50 ms apart on the manual clock put a 100 ms
/// linear run exactly halfway, however little wall time passed.
///
/// Fails against a pump that does not publish its clock's timestamp to the
/// `Vsync` tick: the tick reads the wall clock, microseconds apart, and the
/// value stays near 0.
#[test]
fn pump_ticks_vsync_controllers_at_the_frame_clocks_time() {
    let mut realm = UiRealm::for_test();
    let controller = AnimationController::new(
        Duration::from_millis(100),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller forwards");
    let mut clock = ManualClock::new();
    let mut sink = ScriptedSink::always_presents();

    // Anchors the run at the first pump's timestamp.
    let _ = realm.pump(&mut clock, &mut sink);
    clock.advance(Duration::from_millis(50));
    let _ = realm.pump(&mut clock, &mut sink);

    let value = controller.value();
    assert!(
        (value - 0.5).abs() < 1e-4,
        "50 ms of frame clock into a 100 ms run is halfway (value={value})"
    );
    controller.dispose();
}

/// The realm's `Vsync` registry ticks in the scheduler's persistent phase, at
/// the start of the draw step — not among the transient callbacks, where
/// Flutter's tickers run. A recorded divergence (this crate's
/// `ARCHITECTURE.md`, "`Vsync` ticks in the persistent phase"); moving the tick
/// into begin frame turns this red.
#[test]
fn pump_ticks_vsync_in_the_persistent_phase_not_among_transient_callbacks() {
    let mut realm = UiRealm::for_test();
    let controller = AnimationController::new(
        Duration::from_millis(100),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    let phases = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let phases_in_listener = Arc::clone(&phases);
    let scheduler = realm.scheduler().clone();
    let _listener = controller.add_listener(Arc::new(move || {
        phases_in_listener.lock().push(scheduler.phase());
    }));
    controller.forward().expect("fresh controller forwards");
    phases.lock().clear();

    let _ = realm.pump(
        &mut ManualClock::new(),
        &mut ScriptedSink::always_presents(),
    );

    let phases = phases.lock();
    assert!(
        !phases.is_empty(),
        "the pump must tick the running controller"
    );
    assert!(
        phases
            .iter()
            .all(|phase| *phase == SchedulerPhase::PersistentCallbacks),
        "the Vsync tick runs in the persistent phase (got {phases:?})"
    );
    controller.dispose();
}

/// With frames disabled, `pump_background` clears the scheduler's frame latch
/// BEFORE it polls the async driver, so a future that schedules a frame when
/// polled fires the platform wake again.
///
/// Fails against the reversed order: the poll finds the latch still set, the
/// future's frame request fires nothing, and the loop would sleep through it.
#[test]
fn pump_background_clears_the_frame_latch_before_polling() {
    let (wake, wakes) = counting_wake();
    let mut realm = new_runtime(wake).expect("runtime");
    let scheduler = realm.scheduler().clone();
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    assert!(
        !scheduler.frames_enabled(),
        "precondition: a hidden app has frames disabled"
    );

    let scheduler_in_task = scheduler.clone();
    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_task = Arc::clone(&polled);
    let _token = scheduler.spawn_local(Box::pin(async move {
        polled_in_task.store(true, Ordering::SeqCst);
        scheduler_in_task.request_frame();
    }));
    scheduler.request_frame();
    assert!(
        scheduler.is_frame_scheduled(),
        "precondition: the frame latch is set going into the background wake"
    );
    let before = wakes.load(Ordering::SeqCst);

    realm.pump_background();

    assert!(
        polled.load(Ordering::SeqCst),
        "the background wake polls the async driver"
    );
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        before + 1,
        "the future's frame request must find the latch cleared and fire the wake"
    );
}

/// A command sent before the pump is applied by that pump, at the Idle
/// boundary before its frame begins.
///
/// Fails against a pump that skips the apply-commands step: the pop stays
/// queued and the navigator keeps both routes.
#[test]
fn pump_applies_commands_sent_before_it() {
    let mut realm = new_runtime(noop_wake()).expect("runtime");
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(test_route("/"));
    let _pushed = navigator.push(test_route("/details"));
    realm
        .command_sender()
        .send_navigation(NavigatorCommand::pop(navigator.command_target()))
        .expect("inbox has room");
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "precondition: nothing applies a command before the owner drains it"
    );

    let _ = realm.pump(
        &mut ManualClock::new(),
        &mut ScriptedSink::always_presents(),
    );

    assert_eq!(
        navigator.route_ids().len(),
        1,
        "the pump's apply-commands step commits the queued pop"
    );
}
