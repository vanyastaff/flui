//! The size limits of a Win32 window as the system asks for them: the
//! `WM_GETMINMAXINFO` rectangle and the size a window is about to take.
//!
//! The arithmetic is in [`crate::shared::windows_window_limits`]; this file
//! reads the window (DPI, style) it needs and writes the system's structures.

use flui_foundation::geometry::{Bounds, Size};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, GetWindowRect, MINMAXINFO, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPlacement, SetWindowPos, WINDOWPOS,
    },
};

use super::{platform::WindowContext, window::placement_of};

impl WindowContext {
    /// The app's size limits as outer sizes at the window's current DPI, or
    /// `None` when there are none or the window is fullscreen (it covers its
    /// monitor whatever the limits say).
    fn outer_limits(
        &self,
        hwnd: HWND,
    ) -> Option<crate::shared::windows_window_limits::TrackLimits> {
        if self.mode.get().holds_fullscreen() || self.limits_waived.get() {
            return None;
        }
        // SAFETY: a plain query of the live window the message was sent to.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        self.limits_with_frame_of(hwnd, style)
    }

    /// The app's size limits as outer sizes, whatever mode the window is in,
    /// with the frame measured for `style`: the limits the rectangle a window
    /// returns to must keep. `None` when there are none.
    pub(super) fn limits_with_frame_of(
        &self,
        hwnd: HWND,
        style: u32,
    ) -> Option<crate::shared::windows_window_limits::TrackLimits> {
        use crate::shared::windows_window_limits::{PixelSize, track_limits};

        let constraints = self.constraints.get();
        if constraints.min.is_none() && constraints.max.is_none() {
            return None;
        }
        // SAFETY: plain queries of the live window.
        let (dpi, ex_style) = unsafe {
            (
                windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd),
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32,
            )
        };
        // The frame the window adds around its content at this DPI, measured
        // for the style given, not the rectangle it has.
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

impl super::window::WindowsWindow {
    /// Hold the window to its size limits: the size a normal window has, and
    /// the rectangle any other window returns to.
    pub(super) fn enforce_constraints(&self) {
        let mode =
            super::platform::with_window_context(self.hwnd(), "enforce_constraints", |ctx| {
                ctx.mode.get()
            });
        match mode {
            Some(mode) if mode.is_normal() => self.ask_for_limits_on_current_size(),
            Some(_) => self.clamp_restore_rectangle(),
            None => {}
        }
    }

    /// A size change makes the system ask for the limits: ask with the size the
    /// window already has.
    pub(super) fn ask_for_limits_on_current_size(&self) {
        let mut window = RECT::default();
        // SAFETY: `window` is a stack local out-parameter; `SetWindowPos` takes
        // plain integers and flags for this wrapper's live window.
        unsafe {
            if GetWindowRect(self.hwnd(), &raw mut window).is_err() {
                return;
            }
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                0,
                0,
                window.right - window.left,
                window.bottom - window.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (size limits) failed");
            }
        }
    }

    /// Keep the rectangle a maximized, minimized or fullscreen window returns
    /// to within the limits, so restoring it does not undo what the app set.
    pub(super) fn clamp_restore_rectangle(&self) {
        use crate::shared::windows_window_limits::PixelSize;

        super::platform::with_window_context(self.hwnd(), "clamp_restore_rectangle", |ctx| {
            let mode = ctx.mode.get();
            if let Some(bounds) = mode.fullscreen_bounds() {
                let Some(limits) = ctx.limits_with_frame_of(self.hwnd(), ctx.restore_style.get())
                else {
                    return;
                };
                let size = limits.clamp(PixelSize::new(bounds.size.width, bounds.size.height));
                ctx.mode.set(mode.with_fullscreen_bounds(Bounds {
                    origin: bounds.origin,
                    size: Size::new(size.width, size.height),
                }));
                return;
            }
            // SAFETY: a plain style query of this wrapper's window handle.
            let style = unsafe { GetWindowLongPtrW(self.hwnd(), GWL_STYLE) } as u32;
            let Some(limits) = ctx.limits_with_frame_of(self.hwnd(), style) else {
                return;
            };
            let Ok(mut placement) = placement_of(self.hwnd()) else {
                return;
            };
            let normal = &mut placement.rcNormalPosition;
            let size = limits.clamp(PixelSize::new(
                normal.right.saturating_sub(normal.left),
                normal.bottom.saturating_sub(normal.top),
            ));
            normal.right = normal.left.saturating_add(size.width);
            normal.bottom = normal.top.saturating_add(size.height);
            // SAFETY: `placement` is fully initialized; a stale handle only makes
            // the call fail.
            if let Err(error) = unsafe { SetWindowPlacement(self.hwnd(), &raw const placement) } {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "the restore rectangle could not be limited");
            }
        });
    }
}
