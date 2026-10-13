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

/// The native Win32 backend: pointer messages carry pen input; wheel
/// messages have no phases, and DirectManipulation, input regions and OLE
/// drag sources are not wired yet.
#[must_use]
pub const fn windows() -> InputCapabilities {
    InputCapabilities::builder().pen(Native).build()
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

/// The UIKit backend: Pencil touches carry pen input.
#[must_use]
pub const fn ios() -> InputCapabilities {
    InputCapabilities::builder().pen(Native).build()
}

/// The Android backend: stylus motion events carry pen input.
#[must_use]
pub const fn android() -> InputCapabilities {
    InputCapabilities::builder().pen(Native).build()
}

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
