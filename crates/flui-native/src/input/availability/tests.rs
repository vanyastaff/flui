//! Every backend's availability table (`input::availability`, ADR-0213):
//! what each input path delivers as the code stands, without presence.

use flui_platform::Availability;
use flui_platform::input_regions::InputRegionKind;

/// Each backend states what it implements, as the code stands; X11 has no
/// pen, trackpad gestures or outgoing drags (ADR-0213 §1); input regions reach
/// the OS on iOS, and on Android from API 29 (ADR-0217 §5); an Android
/// touchpad's scroll and pinch arrive from API 34; Win32 starts OS drags
/// through OLE and the web through HTML drags (ADR-0211); the web emulates
/// trackpad pinch from Control-held wheel events unless the browser has
/// WebKit gesture events. Columns: pen, precise scroll, trackpad
/// gestures, input regions, outgoing drags.
#[test]
fn every_backend_states_its_availability() {
    use crate::input::availability::{self as table, AndroidInputApis};
    use Availability::{Emulated, Native, Unavailable};
    let android = |gesture_exclusion, touchpad_gestures, drag_source| {
        table::android(AndroidInputApis {
            gesture_exclusion,
            touchpad_gestures,
            drag_source,
        })
    };
    let rows = [
        (
            "windows with DirectManipulation",
            table::windows(true),
            [Native, Native, Native, Unavailable, Native],
        ),
        (
            "windows without DirectManipulation",
            table::windows(false),
            [Native, Unavailable, Unavailable, Unavailable, Native],
        ),
        (
            "macos",
            table::macos(),
            [Unavailable, Native, Native, Unavailable, Unavailable],
        ),
        (
            "ios",
            table::ios(),
            [Native, Unavailable, Unavailable, Native, Unavailable],
        ),
        (
            "android with every input API",
            android(true, true, true),
            [Native, Native, Native, Native, Native],
        ),
        (
            "android without FluiView",
            android(true, true, false),
            [Native, Native, Native, Native, Unavailable],
        ),
        (
            "android before touchpad gestures",
            android(true, false, true),
            [Native, Unavailable, Unavailable, Native, Native],
        ),
        (
            "android before gesture exclusion",
            android(false, false, true),
            [Native, Unavailable, Unavailable, Unavailable, Native],
        ),
        (
            "android before startDragAndDrop",
            android(false, false, false),
            [Native, Unavailable, Unavailable, Unavailable, Unavailable],
        ),
        (
            "web with ctrl+wheel pinch",
            table::web(false),
            [Native, Unavailable, Emulated, Unavailable, Native],
        ),
        (
            "web with gesture events",
            table::web(true),
            [Native, Unavailable, Native, Unavailable, Native],
        ),
    ];
    for (backend, capabilities, expected) in rows {
        assert_eq!(
            [
                capabilities.pen(),
                capabilities.precise_scroll(),
                capabilities.trackpad_gestures(),
                capabilities.input_region(InputRegionKind::SystemGestureExclusion),
                capabilities.outgoing_drag(),
            ],
            expected,
            "{backend}"
        );
        assert!(
            capabilities.present().is_empty(),
            "{backend}: presence is the registry's"
        );
    }
}

/// Win32 answers the system's hit test from every chrome region and AppKit
/// moves the window from a move area (ADR-0286 §7, §8); the native X11 host
/// moves and resizes a window from them through its window manager (§10) and
/// the native Wayland host through
/// its compositor (§11); the others leave them
/// `Unavailable` until they do.
#[test]
fn the_desktop_hosts_honour_the_chrome_regions_they_answer() {
    use crate::input::availability::{self as table, AndroidInputApis};
    use Availability::{Native, Unavailable};
    use flui_platform::input_regions::{CaptionButton, ResizeEdge};

    let android = table::android(AndroidInputApis {
        gesture_exclusion: true,
        touchpad_gestures: true,
        drag_source: true,
    });
    // Move, resize, caption button, content.
    let all = [Native; 4];
    let none = [Unavailable; 4];
    let move_only = [Native, Unavailable, Unavailable, Native];
    let move_and_resize = [Native, Native, Unavailable, Native];
    for (backend, capabilities, expected) in [
        ("windows", table::windows(false), all),
        ("windows with DirectManipulation", table::windows(true), all),
        ("macos", table::macos(), move_only),
        ("ios", table::ios(), none),
        ("android", android, none),
        ("web", table::web(true), none),
        ("x11", table::x11(false), move_and_resize),
        (
            "wayland",
            table::wayland(table::WaylandInputGlobals::default()),
            move_and_resize,
        ),
    ] {
        let kinds = [
            InputRegionKind::WindowMove,
            InputRegionKind::WindowResize(ResizeEdge::Top),
            InputRegionKind::CaptionButton(CaptionButton::Maximize),
            InputRegionKind::WindowContent,
        ];
        for (kind, expected) in kinds.into_iter().zip(expected) {
            assert_eq!(
                capabilities.input_region(kind),
                expected,
                "{backend}: {kind:?}"
            );
        }
    }
}
