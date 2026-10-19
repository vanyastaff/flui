//! Taking a Win32 window back to its normal mode: a shown window is restored,
//! a hidden one is put back without being shown.

use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, IsIconic, IsWindowVisible, IsZoomed, SW_HIDE, SW_RESTORE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZE,
};

use super::window::{NativeMode, WindowsWindow};
use super::window_placement::restore_rectangle;

impl WindowsWindow {
    /// Bring the window back to its normal mode from any other: a minimized or
    /// maximized window is restored, and a fullscreen one gets its frame and
    /// bounds back. A hidden window stays hidden.
    pub(super) fn restore_normal(&self) {
        // SAFETY: plain state queries on this wrapper's window handle.
        let (away, shown) = unsafe {
            (
                IsIconic(self.hwnd()).as_bool() || IsZoomed(self.hwnd()).as_bool(),
                IsWindowVisible(self.hwnd()).as_bool(),
            )
        };
        if away && shown {
            self.show_native(SW_RESTORE);
        } else if away {
            self.restore_hidden();
        }
        // A fullscreen window that was minimized comes back fullscreen from the
        // restore above; leaving fullscreen is the second step.
        self.set_fullscreen(false);
    }

    /// Take a hidden maximized or minimized window back to its normal rectangle
    /// without showing it.
    ///
    /// A maximized window loses its state bit and is moved to the rectangle it
    /// returns to. The system will not drop a minimized window's state bit, so
    /// that one is restored and hidden again; the show and the hide are not the
    /// window appearing, and the runtime hears neither.
    fn restore_hidden(&self) {
        let iconic = unsafe { IsIconic(self.hwnd()) }.as_bool();
        if iconic {
            super::platform::with_window_context(self.hwnd(), "restore_hidden", |ctx| {
                let _hold = ctx.hold_publishing();
                let requests = ctx.show_requests.get();
                ctx.visibility_muted.set(true);
                self.show_native(SW_RESTORE);
                // The restore runs user callbacks. If one closed this window and
                // opened another on the handle, or asked for this one to be shown,
                // the hide below is not wanted.
                let overtaken = !self.owns_handle() || ctx.show_requests.get() != requests;
                if !overtaken {
                    self.show_native(SW_HIDE);
                }
                ctx.visibility_muted.set(false);
                if self.owns_handle() {
                    // SAFETY: plain queries of this wrapper's live window.
                    let visible = unsafe {
                        IsWindowVisible(self.hwnd()).as_bool() && !IsIconic(self.hwnd()).as_bool()
                    };
                    ctx.reconcile_visibility(visible);
                }
            });
            return;
        }
        let Some(normal) = restore_rectangle(self.hwnd()) else {
            return;
        };
        // The mode it ends in is set first: the `WM_SIZE` the move sends then
        // sees a window that was not minimized, so it reports no visibility for
        // a window that stays hidden.
        super::platform::with_window_context(self.hwnd(), "restore_hidden", |ctx| {
            ctx.mode.set(NativeMode::Normal);
        });
        // SAFETY: plain style and position calls on this wrapper's window; a
        // stale handle only makes them fail.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd(), GWL_STYLE) as u32;
            SetWindowLongPtrW(self.hwnd(), GWL_STYLE, (style & !WS_MAXIMIZE.0) as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                normal.origin.x,
                normal.origin.y,
                normal.size.width,
                normal.size.height,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOZORDER,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (restore hidden) failed");
            }
        }
    }
}
