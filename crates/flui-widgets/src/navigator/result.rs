//! The pop-result channel: [`Completer`] and [`RouteResult`].
//!
//! Private; nothing here is exported.
//!
//! # Flutter parity
//!
//! `navigator.dart:433-434`:
//!
//! ```dart
//! Future<T?> get popped => _popCompleter.future;
//! final Completer<T?> _popCompleter = Completer<T?>();
//! ```
//!
//! `Navigator.push` returns `route.popped` *before* any lifecycle runs
//! (`:5060-5063`), and `didComplete` (`:480-482`) completes it with
//! `result ?? currentResult`.
//!
//! # Exactly once
//!
//! Dart's `Completer.complete` **throws** when called twice. FLUI's
//! [`Completer::complete`] returns `false` instead: double completion is a
//! caller/framework-ordering error, not an internal invariant, so
//! [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md) forbids a panic. In
//! practice `RouteEntry::complete` already refuses to re-enter the completion
//! path once the state has passed `Remove` (`navigator.dart:3431`); the guard
//! here is what makes `double_pop_or_double_remove_does_not_double_complete`
//! true rather than merely likely.
//!
//! # Why a hand-rolled one-shot
//!
//! `flui-widgets` must not depend on an async runtime (ADR-0018). This is ~40
//! lines of `std::task`, and the future is polled by the frame-driven
//! `AsyncDriver` that ADR-0018 already installed.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use parking_lot::Mutex;

/// Completed-ness and the value are distinct: a route may legitimately complete
/// with `None` (Dart's `T?`), which is not the same as "not completed".
enum Completion<T> {
    Pending,
    /// The route completed, with this result.
    Done(Option<T>),
}

impl<T> Completion<T> {
    fn is_done(&self) -> bool {
        matches!(self, Self::Done(_))
    }
}

struct Shared<T> {
    value: Completion<T>,
    waker: Option<Waker>,
}

/// The write half. Held by the route's record; completed exactly once.
pub(crate) struct Completer<T> {
    shared: Arc<Mutex<Shared<T>>>,
}

/// The read half — Flutter's `Route.popped`.
///
/// Resolves to the value passed to `pop`/`remove_route`, or the route's
/// `current_result()` fallback, or `None`. **Dropping it does not cancel
/// anything**: the route completes regardless, exactly as a Dart `Future` that
/// nobody awaits still completes.
// Deliberately **not** `#[must_use]` (raised in the 2026-07-11 API review,
// rejected with evidence): ignoring the handle is the *documented* contract
// above — the route completes regardless, exactly as an unawaited Dart
// `Future` does — and it is what 169 of this crate's own call sites correctly
// do (`seed_initial` for a bootstrap route, a `push` whose result nobody
// wants). A `must_use` here would be a false positive by construction.
pub struct RouteResult<T> {
    shared: Arc<Mutex<Shared<T>>>,
}

impl<T> std::fmt::Debug for RouteResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouteResult")
            .field("completed", &self.is_completed())
            .finish_non_exhaustive()
    }
}

impl<T> Completer<T> {
    /// A fresh, uncompleted pair.
    pub(crate) fn new() -> (Self, RouteResult<T>) {
        let shared = Arc::new(Mutex::new(Shared {
            value: Completion::Pending,
            waker: None,
        }));
        (
            Self {
                shared: Arc::clone(&shared),
            },
            RouteResult { shared },
        )
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.shared.lock().value.is_done()
    }

    /// Complete with `value`. Returns `false` if it was already completed, in
    /// which case `value` is dropped and nothing is woken.
    pub(crate) fn complete(&self, value: Option<T>) -> bool {
        let waker = {
            let mut shared = self.shared.lock();
            if shared.value.is_done() {
                return false;
            }
            shared.value = Completion::Done(value);
            shared.waker.take()
        };
        // Wake outside the lock: the woken task may poll re-entrantly.
        if let Some(waker) = waker {
            waker.wake();
        }
        true
    }
}

impl<T> Future for RouteResult<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared = self.shared.lock();
        if let Completion::Done(value) = core::mem::replace(&mut shared.value, Completion::Pending)
        {
            return Poll::Ready(value);
        }
        shared.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl<T> RouteResult<T> {
    /// The completed value, if any, without awaiting. Consumes it.
    ///
    /// Lets a synchronous test assert on the result without an executor — the
    /// route machinery is pure data, and so are its tests.
    ///
    /// The nesting is meaningful, not accidental: the **outer** `Option` is
    /// "has it completed?", the **inner** one is the result, which is
    /// legitimately absent (Dart's `T?`).
    #[allow(clippy::option_option)]
    #[must_use]
    pub fn try_take(&self) -> Option<Option<T>> {
        match core::mem::replace(&mut self.shared.lock().value, Completion::Pending) {
            Completion::Done(value) => Some(value),
            Completion::Pending => None,
        }
    }

    /// Whether the route has completed (even with `None`).
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.shared.lock().value.is_done()
    }
}
