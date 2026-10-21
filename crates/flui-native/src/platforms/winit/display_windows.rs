//! The winit backend's display source on Windows.
//!
//! winit enumerates monitors only through a live event loop, and keys them by a
//! reusable GDI device name. On Windows the snapshot therefore comes from the
//! native source the Win32 backend uses (`platforms::windows::display`), which
//! needs no event loop: it keys monitors by display target, reads the
//! refresh period from `QueryDisplayConfig` and names a window's display by
//! `MonitorFromWindow`. winit's own monitor list is not read.
//!
//! The native source is invalidated by `WM_DISPLAYCHANGE` on the hidden settings
//! receiver (created on resume with the preference source) and re-sampled where
//! winit reports a move, a scale change or a resume.

use std::sync::Arc;

use flui_platform::{DisplayId, DisplaySnapshot};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;

use super::super::windows::display::WindowsDisplays;
use crate::error::PlatformError;

/// The platform's native monitors, shared with every window.
pub(super) struct WinitDisplaySource {
    native: Arc<WindowsDisplays>,
}

impl WinitDisplaySource {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            native: Arc::new(WindowsDisplays::new()),
        })
    }

    /// The native source, which the settings receiver invalidates.
    pub(super) fn native(&self) -> &Arc<WindowsDisplays> {
        &self.native
    }

    /// Sample the monitors now. Returns whether the snapshot changed; a revision
    /// that cannot advance keeps the previous sample.
    pub(super) fn resample(&self) -> bool {
        match self.native.resample_changed() {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!(%error, "display revisions exhausted; keeping the last snapshot");
                false
            }
        }
    }

    /// The current snapshot: the last sample unless something invalidated it.
    pub(super) fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.native.snapshot()
    }

    /// The display `window` is on, by the monitor nearest its native window.
    pub(super) fn id_of_window(&self, window: &winit::window::Window) -> Option<DisplayId> {
        let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        self.native
            .display_of_window(HWND(handle.hwnd.get() as *mut _))
    }
}
