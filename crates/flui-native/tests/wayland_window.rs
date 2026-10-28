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
use flui_native::testing::wayland::{
    bridge_focus, compositor_report, fullscreen_on, outputs, portal_parent, present, run_for,
    suspend,
};
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
    /// fractional one it prefers for the surface, else the whole one, else
    /// that of the output the surface
    /// entered, as the display source reads it on its own connection.
    fn scale(&self) -> f64 {
        let Some(report) = compositor_report(self.window.as_ref()) else {
            return 1.0;
        };
        if let Some(scale) = report.fractional_scale.filter(|scale| *scale > 0) {
            return f64::from(scale) / 120.0;
        }
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

/// A window the compositor moves to another output of the same scale names
/// that output's display, its handler hears the change, and its scale stays;
/// moved back, it names the first display again. A client has no other way to
/// put its window on an output than fullscreen on it, so that is how the test
/// moves it. Needs a compositor with two outputs and a seat: Weston on its X11
/// backend with `--output-count=2` (desktop-shell places a fullscreen window on
/// the output it names only when it activates it, which needs a seat). With
/// one output it says it was skipped, unless `FLUI_REQUIRE_TWO_OUTPUTS` is set
/// (the two-output run sets it), where it fails instead.
#[test]
fn a_window_moved_to_another_output_names_its_display() {
    use flui_platform::WindowMode::Normal;

    let case = "a_window_moved_to_another_output_names_its_display";
    let Some(platform) = platform(case) else {
        return;
    };
    let announced = outputs(&platform);
    if announced.len() < 2 {
        assert!(
            std::env::var_os("FLUI_REQUIRE_TWO_OUTPUTS").is_none(),
            "{case}: FLUI_REQUIRE_TWO_OUTPUTS is set and the compositor announces {announced:?}"
        );
        eprintln!("{case}: skipped, the compositor announces one output");
        return;
    }
    let mut fixture = WaylandFixture::open(platform, "moved between outputs");
    let on_output = |fixture: &WaylandFixture| {
        compositor_report(fixture.window.as_ref())
            .expect("a Wayland window")
            .outputs
    };
    let entered = on_output(&fixture);
    let [first_output] = entered[..] else {
        panic!("the window opens on one output: {entered:?}");
    };
    let before = fixture.window.state();
    let first = before.display.expect("the window names its display");
    let other = announced
        .iter()
        .copied()
        .find(|output| *output != first_output)
        .expect("a second output");

    fixture.handler.take_calls();
    assert!(fullscreen_on(
        &fixture.platform,
        fixture.window.as_ref(),
        other
    ));
    fixture.settle();
    assert_eq!(on_output(&fixture), vec![other], "the compositor moved it");
    let moved = fixture.window.state();
    let second = moved.display.expect("the moved window names its display");
    assert_ne!(second, first, "the window names the output it moved to");
    let displays = fixture
        .platform
        .display_snapshot()
        .expect("the displays are read");
    assert!(
        displays.displays().iter().any(|info| info.id == second),
        "{second:?} is a display of the snapshot: {displays:?}"
    );
    assert_eq!(
        moved.metrics.map(WindowMetrics::scale),
        before.metrics.map(WindowMetrics::scale),
        "both outputs have one scale"
    );
    assert!(
        fixture.handler.count(&HandlerCall::StateChanged) > 0,
        "the handler hears the move"
    );

    fixture
        .window
        .admit_and_apply(WindowCommand::SetMode(Normal))
        .expect("the window leaves fullscreen");
    fixture.settle();
    assert_eq!(
        fixture.window.state().display,
        Some(first),
        "back on the first output: {:?}",
        on_output(&fixture)
    );
}

/// A window the compositor suspends (occluded) is given no frame, and gives up
/// no frame callback however long it waits; the frame asked for meanwhile is
/// delivered once the compositor shows it again. Neither Weston 13 nor sway
/// 1.9 sends `suspended` (xdg-shell version 6), so the configure's state is
/// put to the window by `testing::wayland::suspend`, as the compositor's
/// configure would.
#[test]
fn an_occluded_window_is_given_its_frame_once_shown() {
    let case = "an_occluded_window_is_given_its_frame_once_shown";
    let Some(platform) = platform(case) else {
        return;
    };
    let fixture = WaylandFixture::open(platform, "occluded");
    fixture.handler.take_calls();
    assert!(
        suspend(fixture.window.as_ref(), true),
        "a configured window"
    );
    assert!(
        !fixture.window.state().visible,
        "a suspended window is not shown"
    );
    // A present armed its callback and the compositor, not showing the
    // window, never answers it; a frame is asked for behind it.
    fixture.window.surface_target().pre_present_notify();
    fixture.window.request_redraw();
    run_for(&fixture.platform, Duration::from_millis(2500)).expect("the loop turns");
    assert_eq!(
        frames(&fixture.handler),
        0,
        "no frame, and no callback given up, while the window is not shown"
    );
    assert!(suspend(fixture.window.as_ref(), false));
    run_for(&fixture.platform, Duration::from_millis(300)).expect("the loop turns");
    assert_eq!(
        frames(&fixture.handler),
        1,
        "the frame asked for while occluded comes once the window is shown"
    );
}

/// `xdg_toplevel.state`'s `activated`.
const ACTIVATED: u32 = 4;

/// A window tells its accessibility bridge each change of the compositor's
/// activation: activated as it opens, and not once another window takes it.
/// Where the compositor activates no window (a headless Weston has no seat),
/// only the first half runs, and the test says so.
#[test]
fn the_accessibility_bridge_follows_the_compositors_activation() {
    let case = "the_accessibility_bridge_follows_the_compositors_activation";
    let Some(platform) = platform(case) else {
        return;
    };
    let fixture = WaylandFixture::open(platform, "activated first");
    let activated = |window: &dyn HostWindow| {
        compositor_report(window)
            .expect("a Wayland window")
            .states
            .contains(&ACTIVATED)
    };
    let first = fixture.window.as_ref();
    assert_eq!(bridge_focus(first), Some(activated(first)), "as it opened");
    if !activated(first) {
        eprintln!("{case}: the compositor activates no window; the change was not run");
        return;
    }
    let second = fixture
        .platform
        .open_window(
            WindowOptions::default().with_title("activated second"),
            RecordingWindowHandler::new() as _,
        )
        .expect("a second window opens");
    for _ in 0..10 {
        run_for(&fixture.platform, Duration::from_millis(50)).expect("the loop turns");
        second.surface_target().pre_present_notify();
        let _ = present(second.as_ref());
        if activated(second.as_ref()) && !activated(first) {
            break;
        }
    }
    run_for(&fixture.platform, Duration::from_millis(100)).expect("the loop turns");
    assert!(
        activated(second.as_ref()) && !activated(first),
        "the compositor moved its activation to the second window"
    );
    assert_eq!(bridge_focus(second.as_ref()), Some(true));
    assert_eq!(
        bridge_focus(first),
        Some(false),
        "the first window told its bridge it is no longer active"
    );
    second.close_for_cleanup();
    eprintln!("{case}: the activation moved, and both bridges heard it");
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

/// `zxdg_toplevel_decoration_v1.mode`'s server side.
const SERVER_SIDE: u32 = 2;

/// A window's frame is the compositor's answer through `xdg-decoration`, never
/// the window's request: turned off and on, the snapshot says what the
/// compositor said each time, and where the compositor overrules the request
/// (sway draws a frame around every tiled window) the command is no longer
/// offered. A compositor without `xdg-decoration` (Weston) draws no frame,
/// and the window refuses the command. Under sway the test's config floats the
/// window titled "a floating frame", whose request sway honours.
#[test]
fn a_window_has_the_frame_its_compositor_answers() {
    use flui_platform::UnsupportedReason::NotOnThisDevice;

    let case = "a_window_has_the_frame_its_compositor_answers";
    for title in ["a floating frame", "a tiled frame"] {
        let Some(platform) = platform(case) else {
            return;
        };
        let mut fixture = WaylandFixture::open(platform, title);
        let refusal = |fixture: &WaylandFixture, wanted: bool| {
            WindowCommand::SetDecorated(wanted)
                .admit(&fixture.window.state().capabilities)
                .err()
                .and_then(|refused| refused.unsupported().map(|unsupported| unsupported.reason))
        };
        let Some(mode) = compositor_report(fixture.window.as_ref())
            .expect("a Wayland window")
            .frame_mode
        else {
            assert!(
                !fixture.window.state().decorated,
                "{title}: nobody draws a frame"
            );
            assert_eq!(refusal(&fixture, true), Some(NotOnThisDevice), "{title}");
            eprintln!("{case}: {title}: the compositor offers no xdg-decoration");
            continue;
        };
        assert_eq!(
            fixture.window.state().decorated,
            mode == SERVER_SIDE,
            "{title}: the frame the compositor answered the opening with"
        );
        let first = fixture.window.state().decorated;
        for wanted in [!first, first] {
            if let Some(reason) = refusal(&fixture, wanted) {
                assert_eq!(reason, NotOnThisDevice, "{title}");
                eprintln!("{case}: {title}: the compositor decides the frame");
                break;
            }
            fixture
                .window
                .admit_and_apply(WindowCommand::SetDecorated(wanted))
                .expect("an offered command is applied");
            fixture.settle();
            let mode = compositor_report(fixture.window.as_ref())
                .expect("a Wayland window")
                .frame_mode;
            let decorated = fixture.window.state().decorated;
            assert_eq!(
                decorated,
                mode == Some(SERVER_SIDE),
                "{title}: after SetDecorated({wanted}) the snapshot is the compositor's answer"
            );
            if decorated == wanted {
                eprintln!("{case}: {title}: the compositor honoured SetDecorated({wanted})");
            } else {
                assert_eq!(
                    refusal(&fixture, !decorated),
                    Some(NotOnThisDevice),
                    "{title}: an overruled request is no longer offered"
                );
                eprintln!("{case}: {title}: the compositor overruled SetDecorated({wanted})");
                break;
            }
        }
    }
}

/// A window draws its buffer at the fractional scale the compositor prefers
/// for its surface (`wp_fractional_scale_v1`, a numerator over 120): the
/// snapshot's scale is that fraction, and its drawable area the logical size
/// times it, rounded half away from zero as the protocol asks. Where the
/// compositor sends none (Weston), the scale is a whole one.
#[test]
fn a_window_draws_at_the_compositors_fractional_scale() {
    let case = "a_window_draws_at_the_compositors_fractional_scale";
    let Some(platform) = platform(case) else {
        return;
    };
    let fixture = WaylandFixture::open(platform, "a fractional scale");
    let report = compositor_report(fixture.window.as_ref()).expect("a Wayland window");
    let state = fixture.window.state();
    let metrics = state.metrics.expect("a configured window has metrics");
    let Some(numerator) = report.fractional_scale else {
        assert_eq!(metrics.scale().get().fract(), 0.0, "a whole scale");
        eprintln!("{case}: the compositor sends no fractional scale");
        return;
    };
    let scale = f64::from(numerator) / 120.0;
    assert_eq!(metrics.scale().get(), scale, "the compositor's fraction");
    let physical = metrics.physical();
    let expected = |logical: f64| (logical * scale).round();
    assert_eq!(
        (f64::from(physical.width), f64::from(physical.height)),
        (
            expected(state.content_size.width),
            expected(state.content_size.height)
        ),
        "the drawable area is the logical size at {scale}"
    );
    eprintln!("{case}: drawn at {scale} ({numerator}/120): {metrics:?}");
}

/// A second client of the compositor, which reads a window's export apart
/// from the window: it imports the handle as a portal does
/// (`zxdg_importer_v2`), and hears when the compositor revokes it.
mod foreign_client {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
    use wayland_protocols::xdg::foreign::zv2::client::zxdg_imported_v2::{self, ZxdgImportedV2};
    use wayland_protocols::xdg::foreign::zv2::client::zxdg_importer_v2::{self, ZxdgImporterV2};

    #[derive(Default)]
    pub(super) struct Heard {
        destroyed: bool,
    }

    impl Dispatch<WlRegistry, GlobalListContents> for Heard {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZxdgImporterV2, ()> for Heard {
        fn event(
            _: &mut Self,
            _: &ZxdgImporterV2,
            _: zxdg_importer_v2::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZxdgImportedV2, ()> for Heard {
        fn event(
            heard: &mut Self,
            _: &ZxdgImportedV2,
            event: zxdg_imported_v2::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let zxdg_imported_v2::Event::Destroyed = event {
                heard.destroyed = true;
            }
        }
    }

    /// The other client.
    pub(super) struct ForeignClient {
        queue: EventQueue<Heard>,
        heard: Heard,
        /// The compositor announces `zxdg_exporter_v2`.
        pub(super) exporter_offered: bool,
        importer: Option<ZxdgImporterV2>,
        imported: Option<ZxdgImportedV2>,
    }

    impl ForeignClient {
        /// Connect to the compositor `WAYLAND_DISPLAY` names.
        pub(super) fn connect() -> Self {
            let connection = Connection::connect_to_env().expect("the compositor answers");
            let (globals, queue) =
                registry_queue_init::<Heard>(&connection).expect("the registry is read");
            let exporter_offered = globals.contents().with_list(|list| {
                list.iter()
                    .any(|global| global.interface == "zxdg_exporter_v2")
            });
            let importer = globals
                .bind::<ZxdgImporterV2, _, _>(&queue.handle(), 1..=1, ())
                .ok();
            Self {
                queue,
                heard: Heard::default(),
                exporter_offered,
                importer,
                imported: None,
            }
        }

        /// Import `handle` as a portal would, and wait for the compositor.
        pub(super) fn import(&mut self, handle: &str) {
            let importer = self
                .importer
                .as_ref()
                .expect("an importer beside the exporter");
            self.imported =
                Some(importer.import_toplevel(handle.to_owned(), &self.queue.handle(), ()));
            self.queue
                .roundtrip(&mut self.heard)
                .expect("the compositor answers");
        }

        /// Whether the compositor revoked what was imported, as of now.
        pub(super) fn revoked(&mut self) -> bool {
            self.queue
                .roundtrip(&mut self.heard)
                .expect("the compositor answers");
            self.heard.destroyed
        }
    }
}

/// A window the compositor exports (`xdg-foreign` v2) hands out the portal
/// parent `wayland:<handle>`, which another client imports as a valid window,
/// and which names nothing once the window closes: the compositor revokes the
/// import, and the window hands out no parent. Where the compositor offers no
/// exporter (Weston), there is no parent.
#[test]
fn a_portal_parent_names_the_exported_window() {
    let case = "a_portal_parent_names_the_exported_window";
    let Some(platform) = platform(case) else {
        return;
    };
    let mut fixture = WaylandFixture::open(platform, "a portal's parent");
    let mut other = foreign_client::ForeignClient::connect();
    let parent = portal_parent(fixture.window.as_ref());
    if !other.exporter_offered {
        assert_eq!(parent, None, "no export without an exporter");
        eprintln!("{case}: the compositor offers no xdg-foreign");
        return;
    }
    let parent = parent.expect("an exported window names its portal parent");
    let handle = parent
        .strip_prefix("wayland:")
        .unwrap_or_else(|| panic!("a Wayland portal parent: {parent}"));
    other.import(handle);
    assert!(
        !other.revoked(),
        "the compositor knows {handle} as a window"
    );

    fixture.close();
    assert!(other.revoked(), "the closed window's export is revoked");
    assert_eq!(
        portal_parent(fixture.window.as_ref()),
        None,
        "a closed window names no parent"
    );
    eprintln!("{case}: {parent} imported, then revoked with the window");
}
