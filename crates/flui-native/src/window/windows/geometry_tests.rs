//! Where a Win32 window is: the pure conversion of its options into the
//! desktop's device pixels (`open_spot`), the pure reading of its sampled
//! rectangles back into positions on displays (`sampled_geometry`), the
//! display a sample measures on (`window_display`), and real windows.

use std::num::NonZeroIsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, WindowCommand,
    WindowExtent, WindowGeometry, WindowMode, WindowOptions, WindowPosition,
};
use flui_testing::window_commands::AdmitAndApply as _;
use windows::Win32::Foundation::{HWND, RECT};

use super::geometry::{DeviceRects, sampled_geometry, window_display};
use super::set::{OpenSpot, open_spot};
use crate::error::PlatformError;
use crate::owner::notices::StaleOnWake;
use crate::owner::windows::HostDisplays;
use crate::platform::Platform as _;

const FRAME: (i32, i32) = (16, 39);

fn geometry(position: Option<WindowPosition>, size: Size<f64>) -> WindowGeometry {
    WindowGeometry::try_new(position, size).expect("a finite rectangle")
}

fn extent(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(Size::new(width, height)).expect("an extent")
}

fn with_frame(_dpi: u32, content: (i32, i32)) -> (i32, i32) {
    (content.0 + FRAME.0, content.1 + FRAME.1)
}

fn ratio(scale: f64) -> DevicePixelRatio {
    DevicePixelRatio::try_new(scale).expect("a positive ratio")
}

fn on(display: DisplayId, x: f64, y: f64) -> WindowPosition {
    WindowPosition::try_new(display, Point::new(x, y)).expect("a finite point")
}

/// A desktop of 3840x2160 monitors at these scales and device origins, and
/// the ids of its displays in that order.
fn desktop(monitors: &[(f64, (i32, i32))]) -> (DisplaySnapshot, Vec<DisplayId>) {
    let registry = DisplayRegistry::new();
    let displays: Vec<DisplayInfo> = monitors
        .iter()
        .zip(1..)
        .map(|(&(scale, (x, y)), key)| {
            let id = registry.mint(&DisplayKey::Injected(key)).expect("id");
            let bounds = Bounds::new(Point::new(x, y), Size::new(3840, 2160));
            DisplayInfo::from_device(id, "d", bounds, bounds, ratio(scale)).expect("a display")
        })
        .collect();
    let ids = displays.iter().map(|display| display.id).collect();
    let snapshot = DisplaySnapshot::empty()
        .successor(displays)
        .expect("a snapshot");
    (snapshot, ids)
}

/// Where `options` open on `desktop`, with a system scale of `system`.
fn spot(options: &WindowOptions, desktop: &DisplaySnapshot, system: f64) -> OpenSpot {
    open_spot(
        options,
        &options.geometry().expect("valid options"),
        desktop,
        ratio(system),
        with_frame,
    )
}

fn the_size_is_the_content_and_the_frame_is_added() {
    let options = WindowOptions::default().size(extent(800.0, 600.0));
    let spot = spot(&options, &DisplaySnapshot::empty(), 1.0);
    assert_eq!(spot.outer, (816, 639));
    assert_eq!(spot.origin, None);
}

/// A position converts through its own display's origin and scale, and the
/// window is sized at that scale, whatever the system scale is.
fn a_position_converts_through_the_named_displays_own_scale() {
    // A 2x display that starts at device x 1920: 100 logical is 200 device
    // pixels from its origin.
    let (desktop, ids) = desktop(&[(1.0, (0, 0)), (2.0, (1920, 0))]);
    let options = WindowOptions::default()
        .size(extent(800.0, 600.0))
        .position(Some(on(ids[1], 100.0, 50.0)));
    let placed = spot(&options, &desktop, 1.0);
    assert_eq!(placed.scale, 2.0);
    assert_eq!(placed.outer, (1600 + 16, 1200 + 39));
    assert_eq!(placed.origin, Some((1920 + 200, 100)));
    // The same numbers on the 1x display land on its own pixels.
    let options = options.position(Some(on(ids[0], 100.0, 50.0)));
    assert_eq!(spot(&options, &desktop, 1.0).origin, Some((100, 50)));
}

/// A position on a display the snapshot no longer holds is no place: the
/// system places the window, at the system's scale.
fn a_position_on_a_display_that_is_gone_places_nothing() {
    let (desktop, _) = desktop(&[(1.0, (0, 0))]);
    let gone = DisplayRegistry::new()
        .mint(&DisplayKey::Injected(9))
        .expect("id");
    let options = WindowOptions::default().position(Some(on(gone, 100.0, 50.0)));
    let placed = spot(&options, &desktop, 1.5);
    assert_eq!(placed.origin, None);
    assert_eq!(placed.scale, 1.5);
}

fn the_normal_rectangle_is_the_outer_geometry_it_returns_to() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0))]);
    let options = WindowOptions::default()
        .mode(WindowMode::Maximized)
        .normal_geometry(Some(geometry(
            Some(on(ids[0], 100.0, 80.0)),
            Size::new(700.0, 500.0),
        )));
    assert_eq!(
        spot(&options, &desktop, 1.0).normal,
        Some((100, 80, 800, 580))
    );
}

/// A window that opens on one display returns to a rectangle on another: the
/// rectangle converts through its own display, at its own scale.
fn the_normal_rectangle_converts_through_its_own_display() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0)), (2.0, (3840, -200))]);
    let options = WindowOptions::default()
        .position(Some(on(ids[0], 10.0, 10.0)))
        .mode(WindowMode::Maximized)
        .normal_geometry(Some(geometry(
            Some(on(ids[1], 100.0, 80.0)),
            Size::new(700.0, 500.0),
        )));
    assert_eq!(
        spot(&options, &desktop, 1.0).normal,
        Some((4040, -40, 4040 + 1400, -40 + 1000))
    );
}

fn a_normal_rectangle_without_a_position_follows_the_windows_own() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0))]);
    let options = WindowOptions::default()
        .position(Some(on(ids[0], 30.0, 40.0)))
        .normal_geometry(Some(geometry(None, Size::new(300.0, 200.0))));
    assert_eq!(
        spot(&options, &desktop, 1.0).normal,
        Some((30, 40, 330, 240))
    );
    let unplaced =
        WindowOptions::default().normal_geometry(Some(geometry(None, Size::new(300.0, 200.0))));
    assert_eq!(spot(&unplaced, &desktop, 1.0).normal, None);
}

/// A rectangle past what a Win32 coordinate holds is refused, never
/// saturated: a saturated one would land on another side of the desktop.
fn an_opening_rectangle_past_the_range_places_nothing() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0))]);
    let options = WindowOptions::default()
        .mode(WindowMode::Maximized)
        .normal_geometry(Some(geometry(
            Some(on(ids[0], 3.0e9, 3.0e9)),
            Size::new(700.0, 500.0),
        )));
    assert_eq!(spot(&options, &desktop, 1.0).normal, None);
    let wide = WindowOptions::default()
        .position(Some(on(ids[0], 0.0, 0.0)))
        .normal_geometry(Some(geometry(None, Size::new(3.0e9, 500.0))));
    assert_eq!(
        spot(&wide, &desktop, 1.0).normal,
        None,
        "a side past the range"
    );
}

#[test]
fn open_geometry() {
    crate::table_test::run_table(
        "open_geometry",
        &[
            (
                "the_size_is_the_content_and_the_frame_is_added",
                the_size_is_the_content_and_the_frame_is_added as fn(),
            ),
            (
                "a_position_converts_through_the_named_displays_own_scale",
                a_position_converts_through_the_named_displays_own_scale as fn(),
            ),
            (
                "a_position_on_a_display_that_is_gone_places_nothing",
                a_position_on_a_display_that_is_gone_places_nothing as fn(),
            ),
            (
                "the_normal_rectangle_is_the_outer_geometry_it_returns_to",
                the_normal_rectangle_is_the_outer_geometry_it_returns_to as fn(),
            ),
            (
                "the_normal_rectangle_converts_through_its_own_display",
                the_normal_rectangle_converts_through_its_own_display as fn(),
            ),
            (
                "an_opening_rectangle_past_the_range_places_nothing",
                an_opening_rectangle_past_the_range_places_nothing as fn(),
            ),
            (
                "a_normal_rectangle_without_a_position_follows_the_windows_own",
                a_normal_rectangle_without_a_position_follows_the_windows_own as fn(),
            ),
        ],
    );
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

/// A normal window `outer` device pixels large at desktop pixel `at`, with
/// the window's own scale.
fn normal_window(at: (i32, i32), outer: (i32, i32), scale: f64) -> DeviceRects {
    DeviceRects {
        window: Some(rect(at.0, at.1, at.0 + outer.0, at.1 + outer.1)),
        client: Some(rect(0, 0, outer.0 - FRAME.0, outer.1 - FRAME.1)),
        restore: None,
        cached: Size::new(1.0, 1.0),
        minimized: false,
        scale: ratio(scale),
    }
}

/// A sampled window's corner is measured on the display it is on, from that
/// display's origin and at its scale, and its extents over its own scale.
/// Each row reads differently through the conversion that divided the
/// desktop pixel by the window's scale alone: on one monitor at the origin
/// the two agree, which is why only a desktop of several shows the wiring.
fn a_sampled_corner_is_measured_on_its_own_display() {
    // A primary at scale 1, a 2x display right of it, and a 1.5x display left
    // of and above it.
    let (desktop, ids) = desktop(&[(1.0, (0, 0)), (2.0, (3840, 0)), (1.5, (-3840, -400))]);
    // (case, display the window is on, window rectangles, position, outer size)
    let rows = [
        (
            "on a 2x display right of the primary",
            ids[1],
            normal_window((3840 + 200, 100), (1632, 1278), 2.0),
            (100.0, 50.0),
            (816.0, 639.0),
        ),
        (
            "on a 1.5x display at a negative origin",
            ids[2],
            normal_window((-3840 + 300, -400 + 150), (1224, 958), 1.5),
            (200.0, 100.0),
            (816.0, 958.0 / 1.5),
        ),
        (
            "left of its 2x display's origin, still in its pixels",
            ids[1],
            normal_window((3840 - 100, 0), (1632, 1278), 2.0),
            (-50.0, 0.0),
            (816.0, 639.0),
        ),
    ];
    for (case, display, rects, (x, y), (width, height)) in rows {
        let sampled = sampled_geometry(rects, Some(display), &desktop).expect(case);
        assert_eq!(
            sampled.outer.position(),
            Some(on(display, x, y)),
            "{case}: the corner"
        );
        assert_eq!(
            sampled.outer.size(),
            Size::new(width, height),
            "{case}: the outer size"
        );
    }
}

/// The rectangle a window returns to is on the display that holds most of
/// it, measured and sized at that display's scale: a window maximized on the
/// primary returns to 700x500 logical pixels of the 2x display beside it,
/// 1400x1000 device pixels, not to 1400x1000 logical ones.
fn a_sampled_normal_rectangle_is_measured_on_the_display_holding_it() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0)), (2.0, (3840, 0))]);
    let maximized = DeviceRects {
        restore: Some(Bounds::new(
            Point::new(3840 + 400, 200),
            Size::new(1400, 1000),
        )),
        ..normal_window((-8, -8), (3856, 2104), 1.0)
    };
    let sampled = sampled_geometry(maximized, Some(ids[0]), &desktop).expect("a geometry");
    let normal = sampled.normal.expect("the rectangle it returns to");
    assert_eq!(normal.position(), Some(on(ids[1], 200.0, 100.0)));
    assert_eq!(normal.size(), Size::new(700.0, 500.0));
}

/// A window whose display the snapshot lacks has no position, a minimized one
/// reports its last content size with no position, a rectangle across the
/// whole coordinate range is measured exactly rather than saturated, and an
/// inverted one is refused.
fn a_sampled_window_without_a_place_publishes_none() {
    let (desktop, ids) = desktop(&[(1.0, (0, 0))]);
    let gone = DisplayRegistry::new()
        .mint(&DisplayKey::Injected(9))
        .expect("id");
    let window = normal_window((100, 100), (816, 639), 1.0);

    let sampled = sampled_geometry(window, Some(gone), &desktop).expect("a geometry");
    assert_eq!(
        sampled.outer.position(),
        None,
        "a display the snapshot lacks"
    );
    let sampled = sampled_geometry(window, None, &desktop).expect("a geometry");
    assert_eq!(sampled.outer.position(), None, "no display");

    let minimized = DeviceRects {
        window: None,
        minimized: true,
        cached: Size::new(800.0, 600.0),
        ..window
    };
    let sampled = sampled_geometry(minimized, Some(ids[0]), &desktop).expect("a geometry");
    assert_eq!(sampled.outer.position(), None, "a minimized window");
    assert_eq!(sampled.content.get(), Size::new(800.0, 600.0));

    let whole_range = DeviceRects {
        window: Some(rect(i32::MIN, 0, i32::MAX, 10)),
        ..window
    };
    let sampled = sampled_geometry(whole_range, Some(ids[0]), &desktop).expect("a geometry");
    assert_eq!(
        sampled.outer.size().width,
        f64::from(i32::MAX) - f64::from(i32::MIN),
        "the exact width"
    );

    let inverted = DeviceRects {
        window: Some(rect(500, 0, 100, 10)),
        ..window
    };
    assert!(
        sampled_geometry(inverted, Some(ids[0]), &desktop).is_err(),
        "an inverted rectangle"
    );
}

#[test]
fn sampled_geometry_table() {
    crate::table_test::run_table(
        "sampled_geometry",
        &[
            (
                "a_sampled_corner_is_measured_on_its_own_display",
                a_sampled_corner_is_measured_on_its_own_display as fn(),
            ),
            (
                "a_sampled_normal_rectangle_is_measured_on_the_display_holding_it",
                a_sampled_normal_rectangle_is_measured_on_the_display_holding_it as fn(),
            ),
            (
                "a_sampled_window_without_a_place_publishes_none",
                a_sampled_window_without_a_place_publishes_none as fn(),
            ),
        ],
    );
}

/// Monitors whose published snapshot lacks the one a window is on until they
/// are read again, as after a monitor is plugged in; `nameable` says whether
/// a read finds an id for it at all.
struct PluggedMonitor {
    before: DisplaySnapshot,
    after: DisplaySnapshot,
    plugged: DisplayId,
    nameable: bool,
    /// Whether the last miss left the snapshot to read again.
    stale: AtomicBool,
    read: AtomicBool,
    samples: AtomicUsize,
}

impl PluggedMonitor {
    fn new(nameable: bool) -> Self {
        let (before, _) = desktop(&[(1.0, (0, 0))]);
        let (after, ids) = desktop(&[(1.0, (0, 0)), (2.0, (3840, 0))]);
        Self {
            before,
            after,
            plugged: ids[1],
            nameable,
            stale: AtomicBool::new(false),
            read: AtomicBool::new(false),
            samples: AtomicUsize::new(0),
        }
    }
}

impl StaleOnWake for PluggedMonitor {
    fn mark_stale(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }
}

impl HostDisplays for PluggedMonitor {
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        if self.stale.swap(false, Ordering::Relaxed) {
            self.samples.fetch_add(1, Ordering::Relaxed);
            self.read.store(true, Ordering::Relaxed);
        }
        Ok(self.published())
    }

    fn invalidate(&self) {
        self.stale.store(true, Ordering::Relaxed);
    }

    fn published(&self) -> DisplaySnapshot {
        if self.read.load(Ordering::Relaxed) {
            self.after.clone()
        } else {
            self.before.clone()
        }
    }

    /// A miss invalidates the snapshot once per revision, as the Win32
    /// source does: after a read that found nothing new, it costs nothing.
    fn display_of_window(&self, _: HWND) -> Option<DisplayId> {
        if self.read.load(Ordering::Relaxed) && self.nameable {
            return Some(self.plugged);
        }
        if self.samples.load(Ordering::Relaxed) == 0 {
            self.stale.store(true, Ordering::Relaxed);
        }
        None
    }

    fn nearest_monitor(&self, _: HWND) -> Option<NonZeroIsize> {
        NonZeroIsize::new(1)
    }

    fn sample_count(&self) -> usize {
        self.samples.load(Ordering::Relaxed)
    }
}

/// A window on a monitor plugged in since the last read has the monitors read
/// again before its sample, so the sample names the new display and measures
/// on the snapshot that holds it; a monitor no read can name costs one read,
/// and every later sample reads nothing.
fn a_window_on_a_new_monitor_reads_the_monitors_again() {
    let plugged = PluggedMonitor::new(true);
    let (display, desktop) = window_display(&plugged, HWND::default());
    assert_eq!(display, Some(plugged.plugged));
    assert!(desktop.get(plugged.plugged).is_some(), "measured on it");
    assert_eq!(plugged.sample_count(), 1);
    let _ = window_display(&plugged, HWND::default());
    assert_eq!(plugged.sample_count(), 1, "a found display reads nothing");

    let unnamed = PluggedMonitor::new(false);
    assert_eq!(window_display(&unnamed, HWND::default()).0, None);
    assert_eq!(window_display(&unnamed, HWND::default()).0, None);
    assert_eq!(unnamed.sample_count(), 1, "one read, not one per sample");
}

#[test]
fn window_display_table() {
    crate::table_test::run_table(
        "window_display",
        &[(
            "a_window_on_a_new_monitor_reads_the_monitors_again",
            a_window_on_a_new_monitor_reads_the_monitors_again as fn(),
        )],
    );
}

fn near(actual: f64, wanted: f64) -> bool {
    (actual - wanted).abs() <= 2.0
}

/// The primary display of `platform`, which a real window opens on.
fn primary(platform: &crate::WindowsPlatform) -> DisplayId {
    let snapshot = platform.display_snapshot().expect("a display snapshot");
    snapshot
        .primary()
        .or_else(|| snapshot.displays().first())
        .map(|display| display.id)
        .expect("the desktop has a display")
}

/// A real window opened with a content size reports that content size: the
/// frame is added to the outer window, not taken out of the content.
fn a_real_window_opens_with_the_content_size_it_asked_for() {
    let platform = crate::WindowsPlatform::try_new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default()
                .size(extent(640.0, 480.0))
                .visible(false),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window");
    let state = window.state();
    assert!(
        near(state.content_size.width(), 640.0) && near(state.content_size.height(), 480.0),
        "content {:?}",
        state.content_size
    );
    assert!(
        state.outer.size().width > state.content_size.width()
            && state.outer.size().height > state.content_size.height(),
        "the outer window carries a frame"
    );
}

/// A size set on a maximized window is the size it returns to: the window
/// stays maximized and restores to the content size asked for.
fn a_size_set_while_maximized_is_the_size_restored_to() {
    let platform = crate::WindowsPlatform::try_new().expect("platform");
    let window = platform
        .open_window(
            WindowOptions::default().visible(false),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window");
    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
        .expect("maximize");
    window
        .admit_and_apply(WindowCommand::SetSize(extent(500.0, 400.0)))
        .expect("resize while maximized");
    assert_eq!(
        window.state().mode,
        WindowMode::Maximized,
        "still maximized"
    );
    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
        .expect("restore");
    let state = window.state();
    assert!(
        near(state.content_size.width(), 500.0) && near(state.content_size.height(), 400.0),
        "restored to {:?}",
        state.content_size
    );
}

/// A window that opens maximized returns to the rectangle it was given, on
/// the display it names.
fn a_window_opened_maximized_returns_to_its_normal_rectangle() {
    let platform = crate::WindowsPlatform::try_new().expect("platform");
    let display = primary(&platform);
    let window = platform
        .open_window(
            WindowOptions::default()
                .mode(WindowMode::Maximized)
                .normal_geometry(Some(geometry(
                    Some(on(display, 120.0, 100.0)),
                    Size::new(700.0, 500.0),
                ))),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window");
    assert_eq!(window.state().mode, WindowMode::Maximized);
    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
        .expect("restore");
    let outer = window.state().outer;
    assert!(
        near(outer.size().width, 700.0) && near(outer.size().height, 500.0),
        "returned to {outer:?}"
    );
    let at = outer
        .position()
        .expect("a Win32 window reports its position");
    assert_eq!(at.display(), display, "on the display it named");
    assert!(
        near(at.at().x, 120.0) && near(at.at().y, 100.0),
        "at {at:?}"
    );
}

/// Maximized and fullscreen windows both report the rectangle they return
/// to, taken from Win32's own `rcNormalPosition`: entering fullscreen from a
/// maximized window does not make the maximized rectangle the one to return
/// to.
fn maximized_and_fullscreen_windows_report_their_normal_rectangle() {
    let platform = crate::WindowsPlatform::try_new().expect("platform");
    let display = primary(&platform);
    let window = platform
        .open_window(
            WindowOptions::default()
                .size(extent(640.0, 480.0))
                .position(Some(on(display, 150.0, 120.0)))
                .visible(false),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window");
    let normal = window.state().outer;
    let same_as_normal = |state: flui_platform::WindowState, what: &str| {
        let reported = state
            .normal_geometry
            .unwrap_or_else(|| panic!("{what} reports the rectangle it returns to"));
        assert!(
            near(reported.size().width, normal.size().width)
                && near(reported.size().height, normal.size().height),
            "{what}: {reported:?} is not {normal:?}"
        );
        let (at, was) = (
            reported.position().expect("a position"),
            normal.position().expect("a position"),
        );
        assert_eq!(at.display(), was.display(), "{what}: on its display");
        assert!(
            near(at.at().x, was.at().x) && near(at.at().y, was.at().y),
            "{what}: at {at:?}, was {was:?}"
        );
    };

    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
        .expect("maximize");
    same_as_normal(window.state(), "a maximized window");
    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Fullscreen))
        .expect("fullscreen");
    same_as_normal(window.state(), "a fullscreen window entered from maximized");
}

#[test]
fn open_real_window() {
    crate::table_test::run_table(
        "open_real_window",
        &[
            (
                "maximized_and_fullscreen_windows_report_their_normal_rectangle",
                maximized_and_fullscreen_windows_report_their_normal_rectangle as fn(),
            ),
            (
                "a_real_window_opens_with_the_content_size_it_asked_for",
                a_real_window_opens_with_the_content_size_it_asked_for as fn(),
            ),
            (
                "a_size_set_while_maximized_is_the_size_restored_to",
                a_size_set_while_maximized_is_the_size_restored_to as fn(),
            ),
            (
                "a_window_opened_maximized_returns_to_its_normal_rectangle",
                a_window_opened_maximized_returns_to_its_normal_rectangle as fn(),
            ),
        ],
    );
}
