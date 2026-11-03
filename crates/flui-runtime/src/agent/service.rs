//! What every conversation of one agent shares, and how a transport opens one.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use flui_protocol::AppInfo;
use flui_view::dev_agent::AgentWindow;
use web_time::Instant;

use super::registry::{self, Registry};
use super::session::Session;

/// The bounds one agent serves within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How long a request waits for the window's answer before it answers `timeout`. A wait
    /// adds its own timeout to it. [`Duration::MAX`] waits without a deadline.
    pub reply_timeout: Duration,
    /// How long a new conversation has to present its hello.
    pub handshake_timeout: Duration,
    /// How many conversations are served at once; [`Service::accept`] refuses one beyond it.
    pub max_connections: usize,
    /// How many requests one conversation holds: waiting, running, and answered but not yet
    /// transmitted. One beyond it is answered `busy`. At least one is always held.
    pub queue: usize,
    /// How many event subscriptions one conversation holds.
    pub subscriptions: usize,
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

/// What a [`Service`] is made with.
pub struct ServiceConfig {
    /// The token a driver presents in its hello. Where the launch is read, it is refused
    /// shorter than [`MIN_TOKEN_LEN`](flui_protocol::launch::MIN_TOKEN_LEN) bytes.
    pub token: String,
    /// What the hello's answer says of the app.
    pub app: AppInfo,
    /// The bounds the service serves within.
    pub limits: Limits,
    /// Runs once when the app's driver goes: the first conversation past its hello ends while
    /// the service is not stopped. `None` claims no driver. It runs on the transport's thread,
    /// with no lock of the service held, and a panic in it is contained.
    pub on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>,
}

impl fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceConfig")
            .field("token", &"<redacted>")
            .field("app", &self.app)
            .field("limits", &self.limits)
            .field("on_driver_gone", &self.on_driver_gone.is_some())
            .finish()
    }
}

/// Why [`Service::accept`] took no conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// [`Limits::max_connections`] conversations are being served.
    Full,
    /// The service has stopped.
    Stopped,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "the agent serves as many conversations as it takes",
            Self::Stopped => "the agent has stopped",
        })
    }
}

impl std::error::Error for Refused {}

/// One development agent: the windows it serves, and the conversations it holds about them.
/// See the [module docs](super).
///
/// `Clone` shares the one service: a transport keeps a clone to [accept](Self::accept)
/// conversations while the host hands it windows through another.
#[derive(Clone)]
pub struct Service {
    shared: Arc<Shared>,
}

/// What the conversations of one service share.
pub(super) struct Shared {
    pub(super) registry: Registry,
    pub(super) token: String,
    pub(super) limits: Limits,
    pub(super) app: AppInfo,
    on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>,
    /// Set by the first conversation past its hello when a driver is claimed.
    driver_claimed: AtomicBool,
    stopped: AtomicBool,
    /// Conversations accepted and not yet dropped.
    live: AtomicUsize,
    next_conversation: AtomicU64,
}

impl Shared {
    pub(super) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Claims the driver for a conversation just past its hello: `true` for the first, when
    /// the service has a driver to claim.
    pub(super) fn claim_driver(&self) -> bool {
        self.on_driver_gone.is_some() && !self.driver_claimed.swap(true, Ordering::SeqCst)
    }

    /// The driver's conversation ended: runs the obligation, contained, unless the service
    /// stopped.
    pub(super) fn driver_gone(&self) {
        if self.is_stopped() {
            return;
        }
        if let Some(gone) = &self.on_driver_gone {
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(gone));
            if let Err(payload) = ran {
                let text = flui_foundation::panic::payload_text(payload.as_ref());
                super::trace(|| {
                    tracing::error!(panic = text, "the driver's disconnect obligation panicked");
                });
                flui_foundation::panic::discard_payload(payload);
            }
        }
    }

    /// A conversation was dropped.
    pub(super) fn release(&self, conversation: u64) {
        self.registry.unwatch(conversation);
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Service {
    /// A service with `config`, serving no window yet.
    #[must_use]
    pub fn new(config: ServiceConfig) -> Self {
        let ServiceConfig {
            token,
            app,
            limits,
            on_driver_gone,
        } = config;
        Self {
            shared: Arc::new(Shared {
                registry: Registry::default(),
                token,
                limits,
                app,
                on_driver_gone,
                driver_claimed: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
                live: AtomicUsize::new(0),
                next_conversation: AtomicU64::new(0),
            }),
        }
    }

    /// Serve `window` to the conversations, those already open included. A stopped service
    /// lets it go.
    pub fn window_opened(&self, window: AgentWindow) {
        if self.shared.is_stopped() {
            return;
        }
        registry::wake_all(self.shared.registry.insert(window));
    }

    /// A conversation for a connection the transport accepted at `now`: its hello is due
    /// within [`Limits::handshake_timeout`].
    ///
    /// # Errors
    ///
    /// [`Refused::Full`] while [`Limits::max_connections`] conversations live, and
    /// [`Refused::Stopped`] once the service has stopped. The transport closes the connection
    /// unread.
    pub fn accept(&self, now: Instant) -> Result<Session, Refused> {
        if self.shared.is_stopped() {
            return Err(Refused::Stopped);
        }
        let max = self.shared.limits.max_connections;
        self.shared
            .live
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |live| {
                (live < max).then_some(live + 1)
            })
            .map_err(|_| Refused::Full)?;
        let conversation = self.shared.next_conversation.fetch_add(1, Ordering::SeqCst);
        Ok(Session::new(Arc::clone(&self.shared), conversation, now))
    }

    /// Stop serving: every window is let go, and every conversation closes at its next poll,
    /// which this wakes. No driver's going runs from here on. Stopping twice does nothing more.
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        registry::wake_all(self.shared.registry.clear());
    }

    /// Whether [`Self::stop`] was called.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.shared.is_stopped()
    }
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("app", &self.shared.app)
            .field("limits", &self.shared.limits)
            .field("live", &self.shared.live.load(Ordering::SeqCst))
            .field("stopped", &self.shared.is_stopped())
            .finish_non_exhaustive()
    }
}
