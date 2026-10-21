//! The parts of the winit platform that exist on Linux only: the X11 window
//! probe, the display check for moves winit does not report, and the system
//! notices (sleep, wake, lock, shutdown, memory pressure) the logind and kernel
//! sources queue for the owner.

use std::sync::Arc;

use flui_foundation::panic::FirstPanic;
use flui_platform::{SessionEndAnswer, SessionEndPhase};
use winit::event_loop::ActiveEventLoop;

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
    pub(super) fn sync_unannounced_displays(&self, event_loop: &ActiveEventLoop) {
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
pub(super) fn moves_unannounced(wayland: bool, awaiting_display: bool) -> bool {
    wayland || awaiting_display
}

impl WinitApp {
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
        // A subscriber that panics must not keep the rest from hearing, nor
        // keep logind waiting for the events behind it: every delivery runs,
        // and the first panic is raised once all have.
        let mut panics = FirstPanic::new();
        for delivery in source.take() {
            panics.contain(|| {
                delivery.deliver_with(|event| match event {
                    HostEvent::Notice(notice) => self.platform.notices.dispatch(notice),
                    // Linux cannot hold a shutdown for the user, so the answer
                    // to the query is not read; the application hears the
                    // query so that it can start saving, then the shutdown.
                    HostEvent::SessionEnding => {
                        let mut phases = FirstPanic::new();
                        let answer =
                            phases.contain(|| self.platform.session_end(SessionEndPhase::Query));
                        if answer == Some(SessionEndAnswer::Block) {
                            tracing::info!(
                                "the application asked to hold the shutdown; logind cannot wait for the user, so it goes on"
                            );
                        }
                        phases.contain(|| self.platform.session_end(SessionEndPhase::Ending));
                        phases.resume();
                    }
                    HostEvent::SessionEndCancelled => {
                        self.platform.session_end(SessionEndPhase::Cancelled);
                    }
                });
            });
        }
        panics.resume();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use parking_lot::Mutex;

    use super::*;
    use crate::traits::Platform;

    type Phases = Arc<Mutex<Vec<SessionEndPhase>>>;

    /// A hook that records each phase it is asked and blocks.
    fn recording_hook() -> (Phases, impl FnMut(SessionEndPhase) -> SessionEndAnswer) {
        let phases = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&phases);
        (phases, move |phase| {
            log.lock().push(phase);
            SessionEndAnswer::Block
        })
    }

    #[test]
    fn session_end_without_a_hook_proceeds() {
        let platform = WinitPlatform::new();
        assert_eq!(
            platform.session_end(SessionEndPhase::Query),
            SessionEndAnswer::Proceed
        );
    }

    #[test]
    fn session_end_asks_the_hook_each_phase_and_returns_its_answer() {
        let platform = WinitPlatform::new();
        let (phases, hook) = recording_hook();
        platform.on_session_end(Box::new(hook));
        for phase in [
            SessionEndPhase::Query,
            SessionEndPhase::Ending,
            SessionEndPhase::Cancelled,
        ] {
            assert_eq!(platform.session_end(phase), SessionEndAnswer::Block);
        }
        assert_eq!(
            *phases.lock(),
            [
                SessionEndPhase::Query,
                SessionEndPhase::Ending,
                SessionEndPhase::Cancelled
            ]
        );
    }

    #[test]
    fn a_session_end_hook_that_panics_stays_installed() {
        let platform = WinitPlatform::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        platform.on_session_end(Box::new(move |_| {
            assert!(
                counter.fetch_add(1, Ordering::SeqCst) != 0,
                "the first phase panics"
            );
            SessionEndAnswer::Proceed
        }));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            platform.session_end(SessionEndPhase::Query);
        }));
        assert!(outcome.is_err(), "the panic reaches the caller");

        platform.session_end(SessionEndPhase::Ending);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the hook heard the next phase"
        );
    }

    #[test]
    fn a_hook_registered_during_a_phase_is_not_overwritten() {
        let platform = Arc::new(WinitPlatform::new());
        let (phases, newer) = recording_hook();
        let newer = Mutex::new(Some(newer));
        let during = Arc::clone(&platform);
        platform.on_session_end(Box::new(move |_| {
            let newer = newer.lock().take();
            if let Some(newer) = newer {
                during.on_session_end(Box::new(newer));
            }
            SessionEndAnswer::Proceed
        }));

        platform.session_end(SessionEndPhase::Query);
        platform.session_end(SessionEndPhase::Ending);
        assert_eq!(
            *phases.lock(),
            [SessionEndPhase::Ending],
            "the hook installed meanwhile took the next phase"
        );
    }
}
