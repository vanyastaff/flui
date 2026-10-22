//! Input (ADR-0151 §10): the model every platform decodes its input with,
//! beside one file per platform.
//!
//! The model is what two or more platforms share: the pointer assembly
//! ([`assembly`], ADR-0217 §3), the key rules ([`keys`]: Win32's tables, which
//! the winit host on Windows reads too, and the adapter from
//! `keyboard-types`), the availability table ([`availability`], ADR-0213),
//! wheel normalization ([`scroll`]) and the event timeline below. Every
//! backend stamps against the one process epoch,
//! [`flui_foundation::event_epoch`], which the interaction clock also counts
//! from (ADR-0207 §2): timestamps from different backends, and the deadlines
//! the arena arms from them, share one timeline.
//!
//! The platform files are `windows` (the window class's input messages, their
//! decoding in `raw` and the precision touchpad in `direct_manipulation`),
//! `apple`, `android`, `web`, `winit` and `headless`. A platform file's pure
//! half is a module inside it, compiled and tested on every host.
//!
//! The model and the pure halves the backend conformance table and the native
//! recordings in `tests/` decode with are public and hidden from the
//! documentation; nothing outside the crate's own tests names them.

use flui_platform::pointer::{PointerId, PointerInfo, PointerKind, PointerRole};

#[doc(hidden)]
pub mod assembly;
#[doc(hidden)]
pub mod availability;
#[doc(hidden)]
pub mod keys;
#[doc(hidden)]
pub mod scroll;

#[doc(hidden)]
pub mod android;
#[doc(hidden)]
pub mod apple;
pub(crate) mod headless;
#[doc(hidden)]
pub mod web;
#[doc(hidden)]
pub mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

/// Nanoseconds since the process event epoch.
#[inline]
pub fn event_timestamp_ns() -> u64 {
    // EventTime is nanoseconds; every producer shares this epoch.
    #[expect(clippy::cast_possible_truncation)] // ~584 years of nanoseconds fit u64
    {
        flui_foundation::event_epoch().elapsed().as_nanos() as u64
    }
}

/// Create a `PointerInfo` for the primary mouse pointer.
#[inline]
#[must_use]
pub(crate) fn primary_mouse_info() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(1_u64).expect("BUG: primary mouse identity is nonzero"),
        PointerKind::Mouse,
    )
    .with_role(PointerRole::Primary)
}

/// What the other features of a window read of its input, on the window's
/// owner thread: whether the mouse is over it, the modifiers its last key
/// carried, and the registry its pointer contacts are assembled in. The Win32
/// input keeps one for each window of FLUI's class beside its own state
/// (`owner::windows::WindowRef::attach`), where the class's procedure is
/// shared between features: the window reads the hover and the modifiers,
/// drag and drop the kind of the contact a drag began with, and haptics the
/// pen in range.
#[cfg(windows)]
pub(crate) struct WindowInput {
    /// Whether the mouse is over the window.
    pub(crate) hovered: std::cell::Cell<bool>,
    /// The modifiers held at the window's last key.
    pub(crate) modifiers: std::cell::Cell<flui_platform::Modifiers>,
    /// The window's pointer contacts and devices.
    pub(crate) registry: std::cell::RefCell<assembly::ContactRegistry>,
}
