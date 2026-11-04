//! What every conversation of one agent shares, and how a transport opens one.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use flui_protocol::AppInfo;
use flui_view::dev_agent::AgentWindow;
use parking_lot::Mutex;
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
    /// How many recordings one conversation holds, ended ones it has not stopped included.
    pub recordings: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            reply_timeout: Duration::from_secs(5),
            handshake_timeout: Duration::from_secs(5),
            max_connections: 4,
            queue: 32,
            subscriptions: 16,
            recordings: 4,
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
    /// Runs once when the app's driver goes: the conversation admitted as its driver ends while
    /// the service is not stopped, and no other driver is admitted after it (ADR-0288 §8).
    /// `None`: the driver's going frees the seat for the next. It runs on the transport's thread,
    /// with no lock of the service held, and a panic in it is contained.
    pub on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>,
    /// Whether the app's windows journal their backend, so a recording may hold the `native`
    /// layer: the hello lists it in `capabilities.record` then (ADR-0288 §11).
    pub records_native: bool,
}

impl fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceConfig")
            .field("token", &"<redacted>")
            .field("app", &self.app)
            .field("limits", &self.limits)
            .field("on_driver_gone", &self.on_driver_gone.is_some())
            .field("records_native", &self.records_native)
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

/// The app's driver seat (ADR-0288 §8): one conversation drives the app at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seat {
    /// No conversation drives the app; the next driver's hello takes the seat.
    Free,
    /// A conversation drives the app.
    Held,
    /// The driver went and its obligation ran: the app is ending, and admits no other driver.
    Ended,
}

/// Why a driver's hello found the seat taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Taken {
    /// Another conversation drives the app.
    Held,
    /// The driver went and the app is ending.
    Ended,
}

/// What the conversations of one service share.
pub(super) struct Shared {
    pub(super) registry: Registry,
    pub(super) token: String,
    pub(super) limits: Limits,
    pub(super) app: AppInfo,
    on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>,
    pub(super) records_native: bool,
    /// Taken by the conversation admitted as the driver, and given back when it ends. Locked
    /// only to read or swap the state, never across user code.
    driver: Mutex<Seat>,
    stopped: AtomicBool,
    /// Conversations accepted and not yet dropped.
    live: AtomicUsize,
    next_conversation: AtomicU64,
}

impl Shared {
    pub(super) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Takes the driver's seat for a conversation whose hello asks to drive.
    ///
    /// # Errors
    ///
    /// [`Taken::Held`] while another conversation drives the app, and [`Taken::Ended`] once the
    /// driver went and the app is ending.
    pub(super) fn claim_driver(&self) -> Result<(), Taken> {
        let mut seat = self.driver.lock();
        match *seat {
            Seat::Free => {
                *seat = Seat::Held;
                Ok(())
            }
            Seat::Held => Err(Taken::Held),
            Seat::Ended => Err(Taken::Ended),
        }
    }

    /// The driver's conversation ended. Unless the service stopped, its obligation runs,
    /// contained, and the seat stays ended; with no obligation, the seat is free again.
    pub(super) fn driver_gone(&self) {
        let gone = self.on_driver_gone.as_ref().filter(|_| !self.is_stopped());
        *self.driver.lock() = if gone.is_some() {
            Seat::Ended
        } else {
            Seat::Free
        };
        if let Some(gone) = gone {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a disconnect obligation runs contained so the service survives"
            )]
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
            records_native,
        } = config;
        Self {
            shared: Arc::new(Shared {
                registry: Registry::default(),
                token,
                limits,
                app,
                on_driver_gone,
                records_native,
                driver: Mutex::new(Seat::Free),
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
