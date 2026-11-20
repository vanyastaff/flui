//! Test support for the driver and for front ends built on it (ADR-0243), behind the `testing`
//! feature:
//!
//! - [`ScriptedLane`]: an OS lane that sends nothing anywhere and records what it was asked to
//!   send, with a [`ScriptedGuard`] that reports foreign input after a set number of events, so
//!   the user-activity rule runs without a desktop.
//! - [`ScriptedAgent`]: a stand-in for an app's agent on a real local socket, answering each
//!   request by a script, so the socket transport's cases (an app that closes mid-call or answers
//!   slowly, a driver that leaves) run without a FLUI app.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_protocol::envelope::{Greeting, Request, ServerLine};
use flui_protocol::error::ErrorObject;
use flui_protocol::framing::LineReader;
use flui_protocol::input::PointerDevice;
use flui_protocol::session::{AppDescription, Capabilities, HelloReply, Isolation, Placement};
use flui_protocol::version::PROTOCOL_VERSION;
use serde_json::Value;

use crate::coordinates::WindowGeometry;
use crate::error::{Error, Result};
use crate::lane::{ActivityGuard, ForeignInput, Lane, LaneCapabilities, PositionFidelity};
use crate::steps::OsEvent;

/// What a [`ScriptedLane`] was asked to do, read from the test's side.
#[derive(Debug, Clone, Default)]
pub struct LaneLog {
    sent: Arc<Mutex<Vec<OsEvent>>>,
    releases: Arc<AtomicUsize>,
    armed: Arc<AtomicUsize>,
}

impl LaneLog {
    /// The events the lane sent, in order.
    #[must_use]
    pub fn sent(&self) -> Vec<OsEvent> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many times the lane released everything it held.
    #[must_use]
    pub fn releases(&self) -> usize {
        self.releases.load(Ordering::SeqCst)
    }

    /// How many times its guard was armed.
    #[must_use]
    pub fn arms(&self) -> usize {
        self.armed.load(Ordering::SeqCst)
    }
}

/// A user-activity guard that reports foreign input once its lane has sent a set number of
/// events.
#[derive(Debug)]
pub struct ScriptedGuard {
    after: Option<(usize, String)>,
    sent: Arc<Mutex<Vec<OsEvent>>>,
    armed: Arc<AtomicUsize>,
    watching: bool,
    reported: bool,
}

impl ActivityGuard for ScriptedGuard {
    fn arm(&mut self) -> Result<()> {
        self.armed.fetch_add(1, Ordering::SeqCst);
        self.watching = true;
        Ok(())
    }

    fn foreign_input(&mut self) -> Option<ForeignInput> {
        let (after, description) = self.after.as_ref()?;
        let sent = self
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        if !self.watching || sent < *after || self.reported {
            return None;
        }
        self.reported = true;
        Some(ForeignInput::new(description.clone()))
    }

    fn disarm(&mut self) {
        self.watching = false;
    }
}

/// An OS lane that sends nothing anywhere: see the [module docs](self).
#[derive(Debug)]
pub struct ScriptedLane {
    isolation: Isolation,
    guard: Option<ScriptedGuard>,
    log: LaneLog,
    /// The capability its check names when it refuses every sequence.
    refuses: Option<String>,
}

impl ScriptedLane {
    /// A lane whose input lands in `isolation`, with no guard, and the log of what it is asked.
    #[must_use]
    pub fn new(isolation: Isolation) -> (Self, LaneLog) {
        let log = LaneLog::default();
        (
            Self {
                isolation,
                guard: None,
                log: log.clone(),
                refuses: None,
            },
            log,
        )
    }

    /// Whose check refuses every sequence as not supported, naming `capability`, as a lane
    /// refuses a key it has no code for.
    #[must_use]
    pub fn refusing(mut self, capability: impl Into<String>) -> Self {
        self.refuses = Some(capability.into());
        self
    }

    /// With a guard that reports no foreign input.
    #[must_use]
    pub fn guarded(mut self) -> Self {
        self.guard = Some(self.make_guard(None));
        self
    }

    /// With a guard that reports `description` once the lane has sent `events` events.
    #[must_use]
    pub fn foreign_input_after(mut self, events: usize, description: impl Into<String>) -> Self {
        self.guard = Some(self.make_guard(Some((events, description.into()))));
        self
    }

    fn make_guard(&self, after: Option<(usize, String)>) -> ScriptedGuard {
        ScriptedGuard {
            after,
            sent: Arc::clone(&self.log.sent),
            armed: Arc::clone(&self.log.armed),
            watching: false,
            reported: false,
        }
    }
}

impl Lane for ScriptedLane {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn capabilities(&self) -> LaneCapabilities {
        LaneCapabilities::new(
            vec![PointerDevice::Mouse, PointerDevice::Touch],
            true,
            self.isolation,
            PositionFidelity::Exact,
        )
    }

    fn guard(&mut self) -> Option<&mut dyn ActivityGuard> {
        self.guard
            .as_mut()
            .map(|guard| guard as &mut dyn ActivityGuard)
    }

    /// The window at the screen's origin: the scripted screen is the window.
    fn locate_window(&mut self, placement: &Placement) -> Result<WindowGeometry> {
        Ok(WindowGeometry::new(
            flui_protocol::geometry::LogicalPoint::new(0.0, 0.0),
            placement.size,
            placement.scale,
        ))
    }

    fn check(&mut self, events: &[OsEvent]) -> Result<()> {
        match &self.refuses {
            Some(capability) => Err(Error::not_supported(
                capability.clone(),
                format!("the scripted lane refuses all {} events", events.len()),
            )),
            None => Ok(()),
        }
    }

    fn send(&mut self, event: &OsEvent) -> Result<()> {
        self.log
            .sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
        Ok(())
    }

    fn release_all(&mut self) -> Result<()> {
        self.log.releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// How a [`ScriptedAgent`] answers one request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Reply {
    /// With this result.
    Result(Value),
    /// With this error.
    Error(ErrorObject),
    /// With this result, then closing the connection.
    ResultThenClose(Value),
    /// By closing the connection, unanswered.
    Close,
}

/// The script of a [`ScriptedAgent`]: the answer to each request, by its method and parameters.
pub type Script = Box<dyn FnMut(&str, &Value) -> Reply + Send>;

/// A stand-in for an app's agent on a local socket: see the [module docs](self).
pub struct ScriptedAgent {
    endpoint: String,
    token: String,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl ScriptedAgent {
    /// Serve one connection at a fresh endpoint on a thread of its own, introducing itself as
    /// serving `methods`.
    ///
    /// # Errors
    ///
    /// When the endpoint cannot be bound.
    pub fn start(methods: &[&str], script: Script) -> Result<Self> {
        let place = crate::launch::EndpointPlace::new()?;
        let token = crate::launch::launch_token()?;
        let listener = listen(&place.address)?;
        let endpoint = place.address.clone();
        let served_token = token.clone();
        let methods: Vec<String> = methods.iter().map(|method| (*method).to_owned()).collect();
        let thread = thread::Builder::new()
            .name("flui-driver-scripted-agent".into())
            .spawn(move || {
                let served = serve_one(&listener, &served_token, &methods, script);
                // The socket's directory (Unix) is removed once the connection was served.
                drop(place);
                served
            })?;
        Ok(Self {
            endpoint,
            token,
            thread: Some(thread),
        })
    }

    /// The endpoint, as a driver attaches to it.
    #[must_use]
    pub fn endpoint(&self) -> crate::connection::socket::Endpoint {
        crate::connection::socket::Endpoint::Local(self.endpoint.clone())
    }

    /// The token it admits.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Wait at most `within` for the connection it served to end: the driver closed it, or the
    /// script did.
    ///
    /// # Errors
    ///
    /// [`Error::Protocol`] when the connection is still open after `within`; the serving thread's
    /// failure.
    ///
    /// # Panics
    ///
    /// Resumes the serving thread's panic.
    pub fn join(mut self, within: Duration) -> Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let deadline = Instant::now() + within;
        while !thread.is_finished() {
            if Instant::now() >= deadline {
                return Err(Error::Protocol {
                    message: format!(
                        "the stand-in still holds its connection after {} ms",
                        within.as_millis()
                    ),
                });
            }
            thread::sleep(Duration::from_millis(5));
        }
        match thread.join() {
            Ok(served) => Ok(served?),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl std::fmt::Debug for ScriptedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedAgent")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

fn listen(address: &str) -> io::Result<interprocess::local_socket::Listener> {
    use interprocess::local_socket::ListenerOptions;
    ListenerOptions::new()
        .name(crate::connection::socket::local_name(address)?)
        .create_sync()
}

/// Accept one connection and serve it until it closes or the script closes it.
fn serve_one(
    listener: &interprocess::local_socket::Listener,
    token: &str,
    methods: &[String],
    mut script: Script,
) -> io::Result<()> {
    use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
    let stream = listener
        .incoming()
        .next()
        .ok_or_else(|| io::Error::other("the listener ended"))??;
    let (mut reading, mut writing) = stream.split();
    let mut lines = LineReader::new(crate::connection::MAX_REPLY);
    let mut chunk = vec![0_u8; 8192];
    let mut greeted = false;
    loop {
        let line = loop {
            match lines.next_line() {
                Ok(Some(line)) => break line,
                Ok(None) => {}
                Err(_) => return Ok(()),
            }
            let read = match reading.read(&mut chunk) {
                Ok(read) => read,
                // The driver went: a pipe reports it as broken rather than ended.
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    0
                }
                Err(error) => return Err(error),
            };
            if read == 0 {
                return Ok(());
            }
            lines.push(&chunk[..read]);
        };
        let mut close_after = false;
        let answer = if greeted {
            let Ok(request) = serde_json::from_slice::<Request<Value>>(&line) else {
                return Ok(());
            };
            match script(&request.method, &request.params) {
                Reply::Result(result) => ServerLine::answer(request.id, Ok(result)),
                Reply::Error(error) => ServerLine::answer(request.id, Err(error)),
                Reply::ResultThenClose(result) => {
                    close_after = true;
                    ServerLine::answer(request.id, Ok(result))
                }
                Reply::Close => return Ok(()),
            }
        } else {
            let presented = serde_json::from_slice::<Greeting>(&line)
                .is_ok_and(|greeting| greeting.hello.token == token);
            if !presented {
                return Ok(());
            }
            greeted = true;
            let mut capabilities = Capabilities::default();
            capabilities.methods = methods.to_vec();
            ServerLine::hello(
                HelloReply::new(PROTOCOL_VERSION)
                    .app(AppDescription::new("scripted", "scripted").with_pid(std::process::id()))
                    .capabilities(capabilities),
            )
        };
        let mut bytes = serde_json::to_vec(&answer).map_err(io::Error::other)?;
        bytes.push(b'\n');
        writing.write_all(&bytes)?;
        writing.flush()?;
        if close_after {
            return Ok(());
        }
    }
}
