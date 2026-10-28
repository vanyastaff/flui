//! The native Wayland host against a real compositor: its window against the
//! window conformance checks of `flui-testing`, with what the compositor said
//! in its configures as the readback, and its loop's wakes.
//!
//! The cases need a compositor: they run where `WAYLAND_DISPLAY` names one (a
//! headless Weston in a test session) and say they were skipped elsewhere.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Size, WindowMetrics};
use flui_native::testing::app::{self, AppCall};
use flui_native::testing::wayland::{compositor_report, present, run_for};
use flui_native::{ExitAnswer, HostWindow, Platform as _, WaylandPlatform};
use flui_platform::{Availability, PlatformWindow, WindowCommand, WindowOptions};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_conformance::{self, Readback, WindowFixture};
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

/// The platform on the compositor `WAYLAND_DISPLAY` names, or `None` where
/// there is none to run against. Where `FLUI_REQUIRE_WAYLAND` is set (CI
/// starts a headless Weston and sets it), a missing compositor fails the case
/// instead of skipping it.
fn platform(case: &str) -> Option<WaylandPlatform> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|name| name.is_empty()) {
        assert!(
            std::env::var_os("FLUI_REQUIRE_WAYLAND").is_none(),
            "{case}: FLUI_REQUIRE_WAYLAND is set and WAYLAND_DISPLAY names no compositor"
        );
        eprintln!("{case}: skipped, WAYLAND_DISPLAY names no compositor");
        return None;
    }
    Some(WaylandPlatform::new().expect("the compositor WAYLAND_DISPLAY names answers"))
}

/// A Wayland window for the conformance checks. Dropping it closes the window
/// and settles, so a case that fails part way leaves no window behind.
struct WaylandFixture {
    platform: WaylandPlatform,
    window: Rc<dyn HostWindow>,
    handler: Rc<RecordingWindowHandler>,
    /// How many of the frames the window asked for were drawn.
    drawn: Cell<usize>,
}

impl WaylandFixture {
    fn open(platform: WaylandPlatform, title: &str) -> Self {
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(
                WindowOptions::default().with_title(title),
                Rc::clone(&handler) as _,
            )
            .expect("a Wayland window opens");
        let mut fixture = Self {
            platform,
            window,
            handler,
            drawn: Cell::new(0),
        };
        fixture.settle();
        fixture
    }

    /// Present a frame for each the window asked for since the last look, as
    /// a renderer would: the raster side's notice, then the commit.
    fn draw_requested_frames(&self) {
        let asked = self
            .handler
            .calls()
            .iter()
            .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
            .count();
        if asked > self.drawn.get() {
            self.drawn.set(asked);
            self.window.surface_target().pre_present_notify();
            let _ = present(self.window.as_ref());
        }
    }

    /// The scale the compositor asks the window's buffer to be drawn at: the
    /// one it prefers for the surface, else that of the output the surface
    /// entered, as the display source reads it on its own connection.
    fn scale(&self) -> f64 {
        let Some(report) = compositor_report(self.window.as_ref()) else {
            return 1.0;
        };
        if let Some(scale) = report.preferred_scale {
            return f64::from(scale);
        }
        let display = self.window.state().display;
        self.platform
            .display_snapshot()
            .ok()
            .and_then(|snapshot| {
                snapshot
                    .displays()
                    .iter()
                    .find(|info| Some(info.id) == display)
                    .map(|info| info.scale_factor)
            })
            .unwrap_or(1.0)
    }
}

impl WindowFixture for WaylandFixture {
    fn window(&self) -> Rc<dyn PlatformWindow> {
        Rc::clone(&self.window) as Rc<dyn PlatformWindow>
    }

    /// What the compositor sent, read here by xdg-shell's own numbering and
    /// never through the window's decoding: the size where it sized the window
    /// itself (maximized or fullscreen), the activation, and whether it shows
    /// the window at all (configured, on an output, not suspended). A floating
    /// window's size is its client's, which the compositor does not report.
    fn readback(&self) -> Readback {
        /// `xdg_toplevel.state` values, as xdg-shell numbers them.
        const MAXIMIZED: u32 = 1;
        const FULLSCREEN: u32 = 2;
        const ACTIVATED: u32 = 4;
        const SUSPENDED: u32 = 9;

        let report =
            compositor_report(self.window.as_ref()).expect("the window is a Wayland window");
        let has = |state: u32| report.states.contains(&state);
        let (width, height) = report.size;
        let metrics = (width > 0 && height > 0 && (has(MAXIMIZED) || has(FULLSCREEN)))
            .then(|| {
                WindowMetrics::from_logical(
                    Size::new(f64::from(width), f64::from(height)),
                    self.scale(),
                )
                .ok()
            })
            .flatten();
        Readback {
            metrics,
            focused: has(ACTIVATED),
            active: has(ACTIVATED),
            visible: report.configures > 0 && !report.outputs.is_empty() && !has(SUSPENDED),
            material: None,
        }
    }

    /// Run the loop until the snapshot stops moving, drawing each frame the
    /// window asks for the way its renderer would.
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut last = self.window.state().revision;
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline {
            run_for(&self.platform, Duration::from_millis(25)).expect("the loop turns");
            self.draw_requested_frames();
            let now = self.window.state().revision;
            if now == last {
                if quiet_since.elapsed() >= Duration::from_millis(250) {
                    return;
                }
            } else {
                last = now;
                quiet_since = Instant::now();
            }
        }
    }

    /// A Wayland window takes `WindowCommand::Close`, which the checks use; the
    /// compositor's own close has no lever a test can pull.
    fn close(&mut self) {
        self.window.close_for_cleanup();
        self.settle();
    }

    /// The Wayland window hands the compositor no input region.
    fn input_regions(&self) -> Availability {
        Availability::Unavailable
    }

    fn has_native_surface(&self) -> bool {
        true
    }
}

impl Drop for WaylandFixture {
    fn drop(&mut self) {
        self.window.close_for_cleanup();
        self.settle();
    }
}

/// The input conformance table on a real Wayland window's capability
/// snapshot (`backend_conformance::wayland_window_conforms`).
#[test]
fn the_wayland_window_input_conforms() {
    super::backend_conformance::wayland_window_conforms();
}

#[test]
fn the_wayland_window_conforms() {
    let Some(platform) = platform("the_wayland_window_conforms") else {
        return;
    };
    let mut fixture = WaylandFixture::open(platform, "window conformance");
    window_conformance::assert_conforms(&mut fixture);
}

/// A compositor that maximized a window before it went fullscreen (Weston's
/// desktop shell) maximizes it again as it leaves fullscreen: asked for normal
/// mode, the window leaves that too, and the compositor's own configure says
/// so.
#[test]
fn a_window_maximized_before_fullscreen_returns_to_normal() {
    use flui_platform::WindowMode::{Fullscreen, Maximized, Normal};

    let Some(platform) = platform("a_window_maximized_before_fullscreen_returns_to_normal") else {
        return;
    };
    let mut fixture = WaylandFixture::open(platform, "maximized under fullscreen");
    for mode in [Maximized, Fullscreen, Normal] {
        fixture
            .window
            .admit_and_apply(WindowCommand::SetMode(mode))
            .unwrap_or_else(|refusal| panic!("SetMode({mode:?}) was refused: {refusal:?}"));
        fixture.settle();
        assert_eq!(fixture.window.state().mode, mode, "after SetMode({mode:?})");
    }
    let report = compositor_report(fixture.window.as_ref()).expect("a Wayland window");
    // xdg-shell's `maximized` (1) and `fullscreen` (2).
    assert!(
        !report.states.contains(&1) && !report.states.contains(&2),
        "the compositor still sizes the window: {report:?}"
    );
}

fn frames(handler: &RecordingWindowHandler) -> usize {
    handler
        .take_calls()
        .iter()
        .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
        .count()
}

/// A settled window asks for no frame of its own; a redraw asked for on
/// another thread wakes the loop and reaches the window's handler.
#[test]
fn a_redraw_from_another_thread_reaches_the_window() {
    let Some(platform) = platform("a_redraw_from_another_thread_reaches_the_window") else {
        return;
    };
    let fixture = WaylandFixture::open(platform, "redraw from another thread");
    fixture.handler.take_calls();
    run_for(&fixture.platform, Duration::from_millis(300)).expect("the loop turns");
    assert_eq!(
        frames(&fixture.handler),
        0,
        "a settled window asks for no frame"
    );

    // Asked while the loop blocks with nothing to do: only the wake reaches it
    // before the run ends.
    let requester = fixture.window.redraw_requester();
    let asking = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        requester.request_redraw();
    });
    run_for(&fixture.platform, Duration::from_millis(400)).expect("the loop turns");
    asking.join().expect("the asking thread");
    assert_eq!(
        frames(&fixture.handler),
        1,
        "the redraw asked for on another thread is one frame"
    );
}

/// A present that is skipped (an outdated surface, a zero size) commits
/// nothing, so the frame callback it armed never comes: the next frame waits
/// for the callback's grace, not for good.
#[test]
fn a_frame_callback_no_commit_carried_is_given_up() {
    let Some(platform) = platform("a_frame_callback_no_commit_carried_is_given_up") else {
        return;
    };
    let fixture = WaylandFixture::open(platform, "a skipped present");
    fixture.handler.take_calls();
    // The raster side armed the callback and then presented nothing.
    fixture.window.surface_target().pre_present_notify();
    fixture.window.request_redraw();
    run_for(&fixture.platform, Duration::from_millis(300)).expect("the loop turns");
    assert_eq!(
        frames(&fixture.handler),
        0,
        "the frame waits on the callback first"
    );
    run_for(&fixture.platform, Duration::from_millis(1200)).expect("the loop turns");
    assert_eq!(
        frames(&fixture.handler),
        1,
        "the frame is delivered once the lost callback is given up"
    );
}

/// A run bootstraps, opens a window, and ends with its last window when the
/// application lets it; the application hears the quit once.
#[test]
fn a_wayland_run_ends_with_its_last_window() {
    let Some(platform) = platform("a_wayland_run_ends_with_its_last_window") else {
        return;
    };
    let window_handler = RecordingWindowHandler::new();
    let opened = Rc::clone(&window_handler);
    let log = app::AppLog::new();
    log.answer_exit_policy(ExitAnswer::End);
    log.bootstrap(move |owner| {
        let window = owner
            .open_window(WindowOptions::default().with_title("a run"), opened as _)
            .expect("a window opens in the bootstrap")
            .try_ready()
            .expect("at once");
        window
            .admit_and_apply(WindowCommand::Close)
            .expect("the window closes");
        Ok(())
    });
    Box::new(platform)
        .run(Rc::clone(&log) as _)
        .expect("the run ends");
    assert_eq!(log.count(AppCall::Ready), 1);
    assert_eq!(
        log.count(AppCall::ExitPolicy),
        1,
        "asked once, at the last close"
    );
    assert_eq!(log.count(AppCall::Quit), 1, "the quit is heard once");
    assert_eq!(window_handler.count(&HandlerCall::Closed), 1);
}

/// A wake deadline that came due is offered to the windows once: a window
/// that produces no frame for it does not turn the loop into a busy one, and
/// the loop waits for the next wake.
#[test]
fn a_due_deadline_no_frame_serves_leaves_the_loop_idle() {
    let Some(platform) = platform("a_due_deadline_no_frame_serves_leaves_the_loop_idle") else {
        return;
    };
    let log = app::AppLog::new();
    let due = Instant::now();
    log.answer_deadline(Some(due));
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    log.bootstrap(move |owner| {
        let window = owner
            .open_window(
                WindowOptions::default().with_title("a due deadline"),
                RecordingWindowHandler::new() as _,
            )
            .expect("a window opens")
            .try_ready()
            .expect("at once");
        // The loop holds the window until it closes; the run ends on the quit
        // below, which releases it.
        drop(window);
        let proxy = owner.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            stopped.store(true, Ordering::SeqCst);
            let _ = proxy.request_quit();
        });
        Ok(())
    });
    Box::new(platform)
        .run(Rc::clone(&log) as _)
        .expect("the run ends");
    assert!(stop.load(Ordering::SeqCst), "the run ended on the quit");
    let asked = log.count(AppCall::WakeDeadline);
    assert!(
        asked < 20,
        "the loop asked for its deadline {asked} times in half a second: it spun"
    );
}
