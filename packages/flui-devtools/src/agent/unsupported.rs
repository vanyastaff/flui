//! The endpoint on a target with no local socket (wasm32): binding always
//! fails, so `attach` logs why and answers that the server does not serve,
//! and the application runs without an agent.

use std::io;

use flui_sdk::view::dev_agent::AgentWindow;

use super::AgentEndpoint;

/// A bound endpoint, which this target never has.
pub(super) enum Running {}

impl Running {
    pub(super) fn window_opened(&self, _window: AgentWindow) {
        match *self {}
    }

    pub(super) fn stop(self) {
        match self {}
    }
}

/// Always fails: this target has no local socket to bind.
pub(super) fn listen(_endpoint: &AgentEndpoint) -> io::Result<Running> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this target has no local socket for the development agent",
    ))
}
