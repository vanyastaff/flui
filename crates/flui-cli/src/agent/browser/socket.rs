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
    /// Answers that came after their request gave up waiting, oldest first: what they hold in
    /// the page is the link's to release ([`Socket::take_late`]).
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

    /// The oldest answer that came after its request gave up waiting.
    pub(super) fn take_late(&mut self) -> Option<Value> {
        self.late.pop_front()
    }

    fn keep_late(&mut self, answer: Value) {
        if self.late.len() >= MAX_LATE {
            self.late.pop_front();
        }
        self.late.push_back(answer);
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
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use serde_json::{Map, Value, json};
    use tungstenite::Message;

    use super::{LinkError, Socket, loopback_address};

    /// An answer that comes after its request gave up waiting is kept for the link, apart from
    /// the events, which still come in order. Fails if a late answer is dropped: what it holds
    /// in the page would stay held for the life of the document.
    #[test]
    fn a_late_answer_is_kept_for_the_link() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let url = format!("ws://{}/", listener.local_addr().expect("an address"));
        let browser = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("the link connects");
            let mut ws = tungstenite::accept(stream).expect("the handshake");
            let request = ws.read().expect("a request");
            let request =
                serde_json::from_str::<Value>(&request.into_text().expect("text")).expect("JSON");
            // Answered only after the request gave up.
            std::thread::sleep(Duration::from_millis(300));
            let answer =
                json!({ "id": request["id"], "result": { "result": { "objectId": "late" } } });
            ws.send(Message::text(answer.to_string()))
                .expect("the answer goes");
            ws.send(Message::text(json!({ "method": "after" }).to_string()))
                .expect("the event goes");
            // Kept open until the link has read both.
            let _ = ws.read();
        });
        let mut socket = Socket::connect(&url, Duration::from_secs(5)).expect("the socket");
        let mut message = Map::new();
        message.insert("method".into(), Value::from("call"));
        let asked = socket.request(message, Instant::now() + Duration::from_millis(50));
        assert_eq!(asked.map(drop), Err(LinkError::Timeout));
        let event = socket
            .next_event(Instant::now() + Duration::from_secs(5))
            .expect("the link reads")
            .expect("the event");
        assert_eq!(event["method"], "after");
        let late = socket.take_late().expect("the late answer is kept");
        assert_eq!(late["result"]["result"]["objectId"], "late");
        assert!(socket.take_late().is_none());
        drop(socket);
        browser.join().expect("the browser's side");
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
