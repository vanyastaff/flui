//! `WindowPlacement`: the saved bytes and the fit of a remembered window to the
//! displays of the next run.

use std::sync::LazyLock;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{
    Availability, DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot,
    InvalidGeometry, PlacementError, PlacementOffset, ResolvedPlacement, WindowCapabilities,
    WindowCapability, WindowExtent, WindowGeometry, WindowMode, WindowPlacement, WindowPosition,
    WindowState,
};

/// Two device pixels per logical pixel.
fn two() -> DevicePixelRatio {
    scale(2.0)
}

fn scale(ratio: f64) -> DevicePixelRatio {
    DevicePixelRatio::try_new(ratio).expect("a ratio a display has")
}

fn ext(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(Size::new(width, height)).expect("an extent")
}

/// A display of `width` by `height` device pixels at `origin`, whose windows
/// may use `usable`.
fn device_display(
    id: DisplayId,
    display: Bounds<i32>,
    usable: Bounds<i32>,
    ratio: DevicePixelRatio,
) -> DisplayInfo {
    DisplayInfo::from_device(id, "display", display, usable, ratio).expect("a display")
}

fn bounds(x: i32, y: i32, width: i32, height: i32) -> Bounds<i32> {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}

struct Desk {
    displays: DisplaySnapshot,
    left: DisplayId,
    right: DisplayId,
}

static DESK: LazyLock<Desk> = LazyLock::new(build_desk);

/// A 1x primary display with a 40 px task bar, and a 2x display to its right.
fn build_desk() -> Desk {
    let registry = DisplayRegistry::new();
    let left = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let right = registry.mint(&DisplayKey::Injected(2)).expect("id");
    let primary = device_display(
        left,
        bounds(0, 0, 1920, 1080),
        bounds(0, 0, 1920, 1040),
        DevicePixelRatio::ONE,
    )
    .with_primary(true)
    .with_persistent_key("key-left");
    let hidpi = device_display(
        right,
        bounds(1920, 0, 3840, 2160),
        bounds(1920, 0, 3840, 2160),
        two(),
    )
    .with_persistent_key("key-right");
    Desk {
        displays: DisplaySnapshot::empty()
            .successor(vec![primary, hidpi])
            .expect("revision"),
        left,
        right,
    }
}

const NO_FRAME: Size<f64> = Size::new(0.0, 0.0);
const FRAME: Size<f64> = Size::new(16.0, 39.0);

fn placement(display: Option<&str>, offset: (f64, f64), size: (f64, f64)) -> WindowPlacement {
    WindowPlacement::new(
        display.map(str::to_owned),
        Some(PlacementOffset::try_new(Point::new(offset.0, offset.1)).expect("a finite offset")),
        ext(size.0, size.1),
        WindowMode::Normal,
        DevicePixelRatio::ONE,
    )
}

fn resolved_with(placement: &WindowPlacement, frame: Size<f64>) -> ResolvedPlacement {
    placement
        .resolve(&DESK.displays, &WindowCapabilities::ALL, frame)
        .expect("a display holds it")
}

fn resolved(placement: &WindowPlacement) -> ResolvedPlacement {
    resolved_with(placement, NO_FRAME)
}

fn the_display_is_found_by_its_key() {
    let on_right = resolved(&placement(Some("key-right"), (100.0, 50.0), (800.0, 600.0)));
    assert_eq!(on_right.display, DESK.right);
    assert_eq!(
        on_right.position().map(WindowPosition::display),
        Some(DESK.right),
        "the position is on the display it resolved to"
    );
    // Relative to that display, whatever the other displays' scales are.
    assert_eq!(
        on_right.position().map(WindowPosition::at),
        Some(Point::new(100.0, 50.0))
    );
    assert_eq!(on_right.size, Size::new(800.0, 600.0));
    let on_left = resolved(&placement(Some("key-left"), (100.0, 50.0), (800.0, 600.0)));
    assert_eq!(on_left.display, DESK.left);
    assert_eq!(
        on_left.position().map(WindowPosition::at),
        Some(Point::new(100.0, 50.0))
    );
}

fn a_missing_display_falls_back_to_the_primary() {
    let gone = resolved(&placement(
        Some("key-unplugged"),
        (100.0, 50.0),
        (800.0, 600.0),
    ));
    assert_eq!(gone.display, DESK.left);
    assert_eq!(
        gone.position().map(WindowPosition::at),
        Some(Point::new(100.0, 50.0))
    );
}

fn a_window_without_a_key_opens_on_the_primary() {
    let keyless = resolved(&placement(None, (10.0, 20.0), (800.0, 600.0)));
    assert_eq!(keyless.display, DESK.left);
    assert_eq!(
        keyless.position().map(WindowPosition::at),
        Some(Point::new(10.0, 20.0))
    );
}

fn no_displays_resolves_to_nothing() {
    let none = placement(Some("key-left"), (10.0, 20.0), (800.0, 600.0)).resolve(
        &DisplaySnapshot::empty(),
        &WindowCapabilities::ALL,
        NO_FRAME,
    );
    assert_eq!(none, None);
}

fn a_window_wider_than_the_work_area_is_cut_to_it() {
    let wide = resolved(&placement(Some("key-left"), (300.0, 0.0), (5000.0, 600.0)));
    assert_eq!(wide.size, Size::new(1920.0, 600.0));
    assert_eq!(
        wide.position().map(WindowPosition::at),
        Some(Point::new(0.0, 0.0))
    );
    let tall = resolved(&placement(Some("key-left"), (0.0, 0.0), (800.0, 2000.0)));
    // The usable area excludes the task bar.
    assert_eq!(tall.size, Size::new(800.0, 1040.0));
}

fn the_outer_size_with_its_frame_is_what_fits() {
    let wide = resolved_with(
        &placement(Some("key-left"), (300.0, 0.0), (5000.0, 600.0)),
        FRAME,
    );
    assert_eq!(
        wide.size,
        Size::new(1904.0, 600.0),
        "1920 outer is 1904 inside"
    );
    assert_eq!(
        wide.position().map(WindowPosition::at),
        Some(Point::new(0.0, 0.0))
    );
    let tall = resolved_with(
        &placement(Some("key-left"), (0.0, 0.0), (800.0, 2000.0)),
        FRAME,
    );
    assert_eq!(
        tall.size,
        Size::new(800.0, 1001.0),
        "1040 outer is 1001 inside"
    );
    let near_edge = resolved_with(
        &placement(Some("key-left"), (1800.0, 1000.0), (800.0, 600.0)),
        FRAME,
    );
    // The 816 x 639 outer window ends exactly at the work area's edge.
    assert_eq!(
        near_edge.position().map(WindowPosition::at),
        Some(Point::new(1104.0, 401.0))
    );
}

fn a_window_off_the_edge_is_shifted_back_onto_the_display() {
    let past = resolved(&placement(
        Some("key-left"),
        (1800.0, 1000.0),
        (800.0, 600.0),
    ));
    assert_eq!(
        past.position().map(WindowPosition::at),
        Some(Point::new(1120.0, 440.0))
    );
    let before = resolved(&placement(
        Some("key-left"),
        (-500.0, -90.0),
        (800.0, 600.0),
    ));
    assert_eq!(
        before.position().map(WindowPosition::at),
        Some(Point::new(0.0, 0.0))
    );
    let right = resolved(&placement(Some("key-right"), (5000.0, 0.0), (800.0, 600.0)));
    // The right display is 1920 logical pixels wide in its own scale.
    assert_eq!(
        right.position().map(WindowPosition::at),
        Some(Point::new(1120.0, 0.0))
    );
}

fn a_window_moved_to_another_scale_keeps_its_physical_size() {
    let moved = WindowPlacement::new(
        Some("key-unplugged".into()),
        Some(PlacementOffset::try_new(Point::new(0.0, 0.0)).expect("a finite offset")),
        ext(400.0, 300.0),
        WindowMode::Normal,
        scale(2.0),
    );
    // It was 2x on a display that is gone; the primary is 1x.
    assert_eq!(resolved(&moved).size, Size::new(800.0, 600.0));
    let same = WindowPlacement::new(
        Some("key-right".into()),
        Some(PlacementOffset::try_new(Point::new(0.0, 0.0)).expect("a finite offset")),
        ext(400.0, 300.0),
        WindowMode::Normal,
        scale(2.0),
    );
    assert_eq!(resolved(&same).size, Size::new(400.0, 300.0));
}

fn a_minimized_window_opens_normal_or_maximized() {
    let minimized = WindowPlacement::new(
        None,
        Some(PlacementOffset::try_new(Point::new(0.0, 0.0)).expect("a finite offset")),
        ext(800.0, 600.0),
        WindowMode::Minimized,
        DevicePixelRatio::ONE,
    );
    assert_eq!(resolved(&minimized).mode, WindowMode::Normal);
    let was_maximized = minimized.with_maximized_before_minimize(true);
    assert_eq!(resolved(&was_maximized).mode, WindowMode::Maximized);
}

fn a_mode_the_window_cannot_enter_becomes_normal() {
    let full = WindowPlacement::new(
        None,
        Some(PlacementOffset::try_new(Point::new(0.0, 0.0)).expect("a finite offset")),
        ext(800.0, 600.0),
        WindowMode::Fullscreen,
        DevicePixelRatio::ONE,
    );
    assert_eq!(resolved(&full).mode, WindowMode::Fullscreen);
    let mut capabilities = WindowCapabilities::ALL.into_parts();
    capabilities.enters_fullscreen = WindowCapability::NOT_ON_THIS_PLATFORM;
    let capabilities = capabilities.into_capabilities();
    let result = full
        .resolve(&DESK.displays, &capabilities, NO_FRAME)
        .expect("resolves");
    assert_eq!(result.mode, WindowMode::Normal);
}

fn without_places_at_open_size_and_mode_stay_but_position_goes() {
    let mut capabilities = WindowCapabilities::ALL.into_parts();
    capabilities.places_at_open = Availability::Unavailable;
    let capabilities = capabilities.into_capabilities();
    let maximized = WindowPlacement::new(
        Some("key-left".into()),
        Some(PlacementOffset::try_new(Point::new(100.0, 100.0)).expect("a finite offset")),
        ext(800.0, 600.0),
        WindowMode::Maximized,
        DevicePixelRatio::ONE,
    );
    let result = maximized
        .resolve(&DESK.displays, &capabilities, NO_FRAME)
        .expect("resolves");
    assert_eq!(result.position(), None);
    assert_eq!(result.size, Size::new(800.0, 600.0));
    assert_eq!(result.mode, WindowMode::Maximized);
}

fn a_placement_without_a_position_stays_unpositioned() {
    let unplaced = WindowPlacement::new(
        Some("key-right".into()),
        None,
        ext(800.0, 600.0),
        WindowMode::Normal,
        scale(2.0),
    );
    let result = resolved(&unplaced);
    assert_eq!(result.display, DESK.right);
    assert_eq!(result.position(), None);
}

fn displays_that_cannot_hold_a_window_are_not_candidates() {
    let registry = DisplayRegistry::new();
    let make = |number, usable: Bounds<i32>, primary| {
        device_display(
            registry.mint(&DisplayKey::Injected(number)).expect("id"),
            bounds(0, 0, 1920, 1080),
            usable,
            DevicePixelRatio::ONE,
        )
        .with_primary(primary)
        .with_persistent_key(format!("key-{number}"))
    };
    let snapshot = DisplaySnapshot::empty()
        .successor(vec![
            // The primary has no usable area, nor the next.
            make(1, bounds(0, 0, 1920, 0), true),
            make(2, bounds(0, 0, 0, 1080), false),
            // No usable area at all.
            make(3, bounds(0, 0, 0, 0), false),
            make(4, bounds(0, 0, 1280, 720), false),
        ])
        .expect("revision");
    let result = placement(Some("key-3"), (0.0, 0.0), (800.0, 600.0))
        .resolve(&snapshot, &WindowCapabilities::ALL, NO_FRAME)
        .expect("the one usable display holds it");
    let expected = snapshot.displays()[3].id;
    assert_eq!(result.display, expected);

    let only_broken = DisplaySnapshot::empty()
        .successor(vec![make(5, bounds(0, 0, 0, 0), true)])
        .expect("revision");
    assert_eq!(
        placement(None, (0.0, 0.0), (800.0, 600.0)).resolve(
            &only_broken,
            &WindowCapabilities::ALL,
            NO_FRAME
        ),
        None
    );
}

fn a_placement_that_describes_no_window_resolves_to_nothing() {
    // A size or offset that is not finite and a scale no display has cannot
    // be held at all ([`WindowExtent`], [`PlacementOffset`],
    // [`DevicePixelRatio`]); what is left to refuse is a size with no area.
    let empty = placement(None, (0.0, 0.0), (800.0, 0.0));
    assert_eq!(
        empty.resolve(&DESK.displays, &WindowCapabilities::ALL, NO_FRAME),
        None
    );
    for (x, y) in [(f64::INFINITY, 0.0), (0.0, f64::NAN)] {
        assert!(
            matches!(
                PlacementOffset::try_new(Point::new(x, y)),
                Err(InvalidGeometry::Position { .. })
            ),
            "({x}, {y})"
        );
    }
}

#[test]
fn placement_resolve() {
    crate::run_table(
        "placement_resolve",
        &[
            (
                "the_display_is_found_by_its_key",
                the_display_is_found_by_its_key,
            ),
            (
                "a_missing_display_falls_back_to_the_primary",
                a_missing_display_falls_back_to_the_primary,
            ),
            (
                "a_window_without_a_key_opens_on_the_primary",
                a_window_without_a_key_opens_on_the_primary,
            ),
            (
                "no_displays_resolves_to_nothing",
                no_displays_resolves_to_nothing,
            ),
            (
                "a_window_wider_than_the_work_area_is_cut_to_it",
                a_window_wider_than_the_work_area_is_cut_to_it,
            ),
            (
                "the_outer_size_with_its_frame_is_what_fits",
                the_outer_size_with_its_frame_is_what_fits,
            ),
            (
                "a_window_off_the_edge_is_shifted_back_onto_the_display",
                a_window_off_the_edge_is_shifted_back_onto_the_display,
            ),
            (
                "a_window_moved_to_another_scale_keeps_its_physical_size",
                a_window_moved_to_another_scale_keeps_its_physical_size,
            ),
            (
                "a_minimized_window_opens_normal_or_maximized",
                a_minimized_window_opens_normal_or_maximized,
            ),
            (
                "a_mode_the_window_cannot_enter_becomes_normal",
                a_mode_the_window_cannot_enter_becomes_normal,
            ),
            (
                "without_places_at_open_size_and_mode_stay_but_position_goes",
                without_places_at_open_size_and_mode_stay_but_position_goes,
            ),
            (
                "a_placement_without_a_position_stays_unpositioned",
                a_placement_without_a_position_stays_unpositioned,
            ),
            (
                "displays_that_cannot_hold_a_window_are_not_candidates",
                displays_that_cannot_hold_a_window_are_not_candidates,
            ),
            (
                "a_placement_that_describes_no_window_resolves_to_nothing",
                a_placement_that_describes_no_window_resolves_to_nothing,
            ),
        ],
    );
}

fn sample() -> WindowPlacement {
    WindowPlacement::new(
        Some("monitor 100%\nsecond".into()),
        Some(PlacementOffset::try_new(Point::new(-120.5, 80.0)).expect("a finite offset")),
        ext(800.25, 600.0),
        WindowMode::Minimized,
        scale(1.5),
    )
    .with_maximized_before_minimize(true)
}

fn a_placement_survives_the_bytes() {
    assert_eq!(
        WindowPlacement::from_bytes(&sample().to_bytes()),
        Ok(sample())
    );
    let keyless = WindowPlacement::new(
        None,
        None,
        ext(1.0, 1.0),
        WindowMode::Normal,
        DevicePixelRatio::ONE,
    );
    assert_eq!(
        WindowPlacement::from_bytes(&keyless.to_bytes()),
        Ok(keyless)
    );
}

fn another_version_is_refused() {
    let text = String::from_utf8(sample().to_bytes()).expect("text");
    let future = text.replacen("flui-window-placement/1", "flui-window-placement/2", 1);
    assert_eq!(
        WindowPlacement::from_bytes(future.as_bytes()),
        Err(PlacementError::UnsupportedVersion)
    );
    assert_eq!(
        WindowPlacement::from_bytes(b"\xff\xfe"),
        Err(PlacementError::UnsupportedVersion)
    );
}

fn truncated_bytes_are_refused_at_every_cut() {
    let bytes = sample().to_bytes();
    let version_line = "flui-window-placement/1\n".len();
    for cut in version_line..bytes.len() - "end\n".len() {
        assert_eq!(
            WindowPlacement::from_bytes(&bytes[..cut]),
            Err(PlacementError::Truncated),
            "cut at {cut}"
        );
    }
    assert_eq!(
        WindowPlacement::from_bytes(b""),
        Err(PlacementError::Truncated)
    );
}

fn a_field_the_format_forbids_is_refused() {
    let good = String::from_utf8(sample().to_bytes()).expect("text");
    for (from, to, field) in [
        ("size=800.25,600", "size=0,600", "size"),
        ("scale=1.5", "scale=-1", "scale"),
        ("scale=1.5", "scale=NaN", "scale"),
        ("scale=1.5", "scale=64", "scale"),
        ("mode=minimized", "mode=floating", "mode"),
        ("offset=-120.5,80", "offset=1", "offset"),
        ("offset=-120.5,80", "offset=inf,80", "offset"),
        ("offset=-120.5,80", "offset=NaN,80", "offset"),
    ] {
        let bad = good.replacen(from, to, 1);
        assert_eq!(
            WindowPlacement::from_bytes(bad.as_bytes()),
            Err(PlacementError::Malformed { field }),
            "{to}"
        );
    }
}

fn a_field_given_twice_is_refused() {
    let good = String::from_utf8(sample().to_bytes()).expect("text");
    for (line, field) in [
        ("display=other\n", "display"),
        ("offset=1,2\n", "offset"),
        ("size=3,4\n", "size"),
        ("mode=normal\n", "mode"),
        ("scale=9\n", "scale"),
        ("maximized_before_minimize=0\n", "maximized_before_minimize"),
    ] {
        let twice = good.replacen("\nend\n", &format!("\n{line}end\n"), 1);
        assert_eq!(
            WindowPlacement::from_bytes(twice.as_bytes()),
            Err(PlacementError::Malformed { field }),
            "{line}"
        );
    }
}

/// The point `(x, y)` of `display`.
fn on(display: DisplayId, x: f64, y: f64) -> WindowPosition {
    WindowPosition::try_new(display, Point::new(x, y)).expect("a finite point")
}

/// A rectangle with this position and size.
fn geometry(position: Option<WindowPosition>, size: Size<f64>) -> WindowGeometry {
    WindowGeometry::try_new(position, size).expect("a finite rectangle")
}

/// A window with this outer rectangle and content size.
fn window(position: Option<WindowPosition>, outer: Size<f64>, content: Size<f64>) -> WindowState {
    WindowState::new(
        geometry(position, outer),
        ext(content.width, content.height),
    )
}

fn from_state_remembers_the_normal_bounds_relative_to_the_usable_area() {
    let state = window(
        Some(on(DESK.right, 100.0, 50.0)),
        Size::new(816.0, 639.0),
        Size::new(800.0, 600.0),
    );
    let captured = WindowPlacement::from_state(&state, &DESK.displays, false).expect("usable");
    assert_eq!(captured.display.as_deref(), Some("key-right"));
    assert_eq!(
        captured.offset.map(PlacementOffset::get),
        Some(Point::new(100.0, 50.0))
    );
    assert_eq!(captured.size, Size::new(800.0, 600.0));
    assert_eq!(captured.scale, two());

    // Maximized on the left display, returning to bounds on the right one:
    // the placement is of the bounds it returns to, on their display.
    let maximized = window(
        Some(on(DESK.left, 0.0, 0.0)),
        Size::new(1920.0, 1040.0),
        Size::new(1904.0, 1001.0),
    )
    .with_mode(WindowMode::Maximized)
    .with_normal_geometry(Some(geometry(
        Some(on(DESK.right, 200.0, 100.0)),
        Size::new(816.0, 639.0),
    )));
    let captured = WindowPlacement::from_state(&maximized, &DESK.displays, false).expect("usable");
    assert_eq!(captured.mode, WindowMode::Maximized);
    assert_eq!(captured.display.as_deref(), Some("key-right"));
    assert_eq!(
        captured.offset.map(PlacementOffset::get),
        Some(Point::new(200.0, 100.0))
    );
    assert_eq!(captured.size, Size::new(800.0, 600.0));
}

/// A position is measured from its display's corner and a placement's offset
/// from the display's usable area: a window 40 logical pixels below a panel
/// along the top of a 1.5x display left of the primary is remembered 40
/// below the panel, and comes back at the same position.
fn a_panel_along_the_top_moves_the_offset_not_the_position() {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(7)).expect("id");
    // 2400x1350 device pixels at 1.5 is 1600x900 logical, the panel 48
    // device pixels (32 logical) high.
    let display = device_display(
        id,
        bounds(-2400, 0, 2400, 1350),
        bounds(-2400, 48, 2400, 1302),
        scale(1.5),
    )
    .with_persistent_key("key-panel");
    let displays = DisplaySnapshot::empty()
        .successor(vec![display])
        .expect("revision");
    let at = on(id, 300.0, 72.0);
    let state = window(Some(at), Size::new(816.0, 639.0), Size::new(800.0, 600.0));
    let captured = WindowPlacement::from_state(&state, &displays, false).expect("usable");
    assert_eq!(
        captured.offset.map(PlacementOffset::get),
        Some(Point::new(300.0, 40.0))
    );
    let back = captured
        .resolve(&displays, &WindowCapabilities::ALL, NO_FRAME)
        .expect("the display holds it");
    assert_eq!(
        back.position(),
        Some(at),
        "the same position on the same display"
    );
}

fn from_state_keeps_no_position_the_system_did_not_report() {
    let unplaced = window(None, Size::new(816.0, 639.0), Size::new(800.0, 600.0))
        .with_display(Some(DESK.right));
    let captured = WindowPlacement::from_state(&unplaced, &DESK.displays, false).expect("usable");
    assert_eq!(captured.offset, None);
    assert_eq!(captured.display.as_deref(), Some("key-right"));
}

/// A host that reports where its windows are and published no position for
/// one (a sample that could not find its monitor) has lost the place: nothing
/// is remembered, so an empty offset never overwrites the placement before.
fn from_state_remembers_nothing_a_placing_host_could_not_place() {
    let unplaced = window(None, Size::new(816.0, 639.0), Size::new(800.0, 600.0))
        .with_display(Some(DESK.right))
        .with_capabilities(WindowCapabilities::ALL);
    assert_eq!(
        WindowPlacement::from_state(&unplaced, &DESK.displays, false),
        None
    );
}

/// A position on a display the snapshot does not hold (a monitor plugged in
/// since it was taken) is no place a later run can find: nothing is
/// remembered from it, so the placement remembered before stays.
fn from_state_remembers_nothing_on_a_display_the_snapshot_lacks() {
    let unknown = DisplayRegistry::new()
        .mint(&DisplayKey::Injected(1))
        .expect("an id");
    let state = window(
        Some(on(unknown, 10.0, 10.0)),
        Size::new(816.0, 639.0),
        Size::new(800.0, 600.0),
    );
    assert_eq!(
        WindowPlacement::from_state(&state, &DESK.displays, false),
        None
    );
}

/// A snapshot holds no number that is not finite, so what is left to refuse
/// is a window with no area.
fn from_state_refuses_a_window_with_no_area() {
    assert_eq!(
        WindowPlacement::from_state(
            &WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO),
            &DESK.displays,
            false
        ),
        None
    );
    // Away from normal with no rectangle to return to, which a window whose
    // window system reported none yet publishes: no 1x1 placement either.
    for mode in [WindowMode::Maximized, WindowMode::Fullscreen] {
        assert_eq!(
            WindowPlacement::from_state(
                &WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO).with_mode(mode),
                &DESK.displays,
                false
            ),
            None,
            "{mode:?}"
        );
    }
}

#[test]
fn placement_codec() {
    crate::run_table(
        "placement_codec",
        &[
            (
                "a_placement_survives_the_bytes",
                a_placement_survives_the_bytes,
            ),
            ("another_version_is_refused", another_version_is_refused),
            (
                "truncated_bytes_are_refused_at_every_cut",
                truncated_bytes_are_refused_at_every_cut,
            ),
            (
                "a_field_the_format_forbids_is_refused",
                a_field_the_format_forbids_is_refused,
            ),
            (
                "a_field_given_twice_is_refused",
                a_field_given_twice_is_refused,
            ),
            (
                "from_state_remembers_the_normal_bounds_relative_to_the_usable_area",
                from_state_remembers_the_normal_bounds_relative_to_the_usable_area,
            ),
            (
                "from_state_keeps_no_position_the_system_did_not_report",
                from_state_keeps_no_position_the_system_did_not_report,
            ),
            (
                "from_state_remembers_nothing_on_a_display_the_snapshot_lacks",
                from_state_remembers_nothing_on_a_display_the_snapshot_lacks,
            ),
            (
                "from_state_remembers_nothing_a_placing_host_could_not_place",
                from_state_remembers_nothing_a_placing_host_could_not_place,
            ),
            (
                "a_panel_along_the_top_moves_the_offset_not_the_position",
                a_panel_along_the_top_moves_the_offset_not_the_position,
            ),
            (
                "from_state_refuses_a_window_with_no_area",
                from_state_refuses_a_window_with_no_area,
            ),
        ],
    );
}
