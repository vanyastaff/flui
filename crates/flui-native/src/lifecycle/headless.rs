//! The headless host's lifecycle: the session ends and system notices a test
//! reports, the way an operating system does.

use std::sync::{Arc, Weak};

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::call_point::{Field, call, install};
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
    fn on_session_end(
        owner: &HeadlessLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        install(
            &Field::new(&*owner.state, |state| &mut state.handlers.session_end),
            callback,
        );
    }

    fn on_host_notice(owner: &HeadlessLoop, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        let notices = owner.with_state(|state| Arc::clone(&state.notices));
        notices.set_hook(callback);
    }

    fn release_session_end(owner: &HeadlessLoop) {
        owner.with_state(|state| state.session_end_releases += 1);
    }

    fn on_reopen(_: &HeadlessLoop, callback: Box<dyn FnMut() + Send>) {
        // Nothing reopens a headless application.
        drop(callback);
    }

    fn activate(_: &HeadlessLoop, _: bool) {
        // Nothing is shown, so nothing comes to the front.
    }
}

/// Reports session ends and system notices to the headless platform's
/// registered hooks after `run` has consumed it — obtained from
/// [`HeadlessPlatform::host_events`](crate::HeadlessPlatform::host_events) before `run`.
///
/// Each call runs the hook on the calling thread through the call point,
/// outside the platform's state lock, so the hook may call back into the
/// platform; a hook registered while it runs replaces it. A hook that panics
/// keeps its place, and its panic reaches the caller.
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
    /// it: the hook registered with `OwnerPlatform::on_session_end` is asked and
    /// its answer returned; [`SessionEndAnswer::Proceed`] when none is
    /// registered or the platform is gone. Like
    /// [`MockWindow::simulate_close`](crate::MockWindow::simulate_close), it
    /// may be called from inside another callback, as a session end arriving
    /// in a nested native loop is.
    pub fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(state) = self.state.upgrade() else {
            return SessionEndAnswer::Proceed;
        };
        call(
            &Field::new(&*state, |state| &mut state.handlers.session_end),
            |hook| hook(phase),
        )
        .or_resume(SessionEndAnswer::Proceed)
    }

    /// A system notice other than a session end's phases: the hook registered
    /// with `OwnerPlatform::on_host_notice` is called. Nothing happens when
    /// none is registered or the platform is gone.
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

    /// How many times the host told the platform it released a session end it
    /// had answered with a block (`Platform::release_session_end`).
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.state
            .upgrade()
            .map_or(0, |state| state.lock().session_end_releases)
    }
}
