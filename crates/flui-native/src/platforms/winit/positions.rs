//! What each position of the winit host's table answers.
//!
//! The table names the types the host's windows and loop hand out
//! ([`WinitWindow`], …), but every answer is read from the loop's state: its
//! one lock, its run state, its owner signal.

use std::{cell::Cell, sync::Arc, thread};

use flui_platform::{
    SessionEndAnswer, SessionEndPhase, SystemNotice, SystemPreferences, WindowCapabilities,
    WindowEvent, WindowId, WindowOptions,
};

#[cfg(windows)]
use crate::owner::winit::PreferenceObservation;
use crate::owner::winit::{ACTIVE_EVENT_LOOP, OpenWindowStateError, WinitLoop};
use crate::{
    error::PlatformError, host, host_window::HostWindow, owner::OpenWindowError,
    owner::call_point::install, owner::winit::control::ControlSendError,
    platforms::winit::WinitWindow,
};
use flui_foundation::{ClaimOutcome, geometry::Size};

impl host::Window<WinitLoop> for WinitWindow {
    fn open(
        owner: &WinitLoop,
        options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(?options, "Requesting window creation");

        // Same-thread fast path: called synchronously from inside `on_ready`
        // (see `WinitApp::resumed`), where a live `ActiveEventLoop` is
        // published for exactly this nested call. Create the window
        // directly instead of enqueuing — the lane's `user_event` consumer
        // cannot run until this call returns, and would
        // deadlock forever otherwise (see `owner/winit.rs`'s module doc).
        if let Some(event_loop_ptr) = ACTIVE_EVENT_LOOP.with(Cell::get) {
            tracing::debug!("Creating window on event-loop thread (same-thread fast path)");
            // SAFETY: `event_loop_ptr` is non-null only while
            // `with_active_event_loop` has published it on THIS thread, for
            // the exact nested `on_ready` call currently executing (see
            // `WinitApp::resumed`). The referent is `resumed`'s live
            // `&ActiveEventLoop` parameter, which outlives that whole call —
            // and this call is strictly nested inside it — so the reference
            // constructed here is valid for the borrow's entire duration.
            let event_loop = unsafe { event_loop_ptr.as_ref() };
            let window_id = owner.create_window_now(event_loop, options)?;
            return owner.window_by_id(window_id);
        }

        let control = match owner.with_state(|state| {
            state
                .run_state
                .control_for_open_window(thread::current().id())
        }) {
            Ok(control) => control,
            // The loop has stopped: the owner is genuinely gone, and the
            // untouched options ride back so the caller can retry against a
            // fresh platform without rebuilding them.
            Err(OpenWindowStateError::Stopped) => {
                return Err(OpenWindowError::OwnerGone {
                    rejected: Some(Box::new(options)),
                });
            }
            // Lifecycle refusals: the loop is not (yet) in a state that can
            // service this call site — typed as `Unavailable`, distinct
            // from a dead owner.
            Err(state_error) => {
                return Err(OpenWindowError::Unavailable {
                    message: match state_error {
                        OpenWindowStateError::NotRunning => {
                            "open_window called before Platform::run — on the winit backend, \
                             create the first windows inside run()'s on_ready callback"
                        }
                        OpenWindowStateError::Starting => {
                            "open_window called from another thread before winit finished \
                             on_ready"
                        }
                        OpenWindowStateError::OwnerWouldBlock => {
                            "open_window cannot block the winit event-loop owner outside \
                             on_ready"
                        }
                        OpenWindowStateError::Stopped => unreachable!("BUG: handled above"),
                    }
                    .to_string(),
                });
            }
        };

        // The command crosses threads as data only. The owner creates the
        // window and completes this claim-slot request (ADR-0039 §3)
        // without exposing winit's thread-affine event-loop capability.
        let handle = control
            .request_open_window(options)
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => {
                    OpenWindowError::LaneFull { capacity, rejected }
                }
                ControlSendError::OwnerGone { rejected } => OpenWindowError::OwnerGone {
                    rejected: Some(rejected),
                },
            })?;

        tracing::debug!("Waiting for window creation response");

        // This `handle` is freshly minted above and never offered to any
        // other caller, so `try_take` cannot have claimed it already —
        // `AlreadyClaimed` here would mean this fast path itself raced a
        // second claim attempt, which the code above never performs.
        // `OwnerGone` is real, though: the event-loop owner can disconnect
        // (quit, panic-unwind) while this request is in flight on the lane.
        let window = match handle.wait() {
            ClaimOutcome::Delivered(result) => result?,
            ClaimOutcome::AlreadyClaimed => {
                unreachable!("BUG: this handle is never polled by another caller before wait")
            }
            ClaimOutcome::OwnerGone => {
                return Err(OpenWindowError::OwnerGone { rejected: None });
            }
        };

        tracing::info!("Window created successfully");
        Ok(window)
    }

    fn active(owner: &WinitLoop) -> Option<WindowId> {
        owner.with_state(|state| state.active_window)
    }

    fn capabilities(owner: &WinitLoop) -> WindowCapabilities {
        owner.expected_shell().capabilities()
    }

    fn frame_inset(owner: &WinitLoop, scale_factor: f64) -> Size<f64> {
        owner.frame_inset.logical(scale_factor)
    }

    fn on_event(owner: &WinitLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|state| &mut state.handlers.window_event),
            callback,
        );
    }
}

impl host::Input<WinitLoop> for WinitLoop {
    fn keyboard_layout(_: &WinitLoop) -> String {
        // winit does not report the active keyboard layout: it is unknown.
        String::new()
    }

    fn on_keyboard_layout_change(_: &WinitLoop, callback: Box<dyn FnMut() + Send>) {
        // No layout is read, so no change is ever reported.
        drop(callback);
    }
}

impl host::Preferences<WinitLoop> for WinitLoop {
    #[cfg(target_os = "linux")]
    fn sample(owner: &WinitLoop) -> Result<SystemPreferences, PlatformError> {
        let signal = owner.owner_signal.lock().clone();
        if !signal
            .as_ref()
            .is_some_and(|signal| signal.accepting() && signal.owner() == thread::current().id())
        {
            return Err(PlatformError::Preferences {
                message: "Linux preferences require the live winit event-loop owner".into(),
            });
        }
        let source = owner.with_state(|state| state.preferences.upgrade());
        source
            .map(|source| source.read())
            .ok_or(PlatformError::PreferencesDeferred)
    }

    #[cfg(target_os = "macos")]
    fn sample(owner: &WinitLoop) -> Result<SystemPreferences, PlatformError> {
        let signal = owner.owner_signal.lock().clone();
        if !signal
            .as_ref()
            .is_some_and(|signal| signal.accepting() && signal.owner() == thread::current().id())
        {
            return Err(PlatformError::Preferences {
                message: "AppKit preferences require the live winit event-loop owner".into(),
            });
        }
        let source = owner.with_state(|state| state.preferences.upgrade());
        source.ok_or(PlatformError::PreferencesDeferred)?.read()
    }

    #[cfg(windows)]
    fn sample(owner: &WinitLoop) -> Result<SystemPreferences, PlatformError> {
        let signal = owner.owner_signal.lock().clone();
        if !signal
            .as_ref()
            .is_some_and(|signal| signal.accepting() && signal.owner() == thread::current().id())
        {
            return Err(PlatformError::Preferences {
                message: "Windows preferences require the live winit event-loop owner".into(),
            });
        }
        match owner.with_state(|state| state.preferences.clone()) {
            PreferenceObservation::Observed(values) => Ok(values),
            PreferenceObservation::Deferred => Err(PlatformError::PreferencesDeferred),
            PreferenceObservation::Failed(message) => Err(PlatformError::Preferences { message }),
        }
    }
}

impl host::Lifecycle<WinitLoop> for WinitLoop {
    #[cfg(target_os = "linux")]
    fn on_session_end(
        owner: &WinitLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        install(
            &owner.handler(|state| &mut state.handlers.session_end),
            callback,
        );
    }

    #[cfg(not(target_os = "linux"))]
    fn on_session_end(
        _: &WinitLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        // Only Linux's login manager reports a session end through winit; on
        // this OS nothing will call the callback.
        drop(callback);
    }

    fn on_host_notice(owner: &WinitLoop, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        owner.notices.set_hook(callback);
    }

    fn release_session_end(_: &WinitLoop) {
        // Linux cannot hold a shutdown for the user and no other OS reports a
        // session end, so no block is ever held.
    }

    fn on_reopen(owner: &WinitLoop, callback: Box<dyn FnMut() + Send>) {
        install(&owner.handler(|state| &mut state.handlers.reopen), callback);
    }

    fn activate(_: &WinitLoop, _ignoring_other_apps: bool) {
        // winit raises one window at a time (`Window::focus_window`); it has no
        // application-wide activation to ask for.
    }
}
