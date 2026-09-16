//! Async-half coverage for [`TickerFuture`], [`TickerCompleter`], and
//! [`TickerDelivery`].
//!
//! Every test here carries an explicit strength label, because a green suite
//! over this family has previously been read as deeper coverage than it was:
//!
//! - **discriminating** — fails on the code this change replaces;
//! - **regression pin** — passes before and after; it exists to redden if a
//!   specific line this change authors (or inherits) is ever reverted, and the
//!   line is named in the test's own doc;
//! - **compile-time fence** — pins a trait bound; no production line reverts
//!   it.
//!
//! A label is only worth having if it is kept honest in both directions: one
//! test below was first labelled "pins nothing" and turned out to pin real
//! production behaviour, which corrupts the inventory just as badly as an
//! over-claim would.
//!
//! The wakers here are `std::task::Wake` implementors, not hand-rolled
//! `RawWakerVTable`s: `Arc::strong_count` on the implementor is the oracle for
//! "is this waker still parked somewhere", the workspace lints `unsafe_code`,
//! and the sibling reproducer in `tests/end_of_frame_lifecycle.rs` already uses
//! this shape.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

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

/// One `tracing` event seen by [`InnerLockProbeSubscriber`], with the ticker's
/// inner-lock state at the moment it was emitted.
struct ObservedEvent {
    target: String,
    /// Call site, used to prove a selector still names exactly one of them.
    line: Option<u32>,
    message: String,
    inner_lock_free: bool,
}

type EventLockLog = Arc<Mutex<Vec<ObservedEvent>>>;

/// Pulls the `message` field out of an event so a test can name the event it
/// means instead of the file it came from.
#[derive(Default)]
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

/// The lock-freedom observations for the one `ticker.rs` event whose message
/// contains `names_the_event`.
///
/// Selected by message, not by target alone: `ticker.rs` emits several
/// distinct events under the target `flui_scheduler::ticker`, so a
/// target-only filter can be satisfied by a *different* one of them after a
/// future edit — the same shape as the hole that let "at least one event"
/// pass without the ticker having logged at all, one step narrower. Selecting
/// by text rather than by line number also survives every edit above the
/// call site, which a line literal would not.
///
/// The call sites are checked rather than returned: two sites emitting the
/// same text would otherwise merge into one oracle silently.
fn lock_states_for_event(log: &EventLockLog, names_the_event: &str) -> Vec<bool> {
    let observed = log.lock();
    let matching: Vec<&ObservedEvent> = observed
        .iter()
        .filter(|event| {
            event.target == "flui_scheduler::ticker" && event.message.contains(names_the_event)
        })
        .collect();
    let sites: std::collections::BTreeSet<Option<u32>> =
        matching.iter().map(|event| event.line).collect();
    assert!(
        sites.len() <= 1,
        "{names_the_event:?} matched events from {} different call sites; the \
         selector no longer names a single event",
        sites.len()
    );
    matching.iter().map(|event| event.inner_lock_free).collect()
}

/// Records whether `Mutex<TickerInner>` was free each time a `tracing` event
/// was emitted on this thread.
struct InnerLockProbeSubscriber {
    ticker_inner: Arc<Mutex<TickerInner>>,
    events: EventLockLog,
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

    fn event(&self, event: &tracing::Event<'_>) {
        let inner_lock_free = self.ticker_inner.try_lock().is_some();
        let mut message = MessageVisitor::default();
        event.record(&mut message);
        self.events.lock().push(ObservedEvent {
            target: event.metadata().target().to_owned(),
            line: event.metadata().line(),
            message: message.0,
            inner_lock_free,
        });
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
/// This is the pin for the lost-wakeup defect #1161's first fix closed: a
/// resolution landing between the first state read and `listen()` used to
/// notify zero listeners and be lost forever. `poll_resolution` itself is
/// unchanged by the controller-owned-future redesign, so this pin travels
/// with it unmodified.
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
    let (_completer, mut future) = TickerFuture::pending();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(
            Pin::new(&mut future).poll(&mut cx).is_pending(),
            "a fresh pending future has nothing to resolve to"
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

// ---------------------------------------------------------------------------
// publication order and resolution shape
// ---------------------------------------------------------------------------

/// **Discriminating**, on the *invariant* rather than the API: the ordering
/// this pins (publish the durable state, THEN notify) predates this PR — it
/// held for `TickerFuture::resolve` before `TickerCompleter::publish`
/// replaced it — but this specific test is new, since it exercises the new
/// completer/future pair. Reverting `TickerCompleter::publish` to notify
/// before it writes the durable state — or to notify while the state guard
/// is still held — reddens it.
///
/// Register-then-recheck is correct only because the durable state is written
/// first; nothing else in this file can fail if that order is ever reversed.
/// The waker records rather than asserts: an assertion failure here would
/// unwind out of `event-listener`'s notify loop, which skips its
/// notified-counter increment and turns a readable test failure into an
/// arithmetic overflow inside the dependency's own `Drop`.
#[test]
fn the_resolution_is_published_before_the_notification() {
    let (completer, mut future) = TickerFuture::pending();
    let future_inner = Arc::clone(&future.inner);
    let observed_states = Arc::new(Mutex::new(Vec::new()));

    struct PublishedBeforeNotifyProbe {
        future_inner: Arc<TickerFutureInner>,
        observed_states: Arc<Mutex<Vec<Option<TickerFutureState>>>>,
    }
    impl Wake for PublishedBeforeNotifyProbe {
        fn wake(self: Arc<Self>) {
            let published = self
                .future_inner
                .state
                .try_lock()
                .map(|state| state.resolution);
            self.observed_states.lock().push(published);
        }
    }

    let probe = Arc::new(PublishedBeforeNotifyProbe {
        future_inner,
        observed_states: Arc::clone(&observed_states),
    });

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    completer.complete().deliver();

    assert_eq!(
        observed_states.lock().as_slice(),
        &[Some(TickerFutureState::Complete)],
        "a woken task must be able to see the resolution that woke it: Pending \
         means the notification outran the publication, and None means the \
         resolver was still holding the state guard while it notified"
    );
}

/// **Discriminating.** A pending future manually polled after its completer
/// cancels (and delivers) must resolve `Err(TickerCanceled)` on the very next
/// poll, with no listener left registered. Reverting `poll_resolution`'s
/// `Canceled` arm (or `TickerFuture`'s `Future` impl) back to "the base
/// future never resolves on cancellation" reddens this.
#[test]
fn a_pending_future_resolves_err_on_cancel() {
    let (completer, mut future) = TickerFuture::pending();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    completer.cancel().deliver();

    let waker = Waker::from(Arc::clone(&probe));
    let mut cx = Context::from_waker(&waker);
    assert_eq!(
        Pin::new(&mut future).poll(&mut cx),
        Poll::Ready(Err(TickerCanceled)),
        "a canceled future must resolve Err(TickerCanceled), not park forever"
    );
    assert_eq!(
        future.inner.event.total_listeners(),
        0,
        "a resolved future must hold no listener once it has reported Ready"
    );
}

// ---------------------------------------------------------------------------
// continuations
// ---------------------------------------------------------------------------

/// **Discriminating.** A continuation registered on a pending future must run
/// exactly once, with the published outcome, once the completer resolves.
/// Reverting `TickerCompleter::publish`'s `mem::take` of the continuation
/// `Vec` (so nothing is ever handed to `TickerDelivery`) reddens this.
#[test]
fn a_continuation_on_a_pending_future_runs_once_with_the_outcome() {
    let (completer, future) = TickerFuture::pending();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));

    let calls2 = Arc::clone(&calls);
    let observed2 = Arc::clone(&observed);
    future.when_complete_or_cancel(move |outcome| {
        calls2.fetch_add(1, Ordering::SeqCst);
        let _prev = observed2.lock().replace(outcome);
    });

    completer.complete().deliver();

    assert_eq!(calls.load(Ordering::SeqCst), 1, "must run exactly once");
    assert_eq!(*observed.lock(), Some(Ok(())));
}

/// **Discriminating.** Registering on an already-resolved future must invoke
/// the callback immediately, on the calling thread, without ever touching the
/// continuation `Vec`. Reverting the fast-path arms of
/// `TickerFuture::when_complete_or_cancel` back to unconditionally pushing
/// onto `state.continuations` reddens this (the callback would never run: the
/// future is already resolved, so no future `TickerCompleter` call remains to
/// drain it).
#[test]
fn when_complete_or_cancel_on_a_resolved_future_runs_immediately() {
    let future = TickerFuture::complete();
    let ran = Arc::new(AtomicBool::new(false));
    let ran2 = Arc::clone(&ran);

    future.when_complete_or_cancel(move |outcome| {
        assert_eq!(outcome, Ok(()));
        ran2.store(true, Ordering::SeqCst);
    });

    assert!(
        ran.load(Ordering::SeqCst),
        "an already-resolved future must run the callback synchronously, before \
         when_complete_or_cancel returns"
    );
}

/// **Discriminating.** Dropping a [`TickerCompleter`] without ever calling
/// `complete`/`cancel` must still resolve its future — as a cancellation, so
/// a run nobody explicitly ended settles rather than hanging its awaiters
/// forever. Deleting `impl Drop for TickerCompleter` reddens this.
#[test]
fn dropping_a_completer_without_resolving_cancels_the_future() {
    let (completer, mut future) = TickerFuture::pending();
    drop(completer);

    assert!(future.is_canceled());

    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    assert_eq!(
        Pin::new(&mut future).poll(&mut cx),
        Poll::Ready(Err(TickerCanceled))
    );
}

/// **Discriminating.** Reverting `TickerCompleter::publish` to run
/// continuations *inside* the `state.lock()` critical section — rather than
/// draining into a `Vec` and running them after the guard drops — reddens
/// this.
#[test]
fn a_continuation_sees_the_state_lock_free() {
    let (completer, future) = TickerFuture::pending();
    let inner = Arc::clone(&future.inner);
    let observed = Arc::new(Mutex::new(None));
    let observed2 = Arc::clone(&observed);

    future.when_complete_or_cancel(move |_outcome| {
        let _prev = observed2.lock().replace(inner.state.try_lock().is_some());
    });

    completer.complete().deliver();

    assert_eq!(
        observed.lock().as_ref(),
        Some(&true),
        "a continuation must see the future's own state lock free — the guard \
         that published the resolution must already be released"
    );
}

/// **Discriminating.** A continuation must run *before* any waker is
/// notified — reverting `deliver_now`'s order (notify, then run
/// continuations) reddens this: the continuation below would observe one
/// wake already delivered instead of zero.
#[test]
fn continuations_run_before_wakers_are_notified() {
    let (completer, mut future) = TickerFuture::pending();
    let probe = Arc::new(CountingWaker::default());

    {
        let waker = Waker::from(Arc::clone(&probe));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
    }

    let probe_in_continuation = Arc::clone(&probe);
    let wakes_seen_by_continuation = Arc::new(AtomicUsize::new(usize::MAX));
    let observed = Arc::clone(&wakes_seen_by_continuation);
    future.when_complete_or_cancel(move |_outcome| {
        observed.store(probe_in_continuation.wakes(), Ordering::SeqCst);
    });

    completer.complete().deliver();

    assert_eq!(
        wakes_seen_by_continuation.load(Ordering::SeqCst),
        0,
        "the continuation must run before the waker is notified"
    );
    assert_eq!(
        probe.wakes(),
        1,
        "the waker must be notified once delivery completes"
    );
}

/// **Discriminating.** Two panicking continuations, with distinct messages,
/// bracket a third that must still run — a panicking continuation must not
/// stop its siblings — and the payload re-raised once delivery finishes must
/// be the FIRST one caught, not the last. Removing the per-continuation
/// `catch_unwind` in `deliver_now` reddens the "siblings still run" half
/// (the middle continuation would never fire); replacing
/// `first_payload.get_or_insert(payload)` with an unconditional overwrite
/// (`Option::insert`) reddens the "first, not last" half — the re-raised
/// text would read "second continuation panics" instead.
#[test]
fn a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised() {
    let (completer, future) = TickerFuture::pending();
    let middle_ran = Arc::new(AtomicBool::new(false));
    let middle_ran2 = Arc::clone(&middle_ran);

    future.when_complete_or_cancel(|_outcome| panic!("first continuation panics"));
    future.when_complete_or_cancel(move |_outcome| {
        middle_ran2.store(true, Ordering::SeqCst);
    });
    future.when_complete_or_cancel(|_outcome| panic!("second continuation panics"));

    let delivery = completer.complete();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| delivery.deliver()));

    assert!(
        middle_ran.load(Ordering::SeqCst),
        "a panicking continuation must not prevent its siblings from running"
    );
    let payload =
        result.expect_err("the first caught payload must be re-raised once delivery has finished");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("first continuation panics"),
        "the FIRST caught payload must be the one re-raised, not the last"
    );
}

/// **Fast-path re-entrancy pin, not discriminating on its own** — the same
/// production line `when_complete_or_cancel_on_a_resolved_future_runs_immediately`
/// (the fast path itself) already reddens on. Splitting
/// `TickerCompleter::publish`'s one lock into two would NOT redden this: a
/// registration from inside a continuation always runs after both locks have
/// finished either way, so this test cannot tell a one-lock publish from a
/// two-lock one — only that a reentrant `when_complete_or_cancel` call
/// during delivery does not get lost. Kept anyway for the re-entrancy shape
/// itself, which nothing else in this file drives.
#[test]
fn a_continuation_registered_from_inside_a_continuation_runs() {
    let (completer, future) = TickerFuture::pending();
    let reentrant_ran = Arc::new(AtomicBool::new(false));
    let reentrant_ran2 = Arc::clone(&reentrant_ran);
    let future_for_reentry = future.clone();

    future.when_complete_or_cancel(move |_outcome| {
        let reentrant_ran3 = Arc::clone(&reentrant_ran2);
        future_for_reentry.when_complete_or_cancel(move |_outcome| {
            reentrant_ran3.store(true, Ordering::SeqCst);
        });
    });

    completer.complete().deliver();

    assert!(
        reentrant_ran.load(Ordering::SeqCst),
        "a continuation registered while the future is already resolving must \
         still run"
    );
}

/// **Discriminating**, and the one test in this file with a process-survival
/// stake: reverting `deliver_now`'s `std::thread::panicking()` gate — always
/// `resume_unwind`ing the first caught payload — turns this into a
/// panic-during-panic, which the Rust runtime aborts rather than unwinds. A
/// `Drop for TickerCompleter` that runs mid-unwind is exactly the shape a
/// caller's own panicking `Drop` produces in production.
#[test]
fn a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort() {
    let (completer, future) = TickerFuture::pending();
    future.when_complete_or_cancel(|_outcome| panic!("continuation panics"));

    let (result, log) = flui_testing::log_capture::capture(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _completer = completer;
            panic!("outer panic drops the completer while already unwinding");
        }))
    });

    assert!(
        result.is_err(),
        "the outer panic must still propagate to catch_unwind"
    );
    assert!(
        future.is_canceled(),
        "Drop for TickerCompleter must still resolve the future, even mid-unwind"
    );
    assert!(
        log.count_containing("already unwinding") >= 1,
        "the continuation's panic must be logged rather than silently lost: {log}"
    );
}

// ---------------------------------------------------------------------------
// mute -> start must still be refused
// ---------------------------------------------------------------------------

/// **Discriminating.** A muted ticker is still running a (paused) turn, so
/// `start` must refuse exactly as it would on an `Active` ticker: the ticker
/// stays `Muted`, the original callback is the one that keeps firing once
/// unmuted, and the replacement callback the refused call supplied never
/// fires at all. Widening `start_inner`'s refusal predicate back down to
/// `state == Active` reddens this — the muted ticker would accept the second
/// `start`, silently replacing the paused run's callback.
#[test]
fn mute_then_start_is_refused_and_keeps_the_muted_run() {
    let scheduler = crate::scheduler::UpdateScheduler::new();
    let mut ticker = Ticker::new_with_scheduler(&scheduler);
    let original_calls = Arc::new(AtomicUsize::new(0));
    let original = Arc::clone(&original_calls);
    ticker.start(move |_| {
        original.fetch_add(1, Ordering::SeqCst);
    });
    ticker.mute();
    assert_eq!(ticker.state(), TickerState::Muted);

    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let replacement = Arc::clone(&replacement_calls);
    ticker.start(move |_| {
        replacement.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(
        ticker.state(),
        TickerState::Muted,
        "a refused start must not half-apply: the ticker stays where it was"
    );

    ticker.unmute();
    scheduler.execute_frame();

    assert_eq!(
        original_calls.load(Ordering::SeqCst),
        1,
        "the original run's callback must still be the one installed"
    );
    assert_eq!(
        replacement_calls.load(Ordering::SeqCst),
        0,
        "the refused replacement callback must never fire"
    );

    ticker.stop();
}

/// **Discriminating on `main`**, but not for the reason the shape suggests, and
/// the difference is worth stating because it is easy to mis-read this test as
/// stronger than it is.
///
/// On the code this change replaces there is no refusal at all, so the
/// tracing oracle below is empty and the test fails. What it pins *going
/// forward* is that the refusal's two pieces of user-visible work — emitting
/// a `tracing` event, whose subscriber is arbitrary user code, and dropping
/// the caller's callback, whose `Drop` is arbitrary user code — happen with
/// `Mutex<TickerInner>` free.
///
/// Of those two, **only the subscriber oracle actually pins anything.** Rust
/// drops a function's body-scope locals (the guard) before its parameters (the
/// callback), so the callback's `Drop` runs after the guard is released even
/// when the `return` sits inside the guard's block — which means deleting the
/// explicit `drop(callback)` at the refusal leaves this test green. The
/// callback oracle is kept because it matches the sibling
/// `stale_callback_is_dropped_outside_the_lock` and would catch a refusal that
/// re-binds the callback into the locked scope, but nothing in the shipped code
/// depends on it, and it must not be cited as defending that `drop`.
#[test]
fn a_refused_start_logs_and_drops_its_callback_with_the_inner_lock_free() {
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    ticker.mute();

    let ticker_inner = Arc::clone(&ticker.inner);
    let events: EventLockLog = Arc::new(Mutex::new(Vec::new()));
    let lock_free_at_drop = Arc::new(Mutex::new(Vec::new()));

    let canary = CallbackDropProbe {
        ticker_inner: Arc::clone(&ticker_inner),
        lock_free_at_drop: Arc::clone(&lock_free_at_drop),
    };

    flui_testing::disarm_interest_cache();
    tracing::subscriber::with_default(
        InnerLockProbeSubscriber {
            ticker_inner: Arc::clone(&ticker_inner),
            events: Arc::clone(&events),
        },
        || {
            ticker.start(move |_| {
                let _keep_alive = &canary;
            });
        },
    );

    assert_eq!(
        ticker.state(),
        TickerState::Muted,
        "precondition: this must be the refusal path"
    );

    let logged = lock_states_for_event(&events, "a run is already installed");
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

/// **Discriminating.** Hoisting the vacant-slot `tracing::warn!` back inside
/// `start_inner`'s `inner.lock()` scope reddens it.
///
/// The refusal is not the only early return in that function that reports
/// something: `start_default()` with nothing to dispatch warns too, and it was
/// doing so from inside the guard, thirty lines below the comment stating the
/// rule. A rule with one of its two sites pinned is how the other one drifts.
#[test]
fn a_start_with_nothing_to_dispatch_logs_with_the_inner_lock_free() {
    let ticker = Ticker::new();
    let ticker_inner = Arc::clone(&ticker.inner);
    let events: EventLockLog = Arc::new(Mutex::new(Vec::new()));
    let mut ticker = ticker;

    flui_testing::disarm_interest_cache();
    tracing::subscriber::with_default(
        InnerLockProbeSubscriber {
            ticker_inner: Arc::clone(&ticker_inner),
            events: Arc::clone(&events),
        },
        || ticker.start_default(),
    );

    assert_eq!(
        ticker.state(),
        TickerState::Idle,
        "precondition: a start with no callback to dispatch is a no-op"
    );

    let logged = lock_states_for_event(&events, "without a pre-loaded callback");
    assert!(
        !logged.is_empty(),
        "a start that dispatches nothing must say so"
    );
    assert!(
        logged.iter().all(|&lock_free| lock_free),
        "that warning must be emitted with Mutex<TickerInner> free"
    );
}

/// **Discriminating.** Moving the discard `tracing::trace!` back inside
/// `TickerLease::drop`'s `inner.lock()` scope reddens it.
///
/// The event whose own text read "outside the inner lock" was itself emitted
/// inside it — a claim that was true of the callback drop it describes and
/// false of the event carrying it.
#[test]
fn a_discarded_lease_callback_logs_with_the_inner_lock_free() {
    let scheduler = crate::scheduler::UpdateScheduler::new();
    let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
    let ticker_inner = Arc::clone(&ticker.lock().inner);
    let events: EventLockLog = Arc::new(Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&ticker);

    // Stopping from inside the tick leaves the slot `CheckedOut` on a ticker
    // that is no longer running, which is exactly the lease's discard arm.
    ticker.lock().start(move |_| {
        let owner = weak
            .upgrade()
            .expect("the outer Arc is held by this test for its whole duration");
        owner.lock().stop();
    });

    flui_testing::disarm_interest_cache();
    tracing::subscriber::with_default(
        InnerLockProbeSubscriber {
            ticker_inner: Arc::clone(&ticker_inner),
            events: Arc::clone(&events),
        },
        || scheduler.execute_frame(),
    );

    let logged = lock_states_for_event(&events, "discarding a superseded or stale callback");
    assert!(
        !logged.is_empty(),
        "discarding a superseded callback must be diagnosable"
    );
    assert!(
        logged.iter().all(|&lock_free| lock_free),
        "the discard's tracing event must be emitted with Mutex<TickerInner> \
         free, like the callback drop it describes"
    );
}

// ---------------------------------------------------------------------------
// contract fences
// ---------------------------------------------------------------------------

/// **Compile-time fence.** No production line reverts this; it exists so that
/// losing `Send`/`Sync` on any of these types — which would make the
/// cross-thread resolve/await shape this crate's own tests use stop compiling
/// — is a deliberate act rather than a side effect. `TickerCompleter` and
/// `TickerDelivery` are also pinned `!Clone`: cloning either would let two
/// handles race the once-only resolution or the once-only delivery.
#[test]
fn ticker_future_completer_and_delivery_auto_traits() {
    static_assertions::assert_impl_all!(TickerFuture: Send, Sync, Unpin, Clone);
    static_assertions::assert_impl_all!(TickerCompleter: Send, Sync);
    static_assertions::assert_not_impl_any!(TickerCompleter: Clone);
    static_assertions::assert_impl_all!(TickerDelivery: Send, Sync);
    static_assertions::assert_not_impl_any!(TickerDelivery: Clone);
    static_assertions::assert_impl_all!(TickerCanceled: Send, Sync, Copy, std::error::Error);
}
