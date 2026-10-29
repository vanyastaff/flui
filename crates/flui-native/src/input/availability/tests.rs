//! Every backend's availability table (`input::availability`, ADR-0213):
//! what each input path delivers as the code stands, without presence.

use flui_platform::Availability;

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
    use crate::input::availability::{self as table, AndroidInputApis, WinitSession};
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
        (
            "winit x11",
            table::winit(WinitSession::X11),
            [
                Unavailable,
                Unavailable,
                Unavailable,
                Unavailable,
                Unavailable,
            ],
        ),
        (
            "winit wayland",
            table::winit(WinitSession::Wayland),
            [Unavailable, Native, Unavailable, Unavailable, Unavailable],
        ),
        (
            "winit macos",
            table::winit(WinitSession::MacOs),
            [Unavailable, Native, Native, Unavailable, Unavailable],
        ),
    ];
    for (backend, capabilities, expected) in rows {
        assert_eq!(
            [
                capabilities.pen(),
                capabilities.precise_scroll(),
                capabilities.trackpad_gestures(),
                capabilities.input_regions(),
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
