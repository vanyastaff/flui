//! One conversation: the hello, then requests answered in order and the events of its
//! subscriptions, as lines for the transport to write.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::task::Waker;
use std::time::Duration;

use flui_protocol::framing::{LineReader, MAX_LINE};
use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{
    Capabilities, ClientRole, ErrorCode, ErrorObject, Greeting, Hello, HelloReply, InputPath,
    Request, RequestId, ServerLine,
};
use flui_view::__runtime::WakeSlot;
use serde::Serialize;
use serde_json::Value;
use web_time::Instant;

use super::dispatch::{self, Admitted, Leases, Running, Subscriptions};
use super::service::{Shared, Taken};

/// What the service writes: a result is the method's output as a JSON value.
type Line = ServerLine<Value>;

/// How long a closing conversation may stay quiet before it closes.
const LINGER_IDLE: Duration = Duration::from_millis(250);

/// The longest a closing conversation lingers, however much its client still sends.
const LINGER_MAX: Duration = Duration::from_secs(5);

/// The most event lines waiting to be transmitted: past it, the streams keep their events, and
/// count the ones they have no room for.
const EVENT_ROOM: usize = 64;

/// The most lines waiting to be transmitted. Past it the client is not reading what it asked
/// for, and the conversation closes.
const MAX_UNSENT: usize = 1024;

/// How soon a window too busy to give a stream is asked again.
const RETRY_SOON: Duration = Duration::from_millis(20);

/// Where a conversation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    /// Read, poll and transmit.
    Open,
    /// Transmit what [`Session::transmit`] gives, and keep handing over what the client sends
    /// (the session discards it) until the status is [`Status::Closed`]: closing at once could
    /// lose the reply with the client's unread input.
    Closing,
    /// Transmit what [`Session::transmit`] still gives, then close the connection.
    Closed,
}

enum Phase {
    /// Waiting for the hello, until the deadline.
    Greeting {
        deadline: Option<Instant>,
    },
    Open,
    /// A refusal is queued; what the client still sends is discarded until it is quiet for
    /// [`LINGER_IDLE`], or [`LINGER_MAX`] passes.
    Lingering {
        quiet_at: Option<Instant>,
        until: Option<Instant>,
    },
    Closed,
}

/// A line waiting to be transmitted.
struct Outgoing {
    bytes: Vec<u8>,
    /// An answer to a request: it counts towards the requests the conversation holds.
    answer: bool,
}

/// One conversation of a [`Service`](super::Service), driven by a transport. See the
/// [module docs](super) for the protocol and the driving contract:
///
/// 1. hand over what the connection read with [`Self::receive`], and its end with
///    [`Self::peer_closed`];
/// 2. call [`Self::poll`] after receiving, when the waker it was given is woken, and once
///    [`Self::next_deadline`] passes;
/// 3. write every line [`Self::transmit`] gives, in order;
/// 4. close the connection once [`Self::status`] is [`Status::Closed`] and nothing is left to
///    transmit, and drop the session.
///
/// Dropping the session ends the conversation: the requests it holds are let go, and when it was
/// the app's driver, the driver's going runs.
pub struct Session {
    shared: Arc<Shared>,
    conversation: u64,
    lines: LineReader,
    phase: Phase,
    /// Woken by the service when a window opens or the service stops.
    wake: Arc<WakeSlot>,
    waiting: VecDeque<Admitted>,
    running: Option<Running>,
    subscriptions: Subscriptions,
    /// Its input leases: what it injected stays held until it releases it or goes.
    leases: Leases,
    outgoing: VecDeque<Outgoing>,
    /// Answers in `outgoing`.
    unsent_answers: usize,
    /// When a window too busy to give a stream is asked again.
    retry_at: Option<Instant>,
    /// The role its hello was admitted with; `None` before.
    role: Option<ClientRole>,
}

impl Session {
    pub(super) fn new(shared: Arc<Shared>, conversation: u64, now: Instant) -> Self {
        let wake = Arc::new(WakeSlot::default());
        shared.registry.watch(conversation, Arc::clone(&wake));
        let deadline = now.checked_add(shared.limits.handshake_timeout);
        Self {
            shared,
            conversation,
            lines: LineReader::new(MAX_LINE),
            phase: Phase::Greeting { deadline },
            wake,
            waiting: VecDeque::new(),
            running: None,
            subscriptions: Subscriptions::default(),
            leases: Leases::default(),
            outgoing: VecDeque::new(),
            unsent_answers: 0,
            retry_at: None,
            role: None,
        }
    }

    /// Hand over what the connection read at `now`. Call [`Self::poll`] after.
    pub fn receive(&mut self, bytes: &[u8], now: Instant) {
        match &mut self.phase {
            Phase::Closed => return,
            Phase::Lingering { quiet_at, .. } => {
                *quiet_at = now.checked_add(LINGER_IDLE);
                return;
            }
            Phase::Greeting { .. } | Phase::Open => {}
        }
        self.lines.push(bytes);
        while matches!(self.phase, Phase::Greeting { .. } | Phase::Open) {
            match self.lines.next_line() {
                Ok(Some(line)) => self.line(&line, now),
                Ok(None) => return,
                Err(_) => {
                    self.too_long(now);
                    break;
                }
            }
        }
        // What the client sent past a refusal is discarded unread.
        self.lines = LineReader::new(MAX_LINE);
    }

    /// The connection's input ended: the client closed it, or it broke. The conversation
    /// closes, and nothing more is transmitted.
    pub fn peer_closed(&mut self) {
        self.close();
    }

    /// Run what the conversation holds: start the next request when the last answered, take
    /// the owners' answers and the subscriptions' events, and close once a deadline passed.
    /// `waker` is woken when there is more to do; poll again then.
    pub fn poll(&mut self, now: Instant, waker: &Waker) {
        if self.shared.is_stopped() {
            self.close();
            return;
        }
        match self.phase {
            Phase::Closed => return,
            Phase::Greeting { deadline } => {
                if deadline.is_some_and(|deadline| now >= deadline) {
                    super::trace(|| tracing::debug!("agent connection refused: no hello in time"));
                    self.close();
                    return;
                }
            }
            Phase::Lingering { quiet_at, until } => {
                if quiet_at.is_none_or(|quiet_at| now >= quiet_at)
                    || until.is_none_or(|until| now >= until)
                {
                    // The refusal stays queued: it is transmitted before the connection closes.
                    self.phase = Phase::Closed;
                }
                return;
            }
            Phase::Open => {}
        }
        // Registered before anything is looked at, so a window opening, or the service
        // stopping, in between still wakes.
        self.wake.register(waker);
        if matches!(self.phase, Phase::Greeting { .. }) {
            return;
        }
        self.run_requests(now, waker);
        let room = EVENT_ROOM.saturating_sub(self.outgoing.len());
        let drained = self.subscriptions.drain(&self.shared.registry, room, waker);
        self.retry_at = if drained.retry {
            now.checked_add(RETRY_SOON)
        } else {
            None
        };
        for event in drained.events {
            // An event that cannot be written is skipped, not the conversation.
            if let Some(bytes) = encode(&Line::Event(event)) {
                self.push(bytes, false);
            }
        }
        for (window, code) in drained.refused {
            super::trace(|| {
                tracing::warn!(window = %window, code = %code, "a subscription cannot reach a window");
            });
        }
        if drained.held {
            // More events are queued than there was room for: poll again once the transport
            // has transmitted what is here.
            waker.wake_by_ref();
        }
    }

    /// Answer requests in order while their answers are at hand.
    fn run_requests(&mut self, now: Instant, waker: &Waker) {
        loop {
            if !matches!(self.phase, Phase::Open) {
                return;
            }
            if self.running.is_none() {
                let Some(admitted) = self.waiting.pop_front() else {
                    return;
                };
                self.running = Some(dispatch::start(
                    &self.shared,
                    &mut self.subscriptions,
                    &mut self.leases,
                    admitted,
                    now,
                ));
            }
            let running = self
                .running
                .as_mut()
                .expect("BUG: a request runs once one was started");
            let Some(outcome) = running.poll(&mut self.subscriptions, now, waker) else {
                return;
            };
            let running = self
                .running
                .take()
                .expect("BUG: the request just answered is the one running");
            let elapsed_ms =
                u64::try_from(now.saturating_duration_since(running.started).as_millis())
                    .unwrap_or(u64::MAX);
            let failed = outcome.as_ref().err().map(|error| error.code);
            let method = running.method.name;
            self.answer(running.id, outcome);
            // Traced once the answer is queued.
            super::trace(|| match failed {
                None => tracing::debug!(method, elapsed_ms, "agent request served"),
                Some(code) => tracing::debug!(
                    method,
                    code = code.name(),
                    elapsed_ms,
                    "agent request failed"
                ),
            });
        }
    }

    /// The next line to write, with its newline; `None` while there is none.
    pub fn transmit(&mut self) -> Option<Vec<u8>> {
        let line = self.outgoing.pop_front()?;
        if line.answer {
            self.unsent_answers -= 1;
        }
        Some(line.bytes)
    }

    /// When [`Self::poll`] is due even if nothing wakes: the handshake's or the running
    /// request's deadline, the end of lingering, or a retry. `None` when only a wake makes
    /// more to do.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        match &self.phase {
            Phase::Greeting { deadline } => *deadline,
            Phase::Lingering { quiet_at, until } => match (quiet_at, until) {
                (Some(quiet_at), Some(until)) => Some(*quiet_at.min(until)),
                (deadline, None) | (None, deadline) => *deadline,
            },
            Phase::Open => {
                let request = self.running.as_ref().and_then(|running| running.deadline);
                match (request, self.retry_at) {
                    (Some(request), Some(retry)) => Some(request.min(retry)),
                    (deadline, None) | (None, deadline) => deadline,
                }
            }
            Phase::Closed => None,
        }
    }

    /// Where the conversation is.
    #[must_use]
    pub fn status(&self) -> Status {
        match self.phase {
            Phase::Greeting { .. } | Phase::Open => Status::Open,
            Phase::Lingering { .. } => Status::Closing,
            Phase::Closed => Status::Closed,
        }
    }

    fn line(&mut self, line: &[u8], now: Instant) {
        match self.phase {
            Phase::Greeting { .. } => self.greet(line, now),
            Phase::Open => self.admit(line),
            Phase::Lingering { .. } | Phase::Closed => {}
        }
    }

    /// Read the hello and answer it.
    fn greet(&mut self, line: &[u8], now: Instant) {
        let Some(hello) = parse_hello(line) else {
            super::trace(|| tracing::debug!("agent connection refused: no hello"));
            self.close();
            return;
        };
        if !same_token(hello.token.as_bytes(), self.shared.token.as_bytes()) {
            super::trace(|| tracing::debug!("agent connection refused: wrong token"));
            self.close();
            return;
        }
        // An app serves every older client its schema still reads (ADR-0238 §2), and answers
        // with its own version: the capabilities say what it answers.
        if !PROTOCOL_VERSION.serves(hello.protocol) {
            let error = ErrorObject::version(PROTOCOL_VERSION, hello.protocol);
            self.refuse_and_close(&Line::refusal(error), now);
            return;
        }
        let mut capabilities = Capabilities::default();
        capabilities.paths = vec![InputPath::Semantic];
        capabilities.methods = dispatch::served(hello.role);
        let welcome = HelloReply::new(PROTOCOL_VERSION)
            .with_app(self.shared.app.clone())
            .with_capabilities(capabilities);
        let Some(bytes) = encode(&Line::hello(welcome)) else {
            self.close();
            return;
        };
        // One driver at a time (ADR-0288 §8): a second driver's hello is refused before it is
        // answered, so the client never holds a conversation it may not act on. The seat is the
        // last thing taken, so a conversation that holds it is always answered.
        if hello.role.is_driver()
            && let Err(taken) = self.shared.claim_driver()
        {
            let error = match taken {
                Taken::Ended => ErrorObject::new(
                    ErrorCode::ShuttingDown,
                    "the app's driver went and the app is ending",
                ),
                Taken::Held => ErrorObject::new(
                    ErrorCode::NotDriver,
                    "the app has a driver; connect as an observer, or once it goes",
                )
                .with_retry(flui_protocol::Retry::Soon),
            };
            self.refuse_and_close(&Line::refusal(error), now);
            return;
        }
        self.role = Some(hello.role);
        self.push(bytes, false);
        self.phase = Phase::Open;
    }

    /// Queue the request on `line`, or the line refusing it.
    fn admit(&mut self, line: &[u8]) {
        let Ok(request) = serde_json::from_slice::<Request<Value>>(line) else {
            self.refuse(&malformed(line));
            return;
        };
        let Request {
            id,
            method: name,
            params,
            ..
        } = request;
        let Some(method) = flui_protocol::method::find(&name) else {
            // Only a known method's name is echoed or traced: an unknown one is the client's
            // text, which may carry anything.
            let error = ErrorObject::new(ErrorCode::InvalidArgument, "no such method");
            self.refuse(&Line::answer(id, Err(error)));
            return;
        };
        if !dispatch::serves(method) {
            self.refuse(&Line::answer(id, Err(dispatch::not_supported(method))));
            return;
        }
        // An observer reads and subscribes; acting is the driver's (ADR-0288 §8).
        if !self
            .role
            .is_some_and(|role| dispatch::may_call(role, method))
        {
            self.refuse(&Line::answer(id, Err(dispatch::not_driver(method))));
            return;
        }
        let held = self.waiting.len() + usize::from(self.running.is_some()) + self.unsent_answers;
        if held >= self.shared.limits.queue.max(1) {
            let error = ErrorObject::new(
                ErrorCode::Busy,
                "the connection holds as many requests as it takes",
            )
            .with_retry(flui_protocol::Retry::Soon);
            self.refuse(&Line::answer(id, Err(error)));
            return;
        }
        let params = if params.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            params
        };
        self.waiting.push_back(Admitted { id, method, params });
    }

    /// A line longer than the protocol takes: before the hello, the conversation closes without
    /// a word; after it, the client is told why.
    fn too_long(&mut self, now: Instant) {
        if matches!(self.phase, Phase::Greeting { .. }) {
            self.close();
            return;
        }
        let error = ErrorObject::new(
            ErrorCode::InvalidArgument,
            format!("a request line is longer than {MAX_LINE} bytes"),
        );
        self.refuse_and_close(&Line::refusal(error), now);
    }

    /// Queue `line`, a refusal, and close once the client has stopped sending.
    fn refuse_and_close(&mut self, line: &Line, now: Instant) {
        self.drop_requests();
        match encode(line) {
            Some(bytes) => {
                self.push(bytes, false);
                if matches!(self.phase, Phase::Closed) {
                    return;
                }
                self.phase = Phase::Lingering {
                    quiet_at: now.checked_add(LINGER_IDLE),
                    until: now.checked_add(LINGER_MAX),
                };
            }
            None => self.close(),
        }
    }

    /// Queue `line`, refusing a request.
    fn refuse(&mut self, line: &Line) {
        if let Some(bytes) = encode(line) {
            self.push(bytes, false);
        }
    }

    /// Queue `id`'s answer.
    fn answer(&mut self, id: RequestId, outcome: Result<Value, ErrorObject>) {
        // An output that does not serialize is answered as such, not dropped.
        let bytes = encode(&Line::answer(id.clone(), outcome)).or_else(|| {
            let error = ErrorObject::new(ErrorCode::Platform, "the answer did not serialize");
            encode(&Line::answer(id, Err(error)))
        });
        if let Some(bytes) = bytes {
            self.push(bytes, true);
        }
    }

    fn push(&mut self, bytes: Vec<u8>, answer: bool) {
        if self.outgoing.len() >= MAX_UNSENT {
            super::trace(|| tracing::debug!("agent connection closed: its client is not reading"));
            self.close();
            return;
        }
        self.unsent_answers += usize::from(answer);
        self.outgoing.push_back(Outgoing { bytes, answer });
    }

    /// Lets go of every request, subscription and input lease: the owners forget what they hold
    /// for them, and let go the input the connection holds.
    fn drop_requests(&mut self) {
        self.waiting.clear();
        self.running = None;
        self.subscriptions = Subscriptions::default();
        self.leases = Leases::default();
        self.retry_at = None;
    }

    /// Ends the conversation with nothing more to transmit.
    fn close(&mut self) {
        self.drop_requests();
        self.outgoing.clear();
        self.unsent_answers = 0;
        self.phase = Phase::Closed;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.release(self.conversation);
        if self.role.is_some_and(|role| role.is_driver()) {
            self.shared.driver_gone();
        }
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("conversation", &self.conversation)
            .field("status", &self.status())
            .field("waiting", &self.waiting.len())
            .field("running", &self.running.is_some())
            .field("unsent", &self.outgoing.len())
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

fn parse_hello(line: &[u8]) -> Option<Hello> {
    serde_json::from_slice::<Greeting>(line)
        .ok()
        .map(|greeting| greeting.hello)
}

/// Compares two tokens in time that depends on their lengths only.
fn same_token(presented: &[u8], expected: &[u8]) -> bool {
    let mut difference = presented.len() ^ expected.len();
    for index in 0..presented.len().max(expected.len()) {
        let a = presented.get(index).copied().unwrap_or(0);
        let b = expected.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    std::hint::black_box(difference) == 0
}

/// The refusal of a line that is not a request: it answers the line's id when the line carries
/// one, so a client that sent a broken request is not left waiting for it.
fn malformed(line: &[u8]) -> Line {
    use serde::Deserialize as _;

    let invalid = |message| ErrorObject::new(ErrorCode::InvalidArgument, message);
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Line::refusal(invalid("the request is not JSON"));
    };
    match value.get("id").map(RequestId::deserialize) {
        Some(Ok(id)) => Line::answer(id, Err(invalid("a request names its method"))),
        _ => Line::refusal(invalid("a request carries an id: a number or a string")),
    }
}

/// `line` as the bytes of one line, its newline included; `None` when it does not serialize.
fn encode(line: &impl Serialize) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec(line).ok()?;
    bytes.push(b'\n');
    Some(bytes)
}
