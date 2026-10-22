//! What winit does not report of a window on Linux: the mode an X11 window
//! manager gave it (read through the X11 window probe), and the display a
//! window moved to without an event.

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;

use crate::owner::winit::{WinitApp, WinitLoop};
use crate::platforms::linux::x11_window_probe::X11WindowProbe;

impl WinitLoop {
    /// The X11 window probe, started on first use. `None` when the platform
    /// is not running, or the X server cannot be reached again; windows then
    /// report what winit does.
    pub(crate) fn x11_probe(&self) -> Option<Arc<X11WindowProbe>> {
        let signal = self.owner_signal.lock().clone()?;
        self.x11_probe
            .get_or_init(|| match X11WindowProbe::connect(&signal) {
                Ok(probe) => Some(probe),
                Err(error) => {
                    tracing::debug!(%error, "window state is read from winit alone");
                    None
                }
            })
            .clone()
    }
}

impl WinitApp {
    /// Sample again the windows whose mode properties the X11 window manager
    /// changed, so `on_state_changed` follows a minimize or a maximize that
    /// winit reports no event for.
    pub(crate) fn refresh_x11_windows(&self) {
        let Some(Some(probe)) = self.platform.x11_probe.get() else {
            return;
        };
        let changed = probe.take_changed();
        if changed.is_empty() {
            return;
        }
        let windows: Vec<_> = self.platform.with_state(|state| {
            state
                .windows
                .values()
                .filter(|window| window.x11_id().is_some_and(|id| changed.contains(&id)))
                .cloned()
                .collect()
        });
        for window in windows {
            window.refresh_state();
        }
    }

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
