//! The pop-result channel: [`Completer`] and [`RouteResult`].
//!
//! Private; nothing here is exported.
//!
//! `Navigator::push` returns the [`RouteResult`] *before* any lifecycle runs, and
//! completing the route resolves it with `result ?? current_result`.
//!
//! # Exactly once
//!
//! [`Completer::complete`] returns `false` on a second call rather than
//! panicking: double completion is a
//! caller/framework-ordering error, not an internal invariant, so
//! [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md) forbids a panic. In
//! practice `RouteEntry::complete` already refuses to re-enter the completion
//! path once the state has passed `Remove`; the guard
//! here is what makes `double_pop_or_double_remove_does_not_double_complete`
//! true rather than merely likely.
//!
//! # Why a hand-rolled one-shot
//!
//! `flui-widgets` must not depend on an async runtime (ADR-0018). This is a
//! small channel built from `std::task`, and the future is polled by the frame-driven
//! `AsyncDriver` that ADR-0018 already installed.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// A typed route result could not be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteResultError {
    /// The route produced a value of a different type than the caller requested.
    TypeMismatch {
        /// `type_name` of the route's declared output type.
        expected: &'static str,
    },
}

impl std::fmt::Display for RouteResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeMismatch { expected } => {
                write!(f, "route result type mismatch: expected `{expected}`")
            }
        }
    }
}

impl std::error::Error for RouteResultError {}

/// Completed-ness and the value are distinct: a route may legitimately complete
/// with `None`, which is not the same as "not completed".
enum Completion<T> {
    Pending,
    /// The route completed, with this result.
    Done(Result<Option<T>, RouteResultError>),
    /// The completed value has already been delivered.
    Consumed,
}

impl<T> Completion<T> {
    fn is_done(&self) -> bool {
        !matches!(self, Self::Pending)
    }

    fn take(&mut self) -> Poll<Result<Option<T>, RouteResultError>> {
        match core::mem::replace(self, Self::Consumed) {
            Self::Done(value) => Poll::Ready(value),
            Self::Pending => {
                *self = Self::Pending;
                Poll::Pending
            }
            Self::Consumed => Poll::Pending,
        }
    }
}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        let value = super::lifecycle::Terminal::new(core::mem::replace(
            &mut self.value,
            Completion::Consumed,
        ));
        let waker = super::lifecycle::Terminal::new(self.waker.take());
        drop(value);
        drop(waker);
    }
}

struct Shared<T> {
    value: Completion<T>,
    waker: Option<Waker>,
}

/// The write half. Held by the route's record; completed exactly once.
pub(crate) struct Completer<T> {
    shared: Rc<RefCell<Shared<T>>>,
}

impl<T> Clone for Completer<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Rc::clone(&self.shared),
        }
    }
}

/// The read half: the route's `popped` future.
///
/// Resolves to the value passed to `pop`/`remove_route`, the route's
/// `current_result()` fallback, no value, or a typed result error. **Dropping
/// it does not cancel anything**: the route completes regardless, as any future
/// that nobody awaits still completes.
// Deliberately **not** `#[must_use]` (raised in the 2026-07-11 API review,
// rejected with evidence): ignoring the handle is the *documented* contract
// above — the route completes regardless, as an unawaited future does — and it is what 169 of this crate's own call sites correctly
// do (`seed_initial` for a bootstrap route, a `push` whose result nobody
// wants). A `must_use` here would be a false positive by construction.
pub struct RouteResult<T> {
    shared: Rc<RefCell<Shared<T>>>,
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
        let shared = Rc::new(RefCell::new(Shared {
            value: Completion::Pending,
            waker: None,
        }));
        (
            Self {
                shared: Rc::clone(&shared),
            },
            RouteResult { shared },
        )
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.shared.borrow().value.is_done()
    }

    /// Complete with `value`. Returns `false` if it was already completed, in
    /// which case `value` is dropped outside the shared borrow and nothing is
    /// woken.
    pub(crate) fn complete(&self, value: Result<Option<T>, RouteResultError>) -> bool {
        let outcome = {
            let mut shared = self.shared.borrow_mut();
            if shared.value.is_done() {
                Err(value)
            } else {
                shared.value = Completion::Done(value);
                Ok(shared.waker.take())
            }
        };
        match outcome {
            Ok(waker) => {
                // Wake outside the borrow: the woken task may poll re-entrantly.
                if let Some(waker) = waker {
                    waker.wake();
                }
                true
            }
            Err(rejected) => {
                drop(rejected);
                false
            }
        }
    }
}

impl<T: 'static> Future for RouteResult<T> {
    type Output = Result<Option<T>, RouteResultError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        {
            let mut shared = self.shared.borrow_mut();
            if let Poll::Ready(value) = shared.value.take() {
                return Poll::Ready(value);
            }
            if shared.value.is_done() {
                return Poll::Ready(Ok(None));
            }
        }
        // Cloning and retiring a waker can invoke user code. Neither happens
        // under the result borrow; clone reentry may complete the route, so
        // check its value again before publishing a pending waiter.
        let waker = cx.waker().clone();
        let (completed, retired) = {
            let mut shared = self.shared.borrow_mut();
            if shared.value.is_done() {
                (true, Some(waker))
            } else {
                (false, shared.waker.replace(waker))
            }
        };
        // Keep any completed user value in the slot until retirement succeeds:
        // a panicking waker destructor must not strand or unwind-drop it.
        drop(retired);
        if completed {
            let value = self.shared.borrow_mut().value.take();
            match value {
                Poll::Ready(value) => Poll::Ready(value),
                Poll::Pending => Poll::Ready(Ok(None)),
            }
        } else {
            Poll::Pending
        }
    }
}

impl<T> RouteResult<T> {
    /// The completed value, if any, without awaiting. Consumes it.
    ///
    /// Lets a synchronous test assert on the result without an executor — the
    /// route machinery is pure data, and so are its tests.
    ///
    /// The nesting is meaningful, not accidental: the **outer** `Option` is
    /// "is a completed value still available?", while the `Result` contains
    /// either the route's optional value or its typed delivery error.
    #[must_use]
    pub fn try_take(&self) -> Option<Result<Option<T>, RouteResultError>> {
        let value = self.shared.borrow_mut().value.take();
        match value {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    }

    /// Whether the route has completed (even with `None`).
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.shared.borrow().value.is_done()
    }
}

#[cfg(test)]
#[expect(
    unsafe_code,
    reason = "RawWaker callbacks expose clone and retirement failures through the real waker API"
)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::mem::ManuallyDrop;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Weak;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::{RawWaker, RawWakerVTable, Wake};

    thread_local! {
        static REENTRY_SHARED: RefCell<Option<Weak<RefCell<Shared<i32>>>>> =
            const { RefCell::new(None) };
        static COMPLETE_ON_CLONE: Cell<bool> = const { Cell::new(false) };
    }

    struct ReentryFixture {
        previous_shared: Option<Weak<RefCell<Shared<i32>>>>,
        previous_complete_on_clone: bool,
    }

    impl ReentryFixture {
        fn install(completer: &Completer<i32>, complete_on_clone: bool) -> Self {
            let previous_shared = REENTRY_SHARED
                .with(|current| current.replace(Some(Rc::downgrade(&completer.shared))));
            let previous_complete_on_clone =
                COMPLETE_ON_CLONE.with(|current| current.replace(complete_on_clone));
            Self {
                previous_shared,
                previous_complete_on_clone,
            }
        }
    }

    impl Drop for ReentryFixture {
        fn drop(&mut self) {
            REENTRY_SHARED.with(|current| {
                current.replace(self.previous_shared.take());
            });
            COMPLETE_ON_CLONE.with(|current| current.set(self.previous_complete_on_clone));
        }
    }

    struct WakerProbe {
        fail_clone: AtomicBool,
        fail_drop: AtomicBool,
        fail_wake: AtomicBool,
        wakes: AtomicUsize,
    }

    impl WakerProbe {
        fn inspect() {
            REENTRY_SHARED.with(|current| {
                let current = current.borrow();
                if let Some(shared) = current.as_ref().and_then(Weak::upgrade) {
                    assert!(
                        shared.try_borrow_mut().is_ok(),
                        "user waker ran under result borrow"
                    );
                }
            });
        }

        fn waker(self: &Arc<Self>) -> Waker {
            // SAFETY: each raw waker owns one Arc strong reference. Its vtable
            // balances that ownership and contains only Send + Sync state.
            unsafe {
                Waker::from_raw(RawWaker::new(
                    Arc::into_raw(Arc::clone(self)).cast(),
                    &VTABLE,
                ))
            }
        }
    }

    fn complete_on_clone() {
        if !COMPLETE_ON_CLONE.with(Cell::get) {
            return;
        }
        COMPLETE_ON_CLONE.with(|current| current.set(false));
        let completer = REENTRY_SHARED.with(|current| {
            let shared = current
                .borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("result remains owned");
            Completer { shared }
        });
        assert!(completer.complete(Ok(Some(7))));
    }

    unsafe fn clone_waker(data: *const ()) -> RawWaker {
        // SAFETY: data originates from Arc::into_raw in WakerProbe::waker; the
        // borrowed original reference must remain owned by the source waker.
        let probe = ManuallyDrop::new(unsafe { Arc::from_raw(data.cast::<WakerProbe>()) });
        WakerProbe::inspect();
        assert!(
            !probe.fail_clone.swap(false, Ordering::Relaxed),
            "clone failure"
        );
        complete_on_clone();
        RawWaker::new(Arc::into_raw(Arc::clone(&probe)).cast(), &VTABLE)
    }

    unsafe fn wake_waker(data: *const ()) {
        // SAFETY: consuming wake takes exactly the strong reference owned by
        // this raw waker, unlike wake_by_ref which borrows it.
        let probe = unsafe { Arc::from_raw(data.cast::<WakerProbe>()) };
        WakerProbe::inspect();
        probe.wakes.fetch_add(1, Ordering::Relaxed);
        assert!(
            !probe.fail_wake.swap(false, Ordering::Relaxed),
            "wake failure"
        );
    }

    unsafe fn wake_waker_by_ref(data: *const ()) {
        // SAFETY: retain the reference owned by the caller's waker.
        let probe = ManuallyDrop::new(unsafe { Arc::from_raw(data.cast::<WakerProbe>()) });
        WakerProbe::inspect();
        probe.wakes.fetch_add(1, Ordering::Relaxed);
    }

    unsafe fn drop_waker(data: *const ()) {
        // SAFETY: drop takes exactly this raw waker's Arc strong reference.
        let probe = unsafe { Arc::from_raw(data.cast::<WakerProbe>()) };
        WakerProbe::inspect();
        assert!(
            !probe.fail_drop.swap(false, Ordering::Relaxed),
            "retirement failure"
        );
    }

    const VTABLE: RawWakerVTable =
        RawWakerVTable::new(clone_waker, wake_waker, wake_waker_by_ref, drop_waker);

    fn probe() -> Arc<WakerProbe> {
        Arc::new(WakerProbe {
            fail_clone: AtomicBool::new(false),
            fail_drop: AtomicBool::new(false),
            fail_wake: AtomicBool::new(false),
            wakes: AtomicUsize::new(0),
        })
    }

    fn poll(
        result: &mut RouteResult<i32>,
        waker: &Waker,
    ) -> Poll<Result<Option<i32>, RouteResultError>> {
        Pin::new(result).poll(&mut Context::from_waker(waker))
    }

    fn try_take_distinguishes_pending_and_typed_completion() {
        let (completer, result) = Completer::new();
        assert_eq!(result.try_take(), None);
        let terminal = completer.clone();
        assert!(completer.complete(Ok(Some(42))));
        assert_eq!(result.try_take(), Some(Ok(Some(42))));
        assert_eq!(result.try_take(), None);
        assert!(!terminal.complete(Ok(None)));

        let (completer, result) = Completer::<i32>::new();
        assert!(completer.complete(Ok(None)));
        assert_eq!(result.try_take(), Some(Ok(None)));
    }

    fn poll_delivers_typed_error() {
        let (completer, mut result) = Completer::<i32>::new();
        let error = RouteResultError::TypeMismatch { expected: "i32" };
        assert_eq!(
            error.to_string(),
            "route result type mismatch: expected `i32`"
        );
        assert!(completer.complete(Err(error)));
        assert_eq!(poll(&mut result, Waker::noop()), Poll::Ready(Err(error)));
    }

    fn clone_reentry_can_complete_before_waiter_registration() {
        let (completer, mut result) = Completer::new();
        let _reentry = ReentryFixture::install(&completer, true);
        let probe = probe();
        let waker = probe.waker();
        assert_eq!(poll(&mut result, &waker), Poll::Ready(Ok(Some(7))));
        assert!(result.is_completed());
        assert!(!completer.complete(Ok(Some(8))));
    }

    fn clone_failure_keeps_pending_work_deliverable() {
        let (completer, mut result) = Completer::new();
        let _reentry = ReentryFixture::install(&completer, false);
        let probe = probe();
        let waker = probe.waker();
        probe.fail_clone.store(true, Ordering::Relaxed);
        let failure = catch_unwind(AssertUnwindSafe(|| poll(&mut result, &waker)))
            .expect_err("clone must fail");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"clone failure"));
        assert_eq!(poll(&mut result, &waker), Poll::Pending);
        assert!(completer.complete(Ok(Some(9))));
        assert_eq!(probe.wakes.load(Ordering::Relaxed), 1);
        assert_eq!(poll(&mut result, Waker::noop()), Poll::Ready(Ok(Some(9))));
    }

    struct WakeCount(AtomicUsize);
    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn old_waiter_retirement_failure_preserves_replacement_and_next_completion() {
        let (completer, mut result) = Completer::new();
        let _reentry = ReentryFixture::install(&completer, false);
        let probe = probe();
        let old = probe.waker();
        assert_eq!(poll(&mut result, &old), Poll::Pending);
        let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
        let replacement = Waker::from(Arc::clone(&counter));
        probe.fail_drop.store(true, Ordering::Relaxed);
        let outcome = catch_unwind(AssertUnwindSafe(|| poll(&mut result, &replacement)));
        // Disarm cleanup before asserting which failure won.
        probe.fail_drop.store(false, Ordering::Relaxed);
        let failure = outcome.expect_err("old waiter retirement must fail");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"retirement failure"));
        assert!(completer.complete(Ok(Some(11))));
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(poll(&mut result, Waker::noop()), Poll::Ready(Ok(Some(11))));
    }

    fn completed_value_survives_unused_waiter_retirement_failure() {
        let (completer, mut result) = Completer::new();
        let _reentry = ReentryFixture::install(&completer, true);
        let probe = probe();
        let waker = probe.waker();
        probe.fail_drop.store(true, Ordering::Relaxed);
        let outcome = catch_unwind(AssertUnwindSafe(|| poll(&mut result, &waker)));
        probe.fail_drop.store(false, Ordering::Relaxed);
        let failure = outcome.expect_err("unused waiter retirement must fail");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"retirement failure"));
        assert!(result.is_completed());
        assert_eq!(poll(&mut result, Waker::noop()), Poll::Ready(Ok(Some(7))));
        assert!(!completer.complete(Ok(Some(8))));
    }

    struct ResultValue {
        shared: Weak<RefCell<Shared<Self>>>,
        fail_drop: bool,
        drops: Arc<AtomicUsize>,
    }

    fn wake_failure_keeps_completion_and_next_result_deliverable() {
        let (completer, mut result) = Completer::new();
        let _reentry = ReentryFixture::install(&completer, false);
        let probe = probe();
        let waker = probe.waker();
        assert_eq!(poll(&mut result, &waker), Poll::Pending);
        probe.fail_wake.store(true, Ordering::Relaxed);
        let failure = catch_unwind(AssertUnwindSafe(|| completer.complete(Ok(Some(13)))))
            .expect_err("wake must fail after completion commits");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"wake failure"));
        assert_eq!(poll(&mut result, Waker::noop()), Poll::Ready(Ok(Some(13))));
        assert!(!completer.complete(Ok(Some(14))));
        let (next, mut next_result) = Completer::new();
        assert!(next.complete(Ok(Some(15))));
        assert_eq!(
            poll(&mut next_result, Waker::noop()),
            Poll::Ready(Ok(Some(15)))
        );
    }

    impl Drop for ResultValue {
        fn drop(&mut self) {
            if let Some(shared) = self.shared.upgrade() {
                assert!(
                    shared.try_borrow_mut().is_ok(),
                    "result retired under result borrow"
                );
            }
            self.drops.fetch_add(1, Ordering::Relaxed);
            assert!(!self.fail_drop, "rejected result failure");
        }
    }

    fn rejected_value_failure_keeps_first_completion_authoritative() {
        let (completer, result) = Completer::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let value = |fail_drop| ResultValue {
            shared: Rc::downgrade(&completer.shared),
            fail_drop,
            drops: Arc::clone(&drops),
        };
        assert!(completer.complete(Ok(Some(value(false)))));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            completer.complete(Ok(Some(value(true))))
        }))
        .expect_err("rejected user value must fail retirement");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"rejected result failure")
        );
        assert!(result.is_completed());
        let accepted = result
            .try_take()
            .expect("completion retained")
            .expect("typed result retained")
            .expect("accepted value retained");
        assert!(!completer.complete(Ok(None)));
        drop(accepted);
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn route_result_failure_matrix() {
        crate::support::test_cases::run_cases(
            "route_result_failure_matrix",
            &[
                (
                    "try_take_distinguishes_pending_and_typed_completion",
                    try_take_distinguishes_pending_and_typed_completion as fn(),
                ),
                ("poll_delivers_typed_error", poll_delivers_typed_error),
                (
                    "wake_failure_keeps_completion_and_next_result_deliverable",
                    wake_failure_keeps_completion_and_next_result_deliverable,
                ),
                (
                    "clone_reentry_can_complete_before_waiter_registration",
                    clone_reentry_can_complete_before_waiter_registration,
                ),
                (
                    "clone_failure_keeps_pending_work_deliverable",
                    clone_failure_keeps_pending_work_deliverable,
                ),
                (
                    "old_waiter_retirement_failure_preserves_replacement_and_next_completion",
                    old_waiter_retirement_failure_preserves_replacement_and_next_completion,
                ),
                (
                    "completed_value_survives_unused_waiter_retirement_failure",
                    completed_value_survives_unused_waiter_retirement_failure,
                ),
                (
                    "rejected_value_failure_keeps_first_completion_authoritative",
                    rejected_value_failure_keeps_first_completion_authoritative,
                ),
            ],
        );
    }
}
