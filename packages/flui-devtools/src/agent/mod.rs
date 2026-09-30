//! The development agent server (ADR-0095 §3): the application's semantics
//! tree, served to an agent over a local endpoint.
//!
//! [`AgentServer`] is a [`DevAgentHook`]: the application installs it on its
//! configuration, the host attaches it when the event loop starts and hands
//! it each window with content, and the server answers agents from threads of
//! its own. Every read and action goes through the window's owner inbox, so
//! nothing here touches a tree: the owner answers at its next drain, a frame
//! boundary, and the server only waits for the answer.
//!
//! ```no_run
//! use flui_devtools::agent::AgentServer;
//!
//! // Reads `FLUI_AGENT_ENDPOINT` and `FLUI_AGENT_TOKEN`, which the tool that
//! // launched the app set.
//! let server = AgentServer::from_env();
//! // flui_app::run_app_with_config(App, AppConfig::new().with_dev_agent(server));
//! # drop(server);
//! ```
//!
//! # The endpoint
//!
//! A named pipe on Windows (`\\.\pipe\<name>`, owner-only, remote clients
//! refused) and a Unix domain socket elsewhere (a path whose parent
//! directory the current user owns with mode `0700`; the socket file is
//! removed when the server stops). No TCP port, and no endpoint at all on a
//! target without local sockets (wasm32), where binding fails as below. The
//! server listens only in
//! debug builds and only with a token of at least [`MIN_TOKEN_LEN`] bytes;
//! otherwise it stays inert and logs why once. A bind failure is logged too,
//! and the application keeps running without an agent.
//!
//! The token keeps out other users and remote callers. It does not keep out
//! other processes of the same user, which can read the application's
//! environment just as they can reach the endpoint.
//!
//! # Framing
//!
//! Newline-delimited JSON, one request per line, at most [`MAX_LINE`] bytes.
//! The first line must be `{"hello":{"token":"…"}}`; the server answers
//! `{"hello":{"protocol":"<version>"}}`, and closes the connection without a
//! word on a wrong token, a first line that is not a hello, or no hello
//! within the handshake timeout. Then:
//!
//! | Request | Reply |
//! |---|---|
//! | `{"id":1,"op":"windows"}` | `{"id":1,"result":{"windows":["w3"]}}` |
//! | `{"id":2,"op":"read","window":"w3","query":{…}}` | `{"id":2,"result":<Tree>}` |
//! | `{"id":3,"op":"act","window":"w3","request":<ActionRequest>}` | `{"id":3,"result":{}}` |
//!
//! Every read and action names its window, because element handles are
//! scoped to one window. A failure answers ADR-0080's error object:
//! `{"id":n,"error":{"code","message","retry","kind"?,"handle"?,"effect"?}}`.
//! A malformed line answers `invalid_argument` and the connection stays open;
//! a line over the limit answers `invalid_argument` and the connection
//! closes.
//!
//! A reply the owner has not sent within the reply timeout answers `timeout`:
//! with retry `soon` for a read, and with the `may_have_run` effect for an
//! action, which is still queued and runs at the owner's next drain. Read the
//! tree for its effect rather than acting again.
//!
//! What the server traces carries the operation, window and element ids,
//! error codes and timings, never a label, a value or a request line.

#[cfg(any(unix, windows))]
mod endpoint;
#[cfg(any(unix, windows))]
mod registry;
#[cfg(any(unix, windows))]
mod session;

#[cfg(not(any(unix, windows)))]
#[path = "unsupported.rs"]
mod endpoint;

use std::fmt;
use std::time::Duration;

use flui_sdk::view::dev_agent::{AgentWindow, DevAgentHook};

use self::endpoint::Running;

/// The environment variable [`AgentServer::from_env`] reads the endpoint
/// address from.
pub const ENDPOINT_ENV: &str = "FLUI_AGENT_ENDPOINT";

/// The environment variable [`AgentServer::from_env`] reads the launch token
/// from.
pub const TOKEN_ENV: &str = "FLUI_AGENT_TOKEN";

/// The shortest launch token the server accepts, in bytes.
pub const MIN_TOKEN_LEN: usize = 32;

/// The longest request line the server reads, in bytes.
pub const MAX_LINE: usize = 1 << 20;

/// Where the server listens and the token a client must present.
#[derive(Clone)]
pub struct AgentEndpoint {
    address: String,
    token: String,
    reply_timeout: Duration,
    handshake_timeout: Duration,
    max_connections: usize,
}

impl AgentEndpoint {
    /// An endpoint at `address` that admits clients presenting `token`.
    ///
    /// On Windows `address` is a pipe name (`flui-agent-1234`, or the full
    /// `\\.\pipe\flui-agent-1234`); elsewhere it is the socket's path.
    #[must_use]
    pub fn new(address: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            token: token.into(),
            reply_timeout: Duration::from_secs(5),
            handshake_timeout: Duration::from_secs(5),
            max_connections: 4,
        }
    }

    /// How long a request waits for the owner's answer before it answers
    /// `timeout` (default 5 s).
    #[must_use]
    pub fn with_reply_timeout(mut self, timeout: Duration) -> Self {
        self.reply_timeout = timeout;
        self
    }

    /// How long a new connection has to present its hello (default 5 s).
    #[must_use]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// How many connections are served at once (default 4); a connection
    /// beyond it is closed unread.
    #[must_use]
    pub fn with_max_connections(mut self, connections: usize) -> Self {
        self.max_connections = connections.max(1);
        self
    }
}

impl fmt::Debug for AgentEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentEndpoint")
            .field("address", &self.address)
            .field("token", &"<redacted>")
            .field("reply_timeout", &self.reply_timeout)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

/// The development agent server: a [`DevAgentHook`] that serves the windows
/// the host hands it over an [`AgentEndpoint`]. See the [module docs](self).
pub struct AgentServer {
    /// `None` when the server is inert.
    endpoint: Option<AgentEndpoint>,
    /// Between a successful bind at `attach` and `detach`.
    running: Option<Running>,
}

impl AgentServer {
    /// A server for `endpoint`. Inert, with one warning logged, in a release
    /// build or when the token is shorter than [`MIN_TOKEN_LEN`].
    #[must_use]
    pub fn new(endpoint: AgentEndpoint) -> Self {
        let endpoint = if !cfg!(debug_assertions) {
            tracing::warn!("the development agent serves debug builds only; it stays inert");
            None
        } else if endpoint.token.len() < MIN_TOKEN_LEN {
            tracing::warn!(
                min_len = MIN_TOKEN_LEN,
                "the development agent's token is too short; it stays inert"
            );
            None
        } else {
            Some(endpoint)
        };
        Self {
            endpoint,
            running: None,
        }
    }

    /// A server for the endpoint and token in [`ENDPOINT_ENV`] and
    /// [`TOKEN_ENV`], read now. Inert, with one warning logged, when either
    /// is missing, and as [`Self::new`] says.
    #[must_use]
    pub fn from_env() -> Self {
        let address = std::env::var(ENDPOINT_ENV).ok().filter(|v| !v.is_empty());
        let token = std::env::var(TOKEN_ENV).ok().filter(|v| !v.is_empty());
        if let (Some(address), Some(token)) = (address, token) {
            return Self::new(AgentEndpoint::new(address, token));
        }
        tracing::warn!(
            endpoint_env = ENDPOINT_ENV,
            token_env = TOKEN_ENV,
            "the development agent has no endpoint or token in the environment; it stays inert"
        );
        Self {
            endpoint: None,
            running: None,
        }
    }

    /// Whether the server is listening: attached, and its endpoint bound.
    #[must_use]
    pub fn is_listening(&self) -> bool {
        self.running.is_some()
    }

    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }
}

impl DevAgentHook for AgentServer {
    fn attach(&mut self) -> bool {
        self.stop();
        let Some(endpoint) = &self.endpoint else {
            return false;
        };
        match endpoint::listen(endpoint) {
            Ok(running) => {
                tracing::info!("development agent listening");
                self.running = Some(running);
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the development agent could not bind its endpoint; the app runs without it"
                );
            }
        }
        self.is_listening()
    }

    fn detach(&mut self) {
        self.stop();
    }

    fn window_opened(&mut self, window: AgentWindow) {
        if let Some(running) = &self.running {
            running.window_opened(window);
        }
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl fmt::Debug for AgentServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentServer")
            .field("endpoint", &self.endpoint)
            .field("listening", &self.is_listening())
            .finish_non_exhaustive()
    }
}
