//! Public-API tests for [`StreamBuilder`].
//!
//! Driven through the real `flui_widgets::prelude` surface and a real
//! `HeadlessBinding` frame — the path `UiRealm::draw_frame` takes.
//!
//! # Scenarios
//!
//! Tracking events and errors of a stream until completion, the builder
//! running with initial data, initial data being ignored on reconfigure, and
//! transitions to another stream or to none.

use std::collections::VecDeque;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use crate::common::{lay_out, loose};
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
        let event = self.channel.events.lock().pop_front();
        if let Some(event) = event {
            return Poll::Ready(event);
        }
        let _prev = self.channel.waker.lock().replace(cx.waker().clone());
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

/// `'tracks events and errors of stream until completion'`:
/// `Waiting` → `Active(d)` → `Active(err)` → `Active(d)` → `Done`.
pub(crate) fn stream_builder_data_error_data_then_done() {
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
