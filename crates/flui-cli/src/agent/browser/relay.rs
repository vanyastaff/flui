//! The session's local endpoint, served by this command for an app in a page: each connection
//! a client (`flui mcp`) makes is carried, line for line, to a connection of the page's export.
//!
//! One thread does all of it, polling: the listener and every client are nonblocking, and the
//! wait for the tab's next event is the pause between turns ([`TICK`]). A client's lines wait
//! in order until its page connection is open: the export appears only once the app's module
//! has started, and again after each reload. A reload ends every page connection, so the relay
//! closes their clients, which connect again.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_protocol::framing::{LineReader, MAX_LINE};
use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{Listener, ListenerNonblockingMode, ListenerOptions, Stream};

use super::super::client::name;
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

/// The most bytes kept for a client that does not read.
const MAX_UNWRITTEN: usize = 64 << 20;

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
    stream: Stream,
    reader: LineReader,
    /// What is still to be written to the client.
    unwritten: Vec<u8>,
    /// Lines read before the page connection opened.
    queued: VecDeque<String>,
    page: Page,
    /// The page ended the connection: the client is closed once what it was sent is written.
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
    /// The client went, or broke the protocol: its page connection is closed.
    Close,
    /// The page ended the connection, or never opened it: nothing is left to close there.
    Drop,
}

impl Turns {
    fn run(&mut self, listener: &Listener, stop: &AtomicBool) -> Result<(), LinkError> {
        while !stop.load(Ordering::SeqCst) {
            self.accept(listener);
            let ids = self.clients.keys().copied().collect::<Vec<u64>>();
            for id in ids {
                match self.turn(id)? {
                    Outcome::Keep => {}
                    Outcome::Close => self.close(id),
                    Outcome::Drop => {
                        self.clients.remove(&id);
                    }
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
                    unwritten: Vec::new(),
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
                Ok(true) | Err(_) => Outcome::Drop,
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
                        return Ok(Outcome::Drop);
                    }
                    Err(LinkError::Gone(why)) => return Err(LinkError::Gone(why)),
                    Err(error) => {
                        crate::ui::debug(format!("the page refused a connection: {error}"));
                        return Ok(Outcome::Drop);
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
                    Err(_) => return Ok(Outcome::Close),
                }
            }
        }
        if !read {
            return Ok(Outcome::Close);
        }
        Ok(match client.write() {
            Ok(_) => Outcome::Keep,
            Err(_) => Outcome::Close,
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
                            // A client this far behind is closed with the rest.
                            client.ending = true;
                            client.unwritten.clear();
                            return;
                        }
                        client.unwritten.extend_from_slice(line.as_bytes());
                        client.unwritten.push(b'\n');
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
                    .map(|(id, _)| *id)
                    .collect::<Vec<u64>>();
                for id in open {
                    self.close(id);
                }
            }
        }
    }

    /// Closes the client `id` and its page connection.
    fn close(&mut self, id: u64) {
        let Some(client) = self.clients.remove(&id) else {
            return;
        };
        if let Page::Open(remote) = client.page {
            let _ = self.link.call_on(&remote, CLOSE_FUNCTION, &[]);
            self.link.release(remote);
        }
    }

    /// Ends every connection: the relay stops.
    fn end(&mut self) {
        let ids = self.clients.keys().copied().collect::<Vec<u64>>();
        for id in ids {
            self.close(id);
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
        while !self.unwritten.is_empty() {
            match (&self.stream).write(&self.unwritten) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => {
                    self.unwritten.drain(..written);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }
}

/// The session's endpoint, private to the current user: a socket in the session's `0700`
/// directory, or a pipe whose DACL grants the current user alone.
#[cfg(unix)]
fn bind(endpoint: &str) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .create_sync()
}

#[cfg(windows)]
fn bind(endpoint: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;{})", current_user_sid()?))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .security_descriptor(descriptor)
        .create_sync()
}

/// The current user's SID, as `whoami` gives it: the user is named by SID rather than as the
/// pipe's owner, which is the Administrators group for an elevated process.
#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    let output = crate::proc::output_with_timeout(
        std::process::Command::new("whoami").arg("/user"),
        Duration::from_secs(10),
    )?;
    sid_in_whoami(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("`whoami /user` named no SID"))
}

/// The SID in `whoami /user`'s table, whose last row is `machine\user  S-1-5-21-…`. The headings
/// are in the system's language and a user's name may hold spaces, so the SID is the last word
/// that starts as one does.
#[cfg(any(windows, test))]
pub(super) fn sid_in_whoami(table: &str) -> Option<&str> {
    table
        .split_whitespace()
        .rev()
        .find(|word| word.starts_with("S-1-"))
}
