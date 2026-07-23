//! Public-API tests for [`StreamBuilder`].
//!
//! Driven through the real `flui_widgets::prelude` surface and a real
//! `HeadlessBinding` frame — the path `AppBinding::draw_frame` takes.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/async_test.dart`
//! (`'tracks events and errors of stream until completion'`,
//! `'runs the builder using given initial data'`,
//! `'ignores initialData when reconfiguring'`,
//! `'gracefully handles transition to other stream'`,
//! `'gracefully handles transition to null stream'`).

mod common;

use std::collections::VecDeque;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use common::{lay_out, loose};
use flui_foundation::ConnectionState;
use flui_widgets::Stream;
use parking_lot::Mutex;

// Exercise the public prelude import path.
use flui_widgets::prelude::*;
use flui_widgets::{SizedBox, SnapshotBuilder, StreamFactory};

/// Deliberately neither `Clone` nor `Copy`.
#[derive(Debug, PartialEq)]
struct Payload(i32);

/// Likewise for the error.
#[derive(Debug, PartialEq)]
struct Boom(&'static str);

/// One queued stream event; `None` ends the stream.
type Event = Option<Result<Payload, Boom>>;

/// What a build observed, flattened so the test can assert without `Clone`.
#[derive(Debug, PartialEq, Clone, Copy)]
struct Seen {
    state: ConnectionState,
    data: Option<i32>,
    error: Option<&'static str>,
}

#[derive(Default)]
struct Channel {
    events: Mutex<VecDeque<Event>>,
    waker: Mutex<Option<Waker>>,
    subscriptions: AtomicUsize,
}

struct Controlled {
    channel: Arc<Channel>,
}

impl Stream for Controlled {
    type Item = Result<Payload, Boom>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(event) = self.channel.events.lock().pop_front() {
            return Poll::Ready(event);
        }
        *self.channel.waker.lock() = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Test-side producer.
#[derive(Clone)]
struct Sender {
    channel: Arc<Channel>,
}

impl Sender {
    fn new() -> Self {
        Self {
            channel: Arc::new(Channel::default()),
        }
    }

    fn factory(&self) -> StreamFactory<Payload, Boom> {
        let channel = Arc::clone(&self.channel);
        Rc::new(move || {
            channel.subscriptions.fetch_add(1, Ordering::Relaxed);
            Box::pin(Controlled {
                channel: Arc::clone(&channel),
            })
        })
    }

    fn subscriptions(&self) -> usize {
        self.channel.subscriptions.load(Ordering::Relaxed)
    }

    fn push(&self, event: Event) {
        self.channel.events.lock().push_back(event);
        if let Some(waker) = self.channel.waker.lock().as_ref() {
            waker.wake_by_ref();
        }
    }

    fn data(&self, value: i32) {
        self.push(Some(Ok(Payload(value))));
    }

    fn error(&self, message: &'static str) {
        self.push(Some(Err(Boom(message))));
    }

    fn end(&self) {
        self.push(None);
    }
}

/// Records every snapshot the builder was handed, by reference.
fn recording_builder(log: Arc<Mutex<Vec<Seen>>>) -> SnapshotBuilder<Payload, Boom> {
    Rc::new(move |_ctx, snapshot| {
        log.lock().push(Seen {
            state: snapshot.connection_state(),
            data: snapshot.data().map(|payload| payload.0),
            error: snapshot.error().map(|boom| boom.0),
        });
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
}

fn last(log: &Arc<Mutex<Vec<Seen>>>) -> Seen {
    *log.lock().last().expect("at least one build")
}

fn active(data: Option<i32>, error: Option<&'static str>) -> Seen {
    Seen {
        state: ConnectionState::Active,
        data,
        error,
    }
}

fn waiting(data: Option<i32>, error: Option<&'static str>) -> Seen {
    Seen {
        state: ConnectionState::Waiting,
        data,
        error,
    }
}

/// Flutter's `afterConnected` is unconditional and Dart's `listen` never delivers
/// synchronously: `Waiting` is always observed before the first event — even one
/// already queued by the producer.
#[test]
fn stream_builder_shows_waiting_before_the_first_event() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sender = Sender::new();
    sender.data(1); // queued BEFORE mount

    let mut laid = lay_out(
        StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );

    assert_eq!(
        last(&log),
        waiting(None, None),
        "a stream must show Waiting before its first event, even one already queued"
    );

    laid.tick();
    assert_eq!(last(&log), active(Some(1), None));
}

/// `'tracks events and errors of stream until completion'`:
/// `Waiting` → `Active(d)` → `Active(err)` → `Active(d)` → `Done`.
#[test]
fn stream_builder_data_error_data_then_done() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sender = Sender::new();

    let mut laid = lay_out(
        StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    assert_eq!(last(&log), waiting(None, None));

    sender.data(1);
    laid.tick();
    assert_eq!(last(&log), active(Some(1), None));

    sender.error("mid");
    laid.tick();
    assert_eq!(
        last(&log),
        active(None, Some("mid")),
        "after_error clears the stale value"
    );

    sender.data(2);
    laid.tick();
    assert_eq!(
        last(&log),
        active(Some(2), None),
        "after_data clears the stale error"
    );

    sender.end();
    laid.tick();
    assert_eq!(
        last(&log),
        Seen {
            state: ConnectionState::Done,
            data: Some(2),
            error: None
        },
        "after_done preserves the last value"
    );
}

/// `'gracefully handles transition to other stream'` +
/// `'ignores initialData when reconfiguring'`.
#[test]
fn stream_builder_key_change_preserves_old_payload_while_waiting() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let first = Sender::new();

    let mut laid = lay_out(
        StreamBuilder::keyed(
            Some(1_u32),
            first.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99))),
        loose(400.0),
    );
    first.data(1);
    laid.tick();
    assert_eq!(last(&log), active(Some(1), None));

    let second = Sender::new();
    laid.pump_widget(
        StreamBuilder::keyed(
            Some(2_u32),
            second.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99))),
    );

    assert_eq!(
        last(&log),
        waiting(Some(1), None),
        "old value visible while the new stream waits; initialData (99) not re-applied"
    );

    second.data(2);
    laid.tick();
    assert_eq!(last(&log), active(Some(2), None));
}

/// `'gracefully handles transition to null stream'`: cancel, drop to `None`,
/// keep the payload.
#[test]
fn stream_builder_transition_to_absent_stream_cancels() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sender = Sender::new();

    let mut laid = lay_out(
        StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    sender.data(4);
    laid.tick();
    assert_eq!(last(&log), active(Some(4), None));

    laid.pump_widget(StreamBuilder::<u32, _, _>::keyed(
        None,
        sender.factory(),
        recording_builder(Arc::clone(&log)),
    ));

    assert_eq!(
        last(&log),
        Seen {
            state: ConnectionState::None,
            data: Some(4),
            error: None
        }
    );
}

/// An unchanged key is an early return: no resubscribe, snapshot untouched.
#[test]
fn stream_builder_same_key_does_not_resubscribe() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sender = Sender::new();

    let mut laid = lay_out(
        StreamBuilder::keyed(
            Some(1_u32),
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    sender.data(3);
    laid.tick();
    assert_eq!(sender.subscriptions(), 1);

    laid.pump_widget(StreamBuilder::keyed(
        Some(1_u32),
        sender.factory(),
        recording_builder(Arc::clone(&log)),
    ));

    assert_eq!(sender.subscriptions(), 1, "no resubscribe");
    assert_eq!(last(&log), active(Some(3), None), "snapshot untouched");
}

/// `'runs the builder using given initial data'` with no stream at all.
#[test]
fn stream_builder_absent_stream_shows_initial_data() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sender = Sender::new();

    let _laid = lay_out(
        StreamBuilder::<u32, _, _>::keyed(
            None,
            sender.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(7))),
        loose(400.0),
    );

    assert_eq!(
        last(&log),
        Seen {
            state: ConnectionState::None,
            data: Some(7),
            error: None
        }
    );
    assert_eq!(sender.subscriptions(), 0, "no stream ⇒ no subscription");
}
