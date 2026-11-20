//! The driver agent's conversation served in process (ADR-0288): what a `flui-app` runner serves
//! over a local socket, with the bytes handed over by the caller instead.
//!
//! A [`HeadlessAgentService`] owns the agent's service: the windows a [`HeadlessHost`] hands it
//! through the [`HeadlessDevAgent`] it gives out, the token a driver presents, and the limits it
//! serves within. A [`HeadlessConversation`] is one connection's conversation: the caller hands it
//! the lines a driver writes, polls it after each frame it pumps, and takes the lines it answers.
//! Nothing reads a clock: every call names the instant it happens at, so a driver running the host
//! on its virtual clock sees the deadlines of that clock, and a run is deterministic.
//!
//! [`HeadlessHost`]: crate::HeadlessHost

use std::fmt;
use std::task::Waker;
use std::time::Duration;

use flui_protocol::session::AppDescription;
use flui_runtime::agent::{Limits, Service, ServiceConfig, Session, Status};
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use web_time::Instant;

use crate::HeadlessDevAgent;

/// The driver agent of headless hosts, served to conversations in process.
///
/// `Clone` shares the one service.
#[derive(Clone)]
pub struct HeadlessAgentService {
    service: Service,
}

impl HeadlessAgentService {
    /// A service that admits a hello presenting `token`, introduces itself as `app`, and answers
    /// `timeout` for a request its window has not answered within `reply_timeout` of the
    /// instants the conversation is polled at.
    #[must_use]
    pub fn new(token: impl Into<String>, app: AppDescription, reply_timeout: Duration) -> Self {
        Self {
            service: Service::new(ServiceConfig {
                token: token.into(),
                app,
                limits: Limits {
                    reply_timeout,
                    ..Limits::default()
                },
                on_driver_gone: None,
                records_native: false,
            }),
        }
    }

    /// A development agent whose hook hands the service each window a host built
    /// [`with`](crate::HeadlessHost::with_dev_agent) it serves, and stops the service when it is
    /// dropped.
    #[must_use]
    pub fn dev_agent(&self) -> HeadlessDevAgent {
        HeadlessDevAgent::attach(ServiceHook {
            service: self.service.clone(),
        })
    }

    /// A conversation for a connection opened at `now`; its hello is due within the handshake
    /// timeout.
    ///
    /// # Errors
    ///
    /// [`ConversationRefused`] when the service serves as many conversations as it takes, or has
    /// stopped.
    pub fn connect(&self, now: Instant) -> Result<HeadlessConversation, ConversationRefused> {
        self.service
            .accept(now)
            .map(|session| HeadlessConversation { session })
            .map_err(|refused| ConversationRefused(refused.to_string()))
    }
}

impl fmt::Debug for HeadlessAgentService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeadlessAgentService")
            .field("service", &self.service)
            .finish()
    }
}

/// Why [`HeadlessAgentService::connect`] opened no conversation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the agent took no conversation: {0}")]
pub struct ConversationRefused(String);

/// The hook [`HeadlessAgentService::dev_agent`] attaches.
struct ServiceHook {
    service: Service,
}

impl DevAgentHook for ServiceHook {
    fn attach(&mut self) -> bool {
        !self.service.is_stopped()
    }

    fn detach(&mut self) {
        self.service.stop();
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.service.window_opened(window);
    }
}

/// One conversation of a [`HeadlessAgentService`]: lines in, lines out. The contract is the
/// agent session's: hand over what the driver wrote, [`poll`](Self::poll) after it and after each
/// frame, write every line [`transmit`](Self::transmit) gives, and drop the conversation once it
/// [is closed](Self::is_closed). Dropping it is the driver leaving: the requests it holds are let
/// go and the input it injected is released.
pub struct HeadlessConversation {
    session: Session,
}

impl HeadlessConversation {
    /// Hand over what the driver wrote at `now`.
    pub fn receive(&mut self, bytes: &[u8], now: Instant) {
        self.session.receive(bytes, now);
    }

    /// The driver closed its end.
    pub fn peer_closed(&mut self) {
        self.session.peer_closed();
    }

    /// Run what the conversation holds at `now`: start the next request, take the window's
    /// answers and the subscriptions' events, and answer `timeout` past a deadline.
    pub fn poll(&mut self, now: Instant) {
        // The caller polls after every frame, so a wake carries nothing it would not see.
        self.session.poll(now, Waker::noop());
    }

    /// The next line to write, its newline included.
    pub fn transmit(&mut self) -> Option<Vec<u8>> {
        self.session.transmit()
    }

    /// When a poll is due even if no frame runs: the handshake's or the running request's
    /// deadline.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.session.next_deadline()
    }

    /// Whether the conversation has ended; what [`Self::transmit`] still gives is its last.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.session.status() == Status::Closed
    }
}

impl fmt::Debug for HeadlessConversation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeadlessConversation")
            .field("session", &self.session)
            .finish()
    }
}
