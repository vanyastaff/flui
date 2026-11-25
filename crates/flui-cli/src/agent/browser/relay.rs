//! The session's local endpoint, served by this command for an app in a page: each connection
//! a client (`flui mcp`) makes is carried, line for line, to a connection of the page's export.
//!
//! One thread does all of it, polling: the listener and every client are nonblocking, and the
//! wait for the tab's next event is the pause between turns ([`TICK`]). A client's lines wait
//! in order until its page connection is open: the export appears only once the app's module
//! has started, and again after each reload. A reload ends every page connection, so the relay
//! closes their clients, which connect again; a client whose connection the page had ended
//! first is written what the page sent it before it is closed.
//!
//! What the relay holds of each connection in the page, and what letting its client go must do
//! there, is one value, the client's [`Page`]: every transition is a change of it, and
//! [`Turns::leave`] reads it alone.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read as _};
use std::mem;
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
    ABANDON_FUNCTION, BrowserLink, CLOSE_FUNCTION, LinkError, LinkEvent, Remote, SEND_FUNCTION,
    connect_function, read_report,
};

/// The longest pause between two turns of the relay.
const TICK: Duration = Duration::from_millis(10);

/// How often a client waiting for the page's export tries it again, and how often a connection
/// the page did not answer is abandoned again.
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
                    drains: 0,
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
    /// How many times the tab's events were delivered: the drain a [`Page::Refused`] waits
    /// one past.
    drains: u64,
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
}

/// Where a client's page connection is: what the relay holds of it in the page, and so what
/// letting the client go does there ([`Turns::leave`]).
enum Page {
    /// The page defines no export yet; tried again at `retry`. Nothing is in the page.
    Waiting { since: Instant, retry: Instant },
    /// Open, as the page keeps it. Letting go closes it in the page and releases it.
    Open(Remote),
    /// The page threw at a line sent on the connection during the turn before the drain
    /// numbered `drain`. Either the page ended the connection, and its end and the lines before
    /// it reached the link before the throw did, so that drain delivers them and the connection
    /// becomes [`Page::Ended`]; or the connection broke, and the turn after that drain closes it
    /// in the page and writes the client what it is owed, as for an ended one. Letting go closes
    /// it in the page, which does nothing to a connection the page ended, and releases it.
    Refused { remote: Remote, drain: u64 },
    /// Nothing is left open in the page: the page ended the connection, or the relay closed it
    /// after a refusal. The client is written what it was sent, a reload notwithstanding, since
    /// those bytes are the relay's already; then the connection is released, never closed again.
    Ended(Remote),
    /// The page did not answer the connect call in time, which does not mean it never runs
    /// it: it may open the connection later, with nobody holding it. The relay abandons the
    /// connection by its ID ([`ABANDON_FUNCTION`]), again at `retry` while the page does not
    /// answer, and the client is closed once the page has. Letting go before that abandons it
    /// once more.
    Abandoning { retry: Instant },
    /// The page answered the abandon: nothing of the connection is there, nor will be.
    Abandoned,
}

/// What a client's turn decided.
enum Outcome {
    Keep,
    /// The client goes, and its connection in the page as its [`Page`] says.
    Leave,
}

impl Turns {
    fn run(&mut self, listener: &Listener, stop: &AtomicBool) -> Result<(), LinkError> {
        while !stop.load(Ordering::SeqCst) {
            self.accept(listener);
            let ids = self.clients.keys().copied().collect::<Vec<u64>>();
            for id in ids {
                if let Outcome::Leave = self.turn(id)? {
                    self.leave(id);
                }
            }
            let deadline = Instant::now() + TICK;
            while let Some(event) = self.link.next_event(deadline)? {
                self.deliver(event);
            }
            self.drains = self.drains.saturating_add(1);
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
        let now = Instant::now();
        let connection = id.to_string();
        if let Page::Waiting { since, retry } = client.page
            && now >= retry
        {
            match self.link.call(&self.connect, &[&self.export, &connection]) {
                Ok(Some(remote)) => client.page = Page::Open(remote),
                Ok(None) if now.duration_since(since) < CONNECT_WAIT => {
                    client.page = Page::Waiting {
                        since,
                        retry: now + CONNECT_RETRY,
                    };
                }
                Ok(None) => {
                    crate::ui::debug("the page defined no agent export in time");
                    return Ok(Outcome::Leave);
                }
                Err(LinkError::Gone(why)) => return Err(LinkError::Gone(why)),
                Err(LinkError::Timeout) => {
                    crate::ui::debug("the page did not answer a connection in time");
                    client.page = Page::Abandoning { retry: now };
                }
                // The page answered: the call opened nothing.
                Err(error) => {
                    crate::ui::debug(format!("the page refused a connection: {error}"));
                    return Ok(Outcome::Leave);
                }
            }
        }
        if let Page::Abandoning { retry } = client.page {
            if now < retry {
                return Ok(Outcome::Keep);
            }
            return match self.link.call(ABANDON_FUNCTION, &[&connection]) {
                Err(LinkError::Gone(why)) => Err(LinkError::Gone(why)),
                Err(LinkError::Timeout) => {
                    client.page = Page::Abandoning {
                        retry: now + CONNECT_RETRY,
                    };
                    Ok(Outcome::Keep)
                }
                answered => {
                    if let Err(error) = answered {
                        crate::ui::debug(format!(
                            "the page refused to abandon a connection: {error}"
                        ));
                    }
                    client.page = Page::Abandoned;
                    Ok(Outcome::Leave)
                }
            };
        }
        client.page = match mem::replace(&mut client.page, Page::Abandoned) {
            // The drain after the refusal brought no end: the connection broke in the page.
            Page::Refused { remote, drain } if self.drains > drain => {
                let _ = self.link.call_on(&remote, CLOSE_FUNCTION, &[&connection]);
                Page::Ended(remote)
            }
            page => page,
        };
        if let Page::Open(remote) = &client.page {
            let mut refused = false;
            while let Some(line) = client.queued.pop_front() {
                match self.link.call_on(remote, SEND_FUNCTION, &[&line]) {
                    Ok(()) => {}
                    Err(LinkError::Gone(why)) => return Err(LinkError::Gone(why)),
                    Err(LinkError::Threw(_)) => {
                        refused = true;
                        break;
                    }
                    // Unanswered, or refused by the browser: the connection may be open.
                    Err(_) => return Ok(Outcome::Leave),
                }
            }
            if refused {
                client.queued.clear();
                client.page = match mem::replace(&mut client.page, Page::Abandoned) {
                    Page::Open(remote) => Page::Refused {
                        remote,
                        drain: self.drains,
                    },
                    page => page,
                };
            }
        }
        if let Page::Ended(_) = client.page {
            return Ok(match client.write() {
                Ok(true) | Err(_) => Outcome::Leave,
                Ok(false) => Outcome::Keep,
            });
        }
        if !read {
            return Ok(Outcome::Leave);
        }
        Ok(match client.write() {
            Ok(_) => Outcome::Keep,
            Err(_) => Outcome::Leave,
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
                // Only a connection the relay holds open reports to its client: one the page
                // ended reports nothing after its end, and one being abandoned is no client's.
                if !matches!(client.page, Page::Open(_) | Page::Refused { .. }) {
                    return;
                }
                match line {
                    Some(line) => {
                        if client.unwritten.len() + line.len() + 1 > MAX_UNWRITTEN {
                            // A client this far behind is given up on at once: a line written
                            // after a gap would read as one it was sent whole.
                            self.leave(id);
                            return;
                        }
                        client.unwritten.extend_from_slice(line.as_bytes());
                        client.unwritten.extend_from_slice(b"\n");
                    }
                    None => {
                        client.page = match mem::replace(&mut client.page, Page::Abandoned) {
                            Page::Open(remote) | Page::Refused { remote, .. } => {
                                Page::Ended(remote)
                            }
                            page => page,
                        };
                    }
                }
            }
            // The document goes, and every connection open in it, so their clients are closed.
            // Each is closed in the page all the same, and released: one opened while the event
            // was on its way belongs to the next document, and would hold its place in the
            // app's agent. A connection the page ended is left to finish writing its client,
            // a client still waiting for the export connects to the next document, and an
            // abandon still owed is still made.
            LinkEvent::Navigated => {
                let open = self
                    .clients
                    .iter()
                    .filter(|(_, client)| {
                        matches!(client.page, Page::Open(_) | Page::Refused { .. })
                    })
                    .map(|(id, _)| *id)
                    .collect::<Vec<u64>>();
                for id in open {
                    self.leave(id);
                }
            }
        }
    }

    /// Lets go of the client `id`, doing in the page what its [`Page`] says: an open connection
    /// is closed and released, an ended one released, an abandon still owed made once more.
    fn leave(&mut self, id: u64) {
        let Some(client) = self.clients.remove(&id) else {
            return;
        };
        let connection = id.to_string();
        match client.page {
            Page::Waiting { .. } | Page::Abandoned => {}
            Page::Open(remote) | Page::Refused { remote, .. } => {
                let _ = self.link.call_on(&remote, CLOSE_FUNCTION, &[&connection]);
                self.link.release(remote);
            }
            Page::Ended(remote) => self.link.release(remote),
            // Only as the relay stops: the last try it can make.
            Page::Abandoning { .. } => {
                let _ = self.link.call(ABANDON_FUNCTION, &[&connection]);
            }
        }
    }

    /// Ends every connection: the relay stops, and what it owes its clients is dropped.
    fn end(&mut self) {
        let ids = self.clients.keys().copied().collect::<Vec<u64>>();
        for id in ids {
            self.leave(id);
        }
    }
}

impl Client {
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
                        // A line can go only on a connection open or still to open.
                        if !matches!(self.page, Page::Waiting { .. } | Page::Open(_)) {
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
