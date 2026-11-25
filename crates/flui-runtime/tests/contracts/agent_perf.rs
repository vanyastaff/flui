//! `perf.start` and `perf.stop` over the agent's wire (ADR-0400 §2, §5): a span is the
//! connection's that started it, through the connection's lease on the window, so a driver and
//! an observer each measure their own, neither ends the other's, and a span ends with its
//! connection.

use std::task::Waker;
use std::time::Duration;

use flui_foundation::testing::CountingWaker;
use flui_protocol::method::{self, Method};
use flui_protocol::{act::ActParams, locator::Locator, locator::Target, wire::ActionName};
use flui_runtime::agent::{Limits, Session};
use flui_testing::host::HeadlessHost;
use serde_json::{Value, json};
use web_time::Instant;

use crate::agent_wire::{greeted, greeted_as, line, service, shown, transmitted};

/// How many pumps a call is given to be answered.
const PUMPS: usize = 200;

/// One client's conversation and the window handle it was issued.
struct Client {
    session: Session,
    window: Value,
    next_id: u64,
}

impl Client {
    /// A conversation admitted as `role` (the driver when `None`) that has listed the window.
    fn connect(
        service: &flui_runtime::agent::Service,
        host: &mut HeadlessHost,
        waker: &Waker,
        role: Option<&str>,
    ) -> Self {
        let now = Instant::now();
        let session = match role {
            None => greeted(service, now, waker),
            Some(role) => greeted_as(service, now, waker, Some(role)).0,
        };
        let mut client = Self {
            session,
            window: Value::Null,
            next_id: 0,
        };
        let listed = client.call(host, waker, "windows.list", json!({}));
        client.window = listed["result"]["windows"][0].clone();
        assert!(client.window.is_string(), "a window is listed: {listed}");
        client
    }

    /// Asks `method` with `params` and pumps the host until the answer comes.
    fn call(
        &mut self,
        host: &mut HeadlessHost,
        waker: &Waker,
        method: &str,
        params: Value,
    ) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let now = Instant::now();
        self.session.receive(
            &line(&json!({ "id": id, "method": method, "params": params })),
            now,
        );
        self.session.poll(now, waker);
        for _ in 0..PUMPS {
            if let Some(answer) = transmitted(&mut self.session)
                .into_iter()
                .find(|line| line["id"] == json!(id))
            {
                return answer;
            }
            let _ = host.pump(Duration::ZERO);
            self.session.poll(now, waker);
        }
        panic!("{method} is answered");
    }

    fn start(&mut self, host: &mut HeadlessHost, waker: &Waker) -> Value {
        let window = self.window.clone();
        self.call(
            host,
            waker,
            method::PerfStart::NAME,
            json!({ "window": window }),
        )
    }

    fn stop(&mut self, host: &mut HeadlessHost, waker: &Waker) -> Value {
        let window = self.window.clone();
        self.call(
            host,
            waker,
            method::PerfStop::NAME,
            json!({ "window": window }),
        )
    }
}

/// Presents `count` frames: `driver` presses "Go" for each, which changes the count shown, and
/// the next pump repaints it.
fn present(driver: &mut Client, host: &mut HeadlessHost, waker: &Waker, count: usize) {
    for _ in 0..count {
        let params = ActParams::new(Target::Locator(Locator::named("Go")), ActionName::Invoke);
        let pressed = driver.call(host, waker, "semantics.act", json!(params));
        assert!(pressed.get("result").is_some(), "{pressed}");
        let _ = host.pump(Duration::from_millis(16));
    }
}

/// The frames an answer to `perf.stop` reports, or a panic naming the answer.
fn frames_of(answer: &Value) -> u64 {
    answer["result"]["frames"]
        .as_u64()
        .unwrap_or_else(|| panic!("perf.stop reports its frames: {answer}"))
}

/// A driver and an observer each start a span on one window: the observer's start is not
/// refused for the driver's span, its stop ends only its own, and the driver's span counts the
/// frames since the driver's start. A second stop from either has no span to end. Asked
/// through no lease, every one of these calls would be refused `invalid_argument`.
#[test]
fn perf_spans_are_each_connections_own_over_the_wire() {
    let service = service(Limits::default(), None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let mut driver = Client::connect(&service, &mut host, &waker, None);
    let mut observer = Client::connect(&service, &mut host, &waker, Some("observer"));

    let started = driver.start(&mut host, &waker);
    assert!(started.get("result").is_some(), "{started}");
    present(&mut driver, &mut host, &waker, 2);
    let started = observer.start(&mut host, &waker);
    assert!(
        started.get("result").is_some(),
        "an observer measures beside the driver: {started}"
    );
    present(&mut driver, &mut host, &waker, 3);
    let observed = observer.stop(&mut host, &waker);
    let observed_frames = frames_of(&observed);
    assert!(observed_frames >= 3, "{observed}");
    present(&mut driver, &mut host, &waker, 1);
    let driven = driver.stop(&mut host, &waker);
    assert!(
        frames_of(&driven) > observed_frames,
        "the driver's span counts from its own start, past the observer's stop: \
         {driven}, {observed}"
    );

    for (name, client) in [("driver", &mut driver), ("observer", &mut observer)] {
        let again = client.stop(&mut host, &waker);
        assert_eq!(
            again["error"]["code"],
            json!("invalid_argument"),
            "the {name}'s span has ended: {again}"
        );
    }
}

/// A window keeps a bounded number of spans, one per connection: once one more is refused
/// `busy`, a connection that goes without `perf.stop` takes its span with it, and the refused
/// connection's next start begins.
#[test]
fn a_perf_span_ends_with_its_connection() {
    let limits = Limits {
        max_connections: 64,
        ..Limits::default()
    };
    let service = service(limits, None);
    let (mut host, _agent, _presses) = shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let mut measuring = vec![Client::connect(&service, &mut host, &waker, None)];
    let mut refused = None;
    while refused.is_none() {
        assert!(
            measuring.len() < limits.max_connections,
            "a window keeps fewer spans than the service admits connections"
        );
        let mut client = Client::connect(&service, &mut host, &waker, Some("observer"));
        let answer = client.start(&mut host, &waker);
        if answer.get("result").is_some() {
            measuring.push(client);
        } else {
            assert_eq!(answer["error"]["code"], json!("busy"), "{answer}");
            refused = Some(client);
        }
    }
    // The first connection started no span yet: its start is refused too, for the same bound.
    let full = measuring[0].start(&mut host, &waker);
    assert_eq!(full["error"]["code"], json!("busy"), "{full}");

    // A measuring connection goes without stopping its span. The owner's next drain releases
    // its lease after the calls queued with it, and the span with the lease.
    drop(measuring.remove(1));
    let _ = host.pump(Duration::ZERO);
    let mut refused = refused.expect("one start was refused");
    let started = refused.start(&mut host, &waker);
    assert!(
        started.get("result").is_some(),
        "the gone connection's span made room: {started}"
    );
    let stopped = refused.stop(&mut host, &waker);
    assert!(stopped.get("result").is_some(), "{stopped}");
}
