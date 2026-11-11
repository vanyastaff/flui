//! The display connections of the hosts whose window system has no connection
//! object (ADR-0305 §2). X11 and Wayland hand out their own, next to their
//! window targets.

use flui_platform::DisplayConnection;
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle};

#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32"
))]
pub(crate) use pointerless::PointerlessDisplay;

#[cfg(any(
    windows,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32"
))]
mod pointerless {
    use flui_platform::DisplayConnection;
    use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle};

    /// The display of a window system whose display handle carries no
    /// pointer (Win32, AppKit, UIKit, Android, the web): the function makes
    /// it, and it answers it on any thread for as long as it is held.
    pub(crate) struct PointerlessDisplay(pub(crate) fn() -> RawDisplayHandle);

    impl HasDisplayHandle for PointerlessDisplay {
        #[expect(
            unsafe_code,
            reason = "a display handle with no pointer is borrowed for no lifetime of its own"
        )]
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            // SAFETY: every handle a host passes here carries no fields (each
            // host's surface target answers the same one), so there is
            // nothing for `borrow_raw` to keep valid.
            Ok(unsafe { DisplayHandle::borrow_raw((self.0)()) })
        }
    }

    impl DisplayConnection for PointerlessDisplay {}
}

/// The display of a host with no window system (headless, and a host without
/// windows): it answers none, so a GPU instance made over it makes no window
/// surface.
pub(crate) struct NoDisplay;

impl HasDisplayHandle for NoDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::NotSupported)
    }
}

impl DisplayConnection for NoDisplay {}
