//! The development agent endpoint end to end: a headless realm hosts the
//! counter, the server serves it over a real named pipe or Unix socket, and a
//! client thread speaks raw newline-delimited JSON while the test thread, the
//! realm's owner, pumps frames.

use std::io::{self, Read as _, Write as _};
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use flui_devtools::agent::{AgentEndpoint, AgentServer, MAX_LINE};
use flui_protocol::{Node, Tree};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{Signal, StatefulView, ViewState};
use flui_sdk::widgets::{Column, RawButton, Text, column};
use flui_testing::{HeadlessDevAgent, HeadlessRealm, HeadlessWindow};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-test";
/// How long a whole scenario may take before the test fails instead of hanging.
const SCENARIO_CAP: Duration = Duration::from_secs(60);
/// How long a client waits for one reply.
const RECV: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// The counter
// ---------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct Counter {
    presses: Arc<AtomicU32>,
}

struct CounterState {
    count: Signal<u32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: Signal::default(),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
    }

    fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        let presses = Arc::clone(&view.presses);
        Column::new(column![
            Text::new(format!("Count: {}", count.get(ctx))),
            RawButton::new(Text::new("Increment")).on_press(move |cx| {
                presses.fetch_add(1, Ordering::SeqCst);
                count.update(cx, |n| *n += 1)
            }),
        ])
    }
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// A fresh endpoint address; the directory (Unix) lives as long as the value.
struct Address {
    address: String,
    #[cfg(unix)]
    _dir: tempfile::TempDir,
}

impl Address {
    fn new() -> Self {
        #[cfg(unix)]
        {
            // `tempfile` creates the directory with mode 0700.
            let dir = tempfile::tempdir().expect("a temporary directory");
            let address = dir
                .path()
                .join("agent.sock")
                .to_str()
                .expect("a UTF-8 path")
                .to_owned();
            Self { address, _dir: dir }
        }
        #[cfg(windows)]
        {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos();
            Self {
                address: format!("flui-agent-test-{}-{nanos}", std::process::id()),
            }
        }
    }

    fn endpoint(&self) -> AgentEndpoint {
        AgentEndpoint::new(self.address.clone(), TOKEN)
    }
}

fn connect(address: &str) -> io::Result<Stream> {
    #[cfg(windows)]
    let name = {
        use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
        address.to_ns_name::<GenericNamespaced>()?
    };
    #[cfg(not(windows))]
    let name = {
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()?
    };
    Stream::connect(name)
}

// ---------------------------------------------------------------------------
// A raw NDJSON client
// ---------------------------------------------------------------------------

enum Recv {
    Line(Value),
    Closed,
    TimedOut,
}

struct Client {
    stream: Stream,
    buffer: Vec<u8>,
    next_id: u64,
}

impl Client {
    fn connect(address: &str) -> Self {
        let stream = connect(address).expect("the endpoint accepts a connection");
        stream.set_nonblocking(true).expect("non-blocking");
        Self {
            stream,
            buffer: Vec::new(),
            next_id: 1,
        }
    }

    /// Connected and past the hello.
    fn hello(address: &str) -> Self {
        let mut client = Self::connect(address);
        client.send(&json!({ "hello": { "token": TOKEN } }));
        match client.recv(RECV) {
            Recv::Line(welcome) => assert!(
                welcome["hello"]["protocol"].is_string(),
                "the server greets: {welcome}"
            ),
            Recv::Closed => panic!("the server closed the connection at the hello"),
            Recv::TimedOut => panic!("the server did not greet"),
        }
        client
    }

    fn send_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let deadline = Instant::now() + RECV;
        let mut written = 0;
        while written < bytes.len() {
            // A non-blocking pipe takes a write whole or not at all.
            let end = bytes.len().min(written + 512);
            match (&self.stream).write(&bytes[written..end]) {
                Ok(count) if count > 0 => written += count,
                Ok(_) => std::thread::sleep(Duration::from_micros(200)),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_micros(200));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
            if Instant::now() > deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
        Ok(())
    }

    fn send(&mut self, value: &Value) {
        let mut bytes = serde_json::to_vec(value).expect("serializes");
        bytes.push(b'\n');
        self.send_bytes(&bytes).expect("the request is written");
    }

    fn recv(&mut self, timeout: Duration) -> Recv {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0_u8; 8192];
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return Recv::Line(serde_json::from_slice(&line).expect("the reply is JSON"));
            }
            if Instant::now() > deadline {
                return Recv::TimedOut;
            }
            match read_some(&self.stream, &mut chunk) {
                Ok(0) => return Recv::Closed,
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Recv::Closed,
            }
        }
    }

    /// Sends `{"id":…,"op":op, ..fields}` and returns the reply.
    fn call(&mut self, op: &str, fields: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "id": id, "op": op });
        if let Value::Object(fields) = fields {
            for (key, value) in fields {
                request[key] = value;
            }
        }
        self.send(&request);
        match self.recv(RECV) {
            Recv::Line(reply) => {
                assert_eq!(reply["id"], json!(id), "replies come in order: {reply}");
                reply
            }
            Recv::Closed => panic!("the server closed the connection during `{op}`"),
            Recv::TimedOut => panic!("no reply to `{op}`"),
        }
    }

    /// Whether the server closed the connection without writing anything.
    fn closed_unread(&mut self) -> bool {
        matches!(self.recv(RECV), Recv::Closed) && self.buffer.is_empty()
    }

    fn windows(&mut self) -> Vec<String> {
        let reply = self.call("windows", json!({}));
        serde_json::from_value(reply["result"]["windows"].clone()).expect("a window list")
    }

    fn only_window(&mut self) -> String {
        let windows = self.windows();
        assert_eq!(windows.len(), 1, "one window is served: {windows:?}");
        windows[0].clone()
    }

    /// Reads `window` until `found` finds something in the tree, retrying
    /// while the window is busy.
    fn read_until<T>(&mut self, window: &str, found: impl Fn(&Tree) -> Option<T>) -> T {
        let deadline = Instant::now() + RECV;
        loop {
            let reply = self.call("read", json!({ "window": window }));
            if reply.get("error").is_some() {
                assert_eq!(
                    reply["error"]["retry"], "soon",
                    "only a retryable failure is retried: {reply}"
                );
            } else {
                let tree: Tree =
                    serde_json::from_value(reply["result"].clone()).expect("a wire tree");
                if let Some(value) = found(&tree) {
                    return value;
                }
            }
            assert!(Instant::now() < deadline, "the tree never showed it");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn button(&mut self, window: &str) -> String {
        self.read_until(window, |tree| {
            node(tree, "Increment").map(|n| n.id.to_string())
        })
    }

    fn wait_for_count(&mut self, window: &str, count: u32) {
        let label = format!("Count: {count}");
        self.read_until(window, |tree| node(tree, &label).map(|_| ()));
    }

    fn tap(&mut self, window: &str, element: &str) -> Value {
        self.call(
            "act",
            json!({ "window": window, "request": { "element": element, "action": "invoke" } }),
        )
    }
}

/// A non-blocking read that returns `Ok(0)` only once the server has closed:
/// on Windows an empty non-blocking pipe reads as end of file, so an empty
/// write (which fails only on a closed pipe) tells the two apart.
fn read_some(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    match (&*stream).read(buffer) {
        Ok(0) if cfg!(windows) => match (&*stream).write(&[]) {
            Ok(_) => Err(io::ErrorKind::WouldBlock.into()),
            Err(_) => Ok(0),
        },
        other => other,
    }
}

fn node(tree: &Tree, name: &str) -> Option<Node> {
    fn walk(nodes: &[Node], name: &str) -> Option<Node> {
        nodes.iter().find_map(|node| {
            (node.name.as_deref() == Some(name))
                .then(|| node.clone())
                .or_else(|| walk(&node.children, name))
        })
    }
    walk(&tree.roots, name)
}

// ---------------------------------------------------------------------------
// The owner: the test thread pumps while a client thread talks
// ---------------------------------------------------------------------------

/// What the client asks the owner to do; each is acknowledged once done.
enum Command {
    /// Stop pumping. Acknowledged once no pump is in flight.
    Pause,
    Resume,
    /// Drop the realm: its window closes.
    CloseWindow,
    /// Drop the agent: the hook detaches.
    Detach,
}

#[derive(Clone)]
struct OwnerHandle(mpsc::Sender<(Command, mpsc::Sender<()>)>);

impl OwnerHandle {
    fn ask(&self, command: Command) {
        let (done, acknowledged) = mpsc::channel();
        self.0.send((command, done)).expect("the owner is running");
        acknowledged.recv().expect("the owner acknowledges");
    }
}

struct Owner {
    agent: Option<HeadlessDevAgent>,
    realm: Option<HeadlessRealm>,
    presses: Arc<AtomicU32>,
}

impl Owner {
    fn new(server: AgentServer) -> Self {
        let agent = HeadlessDevAgent::attach(server);
        let presses = Arc::new(AtomicU32::new(0));
        let realm = HeadlessRealm::new(HeadlessWindow::new(400, 300)).with_dev_agent(&agent);
        realm
            .attach(&Counter {
                presses: Arc::clone(&presses),
            })
            .expect("the counter attaches");
        Self {
            agent: Some(agent),
            realm: Some(realm),
            presses,
        }
    }

    fn presses(&self) -> u32 {
        self.presses.load(Ordering::SeqCst)
    }

    /// Runs `client` on its own thread and pumps until it returns.
    fn drive<R: Send>(&mut self, client: impl FnOnce(OwnerHandle) -> R + Send) -> R {
        let (commands, inbox) = mpsc::channel();
        let handle = OwnerHandle(commands);
        std::thread::scope(|scope| {
            let running = scope.spawn(move || client(handle));
            let cap = Instant::now() + SCENARIO_CAP;
            let mut pumping = true;
            while !running.is_finished() {
                assert!(Instant::now() < cap, "the scenario did not finish");
                while let Ok((command, done)) = inbox.try_recv() {
                    match command {
                        Command::Pause => pumping = false,
                        Command::Resume => pumping = true,
                        Command::CloseWindow => drop(self.realm.take()),
                        Command::Detach => drop(self.agent.take()),
                    }
                    let _ = done.send(());
                }
                if pumping && let Some(realm) = self.realm.as_mut() {
                    let _ = realm.pump(Duration::from_millis(16));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            running
                .join()
                .unwrap_or_else(|payload| resume_unwind(payload))
        })
    }
}

// ---------------------------------------------------------------------------
// The scenario
// ---------------------------------------------------------------------------

#[test]
fn reads_the_counter_and_taps_it_over_the_endpoint() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);
        let reply = client.tap(&window, &button);
        assert_eq!(reply["result"], json!({}), "the tap is delivered: {reply}");
        client.wait_for_count(&window, 1);
    });
    assert_eq!(owner.presses(), 1, "the button's handler ran once");
}

// ---------------------------------------------------------------------------
// The failure table
// ---------------------------------------------------------------------------

fn a_connection_without_the_right_token_is_closed_unread() {
    let address = Address::new();
    let endpoint = address
        .endpoint()
        .with_handshake_timeout(Duration::from_millis(300));
    let mut owner = Owner::new(AgentServer::new(endpoint));
    let at = address.address.clone();
    owner.drive(move |_| {
        let mut no_hello = Client::connect(&at);
        no_hello.send(&json!({ "id": 1, "op": "windows" }));
        assert!(no_hello.closed_unread(), "a request before the hello");

        let mut wrong = Client::connect(&at);
        wrong.send(&json!({ "hello": { "token": "f".repeat(TOKEN.len()) } }));
        assert!(wrong.closed_unread(), "a wrong token");

        let mut prefix = Client::connect(&at);
        prefix.send(&json!({ "hello": { "token": &TOKEN[..TOKEN.len() - 1] } }));
        assert!(prefix.closed_unread(), "the token's prefix");

        let mut silent = Client::connect(&at);
        assert!(silent.closed_unread(), "no hello before the timeout");

        let mut client = Client::hello(&at);
        assert_eq!(client.windows().len(), 1, "a right client is served next");
    });
    assert_eq!(owner.presses(), 0);
}

fn a_client_leaving_mid_request_does_not_stall_the_next() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |owner| {
        let mut leaving = Client::hello(&at);
        let window = leaving.only_window();
        owner.ask(Command::Pause);
        leaving.send(&json!({ "id": 7, "op": "read", "window": window }));
        drop(leaving);
        owner.ask(Command::Resume);

        let mut client = Client::hello(&at);
        client.button(&window);
    });
}

fn an_unanswered_request_times_out_and_a_timed_out_act_still_runs() {
    let address = Address::new();
    let endpoint = address
        .endpoint()
        .with_reply_timeout(Duration::from_millis(200));
    let mut owner = Owner::new(AgentServer::new(endpoint));
    let at = address.address.clone();
    let presses = Arc::clone(&owner.presses);
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);

        owner.ask(Command::Pause);
        let act = client.tap(&window, &button);
        assert_eq!(act["error"]["code"], "timeout", "{act}");
        assert_eq!(act["error"]["effect"]["kind"], "may_have_run", "{act}");
        let read = client.call("read", json!({ "window": window }));
        assert_eq!(read["error"]["code"], "timeout", "{read}");
        assert_eq!(read["error"]["retry"], "soon", "{read}");
        assert_eq!(presses.load(Ordering::SeqCst), 0, "nothing ran unpumped");

        owner.ask(Command::Resume);
        client.wait_for_count(&window, 1);
    });
    assert_eq!(
        owner.presses(),
        1,
        "the timed-out action ran at the next drain, once"
    );
}

fn malformed_and_oversized_lines() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        client
            .send_bytes(b"this is not json\n")
            .expect("the line is written");
        let Recv::Line(reply) = client.recv(RECV) else {
            panic!("a malformed line is answered");
        };
        assert_eq!(reply["error"]["code"], "invalid_argument", "{reply}");
        assert_eq!(client.windows().len(), 1, "the session continues");

        let mut oversized = vec![b'a'; MAX_LINE + 16];
        oversized.push(b'\n');
        // The server may stop reading before the line ends.
        let _ = client.send_bytes(&oversized);
        let Recv::Line(reply) = client.recv(RECV) else {
            panic!("an oversized line is answered");
        };
        assert_eq!(reply["error"]["code"], "invalid_argument", "{reply}");
        assert!(
            matches!(client.recv(RECV), Recv::Closed),
            "then the connection closes"
        );
    });
}

fn a_closed_window_answers_gone_and_leaves_the_list() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        client.button(&window);

        owner.ask(Command::CloseWindow);
        let reply = client.call("read", json!({ "window": window }));
        assert_eq!(reply["error"]["code"], "gone", "{reply}");
        assert_eq!(reply["error"]["kind"], "window", "{reply}");
        assert_eq!(reply["error"]["handle"], json!(window), "{reply}");
        assert!(
            client.windows().is_empty(),
            "the list drops a closed window"
        );
        let again = client.call("read", json!({ "window": window }));
        assert_eq!(
            again["error"]["code"], "gone",
            "and still knows it: {again}"
        );
    });
}

fn detach_closes_the_endpoint() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        client.only_window();
        owner.ask(Command::Detach);
        assert!(
            matches!(client.recv(RECV), Recv::Closed),
            "an open connection closes"
        );
        assert!(connect(&at).is_err(), "no new connection is accepted");
        #[cfg(unix)]
        assert!(
            !std::path::Path::new(&at).exists(),
            "the socket file is removed"
        );
    });
    let realm = owner.realm.as_mut().expect("the realm is open");
    let _ = realm.pump(Duration::from_millis(16));
    assert!(
        !realm.collects_semantics(),
        "the detached server let go of the window's semantics work"
    );
}

fn a_client_that_stops_reading_does_not_hold_up_detach() {
    let address = Address::new();
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    let at = address.address.clone();
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        client.only_window();
        // Ask and never read, until the server has stopped reading too: its
        // replies fill the connection and it is stuck writing one.
        let cap = Instant::now() + Duration::from_secs(20);
        let mut refused_since: Option<Instant> = None;
        let mut id = 1_000_u64;
        let mut pending = Vec::new();
        while refused_since.is_none_or(|since| since.elapsed() < Duration::from_millis(300)) {
            assert!(Instant::now() < cap, "the server never stopped reading");
            if pending.is_empty() {
                pending = format!(
                    "{{\"id\":{id},\"op\":\"windows\"}}
"
                )
                .into_bytes();
                id += 1;
            }
            match (&client.stream).write(&pending) {
                Ok(count) if count > 0 => {
                    pending.drain(..count);
                    refused_since = None;
                }
                Ok(_) => {
                    refused_since.get_or_insert_with(Instant::now);
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    refused_since.get_or_insert_with(Instant::now);
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("the connection failed: {error}"),
            }
        }
        let started = Instant::now();
        owner.ask(Command::Detach);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "detach waited {:?} on a session stuck writing",
            started.elapsed()
        );
        // Not reconnected here: on Windows the pipe keeps the unread replies
        // until this client reads or leaves, and a connect to the name waits.
        drop(client);
    });
}

fn a_bind_failure_leaves_the_realm_running() {
    // The endpoint is taken by another server, serving no window.
    let address = Address::new();
    let first = HeadlessDevAgent::attach(AgentServer::new(address.endpoint()));
    let mut owner = Owner::new(AgentServer::new(address.endpoint()));
    assert!(first.is_attached());
    let at = address.address.clone();
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        assert!(
            client.windows().is_empty(),
            "the endpoint is the first server's; the second serves nothing"
        );
    });
    assert!(
        !owner
            .agent
            .as_ref()
            .is_some_and(HeadlessDevAgent::is_attached),
        "a server that could not bind is not attached"
    );
    let realm = owner.realm.as_mut().expect("the realm is open");
    let _ = realm.pump(Duration::from_millis(16));
    assert!(
        !realm.collects_semantics(),
        "a server that serves nothing costs the realm no semantics work"
    );
    drop(first);

    // A server whose token is too short is inert: not attached, no work.
    let mut owner = Owner::new(AgentServer::new(AgentEndpoint::new(
        Address::new().address,
        "short",
    )));
    assert!(
        !owner
            .agent
            .as_ref()
            .is_some_and(HeadlessDevAgent::is_attached)
    );
    let realm = owner.realm.as_mut().expect("the realm is open");
    let _ = realm.pump(Duration::from_millis(16));
    assert!(!realm.collects_semantics(), "an inert server costs nothing");

    // A socket in a directory others can enter is refused.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("the mode is set");
        let path = dir.path().join("agent.sock");
        let at = path.to_str().expect("a UTF-8 path").to_owned();
        let mut owner = Owner::new(AgentServer::new(AgentEndpoint::new(at.clone(), TOKEN)));
        assert!(!path.exists(), "the refused socket is removed");
        assert!(connect(&at).is_err());
        let realm = owner.realm.as_mut().expect("the realm is open");
        let _ = realm.pump(Duration::from_millis(16));
    }
}

fn traces_carry_no_labels_or_values() {
    use tracing_subscriber::filter::{LevelFilter, Targets};
    use tracing_subscriber::layer::SubscriberExt as _;

    const VALUE: &str = "secret-value-5d1e";
    let log = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let writer = Arc::clone(&log);
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || LogWriter(Arc::clone(&writer))),
        )
        .with(
            Targets::new()
                .with_target("flui_devtools", LevelFilter::TRACE)
                .with_target("flui_runtime::ui_realm::agent", LevelFilter::TRACE),
        );
    tracing::subscriber::set_global_default(subscriber)
        .expect("this binary installs no other global subscriber");

    let address = Address::new();
    let endpoint = address
        .endpoint()
        .with_reply_timeout(Duration::from_millis(200));
    let mut owner = Owner::new(AgentServer::new(endpoint));
    let at = address.address.clone();
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);
        let set = client.call(
            "act",
            json!({ "window": window, "request": { "element": button, "action": "set_value", "value": VALUE } }),
        );
        assert!(set.get("error").is_some(), "a button takes no value: {set}");
        owner.ask(Command::Pause);
        let _ = client.tap(&window, &button);
        let _ = client.call("read", json!({ "window": window }));
        owner.ask(Command::Resume);
        client.wait_for_count(&window, 1);
        owner.ask(Command::CloseWindow);
        let _ = client.call("read", json!({ "window": window }));
    });

    let log = String::from_utf8(log.lock().clone()).expect("the log is UTF-8");
    assert!(
        log.contains("agent request served") && log.contains("agent request failed"),
        "vacuous-pass guard: the server traced its requests\n{log}"
    );
    assert!(
        log.contains("dropping a semantics agent reply nobody waits for"),
        "vacuous-pass guard: the late answers were traced\n{log}"
    );
    for secret in ["Increment", "Count", VALUE, TOKEN] {
        assert!(!log.contains(secret), "`{secret}` reached the log\n{log}");
    }
}

struct LogWriter(Arc<parking_lot::Mutex<Vec<u8>>>);

impl io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Runs every row, then fails naming each row that failed.
fn run_cases(cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            failures.push(format!("case `{name}` failed: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_endpoint_contains_every_failure() {
    run_cases(&[
        (
            "a_connection_without_the_right_token_is_closed_unread",
            a_connection_without_the_right_token_is_closed_unread,
        ),
        (
            "a_client_leaving_mid_request_does_not_stall_the_next",
            a_client_leaving_mid_request_does_not_stall_the_next,
        ),
        (
            "an_unanswered_request_times_out_and_a_timed_out_act_still_runs",
            an_unanswered_request_times_out_and_a_timed_out_act_still_runs,
        ),
        (
            "malformed_and_oversized_lines",
            malformed_and_oversized_lines,
        ),
        (
            "a_closed_window_answers_gone_and_leaves_the_list",
            a_closed_window_answers_gone_and_leaves_the_list,
        ),
        ("detach_closes_the_endpoint", detach_closes_the_endpoint),
        (
            "a_client_that_stops_reading_does_not_hold_up_detach",
            a_client_that_stops_reading_does_not_hold_up_detach,
        ),
        (
            "a_bind_failure_leaves_the_realm_running",
            a_bind_failure_leaves_the_realm_running,
        ),
        (
            "traces_carry_no_labels_or_values",
            traces_carry_no_labels_or_values,
        ),
    ]);
}
