//! The run endpoint: `flui run --agent` serving the methods the protocol's table marks
//! `ServedBy::Supervisor` (`run.reload`, ADR-0401) at an endpoint of its own, beside the app's.
//!
//! The app cannot rebuild itself, and a reload may replace its process, which ends every
//! connection to it; so a client asks the run, at the endpoint the session file names as `run`.
//! The conversation is the app's (ADR-0238 §2): a [`Greeting`] with the session's token, the
//! hello answered with no app named, then requests. A call that changes the app is refused to an
//! observer, as the app refuses it ([`ClientRole::may_call`]).
//!
//! One thread serves every client, polling, and hands each reload to the run's loop as a
//! [`ReloadRequest`] ([`RunEndpoint::next_request`]). A request is answered with the report the
//! loop writes for it, or `shutting_down` when the loop dropped it unanswered: the run ended, or
//! the step that took it failed. A client that goes while its reload runs leaves the reload to
//! run; only the answer is lost.

use std::collections::BTreeMap;
use std::io::{self, Read as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_protocol::envelope::{Greeting, Request, RequestId, ServerLine};
use flui_protocol::error::{ErrorCode, ErrorObject, Retry};
use flui_protocol::framing::{LineReader, MAX_LINE};
use flui_protocol::launch::same_token;
use flui_protocol::method::{Method as _, RunReload};
use flui_protocol::reload::{ReloadParams, ReloadReport, ReloadTier};
use flui_protocol::session::{Capabilities, ClientRole, HelloReply};
use flui_protocol::version::PROTOCOL_VERSION;
use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{Listener, Stream};
use serde_json::Value;

use super::listener::bind;
use crate::error::{CliResult, ResultExt as _};

/// The longest pause between two turns of the endpoint.
const TICK: Duration = Duration::from_millis(10);

/// The most clients the endpoint holds at once; one past them is closed as it connects. Each
/// has at most one reload waiting, so the loop is never handed more than this many.
const MAX_CLIENTS: usize = 8;

/// How long a client may take to send its hello before it is closed.
const GREETING_WAIT: Duration = Duration::from_secs(10);

/// How long the endpoint, once dropped, keeps writing the answers it owes.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// The most bytes kept for a client that does not read its answers.
const MAX_UNWRITTEN: usize = 1 << 20;

/// A reload a client asked for, which the run's loop applies and answers.
#[derive(Debug)]
pub(crate) struct ReloadRequest {
    at_least: Option<ReloadTier>,
    answer: Sender<ReloadReport>,
}

impl ReloadRequest {
    /// The cheapest tier the reload may apply; `None` when the run places it itself.
    pub(crate) const fn at_least(&self) -> Option<ReloadTier> {
        self.at_least
    }

    /// Answers the client with `report`. A request dropped without one is answered
    /// `shutting_down`.
    pub(crate) fn answer(self, report: &ReloadReport) {
        // A client that went while the reload ran left nobody to tell.
        let _ = self.answer.send(report.clone());
    }
}

/// The run endpoint's thread and the reloads its clients asked for. Dropping it answers every
/// request it still owes `shutting_down` and stops the thread.
pub(crate) struct RunEndpoint {
    requests: Option<Receiver<ReloadRequest>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    address: String,
    /// The private directory that keeps the endpoint; removed with it.
    _dir: tempfile::TempDir,
}

impl RunEndpoint {
    /// Serves a fresh private endpoint ([`super::private_endpoint`]), admitting the clients that
    /// present `token`.
    ///
    /// # Errors
    ///
    /// The endpoint could not be made or bound, or the thread not started.
    pub(crate) fn start(token: String) -> CliResult<Self> {
        let (dir, address) = super::private_endpoint("run")?;
        let listener = bind(&address).context("could not bind the run endpoint")?;
        let (handed, requests) = mpsc::sync_channel(MAX_CLIENTS);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("flui-run-endpoint".into())
            .spawn(move || {
                let mut turns = Turns {
                    token,
                    handed,
                    clients: BTreeMap::new(),
                    next_id: 0,
                };
                turns.run(&listener, &stopped);
            })
            .context("could not start the run endpoint's thread")?;
        Ok(Self {
            requests: Some(requests),
            stop,
            thread: Some(thread),
            address,
            _dir: dir,
        })
    }

    /// Where the endpoint is served.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// The next reload a client asked for, without waiting.
    pub(crate) fn next_request(&self) -> Option<ReloadRequest> {
        self.requests.as_ref()?.try_recv().ok()
    }
}

impl Drop for RunEndpoint {
    fn drop(&mut self) {
        // The requests not taken go first, so the thread's last turns answer them.
        drop(self.requests.take());
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The endpoint's state, owned by its thread.
struct Turns {
    token: String,
    handed: SyncSender<ReloadRequest>,
    clients: BTreeMap<u64, Client>,
    next_id: u64,
}

struct Client {
    stream: Stream,
    reader: LineReader,
    unwritten: Vec<u8>,
    phase: Phase,
    /// The reload the client waits for, and the id that answers it.
    waiting: Option<(RequestId, Receiver<ReloadReport>)>,
    /// The client is closed once what it was sent is written.
    ending: bool,
}

enum Phase {
    Greeting { since: Instant },
    Open { role: ClientRole },
}

impl Turns {
    fn run(&mut self, listener: &Listener, stop: &AtomicBool) {
        while !stop.load(Ordering::SeqCst) {
            self.accept(listener);
            self.turn_all();
            thread::sleep(TICK);
        }
        // What is owed is answered: every request the loop did not take was dropped with the
        // endpoint's receiver, so each waiting client now reads `shutting_down`.
        let until = Instant::now() + FLUSH_WAIT;
        while !self.clients.is_empty() && Instant::now() < until {
            self.turn_all();
            self.clients
                .retain(|_, client| client.waiting.is_some() || !client.unwritten.is_empty());
            thread::sleep(TICK);
        }
    }

    fn accept(&mut self, listener: &Listener) {
        loop {
            let stream = match listener.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    crate::ui::debug(format!("the run endpoint accepted no client: {error}"));
                    return;
                }
            };
            if self.clients.len() >= MAX_CLIENTS {
                // Dropped: the client reads the end of its connection.
                continue;
            }
            let Some(next) = self.next_id.checked_add(1) else {
                // Refused: an id is never used twice.
                continue;
            };
            let id = self.next_id;
            self.next_id = next;
            self.clients.insert(
                id,
                Client {
                    stream,
                    reader: LineReader::new(MAX_LINE),
                    unwritten: Vec::new(),
                    phase: Phase::Greeting {
                        since: Instant::now(),
                    },
                    waiting: None,
                    ending: false,
                },
            );
        }
    }

    fn turn_all(&mut self) {
        let ids = self.clients.keys().copied().collect::<Vec<u64>>();
        for id in ids {
            let keep = self
                .clients
                .get_mut(&id)
                .is_some_and(|client| client.turn(&self.token, &self.handed));
            if !keep {
                self.clients.remove(&id);
            }
        }
    }
}

impl Client {
    /// Reads, answers and writes; `false` once the client is to be dropped.
    fn turn(&mut self, token: &str, handed: &SyncSender<ReloadRequest>) -> bool {
        if !self.ending {
            if !self.read(token, handed) {
                return false;
            }
            if let Phase::Greeting { since } = self.phase
                && since.elapsed() >= GREETING_WAIT
            {
                return false;
            }
        }
        if let Some((id, answer)) = &self.waiting {
            let outcome = match answer.try_recv() {
                Ok(report) => Some(Ok(report)),
                Err(TryRecvError::Disconnected) => Some(Err(ErrorObject::new(
                    ErrorCode::ShuttingDown,
                    "the run ended before the reload did",
                ))),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(outcome) = outcome {
                let id = id.clone();
                self.waiting = None;
                self.send(&ServerLine::answer(id, outcome));
            }
        }
        match self.write() {
            Ok(true) => !self.ending,
            Ok(false) => true,
            Err(_) => false,
        }
    }

    /// Takes the lines the client sent; `false` once it went or broke the protocol.
    fn read(&mut self, token: &str, handed: &SyncSender<ReloadRequest>) -> bool {
        let mut chunk = [0_u8; 4 << 10];
        loop {
            match (&self.stream).read(&mut chunk) {
                // A reload already handed on still runs; only its answer has nobody to read it.
                Ok(0) => return false,
                Ok(read) => self.reader.push(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
            loop {
                match self.reader.next_line() {
                    Ok(Some(line)) => {
                        self.line(&line, token, handed);
                        if self.ending {
                            return true;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => return false,
                }
            }
        }
    }

    fn line(&mut self, line: &[u8], token: &str, handed: &SyncSender<ReloadRequest>) {
        match self.phase {
            Phase::Greeting { .. } => self.greet(line, token),
            Phase::Open { role } => self.request(line, role, handed),
        }
    }

    fn greet(&mut self, line: &[u8], token: &str) {
        let Ok(Greeting { hello, .. }) = serde_json::from_slice::<Greeting>(line) else {
            self.ending = true;
            return;
        };
        if !same_token(hello.token.as_bytes(), token.as_bytes()) {
            self.ending = true;
            return;
        }
        if !PROTOCOL_VERSION.serves(hello.protocol) {
            self.send(&ServerLine::refusal(ErrorObject::version(
                PROTOCOL_VERSION,
                hello.protocol,
            )));
            self.ending = true;
            return;
        }
        let mut capabilities = Capabilities::default();
        if hello.role.may_call(RunReload::EFFECT) {
            capabilities.methods = vec![RunReload::NAME.to_owned()];
        }
        self.send(&ServerLine::hello(
            HelloReply::new(PROTOCOL_VERSION).capabilities(capabilities),
        ));
        self.phase = Phase::Open { role: hello.role };
    }

    fn request(&mut self, line: &[u8], role: ClientRole, handed: &SyncSender<ReloadRequest>) {
        let Ok(request) = serde_json::from_slice::<Request<Value>>(line) else {
            self.send(&ServerLine::refusal(ErrorObject::new(
                ErrorCode::InvalidArgument,
                "a request is a JSON object with an id and a method",
            )));
            return;
        };
        let outcome = self.admit(&request, role, handed);
        if let Err(error) = outcome {
            self.send(&ServerLine::answer(request.id, Err(error)));
        }
    }

    /// Hands `request` to the loop, or says why not.
    fn admit(
        &mut self,
        request: &Request<Value>,
        role: ClientRole,
        handed: &SyncSender<ReloadRequest>,
    ) -> Result<(), ErrorObject> {
        if request.method != RunReload::NAME {
            return Err(ErrorObject::new(
                ErrorCode::NotSupported,
                format!(
                    "the run endpoint serves `{}` alone; the app's endpoint serves `{}`",
                    RunReload::NAME,
                    request.method
                ),
            ));
        }
        if !role.may_call(RunReload::EFFECT) {
            return Err(ErrorObject::new(
                ErrorCode::NotDriver,
                "a reload replaces the app; connect as a driver",
            ));
        }
        if self.waiting.is_some() {
            return Err(ErrorObject::new(
                ErrorCode::Busy,
                "this connection waits for a reload already; one runs at a time",
            )
            .retry(Retry::Soon));
        }
        let params = if request.params.is_null() {
            ReloadParams::default()
        } else {
            serde_json::from_value::<ReloadParams>(request.params.clone()).map_err(|error| {
                ErrorObject::new(
                    ErrorCode::InvalidArgument,
                    format!("{}: {error}", RunReload::NAME),
                )
            })?
        };
        let (answer, answered) = mpsc::channel::<ReloadReport>();
        let handing = ReloadRequest {
            at_least: params.at_least,
            answer,
        };
        match handed.try_send(handing) {
            Ok(()) => {
                self.waiting = Some((request.id.clone(), answered));
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(ErrorObject::new(
                ErrorCode::Busy,
                "the run has as many reloads waiting as it takes",
            )
            .retry(Retry::Soon)),
            Err(TrySendError::Disconnected(_)) => Err(ErrorObject::new(
                ErrorCode::ShuttingDown,
                "the run is ending",
            )),
        }
    }

    /// Queues `line` for the client; one that does not read is closed.
    fn send(&mut self, line: &ServerLine<ReloadReport>) {
        let Ok(mut bytes) = serde_json::to_vec(line) else {
            self.ending = true;
            return;
        };
        bytes.push(b'\n');
        if self.unwritten.len() + bytes.len() > MAX_UNWRITTEN {
            self.unwritten.clear();
            self.ending = true;
            return;
        }
        self.unwritten.extend_from_slice(&bytes);
    }

    /// Writes what it can; `true` once all of it is written.
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

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::time::Duration;

    use flui_protocol::build::Diagnostics;
    use flui_protocol::reload::{ReloadReport, ReloadTier, Reloaded};
    use flui_protocol::session::ClientRole;
    use interprocess::local_socket::Stream;
    use interprocess::local_socket::traits::Stream as _;
    use serde_json::{Value, json};

    use super::RunEndpoint;
    use crate::agent::client::{Connection, Failure, name};
    use crate::agent::{Credentials, Endpoint};

    const TOKEN: &str = "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a";
    const WAIT: Duration = Duration::from_secs(10);

    fn serving() -> (RunEndpoint, Credentials) {
        let endpoint = RunEndpoint::start(TOKEN.to_owned()).expect("the endpoint starts");
        let credentials = Credentials {
            endpoint: Endpoint::Local(endpoint.address().to_owned()),
            token: TOKEN.to_owned(),
            pid: 0,
            run: None,
        };
        (endpoint, credentials)
    }

    /// The request the loop takes next, waiting for it.
    fn taken(endpoint: &RunEndpoint) -> super::ReloadRequest {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            if let Some(request) = endpoint.next_request() {
                return request;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no request reached the loop"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn code(outcome: &Result<Value, Failure>) -> Option<&str> {
        match outcome {
            Err(Failure::Wire(error)) => error["code"].as_str(),
            _ => None,
        }
    }

    /// A reload reaches the loop with the tier it asked for, and its client reads the report
    /// the loop answers with.
    #[test]
    fn a_reload_reaches_the_loop_and_its_client_reads_the_report() {
        let (endpoint, credentials) = serving();
        let client = std::thread::spawn(move || {
            let mut connection = Connection::connect(&credentials, ClientRole::Driver, WAIT)
                .expect("the hello is answered");
            connection.request("run.reload", json!({ "at_least": "restart" }), WAIT)
        });
        let request = taken(&endpoint);
        assert_eq!(request.at_least(), Some(ReloadTier::Restart));
        let report = ReloadReport::new(Reloaded::applied(ReloadTier::ProcessRestart), 12);
        request.answer(&report);
        let answered = client.join().expect("the client").expect("answered");
        assert_eq!(answered, serde_json::to_value(&report).unwrap());
    }

    /// Every way a reload ends without its report answers `shutting_down`: the loop dropped the
    /// request it took, or the endpoint went with the request waiting.
    #[test]
    fn a_reload_the_run_drops_is_answered_shutting_down() {
        let (endpoint, credentials) = serving();
        let asked = credentials.clone();
        let client = std::thread::spawn(move || {
            let mut connection =
                Connection::connect(&asked, ClientRole::Driver, WAIT).expect("hello");
            connection.request("run.reload", json!({}), WAIT)
        });
        drop(taken(&endpoint));
        let outcome = client.join().expect("the client");
        assert_eq!(code(&outcome), Some("shutting_down"), "{outcome:?}");

        let client = std::thread::spawn(move || {
            let mut connection =
                Connection::connect(&credentials, ClientRole::Driver, WAIT).expect("hello");
            connection.request("run.reload", json!({}), WAIT)
        });
        // The run ends with the request untaken in the channel, or before it got there: either
        // way the client reads `shutting_down`.
        std::thread::sleep(Duration::from_millis(100));
        drop(endpoint);
        let outcome = client.join().expect("the client");
        assert_eq!(code(&outcome), Some("shutting_down"), "{outcome:?}");
    }

    /// What the endpoint refuses, and how: an observer's reload (`not_driver`), a method it does
    /// not serve (`not_supported`), parameters that do not read (`invalid_argument`), a hello
    /// with another token (closed unanswered). None reaches the loop.
    #[test]
    fn the_endpoint_refuses_what_it_may_not_run_before_the_loop_sees_it() {
        let (endpoint, credentials) = serving();
        let mut observer =
            Connection::connect(&credentials, ClientRole::Observer, WAIT).expect("hello");
        let outcome = observer.request("run.reload", json!({}), WAIT);
        assert_eq!(code(&outcome), Some("not_driver"), "{outcome:?}");

        let mut driver =
            Connection::connect(&credentials, ClientRole::Driver, WAIT).expect("hello");
        let outcome = driver.request("windows.list", json!({}), WAIT);
        assert_eq!(code(&outcome), Some("not_supported"), "{outcome:?}");
        let outcome = driver.request("run.reload", json!({ "at_least": "hot" }), WAIT);
        assert_eq!(code(&outcome), Some("invalid_argument"), "{outcome:?}");

        let mut stranger = credentials;
        stranger.token = "a5".repeat(32);
        assert!(Connection::connect(&stranger, ClientRole::Driver, Duration::ZERO).is_err());
        assert!(
            endpoint.next_request().is_none(),
            "nothing reached the loop"
        );
    }

    /// A connection waits for one reload at a time: a second request while the first runs is
    /// refused `busy`, and the first is still answered.
    #[test]
    fn a_second_reload_on_one_connection_is_busy_until_the_first_is_answered() {
        let (endpoint, _) = serving();
        let stream = Stream::connect(name(endpoint.address()).unwrap()).expect("connect");
        let mut writer = &stream;
        let mut lines = BufReader::new(&stream).lines();
        let protocol = flui_protocol::version::PROTOCOL_VERSION;
        writeln!(
            writer,
            r#"{{"hello":{{"token":"{TOKEN}","protocol":"{protocol}"}}}}"#
        )
        .unwrap();
        let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(
            hello["hello"]["capabilities"]["methods"],
            json!(["run.reload"])
        );
        writeln!(writer, r#"{{"id":1,"method":"run.reload"}}"#).unwrap();
        let first = taken(&endpoint);
        writeln!(writer, r#"{{"id":2,"method":"run.reload"}}"#).unwrap();
        let busy: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(busy["id"], 2);
        assert_eq!(busy["error"]["code"], "busy", "{busy}");
        assert_eq!(busy["error"]["retry"], "soon", "{busy}");
        assert!(
            endpoint.next_request().is_none(),
            "the second never reached the loop"
        );

        first.answer(&ReloadReport::new(Reloaded::Failed(Diagnostics::new()), 3));
        let answered: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(answered["id"], 1);
        assert!(
            answered["result"]["outcome"]["failed"].is_object(),
            "{answered}"
        );
    }
}
