//! Async-half coverage for [`TickerFuture`] and [`TickerFutureOrCancel`].
//!
//! Every test here carries an explicit strength label, because a green suite
//! over this family has previously been read as deeper coverage than it was:
//!
//! - **discriminating** — fails on the code this change replaces;
//! - **regression pin** — passes before and after; it exists to redden if a
//!   specific line this change authors (or inherits) is ever reverted, and the
//!   line is named in the test's own doc;
//! - **compile-time fence** / **contract documentation** — pins a trait bound
//!   or records an observable contract; no production line reverts it.
//!
//! The wakers here are `std::task::Wake` implementors, not hand-rolled
//! `RawWakerVTable`s: `Arc::strong_count` on the implementor is the oracle for
//! "is this waker still parked somewhere", the workspace lints `unsafe_code`,
//! and the sibling reproducer in `tests/end_of_frame_lifecycle.rs` already uses
//! this shape.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use super::*;

// ---------------------------------------------------------------------------
// probes
// ---------------------------------------------------------------------------

/// A waker whose only job is to be countable: the `Arc` it lives in is the
/// oracle for "the future is still holding this waker", and the wake counter
/// keeps the body non-empty so this is not a no-op waker in disguise.
#[derive(Default)]
struct CountingWaker {
    wakes: AtomicUsize,
}

impl CountingWaker {
    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::Acquire)
    }
}

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::AcqRel);
    }
}

/// Samples, from inside `wake()`, whether the two locks a resolution touches
/// are free: the future's own state mutex and the ticker's `TickerInner` mutex.
struct ResolutionLockProbe {
    future_inner: Arc<TickerFutureInner>,
    ticker_inner: Arc<Mutex<TickerInner>>,
    /// One `(future_state_free, ticker_inner_free)` pair per `wake()`.
    observed: Arc<Mutex<Vec<(bool, bool)>>>,
}

impl Wake for ResolutionLockProbe {
    fn wake(self: Arc<Self>) {
        let future_state_free = self.future_inner.state.try_lock().is_some();
        let ticker_inner_free = self.ticker_inner.try_lock().is_some();
        self.observed
            .lock()
            .push((future_state_free, ticker_inner_free));
    }
}

/// Samples, from inside `wake()`, whether the resolution was already published
/// to the durable state. Records rather than asserts: an assertion failure here
/// would unwind out of `event-listener`'s notify loop, which corrupts its
/// internal notified-counter and turns a readable test failure into an
/// arithmetic overflow inside the dependency's `Drop`.
struct PublishedBeforeNotifyProbe {
    future_inner: Arc<TickerFutureInner>,
    observed_states: Arc<Mutex<Vec<TickerFutureState>>>,
}

impl Wake for PublishedBeforeNotifyProbe {
    fn wake(self: Arc<Self>) {
        let published = *self.future_inner.state.lock();
        self.observed_states.lock().push(published);
    }
}

/// Signals a blocked thread. The send is the whole body — it touches neither
/// the ticker nor the event, which is what the `Future` impl's documented
/// precondition on wakers requires.
struct ChannelWaker {
    woken: Mutex<mpsc::Sender<()>>,
}

impl Wake for ChannelWaker {
    fn wake(self: Arc<Self>) {
        // The receiver may already be gone if the poll resolved another way;
        // a failed send is not this waker's problem to report.
        let _ = self.woken.lock().send(());
    }
}

/// Records whether `Mutex<TickerInner>` was free each time a `tracing` event
/// was emitted on this thread.
struct InnerLockProbeSubscriber {
    ticker_inner: Arc<Mutex<TickerInner>>,
    lock_free_per_event: Arc<Mutex<Vec<bool>>>,
}

impl tracing::Subscriber for InnerLockProbeSubscriber {
    fn register_callsite(
        &self,
        _metadata: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, _event: &tracing::Event<'_>) {
        let lock_free = self.ticker_inner.try_lock().is_some();
        self.lock_free_per_event.lock().push(lock_free);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

/// A callback whose `Drop` samples the ticker's inner lock.
struct CallbackDropProbe {
    ticker_inner: Arc<Mutex<TickerInner>>,
    lock_free_at_drop: Arc<Mutex<Vec<bool>>>,
}

impl Drop for CallbackDropProbe {
    fn drop(&mut self) {
        let lock_free = self.ticker_inner.try_lock().is_some();
        self.lock_free_at_drop.lock().push(lock_free);
    }
}

// ---------------------------------------------------------------------------
// ordering: register before the decisive read
// ---------------------------------------------------------------------------

/// **Discriminating.** Reverting the `continue;` that re-reads the durable
/// state after registering turns the trace into `[0]`.
///
/// Three assertions, two different jobs:
///
/// - `read_trace == [0, 1]` is a **shape** pin. It says this implementation
///   reads, registers, then reads again. A strictly-correct helper written the
///   other way round — register unconditionally, then read once — is also
///   correct and would trace `[1]`; whoever makes that change should read a
///   red trace as "the shape moved", not as a correctness regression, and then
///   check the two assertions below, which do not move.
/// - `total_listeners()` and `Arc::strong_count` are the **invariant** pins,
///   and they are sampled from the test *after* `poll` returns, never from
///   inside the helper. That placement is the point: a helper handed a local
///   `Option<EventListener>` instead of the future's own slot produces the
///   same `[0, 1]` trace and then unlinks the entry as it returns, which is a
///   permanent hang. Sampling from the call site sees the listener count fall
///   back to 0 and the waker's strong count fall back to 1.
#[test]
fn a_poll_registers_its_listener_before_the_decisive_state_read() {
    let mut future = TickerFuture::new();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(
            Pin::new(&mut future).poll(&mut cx).is_pending(),
            "a fresh TickerFuture has nothing to resolve to"
        );
    }

    assert_eq!(
        future.inner.read_trace.lock().as_slice(),
        &[0, 1],
        "shape pin: the first decisive read must happen with nothing registered \
         and be repeated once a listener is linked"
    );
    assert_eq!(
        future.inner.event.total_listeners(),
        1,
        "invariant pin: the parked poll must leave its listener linked to the \
         event, not to a local that unlinks on return"
    );
    assert_eq!(
        Arc::strong_count(&probe),
        2,
        "invariant pin: the parked poll must still hold the executor's waker \
         (this test's handle plus the one stored in the listener)"
    );
    assert_eq!(probe.wakes(), 0, "nothing has resolved yet");
}

/// **Discriminating**, same revert as the base-future test above.
///
/// This exists because the fix's premise is "one state machine written twice":
/// a shared helper is only *proven* shared if both `Future` impls are driven
/// through it.
#[test]
fn an_or_cancel_poll_registers_its_listener_before_the_decisive_state_read() {
    let base = TickerFuture::new();
    let mut or_cancel = base.or_cancel();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(
            Pin::new(&mut or_cancel).poll(&mut cx).is_pending(),
            "a fresh or_cancel() has nothing to resolve to"
        );
    }

    assert_eq!(
        base.inner.read_trace.lock().as_slice(),
        &[0, 1],
        "shape pin: or_cancel's poll must register before its decisive read too"
    );
    assert_eq!(
        base.inner.event.total_listeners(),
        1,
        "invariant pin: or_cancel's parked poll must leave its listener linked"
    );
    assert_eq!(
        Arc::strong_count(&probe),
        2,
        "invariant pin: or_cancel's parked poll must still hold the waker"
    );
}

// ---------------------------------------------------------------------------
// publication order and lock discipline at resolution
// ---------------------------------------------------------------------------

/// **Regression pin** (green before this change too). Reverting the resolution
/// helper to notify *before* it writes the durable state reddens it.
///
/// Register-then-recheck is correct only because the durable state is written
/// first; nothing else in this file can fail if that order is ever reversed.
/// The waker records rather than asserts — see [`PublishedBeforeNotifyProbe`].
#[test]
fn the_resolution_is_published_before_the_notification() {
    let mut future = TickerFuture::new();
    let observed_states = Arc::new(Mutex::new(Vec::new()));
    let probe = Arc::new(PublishedBeforeNotifyProbe {
        future_inner: Arc::clone(&future.inner),
        observed_states: Arc::clone(&observed_states),
    });

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    future.set_complete();

    assert_eq!(
        observed_states.lock().as_slice(),
        &[TickerFutureState::Complete],
        "a woken task must be able to see the resolution that woke it; a waker \
         that reads Pending here is being notified of a state change that has \
         not been published yet"
    );
}

/// **Regression pin** (green before this change too). Reverting the `drop` of
/// the state guard before the notification, or moving the resolution back
/// inside the `Mutex<TickerInner>` scope in `stop`, reddens it.
#[test]
fn an_awaiters_waker_runs_with_no_ticker_lock_held() {
    let mut ticker = Ticker::new();
    let mut future = ticker.start(|_| {});
    let observed = Arc::new(Mutex::new(Vec::new()));
    let probe = Arc::new(ResolutionLockProbe {
        future_inner: Arc::clone(&future.inner),
        ticker_inner: Arc::clone(&ticker.inner),
        observed: Arc::clone(&observed),
    });

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    ticker.stop();

    assert_eq!(
        observed.lock().as_slice(),
        &[(true, true)],
        "an awaiter's waker must run with neither the future's state mutex nor \
         Mutex<TickerInner> held — a waker is user code and may re-enter either"
    );
}

/// **Regression pin** (green before this change too). Reverting the
/// `if *state != Pending` guard in the resolution helper reddens it.
///
/// It also pins the invariant that makes "two resolutions cannot interleave"
/// true at all: the transition is once-only, so a re-read after registering can
/// never miss a resolution that a later notification would have to re-announce.
#[test]
fn a_second_resolution_is_ignored_and_the_first_outcome_stands() {
    let future = TickerFuture::new();

    future.set_complete();
    future.set_canceled();

    assert!(
        future.is_complete(),
        "the first resolution stands; the second is a no-op"
    );
    assert!(!future.is_canceled());
}

// ---------------------------------------------------------------------------
// wakeup delivery
// ---------------------------------------------------------------------------

/// **Regression pin** (green before this change too, because it synchronises on
/// an observed `Poll::Pending` and so never enters the racy window). Removing
/// the notification from the resolution helper reddens it.
///
/// Two deliberate shapes:
///
/// - the worker signals only *after* it has observed `Poll::Pending`, so the
///   listener is provably registered before `stop()` is called;
/// - the worker is never `join`ed. If a wakeup is genuinely lost the worker
///   parks forever; `recv_timeout` then fails this test in five seconds and the
///   process exits with the thread abandoned. A `join` would convert that into
///   a hang bounded only by nextest's slow-timeout, which is minutes.
#[test]
fn a_pending_await_is_woken_and_resolves_when_the_ticker_stops() {
    let mut ticker = Ticker::new();
    let future = ticker.start(|_| {});

    let (parked_tx, parked_rx) = mpsc::channel::<()>();
    let (resolved_tx, resolved_rx) = mpsc::channel::<bool>();

    thread::spawn(move || {
        let (wake_tx, wake_rx) = mpsc::channel::<()>();
        let waker = Waker::from(Arc::new(ChannelWaker {
            woken: Mutex::new(wake_tx),
        }));
        let mut cx = Context::from_waker(&waker);
        let mut future = future;

        if Pin::new(&mut future).poll(&mut cx).is_ready() {
            let _ = resolved_tx.send(false);
            return;
        }
        // Only now is the listener provably registered.
        let _ = parked_tx.send(());

        if wake_rx.recv().is_err() {
            return;
        }
        let _ = resolved_tx.send(Pin::new(&mut future).poll(&mut cx).is_ready());
    });

    parked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the worker must reach a parked poll before the ticker stops");

    ticker.stop();

    let resolved = resolved_rx.recv_timeout(Duration::from_secs(5)).expect(
        "a parked awaiter must be woken by stop() and resolve; a timeout here is \
         the lost wakeup itself",
    );
    assert!(
        resolved,
        "the re-poll after the wakeup must observe the published Complete state"
    );
}

// ---------------------------------------------------------------------------
// cancellation
// ---------------------------------------------------------------------------

/// **Discriminating.** Reverting the base future's `Canceled` arm to fall
/// through into the listener-registering path reddens it.
///
/// The base future deliberately never resolves on cancellation — that is
/// Flutter's `_cancel`, which completes only the secondary future — but
/// "never resolves" must mean *parked with nothing held*, not *parked on a
/// listener for an event that can never fire again* plus a pinned executor
/// waker.
///
/// The second poll is load-bearing: after the first one the cancellation has
/// only marked the existing entry `Notified`, and it is still linked, so the
/// listener count reads 1 either way.
#[test]
fn a_canceled_base_future_holds_no_waker_and_no_listener() {
    let mut future = TickerFuture::new();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    future.set_canceled();
    assert_eq!(probe.wakes(), 1, "the parked poll must have been woken");

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(
            Pin::new(&mut future).poll(&mut cx).is_pending(),
            "the base future does not resolve on cancellation (or_cancel does)"
        );
    }

    assert_eq!(
        future.inner.event.total_listeners(),
        0,
        "a canceled base future must hold no listener: the event can never fire \
         again, so a registration here is a leak that outlives its purpose"
    );
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a canceled base future must hold no executor waker"
    );
}

// ---------------------------------------------------------------------------
// mute -> start must not orphan a live future
// ---------------------------------------------------------------------------

/// **Discriminating.** Reverting `start_inner`'s refusal — the
/// `active_future.is_some()` check — reddens it: the muted ticker accepts the
/// start, overwrites `active_future`, and the first run's future is never
/// resolved by anything, ever.
#[test]
fn mute_then_start_does_not_orphan_the_pending_future() {
    let mut ticker = Ticker::new();
    let first_run = ticker.start(|_| {});
    ticker.mute();

    let refused = ticker.start(|_| {});

    assert!(
        Arc::ptr_eq(&first_run.inner, &refused.inner),
        "a refused start must hand back the live future, not a fresh one"
    );
    assert_eq!(
        ticker.state(),
        TickerState::Muted,
        "a refused start must not half-apply: the ticker stays where it was"
    );

    drop(refused);
    ticker.stop();

    assert!(
        first_run.is_complete(),
        "the first run's future must still be the one stop() resolves; an \
         orphaned future is a permanent hang for anyone awaiting it"
    );
}

/// **Discriminating on `main`**, but not for the reason the shape suggests, and
/// the difference is worth stating because it is easy to mis-read this test as
/// stronger than it is.
///
/// On the code this change replaces there is no refusal at all, so both oracles
/// below are empty and the test fails. What it pins *going forward* is that the
/// refusal's two pieces of user-visible work — emitting a `tracing` event,
/// whose subscriber is arbitrary user code, and dropping the caller's callback,
/// whose `Drop` is arbitrary user code — happen with `Mutex<TickerInner>` free.
///
/// Of those two, only the `tracing` event is actually at risk from where the
/// refusal is written: Rust drops a function's body-scope locals (the guard)
/// before its parameters (the callback), so the callback's `Drop` runs after
/// the guard is released even when the `return` sits inside the guard's block.
/// The callback oracle is kept anyway — it matches the sibling
/// `stale_callback_is_dropped_outside_the_lock` and it would catch a refusal
/// that re-binds the callback into the locked scope — but the subscriber oracle
/// is the one that reddens if the `tracing::error!` moves back under the lock.
#[test]
fn a_refused_start_logs_and_drops_its_callback_with_the_inner_lock_free() {
    let mut ticker = Ticker::new();
    let first_run = ticker.start(|_| {});
    ticker.mute();

    let ticker_inner = Arc::clone(&ticker.inner);
    let lock_free_per_event = Arc::new(Mutex::new(Vec::new()));
    let lock_free_at_drop = Arc::new(Mutex::new(Vec::new()));

    let canary = CallbackDropProbe {
        ticker_inner: Arc::clone(&ticker_inner),
        lock_free_at_drop: Arc::clone(&lock_free_at_drop),
    };

    flui_testing::disarm_interest_cache();
    let refused = tracing::subscriber::with_default(
        InnerLockProbeSubscriber {
            ticker_inner: Arc::clone(&ticker_inner),
            lock_free_per_event: Arc::clone(&lock_free_per_event),
        },
        || {
            ticker.start(move |_| {
                let _keep_alive = &canary;
            })
        },
    );

    assert!(
        Arc::ptr_eq(&first_run.inner, &refused.inner),
        "precondition: this must be the refusal path"
    );

    let logged = lock_free_per_event.lock().clone();
    assert!(
        !logged.is_empty(),
        "a refused start must be diagnosable: it has to emit a tracing event"
    );
    assert!(
        logged.iter().all(|&lock_free| lock_free),
        "the refusal's tracing event must be emitted with Mutex<TickerInner> \
         free — a subscriber is user code and may re-enter the ticker"
    );

    assert_eq!(
        lock_free_at_drop.lock().as_slice(),
        &[true],
        "the rejected callback must be dropped exactly once, with \
         Mutex<TickerInner> free"
    );

    ticker.stop();
}

// ---------------------------------------------------------------------------
// contract fences
// ---------------------------------------------------------------------------

/// **Compile-time fence.** No production line reverts this; it exists so that
/// losing `Send`/`Sync` on either future — which would make the cross-thread
/// resolve/await shape this crate's own tests use stop compiling — is a
/// deliberate act rather than a side effect.
#[test]
fn ticker_future_auto_traits() {
    static_assertions::assert_impl_all!(TickerFuture: Send, Sync, Unpin, Clone);
    static_assertions::assert_impl_all!(TickerFutureOrCancel: Send, Sync, Unpin);
    static_assertions::assert_impl_all!(TickerCanceled: Send, Sync, Copy, std::error::Error);
}

/// **Contract documentation; pins nothing.** Recorded because Flutter's
/// `orCancel` is lazily created and, if accessed after the ticker has already
/// resolved, completes immediately with the recorded outcome — and until now
/// nothing here said whether FLUI matched that.
#[test]
fn or_cancel_accessed_after_resolution_resolves_immediately() {
    let future = TickerFuture::new();
    future.set_canceled();

    let mut or_cancel = future.or_cancel();
    let probe = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&probe));
    let mut cx = Context::from_waker(&waker);

    assert_eq!(
        Pin::new(&mut or_cancel).poll(&mut cx),
        Poll::Ready(Err(TickerCanceled)),
        "or_cancel() accessed after resolution must resolve on its first poll"
    );
    assert_eq!(
        future.inner.event.total_listeners(),
        0,
        "an already-resolved poll must not register a listener at all"
    );
}
