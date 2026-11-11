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
    /// pointer: each variant names one, and it answers it on any thread for
    /// as long as it is held. Only these handles can be made, so the borrow
    /// below has nothing to keep valid by construction.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum PointerlessDisplay {
        /// Win32.
        #[cfg(windows)]
        Windows,
        /// AppKit.
        #[cfg(target_os = "macos")]
        AppKit,
        /// UIKit.
        #[cfg(target_os = "ios")]
        UiKit,
        /// Android.
        #[cfg(target_os = "android")]
        Android,
        /// The web.
        #[cfg(target_arch = "wasm32")]
        Web,
    }

    impl PointerlessDisplay {
        fn raw(self) -> RawDisplayHandle {
            match self {
                #[cfg(windows)]
                Self::Windows => {
                    RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new())
                }
                #[cfg(target_os = "macos")]
                Self::AppKit => {
                    RawDisplayHandle::AppKit(raw_window_handle::AppKitDisplayHandle::new())
                }
                #[cfg(target_os = "ios")]
                Self::UiKit => {
                    RawDisplayHandle::UiKit(raw_window_handle::UiKitDisplayHandle::new())
                }
                #[cfg(target_os = "android")]
                Self::Android => {
                    RawDisplayHandle::Android(raw_window_handle::AndroidDisplayHandle::new())
                }
                #[cfg(target_arch = "wasm32")]
                Self::Web => RawDisplayHandle::Web(raw_window_handle::WebDisplayHandle::new()),
            }
        }
    }

    impl HasDisplayHandle for PointerlessDisplay {
        #[expect(
            unsafe_code,
            reason = "a display handle with no pointer is borrowed for no lifetime of its own"
        )]
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            // SAFETY: `raw` makes only the Win32, AppKit, UIKit, Android and
            // web display handles, none of which carries a field, so there
            // is nothing for `borrow_raw` to keep valid.
            Ok(unsafe { DisplayHandle::borrow_raw(self.raw()) })
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
