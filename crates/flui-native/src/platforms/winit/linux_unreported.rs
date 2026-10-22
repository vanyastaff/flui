//! What winit does not report of a window on Linux: the display a window
//! moved to without an event. The mode an X11 window manager gave a window is
//! the window feature's to read (`window::winit`, through the window
//! manager's view).

use winit::event_loop::ActiveEventLoop;

use crate::owner::winit::WinitApp;

impl WinitApp {
    /// Look again at the display of every window winit may have moved without
    /// an event.
    ///
    /// A Wayland surface enters an output with no winit event: none for its
    /// first one, and only a `ScaleFactorChanged` for a later one whose scale
    /// differs, so a window dragged to an output of another refresh rate at
    /// the same scale would keep the old display. The loop goes idle once the
    /// compositor's `enter` has been handled, so every Wayland window is
    /// looked at there, at one monitor lookup each. An X11 window reports a
    /// move with `Moved`, and only one with no display yet is looked at.
    pub(crate) fn sync_unannounced_displays(&self, event_loop: &ActiveEventLoop) {
        let wayland = self.platform.displays.is_wayland();
        let unannounced: Vec<_> = self.platform.with_state(|state| {
            state
                .windows
                .values()
                .filter(|window| moves_unannounced(wayland, window.awaiting_display()))
                .cloned()
                .collect()
        });
        for window in unannounced {
            self.sync_window_display(event_loop, &window, false);
        }
    }
}

/// Whether a Linux window can move to another display without winit saying
/// so: any Wayland window (see `WinitApp::sync_unannounced_displays`), and an
/// X11 window only before its first display is known.
pub(crate) fn moves_unannounced(wayland: bool, awaiting_display: bool) -> bool {
    wayland || awaiting_display
}
