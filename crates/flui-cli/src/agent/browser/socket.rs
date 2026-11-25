//! The browser's WebSocket as JSON messages. Both dialects answer a request with the message
//! that carries its `id`; every message without one is an event, kept in order until it is
//! read, including those that arrive while a request waits for its answer.

use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::LinkError;

/// The longest message read from the browser: a line of the protocol is at most 1 MiB, which
/// JSON may escape to six times as much.
const MAX_MESSAGE: usize = 16 << 20;

/// The most events kept while a request waits for its answer. A page that reports faster
/// than the relay reads is no agent's page.
const MAX_EVENTS: usize = 4096;

/// The most answers kept that no request waits for any more; past it the oldest goes.
const MAX_LATE: usize = 64;

/// A WebSocket to a browser on this host.
pub(super) struct Socket {
    ws: WebSocket<TcpStream>,
    /// Events that arrived while a request waited for its answer, oldest first.
    events: VecDeque<Value>,
    /// Answers that came after their request gave up waiting, oldest first, at most
    /// [`MAX_LATE`]: what they hold in the page is the link's to release
    /// ([`Socket::take_late`]).
    late: VecDeque<Value>,
    /// The ID of the next request; never reused.
    next_id: u64,
}

impl Socket {
    /// Connects to the browser's WebSocket at `url` (`ws://127.0.0.1:9222/…`), which must be on
    /// a loopback address.
    pub(super) fn connect(url: &str, timeout: Duration) -> Result<Self, LinkError> {
        let address = loopback_address(url)?;
        let stream = TcpStream::connect_timeout(&address, timeout).map_err(broken)?;
        stream.set_nodelay(true).map_err(broken)?;
        stream.set_read_timeout(Some(timeout)).map_err(broken)?;
        stream.set_write_timeout(Some(timeout)).map_err(broken)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (ws, _) = tungstenite::client::client_with_config(url, stream, Some(config))
            .map_err(|error| LinkError::Gone(format!("the WebSocket handshake failed: {error}")))?;
        Ok(Self {
            ws,
            events: VecDeque::new(),
            late: VecDeque::new(),
            next_id: 1,
        })
    }

    /// Sends `message` with a fresh `id` and waits no later than `deadline` for the message
    /// that answers it. A late answer to an earlier request is kept aside
    /// ([`Socket::take_late`]).
    pub(super) fn request(
        &mut self,
        mut message: Map<String, Value>,
        deadline: Instant,
    ) -> Result<Value, LinkError> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| LinkError::Gone("the link ran out of request IDs".into()))?;
        message.insert("id".into(), Value::from(id));
        self.send(&Value::Object(message))?;
        loop {
            let Some(message) = self.read(deadline)? else {
                return Err(LinkError::Timeout);
            };
            match message.get("id") {
                Some(answered) if answered.as_u64() == Some(id) => return Ok(message),
                Some(_) => self.keep_late(message),
                None => {
                    if self.events.len() >= MAX_EVENTS {
                        return Err(LinkError::Gone(format!(
                            "the page reported more than {MAX_EVENTS} times during one command"
                        )));
                    }
                    self.events.push_back(message);
                }
            }
        }
    }

    /// The next event, waiting no later than `deadline`; `None` once it has passed.
    pub(super) fn next_event(&mut self, deadline: Instant) -> Result<Option<Value>, LinkError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        loop {
            match self.read(deadline)? {
                None => return Ok(None),
                Some(message) if message.get("id").is_none() => return Ok(Some(message)),
                // An answer no request waits for any more.
                Some(message) => self.keep_late(message),
            }
        }
    }

    /// Every answer kept that came after its request gave up waiting, oldest first; the link
    /// hands back what it did not release ([`Socket::keep_unreleased`]).
    pub(super) fn take_late(&mut self) -> VecDeque<Value> {
        std::mem::take(&mut self.late)
    }

    /// Keeps `unreleased` ahead of the answers kept since [`Socket::take_late`], within the
    /// bound.
    pub(super) fn keep_unreleased(&mut self, unreleased: VecDeque<Value>) {
        let kept = std::mem::take(&mut self.late);
        self.late = ahead_of(unreleased, kept);
    }

    fn keep_late(&mut self, answer: Value) {
        keep_within_bound(&mut self.late, answer);
    }

    fn send(&mut self, message: &Value) -> Result<(), LinkError> {
        self.ws
            .send(Message::text(message.to_string()))
            .map_err(broken)
    }

    /// The next message, or `None` once `deadline` has passed.
    fn read(&mut self, deadline: Instant) -> Result<Option<Value>, LinkError> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.ws
                .get_ref()
                .set_read_timeout(Some(left))
                .map_err(broken)?;
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str::<Value>(&text)
                        .map(Some)
                        .map_err(|error| {
                            LinkError::Gone(format!(
                                "the browser sent a message that is not JSON: {error}"
                            ))
                        });
                }
                Ok(Message::Close(_)) => {
                    return Err(LinkError::Gone("the browser closed its WebSocket".into()));
                }
                // Pings are answered as the socket reads; nothing else carries a message.
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(broken(error)),
            }
        }
    }
}

/// Keeps `answer` after `late`, the oldest going past [`MAX_LATE`].
fn keep_within_bound(late: &mut VecDeque<Value>, answer: Value) {
    if late.len() >= MAX_LATE {
        late.pop_front();
    }
    late.push_back(answer);
}

/// `unreleased` ahead of `kept`, the oldest going past [`MAX_LATE`].
fn ahead_of(mut unreleased: VecDeque<Value>, mut kept: VecDeque<Value>) -> VecDeque<Value> {
    unreleased.append(&mut kept);
    while unreleased.len() > MAX_LATE {
        unreleased.pop_front();
    }
    unreleased
}

/// The loopback address `url` names; an error for any other host.
fn loopback_address(url: &str) -> Result<SocketAddr, LinkError> {
    let refused = || {
        LinkError::Gone(format!(
            "`{url}` is not a WebSocket on this host's loopback"
        ))
    };
    let rest = url.strip_prefix("ws://").ok_or_else(refused)?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host != "localhost" && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return Err(refused());
    }
    authority
        .to_socket_addrs()
        .map_err(|_| refused())?
        .find(|address| address.ip().is_loopback())
        .ok_or_else(refused)
}

fn broken(error: impl std::fmt::Display) -> LinkError {
    LinkError::Gone(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use serde_json::{Map, Value, json};
    use tungstenite::{Message, WebSocket};

    use super::{LinkError, MAX_LATE, Socket, ahead_of, keep_within_bound, loopback_address};

    /// How long the link waits for what the browser's side sends at once.
    const WAIT: Duration = Duration::from_secs(10);

    /// A browser's WebSocket on the loopback, driven by `browser` on its own thread.
    fn browser(
        browser: impl FnOnce(WebSocket<std::net::TcpStream>) + Send + 'static,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let url = format!("ws://{}/", listener.local_addr().expect("an address"));
        let thread = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("the link connects");
            browser(tungstenite::accept(stream).expect("the handshake"));
        });
        (url, thread)
    }

    /// The `id` of the next request the browser's side reads.
    fn read_id(ws: &mut WebSocket<std::net::TcpStream>) -> Value {
        let request = ws.read().expect("a request");
        serde_json::from_str::<Value>(&request.into_text().expect("text")).expect("JSON")["id"]
            .clone()
    }

    fn holding(id: &Value, object: &str) -> Message {
        Message::text(
            json!({ "id": id, "result": { "result": { "objectId": object } } }).to_string(),
        )
    }

    fn call() -> Map<String, Value> {
        let mut message = Map::new();
        message.insert("method".into(), Value::from("call"));
        message
    }

    /// An answer that comes after its request gave up, while the link waits for an event, is
    /// kept for the link, apart from the events. Fails if it is dropped: what it holds in the
    /// page would stay held for the life of the document.
    #[test]
    fn a_late_answer_read_while_waiting_for_an_event_is_kept() {
        let (gave_up, given_up) = mpsc::channel::<()>();
        let (url, thread) = browser(move |mut ws| {
            let id = read_id(&mut ws);
            given_up.recv().expect("the request gave up");
            ws.send(holding(&id, "late")).expect("the answer goes");
            ws.send(Message::text(json!({ "method": "after" }).to_string()))
                .expect("the event goes");
            let _ = ws.read();
        });
        let mut socket = Socket::connect(&url, WAIT).expect("the socket");
        let asked = socket.request(call(), Instant::now() + Duration::from_millis(50));
        assert_eq!(asked.map(drop), Err(LinkError::Timeout));
        gave_up.send(()).expect("the browser's side waits");
        let event = socket
            .next_event(Instant::now() + WAIT)
            .expect("the link reads")
            .expect("the event");
        assert_eq!(event["method"], "after");
        let late = socket.take_late();
        assert_eq!(late.len(), 1);
        assert_eq!(late[0]["result"]["result"]["objectId"], "late");
        drop(socket);
        thread.join().expect("the browser's side");
    }

    /// An answer that comes after its request gave up, while the link waits for the answer to
    /// another request, is kept for the link. Fails if it is passed over: what it holds in the
    /// page would stay held for the life of the document.
    #[test]
    fn a_late_answer_read_while_waiting_for_another_is_kept() {
        let (url, thread) = browser(|mut ws| {
            let first = read_id(&mut ws);
            // The second request comes once the first gave up.
            let second = read_id(&mut ws);
            ws.send(holding(&first, "late"))
                .expect("the late answer goes");
            ws.send(holding(&second, "answer"))
                .expect("the answer goes");
            let _ = ws.read();
        });
        let mut socket = Socket::connect(&url, WAIT).expect("the socket");
        let asked = socket.request(call(), Instant::now() + Duration::from_millis(50));
        assert_eq!(asked.map(drop), Err(LinkError::Timeout));
        let answered = socket
            .request(call(), Instant::now() + WAIT)
            .expect("the second is answered");
        assert_eq!(answered["result"]["result"]["objectId"], "answer");
        let late = socket.take_late();
        assert_eq!(late.len(), 1);
        assert_eq!(late[0]["result"]["result"]["objectId"], "late");
        drop(socket);
        thread.join().expect("the browser's side");
    }

    /// At most [`MAX_LATE`] late answers are kept, the oldest going first, and what the link did
    /// not release goes back ahead of what came since, within the same bound. Fails if the
    /// answers kept grow without bound, or the newest are the ones dropped.
    #[test]
    fn the_late_answers_kept_are_bounded() {
        let answer = |n: usize| json!({ "n": n });
        let mut late = VecDeque::new();
        for n in 0..=MAX_LATE {
            keep_within_bound(&mut late, answer(n));
        }
        assert_eq!(late.len(), MAX_LATE);
        assert_eq!(late.front(), Some(&answer(1)), "the oldest went");
        assert_eq!(late.back(), Some(&answer(MAX_LATE)));

        let unreleased = (0..2).map(answer).collect::<VecDeque<Value>>();
        let kept = (2..=MAX_LATE).map(answer).collect::<VecDeque<Value>>();
        let merged = ahead_of(unreleased, kept);
        assert_eq!(merged.len(), MAX_LATE);
        assert_eq!(merged.front(), Some(&answer(1)), "the oldest went");
        assert_eq!(merged.back(), Some(&answer(MAX_LATE)), "the newest stayed");
    }

    #[test]
    fn a_browser_socket_is_reached_on_the_loopback_alone() {
        for (url, loopback) in [
            ("ws://127.0.0.1:9222/devtools/browser/abc", true),
            ("ws://localhost:9222/session", true),
            ("ws://[::1]:9222/session", true),
            ("ws://10.0.0.2:9222/session", false),
            ("ws://example.com:9222/session", false),
            ("wss://127.0.0.1:9222/session", false),
            ("http://127.0.0.1:9222/", false),
        ] {
            assert_eq!(loopback_address(url).is_ok(), loopback, "{url}");
        }
    }
}
