//! The host's display delivery (ADR-0236), driven end to end through the
//! headless platform: injected displays and changes, real installed windows,
//! real owner turns. Declared in `owner_dispatch.rs` through `#[path]` because
//! the host it exercises is private to the runner.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use flui_platform::traits::PlatformWindow;
use flui_platform::{
    DisplayChange, DisplaySpec, HeadlessDisplayControls, HeadlessOwnerTurns, HeadlessPlatform,
    MockWindow, Platform as _,
};
use flui_platform_api::{DisplayId, DisplaySnapshot, WindowDisplay};

use super::super::host::{
    OwnerHostClearGuard, build_ui_runtime, install_owner_platform, refresh_displays_with,
    with_owner_platform,
};
use super::*;

const LEFT: usize = 0;
const RIGHT: usize = 1;

/// A booted headless host with two displays: the left one runs 16 ms frames,
/// the right one reports no period.
struct Rig {
    controls: HeadlessDisplayControls,
    turns: HeadlessOwnerTurns,
    ids: Vec<DisplayId>,
    _clear: OwnerHostClearGuard,
}

fn boot() -> Rig {
    let clear = OwnerHostClearGuard::arm();
    let platform = HeadlessPlatform::new().with_displays(vec![
        DisplaySpec::new("Left", 1920, 1080).with_refresh_period(Duration::from_millis(16)),
        DisplaySpec::new("Right", 2560, 1440),
    ]);
    let controls = platform.display_controls();
    let turns = platform.owner_turns();
    let ids = platform
        .display_snapshot()
        .expect("snapshot")
        .displays()
        .iter()
        .map(|display| display.id)
        .collect();
    Box::new(platform)
        .run(Box::new(|owner| {
            install_owner_platform(owner)?;
            Ok(())
        }))
        .expect("headless run");
    Rig {
        controls,
        turns,
        ids,
        _clear: clear,
    }
}

impl Rig {
    fn period(&self, index: usize, millis: u64) {
        self.controls
            .simulate_display_change(
                self.ids[index],
                DisplayChange::RefreshPeriod(Some(Duration::from_millis(millis))),
            )
            .expect("change a display");
    }

    fn accepted() -> Option<DisplaySnapshot> {
        APP_RUNTIME.with(|slot| slot.borrow().installed_host.displays().snapshot())
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        teardown_platform_ui_runtime();
    }
}

/// An installed window: its native handle, its presentation, and what the
/// presentation asked of the window.
struct Win {
    native: MockWindow,
    /// The platform keeps its windows alive; the headless one does not, so the
    /// test does.
    _window: Arc<dyn PlatformWindow>,
    dispatcher: PresentationDispatcher,
    /// Redraws the window was asked for.
    redraws: Arc<AtomicUsize>,
    /// While set, the redraw request fails the way a platform wake can.
    fail_redraw: Arc<AtomicBool>,
}

fn open_window(rig: &Rig, display: Option<usize>) -> Win {
    install_window(rig, display, None).0
}

/// A window whose frame driver is the deadline seam over `deadlines`: a frame
/// delivered to it arms the pacing fallback the way a pump that presented
/// nothing does, and the host's wake-deadline query reads it back.
fn open_paced_window(
    rig: &Rig,
    display: Option<usize>,
    deadlines: &Arc<super::super::desktop::DesktopWakeDeadlines>,
) -> (Win, super::super::frame_driver::FrameBinding) {
    let (win, frame) = install_window(rig, display, Some(Arc::clone(deadlines)));
    (win, frame.expect("a paced window has a frame binding"))
}

fn install_window(
    rig: &Rig,
    display: Option<usize>,
    deadlines: Option<Arc<super::super::desktop::DesktopWakeDeadlines>>,
) -> (Win, Option<super::super::frame_driver::FrameBinding>) {
    let host_window =
        with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
            .expect("an installed owner")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless windows are ready");
    let native = host_window
        .as_any()
        .downcast_ref::<MockWindow>()
        .expect("headless window")
        .clone();
    let window: Arc<dyn PlatformWindow> = host_window;
    if let Some(index) = display {
        native.simulate_move_to(rig.ids[index]).expect("move");
    }
    let redraws = Arc::new(AtomicUsize::new(0));
    let fail_redraw = Arc::new(AtomicBool::new(false));
    let counter = Arc::clone(&redraws);
    let failing = Arc::clone(&fail_redraw);
    window.on_request_frame(Box::new(move || {
        assert!(
            !failing.load(Ordering::SeqCst),
            "injected redraw wake failure"
        );
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let runtime = build_ui_runtime(Arc::clone(&window), 1.0).expect("runtime");
    let mut installation = prepare_ui_runtime_alongside(runtime, Arc::clone(&window));
    let dispatcher = installation.dispatcher();
    installation.observe_display();
    let binding = deadlines.map(|deadlines| {
        installation
            .frame_driver(super::super::frame_driver::FrameDriver::TestDeadlines(
                deadlines,
            ))
            .expect("deadline driver")
            .binding
    });
    installation
        .submit()
        .outcome()
        .expect("an idle host publishes synchronously")
        .expect("installed");
    (
        Win {
            native,
            _window: window,
            dispatcher,
            redraws,
            fail_redraw,
        },
        binding,
    )
}

impl Win {
    fn redraws(&self) -> usize {
        self.redraws.load(Ordering::SeqCst)
    }

    /// The display the presentation holds.
    fn display(&self) -> Option<WindowDisplay> {
        let id = self.dispatcher.address.presentation_id;
        let seen = Rc::new(RefCell::new(None));
        let out = Rc::clone(&seen);
        dispatch_platform_ui_runtime(
            self.dispatcher,
            RuntimeTask::TestCallback(Box::new(move |runtime| {
                *out.borrow_mut() = Some(runtime.window_display(id));
            })),
        )
        .expect("read the presentation");
        seen.borrow_mut().take().expect("read synchronously")
    }

    fn period_ms(&self) -> Option<u128> {
        self.display()
            .and_then(|display| display.refresh_period)
            .map(|period| period.get().as_millis())
    }
}

fn seed_holds_every_display_before_the_first_window() {
    let rig = boot();
    let accepted = Rig::accepted().expect("the host sampled at install");
    assert_eq!(accepted.displays().len(), 2, "no window is open yet");
    assert_eq!(
        accepted
            .get(rig.ids[LEFT])
            .and_then(|display| display.refresh_period)
            .map(flui_platform::RefreshPeriod::get),
        Some(Duration::from_millis(16))
    );
}

fn refresh_switch_publishes_a_new_snapshot() {
    let rig = boot();
    let before = Rig::accepted().expect("seed").revision();
    let win = open_window(&rig, None);
    assert_eq!(
        win.period_ms(),
        Some(16),
        "the seed carries the real period"
    );

    rig.period(LEFT, 8);
    rig.turns.drive();

    let after = Rig::accepted().expect("snapshot");
    assert_eq!(after.revision().get(), before.get() + 1);
    assert_eq!(
        win.period_ms(),
        Some(8),
        "the window follows its display's mode"
    );
}

fn a_change_racing_the_sample_still_reaches_the_window() {
    let rig = boot();
    let win = open_window(&rig, None);
    assert_eq!(win.period_ms(), Some(16));

    // The mode switches while the host is sampling: the sample it commits is the
    // old one, and the switch must still reach the window.
    let controls = rig.controls.clone();
    let left = rig.ids[LEFT];
    refresh_displays_with(move |owner| {
        let sampled = owner.display_snapshot();
        controls
            .simulate_display_change(
                left,
                DisplayChange::RefreshPeriod(Some(Duration::from_millis(7))),
            )
            .expect("change during the sample");
        sampled
    })
    .expect("sampling");
    assert_eq!(win.period_ms(), Some(16), "the committed sample was stale");

    rig.turns.drive();
    assert_eq!(
        win.period_ms(),
        Some(7),
        "the change that raced the sample was lost"
    );
}

fn snapshot_is_committed_before_observers_run() {
    let rig = boot();
    let win = open_window(&rig, None);
    let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    win.native.on_request_frame(Box::new(move || {
        let accepted = APP_RUNTIME.with(|slot| slot.borrow().installed_host.displays().snapshot());
        log.lock()
            .push(accepted.map(|snapshot| snapshot.revision().get()));
    }));
    let before = Rig::accepted().expect("seed").revision().get();

    // One sample, taken directly: owner turns re-sample until they are idle, and
    // a later pass would hide what the first one did.
    rig.period(LEFT, 8);
    refresh_displays_with(flui_platform::OwnerPlatform::display_snapshot).expect("sample");

    assert_eq!(
        *seen.lock(),
        [Some(before + 1)],
        "the window observer ran before the host committed the snapshot"
    );
}

fn panicking_window_observer_keeps_the_snapshot_and_the_tail() {
    let rig = boot();
    let failing = open_window(&rig, None);
    let healthy = open_window(&rig, None);
    failing.fail_redraw.store(true, Ordering::SeqCst);
    let before = Rig::accepted().expect("seed").revision().get();

    rig.period(LEFT, 8);
    // The owner loop contains the failure; what is left to check is its effect.
    rig.turns.drive();

    assert_eq!(
        Rig::accepted().expect("snapshot").revision().get(),
        before + 1,
        "a failing window erased the accepted snapshot"
    );
    assert_eq!(
        healthy.period_ms(),
        Some(8),
        "a failing sibling cancelled the delivery of the others"
    );
}

fn apply_panic_keeps_the_debt_and_the_next_equal_change_is_delivered() {
    let rig = boot();
    let win = open_window(&rig, None);
    win.fail_redraw.store(true, Ordering::SeqCst);

    rig.period(LEFT, 8);
    rig.turns.drive();
    assert_eq!(
        win.period_ms(),
        Some(16),
        "a failed apply must leave the old value and the debt"
    );

    // The platform recovers; the next change leaves the window's own display
    // exactly as it was last derived, yet the owed value is delivered.
    win.fail_redraw.store(false, Ordering::SeqCst);
    rig.controls
        .simulate_display_change(rig.ids[LEFT], DisplayChange::Name("renamed".into()))
        .expect("an unrelated field");
    rig.turns.drive();
    assert_eq!(
        win.period_ms(),
        Some(8),
        "an equal derived value after a failed apply was not delivered"
    );
}

fn foreign_thread_invalidation_wakes_the_owner() {
    let rig = boot();
    let win = open_window(&rig, None);
    let owner_thread = std::thread::current().id();
    let ran_on = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let log = Arc::clone(&ran_on);
    win.native.on_request_frame(Box::new(move || {
        log.lock().push(std::thread::current().id());
    }));

    let controls = rig.controls.clone();
    let left = rig.ids[LEFT];
    std::thread::spawn(move || {
        controls
            .simulate_display_change(
                left,
                DisplayChange::RefreshPeriod(Some(Duration::from_millis(8))),
            )
            .expect("change from a foreign thread");
    })
    .join()
    .expect("foreign thread");
    assert!(
        ran_on.lock().is_empty() && win.period_ms() == Some(16),
        "application code ran on the foreign thread"
    );

    rig.turns.drive();
    assert_eq!(
        win.period_ms(),
        Some(8),
        "the invalidation never woke the owner"
    );
    assert_eq!(*ran_on.lock(), [owner_thread]);
}

fn moving_to_another_display_notifies_once() {
    let rig = boot();
    let win = open_window(&rig, None);
    let before = win.redraws();

    win.native.simulate_move_to(rig.ids[RIGHT]).expect("move");
    rig.turns.drive();

    assert_eq!(win.redraws(), before + 1, "one move is one notification");
    assert_eq!(
        win.display().map(|display| display.id),
        Some(rig.ids[RIGHT])
    );
    assert_eq!(win.period_ms(), None, "the right display reports no period");
}

fn refresh_change_of_own_display_notifies() {
    let rig = boot();
    let win = open_window(&rig, None);
    let before = win.redraws();

    rig.period(LEFT, 8);
    rig.turns.drive();

    assert_eq!(win.redraws(), before + 1);
}

fn other_displays_change_is_silent() {
    let rig = boot();
    let win = open_window(&rig, Some(LEFT));
    let before = win.redraws();

    rig.period(RIGHT, 8);
    rig.turns.drive();

    assert_eq!(
        win.redraws(),
        before,
        "a display the window is not on woke it"
    );
    assert_eq!(win.period_ms(), Some(16));
}

fn unread_field_change_is_silent() {
    let rig = boot();
    let win = open_window(&rig, None);
    let before = win.redraws();

    rig.controls
        .simulate_display_change(rig.ids[LEFT], DisplayChange::Name("renamed".into()))
        .expect("rename");
    rig.controls
        .simulate_display_change(rig.ids[LEFT], DisplayChange::ScaleFactor(2.0))
        .expect("rescale");
    rig.turns.drive();

    assert_eq!(
        win.redraws(),
        before,
        "a field the window does not read woke it"
    );
}

fn several_changes_before_the_turn_deliver_the_last() {
    let rig = boot();
    let win = open_window(&rig, None);
    let before = win.redraws();

    rig.period(LEFT, 12);
    rig.period(LEFT, 10);
    rig.period(LEFT, 8);
    rig.turns.drive();

    assert_eq!(win.period_ms(), Some(8));
    assert_eq!(win.redraws(), before + 1, "three changes, one delivery");
}

fn new_secondary_window_starts_on_its_display() {
    let rig = boot();
    let _first = open_window(&rig, None);
    let second = open_window(&rig, Some(RIGHT));

    assert_eq!(
        second.display().map(|display| display.id),
        Some(rig.ids[RIGHT]),
        "the second window started on the primary display's default"
    );
}

fn window_opened_after_a_change_starts_with_the_new_period() {
    let rig = boot();
    rig.period(LEFT, 8);
    rig.turns.drive();

    let win = open_window(&rig, None);

    assert_eq!(
        win.period_ms(),
        Some(8),
        "the first frame would pace by the pre-change period"
    );
}

fn closed_window_receives_nothing() {
    let rig = boot();
    let win = open_window(&rig, None);
    let _other = open_window(&rig, None);
    let before = win.redraws();
    super::close_this_window(win.dispatcher);

    rig.period(LEFT, 8);
    rig.turns.drive();

    assert_eq!(win.redraws(), before, "a closed window was notified");
}

#[test]
fn display_delivery_contract() {
    crate::table_test::run_table(
        "display_delivery_contract",
        &[
            (
                "seed_holds_every_display_before_the_first_window",
                seed_holds_every_display_before_the_first_window as fn(),
            ),
            (
                "refresh_switch_publishes_a_new_snapshot",
                refresh_switch_publishes_a_new_snapshot as fn(),
            ),
            (
                "a_change_racing_the_sample_still_reaches_the_window",
                a_change_racing_the_sample_still_reaches_the_window as fn(),
            ),
            (
                "snapshot_is_committed_before_observers_run",
                snapshot_is_committed_before_observers_run as fn(),
            ),
            (
                "panicking_window_observer_keeps_the_snapshot_and_the_tail",
                panicking_window_observer_keeps_the_snapshot_and_the_tail as fn(),
            ),
            (
                "apply_panic_keeps_the_debt_and_the_next_equal_change_is_delivered",
                apply_panic_keeps_the_debt_and_the_next_equal_change_is_delivered as fn(),
            ),
            (
                "foreign_thread_invalidation_wakes_the_owner",
                foreign_thread_invalidation_wakes_the_owner as fn(),
            ),
        ],
    );
}

#[test]
fn window_display_observation_contract() {
    crate::table_test::run_table(
        "window_display_observation_contract",
        &[
            (
                "moving_to_another_display_notifies_once",
                moving_to_another_display_notifies_once as fn(),
            ),
            (
                "refresh_change_of_own_display_notifies",
                refresh_change_of_own_display_notifies as fn(),
            ),
            (
                "other_displays_change_is_silent",
                other_displays_change_is_silent as fn(),
            ),
            (
                "unread_field_change_is_silent",
                unread_field_change_is_silent as fn(),
            ),
            (
                "several_changes_before_the_turn_deliver_the_last",
                several_changes_before_the_turn_deliver_the_last as fn(),
            ),
            (
                "new_secondary_window_starts_on_its_display",
                new_secondary_window_starts_on_its_display as fn(),
            ),
            (
                "window_opened_after_a_change_starts_with_the_new_period",
                window_opened_after_a_change_starts_with_the_new_period as fn(),
            ),
            (
                "closed_window_receives_nothing",
                closed_window_receives_nothing as fn(),
            ),
        ],
    );
}

fn wake_deadlines() -> Arc<super::super::desktop::DesktopWakeDeadlines> {
    Arc::new(super::super::desktop::DesktopWakeDeadlines {
        device_recovery_backoff: Arc::new(
            super::super::device_recovery::new_device_recovery_backoff(),
        ),
        fallback: Arc::new(super::super::frame_pacing::FallbackWake::new()),
        first_reveal: Arc::new(super::super::first_reveal::FirstReveal::new()),
    })
}

/// 0.95 of a period, the fraction the fallback waits (ADR-0058).
fn deferral(period: Duration) -> Duration {
    period.mul_f64(0.95)
}

/// What the host's wake-deadline query reports across every installed window.
fn frame_deadline() -> Option<web_time::Instant> {
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state
            .installed_host
            .native()
            .frame_deadline(state.installed_host.logical())
    })
}

/// How far ahead of now the window's pacing deadline sits after a frame that
/// presented nothing: the frame is delivered to the window's driver, which arms
/// the fallback from the presentation's refresh interval, and the deadline is
/// read back through the host's wake-deadline query. The smallest of several
/// frames, because a slow host can only lengthen the gap.
fn armed_gap(win: &Win, frame: super::super::frame_driver::FrameBinding) -> Duration {
    (0..5)
        .map(|_| {
            let before = web_time::Instant::now();
            dispatch_platform_ui_runtime(win.dispatcher, RuntimeTask::Frame(frame))
                .expect("deliver a frame");
            frame_deadline()
                .expect("the frame armed the fallback")
                .saturating_duration_since(before)
        })
        .min()
        .expect("five frames")
}

/// The gap is the deferral plus however long the host took; this is how much
/// slack a healthy run is allowed, far below the difference between the
/// periods these rows tell apart.
const SLACK: Duration = Duration::from_millis(5);

fn assert_paced_by(gap: Duration, period: Duration, what: &str) {
    let expected = deferral(period);
    assert!(
        gap >= expected && gap < expected + SLACK,
        "{what}: expected about {expected:?}, got {gap:?}"
    );
}

fn fallback_wake_follows_a_refresh_change_without_resize() {
    let rig = boot();
    let deadlines = wake_deadlines();
    let (win, frame) = open_paced_window(&rig, None, &deadlines);
    assert_paced_by(
        armed_gap(&win, frame),
        Duration::from_millis(16),
        "at install",
    );

    // The display switches mode; the window is not resized or moved.
    rig.period(LEFT, 8);
    rig.turns.drive();

    assert_paced_by(
        armed_gap(&win, frame),
        Duration::from_millis(8),
        "after the mode switch (the fallback kept the period it had at install)",
    );
}

fn fallback_wake_returns_to_60_hz_when_the_period_becomes_unknown() {
    let rig = boot();
    let deadlines = wake_deadlines();
    let (win, frame) = open_paced_window(&rig, None, &deadlines);
    rig.period(LEFT, 8);
    rig.turns.drive();
    assert_paced_by(
        armed_gap(&win, frame),
        Duration::from_millis(8),
        "on the 8 ms display",
    );

    // The window moves to a display that reports nothing.
    win.native.simulate_move_to(rig.ids[RIGHT]).expect("move");
    rig.turns.drive();

    assert_paced_by(
        armed_gap(&win, frame),
        flui_runtime::ui_runtime::DEFAULT_REFRESH_PERIOD,
        "on a display with no period (the fallback held the previous display's)",
    );
}

fn secondary_window_paces_by_its_own_display() {
    let rig = boot();
    rig.period(RIGHT, 8);
    rig.turns.drive();
    let (main_deadlines, secondary_deadlines) = (wake_deadlines(), wake_deadlines());
    let (main, main_frame) = open_paced_window(&rig, Some(LEFT), &main_deadlines);
    let (secondary, secondary_frame) = open_paced_window(&rig, Some(RIGHT), &secondary_deadlines);

    assert_paced_by(
        armed_gap(&main, main_frame),
        Duration::from_millis(16),
        "the main window",
    );
    // The query reports the earliest deadline of any window, so the secondary's
    // is the one it reports only if it is the nearer: a secondary paced by the
    // main window's 16 ms would leave the main window's deadline the earliest.
    assert_paced_by(
        armed_gap(&secondary, secondary_frame),
        Duration::from_millis(8),
        "the secondary window (paced by the main window's display)",
    );
}

fn a_deadline_ten_ms_late_is_abandoned_after_a_move_to_an_8_ms_display() {
    let rig = boot();
    rig.period(RIGHT, 8);
    rig.turns.drive();
    let deadlines = wake_deadlines();
    let (win, frame) = open_paced_window(&rig, Some(LEFT), &deadlines);
    dispatch_platform_ui_runtime(win.dispatcher, RuntimeTask::Frame(frame)).expect("frame");
    let deadline = frame_deadline().expect("armed on the 16 ms display");

    // Let the deadline pass by 10 ms.
    let wait =
        (deadline + Duration::from_millis(10)).saturating_duration_since(web_time::Instant::now());
    std::thread::sleep(wait);
    let late = web_time::Instant::now().saturating_duration_since(deadline);
    if late < Duration::from_millis(16) {
        assert!(
            frame_deadline().is_some(),
            "{late:?} late on a 16 ms display is still a wake to answer"
        );
    }

    win.native.simulate_move_to(rig.ids[RIGHT]).expect("move");
    rig.turns.drive();

    assert_eq!(
        frame_deadline(),
        None,
        "{late:?} late on an 8 ms display is a wake that is not coming"
    );
}

#[test]
fn display_pacing_contract() {
    crate::table_test::run_table(
        "display_pacing_contract",
        &[
            (
                "fallback_wake_follows_a_refresh_change_without_resize",
                fallback_wake_follows_a_refresh_change_without_resize as fn(),
            ),
            (
                "fallback_wake_returns_to_60_hz_when_the_period_becomes_unknown",
                fallback_wake_returns_to_60_hz_when_the_period_becomes_unknown as fn(),
            ),
            (
                "secondary_window_paces_by_its_own_display",
                secondary_window_paces_by_its_own_display as fn(),
            ),
            (
                "a_deadline_ten_ms_late_is_abandoned_after_a_move_to_an_8_ms_display",
                a_deadline_ten_ms_late_is_abandoned_after_a_move_to_an_8_ms_display as fn(),
            ),
        ],
    );
}
