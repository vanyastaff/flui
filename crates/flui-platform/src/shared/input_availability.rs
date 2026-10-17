//! Every backend's input availability table, as pure functions (ADR-0213).
//!
//! Each answers what that backend's input path delivers in an OS session, as
//! the code stands: `Native` is delivered by the OS path, `Emulated` is
//! synthesized from other input, `Unavailable` has no path. Presence is not
//! here: a backend adds the device kinds attached now from its
//! [`DeviceRegistry`](super::pointer_assembly::DeviceRegistry) with
//! [`InputCapabilities::with_present`]. The functions are cfg-free so Linux CI
//! runs every backend's table.

use flui_platform_api::input_capabilities::{Availability, InputCapabilities};

use Availability::{Native, Unavailable};

/// The OS session a winit window runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WinitSession {
    /// Win32.
    Windows,
    /// AppKit.
    MacOs,
    /// A Wayland compositor.
    Wayland,
    /// An X11 server.
    X11,
}

/// The native Win32 backend: pointer messages carry pen input. A window whose
/// DirectManipulation viewport initialised (`direct_manipulation`) delivers
/// the precision touchpad's phased scrolling and pinch; without one, wheel
/// messages have no phases and there are no trackpad gestures. Every window
/// starts OS drag sessions through OLE (`DoDragDrop`). Input regions are not
/// wired yet.
#[must_use]
pub const fn windows(direct_manipulation: bool) -> InputCapabilities {
    let builder = InputCapabilities::builder()
        .pen(Native)
        .outgoing_drag(Native);
    if direct_manipulation {
        builder
            .precise_scroll(Native)
            .trackpad_gestures(Native)
            .build()
    } else {
        builder.build()
    }
}

/// The native AppKit backend: phased precise scrolling and magnify/rotate
/// gestures; tablet events are not translated yet.
#[must_use]
pub const fn macos() -> InputCapabilities {
    InputCapabilities::builder()
        .precise_scroll(Native)
        .trackpad_gestures(Native)
        .build()
}

/// The UIKit backend: Pencil touches carry pen input; system-gesture
/// exclusion regions defer the screen edges they reach
/// (`preferredScreenEdgesDeferringSystemGestures`).
#[must_use]
pub const fn ios() -> InputCapabilities {
    InputCapabilities::builder()
        .pen(Native)
        .input_regions(Native)
        .build()
}

/// The Android backend at API level `api_level`: stylus motion events carry pen
/// input; system-gesture exclusion regions reach the decor view's
/// `setSystemGestureExclusionRects` from API 29; a touchpad's two-finger
/// scroll and pinch arrive as gesture axes from API 34
/// ([`TOUCHPAD_GESTURES_API`](super::android_motion::TOUCHPAD_GESTURES_API)),
/// delivered as phased scrolling and `PanZoom`.
#[must_use]
pub const fn android(api_level: i32) -> InputCapabilities {
    let mut builder = InputCapabilities::builder().pen(Native);
    if api_level >= ANDROID_GESTURE_EXCLUSION_API {
        builder = builder.input_regions(Native);
    }
    if api_level >= super::android_motion::TOUCHPAD_GESTURES_API {
        builder = builder.precise_scroll(Native).trackpad_gestures(Native);
    }
    builder.build()
}

/// The first Android API level with `View.setSystemGestureExclusionRects`.
pub const ANDROID_GESTURE_EXCLUSION_API: i32 = 29;

/// The web backend: pointer events carry pen input; wheel events have no
/// phases.
#[must_use]
pub const fn web() -> InputCapabilities {
    InputCapabilities::builder().pen(Native).build()
}

/// The winit backend in `session`: winit 0.30 reports no pen; touch phases on
/// pixel scrolling arrive on macOS and Wayland; pinch and rotate on macOS
/// only; outgoing drags are not supported.
#[must_use]
pub const fn winit(session: WinitSession) -> InputCapabilities {
    let builder = InputCapabilities::builder();
    match session {
        WinitSession::MacOs => builder
            .precise_scroll(Native)
            .trackpad_gestures(Native)
            .build(),
        WinitSession::Wayland => builder.precise_scroll(Native).build(),
        WinitSession::Windows | WinitSession::X11 => builder.precise_scroll(Unavailable).build(),
    }
}

/// When an OS drag session can start on Win32, AppKit, Android and winit: the
/// drag runs in its window and escalates when the pointer leaves it
/// (ADR-0211 §2). Win32 starts them through OLE; the other backends report
/// outgoing drags `Unavailable` until their native session lands.
pub const DESKTOP_DRAG_START: flui_platform_api::drag::OsDragStart =
    flui_platform_api::drag::OsDragStart::OnLeavingWindow;

/// When an OS drag session can start on iOS and iPadOS: at lift, through
/// `UIDragInteraction`, and never mid-drag (ADR-0211 §11).
pub const IOS_DRAG_START: flui_platform_api::drag::OsDragStart =
    flui_platform_api::drag::OsDragStart::AtStart { eager_data: false };

/// When an OS drag session can start on the web: at `dragstart`, with the
/// representations materialized, since the browser allows no lazy data
/// (ADR-0211 §11).
pub const WEB_DRAG_START: flui_platform_api::drag::OsDragStart =
    flui_platform_api::drag::OsDragStart::AtStart { eager_data: true };
