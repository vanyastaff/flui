//! Every backend's input availability table, as pure functions (ADR-0213).
//!
//! Each answers what that backend's input path delivers in an OS session, as
//! the code stands: `Native` is delivered by the OS path, `Emulated` is
//! synthesized from other input, `Unavailable` has no path. Presence is not
//! here: a backend adds the device kinds attached now from its
//! [`DeviceRegistry`](super::assembly::DeviceRegistry) with
//! [`InputCapabilities::with_present`]. The functions are cfg-free so Linux CI
//! runs every backend's table.

use flui_platform::Availability;
use flui_platform::input_capabilities::InputCapabilities;

use Availability::{Emulated, Native, Unavailable};

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

/// The input APIs above the floor an Android window has, from the device's API
/// level and the view's installation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AndroidInputApis {
    /// `View.setSystemGestureExclusionRects` (API 29).
    pub gesture_exclusion: bool,
    /// The touchpad gesture axes of `MotionEvent` (API 34).
    pub touchpad_gestures: bool,
    /// `FluiView.startDragAndDrop`: API 24, and the view installed (ADR-0247).
    pub drag_source: bool,
}

/// The Android backend with `apis`: stylus motion events carry pen input;
/// system-gesture exclusion regions reach the decor view's
/// `setSystemGestureExclusionRects`; a touchpad's two-finger scroll and pinch
/// arrive as gesture axes, delivered as phased scrolling and `PanZoom`.
/// Outgoing drags start through `FluiView.startDragAndDrop`; without it every
/// drag stays in its window.
#[must_use]
pub const fn android(apis: AndroidInputApis) -> InputCapabilities {
    let mut builder = InputCapabilities::builder().pen(Native);
    if apis.gesture_exclusion {
        builder = builder.input_regions(Native);
    }
    if apis.touchpad_gestures {
        builder = builder.precise_scroll(Native).trackpad_gestures(Native);
    }
    if apis.drag_source {
        builder = builder.outgoing_drag(Native);
    }
    builder.build()
}

/// The web backend: pointer events carry pen input; wheel events have no
/// phases. A trackpad pinch arrives as WebKit gesture events where the
/// browser has them (`gesture_events`), and otherwise is decoded from
/// Control-held wheel events, which a Control-held mouse wheel can also send
/// (`input::web` decodes it). Every canvas starts HTML drags, at `dragstart`
/// ([`WEB_DRAG_START`]).
#[must_use]
pub const fn web(gesture_events: bool) -> InputCapabilities {
    InputCapabilities::builder()
        .pen(Native)
        .trackpad_gestures(if gesture_events { Native } else { Emulated })
        .outgoing_drag(Native)
        .build()
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

/// The protocols beside `wl_seat` a Wayland compositor offers for input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WaylandInputGlobals {
    /// `zwp_tablet_manager_v2`: pens and other tablet tools.
    pub tablet: bool,
    /// `zwp_pointer_gestures_v1`: a touchpad's pinch.
    pub pointer_gestures: bool,
}

/// The native Wayland backend with the compositor's `globals`: a touchpad's
/// two-finger scrolling arrives as `wl_pointer` axis frames from a finger
/// source with a stop at the lift, delivered as phased precise scrolling;
/// pens arrive through `tablet-v2` and a touchpad's pinch through
/// `pointer-gestures`, where the compositor has them. No system gesture can
/// be excluded, and no drag leaves the window yet (`wl_data_device`).
#[must_use]
pub const fn wayland(globals: WaylandInputGlobals) -> InputCapabilities {
    let mut builder = InputCapabilities::builder().precise_scroll(Native);
    if globals.tablet {
        builder = builder.pen(Native);
    }
    if globals.pointer_gestures {
        builder = builder.trackpad_gestures(Native);
    }
    builder.build()
}

/// The native X11 backend: XInput 2 carries pen input (a tablet's stylus and
/// eraser) and touch, and on a server with XInput 2.4 (`gestures`) a
/// touchpad's pinch; its smooth scrolling has no phases, and there is no
/// outgoing drag yet.
#[must_use]
pub const fn x11(gestures: bool) -> InputCapabilities {
    let builder = InputCapabilities::builder().pen(Native);
    if gestures {
        builder.trackpad_gestures(Native).build()
    } else {
        builder.build()
    }
}

/// When an OS drag session can start on Win32, AppKit, Android and winit: the
/// drag runs in its window and escalates when the pointer leaves it
/// (ADR-0211 §2). Win32 starts them through OLE and Android through
/// `startDragAndDrop` (ADR-0247); the other backends report outgoing drags
/// `Unavailable` until their native session lands.
pub const DESKTOP_DRAG_START: flui_platform::drag::OsDragStart =
    flui_platform::drag::OsDragStart::OnLeavingWindow;

/// When an OS drag session can start on iOS and iPadOS: at lift, through
/// `UIDragInteraction`, and never mid-drag (ADR-0211 §11).
pub const IOS_DRAG_START: flui_platform::drag::OsDragStart =
    flui_platform::drag::OsDragStart::AtStart { eager_data: false };

/// When an OS drag session can start on the web: at `dragstart`, with the
/// representations materialized, since the browser allows no lazy data
/// (ADR-0211 §11).
pub const WEB_DRAG_START: flui_platform::drag::OsDragStart =
    flui_platform::drag::OsDragStart::AtStart { eager_data: true };

#[cfg(test)]
mod tests;
