//! A conversation with the development agent's service (ADR-0288) over a window showing a
//! counter, and what the service owes bytes nobody vouches for. `contracts/agent_service.rs`
//! holds its conversations with these; its property test draws a client's chunks at random on
//! every run, and the fuzz target `fuzz/agent_wire.rs` lets libFuzzer mutate them toward code
//! no input has reached yet.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Waker;
use std::time::Duration;

use flui_foundation::testing::CountingWaker;
use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{AppInfo, Greeting, Request, RequestId, ServerLine};
use flui_runtime::agent::{Limits, Service, ServiceConfig, Session};
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::Signal;
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use flui_view::prelude::*;
use flui_widgets::{Column, RawButton, Text};
use serde_json::{Value, json};
use web_time::Instant;

pub(crate) const TOKEN: &str = "a-token-long-enough-for-the-launch-0123456789";

/// A button named "Go" that counts its presses, and the count shown, so a press commits a frame.
#[derive(Clone, StatefulView)]
pub(crate) struct Counter {
    pub(crate) presses: Arc<AtomicU32>,
}

pub(crate) struct CounterState {
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
            RawButton::new(Text::new("Go")).on_pressed(move |ctx| {
                presses.fetch_add(1, Ordering::SeqCst);
                count.update(ctx, |n| *n += 1)
            }),
        ))
    }
}

/// Hands every window the host opens to the service, as a transport's hook does.
pub(crate) struct Served(pub(crate) Service);

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

pub(crate) fn service(
    limits: Limits,
    on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>,
) -> Service {
    Service::new(ServiceConfig {
        records_native: false,
        token: TOKEN.to_owned(),
        app: AppInfo::new("counter", "headless"),
        limits,
        on_driver_gone,
    })
}

/// A host showing [`Counter`] whose window `service` serves.
pub(crate) fn shown(service: &Service) -> (HeadlessHost, HeadlessDevAgent, Arc<AtomicU32>) {
    let mut agent = HeadlessDevAgent::attach(Served(service.clone()));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200)).with_dev_agent(&mut agent);
    let presses = Arc::new(AtomicU32::new(0));
    host.attach(&Counter {
        presses: Arc::clone(&presses),
    })
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    (host, agent, presses)
}

pub(crate) fn line(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("a JSON value serializes");
    bytes.push(b'\n');
    bytes
}

/// A hello presenting `token`; with `role`, asking for it.
pub(crate) fn hello_as(token: &str, role: Option<&str>) -> Vec<u8> {
    let mut hello =
        json!({ "token": token, "protocol": PROTOCOL_VERSION.to_string(), "client": "test" });
    if let Some(role) = role {
        hello["role"] = json!(role);
    }
    line(&json!({ "hello": hello }))
}

/// Every line the session has to transmit now.
pub(crate) fn transmitted(session: &mut Session) -> Vec<Value> {
    std::iter::from_fn(|| session.transmit())
        .map(|bytes| serde_json::from_slice::<Value>(&bytes).expect("each line is JSON"))
        .collect()
}

/// A conversation past its hello, at `now`, asking to drive.
pub(crate) fn greeted(service: &Service, now: Instant, waker: &Waker) -> Session {
    greeted_as(service, now, waker, None).0
}

/// A conversation past a hello asking for `role`, at `now`, and the hello's answer.
pub(crate) fn greeted_as(
    service: &Service,
    now: Instant,
    waker: &Waker,
    role: Option<&str>,
) -> (Session, Value) {
    let mut session = service
        .accept(now)
        .expect("the service takes a conversation");
    session.receive(&hello_as(TOKEN, role), now);
    session.poll(now, waker);
    let mut welcome = transmitted(&mut session);
    assert_eq!(welcome.len(), 1, "one line answers the hello: {welcome:?}");
    assert_eq!(
        welcome[0]["hello"]["protocol"],
        PROTOCOL_VERSION.to_string(),
        "{welcome:?}"
    );
    (session, welcome.remove(0))
}

/// The handle of the one window `session` can name, asked for as a client does.
fn listed_window(
    session: &mut Session,
    host: &mut HeadlessHost,
    now: Instant,
    waker: &Waker,
) -> String {
    session.receive(
        &line(&json!({ "id": "window", "method": "windows.list", "params": {} })),
        now,
    );
    session.poll(now, waker);
    let _ = host.pump(Duration::ZERO);
    session.poll(now, waker);
    let listed = transmitted(session);
    listed
        .first()
        .and_then(|answer| answer["result"]["windows"][0].as_str())
        .unwrap_or_else(|| panic!("windows.list names the window: {listed:?}"))
        .to_owned()
}

/// Whatever a client writes, before its hello (`greet` off) or after it, the session never
/// panics and keeps four promises, and this panics where one of them fails:
/// - every line it writes is one whole line of the protocol;
/// - a line that names a request names one the client sent and that no earlier line
///   answered;
/// - besides the events of a subscription, it has never written more lines than it has
///   received;
/// - it writes nothing to a client whose first line is not a hello that holds the token.
///
/// Each promise is checked as the lines are written, against what the client had sent by
/// then, not once at the end.
///
/// `chunks` is what the client's transport delivers. A greeted client has listed the windows,
/// as every client does before it names one, and `chunks` receives the window's handle: a
/// handle is issued per conversation, so no input can hold it in advance.
pub(crate) fn client_bytes_are_answered_or_refused_in_whole_lines(
    greet: bool,
    chunks: impl FnOnce(Option<&str>) -> Vec<Vec<u8>>,
) {
    let service = service(Limits::default(), None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let now = Instant::now();
    let (mut session, window) = if greet {
        let mut session = greeted(&service, now, &waker);
        let window = listed_window(&mut session, &mut host, now, &waker);
        (session, Some(window))
    } else {
        let session = service
            .accept(now)
            .expect("the service takes a conversation");
        (session, None)
    };
    let mut received = Vec::<u8>::new();
    // Where the first line not read yet starts in `received`.
    let mut unread = 0_usize;
    // Whether the client presented the token where the session reads it: in a hello, as
    // its first line.
    let mut presented = greet;
    let mut lines = 0_usize;
    // The id of each request received and not answered yet.
    let mut asked = Vec::<RequestId>::new();
    let mut answers = 0_usize;
    for chunk in chunks(window.as_deref()) {
        received.extend_from_slice(&chunk);
        while let Some(end) = received
            .get(unread..)
            .and_then(|rest| rest.iter().position(|byte| *byte == b'\n'))
        {
            let line = received.get(unread..unread + end).unwrap_or_default();
            if lines == 0 {
                presented = greet
                    || serde_json::from_slice::<Greeting>(line)
                        .is_ok_and(|greeting| greeting.hello.token == TOKEN);
            }
            asked.extend(asked_id(line));
            lines += 1;
            unread += end + 1;
        }
        session.receive(&chunk, now);
        session.poll(now, &waker);
        let _ = host.pump(Duration::from_millis(16));
        session.poll(now, &waker);
        while let Some(bytes) = session.transmit() {
            let written = String::from_utf8_lossy(&bytes);
            assert!(presented, "written with no token presented: {written:?}");
            assert_eq!(
                bytes.iter().position(|byte| *byte == b'\n'),
                Some(bytes.len() - 1),
                "{written:?}"
            );
            let answered = match serde_json::from_slice::<ServerLine<Value>>(&bytes) {
                Ok(ServerLine::Event(_)) => continue,
                Ok(ServerLine::Answered { id, .. }) => Some(id),
                Ok(ServerLine::Failed { id, .. }) => id,
                Ok(_) => None,
                Err(error) => panic!("{error}: {written:?}"),
            };
            answers += 1;
            assert!(
                answers <= lines,
                "line {answers} written after {lines} received: {written:?}"
            );
            if let Some(id) = answered {
                let request = asked
                    .iter()
                    .position(|sent| *sent == id)
                    .unwrap_or_else(|| panic!("no unanswered request has this id: {written:?}"));
                let _ = asked.swap_remove(request);
            }
        }
    }
}

/// The id `line` asks under, read as the session reads a line: as a request of the protocol,
/// whose reader skips a field it does not know without reading it (so bytes there that are no
/// UTF-8 do not spoil the request), or, where the line is no request, as JSON with an `id`.
fn asked_id(line: &[u8]) -> Option<RequestId> {
    if let Ok(request) = serde_json::from_slice::<Request<Value>>(line) {
        return Some(request.id);
    }
    let value = serde_json::from_slice::<Value>(line).ok()?;
    serde_json::from_value::<RequestId>(value.get("id")?.clone()).ok()
}
