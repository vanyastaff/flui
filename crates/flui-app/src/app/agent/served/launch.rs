//! What the launch tells the agent: where to listen, the token a driver
//! presents, and whether to exit when the driver goes (ADR-0239 §3, §5).

use std::fmt;
use std::time::Duration;

use flui_protocol::launch::{
    ENDPOINT_ENV, EXIT_ON_DISCONNECT_ENV, MIN_TOKEN_LEN, TOKEN_ENV, exit_on_disconnect,
};

/// The bounds one agent serves within.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// How long a request waits for the owner's answer before it answers
    /// `timeout`. A wait adds its own timeout to it.
    pub(crate) reply_timeout: Duration,
    /// How long a new connection has to present its hello.
    pub(crate) handshake_timeout: Duration,
    /// How many connections are served at once; one beyond it is closed
    /// unread.
    pub(crate) max_connections: usize,
    /// How many requests one connection holds, the running one included; one
    /// beyond it is answered `busy`.
    pub(crate) queue: usize,
    /// How many event subscriptions one connection holds.
    pub(crate) subscriptions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            reply_timeout: Duration::from_secs(5),
            handshake_timeout: Duration::from_secs(5),
            max_connections: 4,
            queue: 32,
            subscriptions: 16,
        }
    }
}

/// Where the agent listens, the token a driver presents, and what a
/// driver's disconnect does.
#[derive(Clone)]
pub(crate) struct Launch {
    pub(super) address: String,
    pub(super) token: String,
    exit_on_disconnect: bool,
    pub(super) limits: Limits,
}

impl Launch {
    /// The launch the environment describes, read now. `None`, and nothing
    /// created, when it names no endpoint; `None` with one warning when the
    /// token is missing or shorter than [`MIN_TOKEN_LEN`] bytes.
    pub(crate) fn from_env() -> Option<Self> {
        let read = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let address = read(ENDPOINT_ENV)?;
        let exit = read(EXIT_ON_DISCONNECT_ENV).is_some_and(|value| exit_on_disconnect(&value));
        Self::try_new(address, read(TOKEN_ENV).unwrap_or_default(), exit)
    }

    /// A launch at `address` admitting `token`; `None`, with one warning,
    /// when the token is shorter than [`MIN_TOKEN_LEN`] bytes.
    pub(crate) fn try_new(
        address: String,
        token: String,
        exit_on_disconnect: bool,
    ) -> Option<Self> {
        if token.len() < MIN_TOKEN_LEN {
            tracing::warn!(
                min_len = MIN_TOKEN_LEN,
                "the driver agent's token is missing or too short; the app runs without it"
            );
            return None;
        }
        Some(Self {
            address,
            token,
            exit_on_disconnect,
            limits: Limits::default(),
        })
    }

    /// With `limits` in place of the defaults.
    #[cfg(test)]
    pub(crate) fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Whether the app exits when its driver's connection closes.
    pub(crate) fn exit_on_disconnect(&self) -> bool {
        self.exit_on_disconnect
    }
}

impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launch")
            .field("address", &self.address)
            .field("token", &"<redacted>")
            .field("exit_on_disconnect", &self.exit_on_disconnect)
            .field("limits", &self.limits)
            .finish()
    }
}
