//! The page transport's conversation on the test's own thread, as on a page: the test is the
//! page's event loop, which polls a connection's task when its waker was woken and pumps the
//! window's owner, and the script's end records what reached its `onLine`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Waker};
use std::time::Duration;

use flui_foundation::testing::CountingWaker;
use flui_protocol::AppInfo;
use flui_protocol::version::PROTOCOL_VERSION;
use flui_runtime::agent::{Limits, Service, ServiceConfig};
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use flui_widgets::Text;
use serde_json::{Value, json};
use web_time::Instant;

use super::{Conversation, Peer, converse};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-page";

/// How long a scenario may wait for a line before it fails instead of hanging.
const CAP: Duration = Duration::from_secs(10);

/// Hands every window the host opens to the service, as the page's hook does.
struct Served(Service);

impl DevAgentHook for Served {
    fn attach(&mut self) -> bool {
        true
    }

    fn detach(&mut self) {
        self.0.stop();
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.window_opened(window);
    }
}

/// The script's end of a connection: what its `onLine` received, whether it still listens,
/// whether the application ended the connection, and the deadline it was asked to keep.
#[derive(Default)]
struct Script {
    lines: RefCell<Vec<Value>>,
    deaf: Cell<bool>,
    ended: Cell<bool>,
    deadline: Cell<Option<Instant>>,
    no_timer: Cell<bool>,
}

impl Peer for Rc<Script> {
    fn send(&self, line: &str) -> bool {
        if self.deaf.get() {
            return false;
        }
        let line = serde_json::from_str(line).expect("each line is one JSON value");
        self.lines.borrow_mut().push(line);
        true
    }

    fn close(&self) {
        self.ended.set(true);
    }

    fn wake_at(&self, deadline: Option<Instant>) -> bool {
        self.deadline.set(deadline);
        !self.no_timer.get()
    }
}

/// A connection: its conversation, the script's end, and the task the page's executor runs.
struct Connection {
    conversation: Rc<Conversation>,
    script: Rc<Script>,
    task: Option<Pin<Box<dyn Future<Output = ()>>>>,
    wakes: Arc<CountingWaker>,
}

impl Connection {
    fn accept(service: &Service) -> Self {
        let session = service
            .accept(Instant::now())
            .expect("the service takes a conversation");
        let conversation = Conversation::new(session);
        let script = Rc::new(Script::default());
        let task = Box::pin(converse(Rc::clone(&conversation), Rc::clone(&script)));
        let mut connection = Self {
            conversation,
            script,
            task: Some(task),
            wakes: CountingWaker::new().shared(),
        };
        // A spawned task runs once.
        connection.poll();
        connection
    }

    /// Poll the task as the executor does; whether it has finished.
    fn poll(&mut self) -> bool {
        let Some(task) = self.task.as_mut() else {
            return true;
        };
        let waker = Waker::from(Arc::clone(&self.wakes));
        if task
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_ready()
        {
            self.task = None;
        }
        self.task.is_none()
    }

    fn send(&mut self, line: &Value) {
        let line = serde_json::to_string(line).expect("a JSON value serializes");
        self.conversation.receive(&line);
    }

    /// Pump `host` and poll the task until the script holds a line `found` accepts.
    fn line_where(
        &mut self,
        host: &mut HeadlessHost,
        found: impl Fn(&Value) -> bool,
        what: &str,
    ) -> Value {
        let cap = Instant::now() + CAP;
        loop {
            self.poll();
            if let Some(line) = self.script.lines.borrow().iter().find(|line| found(line)) {
                return line.clone();
            }
            assert!(Instant::now() < cap, "{what} never reached the script");
            let _ = host.pump(Duration::from_millis(16));
        }
    }

    fn greet(&mut self, host: &mut HeadlessHost) -> Value {
        self.send(&json!({
            "hello": { "token": TOKEN, "protocol": PROTOCOL_VERSION.to_string(), "client": "page" }
        }));
        self.line_where(
            host,
            |line| line.get("hello").is_some(),
            "the hello's answer",
        )
    }
}

/// A service that counts its driver's goings, under `limits`.
fn service(limits: Limits) -> (Service, Arc<AtomicUsize>) {
    let gone = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&gone);
    let service = Service::new(ServiceConfig {
        records_native: false,
        token: TOKEN.to_owned(),
        app: AppInfo::new("page", "web"),
        limits,
        on_driver_gone: Some(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })),
    });
    (service, gone)
}

/// The page's one window, served by `service`.
fn page(service: &Service) -> (HeadlessHost, HeadlessDevAgent) {
    let mut agent = HeadlessDevAgent::attach(Served(service.clone()));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200)).with_dev_agent(&mut agent);
    host.attach(&Text::new("Hello")).expect("the page mounts");
    let _ = host.pump(Duration::from_millis(16));
    (host, agent)
}

/// A conversation over the page transport greets, answers what the script sends in order as
/// the owner drains, and ends when the script closes it: the session is dropped, so the driver
/// goes.
#[test]
fn a_page_conversation_answers_its_script_and_ends_when_the_script_closes_it() {
    let (service, gone) = service(Limits::default());
    let (mut host, _agent) = page(&service);
    let mut connection = Connection::accept(&service);

    let welcome = connection.greet(&mut host);
    assert_eq!(welcome["hello"]["app"]["target"], "web", "{welcome}");
    let woken = connection.wakes.count();
    connection.send(&json!({ "id": 1, "method": "windows.list", "params": {} }));
    connection.send(&json!({ "id": 2, "method": "windows.list", "params": {} }));
    assert!(
        connection.wakes.count() > woken,
        "a line the script sends wakes the task"
    );
    let second = connection.line_where(&mut host, |line| line["id"] == 2, "the second answer");
    let ids: Vec<Value> = connection
        .script
        .lines
        .borrow()
        .iter()
        .map(|line| line["id"].clone())
        .collect();
    assert_eq!(
        ids,
        [Value::Null, json!(1), json!(2)],
        "answers come in order"
    );
    assert_eq!(
        second["result"]["windows"].as_array().map(Vec::len),
        Some(1),
        "the page's one window is served: {second}"
    );

    connection.conversation.peer_closed();
    assert!(connection.poll(), "the task ends with its connection");
    assert_eq!(gone.load(Ordering::SeqCst), 1, "the driver went");
    assert!(connection.conversation.session.borrow().is_none());
}

/// A script that stops listening ends its conversation at the next line it misses, as a
/// conversation whose deadline no timer can keep ends at once; neither leaves the session
/// behind.
#[test]
fn a_page_conversation_ends_when_its_script_stops_listening_or_no_timer_keeps_its_deadline() {
    let (service, gone) = service(Limits::default());
    let (mut host, _agent) = page(&service);

    let mut deaf = Connection::accept(&service);
    deaf.greet(&mut host);
    deaf.script.deaf.set(true);
    deaf.send(&json!({ "id": 1, "method": "windows.list", "params": {} }));
    let cap = Instant::now() + CAP;
    while !deaf.poll() {
        assert!(Instant::now() < cap, "the conversation outlived its script");
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        deaf.script.ended.get(),
        "the application ends the connection"
    );
    assert_eq!(gone.load(Ordering::SeqCst), 1, "the driver went with it");

    let untimed = {
        let session = service
            .accept(Instant::now())
            .expect("the service takes a conversation");
        let conversation = Conversation::new(session);
        let script = Rc::new(Script::default());
        script.no_timer.set(true);
        let mut connection = Connection {
            task: Some(Box::pin(converse(
                Rc::clone(&conversation),
                Rc::clone(&script),
            ))),
            conversation,
            script,
            wakes: CountingWaker::new().shared(),
        };
        assert!(
            connection.poll(),
            "a deadline no timer keeps ends it at once"
        );
        connection
    };
    assert!(untimed.script.ended.get());
    assert!(
        untimed.script.lines.borrow().is_empty(),
        "closed without a word"
    );
    assert!(untimed.conversation.session.borrow().is_none());
}

/// The session's deadline is kept through the page's timer: a conversation that never says
/// hello is asked to wake at its handshake's end, and closes without a word once woken there.
#[test]
fn a_page_conversation_keeps_its_session_deadline_through_the_timer() {
    let (service, gone) = service(Limits {
        handshake_timeout: Duration::from_millis(20),
        ..Limits::default()
    });
    let mut connection = Connection::accept(&service);
    let deadline = connection
        .script
        .deadline
        .get()
        .expect("the handshake's deadline is kept");
    assert!(!connection.poll(), "it waits for the hello");

    let left = deadline.saturating_duration_since(Instant::now());
    std::thread::sleep(left + Duration::from_millis(5));
    // What the timer does when the deadline passes.
    connection.conversation.wake();
    assert!(connection.poll(), "the handshake timed out");
    assert!(connection.script.ended.get());
    assert!(
        connection.script.lines.borrow().is_empty(),
        "closed without a word"
    );
    assert_eq!(gone.load(Ordering::SeqCst), 0, "no driver was admitted");
}

/// A line that arrives while the session runs (sent by code the session ran, such as a trace's
/// subscriber) cannot be handed to it: the conversation closes as for a client that broke it,
/// rather than losing the line or panicking on the borrow.
#[test]
fn a_line_sent_while_the_session_runs_closes_the_conversation() {
    let (service, _gone) = service(Limits::default());
    let mut connection = Connection::accept(&service);
    {
        let _running = connection.conversation.session.borrow_mut();
        connection.conversation.receive(r#"{"hello":{}}"#);
    }
    assert!(connection.poll(), "the conversation closed");
    assert!(connection.script.ended.get());
    assert!(connection.script.lines.borrow().is_empty());
}

/// When the agent stops, the export's end closes every conversation: the driver's conversation
/// ends without its going running, since the app is ending.
#[test]
fn a_stopped_service_ends_its_page_conversations_without_the_driver_going() {
    let (service, gone) = service(Limits::default());
    let (mut host, _agent) = page(&service);
    let mut connection = Connection::accept(&service);
    connection.greet(&mut host);

    service.stop();
    connection.conversation.peer_closed();
    assert!(connection.poll(), "the conversation ended");
    assert_eq!(gone.load(Ordering::SeqCst), 0);
}
