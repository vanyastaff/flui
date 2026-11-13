//! The driver agent end to end: a headless UI runtime hosts the counter, the
//! agent serves it over a real named pipe or Unix socket, and a client thread
//! speaks raw protocol lines while the test thread, the UI runtime's owner,
//! pumps frames.

use std::io::{self, Read as _, Write as _};
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use flui_protocol::framing::MAX_LINE;
use flui_protocol::version::{BREAKING, PROTOCOL_VERSION, ProtocolVersion};
use flui_protocol::{ActParams, ActionName, AppInfo, Condition, Locator, Node, Target, Tree};
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::prelude::*;
use flui_view::{Signal, StatefulView, ViewState};
use flui_widgets::{Column, RawButton, Text};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use serde_json::{Value, json};

use super::super::Disconnect;
use super::{DriverAgent, Launch, Limits};

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

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        let presses = Arc::clone(&view.presses);
        Column::new((
            Text::new(format!("Count: {}", count.get(ctx))),
            RawButton::new(Text::new("Increment")).on_pressed(move |ctx| {
                presses.fetch_add(1, Ordering::SeqCst);
                count.update(ctx, |n| *n += 1)
            }),
        ))
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
            // The agent refuses a socket whose directory others can enter,
            // and `tempfile` creates directories with the umask's mode
            // (usually 0755), so the mode is asked for explicitly.
            use std::os::unix::fs::PermissionsExt as _;
            let dir = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .expect("a temporary directory");
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

    fn launch(&self) -> Launch {
        Launch::try_new(self.address.clone(), TOKEN.to_owned(), false).expect("a long token")
    }
}

/// An agent for `launch` that counts the quits a driver's disconnect asks.
fn driver_agent(launch: Launch) -> (DriverAgent, Arc<Disconnect>, Arc<AtomicUsize>) {
    driver_agent_journaling(launch, false)
}

/// An agent for `launch` that counts quits, listing the `native` layer when `records_native`.
fn driver_agent_journaling(
    launch: Launch,
    records_native: bool,
) -> (DriverAgent, Arc<Disconnect>, Arc<AtomicUsize>) {
    let quits = Arc::new(AtomicUsize::new(0));
    let disconnect = Arc::new(Disconnect::default());
    let counted = Arc::clone(&quits);
    disconnect.arm(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let agent = DriverAgent::new(
        launch,
        AppInfo::new("counter", "headless").with_pid(7),
        Arc::clone(&disconnect),
        records_native,
    );
    (agent, disconnect, quits)
}

fn connect(address: &str) -> io::Result<Stream> {
    #[cfg(windows)]
    let name = {
        use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
        address.to_ns_name::<GenericNamespaced>()?
    };
    #[cfg(target_os = "linux")]
    if let Some(name) = address.strip_prefix('@') {
        use interprocess::local_socket::ToNsName as _;
        use interprocess::os::unix::local_socket::AbstractNsUdSocket;
        return Stream::connect(name.to_ns_name::<AbstractNsUdSocket>()?);
    }
    #[cfg(unix)]
    let name = {
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()?
    };
    Stream::connect(name)
}

// ---------------------------------------------------------------------------
// A raw protocol client
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
    /// Event lines read while waiting for a reply.
    events: Vec<Value>,
}

impl Client {
    fn connect(address: &str) -> Self {
        let stream = connect(address).expect("the endpoint accepts a connection");
        stream.set_nonblocking(true).expect("non-blocking");
        Self {
            stream,
            buffer: Vec::new(),
            next_id: 1,
            events: Vec::new(),
        }
    }

    fn hello_line(token: &str) -> Value {
        json!({ "hello": { "token": token, "protocol": PROTOCOL_VERSION.to_string(), "client": "test" } })
    }

    /// Connected and past a hello asking for `role`, `None` for the driver's; the hello's
    /// answer.
    fn greeted_as(address: &str, role: Option<&str>) -> (Self, Value) {
        let mut client = Self::connect(address);
        let mut hello = Self::hello_line(TOKEN);
        if let Some(role) = role {
            hello["hello"]["role"] = json!(role);
        }
        client.send(&hello);
        match client.recv(RECV) {
            Recv::Line(welcome) => (client, welcome),
            Recv::Closed => panic!("the agent closed the connection at the hello"),
            Recv::TimedOut => panic!("the agent did not greet"),
        }
    }

    /// Connected and past the hello, as the driver; the hello's answer.
    fn greeted(address: &str) -> (Self, Value) {
        Self::greeted_as(address, None)
    }

    fn hello(address: &str) -> Self {
        Self::hello_as(address, None)
    }

    fn hello_as(address: &str, role: Option<&str>) -> Self {
        let (client, welcome) = Self::greeted_as(address, role);
        assert_eq!(
            welcome["hello"]["protocol"],
            json!(PROTOCOL_VERSION.to_string()),
            "the agent greets in the client's protocol: {welcome}"
        );
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

    /// The next reply line, keeping the event lines read before it.
    fn reply(&mut self, what: &str) -> Value {
        loop {
            match self.recv(RECV) {
                Recv::Line(line) if line.get("event").is_some() => self.events.push(line),
                Recv::Line(reply) => return reply,
                Recv::Closed => panic!("the agent closed the connection during `{what}`"),
                Recv::TimedOut => panic!("no reply to `{what}`"),
            }
        }
    }

    /// Sends `{"id":…,"method":method,"params":params}` and returns the reply.
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "id": id, "method": method, "params": params }));
        let reply = self.reply(method);
        assert_eq!(reply["id"], json!(id), "replies come in order: {reply}");
        reply
    }

    /// Whether the agent closed the connection without writing anything.
    fn closed_unread(&mut self) -> bool {
        matches!(self.recv(RECV), Recv::Closed) && self.buffer.is_empty()
    }

    fn windows(&mut self) -> Vec<String> {
        let reply = self.call("windows.list", json!({}));
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
            let reply = self.call("semantics.read", json!({ "window": window }));
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

    /// The button's element reference, `w<n>/e<m>`.
    fn button(&mut self, window: &str) -> String {
        let element = self.read_until(window, |tree| node(tree, "Increment").map(|n| n.id));
        format!("{window}/{element}")
    }

    fn wait_for_count(&mut self, window: &str, count: u32) {
        let label = format!("Count: {count}");
        self.read_until(window, |tree| node(tree, &label).map(|_| ()));
    }

    /// Reads until an event `found` accepts has arrived.
    fn next_event(&mut self, found: impl Fn(&Value) -> bool, what: &str) {
        let deadline = Instant::now() + RECV;
        while !self.events.iter().any(&found) {
            assert!(Instant::now() < deadline, "{what} never arrived");
            if let Recv::Line(line) = self.recv(Duration::from_millis(50)) {
                self.events.push(line);
            }
        }
    }

    fn tap(&mut self, element: &str) -> Value {
        self.call(
            "semantics.act",
            json!({ "target": { "element": element }, "action": "invoke" }),
        )
    }
}

/// A non-blocking read that returns `Ok(0)` only once the agent has closed:
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
    /// Drop the UI runtime: its window closes.
    CloseWindow,
    /// Open a second window with its own counter.
    OpenWindow,
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
    ui_runtime: Option<HeadlessHost>,
    /// A window opened while a client talks.
    second: Option<HeadlessHost>,
    presses: Arc<AtomicU32>,
}

impl Owner {
    fn new(agent: DriverAgent) -> Self {
        let mut agent = HeadlessDevAgent::attach(agent);
        let presses = Arc::new(AtomicU32::new(0));
        let ui_runtime =
            HeadlessHost::new(HeadlessWindow::new(400, 300)).with_dev_agent(&mut agent);
        ui_runtime
            .attach(&Counter {
                presses: Arc::clone(&presses),
            })
            .expect("the counter attaches");
        Self {
            agent: Some(agent),
            ui_runtime: Some(ui_runtime),
            second: None,
            presses,
        }
    }

    fn open_second(&mut self) {
        let agent = self.agent.as_mut().expect("the agent is attached");
        let second = HeadlessHost::new(HeadlessWindow::new(400, 300)).with_dev_agent(agent);
        second
            .attach(&Counter {
                presses: Arc::clone(&self.presses),
            })
            .expect("the second counter attaches");
        self.second = Some(second);
    }

    fn presses(&self) -> u32 {
        self.presses.load(Ordering::SeqCst)
    }

    fn is_attached(&self) -> bool {
        self.agent
            .as_ref()
            .is_some_and(HeadlessDevAgent::is_attached)
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
                        Command::CloseWindow => drop(self.ui_runtime.take()),
                        Command::Detach => drop(self.agent.take()),
                        Command::OpenWindow => self.open_second(),
                    }
                    let _ = done.send(());
                }
                if pumping {
                    for ui_runtime in self.ui_runtime.iter_mut().chain(self.second.iter_mut()) {
                        let _ = ui_runtime.pump(Duration::from_millis(16));
                    }
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

/// The hello lists the `native` recording layer exactly when the loop journals its backend.
#[test]
fn the_hello_lists_the_native_layer_only_when_the_loop_journals() {
    for (records_native, layers) in [
        (false, json!(["input"])),
        (true, json!(["input", "native"])),
    ] {
        let address = Address::new();
        let (agent, _, _) = driver_agent_journaling(address.launch(), records_native);
        let mut owner = Owner::new(agent);
        let at = address.address;
        owner.drive(move |_| {
            let (_client, welcome) = Client::greeted(&at);
            assert_eq!(
                welcome["hello"]["capabilities"]["record"], layers,
                "{welcome}"
            );
        });
    }
}

#[test]
fn reads_the_counter_and_taps_it_by_element_and_by_locator() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let (mut client, welcome) = Client::greeted(&at);
        assert_eq!(welcome["hello"]["app"]["name"], "counter", "{welcome}");
        assert_eq!(welcome["hello"]["app"]["pid"], 7, "{welcome}");
        let methods = welcome["hello"]["capabilities"]["methods"].clone();
        for method in [
            "windows.list",
            "semantics.read",
            "semantics.act",
            "wait.until",
        ] {
            assert!(
                methods
                    .as_array()
                    .is_some_and(|all| all.contains(&json!(method))),
                "the hello lists {method}: {welcome}"
            );
        }

        let window = client.only_window();
        let button = client.button(&window);
        let reply = client.tap(&button);
        assert_eq!(
            reply["result"]["via"], "semantic",
            "the tap is delivered: {reply}"
        );
        client.wait_for_count(&window, 1);

        let by_locator = ActParams::new(
            Target::Locator(Locator::named("Increment")),
            ActionName::Invoke,
        );
        let reply = client.call(
            "semantics.act",
            serde_json::to_value(by_locator).expect("serializes"),
        );
        assert_eq!(
            reply["result"]["via"], "semantic",
            "the locator resolves: {reply}"
        );
        client.wait_for_count(&window, 2);

        let missing = ActParams::new(
            Target::Locator(Locator::named("Decrement")),
            ActionName::Invoke,
        );
        let reply = client.call(
            "semantics.act",
            serde_json::to_value(missing).expect("serializes"),
        );
        assert_eq!(reply["error"]["code"], "not_found", "{reply}");
        assert_eq!(reply["error"]["retry"], "when_appears", "{reply}");
    });
    assert_eq!(owner.presses(), 2, "the button's handler ran once per tap");
}

// ---------------------------------------------------------------------------
// The failure table
// ---------------------------------------------------------------------------

fn motion_round_trips() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let paused = client.call(
            "motion.set",
            json!({ "window": window, "request": { "rate": 0.0 } }),
        );
        assert_eq!(paused["result"]["rate"], 0.0, "{paused}");
        let start = paused["result"]["time_ms"]
            .as_f64()
            .expect("animation time");
        let stepped = client.call(
            "motion.set",
            json!({ "window": window, "request": { "step_ms": 100 } }),
        );
        assert_eq!(stepped["result"]["rate"], 0.0, "{stepped}");
        assert!(
            (stepped["result"]["time_ms"].as_f64().expect("time") - start - 100.0).abs() < 1e-9
        );
        let refused = client.call(
            "motion.set",
            json!({ "window": window, "request": { "rate": -1.0, "step_ms": 500 } }),
        );
        assert_eq!(refused["error"]["code"], "invalid_argument", "{refused}");
        let observed = client.call("motion.set", json!({ "window": window, "request": {} }));
        assert_eq!(
            observed["result"], stepped["result"],
            "a refused combined request changes neither field"
        );
        owner.ask(Command::CloseWindow);
        let closed = client.call("motion.set", json!({ "window": window, "request": {} }));
        assert_eq!(closed["error"]["code"], "gone", "{closed}");
    });
}

fn a_connection_without_the_right_hello_is_refused() {
    let address = Address::new();
    let launch = address.launch().with_limits(Limits {
        handshake_timeout: Duration::from_millis(300),
        ..Limits::default()
    });
    let (agent, _, quits) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let mut no_hello = Client::connect(&at);
        no_hello.send(&json!({ "id": 1, "method": "windows.list" }));
        assert!(no_hello.closed_unread(), "a request before the hello");

        let mut wrong = Client::connect(&at);
        wrong.send(&Client::hello_line(&"f".repeat(TOKEN.len())));
        assert!(wrong.closed_unread(), "a wrong token");

        let mut prefix = Client::connect(&at);
        prefix.send(&Client::hello_line(&TOKEN[..TOKEN.len() - 1]));
        assert!(prefix.closed_unread(), "the token's prefix");

        let mut silent = Client::connect(&at);
        assert!(silent.closed_unread(), "no hello before the timeout");

        let mut old = Client::connect(&at);
        old.send(&json!({ "hello": { "token": TOKEN, "protocol": "0.4" } }));
        let Recv::Line(refusal) = old.recv(RECV) else {
            panic!("a hello in another protocol is answered");
        };
        assert_eq!(refusal["error"]["code"], "version", "{refusal}");
        assert_eq!(refusal["error"]["client"], "0.4", "{refusal}");
        assert!(matches!(old.recv(RECV), Recv::Closed), "then it closes");

        // A client older than the last break is refused; one the break does not separate from
        // the app (the version that broke, or a later one) is served, in the app's version.
        let broke = BREAKING
            .iter()
            .map(|broke| broke.version)
            .max()
            .expect("a break is listed");
        let before = ProtocolVersion::new(broke.major, broke.minor - 1);
        let mut across = Client::connect(&at);
        across.send(&json!({ "hello": { "token": TOKEN, "protocol": before.to_string() } }));
        let Recv::Line(refusal) = across.recv(RECV) else {
            panic!("a client across the break is answered");
        };
        assert_eq!(refusal["error"]["code"], "version", "{refusal}");
        assert_eq!(
            refusal["error"]["client"],
            json!(before.to_string()),
            "{refusal}"
        );
        assert!(matches!(across.recv(RECV), Recv::Closed), "then it closes");

        let mut earlier = Client::connect(&at);
        earlier.send(&json!({ "hello": { "token": TOKEN, "protocol": broke.to_string() } }));
        let Recv::Line(welcome) = earlier.recv(RECV) else {
            panic!("a client the break does not separate is greeted");
        };
        assert_eq!(
            welcome["hello"]["protocol"],
            json!(PROTOCOL_VERSION.to_string()),
            "{welcome}"
        );
        assert_eq!(earlier.windows().len(), 1, "and served");
        drop(earlier);

        // A newer one is told the app's version, to say hello again in it.
        let newer = ProtocolVersion::new(PROTOCOL_VERSION.major, PROTOCOL_VERSION.minor + 1);
        let mut later = Client::connect(&at);
        later.send(&json!({ "hello": { "token": TOKEN, "protocol": newer.to_string() } }));
        let Recv::Line(refusal) = later.recv(RECV) else {
            panic!("a newer client is answered");
        };
        assert_eq!(refusal["error"]["code"], "version", "{refusal}");
        assert_eq!(
            refusal["error"]["app"],
            json!(PROTOCOL_VERSION.to_string()),
            "{refusal}"
        );
        assert!(
            refusal.get("id").is_none(),
            "it answers no request: {refusal}"
        );

        let mut client = Client::hello(&at);
        assert_eq!(client.windows().len(), 1, "a right client is served next");
    });
    assert_eq!(owner.presses(), 0);
    assert_eq!(quits.load(Ordering::SeqCst), 0, "no exit was asked for");
}

fn a_client_leaving_mid_request_does_not_stall_the_next() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        // An observer, so the next client is admitted as the driver however soon the agent
        // sees this one go.
        let mut leaving = Client::hello_as(&at, Some("observer"));
        let window = leaving.only_window();
        owner.ask(Command::Pause);
        leaving
            .send(&json!({ "id": 7, "method": "semantics.read", "params": { "window": window } }));
        drop(leaving);
        owner.ask(Command::Resume);

        let mut client = Client::hello(&at);
        client.button(&window);
    });
}

fn an_unanswered_request_times_out_and_a_timed_out_act_still_runs() {
    let address = Address::new();
    let launch = address.launch().with_limits(Limits {
        reply_timeout: Duration::from_millis(200),
        ..Limits::default()
    });
    let (agent, _, _) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    let presses = Arc::clone(&owner.presses);
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);

        owner.ask(Command::Pause);
        let act = client.tap(&button);
        assert_eq!(act["error"]["code"], "timeout", "{act}");
        assert_eq!(act["error"]["effect"]["kind"], "may_have_run", "{act}");
        assert_eq!(act["error"]["retry"], "never", "{act}");
        let read = client.call("semantics.read", json!({ "window": window }));
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

fn a_full_queue_answers_busy_and_keeps_what_it_took() {
    let address = Address::new();
    let launch = address.launch().with_limits(Limits {
        queue: 2,
        ..Limits::default()
    });
    let (agent, _, _) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        client.button(&window);
        owner.ask(Command::Pause);
        // The worker holds the first while the owner is paused, the queue
        // the second; the third has no room.
        for id in 1..=3 {
            client.send(
                &json!({ "id": id, "method": "semantics.read", "params": { "window": window } }),
            );
        }
        let busy = client.reply("a third request");
        assert_eq!(busy["id"], 3, "the refusal comes first: {busy}");
        assert_eq!(busy["error"]["code"], "busy", "{busy}");
        assert_eq!(busy["error"]["retry"], "soon", "{busy}");
        owner.ask(Command::Resume);
        for id in 1..=2 {
            let reply = client.reply("a queued request");
            assert_eq!(
                reply["id"], id,
                "the accepted ones are answered in order: {reply}"
            );
            assert!(reply.get("result").is_some(), "{reply}");
        }
    });
}

fn malformed_unknown_and_unserved_requests() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let (mut client, welcome) = Client::greeted(&at);
        // The hello lists exactly what the app serves: an agent method it leaves out is
        // refused `not_supported`, and one it lists reaches its parameters' check.
        let listed: Vec<String> =
            serde_json::from_value(welcome["hello"]["capabilities"]["methods"].clone())
                .expect("the hello lists methods");
        for method in flui_protocol::METHODS
            .iter()
            .filter(|method| method.served_by == flui_protocol::ServedBy::Agent)
        {
            let reply = client.call(method.name, json!({}));
            let refused = reply["error"]["code"] == "not_supported";
            assert_eq!(
                refused,
                !listed.iter().any(|name| name == method.name),
                "{}: {reply}",
                method.name
            );
            if refused {
                assert_eq!(reply["error"]["capability"], method.name, "{reply}");
            }
        }

        client
            .send_bytes(b"this is not json\n")
            .expect("the line is written");
        let reply = client.reply("a malformed line");
        assert_eq!(reply["error"]["code"], "invalid_argument", "{reply}");
        assert_eq!(client.windows().len(), 1, "the session continues");

        let unknown = client.call("no.such", json!({}));
        assert_eq!(unknown["error"]["code"], "invalid_argument", "{unknown}");
        let driver_side = client.call("session.launch", json!({}));
        assert_eq!(
            driver_side["error"]["code"], "not_supported",
            "{driver_side}"
        );
        assert_eq!(
            driver_side["error"]["capability"], "session.launch",
            "{driver_side}"
        );
        let not_yet = client.call("perf.start", json!({}));
        assert_eq!(not_yet["error"]["code"], "not_supported", "{not_yet}");
        let malformed = client.call("semantics.read", json!({ "window": 3 }));
        assert_eq!(
            malformed["error"]["code"], "invalid_argument",
            "{malformed}"
        );

        let mut oversized = vec![b'a'; MAX_LINE + 16];
        oversized.push(b'\n');
        // The agent may stop reading before the line ends.
        let _ = client.send_bytes(&oversized);
        let reply = client.reply("an oversized line");
        assert_eq!(reply["error"]["code"], "invalid_argument", "{reply}");
        assert!(
            matches!(client.recv(RECV), Recv::Closed),
            "then the connection closes"
        );
    });
}

fn a_closed_window_answers_gone_and_leaves_the_list() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        client.button(&window);

        owner.ask(Command::CloseWindow);
        let reply = client.call("semantics.read", json!({ "window": window }));
        assert_eq!(reply["error"]["code"], "gone", "{reply}");
        assert_eq!(reply["error"]["kind"], "window", "{reply}");
        assert_eq!(reply["error"]["handle"], json!(window), "{reply}");
        assert!(
            client.windows().is_empty(),
            "the list drops a closed window"
        );
        let again = client.call("semantics.read", json!({ "window": window }));
        assert_eq!(
            again["error"]["code"], "gone",
            "and still knows it: {again}"
        );
        let never = client.call("semantics.read", json!({ "window": "w999" }));
        assert_eq!(never["error"]["code"], "unknown_handle", "{never}");
    });
}

fn events_stream_after_a_subscription_until_it_ends() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);
        let subscribed = client.call(
            "events.subscribe",
            json!({ "events": [{ "kind": "semantics" }] }),
        );
        let subscription = subscribed["result"]["subscription"].clone();
        assert!(subscription.is_u64(), "{subscribed}");
        client.tap(&button);
        client.next_event(|event| event["event"] == "semantics", "a semantics event");

        // A window opened after the subscription is streamed too.
        owner.ask(Command::OpenWindow);
        let deadline = Instant::now() + RECV;
        let second = loop {
            if let Some(second) = client.windows().into_iter().find(|id| *id != window) {
                break second;
            }
            assert!(Instant::now() < deadline, "the second window never listed");
            std::thread::sleep(Duration::from_millis(5));
        };
        let button = client.button(&second);
        client.events.clear();
        client.tap(&button);
        client.next_event(
            |event| event["event"] == "semantics" && event["params"]["window"] == second.as_str(),
            "a semantics event of the window opened later",
        );

        let ended = client.call(
            "events.unsubscribe",
            json!({ "subscription": subscription }),
        );
        assert_eq!(ended["result"], json!({}), "{ended}");
        let again = client.call(
            "events.unsubscribe",
            json!({ "subscription": subscription }),
        );
        assert_eq!(again["error"]["code"], "unknown_handle", "{again}");
    });
}

fn a_window_too_busy_for_a_subscription_is_asked_again() {
    let address = Address::new();
    // One past the cap a window keeps (`MAX_SUBSCRIPTIONS`, 64).
    let launch = address.launch().with_limits(Limits {
        subscriptions: 65,
        ..Limits::default()
    });
    let (agent, _, _) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        client.only_window();
        let semantics = json!({ "events": [{ "kind": "semantics" }] });
        let first = client.call("events.subscribe", semantics.clone());
        let first = first["result"]["subscription"].clone();
        for _ in 1..64 {
            let subscribed = client.call("events.subscribe", semantics.clone());
            assert!(
                subscribed["result"]["subscription"].is_u64(),
                "{subscribed}"
            );
        }
        // With no window open, the last subscription reaches none yet.
        owner.ask(Command::CloseWindow);
        let deadline = Instant::now() + RECV;
        while !client.windows().is_empty() {
            assert!(Instant::now() < deadline, "the window never closed");
            std::thread::sleep(Duration::from_millis(5));
        }
        let frames = client.call(
            "events.subscribe",
            json!({ "events": [{ "kind": "frame" }] }),
        );
        assert!(frames["result"]["subscription"].is_u64(), "{frames}");

        // The window opened next holds the 64 subscriptions made first, and
        // refuses the last as busy.
        owner.ask(Command::OpenWindow);
        let second = loop {
            if let Some(second) = client.windows().pop() {
                break second;
            }
            assert!(Instant::now() < deadline, "the second window never listed");
            std::thread::sleep(Duration::from_millis(5));
        };
        let button = client.button(&second);
        // Ending one frees a place, and the refused subscription takes it.
        let ended = client.call("events.unsubscribe", json!({ "subscription": first }));
        assert_eq!(ended["result"], json!({}), "{ended}");
        client.events.clear();
        client.tap(&button);
        client.next_event(
            |event| event["event"] == "frame" && event["params"]["window"] == second.as_str(),
            "a frame event of the window that was busy",
        );
    });
}

fn a_wait_without_a_timeout_is_answered_by_the_window() {
    let address = Address::new();
    let launch = address.launch().with_limits(Limits {
        reply_timeout: Duration::from_millis(300),
        ..Limits::default()
    });
    let (agent, _, _) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let condition = serde_json::to_value(Condition::Appears(Locator::named("Missing")))
            .expect("a condition serializes");
        // No `timeout_ms`: the window's default, longer than the transport's own wait.
        let reply = client.call(
            "wait.until",
            json!({ "window": window, "condition": condition }),
        );
        assert_eq!(
            reply["result"]["satisfied"], false,
            "the window answers its own timeout: {reply}"
        );
    });
}

fn detach_closes_the_endpoint_and_asks_no_exit() {
    let address = Address::new();
    let launch =
        Launch::try_new(address.address.clone(), TOKEN.to_owned(), true).expect("a long token");
    let (agent, disconnect, quits) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
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
    assert!(
        !disconnect.happened(),
        "the agent stopping is no disconnect"
    );
    assert_eq!(quits.load(Ordering::SeqCst), 0);
    let ui_runtime = owner.ui_runtime.as_mut().expect("the ui_runtime is open");
    let _ = ui_runtime.pump(Duration::from_millis(16));
    assert!(
        !ui_runtime.collects_semantics(),
        "the detached agent let go of the window's semantics work"
    );
}

fn the_driver_going_asks_one_exit_when_launched_to() {
    let address = Address::new();
    let launch =
        Launch::try_new(address.address.clone(), TOKEN.to_owned(), true).expect("a long token");
    let (agent, disconnect, quits) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    let counted = Arc::clone(&quits);
    owner.drive(move |_| {
        // A connection refused at its hello is not the driver.
        let mut stranger = Client::connect(&at);
        stranger.send(&Client::hello_line("wrong"));
        assert!(stranger.closed_unread());
        assert_eq!(
            counted.load(Ordering::SeqCst),
            0,
            "a stranger's going asks nothing"
        );

        let mut driver = Client::hello(&at);
        driver.only_window();
        // An inspector observing beside the driver comes and goes: the app stays.
        let mut inspector = Client::hello_as(&at, Some("observer"));
        inspector.only_window();
        drop(inspector);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            counted.load(Ordering::SeqCst),
            0,
            "only the driver's going ends the app"
        );
        driver.only_window();
        drop(driver);
        let deadline = Instant::now() + RECV;
        while counted.load(Ordering::SeqCst) == 0 {
            assert!(
                Instant::now() < deadline,
                "the driver's going asked no exit"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // The app is ending: another driver is refused, an observer still reads.
        let (refused, answer) = Client::greeted(&at);
        assert_eq!(answer["error"]["code"], "shutting_down", "{answer}");
        drop(refused);
        let mut second = Client::hello_as(&at, Some("observer"));
        second.only_window();
        drop(second);
        std::thread::sleep(Duration::from_millis(100));
    });
    assert!(disconnect.happened());
    assert_eq!(quits.load(Ordering::SeqCst), 1, "the exit is asked once");

    // Launched without the flag, the driver's going asks nothing.
    let address = Address::new();
    let (agent, disconnect, quits) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let mut driver = Client::hello(&at);
        driver.only_window();
        drop(driver);
        std::thread::sleep(Duration::from_millis(100));
    });
    assert!(!disconnect.happened());
    assert_eq!(quits.load(Ordering::SeqCst), 0);
}

fn a_client_that_stops_reading_does_not_hold_up_detach() {
    let address = Address::new();
    let (agent, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |owner| {
        let mut client = Client::hello(&at);
        client.only_window();
        // Ask and never read, until the agent has stopped reading too: its
        // replies fill the connection and it is stuck writing one.
        let cap = Instant::now() + Duration::from_secs(20);
        let mut refused_since: Option<Instant> = None;
        let mut id = 1_000_u64;
        let mut pending = Vec::new();
        while refused_since.is_none_or(|since| since.elapsed() < Duration::from_millis(300)) {
            assert!(Instant::now() < cap, "the agent never stopped reading");
            if pending.is_empty() {
                pending = format!("{{\"id\":{id},\"method\":\"windows.list\"}}\n").into_bytes();
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
            started.elapsed() < Duration::from_secs(3),
            "detach waited {:?} on a session stuck writing",
            started.elapsed()
        );
        // Not reconnected here: on Windows the pipe keeps the unread replies
        // until this client reads or leaves, and a connect to the name waits.
        drop(client);
    });
}

fn a_bind_failure_leaves_the_ui_runtime_running() {
    // The endpoint is taken by another agent, serving no window.
    let address = Address::new();
    let (first, _, _) = driver_agent(address.launch());
    let first = HeadlessDevAgent::attach(first);
    let (second, _, _) = driver_agent(address.launch());
    let mut owner = Owner::new(second);
    assert!(first.is_attached());
    let at = address.address;
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        assert!(
            client.windows().is_empty(),
            "the endpoint is the first agent's; the second serves nothing"
        );
    });
    assert!(
        !owner.is_attached(),
        "an agent that could not bind is not attached"
    );
    let ui_runtime = owner.ui_runtime.as_mut().expect("the ui_runtime is open");
    let _ = ui_runtime.pump(Duration::from_millis(16));
    assert!(
        !ui_runtime.collects_semantics(),
        "an agent that serves nothing costs the ui_runtime no semantics work"
    );
    drop(first);

    // A token too short makes no launch, so nothing is created.
    assert!(Launch::try_new(Address::new().address, "short".to_owned(), false).is_none());

    // A socket in a directory others can enter is refused.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))
            .expect("the mode is set");
        let path = dir.path().join("agent.sock");
        let at = path.to_str().expect("a UTF-8 path").to_owned();
        let launch = Launch::try_new(at.clone(), TOKEN.to_owned(), false).expect("a long token");
        let (agent, _, _) = driver_agent(launch);
        let mut owner = Owner::new(agent);
        assert!(!owner.is_attached());
        assert!(!path.exists(), "the refused socket is removed");
        assert!(connect(&at).is_err());
        let ui_runtime = owner.ui_runtime.as_mut().expect("the ui_runtime is open");
        let _ = ui_runtime.pump(Duration::from_millis(16));
    }
}

fn unbounded_timeouts_wait_without_a_deadline() {
    let address = Address::new();
    let launch = address.launch().with_limits(Limits {
        handshake_timeout: Duration::MAX,
        reply_timeout: Duration::MAX,
        ..Limits::default()
    });
    let (agent, _, _) = driver_agent(launch);
    let mut owner = Owner::new(agent);
    let at = address.address;
    owner.drive(move |_| {
        let mut client = Client::hello(&at);
        let window = client.only_window();
        let button = client.button(&window);
        let reply = client.tap(&button);
        assert_eq!(
            reply["result"]["via"], "semantic",
            "the tap is answered: {reply}"
        );
        client.wait_for_count(&window, 1);
    });
    assert_eq!(owner.presses(), 1, "the button's handler ran once");
}

/// An abstract socket (`@name`), Android's endpoint, serves a peer of the app's user, and closes
/// unread a connection whose peer its rule does not admit.
#[cfg(target_os = "linux")]
fn an_abstract_socket_serves_its_user_and_closes_others_unread() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let at = format!("@flui-agent-test-{}-{nanos}", std::process::id());
    let launch = || Launch::try_new(at.clone(), TOKEN.to_owned(), false).expect("a long token");

    let (agent, _, _) = driver_agent(launch());
    let mut owner = Owner::new(agent);
    assert!(owner.is_attached(), "the abstract socket is bound");
    let served = at.clone();
    owner.drive(move |_| {
        let mut client = Client::hello(&served);
        let _ = client.only_window();
    });
    drop(owner);

    let (agent, _, _) = driver_agent(launch().with_peer_rule(|_| false));
    let mut owner = Owner::new(agent);
    let refused = at.clone();
    owner.drive(move |_| {
        let mut stranger = Client::connect(&refused);
        let mut hello = serde_json::to_vec(&Client::hello_line(TOKEN)).expect("serializes");
        hello.push(b'\n');
        // The agent closes a refused peer without reading, so the hello can find the connection
        // already closed; that write failing is the same outcome as the hello left unread.
        if let Err(error) = stranger.send_bytes(&hello) {
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                ),
                "the hello finds the connection open or closed: {error}"
            );
        }
        assert!(
            stranger.closed_unread(),
            "a peer the rule refuses is closed before its hello is read"
        );
    });
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
fn the_driver_agent_contains_every_failure() {
    run_cases(&[
        ("motion_round_trips", motion_round_trips),
        (
            "a_connection_without_the_right_hello_is_refused",
            a_connection_without_the_right_hello_is_refused,
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
            "a_full_queue_answers_busy_and_keeps_what_it_took",
            a_full_queue_answers_busy_and_keeps_what_it_took,
        ),
        (
            "malformed_unknown_and_unserved_requests",
            malformed_unknown_and_unserved_requests,
        ),
        (
            "a_closed_window_answers_gone_and_leaves_the_list",
            a_closed_window_answers_gone_and_leaves_the_list,
        ),
        (
            "events_stream_after_a_subscription_until_it_ends",
            events_stream_after_a_subscription_until_it_ends,
        ),
        (
            "a_window_too_busy_for_a_subscription_is_asked_again",
            a_window_too_busy_for_a_subscription_is_asked_again,
        ),
        (
            "a_wait_without_a_timeout_is_answered_by_the_window",
            a_wait_without_a_timeout_is_answered_by_the_window,
        ),
        (
            "detach_closes_the_endpoint_and_asks_no_exit",
            detach_closes_the_endpoint_and_asks_no_exit,
        ),
        (
            "the_driver_going_asks_one_exit_when_launched_to",
            the_driver_going_asks_one_exit_when_launched_to,
        ),
        (
            "a_client_that_stops_reading_does_not_hold_up_detach",
            a_client_that_stops_reading_does_not_hold_up_detach,
        ),
        (
            "a_bind_failure_leaves_the_ui_runtime_running",
            a_bind_failure_leaves_the_ui_runtime_running,
        ),
        (
            "unbounded_timeouts_wait_without_a_deadline",
            unbounded_timeouts_wait_without_a_deadline,
        ),
        #[cfg(target_os = "linux")]
        (
            "an_abstract_socket_serves_its_user_and_closes_others_unread",
            an_abstract_socket_serves_its_user_and_closes_others_unread,
        ),
    ]);
}

#[test]
fn the_launch_is_read_through_its_keys() {
    use flui_protocol::launch::{ENDPOINT_ENV, EXIT_ON_DISCONNECT_ENV, TOKEN_ENV};

    use super::launch::{ENDPOINT, EXIT_ON_DISCONNECT, TOKEN as TOKEN_KEY};

    // A desktop reads the variables a driver sets, under the keys an Android
    // launch names its extras with.
    for (key, variable) in [
        (ENDPOINT, ENDPOINT_ENV),
        (TOKEN_KEY, TOKEN_ENV),
        (EXIT_ON_DISCONNECT, EXIT_ON_DISCONNECT_ENV),
    ] {
        assert_eq!(key.environment_variable(), variable, "{key}");
    }
    // A page reads the object a driver installs under the name the protocol tells it.
    assert_eq!(
        flui_platform::PAGE_LAUNCH_GLOBAL,
        flui_protocol::launch::PAGE_LAUNCH_GLOBAL
    );

    let read = |endpoint: Option<&str>, token: &str, exit: Option<&str>| {
        Launch::read(|key| {
            let value = if key == ENDPOINT {
                endpoint
            } else if key == TOKEN_KEY {
                Some(token)
            } else if key == EXIT_ON_DISCONNECT {
                exit
            } else {
                panic!("the launch reads no other key: {key}")
            };
            value.map(str::to_owned)
        })
    };
    let at = "/run/user/agent.sock";
    assert!(read(None, TOKEN, None).is_none(), "no endpoint, no agent");
    assert!(read(Some(at), "short", None).is_none(), "a short token");
    let plain = read(Some(at), TOKEN, None).expect("a launch");
    assert_eq!(plain.address, at);
    assert!(!plain.exit_on_disconnect());
    assert!(
        read(Some(at), TOKEN, Some("1"))
            .expect("a launch")
            .exit_on_disconnect()
    );
    assert!(
        !read(Some(at), TOKEN, Some("true"))
            .expect("a launch")
            .exit_on_disconnect(),
        "only `1` turns the exit on"
    );
    assert!(plain.peers.is_none(), "a socket file needs no peer rule");
    let abstract_launch = read(Some("@flui-agent-1"), TOKEN, None).expect("a launch");
    assert_eq!(
        abstract_launch.peers.is_some(),
        cfg!(any(target_os = "linux", target_os = "android")),
        "an abstract socket admits only the peers its rule does"
    );
}

#[test]
fn an_abstract_socket_admits_its_user_root_and_on_android_the_shell() {
    use super::launch::peer_admitted;

    const OWN: u32 = 10_123;
    for (peer, android, admitted) in [
        (OWN, false, true),
        (OWN, true, true),
        (0, false, true),
        (0, true, true),
        (2000, true, true),
        (2000, false, false),
        (10_124, true, false),
        (1000, false, false),
    ] {
        assert_eq!(
            peer_admitted(peer, OWN, android),
            admitted,
            "peer {peer}, android {android}"
        );
    }
}
