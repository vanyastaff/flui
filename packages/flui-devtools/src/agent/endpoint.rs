//! Binding the endpoint, the acceptor thread, and stopping them.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{Listener, ListenerNonblockingMode, ListenerOptions, Name};

use super::AgentEndpoint;
use super::registry::Registry;
use super::session;

/// How often an idle thread looks at the stop flag.
pub(super) const POLL: Duration = Duration::from_millis(10);

/// How long stopping waits for the server's threads.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// What the server's threads share.
pub(super) struct Shared {
    pub(super) registry: Registry,
    pub(super) stop: AtomicBool,
    pub(super) token: String,
    pub(super) reply_timeout: Duration,
    pub(super) handshake_timeout: Duration,
    max_connections: usize,
    /// Connections being served.
    live: AtomicUsize,
}

/// A bound endpoint and its acceptor thread.
pub(super) struct Running {
    shared: Arc<Shared>,
    acceptor: Option<JoinHandle<()>>,
}

impl Running {
    pub(super) fn registry(&self) -> &Registry {
        &self.shared.registry
    }

    /// Stop accepting, close every connection, and wait (boundedly) for the
    /// threads; the endpoint is released before this returns unless its
    /// thread is stuck past the bound.
    pub(super) fn stop(mut self) {
        self.stop_and_wait();
    }

    fn stop_and_wait(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.registry.clear();
        if let Some(acceptor) = self.acceptor.take() {
            let deadline = Instant::now() + STOP_WAIT;
            while !acceptor.is_finished() && Instant::now() < deadline {
                std::thread::sleep(POLL);
            }
            if acceptor.is_finished() {
                let _ = acceptor.join();
            } else {
                tracing::warn!("the development agent's acceptor did not stop in time");
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop_and_wait();
    }
}

/// Bind `endpoint` and start accepting.
pub(super) fn listen(endpoint: &AgentEndpoint, registry: Registry) -> io::Result<Running> {
    let listener = bind(&endpoint.address)?;
    let shared = Arc::new(Shared {
        registry,
        stop: AtomicBool::new(false),
        token: endpoint.token.clone(),
        reply_timeout: endpoint.reply_timeout,
        handshake_timeout: endpoint.handshake_timeout,
        max_connections: endpoint.max_connections,
        live: AtomicUsize::new(0),
    });
    let accepting = Arc::clone(&shared);
    let acceptor = std::thread::Builder::new()
        .name("flui-agent-accept".into())
        .spawn(move || accept_loop(&listener, &accepting))?;
    Ok(Running {
        shared,
        acceptor: Some(acceptor),
    })
}

fn accept_loop(listener: &Listener, shared: &Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => admit(stream, shared),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
            Err(error) => {
                tracing::debug!(kind = ?error.kind(), "agent endpoint accept failed");
                std::thread::sleep(POLL);
            }
        }
    }
    // The listener goes with this thread's return (removing a Unix socket
    // file); connections see the stop flag and close themselves.
    let deadline = Instant::now() + STOP_WAIT;
    while shared.live.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
}

fn admit(stream: interprocess::local_socket::Stream, shared: &Arc<Shared>) {
    if shared.live.load(Ordering::SeqCst) >= shared.max_connections {
        tracing::debug!("agent endpoint full; closing a connection unread");
        return;
    }
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    shared.live.fetch_add(1, Ordering::SeqCst);
    let serving = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("flui-agent-session".into())
        .spawn(move || {
            struct Live<'a>(&'a AtomicUsize);
            impl Drop for Live<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _live = Live(&serving.live);
            session::serve(stream, &serving);
        });
    if spawned.is_err() {
        shared.live.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The endpoint's name for `address`.
pub(crate) fn name(address: &str) -> io::Result<Name<'_>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
        address
            .strip_prefix(r"\\.\pipe\")
            .unwrap_or(address)
            .to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()
    }
}

#[cfg(windows)]
fn bind(address: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    // A protected DACL granting the pipe's owner, the current user, all
    // access and nobody else any. Remote clients are refused by default.
    let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name(address)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .security_descriptor(descriptor)
        .create_sync()
}

#[cfg(not(windows))]
fn bind(address: &str) -> io::Result<Listener> {
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Path;

    let path = Path::new(address);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let listener = ListenerOptions::new()
        .name(name(address)?)
        .nonblocking(ListenerNonblockingMode::Both)
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
