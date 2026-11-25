//! The session's local endpoint, served by this command for an app in a page: each connection
//! a client (`flui mcp`) makes is carried, line for line, to a connection of the page's export.
//!
//! One thread does all of it, polling: the listener and every client are nonblocking, and the
//! wait for the tab's next event is the pause between turns ([`TICK`]). A client's lines wait
//! in order until its page connection is open: the export appears only once the app's module
//! has started, and again after each reload. A reload ends every page connection, so the relay
//! closes their clients, which connect again.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_protocol::endpoint::PolledStream;
use flui_protocol::framing::{LineReader, MAX_LINE};
use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{Listener, Stream};

use super::super::listener::{bind, polled};
use super::super::unwritten::Unwritten;
use super::{
    BrowserLink, CLOSE_FUNCTION, LinkError, LinkEvent, Remote, SEND_FUNCTION, connect_function,
    read_report,
};

/// The longest pause between two turns of the relay.
const TICK: Duration = Duration::from_millis(10);

/// How often a client waiting for the page's export tries it again.
const CONNECT_RETRY: Duration = Duration::from_millis(100);

/// How long a client waits for the page's export before it is closed.
const CONNECT_WAIT: Duration = Duration::from_secs(30);

/// The most lines a client sends before its page connection is open.
const MAX_QUEUED: usize = 16;

/// The most bytes kept for a client that does not read: one that falls further behind is
/// closed at once, its page connection with it.
pub(super) const MAX_UNWRITTEN: usize = 64 << 20;

/// The relay's thread: dropping the handle stops it, ending every connection.
pub(crate) struct Relay {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Relay {
    /// Serves `endpoint`, carrying each connection to the export named `export` in the tab
    /// `link` drives.
    ///
    /// # Errors
    ///
    /// The endpoint could not be bound, or the thread not started.
    pub(crate) fn start(
        link: Box<dyn BrowserLink>,
        endpoint: &str,
        export: String,
    ) -> io::Result<Self> {
        let listener = bind(endpoint)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("flui-agent-relay".into())
            .spawn(move || {
                let mut relay = Turns {
                    link,
                    export,
                    connect: connect_function(),
                    clients: BTreeMap::new(),
                    next_id: 0,
                };
                if let Err(error) = relay.run(&listener, &stopped) {
                    let _ = crate::ui::warning(format!("the agent's page link ended: {error}"));
                }
                relay.end();
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Whether the relay still serves: it ends when the browser or its tab goes.
    pub(crate) fn is_serving(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The relay's state, owned by its thread.
struct Turns {
    link: Box<dyn BrowserLink>,
    export: String,
    /// [`connect_function`], made once.
    connect: String,
    clients: BTreeMap<u64, Client>,
    next_id: u64,
}

/// One client's connection and the page connection it is carried to.
struct Client {
    stream: PolledStream<Stream>,
    reader: LineReader,
    /// What is still to be written to the client.
    unwritten: Unwritten,
    /// Lines read before the page connection opened.
    queued: VecDeque<String>,
    page: Page,
    /// The page ended the connection: the client is closed once what it was sent is written,
    /// and the connection is not closed again in the page.
    ending: bool,
}

/// Where a client's page connection is.
enum Page {
    /// The page defines no export yet; tried again at `retry`.
    Waiting { since: Instant, retry: Instant },
    /// Open, as the page keeps it.
    Open(Remote),
}

/// What a client's turn decided.
enum Outcome {
    Keep,
    Leave(Leaving),
}

/// How a client leaves the relay. Either way what the page holds of an open connection is
/// released ([`BrowserLink::release`]), so it does not live as long as the document.
#[derive(Clone, Copy)]
enum Leaving {
    /// The client went, broke the protocol or fell too far behind, or the relay stops: its
    /// page connection is closed, which ends its place in the app's agent.
    Close,
    /// The page ended the connection, or never opened it: nothing is left to close there.
    Ended,
}

impl Turns {
    fn run(&mut self, listener: &Listener, stop: &AtomicBool) -> Result<(), LinkError> {
        while !stop.load(Ordering::SeqCst) {
            self.accept(listener);
            let ids = self.clients.keys().copied().collect::<Vec<u64>>();
            for id in ids {
                if let Outcome::Leave(leaving) = self.turn(id)? {
                    self.leave(id, leaving);
                }
            }
            let deadline = Instant::now() + TICK;
            while let Some(event) = self.link.next_event(deadline)? {
                self.deliver(event);
            }
        }
        Ok(())
    }

    /// Takes every client waiting on the listener.
    fn accept(&mut self, listener: &Listener) {
        loop {
            let stream = match listener.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    crate::ui::debug(format!("the agent relay accepted no client: {error}"));
                    return;
                }
            };
            let stream = match polled(stream) {
                Ok(stream) => stream,
                Err(error) => {
                    // Dropped: the client reads the end of its connection.
                    crate::ui::debug(format!("the agent relay could not poll a client: {error}"));
                    continue;
                }
            };
            let Some(next) = self.next_id.checked_add(1) else {
                // Refused: an ID is never used twice.
                return;
            };
            let id = self.next_id;
            self.next_id = next;
            let now = Instant::now();
            self.clients.insert(
                id,
                Client {
                    stream,
                    reader: LineReader::new(MAX_LINE),
                    unwritten: Unwritten::default(),
                    queued: VecDeque::new(),
                    page: Page::Waiting {
                        since: now,
                        retry: now,
                    },
                    ending: false,
                },
            );
        }
    }

    /// Reads what the client `id` sent, opens its page connection, carries its lines and
    /// writes what it was sent.
    fn turn(&mut self, id: u64) -> Result<Outcome, LinkError> {
        let Some(client) = self.clients.get_mut(&id) else {
            return Ok(Outcome::Keep);
        };
        let read = client.read();
        if client.ending {
            return Ok(match client.write() {
                Ok(true) | Err(_) => Outcome::Leave(Leaving::Ended),
                Ok(false) => Outcome::Keep,
            });
        }
        if let Page::Waiting { since, retry } = client.page {
            let now = Instant::now();
            if now >= retry {
                match self
                    .link
                    .call(&self.connect, &[&self.export, &id.to_string()])
                {
                    Ok(Some(remote)) => client.page = Page::Open(remote),
                    Ok(None) if now.duration_since(since) < CONNECT_WAIT => {
                        client.page = Page::Waiting {
                            since,
                            retry: now + CONNECT_RETRY,
                        };
                    }
                    Ok(None) => {
                        crate::ui::debug("the page defined no agent export in time");
                        return Ok(Outcome::Leave(Leaving::Ended));
                    }
                    Err(LinkError::Gone(why)) => return Err(LinkError::Gone(why)),
                    Err(error) => {
                        crate::ui::debug(format!("the page refused a connection: {error}"));
                        return Ok(Outcome::Leave(Leaving::Ended));
                    }
                }
            }
        }
        if let Page::Open(remote) = &client.page {
            while let Some(line) = client.queued.pop_front() {
                match self.link.call_on(remote, SEND_FUNCTION, &[&line]) {
                    Ok(()) => {}
                    Err(LinkError::Gone(why)) => return Err(LinkError::Gone(why)),
                    // The connection ended in the page: its end is reported, or was.
                    Err(_) => return Ok(Outcome::Leave(Leaving::Close)),
                }
            }
        }
        if !read {
            return Ok(Outcome::Leave(Leaving::Close));
        }
        Ok(match client.write() {
            Ok(_) => Outcome::Keep,
            Err(_) => Outcome::Leave(Leaving::Close),
        })
    }

    /// Hands the tab's event to the client it concerns.
    fn deliver(&mut self, event: LinkEvent) {
        match event {
            LinkEvent::Report(report) => {
                let Some((id, line)) = read_report(&report) else {
                    return;
                };
                let Some(client) = self.clients.get_mut(&id) else {
                    return;
                };
                match line {
                    Some(line) => {
                        if client.unwritten.len() + line.len() + 1 > MAX_UNWRITTEN {
                            // A client this far behind is given up on at once: a line written
                            // after a gap would read as one it was sent whole.
                            let leaving = client.leaving();
                            self.leave(id, leaving);
                            return;
                        }
                        client.unwritten.extend_from_slice(line.as_bytes());
                        client.unwritten.extend_from_slice(b"\n");
                    }
                    None => client.ending = true,
                }
            }
            // The document goes, and every page connection with it, so their clients are
            // closed. Each is closed in the page all the same: one opened while the event was
            // on its way belongs to the next document, and would hold its place in the app's
            // agent. A client still waiting for the export connects to the next document.
            LinkEvent::Navigated => {
                let open = self
                    .clients
                    .iter()
                    .filter(|(_, client)| matches!(client.page, Page::Open(_)))
                    .map(|(id, client)| (*id, client.leaving()))
                    .collect::<Vec<(u64, Leaving)>>();
                for (id, leaving) in open {
                    self.leave(id, leaving);
                }
            }
        }
    }

    /// Lets go of the client `id`: its page connection is closed as `leaving` says, and
    /// released.
    fn leave(&mut self, id: u64, leaving: Leaving) {
        let Some(client) = self.clients.remove(&id) else {
            return;
        };
        if let Page::Open(remote) = client.page {
            if matches!(leaving, Leaving::Close) {
                let _ = self.link.call_on(&remote, CLOSE_FUNCTION, &[]);
            }
            self.link.release(remote);
        }
    }

    /// Ends every connection: the relay stops.
    fn end(&mut self) {
        let leaving = self
            .clients
            .iter()
            .map(|(id, client)| (*id, client.leaving()))
            .collect::<Vec<(u64, Leaving)>>();
        for (id, leaving) in leaving {
            self.leave(id, leaving);
        }
    }
}

impl Client {
    /// How the client leaves when the relay lets go of it before it is done: closed in the
    /// page, unless the page ended the connection itself.
    fn leaving(&self) -> Leaving {
        if self.ending {
            Leaving::Ended
        } else {
            Leaving::Close
        }
    }

    /// Reads what the client sent, queueing its lines; `false` once it went or broke the
    /// protocol.
    fn read(&mut self) -> bool {
        let mut chunk = [0_u8; 16 << 10];
        loop {
            match (&self.stream).read(&mut chunk) {
                Ok(0) => return false,
                Ok(read) => self.reader.push(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
            loop {
                match self.reader.next_line() {
                    Ok(Some(line)) => {
                        let Ok(line) = String::from_utf8(line) else {
                            return false;
                        };
                        if self.ending {
                            continue;
                        }
                        if self.queued.len() >= MAX_QUEUED {
                            return false;
                        }
                        self.queued.push_back(line);
                    }
                    Ok(None) => break,
                    Err(_) => return false,
                }
            }
        }
        true
    }

    /// Writes what it can of what the client was sent: `true` once all of it is written.
    fn write(&mut self) -> io::Result<bool> {
        self.unwritten.write_to(&self.stream)
    }
}
