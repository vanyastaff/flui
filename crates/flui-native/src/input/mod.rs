//! Input (ADR-0151 §10): the model every platform decodes its input with,
//! beside one file per platform.
//!
//! The model is what two or more platforms share: the pointer assembly
//! ([`assembly`], ADR-0217 §3), the key rules ([`keys`]: Win32's tables, which
//! the winit host on Windows reads too, the adapter from `keyboard-types`
//! and the one table of key locations), the availability table
//! ([`availability`], ADR-0213), the input-region projection (`regions`,
//! ADR-0217 §5), the one wheel step (`scroll`) and the event
//! timeline below. Every
//! backend stamps against the one process epoch,
//! [`flui_foundation::event_epoch`], which the interaction clock also counts
//! from (ADR-0207 §2): timestamps from different backends, and the deadlines
//! the arena arms from them, share one timeline.
//!
//! The platform files are `windows` (the window class's input hook with each
//! window's input, the messages' decoding in `raw` and the precision touchpad
//! in `direct_manipulation`), `apple`, `android`, `web`, `linux` (what the
//! Wayland and X11 hosts share, the XKB keyboard, beside the Wayland seats
//! and the X11 host's XInput 2), `winit` and `headless`. A platform file's
//! pure half is a module inside it, compiled and tested on every host.
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
pub(crate) mod regions;
pub(crate) mod scroll;

#[doc(hidden)]
pub mod android;
#[doc(hidden)]
pub mod apple;
pub(crate) mod headless;
#[doc(hidden)]
pub mod linux;
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
