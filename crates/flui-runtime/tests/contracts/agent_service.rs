//! The development agent's service (ADR-0288) holds a whole conversation with no thread, no
//! socket and no clock of its own: the test hands it lines and instants, pumps the window's
//! owner, and reads the lines it transmits. A waker tells the transport when there is more to
//! do.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::task::{Wake, Waker};
use std::time::Duration;

use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{ActParams, ActionName, AppInfo, Locator, Target};
use flui_runtime::agent::{Limits, Refused, Service, ServiceConfig, Session, Status};
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::Signal;
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use flui_view::prelude::*;
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

/// Counts its wakes: the transport's signal that the session has more to do.
#[derive(Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn service(limits: Limits, on_driver_gone: Option<Box<dyn Fn() + Send + Sync>>) -> Service {
    Service::new(ServiceConfig {
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
    let wakes = Arc::new(Wakes::default());
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
    let woken = wakes.0.load(Ordering::SeqCst);

    let _ = host.pump(Duration::ZERO);
    assert!(
        wakes.0.load(Ordering::SeqCst) > woken,
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
    let waker = Waker::from(Arc::new(Wakes::default()));
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
    let waker = Waker::from(Arc::new(Wakes::default()));
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
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(Arc::clone(&wakes));
    let now = Instant::now();
    let first = greeted(&service, now, &waker);
    let mut second = service.accept(now).expect("a second conversation");
    second.poll(now, &waker);
    assert_eq!(service.accept(now).err(), Some(Refused::Full));
    drop(first);
    let mut third = service.accept(now).expect("the first one's place");
    third.poll(now, &waker);

    let woken = wakes.0.load(Ordering::SeqCst);
    service.stop();
    assert!(
        wakes.0.load(Ordering::SeqCst) >= woken + 2,
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
    let waker = Waker::from(Arc::new(Wakes::default()));
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
    let waker = Waker::from(Arc::new(Wakes::default()));
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
    let wakes = Arc::new(Wakes::default());
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
    let actor_waker = Waker::from(Arc::new(Wakes::default()));
    let mut actor = greeted(&service, now, &actor_waker);
    actor.receive(&press_go(1), now);
    actor.poll(now, &actor_waker);
    let woken = wakes.0.load(Ordering::SeqCst);
    let _ = host.pump(Duration::from_millis(16));
    let _ = host.pump(Duration::from_millis(16));
    assert!(
        wakes.0.load(Ordering::SeqCst) > woken,
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
