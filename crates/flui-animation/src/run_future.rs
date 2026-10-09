//! Run resolution, continuation delivery and waiter custody.

use std::cell::RefCell;
use std::rc::Rc;

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use slab::Slab;

/// Completion state of a ticker future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimationRunFutureState {
    /// Not yet resolved.
    Pending,
    /// Resolved successfully.
    Complete,
    /// Resolved by cancellation.
    Canceled,
}

/// A continuation registered via [`AnimationRunFuture::when_complete_or_cancel`]
/// while the future was still pending.
type Continuation = Box<dyn FnMut(Result<(), RunCanceled>)>;
type PanicPayload = Box<dyn std::any::Any + Send>;

/// The durable resolution plus whatever fan-out was registered before it was
/// known. Both live behind ONE lock so [`RunCompleter::complete`]/
/// [`cancel`](RunCompleter::cancel) can publish the resolution and take
/// every already-registered continuation in a single critical section — a
/// registration landing between two separate locks would be pushed into a
/// `Vec` that publish already drained and never run.
struct FutureState {
    resolution: AnimationRunFutureState,
    continuations: Vec<Continuation>,
    waiters: Slab<std::task::Waker>,
}

/// Shared state for a [`AnimationRunFuture`]/[`RunCompleter`]/[`RunDelivery`]
/// triple.
struct AnimationRunFutureInner {
    state: RefCell<FutureState>,
}

impl AnimationRunFutureInner {
    fn new(resolution: AnimationRunFutureState) -> Self {
        Self {
            state: RefCell::new(FutureState {
                resolution,
                continuations: Vec::new(),
                waiters: Slab::new(),
            }),
        }
    }

    fn read_state(&self) -> AnimationRunFutureState {
        self.state.borrow_mut().resolution
    }
}

/// A terminal ticker outcome: [`AnimationRunFutureState`] with `Pending` made
/// unrepresentable, so a resolved value cannot be handled as if it might still
/// be waiting.
///
/// Carries no derives on purpose: it is constructed and matched, never
/// compared, cloned, or formatted.
enum Resolved {
    /// The ticker stopped normally.
    Complete,
    /// The ticker was canceled.
    Canceled,
}

impl Resolved {
    /// This outcome as the value [`AnimationRunFuture`]'s `Future` impl resolves to.
    const fn as_output(&self) -> Result<(), RunCanceled> {
        match self {
            Resolved::Complete => Ok(()),
            Resolved::Canceled => Err(RunCanceled),
        }
    }
}

/// Register under the same lock that publishes terminal state. The previous
/// waker is retired outside the lock, and repeated polls reuse one live slot.
fn poll_resolution(
    inner: &AnimationRunFutureInner,
    registration: &mut Option<usize>,
    cx: &mut Context<'_>,
) -> Poll<Resolved> {
    {
        let state = inner.state.borrow_mut();
        match state.resolution {
            AnimationRunFutureState::Complete => {
                *registration = None;
                return Poll::Ready(Resolved::Complete);
            }
            AnimationRunFutureState::Canceled => {
                *registration = None;
                return Poll::Ready(Resolved::Canceled);
            }
            AnimationRunFutureState::Pending => {
                if registration.is_some_and(|index| state.waiters[index].will_wake(cx.waker())) {
                    return Poll::Pending;
                }
            }
        }
    }
    // RawWaker cloning can execute foreign code. Preserve the old registration
    // if cloning fails, and recheck publication after reacquiring the lock.
    let replacement = cx.waker().clone();
    let mut state = inner.state.borrow_mut();
    let resolved = match state.resolution {
        AnimationRunFutureState::Complete => Some(Resolved::Complete),
        AnimationRunFutureState::Canceled => Some(Resolved::Canceled),
        AnimationRunFutureState::Pending => None,
    };
    if let Some(resolved) = resolved {
        *registration = None;
        drop(state);
        drop(replacement);
        return Poll::Ready(resolved);
    }
    let previous = if let Some(index) = *registration {
        Some(std::mem::replace(&mut state.waiters[index], replacement))
    } else {
        *registration = Some(state.waiters.insert(replacement));
        None
    };
    drop(state);
    drop(previous);
    Poll::Pending
}

/// Keep chronological failure priority even when reporting it runs foreign
/// tracing subscribers. Opaque secondary payloads cannot safely be destroyed.
fn record_delivery_failure(first: &mut Option<PanicPayload>, payload: PanicPayload) {
    let reported = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let text =
            flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-string panic payload>");
        if std::thread::panicking() {
            tracing::error!(
                payload = text,
                "AnimationRunFuture delivery failed while already unwinding"
            );
        } else {
            tracing::error!(payload = text, "AnimationRunFuture delivery failed");
        }
    }));
    if first.is_none() {
        *first = Some(payload);
    } else {
        flui_foundation::panic::retain_opaque_payload(payload);
    }
    if let Err(secondary) = reported {
        flui_foundation::panic::retain_opaque_payload(secondary);
    }
}

fn invoke_continuation(
    mut continuation: Continuation,
    outcome: Result<(), RunCanceled>,
    first: &mut Option<PanicPayload>,
    retain_after_failure: bool,
) {
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| continuation(outcome)));
    if let Err(payload) = called {
        std::mem::forget(continuation);
        record_delivery_failure(first, payload);
    } else if retain_after_failure || first.is_some() || std::thread::panicking() {
        std::mem::forget(continuation);
    } else if let Err(payload) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(continuation)))
    {
        record_delivery_failure(first, payload);
    }
}

fn finish_delivery(first: Option<PanicPayload>) {
    if let Some(payload) = first {
        if std::thread::panicking() {
            flui_foundation::panic::retain_opaque_payload(payload);
        } else {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Every continuation runs before waiters wake. Foreign code and owning waker
/// retirement run outside all locks; a failure cannot starve the remaining tail.
fn deliver_now(
    outcome: Result<(), RunCanceled>,
    continuations: Vec<Continuation>,
    waiters: Slab<std::task::Waker>,
    retain_after_failure: bool,
) {
    let mut first = None;
    for continuation in continuations {
        invoke_continuation(continuation, outcome, &mut first, retain_after_failure);
    }
    for (_, waker) in waiters {
        let woke = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()));
        if let Err(payload) = woke {
            std::mem::forget(waker);
            record_delivery_failure(&mut first, payload);
        } else if retain_after_failure || first.is_some() || std::thread::panicking() {
            std::mem::forget(waker);
        } else if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)))
        {
            record_delivery_failure(&mut first, payload);
        }
    }
    finish_delivery(first);
}

/// A future representing an animation run.
///
/// Returned by controller run methods. Resolves `Ok(())` when the run ends normally and
/// `Err(RunCanceled)` when it is superseded or torn down.
///
/// # Example
///
/// ```rust
/// use flui_animation::AnimationController;
/// use std::time::Duration;
///
/// let controller = AnimationController::builder(Duration::from_secs(1)).build();
/// let future = controller.forward().expect("live controller");
/// assert!(!future.is_complete());
/// controller.tick_at(Duration::from_secs(1));
/// assert!(future.is_complete());
/// ```
pub struct AnimationRunFuture {
    /// Shared inner state
    inner: Rc<AnimationRunFutureInner>,
    /// One pending waiter slot; clones register independently.
    registration: Option<usize>,
}

impl AnimationRunFuture {
    /// Create a fresh pending future and the [`RunCompleter`] that resolves it.
    #[must_use = "dropping the returned RunCompleter immediately cancels this future"]
    pub(crate) fn pending() -> (RunCompleter, Self) {
        let inner = Rc::new(AnimationRunFutureInner::new(
            AnimationRunFutureState::Pending,
        ));
        (
            RunCompleter {
                inner: Rc::clone(&inner),
            },
            Self {
                inner,
                registration: None,
            },
        )
    }

    /// Create an already-completed ticker future.
    ///
    /// This is useful for implementing objects that normally defer to a ticker
    /// but sometimes can skip the ticker because the animation is of zero
    /// duration, but which still need to represent the completed animation.
    pub(crate) fn complete() -> Self {
        Self {
            inner: Rc::new(AnimationRunFutureInner::new(
                AnimationRunFutureState::Complete,
            )),
            registration: None,
        }
    }

    /// Check if the ticker completed normally
    pub fn is_complete(&self) -> bool {
        self.inner.read_state() == AnimationRunFutureState::Complete
    }

    /// Check if the ticker was canceled
    pub fn is_canceled(&self) -> bool {
        self.inner.read_state() == AnimationRunFutureState::Canceled
    }

    /// Check if the run is still pending.
    pub fn is_pending(&self) -> bool {
        self.inner.read_state() == AnimationRunFutureState::Pending
    }

    /// Calls `f` when this future resolves, however it resolves.
    ///
    /// If the future is already resolved when this method is called —
    /// including between outcome publication and delivery of previously
    /// registered continuations — `f` runs
    /// immediately, on the calling thread. Callers must therefore be safe to
    /// re-enter from this call (nothing is deferred to a later turn), and
    /// a late registration may run before continuations awaiting delivery.
    ///
    /// This never blocks: on a still-pending future, `f` is stored and run
    /// later when the controller completes or cancels the run.
    /// `f` is called exactly once. Its `FnMut` bound keeps captures owned
    /// outside the invocation so a callback panic cannot unwind through them.
    pub fn when_complete_or_cancel<F>(&self, f: F)
    where
        F: FnMut(Result<(), RunCanceled>) + 'static,
    {
        let mut state = self.inner.state.borrow_mut();
        match state.resolution {
            AnimationRunFutureState::Pending => state.continuations.push(Box::new(f)),
            AnimationRunFutureState::Complete => {
                drop(state);
                let mut first = None;
                invoke_continuation(Box::new(f), Ok(()), &mut first, false);
                finish_delivery(first);
            }
            AnimationRunFutureState::Canceled => {
                drop(state);
                let mut first = None;
                invoke_continuation(Box::new(f), Err(RunCanceled), &mut first, false);
                finish_delivery(first);
            }
        }
    }
}

impl Clone for AnimationRunFuture {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            registration: None, // Independent waiter per clone
        }
    }
}

/// Resolves when the ticker run this future was created for ends, either way.
///
///
/// Wakers run after publication and outside the waitset lock, so inline polling
/// or dropping a clone is supported. Panicking wakers cannot starve later waiters.
impl Future for AnimationRunFuture {
    type Output = Result<(), RunCanceled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        poll_resolution(&this.inner, &mut this.registration, cx)
            .map(|resolved| resolved.as_output())
    }
}

impl Drop for AnimationRunFuture {
    fn drop(&mut self) {
        let Some(index) = self.registration.take() else {
            return;
        };
        let retired = {
            let mut state = self.inner.state.borrow_mut();
            if state.resolution == AnimationRunFutureState::Pending {
                state.waiters.try_remove(index)
            } else {
                None
            }
        };
        if std::thread::panicking() {
            std::mem::forget(retired);
        } else {
            drop(retired);
        }
    }
}

impl std::fmt::Debug for AnimationRunFuture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state_str = match self.inner.read_state() {
            AnimationRunFutureState::Pending => "pending",
            AnimationRunFutureState::Complete => "complete",
            AnimationRunFutureState::Canceled => "canceled",
        };
        write!(f, "AnimationRunFuture({state_str})")
    }
}

/// The write half of a [`AnimationRunFuture`], created by [`AnimationRunFuture::pending`].
///
/// [`complete`](Self::complete)/[`cancel`](Self::cancel) publish the durable
/// outcome — and take every continuation registered so far — in one locked
/// step, then hand back a [`RunDelivery`] that runs the user-code side
/// (continuations, then wakers) once the caller is ready for it, typically
/// after releasing a lock of its own. Dropped without either call, the
/// completer publishes `Err(RunCanceled)` and delivers it itself — a run
/// nobody explicitly ended still settles rather than hanging its awaiters
/// forever.
#[must_use = "a pending RunCompleter cancels its future if dropped unresolved"]
pub struct RunCompleter {
    inner: Rc<AnimationRunFutureInner>,
}

impl RunCompleter {
    /// Publish `target`, returning the continuations to run iff this call
    /// performed the (once-only) transition — `None` means the future was
    /// already resolved by an earlier call, so there is nothing left to
    /// deliver.
    fn publish(
        &self,
        target: AnimationRunFutureState,
    ) -> Option<(Vec<Continuation>, Slab<std::task::Waker>)> {
        let mut state = self.inner.state.borrow_mut();
        if state.resolution != AnimationRunFutureState::Pending {
            return None;
        }
        state.resolution = target;
        Some((
            std::mem::take(&mut state.continuations),
            std::mem::take(&mut state.waiters),
        ))
    }

    /// Publish `Ok(())` and hand back the delivery half.
    #[must_use = "a RunDelivery delivers on drop; call deliver() where continuations may run"]
    pub fn complete(self) -> RunDelivery {
        let (continuations, waiters) = self
            .publish(AnimationRunFutureState::Complete)
            .unwrap_or_default();
        RunDelivery::new(Rc::clone(&self.inner), Ok(()), continuations, waiters)
    }

    /// Publish `Err(RunCanceled)` and hand back the delivery half.
    #[must_use = "a RunDelivery delivers on drop; call deliver() where continuations may run"]
    pub fn cancel(self) -> RunDelivery {
        let (continuations, waiters) = self
            .publish(AnimationRunFutureState::Canceled)
            .unwrap_or_default();
        RunDelivery::new(
            Rc::clone(&self.inner),
            Err(RunCanceled),
            continuations,
            waiters,
        )
    }
}

impl Drop for RunCompleter {
    fn drop(&mut self) {
        if let Some((continuations, waiters)) = self.publish(AnimationRunFutureState::Canceled) {
            deliver_now(Err(RunCanceled), continuations, waiters, false);
        }
    }
}

impl std::fmt::Debug for RunCompleter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RunCompleter({:?})", self.inner.read_state())
    }
}

/// The fan-out half of a [`RunCompleter::complete`]/
/// [`cancel`](RunCompleter::cancel) call: running continuations and
/// waking polling waiters.
///
/// The resolution is already published by the time this value exists, so a
/// run this settles is never stuck on it — only the fan-out's TIMING (now,
/// via [`deliver`](Self::deliver), or later, on `Drop`) is this type's
/// choice. Splitting resolution from delivery lets a caller finish a state
/// change while still holding its own lock and defer the user-code fan-out
/// (continuations, wakers) until after that lock is released.
#[must_use = "a RunDelivery delivers on drop; call deliver() where continuations may run"]
pub struct RunDelivery {
    _inner: Rc<AnimationRunFutureInner>,
    outcome: Result<(), RunCanceled>,
    continuations: Vec<Continuation>,
    waiters: Slab<std::task::Waker>,
    delivered: bool,
}

impl RunDelivery {
    fn new(
        inner: Rc<AnimationRunFutureInner>,
        outcome: Result<(), RunCanceled>,
        continuations: Vec<Continuation>,
        waiters: Slab<std::task::Waker>,
    ) -> Self {
        Self {
            _inner: inner,
            outcome,
            continuations,
            waiters,
            delivered: false,
        }
    }

    /// Run every continuation, each inside its own `catch_unwind` (the
    /// payload logged at `error!` immediately), then wake polling waiters,
    /// then re-raise the first caught payload — unless this call is itself
    /// running during an unwind, in which case it is logged instead of
    /// replacing that unwind. A second call, or a `Drop` after this one, is
    /// a no-op.
    pub fn deliver(mut self) {
        self.run(false);
    }

    /// Deliver accepted continuations and wake waiters while an enclosing
    /// recovery boundary already owns a failure. Retain callback captures and
    /// owning wakers even when their invocation succeeds (ADR-0127): catching
    /// the enclosing failure made `thread::panicking()` false, but retiring an
    /// opaque aggregate could still abort before that boundary regains control.
    ///
    /// The outcome and invocation order are unchanged. Any delivery failure is
    /// raised after the tail; the enclosing boundary must preserve its earlier
    /// failure. Use [`deliver`](Self::deliver) for normal delivery so healthy
    /// captures retire normally.
    pub fn deliver_after_failure(mut self) {
        self.run(true);
    }

    fn run(&mut self, retain_after_failure: bool) {
        if self.delivered {
            return;
        }
        self.delivered = true;
        let continuations = std::mem::take(&mut self.continuations);
        let waiters = std::mem::take(&mut self.waiters);
        deliver_now(self.outcome, continuations, waiters, retain_after_failure);
    }
}

impl Drop for RunDelivery {
    fn drop(&mut self) {
        self.run(false);
    }
}

impl std::fmt::Debug for RunDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunDelivery")
            .field("outcome", &self.outcome)
            .field("delivered", &self.delivered)
            .finish_non_exhaustive()
    }
}

/// Cancellation outcome of a [`AnimationRunFuture`].
///
/// `#[non_exhaustive]`: a future cancel reason (which run superseded this
/// one, for instance) is a plausible additive field.
///
/// # Example
///
/// ```rust
/// use flui_animation::{AnimationController, RunCanceled};
/// use std::future::Future;
/// use std::pin::Pin;
/// use std::task::{Context, Poll, Waker};
/// use std::time::Duration;
///
/// let controller = AnimationController::builder(Duration::from_secs(1)).build();
/// let mut future = controller.forward().expect("live controller");
/// controller.stop().expect("live controller");
///
/// let waker = Waker::noop();
/// let mut cx = Context::from_waker(waker);
/// match Pin::new(&mut future).poll(&mut cx) {
///     Poll::Ready(Err(RunCanceled { .. })) => {}
///     other => panic!("expected Err(RunCanceled), got {other:?}"),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunCanceled;

impl std::fmt::Display for RunCanceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The animation run was canceled")
    }
}

impl std::error::Error for RunCanceled {}

#[cfg(test)]
#[path = "run_future_recovery.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "run_future_unwind.rs"]
mod unwind_tests;
