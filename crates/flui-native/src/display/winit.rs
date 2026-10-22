//! The winit host's displays: the native source of the operating system it
//! runs on, never winit's monitor list, which exists only inside a live event
//! loop and keys monitors by labels that are reused (ADR-0236). On Windows and
//! macOS that source is the Win32 or AppKit host's own, which implements the
//! winit loop's hook beside its own loop's (`display::windows`,
//! `display::apple::macos`); this file is the source on Linux.
//!
//! winit gives no change notice outside its event loop, so on Linux the
//! snapshot comes from the compositor directly (`display::linux`): a
//! connection of its own to the Wayland compositor or X server that winit
//! opened, read by a thread that wakes the owner when the outputs change.
//!
//! A window names its display through its monitor's native number, which is
//! the one identity both sides share: the CRTC on X11
//! (`MonitorHandleExtX11::native_id`) and the `wl_output` global name on
//! Wayland (`MonitorHandleExtWayland::native_id`). Bounds, scale and refresh
//! come from the snapshot, never from the monitor handle.
//!
//! # What reaches the window
//!
//! `Moved`, `ScaleFactorChanged` and `Resized` ask a window whether its display
//! changed and raise its hint if so. A change of the monitor list needs no
//! event of its own: the display thread wakes the owner, the host samples the
//! snapshot again and derives every window's display from it.
//!
//! Wayland sends a window no `Moved`, and winit reports no event when a window
//! crosses to another output of the same scale. Such a move changes the
//! window's display without any hint, so a refresh rate that differs between
//! two same-scale outputs is not followed until another event (a resize, a
//! scale change, a change of the output list) re-derives it. The wl_surface
//! enter and leave events that would tell belong to winit's connection, which
//! this source does not share.

use std::sync::Arc;

use flui_platform::{DisplayId, DisplaySnapshot};
use parking_lot::Mutex;
use raw_window_handle::HasDisplayHandle;
use winit::event_loop::ActiveEventLoop;
use winit::platform::{wayland::MonitorHandleExtWayland, x11::MonitorHandleExtX11};

use crate::display::linux::{
    protocol::Protocol,
    source::{DisplayWatcher, LinuxDisplays},
};
use crate::error::PlatformError;
use crate::owner::signal::OwnerSignal;
use crate::owner::watch::Watch;
use crate::owner::winit::HostDisplays;
use crate::shared::system_notice::StaleOnWake;

/// The platform's outputs, shared with every window.
pub struct WinitDisplaySource {
    native: Arc<LinuxDisplays>,
    /// The connection and thread, and the protocol they read. Dropping the
    /// watcher stops the thread, so it is never dropped under this lock.
    watcher: Mutex<Option<(Protocol, DisplayWatcher)>>,
}

impl std::fmt::Debug for WinitDisplaySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitDisplaySource").finish_non_exhaustive()
    }
}

/// A wake marks the outputs stale.
impl StaleOnWake for WinitDisplaySource {
    fn mark_stale(&self) {
        self.native.mark_stale();
    }
}

impl HostDisplays for WinitDisplaySource {
    /// The current snapshot. Before the event loop runs it is read once from the
    /// compositor the environment names.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.native.snapshot()
    }

    /// The display `window` is on: the one its monitor's native number names in
    /// the last snapshot. `None` for a window with no monitor, or on an output
    /// the snapshot does not hold.
    fn id_of_window(&self, window: &winit::window::Window) -> Option<DisplayId> {
        let protocol = Protocol::of_display(window.display_handle().ok()?.as_raw())?;
        let monitor = window.current_monitor()?;
        let native = match protocol {
            Protocol::X11 => MonitorHandleExtX11::native_id(&monitor),
            Protocol::Wayland => MonitorHandleExtWayland::native_id(&monitor),
        };
        self.native.display_of_native(protocol, native)
    }

    /// Make sure the display thread reads the protocol winit opened (it starts
    /// one on first use); thereafter the thread wakes the owner itself.
    fn resample(&self, event_loop: &ActiveEventLoop, signal: Option<&Arc<OwnerSignal>>) -> bool {
        signal.is_some_and(|signal| self.follow(event_loop, signal))
    }

    /// The display thread, which `resample` starts and wakes the owner from,
    /// as a watch: dropping it stops the thread and closes its connection. The
    /// loop drops it once the owner signal's admission is closed, so a wake
    /// still on its way finds nobody to deliver to.
    fn observe(self: Arc<Self>, _signal: &Arc<OwnerSignal>) -> Watch {
        Watch::new(StopsDisplayThread(self))
    }

    /// Whether windows will be Wayland surfaces: the protocol the watcher reads
    /// once one runs (winit's own choice), else the one the environment names.
    /// A caller that asks before the event loop runs gets the environment's
    /// answer, which is winit's too unless winit was told otherwise.
    fn is_wayland(&self) -> bool {
        match &*self.watcher.lock() {
            Some((protocol, _)) => *protocol == Protocol::Wayland,
            None => Protocol::current() == Some(Protocol::Wayland),
        }
    }
}

/// Stops the source's display thread when dropped.
struct StopsDisplayThread(Arc<WinitDisplaySource>);

impl Drop for StopsDisplayThread {
    fn drop(&mut self) {
        // Taken out first: the watcher joins its thread as it drops, never
        // under the lock.
        let stopped = self.0.watcher.lock().take();
        drop(stopped);
    }
}

impl WinitDisplaySource {
    pub(crate) fn new() -> Self {
        Self {
            native: LinuxDisplays::new(),
            watcher: Mutex::new(None),
        }
    }

    /// Make sure a watcher reads the protocol of `window_system`, the display
    /// winit opened, starting or replacing one if it reads another. Returns
    /// whether a watcher was (re)started, which changes what the snapshot says.
    ///
    /// The environment decides which protocol the source tries first; the
    /// display winit really opened is the authority, and the two differ when
    /// winit was told to use X11 on a Wayland session.
    fn follow(&self, window_system: &impl HasDisplayHandle, signal: &Arc<OwnerSignal>) -> bool {
        let Some(protocol) = window_system
            .display_handle()
            .ok()
            .and_then(|handle| Protocol::of_display(handle.as_raw()))
        else {
            return false;
        };
        let replaced = {
            let mut watcher = self.watcher.lock();
            // A watcher whose thread has ended (the connection was lost or the
            // thread panicked) is replaced like one that never started. This
            // runs only on events (resume, window creation, a scale change), so
            // a server that stays down costs one connect per event, not a loop.
            if watcher
                .as_ref()
                .is_some_and(|(running, live)| *running == protocol && !live.finished())
            {
                return false;
            }
            watcher.take()
        };
        // The old thread is stopped and joined with no lock held.
        drop(replaced);
        match self.native.watch(protocol, signal) {
            Ok(watcher) => {
                *self.watcher.lock() = Some((protocol, watcher));
                true
            }
            Err(error) => {
                tracing::warn!(%error, "the Linux display source could not start");
                // The source now answers with the error; the host reports it.
                true
            }
        }
    }
}
