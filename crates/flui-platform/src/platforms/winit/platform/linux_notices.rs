//! The parts of the winit platform that exist on Linux only: the X11 window
//! probe, and the system notices (sleep, wake, lock, shutdown, memory
//! pressure) the logind and kernel sources queue for the owner.

use std::sync::Arc;

use flui_platform_api::{SessionEndAnswer, SessionEndPhase};

use super::{WinitApp, WinitPlatform};
use crate::platforms::linux::{
    host_notices::{HostEvent, HostNoticeSource},
    x11_window_probe::X11WindowProbe,
};

impl WinitPlatform {
    /// One phase of a session end: the hook the host registered is asked and
    /// its answer returned; [`SessionEndAnswer::Proceed`] when none is
    /// registered. The hook runs outside the state lock (ADR-0039) and is
    /// restored unless a newer one replaced it meanwhile.
    pub(super) fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(mut hook) = self.with_state(|state| state.handlers.session_end.take()) else {
            return SessionEndAnswer::Proceed;
        };
        // The hook goes back even if it panicked: a later session end still
        // reaches the host.
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(phase)));
        self.with_state(|state| {
            if state.handlers.session_end.is_none() {
                state.handlers.session_end = Some(hook);
            }
        });
        answer.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    }

    /// The X11 window probe, started on first use. `None` when the platform
    /// is not running, or the X server cannot be reached again; windows then
    /// report what winit does.
    pub(super) fn x11_probe(&self) -> Option<Arc<X11WindowProbe>> {
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
    pub(super) fn refresh_x11_windows(&self) {
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

    /// Start listening for sleep, shutdown, lock and memory pressure. A source
    /// that cannot start says so in the log and leaves the others running.
    pub(super) fn start_host_notices(&mut self) {
        if self.host_notices.is_some() {
            return;
        }
        let Some(signal) = self.platform.owner_signal.lock().clone() else {
            return;
        };
        let executor = self
            .platform
            .with_state(|state| Arc::clone(&state.background_executor));
        self.host_notices = Some(HostNoticeSource::start(&executor, &signal));
    }

    /// Deliver the notices the Linux sources have queued, oldest first. The
    /// source holds a sleep or shutdown until its event has been delivered, so
    /// each runs to the end here before the next.
    pub(super) fn deliver_host_notices(&self) {
        let Some(source) = &self.host_notices else {
            return;
        };
        for delivery in source.take() {
            delivery.deliver_with(|event| match event {
                HostEvent::Notice(notice) => self.platform.notices.dispatch(notice),
                // Linux cannot hold a shutdown for the user, so the answer to
                // the query is not read; the application hears the query so
                // that it can start saving, then the shutdown.
                HostEvent::SessionEnding => {
                    self.platform.session_end(SessionEndPhase::Query);
                    self.platform.session_end(SessionEndPhase::Ending);
                }
                HostEvent::SessionEndCancelled => {
                    self.platform.session_end(SessionEndPhase::Cancelled);
                }
            });
        }
    }
}
