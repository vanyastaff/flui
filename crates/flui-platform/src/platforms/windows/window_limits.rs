//! The size limits of a Win32 window as the system asks for them: the
//! `WM_GETMINMAXINFO` rectangle and the size a window is about to take.
//!
//! The arithmetic is in [`crate::shared::windows_window_limits`]; this file
//! reads the window (DPI, style) it needs and writes the system's structures.

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, MINMAXINFO, SWP_NOSIZE, WINDOWPOS,
    },
};

use super::platform::WindowContext;

impl WindowContext {
    /// The app's size limits as outer sizes at the window's current DPI, or
    /// `None` when there are none or the window is fullscreen (it covers its
    /// monitor whatever the limits say).
    fn outer_limits(
        &self,
        hwnd: HWND,
    ) -> Option<crate::shared::windows_window_limits::TrackLimits> {
        use crate::shared::windows_window_limits::{PixelSize, track_limits};

        let constraints = self.constraints.get();
        if (constraints.min.is_none() && constraints.max.is_none())
            || self.mode.get().holds_fullscreen()
            || self.limits_waived.get()
        {
            return None;
        }
        // SAFETY: plain queries of the live window the message was sent to.
        let (dpi, style, ex_style) = unsafe {
            (
                windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd),
                GetWindowLongPtrW(hwnd, GWL_STYLE) as u32,
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32,
            )
        };
        // The frame the window adds around its content at this DPI, measured
        // for the style it has, not the rectangle it has.
        let (frame_width, frame_height) =
            super::window::outer_size(style, ex_style, dpi, (0, 0)).unwrap_or((0, 0));
        Some(track_limits(
            constraints.min,
            constraints.max,
            dpi,
            PixelSize::new(frame_width, frame_height),
        ))
    }

    /// Fill in `WM_GETMINMAXINFO` with the app's size limits at the window's
    /// current DPI. A fullscreen window keeps the system's rectangle.
    pub(super) fn fill_min_max_info(&self, hwnd: HWND, info: &mut MINMAXINFO) {
        use crate::shared::windows_window_limits::{MinMaxInfo, PixelSize, constrain};

        let Some(limits) = self.outer_limits(hwnd) else {
            return;
        };
        let os = MinMaxInfo {
            max_size: PixelSize::new(info.ptMaxSize.x, info.ptMaxSize.y),
            min_track: PixelSize::new(info.ptMinTrackSize.x, info.ptMinTrackSize.y),
            max_track: PixelSize::new(info.ptMaxTrackSize.x, info.ptMaxTrackSize.y),
        };
        let constrained = constrain(os, limits);
        info.ptMaxSize.x = constrained.max_size.width;
        info.ptMaxSize.y = constrained.max_size.height;
        info.ptMinTrackSize.x = constrained.min_track.width;
        info.ptMinTrackSize.y = constrained.min_track.height;
        info.ptMaxTrackSize.x = constrained.max_track.width;
        info.ptMaxTrackSize.y = constrained.max_track.height;
    }

    /// Keep a size the window is about to take within the app's limits.
    ///
    /// `DefWindowProc` asks for `WM_GETMINMAXINFO` while sizing only windows
    /// with a sizing border; a window without one (undecorated, or not
    /// resizable) is held to its limits here instead.
    pub(super) fn clamp_pending_size(&self, hwnd: HWND, pos: &mut WINDOWPOS) {
        if !self.mode.get().is_normal() || pos.flags.0 & SWP_NOSIZE.0 != 0 {
            return;
        }
        let Some(limits) = self.outer_limits(hwnd) else {
            return;
        };
        if let Some(min) = limits.min {
            pos.cx = pos.cx.max(min.width);
            pos.cy = pos.cy.max(min.height);
        }
        if let Some(max) = limits.max {
            pos.cx = pos.cx.min(max.width);
            pos.cy = pos.cy.min(max.height);
        }
    }
}
