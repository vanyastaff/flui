//! `WindowPlacement`: the saved bytes and the fit of a remembered window to the
//! displays of the next run.

use std::sync::LazyLock;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, PlacementError,
    ResolvedPlacement, WindowCapabilities, WindowGeometry, WindowMode, WindowPlacement,
    WindowState,
};

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
    let primary = DisplayInfo::new(left, "left", bounds(0, 0, 1920, 1080), 1.0)
        .with_usable_bounds(bounds(0, 0, 1920, 1040))
        .with_primary(true)
        .with_persistent_key("key-left");
    let hidpi = DisplayInfo::new(right, "right", bounds(1920, 0, 3840, 2160), 2.0)
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
        Some(Point::new(offset.0, offset.1)),
        Size::new(size.0, size.1),
        WindowMode::Normal,
        1.0,
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
    // Relative to that display, whatever the other displays' scales are.
    assert_eq!(on_right.offset_in_display, Some(Point::new(100.0, 50.0)));
    assert_eq!(on_right.size, Size::new(800.0, 600.0));
    let on_left = resolved(&placement(Some("key-left"), (100.0, 50.0), (800.0, 600.0)));
    assert_eq!(on_left.display, DESK.left);
    assert_eq!(on_left.offset_in_display, Some(Point::new(100.0, 50.0)));
}

fn a_missing_display_falls_back_to_the_primary() {
    let gone = resolved(&placement(
        Some("key-unplugged"),
        (100.0, 50.0),
        (800.0, 600.0),
    ));
    assert_eq!(gone.display, DESK.left);
    assert_eq!(gone.offset_in_display, Some(Point::new(100.0, 50.0)));
}

fn a_window_without_a_key_opens_on_the_primary() {
    let keyless = resolved(&placement(None, (10.0, 20.0), (800.0, 600.0)));
    assert_eq!(keyless.display, DESK.left);
    assert_eq!(keyless.offset_in_display, Some(Point::new(10.0, 20.0)));
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
    assert_eq!(wide.offset_in_display, Some(Point::new(0.0, 0.0)));
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
    assert_eq!(wide.offset_in_display, Some(Point::new(0.0, 0.0)));
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
    assert_eq!(near_edge.offset_in_display, Some(Point::new(1104.0, 401.0)));
}

fn a_window_off_the_edge_is_shifted_back_onto_the_display() {
    let past = resolved(&placement(
        Some("key-left"),
        (1800.0, 1000.0),
        (800.0, 600.0),
    ));
    assert_eq!(past.offset_in_display, Some(Point::new(1120.0, 440.0)));
    let before = resolved(&placement(
        Some("key-left"),
        (-500.0, -90.0),
        (800.0, 600.0),
    ));
    assert_eq!(before.offset_in_display, Some(Point::new(0.0, 0.0)));
    let right = resolved(&placement(Some("key-right"), (5000.0, 0.0), (800.0, 600.0)));
    // The right display is 1920 logical pixels wide in its own scale.
    assert_eq!(right.offset_in_display, Some(Point::new(1120.0, 0.0)));
}

fn a_window_moved_to_another_scale_keeps_its_physical_size() {
    let moved = WindowPlacement::new(
        Some("key-unplugged".into()),
        Some(Point::new(0.0, 0.0)),
        Size::new(400.0, 300.0),
        WindowMode::Normal,
        2.0,
    );
    // It was 2x on a display that is gone; the primary is 1x.
    assert_eq!(resolved(&moved).size, Size::new(800.0, 600.0));
    let same = WindowPlacement::new(
        Some("key-right".into()),
        Some(Point::new(0.0, 0.0)),
        Size::new(400.0, 300.0),
        WindowMode::Normal,
        2.0,
    );
    assert_eq!(resolved(&same).size, Size::new(400.0, 300.0));
}

fn a_minimized_window_opens_normal_or_maximized() {
    let minimized = WindowPlacement::new(
        None,
        Some(Point::new(0.0, 0.0)),
        Size::new(800.0, 600.0),
        WindowMode::Minimized,
        1.0,
    );
    assert_eq!(resolved(&minimized).mode, WindowMode::Normal);
    let was_maximized = minimized.with_maximized_before_minimize(true);
    assert_eq!(resolved(&was_maximized).mode, WindowMode::Maximized);
}

fn a_mode_the_window_cannot_enter_becomes_normal() {
    let full = WindowPlacement::new(
        None,
        Some(Point::new(0.0, 0.0)),
        Size::new(800.0, 600.0),
        WindowMode::Fullscreen,
        1.0,
    );
    assert_eq!(resolved(&full).mode, WindowMode::Fullscreen);
    let mut capabilities = WindowCapabilities::ALL;
    capabilities.enters_fullscreen = false;
    let result = full
        .resolve(&DESK.displays, &capabilities, NO_FRAME)
        .expect("resolves");
    assert_eq!(result.mode, WindowMode::Normal);
}

fn without_places_at_open_size_and_mode_stay_but_position_goes() {
    let mut capabilities = WindowCapabilities::ALL;
    capabilities.places_at_open = false;
    let maximized = WindowPlacement::new(
        Some("key-left".into()),
        Some(Point::new(100.0, 100.0)),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    let result = maximized
        .resolve(&DESK.displays, &capabilities, NO_FRAME)
        .expect("resolves");
    assert_eq!(result.offset_in_display, None);
    assert_eq!(result.size, Size::new(800.0, 600.0));
    assert_eq!(result.mode, WindowMode::Maximized);
}

fn a_placement_without_a_position_stays_unpositioned() {
    let unplaced = WindowPlacement::new(
        Some("key-right".into()),
        None,
        Size::new(800.0, 600.0),
        WindowMode::Normal,
        2.0,
    );
    let result = resolved(&unplaced);
    assert_eq!(result.display, DESK.right);
    assert_eq!(result.offset_in_display, None);
}

fn displays_that_cannot_hold_a_window_are_not_candidates() {
    let registry = DisplayRegistry::new();
    let make = |number, scale, usable: Bounds<i32>, primary| {
        DisplayInfo::new(
            registry.mint(&DisplayKey::Injected(number)).expect("id"),
            "display",
            bounds(0, 0, 1920, 1080),
            scale,
        )
        .with_usable_bounds(usable)
        .with_primary(primary)
        .with_persistent_key(format!("key-{number}"))
    };
    let snapshot = DisplaySnapshot::empty()
        .successor(vec![
            // The primary reports a scale that cannot be divided by.
            make(1, f64::NAN, bounds(0, 0, 1920, 1080), true),
            make(2, 0.0, bounds(0, 0, 1920, 1080), false),
            // No usable area at all.
            make(3, 1.0, bounds(0, 0, 0, 0), false),
            make(4, 1.0, bounds(0, 0, 1280, 720), false),
        ])
        .expect("revision");
    let result = placement(Some("key-3"), (0.0, 0.0), (800.0, 600.0))
        .resolve(&snapshot, &WindowCapabilities::ALL, NO_FRAME)
        .expect("the one usable display holds it");
    let expected = snapshot.displays()[3].id;
    assert_eq!(result.display, expected);

    let only_broken = DisplaySnapshot::empty()
        .successor(vec![make(5, f64::INFINITY, bounds(0, 0, 100, 100), true)])
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
    for bad in [
        placement(None, (0.0, 0.0), (f64::NAN, 600.0)),
        placement(None, (0.0, 0.0), (800.0, 0.0)),
        placement(None, (f64::INFINITY, 0.0), (800.0, 600.0)),
        WindowPlacement::new(
            None,
            Some(Point::new(0.0, 0.0)),
            Size::new(800.0, 600.0),
            WindowMode::Normal,
            0.0,
        ),
    ] {
        assert_eq!(
            bad.resolve(&DESK.displays, &WindowCapabilities::ALL, NO_FRAME),
            None,
            "{bad:?}"
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
        Some(Point::new(-120.5, 80.0)),
        Size::new(800.25, 600.0),
        WindowMode::Minimized,
        1.5,
    )
    .with_maximized_before_minimize(true)
}

fn a_placement_survives_the_bytes() {
    assert_eq!(
        WindowPlacement::from_bytes(&sample().to_bytes()),
        Ok(sample())
    );
    let keyless = WindowPlacement::new(None, None, Size::new(1.0, 1.0), WindowMode::Normal, 1.0);
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

fn from_state_remembers_the_normal_bounds_relative_to_the_usable_area() {
    let state = WindowState::new(
        WindowGeometry::new(Some(Point::new(1060.0, 50.0)), Size::new(816.0, 639.0)),
        Size::new(800.0, 600.0),
    )
    .with_display(Some(DESK.right));
    let captured = WindowPlacement::from_state(&state, &DESK.displays, false).expect("usable");
    assert_eq!(captured.display.as_deref(), Some("key-right"));
    assert_eq!(captured.offset, Some(Point::new(100.0, 50.0)));
    assert_eq!(captured.size, Size::new(800.0, 600.0));
    assert_eq!(captured.scale, 2.0);

    let maximized = WindowState::new(
        WindowGeometry::new(Some(Point::new(0.0, 0.0)), Size::new(1920.0, 1040.0)),
        Size::new(1904.0, 1001.0),
    )
    .with_mode(WindowMode::Maximized)
    .with_normal_geometry(Some(WindowGeometry::new(
        Some(Point::new(200.0, 100.0)),
        Size::new(816.0, 639.0),
    )))
    .with_display(Some(DESK.left));
    let captured = WindowPlacement::from_state(&maximized, &DESK.displays, false).expect("usable");
    assert_eq!(captured.mode, WindowMode::Maximized);
    assert_eq!(captured.offset, Some(Point::new(200.0, 100.0)));
    assert_eq!(captured.size, Size::new(800.0, 600.0));
}

fn from_state_keeps_no_position_the_system_did_not_report() {
    let unplaced = WindowState::new(
        WindowGeometry::new(None, Size::new(816.0, 639.0)),
        Size::new(800.0, 600.0),
    );
    let captured = WindowPlacement::from_state(&unplaced, &DESK.displays, false).expect("usable");
    assert_eq!(captured.offset, None);
}

fn from_state_refuses_numbers_that_could_not_be_restored() {
    let off = |x: f64| {
        WindowState::new(
            WindowGeometry::new(Some(Point::new(x, 0.0)), Size::new(816.0, 639.0)),
            Size::new(800.0, 600.0),
        )
    };
    assert_eq!(
        WindowPlacement::from_state(&off(f64::NAN), &DESK.displays, false),
        None
    );
    assert_eq!(
        WindowPlacement::from_state(
            &WindowState::new(
                WindowGeometry::new(None, Size::new(0.0, 0.0)),
                Size::new(0.0, 0.0)
            ),
            &DESK.displays,
            false
        ),
        None
    );
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
                "from_state_refuses_numbers_that_could_not_be_restored",
                from_state_refuses_numbers_that_could_not_be_restored,
            ),
        ],
    );
}
