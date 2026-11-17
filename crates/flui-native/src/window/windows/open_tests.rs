//! Where a Win32 window opens: the pure conversion of its options into
//! physical pixels (`open_spot`), and real windows that open there.

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, WindowCommand,
    WindowExtent, WindowGeometry, WindowMode, WindowOptions, WindowPosition,
};
use flui_testing::window_commands::AdmitAndApply as _;

use super::set::{OpenSpot, open_spot};
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
