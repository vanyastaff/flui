//! The development agent's service (ADR-0288) holds a whole conversation with no thread, no
//! socket and no clock of its own: the test hands it lines and instants, pumps the window's
//! owner, and reads the lines it transmits. A waker tells the transport when there is more to
//! do.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::task::Waker;
use std::time::Duration;

use flui_foundation::testing::CountingWaker;
use flui_interaction::events::Code;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform::PlatformInput;
use flui_protocol::native_recording::Line as NativeLine;
use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{
    ActParams, ActionName, ActionSequence, AppInfo, InputParams, InputSource, Locator,
    LogicalPoint, PointerAction, PointerButton, PointerDevice, Target, WindowId,
};
use flui_runtime::agent::{Limits, Refused, Service, ServiceConfig, Session, Status};
use flui_runtime::ui_runtime::{NativeForms, NativeLines};
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use flui_view::prelude::*;
use flui_view::{CloseReason, Signal};
use flui_widgets::{Column, RawButton, Text, column};
use serde_json::{Value, json};
use web_time::Instant;

const TOKEN: &str = "a-token-long-enough-for-the-launch-0123456789";

/// A button named "Go" that counts its presses, and the count shown, so a press commits a frame.
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
            RawButton::new(Text::new("Go")).on_press(move |cx| {
                presses.fetch_add(1, Ordering::SeqCst);
                count.update(cx, |n| *n += 1)
            }),
        ])
    }
}

/// Hands every window the host opens to the service, as a transport's hook does.
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

fn service(limits: Limits, on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>) -> Service {
    Service::new(ServiceConfig {
        records_native: false,
        token: TOKEN.to_owned(),
        app: AppInfo::new("counter", "headless"),
        limits,
        on_driver_gone,
    })
}

/// A host showing [`Counter`] whose window `service` serves.
fn shown(service: &Service) -> (HeadlessHost, HeadlessDevAgent, Arc<AtomicU32>) {
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

fn line(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("a JSON value serializes");
    bytes.push(b'\n');
    bytes
}

/// A hello presenting `token`; with `role`, asking for it.
fn hello_as(token: &str, role: Option<&str>) -> Vec<u8> {
    let mut hello =
        json!({ "token": token, "protocol": PROTOCOL_VERSION.to_string(), "client": "test" });
    if let Some(role) = role {
        hello["role"] = json!(role);
    }
    line(&json!({ "hello": hello }))
}

fn hello(token: &str) -> Vec<u8> {
    hello_as(token, None)
}

/// Every line the session has to transmit now.
fn transmitted(session: &mut Session) -> Vec<Value> {
    std::iter::from_fn(|| session.transmit())
        .map(|bytes| serde_json::from_slice::<Value>(&bytes).expect("each line is JSON"))
        .collect()
}

fn press_go(id: u64) -> Vec<u8> {
    let params = ActParams::new(Target::Locator(Locator::named("Go")), ActionName::Invoke);
    line(&json!({ "id": id, "method": "semantics.act", "params": params }))
}

/// A conversation past its hello, at `now`, asking to drive.
fn greeted(service: &Service, now: Instant, waker: &Waker) -> Session {
    greeted_as(service, now, waker, None).0
}

/// A conversation past a hello asking for `role`, at `now`, and the hello's answer.
fn greeted_as(
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

/// Requests run in order, one at a time, each answered once the window's owner drains it: the
/// owner's answer wakes the transport, and the next request starts only after the one before
/// answered, so an act followed by another sees the first one's effect.
#[test]
fn a_conversation_answers_in_order_as_the_owner_drains_and_wakes_its_transport() {
    let service = service(Limits::default(), None);
    let (mut host, _agent, presses) = shown(&service);
    let wakes = CountingWaker::new().shared();
    let waker = Waker::from(Arc::clone(&wakes));
    let now = Instant::now();
    let mut session = greeted(&service, now, &waker);

    let mut both = press_go(1);
    both.extend(press_go(2));
    session.receive(&both, now);
    session.poll(now, &waker);
    assert!(
        transmitted(&mut session).is_empty(),
        "nothing answers before the owner drains"
    );
    let woken = wakes.count();

    let _ = host.pump(Duration::ZERO);
    assert!(
        wakes.count() > woken,
        "the owner's answer woke the transport"
    );
    session.poll(now, &waker);
    let first = transmitted(&mut session);
    assert_eq!(
        first.len(),
        1,
        "the second request waits for the first: {first:?}"
    );
    assert_eq!(first[0]["id"], 1, "{first:?}");
    assert!(first[0].get("result").is_some(), "{first:?}");
    assert_eq!(presses.load(Ordering::SeqCst), 1);

    let _ = host.pump(Duration::from_millis(16));
    session.poll(now, &waker);
    let second = transmitted(&mut session);
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0]["id"], 2, "{second:?}");
    assert_eq!(
        presses.load(Ordering::SeqCst),
        2,
        "each act ran once, in order"
    );
}

/// A request the owner has not answered by its deadline answers `timeout`; an act still runs at
/// the owner's next drain, and says it may have.
#[test]
fn an_act_past_its_deadline_answers_timeout_and_still_runs() {
    let limits = Limits {
        reply_timeout: Duration::from_millis(100),
        ..Limits::default()
    };
    let service = service(limits, None);
    let (mut host, _agent, presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let mut session = greeted(&service, now, &waker);

    session.receive(&press_go(1), now);
    session.poll(now, &waker);
    let deadline = now + Duration::from_millis(100);
    assert_eq!(session.next_deadline(), Some(deadline));
    session.poll(deadline, &waker);
    let answer = transmitted(&mut session);
    assert_eq!(answer.len(), 1, "{answer:?}");
    assert_eq!(answer[0]["error"]["code"], "timeout", "{answer:?}");
    assert_eq!(answer[0]["error"]["retry"], "never", "{answer:?}");
    assert_eq!(
        answer[0]["error"]["effect"]["kind"], "may_have_run",
        "{answer:?}"
    );

    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the act ran at the next drain"
    );
    session.poll(deadline, &waker);
    assert!(
        transmitted(&mut session).is_empty(),
        "a request is answered once"
    );
}

/// A conversation that does not open with the right hello in time closes without a word.
#[test]
fn a_conversation_without_the_right_hello_in_time_closes_without_a_word() {
    let service = service(Limits::default(), None);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();

    let mut silent = service.accept(now).expect("a conversation");
    let due = now + Limits::default().handshake_timeout;
    assert_eq!(silent.next_deadline(), Some(due));
    silent.poll(due, &waker);
    assert_eq!(silent.status(), Status::Closed);
    assert!(silent.transmit().is_none());

    let mut wrong = service.accept(now).expect("a conversation");
    wrong.receive(&hello("not-the-token"), now);
    wrong.poll(now, &waker);
    assert_eq!(wrong.status(), Status::Closed);
    assert!(wrong.transmit().is_none());
}

/// The service takes as many conversations as its limit, one more once one ends, and none once
/// stopped; stopping wakes and closes the open ones.
#[test]
fn the_service_takes_conversations_up_to_its_limit_and_none_once_stopped() {
    let limits = Limits {
        max_connections: 2,
        ..Limits::default()
    };
    let service = service(limits, None);
    let wakes = CountingWaker::new().shared();
    let waker = Waker::from(Arc::clone(&wakes));
    let now = Instant::now();
    let first = greeted(&service, now, &waker);
    let mut second = service.accept(now).expect("a second conversation");
    second.poll(now, &waker);
    assert_eq!(service.accept(now).err(), Some(Refused::Full));
    drop(first);
    let mut third = service.accept(now).expect("the first one's place");
    third.poll(now, &waker);

    let woken = wakes.count();
    service.stop();
    assert!(
        wakes.count() >= woken + 2,
        "stopping woke both conversations"
    );
    for session in [&mut second, &mut third] {
        session.poll(now, &waker);
        assert_eq!(session.status(), Status::Closed);
    }
    assert_eq!(service.accept(now).err(), Some(Refused::Stopped));
}

/// The hello's answer to a conversation refused at its hello, and where that leaves it.
fn refused_hello(service: &Service, now: Instant, waker: &Waker, role: Option<&str>) -> Value {
    let mut session = service.accept(now).expect("a conversation");
    session.receive(&hello_as(TOKEN, role), now);
    session.poll(now, waker);
    let refusal = transmitted(&mut session);
    assert_eq!(refusal.len(), 1, "one line refuses the hello: {refusal:?}");
    assert!(refusal[0].get("hello").is_none(), "{refusal:?}");
    assert_ne!(session.status(), Status::Open, "{refusal:?}");
    refusal[0]["error"].clone()
}

/// One conversation drives the app at a time: a second driver's hello is refused `not_driver`
/// until the first goes. An observer is admitted beside the driver, told only the methods it
/// may call, and refused `not_driver` when it acts, which reaches the app no more.
#[test]
fn one_driver_at_a_time_and_an_observer_only_reads() {
    let service = service(Limits::default(), None);
    let (mut host, _agent, presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let (mut driver, welcome) = greeted_as(&service, now, &waker, None);
    let methods = &welcome["hello"]["capabilities"]["methods"];
    assert!(
        methods
            .as_array()
            .is_some_and(|methods| methods.contains(&json!("semantics.act"))),
        "the driver may act: {methods}"
    );

    let refused = refused_hello(&service, now, &waker, Some("driver"));
    assert_eq!(refused["code"], "not_driver", "{refused}");
    assert_eq!(refused["retry"], "soon", "{refused}");

    let (mut observer, welcome) = greeted_as(&service, now, &waker, Some("observer"));
    let methods = &welcome["hello"]["capabilities"]["methods"];
    assert!(
        methods.as_array().is_some_and(|methods| {
            methods.contains(&json!("semantics.read"))
                && methods.contains(&json!("events.subscribe"))
                && !methods.contains(&json!("semantics.act"))
        }),
        "an observer is told it reads and subscribes: {methods}"
    );
    observer.receive(&press_go(1), now);
    observer.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    observer.poll(now, &waker);
    let answer = transmitted(&mut observer);
    assert_eq!(answer.len(), 1, "{answer:?}");
    assert_eq!(answer[0]["id"], 1, "{answer:?}");
    assert_eq!(answer[0]["error"]["code"], "not_driver", "{answer:?}");
    assert_eq!(answer[0]["error"]["retry"], "never", "{answer:?}");
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "an observer's act never ran"
    );

    driver.receive(&press_go(2), now);
    driver.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    driver.poll(now, &waker);
    let answer = transmitted(&mut driver);
    assert!(
        answer[0].get("result").is_some(),
        "the driver acts: {answer:?}"
    );
    assert_eq!(presses.load(Ordering::SeqCst), 1);

    drop(observer);
    let refused = refused_hello(&service, now, &waker, None);
    assert_eq!(
        refused["code"], "not_driver",
        "an observer's end frees no seat: {refused}"
    );
    drop(driver);
    let (_next, _) = greeted_as(&service, now, &waker, None);
}

/// The driver's end runs the obligation, once, and an observer's does not; after it the app is
/// ending, and a driver's hello is answered `shutting_down`. A service that stopped runs it no
/// more.
#[test]
fn the_drivers_end_runs_its_obligation_once_and_not_after_a_stop() {
    let gone = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&gone);
    let service = service(
        Limits::default(),
        Some(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        })),
    );
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let driver = greeted(&service, now, &waker);
    let (observer, _) = greeted_as(&service, now, &waker, Some("observer"));
    drop(observer);
    assert_eq!(
        gone.load(Ordering::SeqCst),
        0,
        "an observer's end asks nothing"
    );
    drop(driver);
    assert_eq!(gone.load(Ordering::SeqCst), 1, "the driver's end asks once");
    let refused = refused_hello(&service, now, &waker, None);
    assert_eq!(refused["code"], "shutting_down", "{refused}");
    let (observer, _) = greeted_as(&service, now, &waker, Some("observer"));
    drop(observer);
    assert_eq!(gone.load(Ordering::SeqCst), 1, "no driver came after");

    let stopped_gone = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&stopped_gone);
    let stopped = service_with_obligation(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let driver = greeted(&stopped, now, &waker);
    stopped.stop();
    drop(driver);
    assert_eq!(
        stopped_gone.load(Ordering::SeqCst),
        0,
        "a driver let go by a stop is not gone"
    );
}

fn service_with_obligation(gone: impl Fn() + Send + Sync + 'static) -> Service {
    service(Limits::default(), Some(Box::new(gone)))
}

/// A subscription streams the window's events between answers, each one waking the transport.
#[test]
fn a_subscription_streams_the_windows_events_as_it_publishes_them() {
    let service = service(Limits::default(), None);
    let (mut host, _agent, _presses) = shown(&service);
    let wakes = CountingWaker::new().shared();
    let waker = Waker::from(Arc::clone(&wakes));
    let now = Instant::now();
    // It watches as an observer while the driver below acts.
    let (mut session, _) = greeted_as(&service, now, &waker, Some("observer"));

    let subscribe = json!({
        "id": 1,
        "method": "events.subscribe",
        "params": { "events": [{ "kind": "frame" }, { "kind": "semantics" }] },
    });
    session.receive(&line(&subscribe), now);
    session.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    session.poll(now, &waker);
    let subscribed = transmitted(&mut session);
    assert_eq!(subscribed.len(), 1, "{subscribed:?}");
    assert_eq!(subscribed[0]["result"]["subscription"], 1, "{subscribed:?}");

    // Another conversation's press commits a frame with a new tree: only the window's events
    // wake this one.
    let actor_waker = CountingWaker::new().shared().waker();
    let mut actor = greeted(&service, now, &actor_waker);
    actor.receive(&press_go(1), now);
    actor.poll(now, &actor_waker);
    let woken = wakes.count();
    let _ = host.pump(Duration::from_millis(16));
    let _ = host.pump(Duration::from_millis(16));
    assert!(
        wakes.count() > woken,
        "the window's events woke the transport"
    );
    session.poll(now, &waker);
    let events = transmitted(&mut session);
    for kind in ["frame", "semantics"] {
        assert!(
            events.iter().any(|event| event["event"] == kind),
            "a {kind} event follows the press's frame: {events:?}"
        );
    }
}

/// What a connection's injected input holds is the connection's: it stays down between calls,
/// and the connection's going lets it go, as a release would.
#[test]
fn input_a_connection_holds_is_let_go_when_it_goes() {
    let service = service(Limits::default(), None);
    let mut agent = HeadlessDevAgent::attach(Served(service.clone()));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200)).with_dev_agent(&mut agent);
    let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let log = |name: &'static str| {
        let seen = Arc::clone(&seen);
        move |_: &mut flui_view::EventCx<'_>, _: flui_interaction::PointerDispatch<'_>| {
            seen.lock().push(name);
        }
    };
    host.attach(
        &flui_widgets::Listener::new()
            .behavior(flui_rendering::hit_testing::HitTestBehavior::Opaque)
            .on_pointer_down(log("down"))
            .on_pointer_up(log("up"))
            .on_pointer_cancel(log("cancel"))
            .child(flui_widgets::SizedBox::new(200.0, 200.0)),
    )
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let (mut session, welcome) = greeted_as(&service, now, &waker, None);
    for method in ["input.inject", "input.release"] {
        assert!(
            welcome["hello"]["capabilities"]["methods"]
                .as_array()
                .is_some_and(|methods| methods.contains(&json!(method))),
            "the hello lists {method}: {welcome}"
        );
    }

    session.receive(
        &line(&json!({ "id": 1, "method": "windows.list", "params": {} })),
        now,
    );
    session.poll(now, &waker);
    let listed = transmitted(&mut session);
    let window = serde_json::from_value::<WindowId>(listed[0]["result"]["windows"][0].clone())
        .expect("the window's id");
    let press = InputParams::new(
        window,
        ActionSequence::new(vec![InputSource::Pointer {
            id: "finger".to_owned(),
            device: PointerDevice::Touch,
            actions: vec![
                PointerAction::Move {
                    to: LogicalPoint::new(50.0, 50.0),
                    duration_ms: 0,
                    pressure: None,
                },
                PointerAction::Down {
                    button: PointerButton::Primary,
                    pressure: None,
                },
            ],
        }]),
    );
    session.receive(
        &line(&json!({ "id": 2, "method": "input.inject", "params": press })),
        now,
    );
    let mut answer = Vec::new();
    for _ in 0..10 {
        session.poll(now, &waker);
        answer = transmitted(&mut session);
        if !answer.is_empty() {
            break;
        }
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(answer.len(), 1, "the press answers: {answer:?}");
    assert_eq!(answer[0]["result"]["via"], "synthetic", "{answer:?}");
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        *seen.lock(),
        ["down"],
        "the contact stays down between calls"
    );

    drop(session);
    let _ = host.pump(Duration::from_millis(16));
    assert_eq!(
        *seen.lock(),
        ["down", "cancel"],
        "the connection's going let its contact go"
    );
}

/// A key pressed on the window, one row of a recording.
fn key(code: Code) -> PlatformInput {
    PlatformInput::Keyboard(KeyEventBuilder::new(code).build())
}

/// Poll `session`, let the window's owner drain what it asked, poll again, and take the lines.
fn exchange(
    session: &mut Session,
    host: &mut HeadlessHost,
    now: Instant,
    waker: &Waker,
) -> Vec<Value> {
    session.poll(now, waker);
    let _ = host.pump(Duration::ZERO);
    session.poll(now, waker);
    transmitted(session)
}

/// Every line `session` sends until the answer to request `id`, polling as the transport
/// would once it has written what was there.
fn until_answered(session: &mut Session, id: u64, now: Instant, waker: &Waker) -> Vec<Value> {
    let mut lines = Vec::new();
    for _ in 0..1000 {
        session.poll(now, waker);
        lines.extend(transmitted(session));
        if lines.last().is_some_and(|line| line["id"] == id) {
            return lines;
        }
    }
    panic!("request {id} was not answered: {lines:?}");
}

/// Start recording `window` (the only one when `None`) on `session`: the answer, then the rows
/// recorded by the time it came (the frame that drained the request is one).
fn record(
    session: &mut Session,
    host: &mut HeadlessHost,
    window: Option<WindowId>,
    now: Instant,
    waker: &Waker,
) -> Vec<Value> {
    let params = flui_protocol::RecordParams::input(window);
    session.receive(
        &line(&json!({ "id": 1, "method": "record.start", "params": params })),
        now,
    );
    let started = exchange(session, host, now, waker);
    assert!(
        started.first().is_some_and(|line| line["id"] == 1)
            && started[1..].iter().all(|line| line["event"] == "record"),
        "the answer to record.start comes first, then its rows: {started:?}"
    );
    started
}

/// The rows of the `record` events among `lines`, in order.
fn rows_of(lines: &[Value]) -> Vec<&Value> {
    lines
        .iter()
        .filter(|line| line["event"] == "record")
        .map(|line| &line["params"]["row"])
        .collect()
}

/// How many of `rows` a recording counts as its own: every row but its `gap` and `end` rows.
fn counted(rows: &[&Value]) -> usize {
    rows.iter()
        .filter(|row| row.get("gap").is_none() && row.get("end").is_none())
        .count()
}

/// How many of `rows` are keys: the input rows that are not frames.
fn keys(rows: &[&Value]) -> usize {
    rows.iter()
        .filter(|row| {
            row.get("input")
                .is_some_and(|input| input["input"].get("frame").is_none())
        })
        .count()
}

/// A recording's rows go to the connection that started it, as `record` events after the
/// answer that carries its header, until `record.stop`: its `end` row goes out, then the answer
/// says what it came to, and the runtime records nothing more for it.
#[test]
fn a_recording_streams_the_runtimes_rows_to_its_connection_until_it_stops() {
    let service = service(Limits::default(), None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let (mut recorder, welcome) = greeted_as(&service, now, &waker, Some("observer"));
    assert_eq!(
        welcome["hello"]["capabilities"]["record"],
        json!(["input"]),
        "the hello says what is recorded: {welcome}"
    );
    let mut other = greeted_as(&service, now, &waker, Some("observer")).0;

    let mut lines = record(&mut recorder, &mut host, None, now, &waker);
    assert_eq!(lines[0]["result"]["recording"], 1, "{lines:?}");
    let header = &lines[0]["result"]["header"];
    assert_eq!(
        (
            &header["flui_recording"],
            &header["clock"],
            &header["keep_text"]
        ),
        (&json!(3), &json!("virtual"), &json!(false)),
        "{header}"
    );
    assert_eq!(
        header["windows"].as_array().map(Vec::len),
        Some(1),
        "{header}"
    );
    assert!(host.input_taps().is_recording());

    host.dispatch(key(Code::KeyA));
    host.dispatch(key(Code::KeyB));
    recorder.poll(now, &waker);
    let recorded = transmitted(&mut recorder);
    assert!(
        recorded
            .iter()
            .all(|line| line["event"] == "record" && line["params"]["recording"] == 1),
        "{recorded:?}"
    );
    assert_eq!(
        keys(&rows_of(&recorded)),
        2,
        "each key is a row: {recorded:?}"
    );
    lines.extend(recorded);
    other.poll(now, &waker);
    assert!(
        transmitted(&mut other).is_empty(),
        "another connection is sent none of its rows"
    );

    recorder.receive(
        &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    lines.extend(exchange(&mut recorder, &mut host, now, &waker));
    let (answer, lines) = lines.split_last().expect("record.stop answers");
    let rows = rows_of(lines);
    assert_eq!(
        rows.last(),
        Some(&&json!({ "end": { "lost_rows": 0 } })),
        "the end row goes out before the answer: {lines:?}"
    );
    assert_eq!(
        (&answer["id"], &answer["result"]),
        (
            &json!(2),
            &json!({ "rows": counted(&rows), "lost_rows": 0, "complete": true })
        ),
        "{lines:?}"
    );
    assert!(
        !host.input_taps().is_recording(),
        "the stop removed its tap"
    );
    host.dispatch(key(Code::KeyC));
    recorder.poll(now, &waker);
    assert!(
        transmitted(&mut recorder).is_empty(),
        "nothing after its end"
    );
}

/// A recording ends with its `end` row when the window it was started on closes and when its
/// runtime goes, and is held for `record.stop` to say what it came to; the connection that
/// started it going removes its tap with no row.
#[test]
fn a_recording_ends_with_its_window_or_runtime_and_goes_with_its_connection() {
    #[derive(Debug, Clone, Copy)]
    enum Ending {
        WindowCloses,
        RuntimeGoes,
    }
    for ending in [Ending::WindowCloses, Ending::RuntimeGoes] {
        let service = service(Limits::default(), None);
        let (mut host, mut agent, _presses) = shown(&service);
        let second = host.open_window(HeadlessWindow::new(120, 120));
        host.attach_to(
            second,
            &Counter {
                presses: Arc::new(AtomicU32::new(0)),
            },
        )
        .expect("mount the second window");
        host.serve_window(second, &mut agent);
        let _ = host.pump(Duration::from_millis(16));
        let waker = CountingWaker::new().shared().waker();
        let now = Instant::now();

        let mut gone = greeted_as(&service, now, &waker, Some("observer")).0;
        gone.receive(
            &line(&json!({ "id": 9, "method": "windows.list", "params": {} })),
            now,
        );
        gone.poll(now, &waker);
        let listed = transmitted(&mut gone);
        let windows =
            serde_json::from_value::<Vec<WindowId>>(listed[0]["result"]["windows"].clone())
                .expect("the windows' ids");
        let (first, opened) = match windows.as_slice() {
            [first, opened] => (*first, *opened),
            _ => panic!("both windows are served: {listed:?}"),
        };
        let _ = record(&mut gone, &mut host, Some(first), now, &waker);
        assert!(host.input_taps().is_recording(), "{ending:?}");
        drop(gone);
        let _ = host.pump(Duration::ZERO);
        assert!(
            !host.input_taps().is_recording(),
            "{ending:?}: the connection's going removed its tap"
        );

        let mut recorder = greeted_as(&service, now, &waker, Some("observer")).0;
        let mut lines = record(&mut recorder, &mut host, Some(opened), now, &waker);
        host.dispatch(key(Code::KeyA));
        match ending {
            Ending::WindowCloses => assert!(
                host.request_close(second, CloseReason::User),
                "the window closes"
            ),
            Ending::RuntimeGoes => {
                // The registry outlives the runtime in a clone the application holds: the
                // runtime ends its recordings itself as it goes.
                let registry = host.input_taps().clone();
                drop(host);
                drop(registry);
            }
        }
        recorder.receive(
            &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
            now,
        );
        lines.extend(until_answered(&mut recorder, 2, now, &waker));
        let (answer, lines) = lines.split_last().expect("record.stop answers");
        let rows = rows_of(lines);
        assert_eq!(keys(&rows), 1, "{ending:?}: {lines:?}");
        assert_eq!(
            rows.last(),
            Some(&&json!({ "end": { "lost_rows": 0 } })),
            "{ending:?}: {lines:?}"
        );
        assert_eq!(
            answer["result"],
            json!({ "rows": counted(&rows), "lost_rows": 0, "complete": true }),
            "{ending:?}: {lines:?}"
        );
    }
}

/// A reader that falls behind loses only its own rows: the ones its queue had no room for are
/// counted, a `gap` row says how many before the next row that fits, and the `end` row counts
/// them all.
#[test]
fn a_recording_counts_the_rows_its_reader_fell_behind_on() {
    const KEYS: usize = 1100;
    let service = service(Limits::default(), None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let mut recorder = greeted_as(&service, now, &waker, Some("observer")).0;
    let mut lines = record(&mut recorder, &mut host, None, now, &waker);

    // Nothing is read while the keys go in.
    for _ in 0..KEYS {
        host.dispatch(key(Code::KeyA));
    }
    recorder.receive(
        &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    recorder.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    lines.extend(until_answered(&mut recorder, 2, now, &waker));
    let (answer, lines) = lines.split_last().expect("record.stop answers");
    let rows = rows_of(lines);
    let lost = answer["result"]["lost_rows"]
        .as_u64()
        .expect("the answer counts what was lost");
    assert!(lost > 0, "the queue had no room for every key: {answer}");
    let gaps: Vec<&Value> = rows.iter().filter_map(|row| row.get("gap")).collect();
    assert_eq!(gaps, [&json!({ "rows": lost })], "one gap row names them");
    assert_eq!(rows.last(), Some(&&json!({ "end": { "lost_rows": lost } })));
    assert_eq!(
        answer["result"]["rows"].as_u64(),
        u64::try_from(counted(&rows)).ok(),
        "{answer}"
    );
    assert!(
        u64::try_from(keys(&rows)).expect("a count fits") + lost >= KEYS as u64,
        "every key is a row kept or counted lost: {answer}"
    );
}

/// A connection holds as many recordings as its limit, ended ones it has not stopped included,
/// and `record.stop` names only one of its own.
#[test]
fn a_connection_holds_its_recordings_up_to_its_limit_and_stops_only_its_own() {
    let limits = Limits {
        recordings: 2,
        ..Limits::default()
    };
    let service = service(limits, None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let mut recorder = greeted_as(&service, now, &waker, Some("observer")).0;
    let mut other = greeted_as(&service, now, &waker, Some("observer")).0;
    for number in 1..=2 {
        let started = record(&mut recorder, &mut host, None, now, &waker);
        assert_eq!(started[0]["result"]["recording"], number, "{started:?}");
    }
    recorder.receive(
        &line(&json!({ "id": 3, "method": "record.start", "params": { "layers": ["input"] } })),
        now,
    );
    let refused = until_answered(&mut recorder, 3, now, &waker);
    assert_eq!(
        refused.last().map(|line| &line["error"]["code"]),
        Some(&json!("busy")),
        "{refused:?}"
    );

    other.receive(
        &line(&json!({ "id": 1, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    let foreign = until_answered(&mut other, 1, now, &waker);
    assert_eq!(
        (
            &foreign[0]["error"]["code"],
            &foreign[0]["error"]["kind"],
            &foreign[0]["error"]["handle"]
        ),
        (&json!("unknown_handle"), &json!("recording"), &json!("1")),
        "{foreign:?}"
    );
    assert!(
        host.input_taps().is_recording(),
        "another connection's stop left the recording running"
    );
}

/// A service whose app journals its backend, so its hello lists the `native` layer.
fn native_service() -> Service {
    Service::new(ServiceConfig {
        records_native: true,
        token: TOKEN.to_owned(),
        app: AppInfo::new("counter", "headless"),
        limits: Limits::default(),
        on_driver_gone: None,
    })
}

/// A native line from its JSON.
fn native(line: Value) -> NativeLine {
    serde_json::from_value::<NativeLine>(line).expect("a native line")
}

/// A native header whose `provenance` says which form it is.
fn native_header(form: &str) -> NativeLine {
    native(json!({ "header": {
        "flui_native_recording": 1, "backend": "win32", "provenance": form,
    } }))
}

/// A native row; `notice` tells one apart from another.
fn native_row(notice: &str) -> NativeLine {
    native(json!({ "row": { "row": "notice", "notice": notice } }))
}

/// Send `kept` and `redacted` as the next line of `stream`, as a backend's journal does: an
/// ordinary line only in the forms the recordings want.
fn journal(
    lines: &NativeLines,
    stream: u32,
    setup: bool,
    kept: &NativeLine,
    redacted: &NativeLine,
) {
    let wants = lines.wants();
    lines.line(
        stream,
        setup,
        (setup || wants.kept).then_some(kept),
        (setup || wants.redacted).then_some(redacted),
    );
}

/// Start a recording of the input and native layers on `session`, as request `id`.
fn record_native(
    session: &mut Session,
    host: &mut HeadlessHost,
    id: u64,
    keep_text: bool,
    now: Instant,
    waker: &Waker,
) -> Vec<Value> {
    let params = json!({ "layers": ["input", "native"], "keep_text": keep_text });
    session.receive(
        &line(&json!({ "id": id, "method": "record.start", "params": params })),
        now,
    );
    exchange(session, host, now, waker)
}

/// The `(stream, line)` of each `record_native` event among `lines`, in order.
fn native_of(lines: &[Value]) -> Vec<(u64, &Value)> {
    lines
        .iter()
        .filter(|line| line["event"] == "record_native")
        .map(|line| {
            (
                line["params"]["stream"].as_u64().expect("a stream number"),
                &line["params"]["line"],
            )
        })
        .collect()
}

/// A recording of the `native` layer is sent each stream's lines as `record_native` events in
/// the form its privacy asks for. One that starts after a stream opened begins it with the
/// stream's header and setup lines kept for it; a row the backend could not journal is a `gap`
/// line of its stream before the next one, counted by `record.stop`; nobody's form is made once
/// no recording wants it.
#[test]
fn a_native_recording_begins_each_stream_with_its_setup_and_streams_its_lines() {
    let service = native_service();
    let (mut host, _agent, _presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let journaled = host.attach_native_journal();
    assert_eq!(
        journaled.wants(),
        NativeForms::default(),
        "no recording wants a row"
    );
    journal(
        &journaled,
        0,
        true,
        &native_header("kept"),
        &native_header("redacted"),
    );
    journal(
        &journaled,
        0,
        true,
        &native_row("session_locked"),
        &native_row("session_unlocked"),
    );
    journal(
        &journaled,
        0,
        false,
        &native_row("sleep"),
        &native_row("wake"),
    );
    // A stream keeps its last 64 setup lines: one more lets the first go.
    journal(
        &journaled,
        1,
        true,
        &native_header("kept"),
        &native_header("redacted"),
    );
    for _ in 0..=64 {
        journal(
            &journaled,
            1,
            true,
            &native_row("session_locked"),
            &native_row("session_unlocked"),
        );
    }

    let (mut redacting, welcome) = greeted_as(&service, now, &waker, Some("observer"));
    assert_eq!(
        welcome["hello"]["capabilities"]["record"],
        json!(["input", "native"]),
        "{welcome}"
    );
    let mut keeping = greeted_as(&service, now, &waker, Some("observer")).0;
    let mut started = record_native(&mut redacting, &mut host, 1, false, now, &waker);
    // The rest of the prefix goes out as the transport writes what was there.
    redacting.poll(now, &waker);
    started.extend(transmitted(&mut redacting));
    assert_eq!(started[0]["result"]["recording"], 1, "{started:?}");
    assert_eq!(
        native_of(&started)
            .into_iter()
            .filter(|(stream, _)| *stream == 0)
            .collect::<Vec<_>>(),
        [
            (
                0,
                &json!({ "header": serde_json::to_value(native_header("redacted")).expect("json")["header"] })
            ),
            (
                0,
                &json!({ "row": { "row": "notice", "notice": "session_unlocked" } })
            ),
        ],
        "a stream begins with its header and setup, in the recording's form: {started:?}"
    );
    let second: Vec<&Value> = native_of(&started)
        .into_iter()
        .filter(|(stream, _)| *stream == 1)
        .map(|(_, line)| line)
        .collect();
    assert_eq!(
        (second.len(), second.get(1)),
        (2 + 64, Some(&&json!({ "gap": { "rows": 1 } }))),
        "a stream whose setup outgrew what it keeps begins with a gap after its header: {second:?}"
    );
    let kept = record_native(&mut keeping, &mut host, 1, true, now, &waker);
    assert_eq!(
        native_of(&kept)
            .iter()
            .filter(|(stream, _)| *stream == 0)
            .map(|(_, line)| line.to_string())
            .collect::<Vec<_>>(),
        [
            serde_json::to_value(native_header("kept"))
                .expect("json")
                .to_string(),
            json!({ "row": { "row": "notice", "notice": "session_locked" } }).to_string(),
        ],
        "{kept:?}"
    );
    let wants = journaled.wants();
    assert!(
        wants.kept && wants.redacted,
        "both forms are wanted: {wants:?}"
    );
    journal(
        &journaled,
        0,
        false,
        &native_row("sleep"),
        &native_row("wake"),
    );
    journaled.lost(0);
    journal(
        &journaled,
        0,
        false,
        &native_row("sleep"),
        &native_row("wake"),
    );
    redacting.poll(now, &waker);
    let lines = transmitted(&mut redacting);
    assert_eq!(
        native_of(&lines),
        [
            (0, &json!({ "row": { "row": "notice", "notice": "wake" } })),
            (0, &json!({ "gap": { "rows": 1 } })),
            (0, &json!({ "row": { "row": "notice", "notice": "wake" } })),
        ],
        "{lines:?}"
    );

    redacting.receive(
        &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    let stopped = exchange(&mut redacting, &mut host, now, &waker);
    assert_eq!(
        stopped.last().map(|line| {
            let result = &line["result"];
            (
                &result["lost_rows"],
                &result["native_lost_rows"],
                &result["complete"],
            )
        }),
        // The setup line stream 1 let go before the recording began, and the row lost.
        Some((&json!(0), &json!(2), &json!(true))),
        "{stopped:?}"
    );
    keeping.receive(
        &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    let _ = exchange(&mut keeping, &mut host, now, &waker);
    assert_eq!(
        journaled.wants(),
        NativeForms::default(),
        "a stopped recording wants nothing"
    );
}

/// The native layer has a share of a recording's queue and the input the rest: a backend that
/// journals more than its reader takes loses native rows, counted per stream, never input.
#[test]
fn a_native_stream_that_floods_a_recording_loses_its_own_rows_not_the_input() {
    const FLOOD: usize = 1500;
    const KEYS: usize = 200;
    let service = native_service();
    let (mut host, _agent, _presses) = shown(&service);
    let waker = CountingWaker::new().shared().waker();
    let now = Instant::now();
    let journaled = host.attach_native_journal();
    let mut recorder = greeted_as(&service, now, &waker, Some("observer")).0;
    let mut lines = record_native(&mut recorder, &mut host, 1, false, now, &waker);

    // Nothing is read while the backend and the keys go in.
    for stream in [0, 1] {
        journal(
            &journaled,
            stream,
            true,
            &native_header("kept"),
            &native_header("redacted"),
        );
    }
    for _ in 0..FLOOD {
        journal(
            &journaled,
            1,
            false,
            &native_row("sleep"),
            &native_row("wake"),
        );
    }
    for _ in 0..KEYS {
        host.dispatch(key(Code::KeyA));
    }
    recorder.receive(
        &line(&json!({ "id": 2, "method": "record.stop", "params": { "recording": 1 } })),
        now,
    );
    recorder.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    lines.extend(until_answered(&mut recorder, 2, now, &waker));
    let (answer, lines) = lines.split_last().expect("record.stop answers");
    assert_eq!(keys(&rows_of(lines)), KEYS, "every key is kept: {answer}");
    assert_eq!(answer["result"]["lost_rows"], 0, "{answer}");
    let native = native_of(lines);
    let lost = answer["result"]["native_lost_rows"]
        .as_u64()
        .expect("the answer counts the native rows lost");
    let rows = native
        .iter()
        .filter(|(stream, line)| *stream == 1 && line.get("row").is_some())
        .count();
    assert!(
        lost > 0,
        "the native share had no room for the flood: {answer}"
    );
    assert_eq!(
        rows as u64 + lost,
        FLOOD as u64,
        "each native row is sent or counted lost: {answer}"
    );
    assert_eq!(
        native
            .iter()
            .filter(|(_, line)| line.get("gap").is_some())
            .map(|(stream, line)| (*stream, line["gap"]["rows"].as_u64()))
            .collect::<Vec<_>>(),
        [(1, Some(lost))],
        "only the flooding stream has a gap, before the end: {lines:?}"
    );
}

/// A recording of `native` records the input too, and is refused `not_supported` by an app
/// whose backend journals nothing, which its hello does not list.
#[test]
fn a_native_recording_is_refused_without_the_input_or_a_journal() {
    struct Case {
        name: &'static str,
        records_native: bool,
        attached: bool,
        layers: Value,
        code: &'static str,
    }
    let cases = [
        Case {
            name: "native alone",
            records_native: true,
            attached: true,
            layers: json!(["native"]),
            code: "invalid_argument",
        },
        Case {
            name: "no journal attached",
            records_native: true,
            attached: false,
            layers: json!(["input", "native"]),
            code: "not_supported",
        },
        Case {
            name: "an app that journals nothing",
            records_native: false,
            attached: false,
            layers: json!(["input", "native"]),
            code: "not_supported",
        },
    ];
    for case in cases {
        let service = if case.records_native {
            native_service()
        } else {
            service(Limits::default(), None)
        };
        let (mut host, _agent, _presses) = shown(&service);
        let _journal = case.attached.then(|| host.attach_native_journal());
        let waker = CountingWaker::new().shared().waker();
        let now = Instant::now();
        let mut recorder = greeted_as(&service, now, &waker, Some("observer")).0;
        recorder.receive(
            &line(&json!({
                "id": 1, "method": "record.start", "params": { "layers": case.layers },
            })),
            now,
        );
        let answered = exchange(&mut recorder, &mut host, now, &waker);
        assert_eq!(
            answered.last().map(|line| &line["error"]["code"]),
            Some(&json!(case.code)),
            "{}: {answered:?}",
            case.name
        );
        assert!(!host.input_taps().is_recording(), "{}", case.name);
    }
}
