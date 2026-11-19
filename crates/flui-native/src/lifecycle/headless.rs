//! The headless host's lifecycle: the session ends and system notices a test
//! reports, the way an operating system does.

use std::sync::Weak;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::{Opened, SessionEndAnswer, SessionEndPhase, SystemNotice};

use crate::host;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::headless::HeadlessLoop;
use crate::owner::notices::NoticeChannel;
#[cfg(feature = "scripted")]
use {
    crate::{host::Host, platform::HostPlatform},
    std::sync::Arc,
};

/// The headless platform's lifecycle controls, taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
#[cfg(feature = "scripted")]
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Obtain the host-event handle before `run` consumes this platform: the
    /// handle reports session ends and system notices the way an operating
    /// system does. See [`HeadlessHostEvents`].
    #[must_use]
    pub fn host_events(&self) -> HeadlessHostEvents {
        HeadlessHostEvents {
            app: self.owner().with_state(|state| Arc::downgrade(&state.app)),
            notices: self
                .owner()
                .with_state(|state| Arc::downgrade(&state.notices)),
            releases: self
                .owner()
                .with_state(|state| Arc::downgrade(&state.session_end_releases)),
            signal: self.owner().with_state(|state| state.owner_signal.clone()),
        }
    }

    /// Stand in for a platform whose session end works as `support` says: a
    /// test of a phone passes
    /// [`Background`](flui_platform::SessionEndSupport::Background), one of a
    /// browser tab [`NotifyOnly`](flui_platform::SessionEndSupport::NotifyOnly). The
    /// default is [`Hold`](flui_platform::SessionEndSupport::Hold), a
    /// desktop that waits for a block.
    #[must_use]
    pub fn with_session_end_support(self, support: flui_platform::SessionEndSupport) -> Self {
        self.owner()
            .with_state(|state| state.session_end_support = support);
        self
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
        owner.with_state(|state| state.session_end_releases.fetch_add(1, Ordering::SeqCst));
    }

    /// A desktop that waits for a block unless the test chose otherwise: a
    /// test reads the answer and releases it as Windows does.
    fn session_end_support(owner: &HeadlessLoop) -> flui_platform::SessionEndSupport {
        owner.with_state(|state| state.session_end_support)
    }

    fn activate(_: &HeadlessLoop, _: bool) {
        // Nothing is shown, so nothing comes to the front.
    }
}

/// Reports session ends, system notices, reopens and what the operating
/// system opened to the application's
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
    app: Weak<AppDelivery>,
    notices: Weak<dyn NoticeChannel>,
    releases: Weak<AtomicUsize>,
    /// The owner's signal, which a granted quit asks to stop the platform.
    signal: Weak<crate::owner::signal::OwnerSignal>,
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
    /// [`HeadlessHostWindow::simulate_close`](crate::HeadlessHostWindow::simulate_close), it
    /// may be called from inside another method, as a session end arriving in
    /// a nested native loop is.
    pub fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(app) = self.app.upgrade() else {
            return SessionEndAnswer::Proceed;
        };
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
        if let Some(notices) = self.notices.upgrade() {
            notices.dispatch(notice);
        }
    }

    /// The user asked the application to quit, as AppKit reports the Dock's
    /// Quit or a quit Apple Event: the application's handler is asked
    /// ([`AppHandler::quit_asked`](crate::AppHandler::quit_asked)) and its
    /// answer returned. On [`QuitAnswer::Quit`](crate::QuitAnswer::Quit) the
    /// platform stops at its next owner turn, past every window's handler,
    /// as AppKit's loop does; on
    /// [`KeepRunning`](crate::QuitAnswer::KeepRunning) nothing else happens.
    /// With no handler to ask, or once the platform is gone, the answer is
    /// `Quit`.
    pub fn quit(&self) -> crate::QuitAnswer {
        let signal = self.signal.clone();
        // The quit is carried out before a later report's panic resumes, so a
        // granted quit is never lost to it.
        let act = move |answer: &crate::QuitAnswer| {
            if *answer == crate::QuitAnswer::Quit
                && let Some(signal) = signal.upgrade()
            {
                // A platform that stopped already refuses the request: the
                // quit it asks for has happened.
                let _ = signal.request_quit();
            }
        };
        if let Some(app) = self.app.upgrade() {
            app.quit_asked()
                .or_resume_after(crate::QuitAnswer::Quit, act)
        } else {
            act(&crate::QuitAnswer::Quit);
            crate::QuitAnswer::Quit
        }
    }

    /// The user reopened the application, as AppKit reports a click on the
    /// Dock icon: the application's handler hears it
    /// ([`AppHandler::reopen`](crate::AppHandler::reopen)). Nothing happens
    /// once the platform is gone.
    pub fn reopen(&self) {
        if let Some(app) = self.app.upgrade() {
            app.reopen().or_resume(());
        }
    }

    /// The operating system opened URLs or files with the application, as a
    /// link, a launch argument or a document does: the application's handler
    /// hears them ([`AppHandler::opened`](crate::AppHandler::opened)), as the
    /// caller bounded them ([`Opened::bounded`]). Nothing happens once the
    /// platform is gone, and an empty `opened` reports nothing.
    pub fn open(&self, opened: Opened) {
        if let Some(app) = self.app.upgrade() {
            app.opened(opened).or_resume(());
        }
    }

    /// How many times the host told the platform it released a session end it
    /// had answered with a block (`Platform::release_session_end`).
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.releases
            .upgrade()
            .map_or(0, |releases| releases.load(Ordering::SeqCst))
    }
}
