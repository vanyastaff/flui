//! Binding the endpoint, the acceptor thread, and stopping them.
//!
//! The acceptor blocks in `accept`; stopping wakes it by connecting to the endpoint itself.
//! Every connection's thread drives its own [`Session`](flui_runtime::agent::Session), and
//! stopping closes every connection's stream, which ends its blocking reads and writes.

use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use flui_protocol::AppInfo;
use flui_runtime::agent::{Refused, Service, ServiceConfig};
use flui_view::dev_agent::AgentWindow;
use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{
    Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream,
};
use parking_lot::Mutex;
use web_time::Instant;

use super::super::Disconnect;
use super::connection::{self, Closer};
#[cfg(unix)]
use super::launch::abstract_name;
use super::launch::{Launch, PeerRule};

/// How long stopping waits for the agent's threads.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// How long stopping waits for the acceptor before it connects to the endpoint again: a
/// connection made while the acceptor was between two accepts may not reach it.
const STOP_RETRY: Duration = Duration::from_millis(50);

/// How long the acceptor waits after a failed accept before the next: a failure that persists
/// (out of descriptors) must not spin. Stopping wakes it.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(50);

/// Held by every thread of the agent: stopping waits until each has dropped its clone.
pub(super) type Done = Sender<()>;

/// What the agent's threads share beside the service.
pub(super) struct Transport {
    stop: AtomicBool,
    /// The live connections, as handles that close them.
    open: Mutex<BTreeMap<u64, Closer>>,
    next: AtomicU64,
}

impl Transport {
    /// Register a connection's `closer` until the returned guard drops; `None` once the agent
    /// is stopping, and the connection is then closed here.
    pub(super) fn open(self: &Arc<Self>, closer: Closer) -> Option<Open> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let mut open = self.open.lock();
        // Read under the lock that stopping closes the connections under, so a connection is
        // either closed by the stop or refused here.
        if self.stop.load(Ordering::SeqCst) {
            drop(open);
            connection::close(&closer);
            return None;
        }
        open.insert(id, closer);
        Some(Open {
            transport: Arc::clone(self),
            id,
        })
    }

    /// Whether the agent is stopping.
    #[cfg_attr(
        unix,
        expect(dead_code, reason = "a Unix connection's writes end with its socket")
    )]
    pub(super) fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Close every live connection.
    fn close_all(&self) {
        let open = std::mem::take(&mut *self.open.lock());
        for closer in open.values() {
            connection::close(closer);
        }
    }
}

/// A registered connection; dropping it closes and forgets the connection.
pub(super) struct Open {
    transport: Arc<Transport>,
    id: u64,
}

impl Drop for Open {
    fn drop(&mut self) {
        let closer = self.transport.open.lock().remove(&self.id);
        // Closed once the lock is let go: the conversation has ended.
        if let Some(closer) = closer {
            connection::close(&closer);
        }
    }
}

/// A bound endpoint, its acceptor thread, and the service its connections hold conversations
/// with.
pub(super) struct Running {
    service: Service,
    transport: Arc<Transport>,
    address: String,
    acceptor: Option<JoinHandle<()>>,
    /// Disconnected once the acceptor has returned, its listener dropped.
    acceptor_done: Receiver<()>,
    /// Disconnected once every thread of the agent has returned.
    threads_done: Receiver<()>,
}

impl Running {
    /// Serve `window` to the drivers that connect.
    pub(super) fn window_opened(&self, window: AgentWindow) {
        self.service.window_opened(window);
    }

    /// Stop accepting, close every connection, and wait (boundedly) for the threads; the
    /// endpoint is released before this returns unless its thread is stuck past the bound.
    pub(super) fn stop(mut self) {
        self.stop_and_wait();
    }

    fn stop_and_wait(&mut self) {
        let Some(acceptor) = self.acceptor.take() else {
            return;
        };
        self.transport.stop.store(true, Ordering::SeqCst);
        self.service.stop();
        self.transport.close_all();
        let deadline = Instant::now() + STOP_WAIT;
        // Held until the acceptor returns: a connection that closed before the acceptor saw it
        // could be cleared by the listener without waking it.
        let mut wake = None;
        loop {
            if wake.is_none() {
                wake = connect(&self.address).ok();
            }
            acceptor.thread().unpark();
            let left = deadline.saturating_duration_since(Instant::now());
            match self.acceptor_done.recv_timeout(left.min(STOP_RETRY)) {
                Err(RecvTimeoutError::Disconnected) => break,
                Ok(()) | Err(RecvTimeoutError::Timeout) if left > STOP_RETRY => {}
                Ok(()) | Err(RecvTimeoutError::Timeout) => {
                    tracing::warn!("the driver agent's acceptor did not stop in time");
                    return;
                }
            }
        }
        drop(wake);
        let _ = acceptor.join();
        let left = deadline.saturating_duration_since(Instant::now());
        if let Ok(()) | Err(RecvTimeoutError::Timeout) = self.threads_done.recv_timeout(left) {
            tracing::warn!("the driver agent's connections did not close in time");
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop_and_wait();
    }
}

/// Bind `launch`'s endpoint and start accepting.
pub(super) fn listen(
    launch: &Launch,
    app: AppInfo,
    disconnect: Option<Arc<Disconnect>>,
) -> io::Result<Running> {
    let listener = bind(&launch.address)?;
    let on_driver_gone = disconnect
        .map(|disconnect| -> Box<dyn Fn() + Send + Sync> { Box::new(move || disconnect.fire()) });
    let service = Service::new(ServiceConfig {
        records_native: false,
        token: launch.token.clone(),
        app,
        limits: launch.limits,
        on_driver_gone,
    });
    let transport = Arc::new(Transport {
        stop: AtomicBool::new(false),
        open: Mutex::new(BTreeMap::new()),
        next: AtomicU64::new(0),
    });
    let (acceptor_tx, acceptor_done) = mpsc::channel::<()>();
    let (threads_tx, threads_done) = mpsc::channel::<()>();
    let accepting = service.clone();
    let accepting_transport = Arc::clone(&transport);
    let peers = launch.peers;
    let acceptor = std::thread::Builder::new()
        .name("flui-agent-accept".into())
        .spawn(move || {
            let _done = (acceptor_tx, threads_tx.clone());
            accept_loop(
                &listener,
                peers,
                &accepting,
                &accepting_transport,
                &threads_tx,
            );
        })?;
    Ok(Running {
        service,
        transport,
        address: launch.address.clone(),
        acceptor: Some(acceptor),
        acceptor_done,
        threads_done,
    })
}

fn accept_loop(
    listener: &Listener,
    peers: Option<PeerRule>,
    service: &Service,
    transport: &Arc<Transport>,
    done: &Done,
) {
    loop {
        let accepted = listener.accept();
        if transport.stop.load(Ordering::SeqCst) {
            // The listener goes with this thread's return, removing a Unix socket file.
            return;
        }
        match accepted {
            Ok(stream) => admit(stream, peers, service, transport, done),
            Err(error) => {
                super::trace(
                    || tracing::debug!(kind = ?error.kind(), "agent endpoint accept failed"),
                );
                std::thread::park_timeout(ACCEPT_BACKOFF);
            }
        }
    }
}

fn admit(
    stream: Stream,
    peers: Option<PeerRule>,
    service: &Service,
    transport: &Arc<Transport>,
    done: &Done,
) {
    if let Some(rule) = peers
        && !peer_admitted(&stream, rule)
    {
        super::trace(|| {
            tracing::debug!("agent endpoint: a peer the socket does not admit; closing it unread");
        });
        return;
    }
    let session = match service.accept(Instant::now()) {
        Ok(session) => session,
        Err(refused) => {
            if refused == Refused::Full {
                super::trace(|| {
                    tracing::debug!("agent endpoint full; closing a connection unread");
                });
            }
            return;
        }
    };
    let serving = Arc::clone(transport);
    let thread_done = done.clone();
    // A spawn that fails drops the session and the stream with the closure.
    let _ = std::thread::Builder::new()
        .name("flui-agent-session".into())
        .spawn(move || {
            connection::serve(stream, session, &serving, &thread_done);
        });
}

/// Whether `rule` admits `stream`'s peer, by its effective user ID; a peer whose credentials
/// cannot be read is not admitted.
fn peer_admitted(stream: &Stream, rule: PeerRule) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use interprocess::local_socket::traits::StreamCommon as _;
        stream
            .peer_creds()
            .ok()
            .and_then(|credentials| credentials.euid())
            .is_some_and(rule)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        // Only Linux and Android bind abstract sockets, the endpoints a rule guards.
        let _ = (stream, rule);
        false
    }
}

/// The endpoint's name for `address`.
fn name(address: &str) -> io::Result<Name<'_>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
        address
            .strip_prefix(r"\\.\pipe\")
            .unwrap_or(address)
            .to_ns_name::<GenericNamespaced>()
    }
    #[cfg(unix)]
    {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Some(name) = abstract_name(address) {
            use interprocess::local_socket::ToNsName as _;
            use interprocess::os::unix::local_socket::AbstractNsUdSocket;
            return name.to_ns_name::<AbstractNsUdSocket>();
        }
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()
    }
}

/// A connection to the endpoint at `address`, which wakes its acceptor. Bounded: the stop that
/// makes it must not wait long on an endpoint with no instance free.
fn connect(address: &str) -> io::Result<Stream> {
    interprocess::local_socket::ConnectOptions::new()
        .name(name(address)?)
        .wait_mode(interprocess::ConnectWaitMode::Timeout(STOP_RETRY))
        .connect_sync()
}

#[cfg(windows)]
fn bind(address: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    // A protected DACL granting the current user all access and nobody else
    // any; `current_user_sid` says why the user is named by SID. Remote
    // clients are refused by default.
    let sddl = widestring::U16CString::from_str(format!(
        "D:P(A;;GA;;;{})",
        flui_native::current_user_sid()?
    ))
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name(address)?)
        .nonblocking(ListenerNonblockingMode::Neither)
        .security_descriptor(descriptor)
        .create_sync()
}

#[cfg(unix)]
fn bind(address: &str) -> io::Result<Listener> {
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;

    // An abstract socket has no file and no directory to check: the peer rule keeps it the
    // current user's.
    if abstract_name(address).is_some() {
        return ListenerOptions::new()
            .name(name(address)?)
            .nonblocking(ListenerNonblockingMode::Neither)
            .create_sync();
    }
    let path = Path::new(address);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let listener = ListenerOptions::new()
        .name(name(address)?)
        .nonblocking(ListenerNonblockingMode::Neither)
        .create_sync()?;
    // Checked after the bind, before anything is accepted: the socket file
    // is ours, so its owner is the current user. A directory anyone else can
    // enter or write lets them reach, or replace, the socket.
    let socket_owner = std::fs::metadata(path)?.uid();
    let directory = std::fs::metadata(parent)?;
    if directory.uid() != socket_owner || directory.mode() & 0o077 != 0 {
        drop(listener);
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the socket's directory must be the current user's, with mode 0700",
        ));
    }
    Ok(listener)
}
