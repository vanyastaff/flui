//! The application's lifecycle as the winit loop hears it on Linux: the
//! session end it asks the application's handler about when logind announces
//! a shutdown, and the release of a held one, which logind cannot do.

use flui_platform::{SessionEndAnswer, SessionEndPhase};

use super::WinitLoop;
use crate::owner::notices::LifecycleHooks;

/// logind cannot wait for the user, so no session end is held open.
impl LifecycleHooks for WinitLoop {
    fn release_session_end(&self) {
        // Linux cannot hold a shutdown for the user, so no block is ever held.
    }
}

impl WinitLoop {
    /// One phase of a session end: the application's handler is asked and its
    /// answer returned; [`SessionEndAnswer::Proceed`] when no handler is
    /// bound. The handler is asked through the call point, outside the state
    /// lock (ADR-0039); its panic reaches the caller.
    pub(crate) fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.app
            .session_end(phase)
            .or_resume(SessionEndAnswer::Proceed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::owner::app_delivery::testing::{AppCall, AppLog, bind};
    use crate::owner::winit::testing::quiet_platform;

    #[test]
    fn session_end_without_a_handler_proceeds() {
        let platform = quiet_platform();
        assert_eq!(
            platform.owner().session_end(SessionEndPhase::Query),
            SessionEndAnswer::Proceed
        );
    }

    #[test]
    fn session_end_asks_the_handler_each_phase_and_returns_its_answer() {
        let platform = quiet_platform();
        let app = AppLog::new();
        app.answer_session_end(SessionEndAnswer::Block);
        let _run = bind(Arc::clone(&platform.owner().app), app.clone());
        for phase in [
            SessionEndPhase::Query,
            SessionEndPhase::Ending,
            SessionEndPhase::Cancelled,
        ] {
            assert_eq!(platform.owner().session_end(phase), SessionEndAnswer::Block);
        }
        assert_eq!(
            app.calls(),
            [
                AppCall::SessionEnd(SessionEndPhase::Query),
                AppCall::SessionEnd(SessionEndPhase::Ending),
                AppCall::SessionEnd(SessionEndPhase::Cancelled),
            ]
        );
    }

    #[test]
    fn a_session_end_handler_that_panics_hears_the_next_phase() {
        let platform = quiet_platform();
        let app = AppLog::new();
        let _run = bind(Arc::clone(&platform.owner().app), app.clone());
        let first = std::cell::Cell::new(true);
        app.react(move |_, _| assert!(!first.replace(false), "the first phase panics"));

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            platform.owner().session_end(SessionEndPhase::Query);
        }));
        assert!(outcome.is_err(), "the panic reaches the caller");

        platform.owner().session_end(SessionEndPhase::Ending);
        assert_eq!(app.calls().len(), 2, "the handler heard the next phase");
    }
}
