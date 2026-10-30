//! The headless host's lifecycle: the session ends and system notices a test
//! reports, the way an operating system does.

use std::sync::{Arc, Weak};

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::headless::{HeadlessLoop, HeadlessState};
use crate::platform::HostPlatform;

/// The headless platform's lifecycle controls, taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Obtain the host-event handle before `run` consumes this platform: the
    /// handle reports session ends and system notices the way an operating
    /// system does. See [`HeadlessHostEvents`].
    #[must_use]
    pub fn host_events(&self) -> HeadlessHostEvents {
        HeadlessHostEvents {
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Simulate one phase of the user's session ending; see
    /// [`HeadlessHostEvents::session_end`].
    pub fn simulate_session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.host_events().session_end(phase)
    }

    /// Simulate a system notice; see [`HeadlessHostEvents::notice`].
    pub fn simulate_host_notice(&self, notice: SystemNotice) {
        self.host_events().notice(notice);
    }
}

impl host::Lifecycle<HeadlessLoop> for HeadlessHostEvents {
    fn release_session_end(owner: &HeadlessLoop) {
        owner.with_state(|state| state.session_end_releases += 1);
    }

    /// The headless platform stands in for a desktop that waits for a block:
    /// a test reads the answer and releases it as Windows does.
    fn session_end_support(_: &HeadlessLoop) -> flui_platform::SessionEndSupport {
        flui_platform::SessionEndSupport::Hold
    }

    fn activate(_: &HeadlessLoop, _: bool) {
        // Nothing is shown, so nothing comes to the front.
    }
}

/// Reports session ends, system notices and reopens to the application's
/// handler of the headless platform after `run` has consumed it — obtained
/// from [`HeadlessPlatform::host_events`](crate::HeadlessPlatform::host_events)
/// before `run`.
///
/// The test thread is the headless platform's owner thread. Each report goes
/// through the platform's delivery to the handler, outside the platform's
/// state lock, so the handler may call back into the platform. A report made
/// before `run`, while another method runs, or from another thread waits for
/// the handler and is heard in order; a session end, which asks, is answered
/// at once. A handler's panic reaches the caller after what was queued behind
/// it is heard.
#[derive(Clone)]
pub struct HeadlessHostEvents {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessHostEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHostEvents").finish_non_exhaustive()
    }
}

impl HeadlessHostEvents {
    /// One phase of the user's session ending, as the operating system reports
    /// it: the application's handler is asked
    /// ([`AppHandler::session_end`](crate::AppHandler::session_end)) and its
    /// answer returned; [`SessionEndAnswer::Proceed`] when no handler can be
    /// asked or the platform is gone. Like
    /// [`MockWindow::simulate_close`](crate::MockWindow::simulate_close), it
    /// may be called from inside another method, as a session end arriving in
    /// a nested native loop is.
    pub fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(state) = self.state.upgrade() else {
            return SessionEndAnswer::Proceed;
        };
        let app = Arc::clone(&state.lock().app);
        app.session_end(phase).or_resume(SessionEndAnswer::Proceed)
    }

    /// A system notice other than a session end's phases, reported to the
    /// application's handler
    /// ([`AppHandler::host_notice`](crate::AppHandler::host_notice)). Nothing
    /// happens once the platform is gone.
    ///
    /// A wake marks the state of every window opened on this platform stale
    /// first (`owner::notices`).
    pub fn notice(&self, notice: SystemNotice) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let notices = Arc::clone(&state.lock().notices);
        notices.dispatch(notice);
    }

    /// The user reopened the application, as AppKit reports a click on the
    /// Dock icon: the application's handler hears it
    /// ([`AppHandler::reopen`](crate::AppHandler::reopen)). Nothing happens
    /// once the platform is gone.
    pub fn reopen(&self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let app = Arc::clone(&state.lock().app);
        app.reopen().or_resume(());
    }

    /// How many times the host told the platform it released a session end it
    /// had answered with a block (`Platform::release_session_end`).
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.state
            .upgrade()
            .map_or(0, |state| state.lock().session_end_releases)
    }
}
