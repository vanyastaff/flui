//! `WindowPlacement`: the saved bytes and the fit of a remembered window to the
//! displays of the next run.

use std::sync::LazyLock;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, PlacementError,
    WindowCapabilities, WindowGeometry, WindowMode, WindowPlacement, WindowState,
};

fn bounds(x: i32, y: i32, width: i32, height: i32) -> Bounds<i32> {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}

static DESK: LazyLock<(DisplaySnapshot, DisplayId, DisplayId)> = LazyLock::new(build_desk);

fn desk() -> (DisplaySnapshot, DisplayId, DisplayId) {
    DESK.clone()
}

/// A 1x primary display with a 40 px task bar, and a 2x display to its right.
fn build_desk() -> (DisplaySnapshot, DisplayId, DisplayId) {
    let registry = DisplayRegistry::new();
    let first = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let second = registry.mint(&DisplayKey::Injected(2)).expect("id");
    let primary = DisplayInfo::new(first, "left", bounds(0, 0, 1920, 1080), 1.0)
        .with_usable_bounds(bounds(0, 0, 1920, 1040))
        .with_primary(true)
        .with_persistent_key("key-left");
    let hidpi = DisplayInfo::new(second, "right", bounds(1920, 0, 3840, 2160), 2.0)
        .with_persistent_key("key-right");
    let snapshot = DisplaySnapshot::empty()
        .successor(vec![primary, hidpi])
        .expect("revision");
    (snapshot, first, second)
}

fn placement(display: Option<&str>, offset: (f64, f64), size: (f64, f64)) -> WindowPlacement {
    WindowPlacement::new(
        display.map(str::to_owned),
        Point::new(offset.0, offset.1),
        Size::new(size.0, size.1),
        WindowMode::Normal,
        1.0,
    )
}

fn resolved(placement: &WindowPlacement) -> flui_platform_api::ResolvedPlacement {
    placement.resolve(&desk().0, &WindowCapabilities::all())
}

fn the_display_is_found_by_its_key() {
    let (_, left, right) = desk();
    let on_right = resolved(&placement(Some("key-right"), (100.0, 50.0), (800.0, 600.0)));
    assert_eq!(on_right.display, Some(right));
    // The right display starts at device x 1920, which is logical 960.
    assert_eq!(on_right.position, Some(Point::new(1060.0, 50.0)));
    assert_eq!(on_right.size, Size::new(800.0, 600.0));
    let on_left = resolved(&placement(Some("key-left"), (100.0, 50.0), (800.0, 600.0)));
    assert_eq!(on_left.display, Some(left));
    assert_eq!(on_left.position, Some(Point::new(100.0, 50.0)));
}

fn a_missing_display_falls_back_to_the_primary() {
    let (_, left, _) = desk();
    let gone = resolved(&placement(
        Some("key-unplugged"),
        (100.0, 50.0),
        (800.0, 600.0),
    ));
    assert_eq!(gone.display, Some(left));
    assert_eq!(gone.position, Some(Point::new(100.0, 50.0)));
}

fn a_window_without_a_key_opens_on_the_primary() {
    let (_, left, _) = desk();
    let keyless = resolved(&placement(None, (10.0, 20.0), (800.0, 600.0)));
    assert_eq!(keyless.display, Some(left));
    assert_eq!(keyless.position, Some(Point::new(10.0, 20.0)));
}

fn no_displays_leaves_the_window_unplaced() {
    let none = placement(Some("key-left"), (10.0, 20.0), (800.0, 600.0))
        .resolve(&DisplaySnapshot::empty(), &WindowCapabilities::all());
    assert_eq!(none.display, None);
    assert_eq!(none.position, None);
    assert_eq!(none.size, Size::new(800.0, 600.0));
}

fn a_window_wider_than_the_work_area_is_cut_to_it() {
    let wide = resolved(&placement(Some("key-left"), (300.0, 0.0), (5000.0, 600.0)));
    assert_eq!(wide.size, Size::new(1920.0, 600.0));
    assert_eq!(wide.position, Some(Point::new(0.0, 0.0)));
    let tall = resolved(&placement(Some("key-left"), (0.0, 0.0), (800.0, 2000.0)));
    // The usable area excludes the task bar.
    assert_eq!(tall.size, Size::new(800.0, 1040.0));
}

fn a_window_off_the_edge_is_shifted_back_onto_the_display() {
    let past = resolved(&placement(
        Some("key-left"),
        (1800.0, 1000.0),
        (800.0, 600.0),
    ));
    assert_eq!(past.position, Some(Point::new(1120.0, 440.0)));
    let before = resolved(&placement(
        Some("key-left"),
        (-500.0, -90.0),
        (800.0, 600.0),
    ));
    assert_eq!(before.position, Some(Point::new(0.0, 0.0)));
    let right = resolved(&placement(Some("key-right"), (5000.0, 0.0), (800.0, 600.0)));
    // The right display's usable area ends at logical x 1920 + 960.
    assert_eq!(right.position, Some(Point::new(2080.0, 0.0)));
}

fn a_window_moved_to_another_scale_keeps_its_physical_size() {
    let moved = WindowPlacement::new(
        Some("key-unplugged".into()),
        Point::new(0.0, 0.0),
        Size::new(400.0, 300.0),
        WindowMode::Normal,
        2.0,
    );
    // It was 2x on a display that is gone; the primary is 1x.
    assert_eq!(resolved(&moved).size, Size::new(800.0, 600.0));
    let same = WindowPlacement::new(
        Some("key-right".into()),
        Point::new(0.0, 0.0),
        Size::new(400.0, 300.0),
        WindowMode::Normal,
        2.0,
    );
    assert_eq!(resolved(&same).size, Size::new(400.0, 300.0));
}

fn a_minimized_window_opens_normal_or_maximized() {
    let minimized = WindowPlacement::new(
        None,
        Point::new(0.0, 0.0),
        Size::new(800.0, 600.0),
        WindowMode::Minimized,
        1.0,
    );
    assert_eq!(resolved(&minimized).mode, WindowMode::Normal);
    let was_maximized = minimized.with_restores_maximized(true);
    assert_eq!(resolved(&was_maximized).mode, WindowMode::Maximized);
}

fn a_mode_the_window_cannot_enter_becomes_normal() {
    let full = WindowPlacement::new(
        None,
        Point::new(0.0, 0.0),
        Size::new(800.0, 600.0),
        WindowMode::Fullscreen,
        1.0,
    );
    assert_eq!(resolved(&full).mode, WindowMode::Fullscreen);
    let mut capabilities = WindowCapabilities::all();
    capabilities.fullscreen = false;
    assert_eq!(
        full.resolve(&desk().0, &capabilities).mode,
        WindowMode::Normal
    );
}

fn without_restores_position_size_and_mode_stay_but_position_goes() {
    let mut capabilities = WindowCapabilities::all();
    capabilities.restores_position = false;
    let maximized = WindowPlacement::new(
        Some("key-left".into()),
        Point::new(100.0, 100.0),
        Size::new(800.0, 600.0),
        WindowMode::Maximized,
        1.0,
    );
    let result = maximized.resolve(&desk().0, &capabilities);
    assert_eq!(result.position, None);
    assert_eq!(result.size, Size::new(800.0, 600.0));
    assert_eq!(result.mode, WindowMode::Maximized);
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
                "no_displays_leaves_the_window_unplaced",
                no_displays_leaves_the_window_unplaced,
            ),
            (
                "a_window_wider_than_the_work_area_is_cut_to_it",
                a_window_wider_than_the_work_area_is_cut_to_it,
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
                "without_restores_position_size_and_mode_stay_but_position_goes",
                without_restores_position_size_and_mode_stay_but_position_goes,
            ),
        ],
    );
}

fn sample() -> WindowPlacement {
    WindowPlacement::new(
        Some("monitor 100%\nsecond".into()),
        Point::new(-120.5, 80.0),
        Size::new(800.25, 600.0),
        WindowMode::Minimized,
        1.5,
    )
    .with_restores_maximized(true)
}

fn a_placement_survives_encode_and_decode() {
    assert_eq!(WindowPlacement::decode(&sample().encode()), Ok(sample()));
    let keyless = placement(None, (0.0, 0.0), (1.0, 1.0));
    assert_eq!(WindowPlacement::decode(&keyless.encode()), Ok(keyless));
}

fn another_version_is_refused() {
    let bytes = sample().encode();
    let text = String::from_utf8(bytes).expect("text");
    let future = text.replacen("flui-window-placement/1", "flui-window-placement/2", 1);
    assert_eq!(
        WindowPlacement::decode(future.as_bytes()),
        Err(PlacementError::UnsupportedVersion)
    );
    assert_eq!(
        WindowPlacement::decode(b"\xff\xfe"),
        Err(PlacementError::UnsupportedVersion)
    );
}

fn truncated_bytes_are_refused_at_every_cut() {
    let bytes = sample().encode();
    let version_line = "flui-window-placement/1\n".len();
    for cut in version_line..bytes.len() - "end\n".len() {
        assert_eq!(
            WindowPlacement::decode(&bytes[..cut]),
            Err(PlacementError::Truncated),
            "cut at {cut}"
        );
    }
    assert_eq!(WindowPlacement::decode(b""), Err(PlacementError::Truncated));
}

fn a_field_the_format_forbids_is_refused() {
    let good = String::from_utf8(sample().encode()).expect("text");
    for (from, to, field) in [
        ("size=800.25,600", "size=0,600", "size"),
        ("scale=1.5", "scale=-1", "scale"),
        ("scale=1.5", "scale=NaN", "scale"),
        ("mode=minimized", "mode=floating", "mode"),
        ("offset=-120.5,80", "offset=1", "offset"),
    ] {
        let bad = good.replacen(from, to, 1);
        assert_eq!(
            WindowPlacement::decode(bad.as_bytes()),
            Err(PlacementError::Malformed { field }),
            "{to}"
        );
    }
}

fn capture_remembers_the_normal_bounds_relative_to_the_usable_area() {
    let (displays, left, right) = desk();
    let state = WindowState::new(
        WindowGeometry::new(Some(Point::new(1060.0, 50.0)), Size::new(816.0, 639.0)),
        Size::new(800.0, 600.0),
    )
    .with_display(Some(right));
    let captured = WindowPlacement::capture(&state, &displays, false).expect("usable");
    assert_eq!(captured.display.as_deref(), Some("key-right"));
    assert_eq!(captured.offset, Point::new(100.0, 50.0));
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
    .with_display(Some(left));
    let captured = WindowPlacement::capture(&maximized, &displays, false).expect("usable");
    assert_eq!(captured.mode, WindowMode::Maximized);
    assert_eq!(captured.offset, Point::new(200.0, 100.0));
    assert_eq!(captured.size, Size::new(800.0, 600.0));
}

#[test]
fn placement_codec() {
    crate::run_table(
        "placement_codec",
        &[
            (
                "a_placement_survives_encode_and_decode",
                a_placement_survives_encode_and_decode,
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
                "capture_remembers_the_normal_bounds_relative_to_the_usable_area",
                capture_remembers_the_normal_bounds_relative_to_the_usable_area,
            ),
        ],
    );
}
