//! The winit backend's display source on macOS.
//!
//! winit enumerates monitors only through a live event loop and keys them by a
//! `CGDirectDisplayID`. On macOS the snapshot therefore comes from the native
//! source the AppKit backend uses (`platforms::macos::display`): it reads
//! `NSScreen.screens`, keys each screen by its display UUID, takes the refresh
//! period from the display's current mode and names a window's display by the
//! window's `NSScreen`. winit's own monitor list is not read.
//!
//! The native source is invalidated by
//! `NSApplicationDidChangeScreenParametersNotification` (a
//! `ScreenObserver` the event-loop owner installs on resume) and by a window
//! whose screen is one the last sample lacks.

use std::sync::Arc;

use flui_platform::{DisplayId, DisplaySnapshot};
use objc2::MainThreadMarker;
use objc2_app_kit::NSView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::super::macos::display::MacosDisplays;
use crate::error::PlatformError;

/// The platform's screens, shared with every window.
pub struct WinitDisplaySource {
    native: Arc<MacosDisplays>,
}

impl std::fmt::Debug for WinitDisplaySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitDisplaySource").finish_non_exhaustive()
    }
}

impl WinitDisplaySource {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            native: Arc::new(MacosDisplays::new()),
        })
    }

    /// The native source, which the screen observer invalidates.
    pub(crate) fn native(&self) -> &Arc<MacosDisplays> {
        &self.native
    }

    /// The current snapshot: the last sample unless something invalidated it.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] off the main thread, where `NSScreen` is not
    /// read, or when the snapshot revision space is used up.
    pub(crate) fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Displays {
            message: "the screens are read on the main thread".into(),
        })?;
        self.native.snapshot(mtm)
    }

    /// The display `window` is on, by the `NSScreen` of its native window.
    /// `None` off the main thread, for a window on no screen, and for a screen
    /// the source leaves out.
    pub(super) fn id_of_window(&self, window: &winit::window::Window) -> Option<DisplayId> {
        // `NSView` is main-thread state.
        MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit's AppKit handle is the window's content `NSView`, which
        // the borrowed `window` keeps alive for this call, and the main-thread
        // check above makes the message send legal.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let screen = view.window()?.screen()?;
        self.native.id_of(&screen)
    }
}
