//! The conversation with a web app's agent through its page export (ADR-0239 §3, ADR-0288 §6),
//! carried by the Chrome lane's WebDriver BiDi session.
//!
//! The page defines its agent's export as a global (`__flui_agent`, the launch's endpoint):
//! `connect(onLine)` opens a connection and answers `{ version, send(line), close() }`. The driver
//! calls it with a BiDi channel as `onLine`, so each line the app writes reaches the driver as a
//! `script.message` on that channel, and `onLine(null)` as the app ending the connection; it sends
//! a line by calling `send` on the connection the page answered (`script.callFunction`). The
//! lines are the same as over a socket: a hello, then one request at a time, each answered by the
//! line that carries its id.
//!
//! The session's thread hands the page's lines over in order, bounded (`lane::chrome::bidi`), and
//! the first reason the conversation broke is recorded once: a broken socket, the tab closed, the
//! app ending the connection, more lines than the driver reads. Every later call fails at once with
//! it, as the app gone.

use std::time::{Duration, Instant};

use flui_protocol::events::Event;
use flui_protocol::launch::PAGE_EXPORT_VERSION;
use flui_protocol::session::HelloReply;
use serde_json::{Value, json};

use super::{Call, Connection, Events, Heard};
use crate::error::{Error, Result};
use crate::lane::chrome::bidi::{COMMAND_TIMEOUT, PageEvent, PageLines, Session};

/// The global a web app defines its agent's export as: the endpoint a launch names.
pub(crate) const PAGE_EXPORT: &str = "__flui_agent";

/// How long the hello's answer may take.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the page is asked again for its export while it is still loading.
const CONNECT_RETRY: Duration = Duration::from_millis(100);

/// Opens a connection to the export named by its first argument, its second the channel each line
/// is reported on; `null` while the page defines no export yet.
const CONNECT: &str = "(name, onLine) => {
    const page = globalThis[name];
    if (page === undefined || page === null || typeof page.connect !== 'function') {
        return null;
    }
    if (page.version !== VERSION) {
        throw new Error(`the page's agent speaks version ${page.version} of its export, not VERSION`);
    }
    return page.connect((line) => onLine(line));
}";

/// Sends its argument on the connection it is called on.
const SEND: &str = "function (line) { this.send(line); }";

/// Ends the connection it is called on.
const CLOSE: &str = "function () { this.close(); }";

/// A conversation over a page export.
pub(crate) struct BidiConnection {
    session: Session,
    lines: PageLines,
    /// The page's connection object, by the browser's handle.
    remote: String,
    hello: HelloReply,
    next_id: u64,
    events: Events,
    /// Why the conversation broke, once it did.
    broken: Option<String>,
    opened: Instant,
}

impl BidiConnection {
    /// Connect to the page's export through `session`, asking again while the page still loads
    /// until `wait` has passed, and present `token`.
    pub(crate) fn connect(
        session: Session,
        lines: PageLines,
        token: &str,
        wait: Duration,
    ) -> Result<Self> {
        let now = Instant::now();
        let deadline = now.checked_add(wait).unwrap_or(now + COMMAND_TIMEOUT);
        let connect = CONNECT.replace("VERSION", &PAGE_EXPORT_VERSION.to_string());
        let remote = loop {
            let result = session.call_function(
                &connect,
                None,
                vec![
                    json!({ "type": "string", "value": PAGE_EXPORT }),
                    json!({ "type": "channel", "value": { "channel": session.channel() } }),
                ],
                "root",
            )?;
            if let Some(handle) = result.get("handle").and_then(Value::as_str) {
                break handle.to_owned();
            }
            if Instant::now() >= deadline {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "the page defined no agent export `{PAGE_EXPORT}` within {} ms",
                        wait.as_millis()
                    ),
                )));
            }
            std::thread::sleep(CONNECT_RETRY);
        };
        let mut connection = Self {
            session,
            lines,
            remote,
            hello: HelloReply::new(flui_protocol::version::PROTOCOL_VERSION),
            next_id: 1,
            events: Events::default(),
            broken: None,
            opened: Instant::now(),
        };
        connection.greet(token)?;
        Ok(connection)
    }

    fn greet(&mut self, token: &str) -> Result<()> {
        self.send(&super::hello_line(token)?)?;
        let deadline = Instant::now() + HELLO_TIMEOUT;
        loop {
            let Some(line) = self.receive(deadline).map_err(|why| Error::Protocol {
                message: format!(
                    "the page closed the connection at the hello ({why}): the token is wrong, or \
                     the export is not a FLUI agent"
                ),
            })?
            else {
                return Err(Error::Protocol {
                    message: "the page did not answer the hello".into(),
                });
            };
            match super::hear(line.as_bytes(), None)? {
                Heard::Hello(hello) => {
                    self.hello = *hello;
                    return Ok(());
                }
                Heard::Answer(Err(refused)) => return Err(refused),
                Heard::Event(event) => self.events.push(event),
                Heard::Answer(Ok(_)) | Heard::Other => {}
            }
        }
    }

    /// Send `line`, its newline dropped: the page takes a line without one.
    fn send(&mut self, line: &[u8]) -> Result<()> {
        if let Some(why) = &self.broken {
            return Err(Error::process_gone(why.clone(), None));
        }
        let text = std::str::from_utf8(line)
            .map_err(|_| Error::Protocol {
                message: "a request is not UTF-8".into(),
            })?
            .trim_end_matches('\n');
        let sent = self.session.call_function(
            SEND,
            Some(&self.remote),
            vec![json!({ "type": "string", "value": text })],
            "none",
        );
        match sent {
            Ok(_) => Ok(()),
            // The browser did not say whether the page took it: the request may be in the app.
            Err(error @ Error::Timeout { .. }) => Err(error),
            Err(error) => {
                // The page's connection is closed, or the session broke: the conversation is over.
                let why = format!("the request could not be handed to the page: {error}");
                self.broken.get_or_insert(why.clone());
                Err(Error::process_gone(why, None))
            }
        }
    }

    /// The next line, waiting until `deadline`: `Ok(None)` when none came by then, `Err` with why
    /// once the conversation broke.
    fn receive(&mut self, deadline: Instant) -> std::result::Result<Option<String>, String> {
        if let Some(why) = &self.broken {
            return Err(why.clone());
        }
        loop {
            let why = match self.lines.next(deadline) {
                Ok(Some(PageEvent::Line(line))) => return Ok(Some(line)),
                Ok(Some(PageEvent::ContextGone(context))) if context != self.session.context() => {
                    continue;
                }
                Ok(Some(PageEvent::ContextGone(_))) => "the page's tab closed".to_owned(),
                Ok(Some(PageEvent::Ended)) => "the app closed the connection".to_owned(),
                Ok(Some(PageEvent::Broken(why))) => why,
                Ok(None) => return Ok(None),
                Err(()) => "the browser session ended".to_owned(),
            };
            self.broken = Some(why.clone());
            return Err(why);
        }
    }
}

impl Connection for BidiConnection {
    fn hello(&self) -> &HelloReply {
        &self.hello
    }

    fn call(&mut self, call: Call) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        match self.send(&super::request_line(id, &call)?) {
            Ok(()) => {}
            Err(Error::Timeout { .. }) => return Err(super::unanswered(&call)),
            Err(error) => return Err(error),
        }
        let deadline = Instant::now() + call.budget.min(super::MAX_BUDGET);
        loop {
            let line = match self.receive(deadline) {
                Ok(Some(line)) => line,
                Ok(None) => return Err(super::unanswered(&call)),
                Err(why) => return Err(Error::process_gone(why, None)),
            };
            match super::hear(line.as_bytes(), Some(id))? {
                Heard::Answer(answer) => return answer,
                Heard::Event(event) => self.events.push(event),
                Heard::Hello(_) => {
                    return Err(Error::Protocol {
                        message: "the app said hello again".into(),
                    });
                }
                Heard::Other => {}
            }
        }
    }

    fn take_events(&mut self) -> (Vec<Event>, u64) {
        // Lines already handed over go to the queue first, so nothing the app sent before is left
        // out.
        while let Ok(Some(line)) = self.receive(Instant::now()) {
            if let Ok(Heard::Event(event)) = super::hear(line.as_bytes(), None) {
                self.events.push(event);
            }
        }
        self.events.take()
    }

    fn clock(&self) -> Duration {
        self.opened.elapsed()
    }
}

impl Drop for BidiConnection {
    fn drop(&mut self) {
        // The driver leaves: the page's connection is closed and its handle let go. A session
        // that already broke has nothing to close.
        if self.broken.is_some() {
            return;
        }
        let _ = self
            .session
            .call_function(CLOSE, Some(&self.remote), Vec::new(), "none");
        let _ = self.session.command(
            "script.disown",
            json!({ "handles": [self.remote], "target": { "context": self.session.context() } }),
            COMMAND_TIMEOUT,
        );
    }
}
