//! What reads the display's refresh period once the host has delivered it
//! (ADR-0236): the frame budget, the pointer resampler's window, the
//! refresh-interval metric and the haptic frame deadline. Each row delivers a
//! display through the host's own observation and reads the consequence
//! through the public runtime.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::PresentationAddress;
use flui_interaction::pointer::DEFAULT_SAMPLE_PERIOD;
use flui_platform::{
    DisplayKey, DisplayRegistry, GestureGeometry, PlatformWindow, PreferenceQueryError,
    RefreshPeriod, WindowDisplay,
};
use flui_runtime::owner::{
    DisplayUpdate, InputOutcome, OwnerEffects, OwnerHost, PresentationDispatcher, WindowObservation,
};

use crate::HostInputExt as _;
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::SampledClock;
use flui_runtime::ui_runtime::{DEFAULT_REFRESH_PERIOD, RuntimeHostServices, UiRuntime};

struct Sink;
impl flui_runtime::sink::FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (160, 80)
    }
    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

/// An owner whose frames pump the runtime at the instant the test chose.
struct Effects {
    frame_at: Cell<Option<web_time::Instant>>,
}

impl OwnerEffects for Effects {
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn finish_install(
        &self,
        _: PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn cancel_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) {
        panic!("no pending install");
    }
    fn resize_surface(&self, _: PresentationAddress, _: flui_foundation::geometry::WindowMetrics) {}
    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        let at = self.frame_at.take().expect("a frame was asked for");
        let outcome = runtime.pump(&mut SampledClock(at), &mut Sink);
        assert!(outcome.presented() || !outcome.presented());
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn request_continuation(&self) -> bool {
        true
    }
}

/// A window that counts how often the gesture geometry was re-read.
struct CountingWindow {
    inner: Arc<dyn PlatformWindow>,
    geometry_reads: Arc<std::sync::atomic::AtomicUsize>,
}

impl PlatformWindow for CountingWindow {
    fn id(&self) -> flui_platform::WindowId {
        self.inner.id()
    }
    fn physical_size(&self) -> flui_foundation::geometry::Size<i32> {
        self.inner.physical_size()
    }
    fn logical_size(&self) -> flui_foundation::geometry::Size<f64> {
        self.inner.logical_size()
    }
    fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }
    fn request_redraw(&self) {
        self.inner.request_redraw();
    }
    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }
    fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }
    fn set_cursor(
        &self,
        cursor: flui_platform::CursorIcon,
    ) -> Result<(), flui_platform::CursorError> {
        self.inner.set_cursor(cursor)
    }
    fn state(&self) -> flui_platform::WindowState {
        let size = self.logical_size();
        flui_platform::WindowState::new(flui_platform::WindowGeometry::new(None, size), size)
    }

    fn capabilities(&self) -> flui_platform::WindowCapabilities {
        flui_platform::WindowCapabilities::NONE
    }

    fn apply(
        &self,
        command: flui_platform::WindowCommand,
    ) -> Result<(), flui_platform::WindowCommandError> {
        command.admit(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }

    fn text_input(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
        self.geometry_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(None)
    }
}

fn runtime_on(window: PresentationWindow) -> UiRuntime {
    UiRuntime::new(
        window,
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime")
}

struct Rig {
    owner: OwnerHost,
    address: PresentationAddress,
    effects: Effects,
    window: PresentationDispatcher,
    displays: DisplayRegistry,
}

fn rig_with(window: PresentationWindow) -> Rig {
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime_on(window)))
        .expect("publish")
        .commit();
    let window = owner.presentation_dispatcher(address).expect("window");
    Rig {
        owner,
        address,
        effects: Effects {
            frame_at: Cell::new(None),
        },
        window,
        displays: DisplayRegistry::new(),
    }
}

fn rig() -> Rig {
    rig_with(crate::owner_publication::window())
}

fn period(micros: u64) -> RefreshPeriod {
    RefreshPeriod::try_from(Duration::from_micros(micros)).expect("a valid period")
}

impl Rig {
    fn display(&self, key: u64, micros: Option<u64>) -> WindowDisplay {
        WindowDisplay::new(
            self.displays.mint(&DisplayKey::Injected(key)).expect("id"),
            micros.map(period),
        )
    }

    /// Deliver `display` to the primary presentation.
    fn deliver(&self, display: Option<WindowDisplay>) {
        self.deliver_to(&self.window, display);
    }

    fn deliver_to(&self, window: &PresentationDispatcher, display: Option<WindowDisplay>) {
        assert_eq!(
            window.observe(
                WindowObservation::Display(DisplayUpdate::new(display)),
                &self.effects,
            ),
            Ok(InputOutcome::Handled)
        );
    }

    /// Read something off the primary presentation's runtime.
    fn read<T: 'static>(
        &self,
        read: impl FnOnce(&UiRuntime, PresentationAddress) -> T + 'static,
    ) -> T {
        let address = self.address;
        let out = Rc::new(RefCell::new(None));
        let slot = Rc::clone(&out);
        self.window
            .test_callback(
                Box::new(move |runtime| *slot.borrow_mut() = Some(read(runtime, address))),
                &self.effects,
            )
            .expect("read the runtime");
        out.borrow_mut().take().expect("read synchronously")
    }

    /// Janky frames the scheduler counted after one frame whose timestamp is
    /// `late` in the past, so it has already run that long when it ends.
    fn janky_after_a_frame_that_ran(&self, late: Duration) -> u64 {
        self.effects.frame_at.set(Some(
            web_time::Instant::now()
                .checked_sub(late)
                .expect("the clock reaches back a frame"),
        ));
        self.owner
            .frame_dispatcher(self.address)
            .expect("frame authority")
            .deliver(crate::VSYNC, &self.effects)
            .expect("frame");
        self.read(|runtime, _| runtime.scheduler().janky_frame_count())
    }
}

/// A frame this far in the past is janky on a 120 Hz display and not against a
/// one-second period. The smooth case uses a period far above any run time, as
/// a loaded host can only lengthen the frame.
const FRAME_RUNTIME: Duration = Duration::from_millis(12);
const SMOOTH_PERIOD_US: u64 = 1_000_000;

fn jank_is_judged_against_the_current_period() {
    for (micros, janky) in [(8_333, 1), (SMOOTH_PERIOD_US, 0)] {
        let rig = rig();
        let display = rig.display(1, Some(micros));
        rig.deliver(Some(display));
        assert_eq!(
            rig.janky_after_a_frame_that_ran(FRAME_RUNTIME),
            janky,
            "an {FRAME_RUNTIME:?} frame on a {micros:?} us display"
        );
    }
}

fn shared_frame_budget_uses_the_shortest_period() {
    // One runtime serving a 60 Hz window and a 120 Hz window judges its frame by
    // the faster one, whichever window the frame happens to belong to.
    for fast_first in [true, false] {
        let rig = rig();
        let second = rig
            .owner
            .prepare_presentation(rig.address, crate::owner_publication::window())
            .expect("second presentation");
        let second = rig.owner.publication(second).expect("publish").commit();
        let second = rig
            .owner
            .presentation_dispatcher(second)
            .expect("second window");
        let (fast, slow) = (
            rig.display(1, Some(8_333)),
            rig.display(2, Some(SMOOTH_PERIOD_US)),
        );
        let (first_display, second_display) = if fast_first {
            (fast, slow)
        } else {
            (slow, fast)
        };
        rig.deliver_to(&rig.window, Some(first_display));
        rig.deliver_to(&second, Some(second_display));
        assert_eq!(
            rig.janky_after_a_frame_that_ran(FRAME_RUNTIME),
            1,
            "the budget followed the slower window (fast window first: {fast_first})"
        );
    }
}

fn a_hidden_window_does_not_set_the_shared_frame_budget() {
    // A minimised window on a fast display is not produced for, so the frames
    // of the visible window beside it are judged by the visible window's period.
    let rig = rig();
    let hidden = rig
        .owner
        .prepare_presentation(rig.address, crate::owner_publication::window())
        .expect("second presentation");
    let hidden = rig.owner.publication(hidden).expect("publish").commit();
    let hidden = rig
        .owner
        .presentation_dispatcher(hidden)
        .expect("second window");
    rig.deliver_to(&rig.window, Some(rig.display(2, Some(SMOOTH_PERIOD_US))));
    rig.deliver_to(&hidden, Some(rig.display(1, Some(8_333))));
    assert_eq!(
        hidden.observe(WindowObservation::Visibility(false), &rig.effects),
        Ok(InputOutcome::Handled)
    );
    assert_eq!(
        rig.janky_after_a_frame_that_ran(FRAME_RUNTIME),
        0,
        "the hidden window's faster display does not judge the visible window's frame"
    );
}

fn frame_aligned_resampling_window_follows_the_display_period() {
    let rig = rig();
    let window_of = |rig: &Rig| rig.read(|runtime, _| runtime.gestures().sampling_clock().period());
    assert_eq!(
        window_of(&rig),
        DEFAULT_SAMPLE_PERIOD,
        "a presentation with no display starts on the 60 Hz window"
    );
    rig.deliver(Some(rig.display(1, Some(8_333))));
    assert_eq!(
        window_of(&rig),
        Duration::from_micros(8_333),
        "a 120 Hz display left the resampling window at 60 Hz"
    );
    rig.deliver(None);
    assert_eq!(
        window_of(&rig),
        DEFAULT_REFRESH_PERIOD,
        "a window moved to an unknown display kept the previous window"
    );
}

fn unknown_refresh_period_uses_the_60_hz_fallback() {
    let rig = rig();
    let interval =
        |rig: &Rig| rig.read(|runtime, address| runtime.refresh_interval(address.presentation_id));
    assert_eq!(
        interval(&rig),
        DEFAULT_REFRESH_PERIOD,
        "before any delivery"
    );
    rig.deliver(Some(rig.display(1, Some(8_333))));
    assert_eq!(interval(&rig), Duration::from_micros(8_333));
    // A display that reports no period is a known display with an unknown period.
    rig.deliver(Some(rig.display(2, None)));
    assert_eq!(interval(&rig), DEFAULT_REFRESH_PERIOD);
    rig.deliver(Some(rig.display(1, Some(8_333))));
    rig.deliver(None);
    assert_eq!(
        interval(&rig),
        DEFAULT_REFRESH_PERIOD,
        "a window on an unknown display kept the previous period"
    );
    assert_eq!(DEFAULT_REFRESH_PERIOD, Duration::from_micros(16_667));
}

/// A frame-aligned haptic queued on a display gets that display's deadline:
/// the factor times its period, read again after the window moves. The
/// runtime's next wake is the earliest of its deadlines, so a request moves
/// it only when its own deadline is the earliest.
fn haptic_frame_deadline_follows_the_display() {
    use flui_platform::HapticFeedback;
    use flui_platform::haptics::{HapticOutcome, HapticRequest, HapticTiming};

    // Two periods of a one-second display end after 900 ms, so that request
    // never wakes the runtime before its other wakes or that bound; two
    // periods of a 120 Hz display end before 50 ms (as do two 60 Hz frames,
    // the deadline of a display read as unknown, which the first half tells
    // apart).
    const SLOW_US: u64 = 1_000_000;
    const FAST_US: u64 = 8_333;
    const SLOW_BOUND: Duration = Duration::from_millis(900);
    const FAST_BOUND: Duration = Duration::from_millis(50);
    let rig = rig();
    // The next wake before and after one request is queued, and when.
    let queue = |rig: &Rig| {
        rig.read(|runtime, _| {
            let before = runtime.next_wake();
            let handle = runtime
                .widgets()
                .with_build_owner(|owner| owner.haptics_handle().cloned())
                .expect("the presentation installs its haptics");
            let queued = web_time::Instant::now();
            let outcome = handle.perform(
                HapticFeedback::Drop,
                HapticRequest::new(HapticTiming::FramePresented),
            );
            assert!(matches!(outcome, HapticOutcome::Queued(_)), "{outcome:?}");
            let after = runtime
                .next_wake()
                .expect("a queued request wakes the runtime at its deadline");
            (before, queued, after)
        })
    };

    rig.deliver(Some(rig.display(1, Some(SLOW_US))));
    let (before, queued, after) = queue(&rig);
    let earliest_other = before.map_or(queued + SLOW_BOUND, |wake| wake.min(queued + SLOW_BOUND));
    assert!(
        after >= earliest_other,
        "a request on a one-second display woke the runtime after {:?}",
        after.saturating_duration_since(queued)
    );

    rig.deliver(Some(rig.display(2, Some(FAST_US))));
    let (_, queued, after) = queue(&rig);
    assert!(
        after <= queued + FAST_BOUND,
        "after a move to a 120 Hz display a request woke the runtime after {:?}",
        after.saturating_duration_since(queued)
    );
}

fn display_change_runs_the_dpi_geometry_barrier() {
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let window: Arc<dyn PlatformWindow> = Arc::new(CountingWindow {
        inner: flui_native::headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless window"),
        geometry_reads: Arc::clone(&reads),
    });
    // The presentation holds its window weakly; the platform keeps it alive.
    let _platform_keeps_the_window = Arc::clone(&window);
    let rig = rig_with(PresentationWindow::new(window, None));
    let count = || reads.load(std::sync::atomic::Ordering::SeqCst);
    let before = count();

    rig.deliver(Some(rig.display(1, Some(8_333))));
    assert!(
        count() > before,
        "a new display did not re-read the gesture geometry"
    );

    let settled = count();
    rig.deliver(Some(rig.display(1, Some(8_333))));
    assert_eq!(
        count(),
        settled,
        "an unchanged display re-read the geometry"
    );
}

#[test]
fn display_timing_contract() {
    crate::table_test::run_table(
        "display_timing_contract",
        &[
            (
                "jank_is_judged_against_the_current_period",
                jank_is_judged_against_the_current_period as fn(),
            ),
            (
                "shared_frame_budget_uses_the_shortest_period",
                shared_frame_budget_uses_the_shortest_period as fn(),
            ),
            (
                "a_hidden_window_does_not_set_the_shared_frame_budget",
                a_hidden_window_does_not_set_the_shared_frame_budget as fn(),
            ),
            (
                "frame_aligned_resampling_window_follows_the_display_period",
                frame_aligned_resampling_window_follows_the_display_period as fn(),
            ),
            (
                "unknown_refresh_period_uses_the_60_hz_fallback",
                unknown_refresh_period_uses_the_60_hz_fallback as fn(),
            ),
            (
                "haptic_frame_deadline_follows_the_display",
                haptic_frame_deadline_follows_the_display as fn(),
            ),
            (
                "display_change_runs_the_dpi_geometry_barrier",
                display_change_runs_the_dpi_geometry_barrier as fn(),
            ),
        ],
    );
}
