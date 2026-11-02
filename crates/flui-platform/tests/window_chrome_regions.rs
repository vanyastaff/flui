//! A backend answers the system's hit test from the chrome regions of the last composited
//! frame: the last region painted over a point decides, content cut out of a move area gives
//! the point back to the application, and a window honours only the kinds its input
//! capabilities name.

use flui_foundation::geometry::{DevicePoint, DeviceRect};
use flui_platform::Availability;
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::input_regions::{
    CaptionButton, ChromeHit, DeviceInputRegion, InputRegionKind, ResizeEdge, chrome_hit,
};

use super::run_table;

fn region(
    kind: InputRegionKind,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> DeviceInputRegion {
    DeviceInputRegion::new(
        kind,
        DeviceRect::from_min_max(DevicePoint::new(left, top), DevicePoint::new(right, bottom)),
    )
}

/// A 400×300 window: a 32 px title bar with a search field cut out of it and a close
/// button at its trailing end, an 8 px resize border along the top, and a gesture
/// exclusion strip down the left edge, in the order a frame paints them.
fn window() -> Vec<DeviceInputRegion> {
    vec![
        region(InputRegionKind::SystemGestureExclusion, 0, 0, 20, 300),
        region(InputRegionKind::WindowMove, 0, 0, 400, 32),
        region(InputRegionKind::WindowContent, 100, 4, 300, 28),
        region(
            InputRegionKind::CaptionButton(CaptionButton::Close),
            352,
            0,
            400,
            32,
        ),
        region(InputRegionKind::WindowResize(ResizeEdge::Top), 0, 0, 400, 8),
    ]
}

fn hit(x: i32, y: i32) -> ChromeHit {
    chrome_hit(&window(), DevicePoint::new(x, y))
}

#[test]
fn chrome_hit_takes_the_last_region_painted_over_the_point() {
    run_table(
        "chrome_hit_takes_the_last_region_painted_over_the_point",
        &[
            ("a title bar moves the window", || {
                assert_eq!(hit(50, 20), ChromeHit::Move);
            }),
            ("content cut out of the bar is the application's", || {
                assert_eq!(hit(200, 20), ChromeHit::Content);
            }),
            ("a button over the bar is the button", || {
                assert_eq!(hit(380, 20), ChromeHit::CaptionButton(CaptionButton::Close));
            }),
            ("a border painted last lies over the button", || {
                assert_eq!(hit(380, 2), ChromeHit::Resize(ResizeEdge::Top));
            }),
            ("a gesture exclusion is not chrome", || {
                assert_eq!(hit(10, 20), ChromeHit::Move);
                assert_eq!(hit(10, 200), ChromeHit::Content);
            }),
            ("a point no region holds is content", || {
                assert_eq!(hit(200, 200), ChromeHit::Content);
            }),
            ("no regions is all content", || {
                assert_eq!(chrome_hit(&[], DevicePoint::new(0, 0)), ChromeHit::Content);
            }),
        ],
    );
}

#[test]
fn a_chrome_region_holds_its_minimum_edges_and_not_its_maximum_ones() {
    run_table(
        "a_chrome_region_holds_its_minimum_edges_and_not_its_maximum_ones",
        &[
            ("the top-left corner is inside", || {
                assert_eq!(hit(0, 9), ChromeHit::Move);
            }),
            ("the bottom edge is outside", || {
                assert_eq!(hit(50, 32), ChromeHit::Content);
                assert_eq!(hit(50, 31), ChromeHit::Move);
            }),
            ("the right edge is outside", || {
                assert_eq!(hit(400, 20), ChromeHit::Content);
                assert_eq!(hit(399, 20), ChromeHit::CaptionButton(CaptionButton::Close));
            }),
            ("a cut-out ends where its rectangle does", || {
                assert_eq!(hit(99, 20), ChromeHit::Move);
                assert_eq!(hit(300, 20), ChromeHit::Move);
            }),
        ],
    );
}

#[test]
fn a_window_honours_the_region_kinds_its_input_capabilities_name() {
    let resize_only = InputCapabilities::builder()
        .window_resize(Availability::Native)
        .build();
    let move_only = InputCapabilities::builder()
        .window_move(Availability::Native)
        .build();
    let exclusion_only = InputCapabilities::builder()
        .system_gesture_exclusion(Availability::Native)
        .build();
    let edge = InputRegionKind::WindowResize(ResizeEdge::BottomRight);
    let close = InputRegionKind::CaptionButton(CaptionButton::Close);
    let rows = [
        (
            "none",
            InputCapabilities::UNAVAILABLE,
            [false, false, false, false, false],
            false,
        ),
        (
            "resize only",
            resize_only,
            [false, false, true, false, true],
            true,
        ),
        (
            "move only",
            move_only,
            [false, true, false, false, true],
            true,
        ),
        (
            "gesture exclusion only",
            exclusion_only,
            [true, false, false, false, false],
            true,
        ),
    ];
    for (name, capabilities, expected, honours) in rows {
        let honoured = [
            InputRegionKind::SystemGestureExclusion,
            InputRegionKind::WindowMove,
            edge,
            close,
            InputRegionKind::WindowContent,
        ]
        .map(|kind| capabilities.input_region(kind).is_available());
        assert_eq!(honoured, expected, "{name}");
        assert_eq!(capabilities.honours_input_regions(), honours, "{name}");
    }
}
