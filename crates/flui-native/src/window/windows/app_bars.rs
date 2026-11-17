//! The auto-hide app bars a maximized Win32 window without the system's
//! title bar leaves their edge (ADR-0286 §7): what the shell answers for a
//! monitor, the window's keeping of that answer, where a window without a
//! frame is maximized, and measuring a maximized window again when the bars
//! change. The rules are the pure half's (`chrome::leave_auto_hide_bars`,
//! `chrome::maximized_area`); `proc` takes them in `WM_NCCALCSIZE`,
//! `WM_GETMINMAXINFO`, `WM_SETTINGCHANGE` and `WM_DISPLAYCHANGE`.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsZoomed, WS_CAPTION, WS_THICKFRAME,
};

use super::chrome::{AutoHideEdges, Edges, maximized_area};
use super::proc::{WindowContext, edges};

/// The auto-hide app bars of one monitor, as the shell answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MonitorAppBars {
    /// The monitor's handle as an integer: compared, never dereferenced.
    pub(crate) monitor: isize,
    pub(crate) edges: AutoHideEdges,
}

/// The monitor `rect` lies on most (the nearest when it lies on none), as a
/// handle's integer and the monitor's rectangle; `None` when the monitor
/// cannot be read.
pub(super) fn monitor_at(rect: &RECT) -> Option<(isize, RECT)> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `rect` is a live reference read by the call; `info` is a stack
    // local with `cbSize` set, which the call fills.
    unsafe {
        let monitor = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &raw mut info)
            .as_bool()
            .then_some((monitor.0 as isize, info.rcMonitor))
    }
}

/// The edges of the monitor `monitor` (its rectangle) that hold an app bar set
/// to hide itself, as the shell answers: none at all unless the taskbar hides
/// itself (`ABM_GETSTATE`), else each edge's `ABM_GETAUTOHIDEBAREX`. An edge
/// the shell cannot answer for (no shell) reads as holding none.
///
/// Each answer is a message the shell's taskbar answers from another process,
/// so the thread may run a message sent to one of its windows meanwhile, and
/// a hung shell holds the caller until the system gives up on it; the window
/// asks once per monitor until the settings or the displays change
/// ([`WindowContext::auto_hide_edges_at`]), and holds no borrow across it.
fn shell_auto_hide_edges(monitor: RECT) -> AutoHideEdges {
    use windows::Win32::UI::Shell::{
        ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, ABM_GETSTATE, ABS_AUTOHIDE,
        APPBARDATA, SHAppBarMessage,
    };

    let ask = |message: u32, edge: u32| {
        let mut query = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            uEdge: edge,
            rc: monitor,
            ..Default::default()
        };
        // SAFETY: `query` is a stack local with `cbSize` set, read by the call
        // and not kept; the answer is a plain value.
        unsafe { SHAppBarMessage(message, &raw mut query) }
    };
    if ask(ABM_GETSTATE, 0) & ABS_AUTOHIDE as usize == 0 {
        return AutoHideEdges::default();
    }
    let holds_bar = |edge: u32| ask(ABM_GETAUTOHIDEBAREX, edge) != 0;
    AutoHideEdges {
        left: holds_bar(ABE_LEFT),
        top: holds_bar(ABE_TOP),
        right: holds_bar(ABE_RIGHT),
        bottom: holds_bar(ABE_BOTTOM),
    }
}

impl WindowContext {
    /// The edges holding an auto-hide app bar of the monitor `rect` lies on:
    /// the ones the window measured for that monitor, or the shell's answer,
    /// which it keeps until the settings or the displays change.
    pub(crate) fn auto_hide_edges_at(&self, rect: &RECT) -> AutoHideEdges {
        let Some((monitor, bounds)) = monitor_at(rect) else {
            return AutoHideEdges::default();
        };
        if let Some(bars) = self.app_bars.get().filter(|bars| bars.monitor == monitor) {
            return bars.edges;
        }
        let edges = shell_auto_hide_edges(bounds);
        self.app_bars.set(Some(MonitorAppBars { monitor, edges }));
        edges
    }

    /// The monitor of a window without a frame (no caption, no sizing border)
    /// and where the window is maximized on it, relative to the monitor's
    /// top-left corner as `MINMAXINFO::ptMaxPosition` takes it: the monitor's
    /// work area, standing back from each edge that holds an auto-hide app bar
    /// ([`maximized_area`]). Left to itself, the system maximizes such a
    /// window over the whole monitor, taskbar included. `None` for a window
    /// with a frame, which the system maximizes to the work area itself, for
    /// a fullscreen one, which covers its monitor, or when the monitor cannot
    /// be read.
    pub(super) fn frameless_maximized_area(&self, hwnd: HWND) -> Option<(Edges, Edges)> {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        };

        // SAFETY: a plain style query of the live window.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        if style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0 || !self.chrome_answers_the_hit_test() {
            return None;
        }
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `monitor` is a stack local with `cbSize` set; the monitor
        // handle comes from the live window.
        let read = unsafe {
            GetMonitorInfoW(
                MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                &raw mut monitor,
            )
        };
        if !read.as_bool() {
            return None;
        }
        let auto_hide = self.auto_hide_edges_at(&monitor.rcMonitor);
        let bounds = edges(monitor.rcMonitor);
        Some((
            bounds,
            maximized_area(bounds, edges(monitor.rcWork), auto_hide),
        ))
    }

    /// The settings or the displays changed, so the app bars may have: an
    /// auto-hide taskbar turned on or off, or moved to another edge, which
    /// leaves the work area the whole monitor, so the system does not size
    /// the window again. The window forgets what it measured, and a maximized
    /// window without the system's title bar is measured again when its
    /// monitor's bars changed.
    pub(crate) fn app_bars_may_have_changed(&self, hwnd: HWND) {
        use windows::Win32::UI::WindowsAndMessaging::{
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowPos,
        };

        let measured = self.app_bars.take();
        // SAFETY: a plain query of the live window the message was sent to.
        let maximized = unsafe { IsZoomed(hwnd) }.as_bool();
        let Some(measured) = measured.filter(|_| maximized && self.chrome_answers_the_hit_test())
        else {
            return;
        };
        let mut window = RECT::default();
        // SAFETY: `window` is a stack local out-parameter.
        if unsafe { GetWindowRect(hwnd, &raw mut window) }.is_err()
            || self.auto_hide_edges_at(&window) == measured.edges
        {
            return;
        }
        let keep = SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;
        // A window without a frame takes the area it is maximized to again;
        // one with an application's title bar measures its client again
        // (the frame change sends `WM_NCCALCSIZE`).
        let (rect, flags) = match self.frameless_maximized_area(hwnd) {
            Some((monitor, area)) => (
                RECT {
                    left: monitor.left.saturating_add(area.left),
                    top: monitor.top.saturating_add(area.top),
                    right: monitor.left.saturating_add(area.right),
                    bottom: monitor.top.saturating_add(area.bottom),
                },
                keep,
            ),
            None => (
                RECT::default(),
                keep | SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE,
            ),
        };
        // SAFETY: plain integer and flag arguments on the live window, with
        // `None` as the documented no-op z-order handle.
        let placed = unsafe {
            SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right.saturating_sub(rect.left),
                rect.bottom.saturating_sub(rect.top),
                flags,
            )
        };
        if let Err(error) = placed {
            tracing::warn!(handle = ?hwnd, ?error, "SetWindowPos (app bars changed) failed");
        }
    }
}
