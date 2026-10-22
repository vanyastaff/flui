//! The application's lifecycle hooks, as the winit loop keeps them on Linux:
//! the session-end hook it asks when logind announces a shutdown, the notice
//! channel the host's sources deliver to on its turns (`lifecycle::linux`),
//! and the reopen hook.

use flui_platform::{SessionEndAnswer, SessionEndPhase};

use super::WinitLoop;
use crate::owner::call_point::{call, install};
use crate::owner::handlers::SessionEndHook;
use crate::owner::notices::{LifecycleHooks, NoticeChannel};

/// logind cannot wait for the user, so no session end is held open.
impl LifecycleHooks for WinitLoop {
    fn notices(&self) -> &dyn NoticeChannel {
        &*self.notices
    }

    fn set_session_end_hook(&self, hook: SessionEndHook) {
        install(&self.handler(|state| &mut state.handlers.session_end), hook);
    }

    fn release_session_end(&self) {
        // Linux cannot hold a shutdown for the user, so no block is ever held.
    }

    fn set_reopen_hook(&self, hook: Box<dyn FnMut() + Send>) {
        install(&self.handler(|state| &mut state.handlers.reopen), hook);
    }
}

impl WinitLoop {
    /// One phase of a session end: the hook the host registered is asked and
    /// its answer returned; [`SessionEndAnswer::Proceed`] when none is
    /// registered. The hook runs through the call point, outside the state
    /// lock (ADR-0039), and is restored unless a newer one replaced it
    /// meanwhile — even when it panicked, so a later session end still reaches
    /// the host; its panic reaches the caller.
    pub(crate) fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        call(
            &self.handler(|state| &mut state.handlers.session_end),
            |hook| hook(phase),
        )
        .or_resume(SessionEndAnswer::Proceed)
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
    use crate::{WinitPlatform, platform::Platform};

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
            platform.owner().session_end(SessionEndPhase::Query),
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
            assert_eq!(platform.owner().session_end(phase), SessionEndAnswer::Block);
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
            platform.owner().session_end(SessionEndPhase::Query);
        }));
        assert!(outcome.is_err(), "the panic reaches the caller");

        platform.owner().session_end(SessionEndPhase::Ending);
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

        platform.owner().session_end(SessionEndPhase::Query);
        platform.owner().session_end(SessionEndPhase::Ending);
        assert_eq!(
            *phases.lock(),
            [SessionEndPhase::Ending],
            "the hook installed meanwhile took the next phase"
        );
    }
}
