//! Public-API tests for [`FutureBuilder`].
//!
//! These drive the widget through the real `flui_widgets::prelude` surface and a
//! real `HeadlessBinding` frame — the same path `UiRealm::draw_frame` takes.
//! The `flui-view` unit tests cover the seam's internals; this file covers what an
//! app author can observe.
//!
//! # Scenarios
//!
//! Expected values are fixed by the documented contract, not by running the
//! code first: the life-cycle of a future to success and to error, the
//! snapshot for an already-ready future, the builder running with initial
//! data, initial data being ignored on reconfigure, and transitions to
//! another future or to none.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::{rc::Rc, sync::Arc};

use crate::common::{lay_out, loose};
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
        let result = self.result.lock().take();
        if let Some(result) = result {
            return Poll::Ready(result);
        }
        let _prev = self.waker.lock().replace(cx.waker().clone());
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

/// `'tracks life-cycle of Future to error'`: the error clears the data.
pub(crate) fn future_builder_pending_then_error() {
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
