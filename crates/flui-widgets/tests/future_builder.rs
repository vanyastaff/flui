//! Public-API tests for [`FutureBuilder`].
//!
//! These drive the widget through the real `flui_widgets::prelude` surface and a
//! real `HeadlessBinding` frame — the same path `AppBinding::draw_frame` takes.
//! The `flui-view` unit tests cover the seam's internals; this file covers what an
//! app author can observe.
//!
//! # Parity oracles
//!
//! Expected values come from Flutter, not from running the code first:
//! `.flutter/packages/flutter/test/widgets/async_test.dart`
//! (`'tracks life-cycle of Future to success'`, `'… to error'`,
//! `'gives expected snapshot with SynchronousFuture'`,
//! `'runs the builder using given initial data'`,
//! `'ignores initialData when reconfiguring'`,
//! `'gracefully handles transition to other future'`,
//! `'gracefully handles transition to null future'`).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::{rc::Rc, sync::Arc};

use common::{lay_out, loose};
use flui_foundation::ConnectionState;
use parking_lot::Mutex;

// Exercise the public prelude import path: if `FutureBuilder` were not exported
// from `flui_widgets::prelude`, this file would not compile.
use flui_widgets::prelude::*;
use flui_widgets::{FutureFactory, SizedBox, SnapshotBuilder};

/// Deliberately neither `Clone` nor `Copy` — the public API must not need either.
#[derive(Debug, PartialEq)]
struct Payload(i32);

/// Likewise for the error.
#[derive(Debug, PartialEq)]
struct Boom(&'static str);

/// What a build observed, flattened so the test can assert without `Clone`.
#[derive(Debug, PartialEq, Clone, Copy)]
struct Seen {
    state: ConnectionState,
    data: Option<i32>,
    error: Option<&'static str>,
}

/// A future the test completes by hand.
struct Controlled {
    result: Arc<Mutex<Option<Result<Payload, Boom>>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl std::future::Future for Controlled {
    type Output = Result<Payload, Boom>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.result.lock().take() {
            return Poll::Ready(result);
        }
        *self.waker.lock() = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Test-side handle: build the factory, then complete the future.
#[derive(Clone)]
struct Completer {
    result: Arc<Mutex<Option<Result<Payload, Boom>>>>,
    waker: Arc<Mutex<Option<Waker>>>,
    subscriptions: Arc<AtomicUsize>,
}

impl Completer {
    fn new() -> Self {
        Self {
            result: Arc::new(Mutex::new(None)),
            waker: Arc::new(Mutex::new(None)),
            subscriptions: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Pre-seed the result: the future is `Ready` on its very first poll — the
    /// Rust analogue of Dart's `SynchronousFuture`.
    fn ready(result: Result<Payload, Boom>) -> Self {
        let completer = Self::new();
        *completer.result.lock() = Some(result);
        completer
    }

    fn factory(&self) -> FutureFactory<Payload, Boom> {
        let result = Arc::clone(&self.result);
        let waker = Arc::clone(&self.waker);
        let subscriptions = Arc::clone(&self.subscriptions);
        Rc::new(move || {
            subscriptions.fetch_add(1, Ordering::Relaxed);
            Box::pin(Controlled {
                result: Arc::clone(&result),
                waker: Arc::clone(&waker),
            })
        })
    }

    fn subscriptions(&self) -> usize {
        self.subscriptions.load(Ordering::Relaxed)
    }

    /// Complete from outside a frame, as a real async completion would.
    fn complete(&self, result: Result<Payload, Boom>) {
        *self.result.lock() = Some(result);
        if let Some(waker) = self.waker.lock().as_ref() {
            waker.wake_by_ref();
        }
    }
}

/// Records every snapshot the builder was handed. Reads by reference, so `T`/`E`
/// never need `Clone`.
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

fn done(data: Option<i32>, error: Option<&'static str>) -> Seen {
    Seen {
        state: ConnectionState::Done,
        data,
        error,
    }
}

/// `'gives expected snapshot with SynchronousFuture'`: an already-ready future
/// must never let the builder observe `Waiting`.
#[test]
fn future_builder_immediately_ready_never_shows_waiting() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::ready(Ok(Payload(5)));

    let _laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );

    let observed = log.lock().clone();
    assert!(
        !observed.iter().any(|s| s.state == ConnectionState::Waiting),
        "a synchronously-complete future must never flash Waiting: {observed:?}"
    );
    assert_eq!(last(&log), done(Some(5), None));
}

/// `'tracks life-cycle of Future to success'`: `Waiting` → `Done + data`.
#[test]
fn future_builder_pending_then_success() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::new();

    let mut laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    assert_eq!(
        last(&log),
        Seen {
            state: ConnectionState::Waiting,
            data: None,
            error: None
        }
    );

    completer.complete(Ok(Payload(42)));
    laid.tick();

    assert_eq!(last(&log), done(Some(42), None));
}

/// `'tracks life-cycle of Future to error'`: the error clears the data.
#[test]
fn future_builder_pending_then_error() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::new();

    let mut laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(1))),
        loose(400.0),
    );
    assert_eq!(last(&log).data, Some(1), "initial data survives Waiting");

    completer.complete(Err(Boom("bad")));
    laid.tick();

    assert_eq!(last(&log), done(None, Some("bad")));
}

/// `'gracefully handles transition to other future'` +
/// `'ignores initialData when reconfiguring'`: the old value stays visible while
/// the new future is `Waiting`, and the seed is not re-applied.
#[test]
fn future_builder_key_change_preserves_old_payload_while_waiting() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let first = Completer::new();

    let mut laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            first.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99))),
        loose(400.0),
    );
    first.complete(Ok(Payload(1)));
    laid.tick();
    assert_eq!(last(&log), done(Some(1), None));

    let second = Completer::new();
    laid.pump_widget(
        FutureBuilder::keyed(
            Some(2_u32),
            second.factory(),
            recording_builder(Arc::clone(&log)),
        )
        .with_initial_data(Rc::new(|| Payload(99))),
    );

    assert_eq!(
        last(&log),
        Seen {
            state: ConnectionState::Waiting,
            data: Some(1),
            error: None
        },
        "old value visible while the new future waits; initialData (99) not re-applied"
    );

    second.complete(Ok(Payload(2)));
    laid.tick();
    assert_eq!(last(&log), done(Some(2), None));
}

/// `'gracefully handles transition to null future'`: the task is cancelled, the
/// snapshot drops to `None` keeping the old payload, and a late completion of the
/// old future changes nothing.
#[test]
fn future_builder_transition_to_absent_future_cancels() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::new();

    let mut laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    completer.complete(Ok(Payload(4)));
    laid.tick();
    assert_eq!(last(&log), done(Some(4), None));

    laid.pump_widget(FutureBuilder::<u32, _, _>::keyed(
        None,
        completer.factory(),
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
fn future_builder_same_key_does_not_resubscribe() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::new();

    let mut laid = lay_out(
        FutureBuilder::keyed(
            Some(1_u32),
            completer.factory(),
            recording_builder(Arc::clone(&log)),
        ),
        loose(400.0),
    );
    completer.complete(Ok(Payload(3)));
    laid.tick();
    assert_eq!(completer.subscriptions(), 1);

    laid.pump_widget(FutureBuilder::keyed(
        Some(1_u32),
        completer.factory(),
        recording_builder(Arc::clone(&log)),
    ));

    assert_eq!(completer.subscriptions(), 1, "no resubscribe");
    assert_eq!(last(&log), done(Some(3), None), "snapshot untouched");
}

/// `'runs the builder using given initial data'` with no future at all.
#[test]
fn future_builder_absent_future_shows_initial_data() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let completer = Completer::new();

    let _laid = lay_out(
        FutureBuilder::<u32, _, _>::keyed(
            None,
            completer.factory(),
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
    assert_eq!(completer.subscriptions(), 0, "no future ⇒ no subscription");
}
