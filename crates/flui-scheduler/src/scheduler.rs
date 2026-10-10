//! Main scheduler - coordinates frame lifecycle and task execution
//!
//! The UpdateScheduler is the central orchestrator for FLUI's rendering pipeline,
//! with proper phase separation.
//!
//! ## Frame Lifecycle
//!
//! ```text
//! VSync Signal
//!     ↓
//! handleBeginFrame() ─────────────────────────────────────────┐
//!     │  Phase: TransientCallbacks                            │
//!     │  • Animation tickers fire                             │
//!     │  • One-time frame callbacks execute                   │
//!     ↓                                                       │
//! (microtasks flush)                                          │
//!     │  Phase: MidFrameMicrotasks                            │
//!     ↓                                                       │
//! handleDrawFrame() ──────────────────────────────────────────┤
//!     │  Phase: PersistentCallbacks                           │
//!     │  • Rendering pipeline runs (build/layout/paint)       │
//!     ↓                                                       │
//! (post-frame cleanup)                                        │
//!     │  Phase: PostFrameCallbacks                            │
//!     │  • Cleanup callbacks                                  │
//!     ↓                                                       │
//! Phase: Idle ←───────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust
//! use flui_scheduler::{OwnerFrame, Priority, UpdateScheduler};
//!
//! let scheduler = UpdateScheduler::new();
//! let owner = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
//!
//! // Schedule animation callback (fires during TransientCallbacks)
//! scheduler.schedule_frame_callback(Box::new(|vsync_time| {
//!     // Animation tick - all tickers get same vsync timestamp
//! }));
//!
//! // Add rendering callback (fires during PersistentCallbacks)
//! scheduler.add_persistent_frame_callback(std::rc::Rc::new(|timing| {
//!     // Run build/layout/paint pipeline
//! }));
//!
//! // Execute a frame (typically called by event loop on vsync)
//! let vsync_time = web_time::Instant::now();
//! scheduler.handle_begin_frame(vsync_time, &owner);
//! scheduler.handle_draw_frame();
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

use flui_foundation::panic::retain_opaque_payload as discard_panic_payload;
use parking_lot::Mutex;
use web_time::{Duration, Instant};

use crate::{
    budget::FrameBudget,
    config::TimingsCallback,
    duration::{FrameDuration, Milliseconds},
    frame::{
        AppLifecycleState, FrameId, FramePhase, FrameTiming, OneShotFrameCallback,
        PostFrameCallback, RecurringFrameCallback, SchedulerPhase,
    },
    id::{CallbackId, IdGenerator},
    task::{Priority, TaskQueue},
};

pub(crate) mod execution;
mod identity;
mod post_frame_dispatch;
mod teardown;

// CallbackId is imported from crate::id (re-exported from flui_foundation::FrameCallbackId)

/// Total `Priority::Build` drain passes — including the frame's first,
/// non-reentrant drain — [`UpdateScheduler::handle_draw_frame`] runs before
/// giving up on a chain and tracing a warning. The counter increments on
/// that first ordinary drain too, so this permits 31 *extra* reentrant
/// passes beyond it, not 32. Generous relative to any legitimate reentrant
/// chain (a widget rebuild enqueuing one more rebuild is not expected to
/// nest more than a handful of levels deep in a single frame).
///
/// This cap bounds reentrancy *depth*, not frame time: Animation and Build
/// tasks running to completion in the frame that enqueues them is the
/// documented invariant (only Idle-priority work is deadline-bounded — see
/// `is_idle_deadline_passed`). A task that hits this cap is re-enqueuing
/// itself every single pass, which is a task bug this cap turns into a
/// diagnosable trace rather than an unbounded loop.
///
/// This counts CALLS to [`TaskQueue::execute_until`](crate::TaskQueue::execute_until)
/// in this loop, each of which -- since issue #1057's count budget -- pops
/// at most as many tasks as were queued at that call's OWN start, not
/// "until the queue is empty". A single self-re-enqueuing `Priority::Build`
/// task therefore runs exactly this many times from THIS loop before the
/// cap gives up and warns — but `handle_draw_frame` still ends with one
/// more, UNCONDITIONAL `execute_until(Priority::Idle)` sweep, whose
/// threshold accepts any priority: it picks up the one task this cap's own
/// last pass left queued, so a task that survives the whole cap is
/// actually observed running `MAX_BUILD_REENTRY_PASSES + 1` times in that
/// frame, not this many — see `tests/update_scheduler_reshape.rs`'s
/// `a_self_reenqueuing_build_task_is_bounded_by_the_reentry_cap_not_hung_forever`
/// for the pinned count and why the `+ 1` is not this cap's own doing.
///
/// `#[doc(hidden)] pub` rather than crate-private: an integration test
/// cannot otherwise reference this exact value, and a hardcoded duplicate
/// in that test would silently drift from a change made only here.
#[doc(hidden)]
pub const MAX_BUILD_REENTRY_PASSES: usize = 32;

/// Total outer passes [`UpdateScheduler::flush_microtasks`] runs before
/// giving up on a chain and, if work is still queued, tracing a warning —
/// see that method's own doc for what the bound does and does not mean.
/// Crate-private, unlike
/// [`MAX_BUILD_REENTRY_PASSES`]: its pinning test lives in this same module,
/// which is the only place that needs to name this exact value.
const MAX_MICROTASK_REENTRY_PASSES: usize = 32;

/// Cancellable transient callback with ID
struct CancellableTransientCallback {
    id: CallbackId,
    callback: OneShotFrameCallback,
}

/// Cancellable persistent callback with ID
struct CancellablePersistentCallback {
    id: CallbackId,
    callback: RecurringFrameCallback,
}

/// Lifecycle state listener with ID for removal
struct LifecycleListener {
    id: CallbackId,
    callback: Rc<dyn Fn(AppLifecycleState)>,
}

/// How a frame that an [`end_of_frame`](UpdateScheduler::end_of_frame) waiter
/// was pending for finished.
///
/// `Completed` and `Aborted` carry the SAME [`FrameTiming`] shape but are
/// distinguished on purpose: a post-frame callback's own panic still closes
/// via [`end_frame`](UpdateScheduler::end_frame) and resolves
/// `Completed` (the pipeline already committed layout and paint; only the
/// callback failed), while `Aborted` is the outcome of
/// [`abort_frame`](UpdateScheduler::abort_frame) alone -- a frame whose
/// post-frame callbacks never ran at all. Both variants are struct-shaped
/// and carry variant-level `#[non_exhaustive]`, symmetrically: `Aborted`'s
/// field set can grow (`abort_frame` records nothing else about how the
/// frame ended today, and a reason or phase is a plausible additive field
/// later), and even where `Completed` has no such field pending, leaving it
/// as a plain tuple variant would reopen exactly the hole variant-level
/// `#[non_exhaustive]` closes: an external caller constructing its own
/// `FrameOutcome::Completed(fabricated_timing)` out of thin air. Values of
/// this type are constructed only by the scheduler itself; external code
/// matches them -- through [`timing`](Self::timing) for the field both
/// variants share, or a `{ .. }` pattern for the tag alone -- but can never
/// build one, and there is no public API that takes a `FrameOutcome` as
/// input. `#[non_exhaustive]` at the enum level, too: a caller must not
/// assume these are the only two ways a frame can end.
///
/// This type tells a caller of the end-of-frame future whether the frame
/// aborted. See this crate's
/// `ARCHITECTURE.md` `## Mapping decisions` entry for #1162.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum FrameOutcome {
    /// The frame closed through [`end_frame`](UpdateScheduler::end_frame):
    /// its post-frame callbacks ran (even if one of them panicked -- see the
    /// type's own doc).
    #[non_exhaustive]
    Completed {
        /// The timing for the frame that just completed.
        timing: FrameTiming,
    },
    /// The frame closed through [`abort_frame`](UpdateScheduler::abort_frame):
    /// a panic before the pipeline's post-frame slot, so its post-frame
    /// callbacks never ran.
    #[non_exhaustive]
    Aborted {
        /// The timing as it stood when the frame was cut, not a completed
        /// frame's: per [`FrameTiming::phase_duration`]'s own contract, a
        /// phase's entry is only ever populated once that phase actually
        /// completes, and a phase this frame never reached (or was midway
        /// through when it panicked) never gets that chance.
        timing: FrameTiming,
    },
}

impl FrameOutcome {
    /// The timing this outcome carries, whichever variant it turns out to
    /// be.
    ///
    /// Both variants carry a [`FrameTiming`] (see this type's own doc for
    /// why `Aborted`'s is not a completed frame's), so this is the way to
    /// reach it without matching the variant first -- the only way at all
    /// from a `#[non_exhaustive]` catch-all arm, which cannot bind a
    /// variant's fields.
    #[must_use]
    pub fn timing(&self) -> FrameTiming {
        match self {
            Self::Completed { timing } | Self::Aborted { timing } => *timing,
        }
    }
}

/// The scheduler backing an [`end_of_frame`](UpdateScheduler::end_of_frame)
/// future was dropped before this frame's completion could be delivered.
///
/// Only a frame -- or the scheduler's own teardown -- resolves that future,
/// so a caller holding one across the scheduler's last strong reference
/// being dropped would otherwise wait forever; the scheduler's `Drop`
/// implementation resolves every registered waiter with this error instead.
/// Not `#[non_exhaustive]`: this is a plain unit value with nothing else it
/// could ever carry.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::SchedulerClosed;
///
/// let error = SchedulerClosed;
/// assert_eq!(
///     error.to_string(),
///     "the scheduler was dropped before this frame's completion could be delivered"
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerClosed;

impl std::fmt::Display for SchedulerClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the scheduler was dropped before this frame's completion could be delivered"
        )
    }
}

impl std::error::Error for SchedulerClosed {}

/// Shared state for frame completion future
struct FrameCompletionState {
    /// Resolved outcome (`Some` once a frame closed this waiter, one way or
    /// the other -- see [`FrameCompletionFuture::poll`]).
    completed: Option<Result<FrameOutcome, SchedulerClosed>>,
    /// Waker to notify when frame completes
    waker: Option<Waker>,
}

// A cancelled waiter can own the final executor envelope. During unrelated
// unwind, invoking its opaque destruction could abort before any catch returns.
impl Drop for FrameCompletionState {
    fn drop(&mut self) {
        let waker = self.waker.take();
        if std::thread::panicking() {
            std::mem::forget(waker);
        } else {
            drop(waker);
        }
    }
}

/// Future that resolves when a frame completes
///
/// This is returned by `UpdateScheduler::end_of_frame()` and allows awaiting
/// the completion of the current or next frame.
///
/// # Cancellation
///
/// Dropping this future is the cancellation: it holds the only long-lived
/// strong reference to its shared state, so the drop frees the state and
/// the stored [`Waker`] with it, immediately, taking no scheduler lock. The
/// registry keeps only a [`Weak`] handle and skips a dead one on its next
/// drain or compaction.
///
/// One exception to "immediately", and it is bounded: while
/// `notify_frame_completion` is servicing this entry it holds a temporary
/// strong reference from its own `upgrade()`. A drop racing that window
/// frees nothing until the notifier's loop iteration ends. The waker is
/// already `take()`n out by then, so what that iteration finally drops is
/// an empty state and never caller code under a lock.
///
/// # Fused: polling again after `Ready` repeats the same value
///
/// `Output` is `Result<FrameOutcome, SchedulerClosed>`, and both arms are
/// `Copy` (`FrameTiming` is `Copy`; `SchedulerClosed` is a unit struct) --
/// so `poll` peeks the stored value by copy rather than `take()`-ing it.
/// Nothing removes it once written, so a second poll after `Poll::Ready`
/// returns that same value again instead of hanging forever. There is still no reason to poll
/// it more than once: nothing changes between polls, and every ordinary
/// executor stops polling a future the moment it returns `Ready`.
///
/// # A dropped scheduler resolves the future, it does not strand it
///
/// Only a frame, or the scheduler's own teardown, resolves this future. If
/// the last `UpdateScheduler` handle is dropped while this future is
/// pending, the scheduler's `Drop` implementation resolves every registered
/// waiter with `Err(SchedulerClosed)` and wakes it -- see that
/// implementation's own doc for the one case it cannot reach (a live strong
/// clone another owner still holds defers the drop, the same as any other
/// `Arc`).
///
/// # Example
///
/// ```rust,no_run
/// use flui_scheduler::{FrameOutcome, UpdateScheduler};
///
/// async fn do_end_of_frame_work(scheduler: &UpdateScheduler) {
///     match scheduler.end_of_frame().await {
///         Ok(FrameOutcome::Completed { timing, .. }) => {
///             // Now safe to do post-frame cleanup
///             println!(
///                 "Frame {} completed in {}ms",
///                 timing.id.get(),
///                 timing.elapsed().value()
///             );
///         }
///         Ok(FrameOutcome::Aborted { timing, .. }) => {
///             println!("frame {} aborted before its post-frame callbacks ran", timing.id.get());
///         }
///         // A variant added later still carries a timing -- `.timing()`
///         // reaches it here without knowing which one this is, the only
///         // way to from a `#[non_exhaustive]` catch-all arm.
///         Ok(outcome) => {
///             println!("frame {} ended some other way", outcome.timing().id.get());
///         }
///         Err(_closed) => {
///             println!("the scheduler was dropped before this frame completed");
///         }
///     }
/// }
/// ```
#[must_use = "an unbound FrameCompletionFuture drops immediately and cancels its wait"]
pub struct FrameCompletionFuture {
    state: Arc<Mutex<FrameCompletionState>>,
}

impl std::fmt::Debug for FrameCompletionFuture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock` so Debug never blocks (or deadlocks) on the shared state.
        let completed = self.state.try_lock().map(|s| s.completed.is_some());
        f.debug_struct("FrameCompletionFuture")
            .field("completed", &completed)
            .finish_non_exhaustive()
    }
}

impl Future for FrameCompletionFuture {
    type Output = Result<FrameOutcome, SchedulerClosed>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // NO caller code runs under this guard, on either path -- which
        // rests on a drop-order dependency named at the slow path below,
        // not on the structure alone. That is stronger than the registry's
        // lock-order rule demands, and it is why this reads as two
        // acquisitions rather than one: `Waker`'s
        // `clone`, `wake`, and `drop` are all vtable calls into executor
        // code, and any of them may re-enter this same future. `state` is a
        // non-reentrant `parking_lot::Mutex`, so a re-entrant one under the
        // guard hangs rather than returning. Same lock-then-caller-code
        // trap as `set_on_frame_scheduled` (#1038) and
        // `cancel_frame_callback` (#1156).

        // Fast path: already resolved, or the executor re-polled with the
        // very waker already stored. Neither needs a clone, so the common
        // case of a repeatedly-polled pending future does no executor-owned
        // work at all.
        {
            let state = self.state.lock();

            // Peeked by copy, not `.take()`n: `Result<FrameOutcome,
            // SchedulerClosed>` is `Copy` (both arms are), so nothing is
            // lost by leaving it in place, and a second poll after `Ready`
            // returns the same value again instead of hanging -- see this
            // type's own doc.
            if let Some(outcome) = state.completed {
                return Poll::Ready(outcome);
            }

            if state
                .waker
                .as_ref()
                .is_some_and(|stored| stored.will_wake(cx.waker()))
            {
                return Poll::Pending;
            }
        }

        // Slow path: first poll, or the executor handed us a different
        // waker. Clone outside the guard, then re-acquire to store it.
        //
        // `fresh_waker` outlives the block below, and on the early
        // `Poll::Ready` return inside it the clone is dropped on the way
        // out. That drop is caller code, and it lands OUTSIDE the guard
        // only because `state` is declared in that inner block: locals drop
        // innermost-scope-first, so the guard goes before this binding
        // does. Hoisting `let mut state` to function scope in a later
        // refactor silently reverses that order and runs `Waker::drop`
        // under the lock -- issue #1057's hang, reintroduced by a change
        // that looks like tidying.
        let fresh_waker = cx.waker().clone();

        let displaced_waker = {
            let mut state = self.state.lock();

            // Re-check, because the guard was released across the clone.
            // The window is narrow and real: a frame completing in it has
            // already taken the OLD waker and woken it, and the executor
            // may have replaced that waker precisely because it is no
            // longer the one to wake. Returning `Pending` here on the
            // strength of the check before the clone would then strand the
            // task forever, with the completion delivered to a waker
            // nobody is listening on.
            //
            // Pinned by `a_frame_completing_while_poll_clones_the_waker_
            // still_resolves_it` in `tests/integration_tests.rs`, which
            // opens the window deterministically with a hand-rolled
            // `RawWakerVTable` whose `clone` drives a frame. An ordinary
            // safe waker can land a frame in the same window from another
            // thread; the hand-built one only makes it happen on every run
            // instead of occasionally. Delete these lines without that
            // test and the whole suite stays green.
            if let Some(outcome) = state.completed {
                return Poll::Ready(outcome);
            }

            // Store waker for notification when frame completes
            state.waker.replace(fresh_waker)
        };
        drop(displaced_waker);

        Poll::Pending
    }
}

impl FrameCompletionFuture {
    /// Create a new frame completion future.
    ///
    /// The returned future holds the only *lasting* strong reference to the
    /// shared state; the registry takes a [`Weak`] one through
    /// [`notifier`](Self::notifier). That asymmetry is what makes dropping
    /// the future a complete, lock-free cancellation. The one other strong
    /// reference is the temporary `notify_frame_completion` upgrades for
    /// the length of a single loop iteration.
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FrameCompletionState {
                completed: None,
                waker: None,
            })),
        }
    }

    /// The registry's handle on this future's shared state.
    fn notifier(&self) -> FrameCompletionNotifier {
        FrameCompletionNotifier {
            state: Arc::downgrade(&self.state),
        }
    }
}

/// Pending frame completion notifier.
///
/// [`Weak`], never [`Arc`]: the registry must not keep a cancelled waiter's
/// [`Waker`] alive, and an entry whose `upgrade()` fails means *cancelled*,
/// never *error*.
struct FrameCompletionNotifier {
    state: Weak<Mutex<FrameCompletionState>>,
}

/// Registrations left to compact before a push scans for dead entries, and
/// the value the threshold resets to after every scan and every drain.
///
/// Not zero: a scan that leaves the registry empty would set the threshold
/// to `2 * 0 + 0`, and every later push would then scan, forever.
const COMPACTION_SLACK: usize = 8;

/// The registry of pending [`FrameCompletionFuture`]s.
///
/// # Lock order
///
/// **`completion_waiters` strictly before [`FrameCompletionState`], never
/// nested, and neither held across ANY [`Waker`] operation (`clone`,
/// `wake`, or drop) or across `schedule_frame_if_enabled()`.** Every one of
/// those is a vtable call into caller code that may re-enter this
/// scheduler, and both mutexes are non-reentrant `parking_lot` locks: a
/// re-entry under either one hangs rather than returning.
///
/// `clone` belongs in that list beside `wake` and drop even though it looks
/// inert, because `RawWakerVTable::clone` is as arbitrary as the other two.
/// `FrameCompletionFuture::poll`, `UpdateScheduler::end_of_frame`, and
/// `UpdateScheduler::notify_frame_completion` are the three sites that have
/// to honor this, and each carries the reason at the line that does so.
struct FrameCompletionRegistry {
    /// Live and cancelled registrations, in registration order.
    ///
    /// Cancelled entries are tombstones: dropping a future takes no lock
    /// and so cannot remove its own entry. They are reclaimed by the drain
    /// (which empties the vec outright) or by `compact_if_due`.
    waiters: Vec<FrameCompletionNotifier>,
    /// Length at which the next push compacts.
    next_compaction: usize,
    /// Index of the earliest entry that might still be live.
    ///
    /// Everything below it was observed dead by an earlier scan, and dead
    /// is a final observation for a [`Weak`], so no later scan has to look
    /// at it again. This is what keeps the demand predicate amortized
    /// O(1): without it, a run of tombstones at the front of the vec is
    /// re-walked on every single registration, and the compaction that
    /// would clear them does not arrive until `len` reaches a threshold an
    /// earlier, larger live population already raised. That is quadratic
    /// in an ordinary workload, not an adversarial one, and it runs under
    /// this registry's mutex.
    ///
    /// Reset to zero by anything that moves entries: `compact_if_due`'s
    /// `retain` and `drain` both leave it pointing at a vec whose entries
    /// were all live when they were kept, so zero is both correct and the
    /// cheapest place to resume from.
    first_possibly_live: usize,
    /// Entries examined by every compaction scan so far, for the test that
    /// pins the amortized bound.
    #[cfg(test)]
    compaction_scan_work: usize,
    /// Entries probed by every demand scan so far. Counted separately from
    /// the compaction scan because the two bounds fail independently, and
    /// the demand scan is the one that shipped quadratic.
    #[cfg(test)]
    demand_scan_work: usize,
}

impl FrameCompletionRegistry {
    fn new() -> Self {
        Self {
            waiters: Vec::new(),
            next_compaction: COMPACTION_SLACK,
            first_possibly_live: 0,
            #[cfg(test)]
            compaction_scan_work: 0,
            #[cfg(test)]
            demand_scan_work: 0,
        }
    }

    /// Drop tombstoned entries if the registry has grown past its
    /// threshold, then set the next threshold to `2 * live +
    /// COMPACTION_SLACK`.
    ///
    /// The drain empties the vec every completed frame, so this only ever
    /// matters while registrations accumulate and no frame completes.
    ///
    /// # What the bound is, stated against the peak
    ///
    /// The threshold set here is read from the live count *at this scan*,
    /// and nothing lowers it again until the next scan or drain. So the
    /// bound is `len() <= 2 * live_at_last_compaction + COMPACTION_SLACK`,
    /// and therefore `len() <= 2 * peak_live + COMPACTION_SLACK`, with
    /// total scan work over *n* pushes in O(*n*).
    ///
    /// Phrased against the *instantaneous* live count it is false, and the
    /// gap is not small: hold `3 * COMPACTION_SLACK` waiters until a scan
    /// lifts the threshold, drop all of them, then keep registering and
    /// cancelling, and the registry sits at `len = 3 * COMPACTION_SLACK`
    /// with `live = 0` against a ceiling of one slack. At the shipped
    /// `COMPACTION_SLACK = 8` that is 24 entries against a budget of 8.
    ///
    /// It also does **not** buy "dead entries do not accumulate", which is
    /// a stronger claim than amortized doubling makes.
    ///
    /// Two properties make scanning under the registry guard safe, and only
    /// the second is about correctness:
    ///
    /// * Dropping a [`Weak`] whose strong count is already zero runs no
    ///   caller code. `FrameCompletionState::drop` ran when the count hit
    ///   zero; all that remains is freeing the allocation. So this `retain`
    ///   does not violate the lock order above.
    /// * `strong_count() == 0` is a **final** observation: `upgrade()`
    ///   already fails at zero and the count can never rise again, so this
    ///   can never discard a live waiter. The inverse race — reading 1 for
    ///   an entry that drops to zero immediately after — is harmless: that
    ///   entry survives to the next scan, or to the drain, where
    ///   `upgrade()` returns `None`.
    fn compact_if_due(&mut self) {
        if self.waiters.len() < self.next_compaction {
            return;
        }

        #[cfg(test)]
        {
            self.compaction_scan_work += self.waiters.len();
        }

        // Reset BEFORE the retain, not after. `retain` moves every
        // surviving entry, so the old cursor indexes nothing meaningful
        // once it runs -- and `parking_lot` does not poison, so if anything
        // in that closure ever unwound (`Weak::strong_count` cannot today,
        // but a `tracing` call added there with a panicking subscriber
        // could), the next lock would find a partially-retained vec behind
        // a stale cursor pointing at LIVE entries. That silently skips a
        // live waiter and reproduces this issue's hang, from a site neither
        // guarded path touches. Zero is unconditionally a valid cursor, so
        // writing it first costs nothing and closes that ordering.
        self.first_possibly_live = 0;
        self.waiters
            .retain(|notifier| notifier.state.strong_count() > 0);
        self.next_compaction = 2 * self.waiters.len() + COMPACTION_SLACK;
    }

    /// Whether any registration still has a live future, advancing the
    /// scan cursor past every tombstone it walks over.
    ///
    /// This is the demand predicate: a registration issues a frame request
    /// exactly when this returns `false`, which is the zero-to-one
    /// transition of the live-waiter set.
    ///
    /// Amortized O(1). Each call either stops on the first entry it looks
    /// at, or permanently retires however many tombstones it walks past,
    /// so the total probes over *n* registrations is O(*n*) rather than
    /// O(*n*) per registration. Skipping a retired prefix can never skip a
    /// live waiter: `strong_count() == 0` is final for a [`Weak`], since
    /// `upgrade()` already fails at zero and the count can never rise
    /// again.
    fn has_live_waiter(&mut self) -> bool {
        // The cursor's whole safety argument is that everything below it was
        // observed dead. That holds only while the sole mutation between
        // scans is an append -- and `end_of_frame` pushes through the
        // `waiters` field directly, so nothing but this assertion stops a
        // future index-shifting mutation from stranding the cursor over live
        // entries. Debug-only, and invisible to the scan counter, so it
        // changes neither the shipped cost nor what the bound test measures;
        // what it buys is that the violation surfaces as a loud unit-test
        // failure rather than as a silently skipped waiter that hangs.
        debug_assert!(
            self.first_possibly_live <= self.waiters.len(),
            "BUG: the demand-scan cursor ({}) is past the end of the registry ({}); \
             a mutation that shrank or moved entries did not reset it",
            self.first_possibly_live,
            self.waiters.len()
        );
        debug_assert!(
            self.waiters[..self.first_possibly_live.min(self.waiters.len())]
                .iter()
                .all(|notifier| notifier.state.strong_count() == 0),
            "BUG: the demand-scan cursor ({}) has passed a live waiter; every entry \
             below it must already be a tombstone",
            self.first_possibly_live
        );

        while self.first_possibly_live < self.waiters.len() {
            #[cfg(test)]
            {
                self.demand_scan_work += 1;
            }

            if self.waiters[self.first_possibly_live].state.strong_count() > 0 {
                // Deliberately NOT advanced past a live entry: it may die
                // later, and re-checking one entry is O(1).
                return true;
            }

            self.first_possibly_live += 1;
        }

        false
    }

    /// Take every registration, leaving the registry empty and its
    /// compaction threshold back at its floor.
    ///
    /// Resetting the threshold is load-bearing: without it, it ratchets to
    /// the all-time peak registration count and the bound becomes a
    /// high-water mark rather than a statement about the live population.
    fn drain(&mut self) -> Vec<FrameCompletionNotifier> {
        self.next_compaction = COMPACTION_SLACK;
        self.first_possibly_live = 0;
        std::mem::take(&mut self.waiters)
    }
}

/// The Idle-slice deadline [`UpdateScheduler::drive_frame`] bounds
/// `Priority::Idle` task execution by (see that method's own doc — it
/// never gates `Priority::Animation`/`Build`, and it can never skip a
/// frame).
///
/// A distinct type from `drive_frame`'s `vsync_time: Instant` parameter on
/// purpose: two adjacent, same-typed `Instant` parameters can be silently
/// swapped at a call site (a swap still type-checks, and inverts this
/// method's whole deadline semantics without a compiler diagnostic).
/// Wrapping the second one closes that hole structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdleDeadline(pub Instant);

impl IdleDeadline {
    /// An Idle-slice deadline far enough past `now` that it never passes in
    /// practice — for a caller with no real deadline source yet (no
    /// `FrameClock` wired into any backend; see `drive_frame`'s doc). Idle
    /// work is never deferred under this deadline, matching `drive_frame`'s
    /// behavior before it took a deadline parameter at all.
    #[must_use]
    pub fn far_future(now: Instant) -> Self {
        Self(now + std::time::Duration::from_hours(1))
    }
}

impl From<Instant> for IdleDeadline {
    fn from(instant: Instant) -> Self {
        Self(instant)
    }
}

/// Clears the scheduler's Idle-slice deadline on drop — including during an
/// unwind. See [`UpdateScheduler::drive_frame`]'s own doc ("A panicking task
/// never leaks a stale deadline") for why this must be `Drop`-based rather
/// than a plain "clear after the call" statement: a panicking task or
/// persistent callback inside `handle_begin_frame`/`handle_draw_frame`
/// unwinds straight past ordinary sequential cleanup code.
struct IdleDeadlineGuard<'a> {
    slot: &'a RefCell<Option<Instant>>,
}

impl Drop for IdleDeadlineGuard<'_> {
    fn drop(&mut self) {
        *self.slot.borrow_mut() = None;
    }
}

/// Frame lifecycle and timing state (atomics + guarded fields)
struct FrameState {
    /// Current frame timing
    current_frame: RefCell<Option<FrameTiming>>,
    /// VSync timestamp for current frame
    current_vsync_time: RefCell<Option<Instant>>,
    /// Per-phase timing statistics against a caller-chosen target framerate.
    /// This is the only place `flui-scheduler` tracks a frame-duration
    /// value at all — it is stats-only (`UpdateScheduler::budget_snapshot`,
    /// `avg_fps`, `is_janky`), never a gate. There is deliberately no
    /// separate `frame_duration`/`target_fps` field or accessor on
    /// `UpdateScheduler` itself; see `UpdateScheduler::new`'s doc.
    budget: RefCell<FrameBudget>,
    /// Frame counter
    frame_count: AtomicU64,
    /// Jank tracking - count of frames that exceeded budget
    janky_frame_count: AtomicU64,
    /// Whether warm-up frame was executed
    warm_up_done: AtomicBool,
    /// The current frame's Idle-slice deadline, set by
    /// [`UpdateScheduler::drive_frame`] and consumed by
    /// [`UpdateScheduler::handle_draw_frame`] to decide whether Idle-priority
    /// tasks still fit. `None` (the default, and the state outside
    /// `drive_frame`) means unbounded — a caller driving the phase machine
    /// by hand (`handle_begin_frame`/`handle_draw_frame` directly, as
    /// `HeadlessBinding` does) never has Idle work deferred. Only Idle is
    /// ever gated this way: Animation and Build tasks run unconditionally
    /// regardless of the deadline (see `drive_frame`'s doc).
    idle_deadline: RefCell<Option<Instant>>,
    /// Pending frame completion futures. See
    /// [`FrameCompletionRegistry`] for the lock order this field imposes on
    /// everything that touches it.
    completion_waiters: RefCell<FrameCompletionRegistry>,
    /// The thread driving the currently-open (or most recently opened)
    /// frame, recorded by [`UpdateScheduler::handle_begin_frame`] at the
    /// same point it clears `frame_scheduled`, and load-bearing on the
    /// ORDER of that store relative to the phase transition: see
    /// `handle_begin_frame`'s own comment at the store site for why it
    /// must happen before the phase leaves `Idle`.
    ///
    /// `None` only until the first `handle_begin_frame` call; from then on
    /// it always names the most recently opened frame's driver and is
    /// never cleared back to `None`. That is sound because it is read only
    /// from [`ensure_visual_update`](UpdateScheduler::ensure_visual_update),
    /// and only while `phase()` reports a mid-frame phase, which itself
    /// implies a frame is currently in flight on the thread this field
    /// names — so a value left over from a PREVIOUS frame is never read as
    /// if it were the current one, even though the field itself is never
    /// reset between frames.
    ///
    /// `UpdateScheduler` is `Send + Sync` and documented as reachable from
    /// any thread; this field is what lets `ensure_visual_update`
    /// distinguish "I am the thread already driving this frame" (safe to
    /// trust the phase classification) from "some other thread is
    /// mid-frame and I am not it" (must request regardless of phase, since
    /// nothing on this thread will otherwise observe what prompted the
    /// call).
    frame_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
}

/// Callback registration and cancellation state
struct CallbackState {
    /// Transient callbacks - animation tickers.
    ///
    /// A `VecDeque`, not a `Vec`: `handle_begin_frame` pops one entry at a
    /// time from the front and invokes it outside this lock (see that
    /// method's own comment) so a panicking callback loses only itself --
    /// every callback still behind it in the queue is untouched, not
    /// silently dropped with the batch a single-lock drain would already
    /// have removed it into (issue #1057).
    transient: RefCell<VecDeque<CancellableTransientCallback>>,
    /// Cancelled callback IDs. `DashMap`: a sharded `RwLock`, not
    /// lock-free; `contains_key` releases its shard before returning.
    cancelled: RefCell<HashSet<CallbackId>>,
    /// Callback ID generator
    id_gen: IdGenerator<flui_foundation::markers::FrameCallback>,
    /// Persistent frame callbacks (every frame)
    persistent: RefCell<Vec<CancellablePersistentCallback>>,
    /// Post-frame callbacks (after frame completes)
    post_frame: RefCell<crate::post_frame::PostFrameStorage>,
    /// Microtask queue
    microtasks: RefCell<VecDeque<Box<dyn FnOnce()>>>,
    /// Idle callbacks
    idle: RefCell<Vec<Box<dyn FnOnce()>>>,
    /// Lifecycle state change listeners
    lifecycle_listeners: RefCell<Vec<LifecycleListener>>,
}

/// Application lifecycle and frame timing reporting state.
struct BindingState {
    /// Application lifecycle state
    lifecycle_state: AtomicU8,
    /// Timings callbacks for performance reporting
    timings_callbacks: RefCell<Vec<TimingsCallback>>,
    /// Pending frame timings awaiting report
    pending_timings: RefCell<Vec<FrameTiming>>,
    /// Last timings report time
    last_timings_report: RefCell<Instant>,
}

/// Cross-thread demand and its platform hook contain no UI callback storage.
struct WakeShared {
    scheduler_phase: AtomicU8,
    frames_enabled: AtomicBool,
    frame_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
    frame_scheduled: AtomicBool,
    wake_delivery: crate::wake_delivery::WakeDelivery,
    on_frame_scheduled: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    closed: AtomicBool,
}

/// Every piece of scheduler state, unified behind one allocation.
///
/// Before this type existed, `UpdateScheduler` held independent `Arc` blobs
/// (`frame`/`callbacks`/`binding`/`task_queue`, then also the async driver) — a "handle"
/// in spirit, but a double indirection wherever code wanted a single owning
/// reference to hand out (`create_ticker` allocated a fresh `Arc<UpdateScheduler>`
/// over the five already-`Arc` fields just to give the ticker something to
/// hold). Collapsing them into one `Arc<SchedulerInner>` makes [`UpdateScheduler`]
/// a plain cheap-clone handle over ONE allocation, and — the reason this
/// exists — makes a true, non-owning [`WeakUpdateScheduler`] possible: a `Weak`
/// over five separate Arcs cannot express "this scheduler is gone", only
/// "this one piece of it is gone".
struct SchedulerInner {
    wake: Arc<WakeShared>,
    /// Frame lifecycle and timing
    frame: FrameState,
    /// Callback registration
    callbacks: CallbackState,
    /// Binding integration
    binding: BindingState,
    /// Task queue (priority-based, already internally synchronized)
    task_queue: TaskQueue,
    /// Set while an [OwnerFrame](crate::OwnerFrame) for this scheduler
    /// lives: a scheduler has at most one, so the owner a frame drive polls is
    /// the only one tasks can be admitted to.
    owner_frame_claimed: AtomicBool,
    /// Terminal execution release reports contained cleanup failure to the
    /// caller that still owns its produced output. Ordinary turns use no sink.
    execution_release: RefCell<Option<teardown::ExecutionRelease>>,
}

/// Main scheduler for frame and task management
///
/// Implements phased scheduling:
/// - TransientCallbacks: Animation tickers
/// - PersistentCallbacks: Rendering pipeline
/// - PostFrameCallbacks: Cleanup
///
/// `UpdateScheduler` is a cheap-clone handle over one `Arc<SchedulerInner>`
/// allocation — cloning bumps one refcount, not five. [`UpdateScheduler::downgrade`]
/// vends a [`WeakUpdateScheduler`] for a handle that must not keep a dead UI runtime's
/// scheduler alive (see that type's doc).
///
/// ## Callback Cancellation
///
/// All callback registration methods return a `CallbackId` that can be used to
/// cancel:
///
/// ```rust
/// use flui_scheduler::UpdateScheduler;
///
/// let scheduler = UpdateScheduler::new();
///
/// // Register a callback and get its ID
/// let id = scheduler.schedule_frame_callback(Box::new(|_vsync| {
///     println!("This might be cancelled!");
/// }));
///
/// // Cancel before it fires
/// scheduler.cancel_frame_callback(id);
/// ```
#[derive(Clone)]
pub struct UpdateScheduler {
    inner: Rc<SchedulerInner>,
}

/// A non-owning reference to a [`UpdateScheduler`], obtained via [`UpdateScheduler::downgrade`].
///
/// Exists so a handle that must outlive its scheduler's *owner* (a `Ticker`
/// stored on an `AnimationController`, a `PostFrameHandle` vended to a
/// widget capability) can fail closed instead of keeping the whole scheduler
/// — and everything it owns — alive. [`upgrade`](Self::upgrade) returns
/// `None` once the UI runtime that owns the backing `UpdateScheduler` has dropped its
/// last strong reference; every caller here treats that as "silently done",
/// matching a disposed ticker's own short-circuit.
///
/// `Send + Sync`, same as `UpdateScheduler` — cancellation from a foreign thread
/// keeps working exactly as it did when `Ticker` held a strong `Arc<UpdateScheduler>`.
#[derive(Clone)]
pub struct WeakUpdateScheduler {
    inner: std::rc::Weak<SchedulerInner>,
}

impl UpdateScheduler {
    /// Obtain a non-owning [`WeakUpdateScheduler`] over this scheduler.
    #[must_use]
    pub fn downgrade(&self) -> WeakUpdateScheduler {
        WeakUpdateScheduler {
            inner: Rc::downgrade(&self.inner),
        }
    }
}

impl WeakUpdateScheduler {
    /// Upgrade to a strong [`UpdateScheduler`] handle, or `None` if every strong
    /// reference to the backing scheduler has already been dropped (its
    /// owning UI runtime has torn down).
    #[must_use]
    pub fn upgrade(&self) -> Option<UpdateScheduler> {
        self.inner.upgrade().map(|inner| UpdateScheduler { inner })
    }
}

impl std::fmt::Debug for WeakUpdateScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeakUpdateScheduler")
            .field("alive", &(self.inner.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for UpdateScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Report lock-free state only (atomics + TaskQueue's atomic len);
        // the callback/binding state is opaque `dyn Fn` storage.
        f.debug_struct("UpdateScheduler")
            .field("phase", &self.phase())
            .field("frame_count", &self.frame_count())
            .field(
                "frame_scheduled",
                &self.inner.wake.frame_scheduled.load(Ordering::Acquire),
            )
            .field("task_queue", &self.inner.task_queue)
            .finish_non_exhaustive()
    }
}

/// Asks a scheduler for a frame from any thread.
///
/// Obtained from [`UpdateScheduler::frame_waker`]. `Clone + Send + Sync`:
/// the cross-thread half of a UI runtime's scheduling, which a worker keeps while
/// everything it would wake stays on the owner thread. Holds a `Weak` to its
/// scheduler, so it wakes only its own UI runtime, keeps nothing alive, and is a
/// no-op once the UI runtime is gone.
#[derive(Clone)]
pub struct FrameWaker {
    inner: std::sync::Weak<WakeShared>,
}

impl FrameWaker {
    /// Request a frame.
    ///
    /// Sets the scheduler's frame latch; only its `false → true` edge fires
    /// the platform wake hook, so a burst of requests before the next frame
    /// is one wake. The latch is cleared at the next begin frame, or by
    /// [`UpdateScheduler::finish_async_pump`] for a wake that runs no frame;
    /// a failed hook keeps the demand for the next request or hook
    /// installation.
    ///
    /// # Panics
    ///
    /// Propagates a panic from the platform wake hook after recording the
    /// demand as undelivered.
    pub fn request_frame(&self) {
        if let Some(inner) = self.inner.upgrade() {
            request_frame_impl(&inner);
        }
    }

    /// Requests an enabled visual update, preserving the owner thread's frame
    /// phase gate. Cross-thread requests always remain deliverable.
    pub fn ensure_visual_update(&self) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return false;
        };
        ensure_visual_update_impl(&inner)
    }
}

impl std::fmt::Debug for FrameWaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameWaker")
            .field("alive", &(self.inner.strong_count() > 0))
            .finish()
    }
}

/// Shared body of [`UpdateScheduler::request_frame`] and [`FrameWaker::request_frame`].
///
/// Factored out so both callers can hand it plain field references
/// (`&FrameState`, `&BindingState`) rather than duplicating coalescing logic.
fn request_frame_impl(wake: &WakeShared) {
    request_frame_impl_preserving_failure(wake, false);
}

fn request_frame_impl_preserving_failure(wake: &WakeShared, preserve_failure: bool) {
    if wake.closed.load(Ordering::Acquire) {
        return;
    }
    wake.wake_delivery.request_preserving_failure(
        preserve_failure,
        || !wake.frame_scheduled.swap(true, Ordering::SeqCst),
        || wake.on_frame_scheduled.lock().clone(),
    );
}

fn ensure_visual_update_impl(wake: &WakeShared) -> bool {
    if wake.closed.load(Ordering::Acquire) || !wake.frames_enabled.load(Ordering::Acquire) {
        return false;
    }
    let phase = SchedulerPhase::try_from_u8(wake.scheduler_phase.load(Ordering::Acquire))
        .unwrap_or(SchedulerPhase::Idle);
    match phase {
        SchedulerPhase::Idle | SchedulerPhase::PostFrameCallbacks => {}
        SchedulerPhase::TransientCallbacks
        | SchedulerPhase::MidFrameMicrotasks
        | SchedulerPhase::PersistentCallbacks => {
            let frame_thread = *wake.frame_thread.lock();
            if frame_thread == Some(std::thread::current().id()) {
                return false;
            }
        }
    }
    request_frame_impl(wake);
    true
}

impl UpdateScheduler {
    /// Create a new scheduler with the default configuration
    /// (equivalent to `SchedulerBuilder::new().build()`).
    ///
    /// `UpdateScheduler` makes no refresh-rate assumption of its own — it
    /// never knows a display, a surface, or a frame rate (that split lives
    /// in the presentation-owned `FrameClock`; see the ownership rule in
    /// ADR-0044), and carries no `frame_duration`/`target_fps` field or
    /// accessor. The internal [`FrameBudget`] [`SchedulerBuilder`] seeds is a
    /// stats-window default only (`avg_fps`/jank tracking for a caller who
    /// never configured [`SchedulerBuilder::target_fps`]) — it does not gate
    /// anything. [`Self::drive_frame`]'s `deadline` parameter is the only
    /// thing that bounds work, and it bounds Idle priority tasks alone:
    /// Animation and Build tasks always run.
    pub fn new() -> Self {
        SchedulerBuilder::new().build()
    }

    /// [`SchedulerBuilder::build`]'s constructor seam: `budget_target` seeds
    /// only the internal, stats-only [`FrameBudget`] (see [`Self::new`]'s
    /// doc) — it is not stored anywhere else, and `UpdateScheduler` exposes
    /// no way to read or change it after construction.
    ///
    /// Taking the queue as a parameter avoids needing mutable access to the
    /// constructed `Arc` at all: `Arc::get_mut` fails as soon as any
    /// [`FrameWaker`] or weak handle exists.
    fn with_budget_target_and_task_queue(
        budget_target: FrameDuration,
        task_queue: TaskQueue,
    ) -> Self {
        let target_fps = budget_target.fps() as u32;
        let frame_thread = Arc::new(Mutex::new(None));
        let inner = Rc::new(SchedulerInner {
            wake: Arc::new(WakeShared {
                scheduler_phase: AtomicU8::new(SchedulerPhase::Idle as u8),
                frames_enabled: AtomicBool::new(true),
                frame_thread: Arc::clone(&frame_thread),
                frame_scheduled: AtomicBool::new(false),
                wake_delivery: crate::wake_delivery::WakeDelivery::default(),
                on_frame_scheduled: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
            frame: FrameState {
                current_frame: RefCell::new(None),
                current_vsync_time: RefCell::new(None),
                budget: RefCell::new(FrameBudget::new(target_fps)),
                frame_count: AtomicU64::new(0),
                janky_frame_count: AtomicU64::new(0),
                warm_up_done: AtomicBool::new(false),
                idle_deadline: RefCell::new(None),
                completion_waiters: RefCell::new(FrameCompletionRegistry::new()),
                frame_thread,
            },
            callbacks: CallbackState {
                transient: RefCell::new(VecDeque::new()),
                cancelled: RefCell::new(HashSet::new()),
                id_gen: IdGenerator::new(),
                persistent: RefCell::new(Vec::new()),
                post_frame: RefCell::new(crate::post_frame::PostFrameStorage::new()),
                microtasks: RefCell::new(VecDeque::new()),
                idle: RefCell::new(Vec::new()),
                lifecycle_listeners: RefCell::new(Vec::new()),
            },
            binding: BindingState {
                lifecycle_state: AtomicU8::new(AppLifecycleState::Resumed as u8),
                timings_callbacks: RefCell::new(Vec::new()),
                pending_timings: RefCell::new(Vec::new()),
                last_timings_report: RefCell::new(Instant::now()),
            },
            task_queue,
            owner_frame_claimed: AtomicBool::new(false),
            execution_release: RefCell::new(None),
        });

        Self { inner }
    }

    /// The cross-thread "please run a frame" capability for this scheduler.
    ///
    /// The one scheduler capability a worker thread holds: an
    /// [`OwnerFrame`](crate::OwnerFrame) installs it as its task wakes' frame
    /// hook, and a worker that needs a frame for any other reason sends a
    /// clone. It requests a frame exactly as [`request_frame`](Self::request_frame)
    /// does — one platform wake per `false → true` edge of the frame latch —
    /// wakes this scheduler's UI runtime and no other, and does nothing once the
    /// scheduler is gone.
    #[must_use]
    pub fn frame_waker(&self) -> FrameWaker {
        FrameWaker {
            inner: Arc::downgrade(&self.inner.wake),
        }
    }

    // =========================================================================
    // Phase Management
    // =========================================================================

    /// Get current scheduler phase
    pub fn phase(&self) -> SchedulerPhase {
        // Saturating default to Idle on invalid atomic byte (Principle 6:
        // never panic in production paths). Invalid byte is unreachable in
        // normal operation; this is defensive against memory corruption only.
        SchedulerPhase::try_from_u8(self.inner.wake.scheduler_phase.load(Ordering::Acquire))
            .unwrap_or(SchedulerPhase::Idle)
    }

    /// Set scheduler phase with validation
    fn set_scheduler_phase(&self, new_phase: SchedulerPhase) {
        let current =
            SchedulerPhase::try_from_u8(self.inner.wake.scheduler_phase.load(Ordering::Acquire))
                .unwrap_or(SchedulerPhase::Idle);
        debug_assert!(
            current.can_transition_to(new_phase),
            "Invalid phase transition: {current:?} -> {new_phase:?}"
        );
        self.inner
            .wake
            .scheduler_phase
            .store(new_phase as u8, Ordering::Release);
    }

    pub(crate) fn with_post_frame_registration<R>(
        &self,
        callback: impl FnOnce(CallbackId) -> R,
    ) -> R {
        callback(self.inner.callbacks.id_gen.next())
    }

    pub(crate) fn inner_post_frame_storage(&self) -> &RefCell<crate::post_frame::PostFrameStorage> {
        &self.inner.callbacks.post_frame
    }

    /// Check if currently in a frame
    pub fn is_in_frame(&self) -> bool {
        self.phase().is_in_frame()
    }

    // =========================================================================
    // Frame Scheduling (begin frame / draw frame)
    // =========================================================================

    /// Handle begin frame - called when vsync signal arrives
    ///
    /// Executes transient callbacks (animation tickers) with the vsync
    /// timestamp, flushes microtasks, then polls `owner`'s ready async tasks
    /// in the mid-frame slot.
    ///
    /// # Panics
    ///
    /// Panics before consuming frame demand if `owner` belongs to another scheduler.
    pub fn handle_begin_frame(&self, vsync_time: Instant, owner: &crate::OwnerFrame) -> FrameId {
        assert!(
            owner.belongs_to(self),
            "BUG: frame owner belongs to another scheduler"
        );
        // Store vsync time for all tickers to use
        *self.inner.frame.current_vsync_time.borrow_mut() = Some(vsync_time);

        // Create frame timing with vsync timestamp. `FrameTiming` labels its
        // own stats with the persistent budget's target — the only
        // frame-duration value this scheduler tracks anywhere (see
        // `UpdateScheduler::new`'s doc); this does not gate anything.
        let frame_duration = self.inner.frame.budget.borrow_mut().frame_duration();
        let mut timing = FrameTiming::with_duration(frame_duration);
        timing.start_time = vsync_time;
        timing.phase = FramePhase::Build;

        let frame_id = timing.id;
        *self.inner.frame.current_frame.borrow_mut() = Some(timing);
        self.inner.wake.wake_delivery.consume(|| {
            self.inner
                .wake
                .frame_scheduled
                .store(false, Ordering::Release);
        });
        // Recorded at the same point `frame_scheduled` clears: this thread
        // is now the one driving the frame `ensure_visual_update`'s
        // same-thread arms trust (see `frame_thread`'s own doc). This store
        // MUST happen before the phase transition below takes it out of
        // `Idle` -- a foreign thread that observes a mid-frame phase
        // (`Acquire` on `scheduler_phase`) and then locks `frame_thread`
        // must already observe THIS frame's driver, never a stale one from
        // whatever frame preceded it. Reordered, a thread that drove the
        // PREVIOUS frame could read the new mid-frame phase together with
        // its own (now stale) id still in `frame_thread`, mistake itself
        // for the current driver, and drop a cross-thread wake for the
        // frame actually in flight.
        *self.inner.frame.frame_thread.lock() = Some(std::thread::current().id());
        self.inner.frame.frame_count.fetch_add(1, Ordering::Relaxed);

        // Phase 1: TransientCallbacks (animation tickers)
        self.set_scheduler_phase(SchedulerPhase::TransientCallbacks);

        // Execute transient callbacks (animations get vsync timestamp).
        //
        // Pop one callback at a time and invoke it OUTSIDE the lock -- the
        // same shape `flush_microtasks` uses -- rather than draining the
        // whole queue into a batch before running any of it. A callback
        // that panics then loses only itself: every callback still behind
        // it in `transient` is untouched and runs on the NEXT
        // `handle_begin_frame`, instead of having already been removed from
        // the queue by this frame's own drain and lost with it (issue
        // #1057; see `flui-scheduler/ARCHITECTURE.md`'s mapping entry).
        //
        // Bounded by an id WATERMARK -- the id of the last (back) entry
        // queued at entry, read once before invoking anything -- not a
        // remaining-iteration COUNT. `CallbackId`s are minted from this
        // scheduler's own monotonic `id_gen` in registration order, so
        // `back().id` at entry is exactly the highest id already queued.
        // A callback that registers another transient callback of its own
        // from inside itself must have the new one deferred to the NEXT
        // frame (issue #1058's reentrant-registration contract): the fresh
        // entry always gets a strictly greater id, so `front().id <=
        // watermark` excludes it regardless of where in the queue it lands.
        // A remaining-COUNT bound does not survive cancellation: `A`
        // cancelling a not-yet-run sibling `B` (`cancel_frame_callback`
        // removes `B` from the live queue directly, #1156) and registering
        // a fresh `C` leaves the queue's LENGTH unchanged from `A`'s own
        // perspective, so a count budget still reaches `C` this same call
        // even though `C`'s id exceeds the watermark (issue #1057's own
        // regression, measured: `C` ran in the same frame it was
        // registered in).
        let (watermark, pending) = {
            let cbs = self.inner.callbacks.transient.borrow_mut();
            (cbs.back().map(|c| c.id), cbs.len())
        };
        if let Some(watermark) = watermark {
            tracing::debug!(count = pending, "executing transient callbacks");
            loop {
                let cancellable = {
                    let mut cbs = self.inner.callbacks.transient.borrow_mut();
                    match cbs.front() {
                        Some(front) if front.id <= watermark => cbs.pop_front(),
                        _ => None,
                    }
                };
                let Some(cancellable) = cancellable else {
                    break;
                };

                // Skip if cancelled. DashMap: sharded `RwLock`; `contains_key`
                // releases its shard before returning, so nothing is held
                // across the callback invocation below.
                if self
                    .inner
                    .callbacks
                    .cancelled
                    .borrow()
                    .contains(&cancellable.id)
                {
                    continue;
                }
                (cancellable.callback)(vsync_time);
            }
        }

        // NOTE: Do NOT clear cancelled_callbacks here. Cancellations requested
        // during transient callbacks (e.g. cancelling a post-frame callback)
        // must survive until handle_draw_frame checks them. The single clear()
        // at the end of handle_draw_frame is sufficient.

        // Phase 2: MidFrameMicrotasks
        self.set_scheduler_phase(SchedulerPhase::MidFrameMicrotasks);
        self.flush_microtasks();

        // The async-driver step: exactly one poll per frame in the mid-frame
        // microtask slot, after transient callbacks and before persistent ones in
        // `handle_draw_frame`. A future completing here calls `RebuildHandle::schedule()`,
        // whose id the pipeline's `build_scope` drains — so a completion lands in THIS frame.
        //
        // The begin frame owns the step and takes the owner frame as a parameter,
        // so no frame driver can forget it, run it twice, or poll some other
        // ui_runtime's tasks.
        owner.poll_ready();

        frame_id
    }

    /// Run the frame's **persistent callbacks** and priority task queue.
    ///
    /// Leaves the scheduler in [`SchedulerPhase::PersistentCallbacks`]: the caller's
    /// **pipeline** (build → layout → compositing → paint) occupies that slot next,
    /// and [`end_frame`](Self::end_frame) closes the frame afterwards.
    ///
    /// # This no longer finishes the frame
    ///
    /// This method now leaves the scheduler in `PersistentCallbacks` after running
    /// persistent callbacks. Previously it also drained the post-frame queue and
    /// returned to `Idle`, which meant every post-frame callback ran *before* the
    /// pipeline it was supposed to observe. Use [`drive_frame`](Self::drive_frame),
    /// or pair this with `end_frame`.
    ///
    /// # Panics
    ///
    /// Debug-asserts an illegal phase transition if no frame is open
    /// (`handle_begin_frame` was not called).
    ///
    /// # Idle-slice gating is the only gate
    ///
    /// Animation and Build priority tasks always run to completion here,
    /// unconditionally. Only `Priority::Idle` work is bounded, and only by
    /// the deadline [`drive_frame`](Self::drive_frame) supplies (see the
    /// private `is_idle_deadline_passed` helper below) — a caller driving
    /// this method directly, without going through `drive_frame`, has no
    /// deadline set and Idle work always runs too.
    /// A deadline can defer low-priority work; it can never skip a frame or
    /// starve logical (Animation/Build) work.
    pub fn handle_draw_frame(&self) {
        // Phase 3: PersistentCallbacks (the pipeline's slot)
        self.set_scheduler_phase(SchedulerPhase::PersistentCallbacks);

        // Reset budget at start of rendering
        self.inner.frame.budget.borrow_mut().reset();

        // Execute persistent frame callbacks. Copy FrameTiming once outside the
        // loop to avoid re-locking per callback. Clone callbacks to release the
        // lock before invoking (callbacks may call scheduler methods that take
        // other locks).
        let timing_snapshot = *self.inner.frame.current_frame.borrow_mut();
        if let Some(timing) = timing_snapshot {
            let persistent_callbacks: Vec<_> = {
                let cbs = self.inner.callbacks.persistent.borrow_mut();
                cbs.iter()
                    .filter(|c| !self.inner.callbacks.cancelled.borrow().contains(&c.id))
                    .map(|c| c.callback.clone())
                    .collect()
            };

            for callback in &persistent_callbacks {
                callback(&timing);
            }
        }

        // Animation and Build always run — never gated. Only Idle work is
        // deadline-bounded (see this method's own doc and `drive_frame`).
        //
        // `TaskQueue::execute_until` bounds each call to an id watermark
        // captured once, under its first lock acquisition (see its own
        // doc) — a task enqueued reentrantly during the call always gets a
        // strictly greater id and is left queued for the NEXT call, not
        // this one. A Priority::Animation (or Priority::UserInput) task
        // that itself calls `add_task` with Build-or-higher priority while
        // it runs is therefore invisible to the SAME `execute_until` call
        // it ran inside of: the freshly-queued work would otherwise sit
        // until the *next* frame's `handle_draw_frame`, silently deferred a
        // whole frame for no reason this method's own doc promises (a
        // deadline bounds Idle work only). Loop until a pass finds nothing
        // new, so reentrant Build work still runs THIS frame, one pass
        // later. Bounded: a task that re-enqueues itself every single pass
        // is a task bug (an unbounded reentrant chain), not a reason to
        // hang this frame — cap the passes and trace loudly if the cap is
        // hit, rather than looping forever. That cap depends on
        // `execute_until` actually returning once its own watermarked work
        // is done: a version that re-peeks the LIVE queue instead (issue
        // #1057's own regression) absorbs an unboundedly self-re-enqueuing
        // chain inside ONE call and never returns, so this loop's own pass
        // counter never gets a chance to fire the cap at all.
        let mut reentry_passes = 0usize;
        loop {
            let executed = self.inner.task_queue.execute_until(Priority::Build);
            if executed == 0 {
                break;
            }
            reentry_passes += 1;
            if reentry_passes >= MAX_BUILD_REENTRY_PASSES {
                tracing::warn!(
                    reentry_passes,
                    "Priority::Build (or higher) task queue kept yielding new \
                     work across {MAX_BUILD_REENTRY_PASSES} reentrant drain \
                     passes in one frame -- a task is likely re-enqueuing \
                     itself every pass; stopping here rather than hanging \
                     this frame"
                );
                break;
            }
        }

        if !self.is_idle_deadline_passed() {
            self.inner.task_queue.execute_until(Priority::Idle);
        }
    }

    /// Whether the Idle-slice deadline [`drive_frame`](Self::drive_frame) set
    /// for the current frame has already passed.
    ///
    /// `false` (never passed, i.e. Idle work is never deferred) when no
    /// deadline is set — the state outside a `drive_frame` call, and for a
    /// caller driving `handle_begin_frame`/`handle_draw_frame` by hand.
    fn is_idle_deadline_passed(&self) -> bool {
        self.inner
            .frame
            .idle_deadline
            .borrow_mut()
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Close the frame: run its **post-frame callbacks**, record timing, notify
    /// waiters, and return to [`SchedulerPhase::Idle`].
    ///
    /// Called **after** the pipeline has committed layout and paint, so a
    /// post-frame callback observes this frame's geometry (for example, a
    /// hero transition measuring its endpoints).
    ///
    /// A callback registered *from* a post-frame callback runs on the **next**
    /// frame: the queue is drained into a local buffer before any callback is
    /// invoked, so re-registrations land in the now-empty queue.
    ///
    /// Each callback runs **exactly once** — the queue is drained, not iterated.
    ///
    /// Drains `owner`'s queue in `CallbackId` registration order. Registrations
    /// through [`crate::PostFrameHandle::schedule`] share this same queue. An `owner`
    /// made for another scheduler is not drained (and the mismatch is logged).
    ///
    /// # Panics
    ///
    /// Debug-asserts an illegal phase transition unless the scheduler is in
    /// `PersistentCallbacks` (i.e. `handle_draw_frame` ran). To finish a frame
    /// *without* running its post-frame callbacks, use
    /// [`abort_frame`](Self::abort_frame).
    pub fn end_frame(&self, owner: &crate::OwnerFrame) {
        // Phase 4: PostFrameCallbacks
        self.set_scheduler_phase(SchedulerPhase::PostFrameCallbacks);

        let timing = self.inner.frame.current_frame.borrow_mut().take();

        if let Some(timing) = timing {
            // Record timing and check for jank
            let elapsed = timing.elapsed();
            self.inner
                .frame
                .budget
                .borrow_mut()
                .record_frame_duration(elapsed);

            if timing.is_janky() {
                self.inner
                    .frame
                    .janky_frame_count
                    .fetch_add(1, Ordering::Relaxed);
            }

            // Record timing for batched reporting
            self.inner.binding.pending_timings.borrow_mut().push(timing);

            // Drain BEFORE invoking: a post-frame callback that registers another
            // one must not have it run in this same frame. The owner frame's
            // queue is taken in here, inside the "a frame was actually open"
            // branch, not unconditionally at the top of this function — taking
            // it on a no-open-frame call (no preceding `handle_begin_frame`/
            // `handle_draw_frame`, or a second `end_frame` call with nothing
            // new to close) would silently drop every already-queued local
            // entry un-run. Keeping the take and the invoke in the same branch
            // means a queue that isn't drained this call is simply left alone
            // for whenever a real frame next closes.
            //
            // The take also verifies the owner frame belongs to `self`. One
            // made for a DIFFERENT scheduler must not be drained here: its
            // callbacks would get this frame's `FrameTiming`, be removed
            // before their own scheduler's frame ever runs, and its
            // `CallbackId`s — meaningless in this scheduler's sequence —
            // would corrupt the sort below. On a mismatch the take traces the
            // error and the owner-local queue is left untouched.
            let callback_result = self.dispatch_post_frame_callbacks(owner, &timing);
            if callback_result.is_err() {
                owner.record_execution_failure();
            }

            // Notify frame completion futures. Caught here, alongside
            // `callback_result`, rather than left to propagate bare: a
            // panicking waker is otherwise the exact class of bug issue
            // #1057 exists to close, just on the CLEAN path this time --
            // without this, it would escape straight past the phase reset
            // below and leave the scheduler stuck at `PostFrameCallbacks`
            // with `current_vsync_time` still set, reachable through
            // `drive_frame`'s `Ok` arm rather than its `Err` one.
            let notify_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.notify_frame_completion(
                    FrameOutcome::Completed { timing },
                    callback_result.is_err(),
                    Some(owner.execution_failure_signal()),
                );
            }));

            if callback_result.is_err() || notify_result.is_err() {
                self.inner
                    .wake
                    .scheduler_phase
                    .store(SchedulerPhase::Idle as u8, Ordering::Release);
                *self.inner.frame.current_vsync_time.borrow_mut() = None;
            }
            // The post-frame callback's panic happened first in this frame's
            // own order, so it is what a caller observes when both panicked;
            // `notify_frame_completion` already traced its own panic via
            // `tracing::error!` above, so a discarded notify payload here is
            // not silently lost, only not the one that propagates. Matching
            // on both `Result`s (rather than `Option::or`-ing their errors
            // together) is what routes the DISCARDED side through
            // `discard_panic_payload` instead of an uncontained `drop` --
            // the payload can itself be a type whose own `Drop` panics.
            match (callback_result, notify_result) {
                (Ok(()), Ok(())) => {}
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => {
                    std::panic::resume_unwind(payload);
                }
                (Err(callback_payload), Err(notify_payload)) => {
                    discard_panic_payload(notify_payload);
                    std::panic::resume_unwind(callback_payload);
                }
            }
        }

        // Return to idle
        self.set_scheduler_phase(SchedulerPhase::Idle);
        *self.inner.frame.current_vsync_time.borrow_mut() = None;
    }

    /// Abandon the open frame: return to [`SchedulerPhase::Idle`] **without**
    /// running its post-frame callbacks.
    ///
    /// When a persistent callback panics, the
    /// post-frame loop is skipped but the phase is still reset. The queued
    /// callbacks survive and run on the next completed frame.
    ///
    /// # Who calls this, and why it is not a `Drop` guard
    ///
    /// [`drive_frame`](Self::drive_frame) **does** catch a panic from any phase it
    /// owns — `handle_begin_frame`, `handle_draw_frame`, or the pipeline, all three
    /// under one `catch_unwind` — calls this, then `resume_unwind`s. So this runs
    /// *between* the catch and the resume — the panic payload is already captured
    /// and nothing here executes during unwinding. A caller that drives a frame by
    /// hand (`handle_begin_frame` + `handle_draw_frame`) and panics must call this
    /// itself.
    ///
    /// A `Drop` guard would be wrong twice. It would have to force
    /// `PersistentCallbacks -> Idle`, which
    /// [`can_transition_to`](crate::SchedulerPhase::can_transition_to) forbids, so
    /// the validated setter's `debug_assert!` would fire *while already panicking* —
    /// a double panic, i.e. `abort`. And running any user callback during unwind is a
    /// second hazard for no benefit. Hence an explicit, non-panicking call that
    /// bypasses the phase validator by design: the one sanctioned way out of a
    /// half-open frame.
    ///
    /// Completion waiters **are** notified: an aborted frame is a frame that
    /// finished, badly. Leaving them queued would hang `end_of_frame()` forever.
    /// They resolve with [`FrameOutcome::Aborted`], distinct from
    /// [`FrameOutcome::Completed`] — see that type's own doc for exactly what
    /// the distinction does and does not mean, and this crate's
    /// `ARCHITECTURE.md` `## Mapping decisions` entry for the rationale.
    ///
    /// # `frame_scheduled` is already closed; a catcher that wants another frame
    /// # must ask for one
    ///
    /// By the time any callback this method could be recovering from has even run,
    /// `handle_begin_frame` has already cleared `frame_scheduled` back to `false`
    /// (it does so unconditionally, before the transient-callback loop) — so
    /// [`is_frame_scheduled`](Self::is_frame_scheduled) reads `false` once `abort_frame`
    /// returns, exactly as it would after any other completed frame. This method does
    /// **not** re-arm it: doing so here would hot-loop a caller that keeps re-driving a
    /// deterministically panicking frame. A caller that catches the resumed panic and
    /// decides the show must go on owns calling [`request_frame`](Self::request_frame)
    /// itself before the next platform wake, the same as it would after any other
    /// frame that produced no visible work.
    ///
    /// Idempotent: a no-op when no frame is open.
    pub fn abort_frame(&self) {
        self.abort_frame_impl(false);
    }

    fn abort_frame_impl(&self, preserve_failure: bool) {
        if self.phase() == SchedulerPhase::Idle {
            return;
        }

        let timing = self.inner.frame.current_frame.borrow_mut().take();

        // Raw store: this is the deliberate exception to the forward-only phase
        // machine. `set_scheduler_phase` would `debug_assert!` on
        // `PersistentCallbacks -> Idle`.
        self.inner
            .wake
            .scheduler_phase
            .store(SchedulerPhase::Idle as u8, Ordering::Release);
        *self.inner.frame.current_vsync_time.borrow_mut() = None;
        self.inner.callbacks.cancelled.borrow_mut().clear();

        if let Some(timing) = timing {
            self.notify_frame_completion(FrameOutcome::Aborted { timing }, preserve_failure, None);
        }

        tracing::warn!("frame aborted; its post-frame callbacks were not run");
    }

    /// The one shared frame ordering: **begin → persistent → pipeline → post-frame → idle.**
    ///
    /// Every frame driver goes through here — `HeadlessBinding::pump_frame` on its
    /// binding-local scheduler, and the desktop / android / wasm runners on
    /// the UI runtime's own owned `UpdateScheduler` (`UiRuntime.scheduler`, in flui-app —
    /// there is no process-global scheduler singleton any more) — so
    /// headless and production cannot drift.
    ///
    /// `pipeline` is the binding's build → layout → compositing → paint step. It
    /// runs in the [`SchedulerPhase::PersistentCallbacks`] slot without being
    /// registered as a callback: FLUI's bindings own their element tree by value,
    /// so no `Fn` closure could drive it.
    ///
    /// # `deadline` bounds Idle work only
    ///
    /// `deadline` is a plain instant in time, supplied by the caller — this
    /// scheduler has no refresh-rate or display of its own to derive one
    /// from (that split lives in the presentation-owned `FrameClock`).
    /// Once `deadline` has passed, [`handle_draw_frame`](Self::handle_draw_frame)
    /// stops running `Priority::Idle` tasks for this frame. It can **never**
    /// skip the frame itself, and it can never defer `Priority::Animation`
    /// or `Priority::Build` work — those always run to completion,
    /// regardless of `deadline`. A caller with no meaningful deadline yet
    /// (no `FrameClock` wired in) can pass [`IdleDeadline::far_future`];
    /// Idle work then always runs, matching this method's behavior before
    /// `deadline` existed.
    ///
    /// # A panicking task never leaks a stale deadline
    ///
    /// `deadline` is visible to the private `is_idle_deadline_passed` helper
    /// only for the duration of `handle_begin_frame` + `handle_draw_frame`,
    /// immediately below, guarded by an RAII guard whose `Drop` clears it —
    /// including when it runs *during an unwind*. Without that guard, a
    /// panicking `Priority::Build`/`Animation` task inside
    /// `handle_draw_frame` would unwind straight past a plain
    /// "clear after the call" statement, leaving `Some(already-passed)`
    /// behind forever: every *later* frame — including one driven by hand
    /// via `handle_begin_frame`/`handle_draw_frame` directly, skipping
    /// `drive_frame` entirely, as `HeadlessBinding` does — would then see a
    /// deadline that has always already passed and defer `Priority::Idle`
    /// permanently. The guard closes that leak structurally rather than by
    /// discipline.
    ///
    /// # Errors and panics
    ///
    /// A pipeline that **returns** an error value is a completed frame: `end_frame`
    /// runs, post-frame callbacks fire, exactly once.
    ///
    /// A panic from **any** phase this method drives — a transient callback or the
    /// mid-frame async-driver poll inside `handle_begin_frame`; a persistent
    /// callback or a `Priority::Animation`/`Build`/`Idle` task inside
    /// `handle_draw_frame`; or the pipeline itself — is an abandoned frame. All
    /// three phases run under ONE `catch_unwind`, so the panic is caught no matter
    /// which one raised it, [`abort_frame`](Self::abort_frame) resets the phase
    /// **without running any post-frame callback**, and the panic is then resumed
    /// unchanged. This is issue #1057: earlier, only the pipeline sat inside the
    /// `catch_unwind`, so a panic from `handle_begin_frame`/`handle_draw_frame`
    /// escaped straight past `abort_frame` and left the phase machine, the
    /// `frame_scheduled` latch, and every registered completion waiter stuck at
    /// whatever state that frame happened to reach.
    ///
    /// Every queue this scheduler owns preserves the callbacks/tasks still behind
    /// the panicking entry — see `handle_begin_frame`'s transient-callback loop and
    /// [`TaskQueue::execute_until`](crate::TaskQueue::execute_until)'s own docs — so
    /// only the entry that actually panicked is lost; everything queued after it
    /// runs on the next completed frame, not retried and not silently dropped. A
    /// panicking persistent callback skips the post-frame loop but still resets
    /// the phase. FLUI does not isolate per callback — a panic here poisons
    /// and propagates the whole frame, same as before this issue — this method's
    /// contract is only that the scheduler's OWN bookkeeping is never left
    /// half-closed by it.
    ///
    /// The recovery runs *between* `catch_unwind` and `resume_unwind` — the panic
    /// payload is already captured, so nothing here executes during unwinding, and
    /// no `Drop` guard is involved for THIS part (see `abort_frame` for why a guard
    /// would `abort` the process here specifically — the earlier `IdleDeadlineGuard`
    /// is a different, narrower guard over a plain field, not the phase machine, and
    /// now sits INSIDE this same `catch_unwind`). Without this, a panicking pipeline
    /// would leave the frame open at `PersistentCallbacks` and the *next*
    /// `handle_begin_frame` would attempt the illegal `PersistentCallbacks ->
    /// TransientCallbacks` transition; a panic from an earlier phase left it open at
    /// whichever phase that was instead.
    ///
    /// See [`abort_frame`](Self::abort_frame)'s own doc for the `frame_scheduled`
    /// contract a catcher inherits: closed, not re-armed, so a caller that wants
    /// another frame after catching this one's resumed panic must request it.
    ///
    /// Under `panic = "abort"` nothing is caught and the process dies with the
    /// frame open, which is moot.
    ///
    /// `owner` is the UI runtime's owner-local frame state: its ready tasks are
    /// polled in the mid-frame slot and its post-frame queue drains with the
    /// shared one (see [`end_frame`](Self::end_frame) for the ordering).
    pub fn drive_frame<R>(
        &self,
        owner: &crate::OwnerFrame,
        vsync_time: Instant,
        deadline: IdleDeadline,
        pipeline: impl FnOnce() -> R,
    ) -> R {
        self.drive_frame_impl(owner, vsync_time, deadline, pipeline)
            .1
    }

    /// The shared implementation behind [`drive_frame`](Self::drive_frame) and
    /// [`execute_frame`](Self::execute_frame) — the convenience method routes
    /// through here too (with a no-op `pipeline`) rather than hand-rolling
    /// `handle_begin_frame` + `handle_draw_frame` + `end_frame` directly, so
    /// every caller gets the identical single-`catch_unwind` recovery boundary
    /// this method's own doc describes; a bare sequential call site here would
    /// silently reopen the exact gap issue #1057 closed.
    ///
    /// Returns the [`FrameId`] `handle_begin_frame` minted alongside
    /// `pipeline`'s own result: `drive_frame` discards it to keep its `-> R`
    /// signature, while `execute_frame` discards `R` (always `()` there) and
    /// returns the id instead.
    ///
    /// # The `frame` span
    ///
    /// Opens a `DEBUG` span named `frame` for exactly the stretch the
    /// scheduler counts as "in a frame" -- `handle_begin_frame` through
    /// `end_frame` (or `abort_frame`) -- so the pipeline's `build`, `layout`,
    /// `paint` and `compositing` spans nest inside it. This is the profiler
    /// contract: `flui-devtools`' `FrameTimingLayer` delimits frames by this
    /// name, and its end-to-end test drives this method through
    /// `HeadlessBinding::pump_frame`. Rename it on either side and that test
    /// fails, which is the point of the test.
    fn drive_frame_impl<R>(
        &self,
        owner: &crate::OwnerFrame,
        vsync_time: Instant,
        deadline: IdleDeadline,
        pipeline: impl FnOnce() -> R,
    ) -> (FrameId, R) {
        use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

        assert!(
            owner.belongs_to(self),
            "BUG: frame owner belongs to another scheduler"
        );
        *self.inner.frame.idle_deadline.borrow_mut() = Some(deadline.0);

        // Entered for the whole frame, the panic path included: on a panic
        // the guard drops during unwinding, after `abort_frame`, so the span
        // closes exactly when the frame does either way. The id is recorded
        // once `handle_begin_frame` has minted it.
        let frame_span = tracing::debug_span!("frame", id = tracing::field::Empty);
        let _in_frame = frame_span.enter();

        // ONE recovery boundary over the whole frame lifetime this method
        // owns -- `handle_begin_frame`, `handle_draw_frame`, AND `pipeline`
        // -- not merely `pipeline` alone (issue #1057). A transient
        // callback, a mid-frame async poll, a persistent callback, or a
        // priority task can panic just as the pipeline can, and every one
        // of those phases runs before this frame would otherwise reach
        // `end_frame`; leaving any of them outside this `catch_unwind` means
        // its panic skips `abort_frame` entirely and leaves the phase
        // machine, the `frame_scheduled` latch, and every completion waiter
        // stuck wherever that phase left them.
        let attempt = catch_unwind(AssertUnwindSafe(|| {
            let idle_deadline_guard = IdleDeadlineGuard {
                slot: &self.inner.frame.idle_deadline,
            };

            let frame_id = self.handle_begin_frame(vsync_time, owner);
            frame_span.record("id", tracing::field::debug(&frame_id));
            self.handle_draw_frame();
            // The deadline only ever needs to be visible for this frame's
            // own `handle_draw_frame` call, immediately above; drop the
            // guard now (rather than leaving it to this closure's own end)
            // so a caller that later drives `handle_begin_frame`/
            // `handle_draw_frame` by hand never inherits a stale deadline,
            // and so it is visibly gone before `pipeline` runs. The guard's
            // `Drop` — not this explicit call alone — is what still clears
            // the field if `handle_begin_frame` or `handle_draw_frame`
            // above panics; see this method's own doc.
            drop(idle_deadline_guard);

            (frame_id, pipeline())
        }));

        match attempt {
            Ok((frame_id, result)) => {
                self.end_frame(owner);
                (frame_id, result)
            }
            Err(payload) => {
                // `abort_frame` calls `notify_frame_completion`, which can
                // itself panic (a completion waiter's waker) -- a SECOND,
                // unrelated panic on top of `payload`. Contained here so
                // that panic can never displace the original: without this,
                // `self.abort_frame()` would panic with the waker's payload
                // before `resume_unwind(payload)` below ever ran, and the
                // caller would observe the waker's failure instead of
                // whichever phase actually caused this frame to abort.
                if let Err(secondary_payload) =
                    catch_unwind(AssertUnwindSafe(|| self.abort_frame_impl(true)))
                {
                    crate::completion_wake::retain_reported(
                        secondary_payload,
                        "abort_frame closing an already failed frame",
                    );
                }
                resume_unwind(payload)
            }
        }
    }

    /// Execute a complete frame (convenience method)
    ///
    /// begin → persistent → end, with a no-op pipeline, through the SAME
    /// shared implementation [`drive_frame`](Self::drive_frame) uses — not
    /// a second, hand-rolled `handle_begin_frame` + `handle_draw_frame` +
    /// `end_frame` sequence. Use this for simple cases (warm-up frames,
    /// tests); for proper vsync integration, call `drive_frame` with a real
    /// pipeline.
    ///
    /// Preserves this method's original behavior on the clean path: the
    /// frame completes and its post-frame callbacks run. On the panic path
    /// it now shares `drive_frame`'s recovery contract too (issue #1057):
    /// a transient callback, mid-frame async poll, persistent callback, or
    /// priority task that panics closes the phase/`frame_scheduled`/
    /// completion bookkeeping via `abort_frame` before the panic resumes,
    /// rather than escaping past a bare sequential call site the way it did
    /// before this method routed through the shared implementation.
    #[tracing::instrument(skip(self, owner))]
    pub fn execute_frame(&self, owner: &crate::OwnerFrame) -> FrameId {
        let vsync_time = Instant::now();
        self.drive_frame_impl(
            owner,
            vsync_time,
            IdleDeadline::far_future(vsync_time),
            || {},
        )
        .0
    }

    /// Schedule a warm-up frame (synchronous, no vsync wait)
    ///
    /// This forces an immediate frame to be processed, useful for:
    /// - App initialization
    /// - Reducing first-frame jank
    /// - Forcing immediate layout updates
    #[tracing::instrument(skip(self, owner))]
    pub fn schedule_warm_up_frame(&self, owner: &crate::OwnerFrame) {
        if self.inner.frame.warm_up_done.load(Ordering::Acquire) {
            return;
        }

        // Execute frame immediately without vsync
        self.execute_frame(owner);
        self.inner.frame.warm_up_done.store(true, Ordering::Release);
    }

    // =========================================================================
    // Callback Registration
    // =========================================================================

    /// Schedule a transient frame callback (animation)
    ///
    /// The callback receives the vsync timestamp and fires during
    /// TransientCallbacks phase. This is the correct way for tickers to
    /// receive frame timing.
    ///
    /// Returns a `CallbackId` that can be used to cancel the callback before it
    /// fires.
    ///
    /// # Id order is structural, not incidental
    ///
    /// The id is minted INSIDE the same `transient` lock acquisition as the
    /// push, not before it: `handle_begin_frame`'s id-watermark bound
    /// (issue #1057) assumes the deque's push order and its ids' numeric
    /// order always agree, and `UpdateScheduler` is `Sync` — two threads
    /// racing this method with the mint OUTSIDE the lock could push
    /// `[id6, id5]` (whichever thread reaches the lock first pushes,
    /// regardless of which minted first). `back().id` (5) then sits BELOW
    /// `front().id` (6), so the watermark loop's very first check fails and
    /// NEITHER callback ever runs, every frame, until a later registration
    /// happens to raise the watermark. Minting under the lock makes
    /// whichever thread acquires it first also mint the smaller id first,
    /// structurally, the same discipline `with_post_frame_registration`
    /// already uses for post-frame registration.
    pub fn schedule_frame_callback(&self, callback: OneShotFrameCallback) -> CallbackId {
        let id = {
            let mut cbs = self.inner.callbacks.transient.borrow_mut();
            let id = self.inner.callbacks.id_gen.next();
            cbs.push_back(CancellableTransientCallback { id, callback });
            id
        };
        // Registering a tick demands a frame to run it in. `request_frame`
        // wakes the platform on the false->true transition.
        self.request_frame();
        tracing::debug!("schedule_frame_callback: registered callback id={:?}", id);
        id
    }

    /// Cancel a transient frame callback by ID
    ///
    /// Returns `true` if the callback was found and cancelled, `false` if it
    /// was already executed or not found.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::UpdateScheduler;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let id = scheduler.schedule_frame_callback(Box::new(|_| {
    ///     println!("This won't be called!");
    /// }));
    /// scheduler.cancel_frame_callback(id);
    /// ```
    pub fn cancel_frame_callback(&self, id: CallbackId) -> bool {
        // Locate-then-remove instead of `Vec::retain`'s drop-in-place: `retain`
        // would run the cancelled callback's `Drop` while `transient` is
        // still held, and a capture that re-enters the scheduler (e.g.
        // registering another callback) would deadlock on this same mutex.
        // `position` + `remove` finds the single matching entry (ids are
        // unique, so at most one) with no allocation and no full-Vec
        // move — unlike a partition copy, the common no-match path (every
        // `Ticker::stop`/`dispose` that outlives its callback) costs nothing
        // extra. The removed value is dropped only after the guard falls.
        let removed = {
            let mut callbacks = self.inner.callbacks.transient.borrow_mut();
            callbacks
                .iter()
                .position(|callback| callback.id == id)
                .and_then(|index| callbacks.remove(index))
        };

        if let Some(callback) = removed {
            drop(callback);
            return true; // Found and removed
        }

        // If not found, mark as cancelled (in case it's about to be executed)
        self.inner.callbacks.cancelled.borrow_mut().insert(id);
        false
    }

    /// Request a frame (without callback).
    ///
    /// On the `frame_scheduled` false→true transition this fires the
    /// platform wake hook (see
    /// [`set_on_frame_scheduled`](Self::set_on_frame_scheduled)) so an
    /// idle event loop actually produces the requested frame. While a
    /// frame is already pending the hook stays silent — one wake per
    /// scheduled frame.
    pub fn request_frame(&self) {
        request_frame_impl(&self.inner.wake);
    }

    // =========================================================================
    // Background wakes
    // =========================================================================

    /// Clears the `frame_scheduled` latch for a wake that will call
    /// [`OwnerFrame::poll_ready`](crate::OwnerFrame::poll_ready) WITHOUT a surrounding
    /// [`handle_begin_frame`](Self::handle_begin_frame) — the frames-disabled
    /// `PumpAsync` path (`ADR-0035`'s `wake_action`), which deliberately
    /// never begins/draws a real frame.
    ///
    /// # Why this exists
    ///
    /// `on_frame_scheduled` fires only on `frame_scheduled`'s false→true
    /// transition, and exactly two places ever clear the latch back to
    /// `false`: `handle_begin_frame`, unconditionally at the top of every
    /// real frame, and this method. A `PumpAsync` wake has no frame to do
    /// that clearing —
    /// without this call, a `request_frame()` made before entering
    /// `PumpAsync` (a task's initial spawn, or an earlier wake) leaves the
    /// latch `true` forever: a LATER, independent wake (a network
    /// response's `Waker::wake()`, arriving on a different thread after this
    /// pump cycle already returned) finds the latch already set, never
    /// transitions it, and the hook never fires again — the platform loop is
    /// never told to wake, and the future silently stops advancing with no
    /// visible error.
    ///
    /// # Call before, not after, the poll
    ///
    /// Call this immediately **before** `OwnerFrame::poll_ready`, mirroring
    /// `handle_begin_frame`'s clear-at-the-top order — not after. A polled
    /// future may legitimately re-arm itself synchronously (call
    /// `Waker::wake_by_ref` from inside its own `poll`, wanting to be polled
    /// again next cycle); that call sets `frame_scheduled` back to `true`
    /// *during* the poll. Clearing the latch again *afterward*
    /// would silently erase that signal — the exact starvation this method
    /// exists to prevent, just for a self-waking task instead of an
    /// externally-woken one.
    ///
    /// # Clearing the latch must not strand a live `end_of_frame` waiter
    ///
    /// A pending [`end_of_frame`](Self::end_of_frame) waiter IS frame
    /// demand: registering issues one, and every later registration stays
    /// silent while that waiter is live. This method clears the latch
    /// without draining the completion registry, so a naive clear here
    /// would revoke that demand with the waiter still queued, and nothing
    /// short of a frames-enabled edge re-issuing a demand it never recorded
    /// as lost would ever wake it again (issue #1162). Reachable, but not
    /// off a runner's own decide-then-pump order: every `flui-app` runner
    /// calls this method only from `wake_action`'s `PumpAsync` arm, chosen
    /// iff `!frames_enabled` at that same read, with this call following
    /// immediately on the same thread and no application code in between
    /// (`frame_pacing.rs`'s `wake_action`) — so a register → disable →
    /// enable → pump order can never land here with `frames_enabled` back
    /// to `true`. What DOES reach this method's live-waiter re-check with
    /// `frames_enabled` true: this `pub fn` called directly, out of
    /// `wake_action`'s order (an embedder or a test bypassing it), or a
    /// cross-thread enable landing in the store-buffering window the next
    /// section names. Even on that reachable path, a plain disable→enable
    /// edge in between would not help on its own: `frame_scheduled` was
    /// already latched `true` from the waiter's own registration and never
    /// cleared by disabling frames, so `request_frame_impl`'s own
    /// false→true edge does not fire on re-enable and the hook stays
    /// silent — this method's own re-check is what recovers it.
    ///
    /// So this method re-checks for a live waiter itself, at the exact point
    /// it would otherwise drop the demand, rather than trusting an earlier
    /// edge to have covered it: if frames are enabled and the completion
    /// registry still holds a live entry once the latch is clear, it
    /// re-issues the demand right here. This method is `pub` and takes
    /// `&self` on a `Clone + Send + Sync` scheduler, so an embedder can
    /// reach it from any thread; call it only from the wake arm it
    /// documents above.
    ///
    /// # A store-buffering hazard makes `SeqCst` load-bearing here
    ///
    /// This method's `frame_scheduled` swap and `frames_enabled` read race a
    /// frames-enabled edge's own `frames_enabled` swap and (via
    /// `request_frame_impl`) `frame_scheduled` swap, on another thread, with
    /// no reads-from edge between the two calls to synchronize on. Under
    /// `Acquire`/`Release` alone, "this thread's load of `frames_enabled`
    /// still sees the OLD value AND the edge thread's swap of
    /// `frame_scheduled` still sees the OLD value too" is a legal outcome —
    /// a real store-buffering effect on hardware like x86, where a plain
    /// store can sit in the executing core's store buffer while a
    /// subsequent plain load on the SAME thread already executes — not
    /// merely a hypothetical one. Any single race instance touches four
    /// accesses: this method's swap and load, plus whichever edge fired
    /// (`set_frames_enabled`'s or `handle_app_lifecycle_state_change`'s
    /// `frames_enabled` swap) and `request_frame_impl`'s `frame_scheduled`
    /// swap. `SeqCst` marks all five code sites -- both edge swaps count
    /// separately even though only one of them runs per race instance --
    /// which puts every access on one global total order and rules that
    /// outcome out. No single-threaded test can redden a reversion of this;
    /// only a `loom` model could prove it, and none exists yet.
    ///
    /// Reentrant demand defers delivery to the outer hook invocation, with
    /// at most one compensating attempt; hooks must not drive a frame inline.
    pub fn finish_async_pump(&self) {
        self.inner.wake.wake_delivery.consume(|| {
            self.inner
                .wake
                .frame_scheduled
                .swap(false, Ordering::SeqCst);
        });

        // The `completion_waiters` guard `has_live_waiter()` takes is a
        // temporary of this `if`'s condition, so it drops at the end of the
        // condition, before the block below runs -- `request_frame()` is
        // never reached with it still held, honoring this crate's lock
        // order (`completion_waiters` never held across `request_frame`,
        // which fires the wake hook synchronously).
        if self.inner.wake.frames_enabled.load(Ordering::SeqCst)
            && self
                .inner
                .frame
                .completion_waiters
                .borrow_mut()
                .has_live_waiter()
        {
            self.request_frame();
        }
    }

    /// Install the platform wake hook and retry any undelivered demand.
    ///
    /// The hook runs on whichever thread schedules the frame and may run
    /// while callers hold their own locks — it must only touch wake machinery,
    /// never drive a frame inline. Reentrant demand is coalesced without recursion.
    ///
    /// "Whichever thread" is not a formality: a task waker (through the owner
    /// frame's [`FrameWaker`]) fires this from whatever thread completed the
    /// future, which for a job on
    /// `ExecutionServices`' IO lane is a pool worker. This doc used to offer
    /// `request_redraw` as the example of acceptable wake machinery, and
    /// `flui-app`'s `FrameWakeHandle` — the hook production installs — calls it
    /// directly. On macOS that messages the NSWindow's content view, while
    /// `MacOSWindow`'s `unsafe impl Send` justifies itself with "the NSWindow
    /// pointer is only messaged from the main thread"; that backend is compiled
    /// by CI and never executed, so nothing disagrees. Issue #949 carries the
    /// chain and the fix. Until it lands, a NEW hook should set a flag or push
    /// onto a channel the owner thread drains rather than reach a platform API
    /// from here.
    ///
    /// The *previous* hook is dropped only once this lock is released: if
    /// its `Arc` is the last reference, `Drop` runs whatever the captured
    /// closure's state destructs — user code, which may call back into the
    /// scheduler (this method again, or anything that reads the hook, such
    /// as `request_frame_impl`) and must not deadlock on this mutex.
    pub fn set_on_frame_scheduled(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        let previous = { std::mem::replace(&mut *self.inner.wake.on_frame_scheduled.lock(), hook) };
        drop(previous);
        self.inner.wake.wake_delivery.request(
            || false,
            || self.inner.wake.on_frame_scheduled.lock().clone(),
        );
    }

    /// Add a persistent frame callback.
    ///
    /// Fires every frame during PersistentCallbacks phase. Use for the
    /// rendering pipeline (build/layout/paint).
    ///
    /// Persistent frame callbacks cannot be unregistered: once registered,
    /// they are called for every frame for the lifetime of the application.
    /// Returns `()` — no removal handle.
    pub fn add_persistent_frame_callback(&self, callback: RecurringFrameCallback) {
        let id = self.inner.callbacks.id_gen.next();
        self.inner
            .callbacks
            .persistent
            .borrow_mut()
            .push(CancellablePersistentCallback { id, callback });
    }

    /// Add a post-frame callback.
    ///
    /// Fires once after the current/next frame completes.
    ///
    /// Post-frame callbacks are called exactly once and cannot be
    /// cancelled before they fire. Returns `()` — no cancellation handle.
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) {
        let _ = crate::PostFrameHandle::new(self).schedule(callback);
    }

    // =========================================================================
    // Microtask Queue
    // =========================================================================

    /// Schedule a microtask
    ///
    /// Microtasks are executed during MidFrameMicrotasks phase,
    /// after animations but before rendering.
    pub fn schedule_microtask(&self, task: Box<dyn FnOnce()>) {
        self.inner.callbacks.microtasks.borrow_mut().push_back(task);
    }

    /// One bounded pass over the microtask queue: run up to as many
    /// microtasks as were queued when this pass began, one at a time,
    /// outside the queue's own lock.
    ///
    /// Mirrors [`TaskQueue::execute_until`](crate::task::TaskQueue::execute_until)'s
    /// count-budget shape: the budget is `queue.len()` read once, under the
    /// first lock acquisition, and decremented once per pop, so a microtask
    /// enqueued reentrantly DURING this pass gets no budget of its own here
    /// -- it is left queued for the NEXT pass rather than absorbed into an
    /// unbounded live re-peek of the queue.
    ///
    /// Returns the number of microtasks executed.
    fn flush_microtasks_pass(&self) -> usize {
        let mut budget = self.inner.callbacks.microtasks.borrow_mut().len();
        let mut executed = 0usize;
        while budget > 0 {
            let task = self.inner.callbacks.microtasks.borrow_mut().pop_front();
            let Some(task) = task else {
                break;
            };
            budget -= 1;
            task();
            executed += 1;
        }
        executed
    }

    /// Flush all pending microtasks.
    ///
    /// Bounded by [`MAX_MICROTASK_REENTRY_PASSES`] outer passes, mirroring
    /// the Build/Animation reentrant-pass loop in
    /// [`handle_draw_frame`](Self::handle_draw_frame): a microtask enqueued
    /// reentrantly by another one in the SAME pass is invisible to
    /// [`flush_microtasks_pass`](Self::flush_microtasks_pass) and instead
    /// runs in the NEXT pass, one pass later -- nested-microtask
    /// semantics are preserved for a chain no deeper than the cap (a
    /// microtask scheduled from inside another still runs before THIS flush
    /// call returns), just spread over one extra pass rather than a
    /// live re-peek. **This is only true up to the cap.** A FINITE chain
    /// deeper than [`MAX_MICROTASK_REENTRY_PASSES`] passes -- 33 levels of
    /// nesting, say, with no genuinely unbounded re-enqueuing anywhere in
    /// it -- is deferred exactly like an unbounded one: whatever the cap
    /// leaves queued waits for the NEXT `flush_microtasks` call (the
    /// following frame's `handle_begin_frame`), not this one. This bounds
    /// reentrancy *depth*, not *width*: a pass whose every popped microtask
    /// enqueues two more still finishes in [`MAX_MICROTASK_REENTRY_PASSES`]
    /// passes, not because the fan-out is bounded, but because passes are
    /// (mirrors [`MAX_BUILD_REENTRY_PASSES`]'s own caveat). The cap cannot
    /// tell a genuinely unbounded, self-re-enqueuing chain apart from a
    /// merely deep finite one that is still queuing work when the cap
    /// trips -- both pay the same one-frame deferral and the same warning
    /// at the same depth, and only the unbounded case would otherwise hang
    /// the frame (issue #1159). A finite chain that happens to land
    /// EXACTLY on the cap boundary -- its last level runs on the very pass
    /// that trips the threshold and enqueues nothing further -- is not in
    /// that group: the queue is empty afterward, nothing was deferred, and
    /// [`flush_microtasks`](Self::flush_microtasks) does not warn for it.
    ///
    /// A capped flush leaves its unrun leftovers queued, requesting nothing
    /// of its own: [`schedule_microtask`](Self::schedule_microtask) never
    /// calls `request_frame` (unlike `TaskQueue::add` for Build-or-higher
    /// priority), so a leftover simply waits for the next frame's
    /// `handle_begin_frame` to flush it -- unchanged, pre-existing behavior,
    /// not something this cap adds.
    ///
    /// Hitting the pass count and having something left to warn about are
    /// two different facts: a chain exactly [`MAX_MICROTASK_REENTRY_PASSES`]
    /// levels deep empties the queue on the very pass that trips the
    /// threshold, so nothing was actually cut off, and this does not warn
    /// -- only a nonempty queue after the capped pass means real deferred
    /// work. (The Build/Animation cap in
    /// [`handle_draw_frame`](Self::handle_draw_frame) shares this same
    /// unconditional-on-count shape; narrowing it the same way is out of
    /// scope here.)
    fn flush_microtasks(&self) {
        let mut passes = 0usize;
        loop {
            let executed = self.flush_microtasks_pass();
            if executed == 0 {
                break;
            }
            passes += 1;
            if passes >= MAX_MICROTASK_REENTRY_PASSES {
                let deferred = self.inner.callbacks.microtasks.borrow_mut().len();
                if deferred > 0 {
                    tracing::warn!(
                        passes,
                        deferred,
                        "microtask queue kept yielding new work across \
                         {MAX_MICROTASK_REENTRY_PASSES} reentrant flush passes in one frame -- \
                         a microtask is likely re-enqueuing itself every pass; stopping here \
                         rather than hanging this frame"
                    );
                }
                break;
            }
        }
    }

    // =========================================================================
    // Task Queue
    // =========================================================================

    /// Add a task with priority
    pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + 'static) {
        self.inner.task_queue.add(priority, callback);
    }

    /// Get task queue reference
    pub fn task_queue(&self) -> &TaskQueue {
        &self.inner.task_queue
    }

    // =========================================================================
    // Vsync Timestamp
    // =========================================================================

    /// Get current vsync timestamp (if in frame)
    pub fn current_vsync_time(&self) -> Option<Instant> {
        *self.inner.frame.current_vsync_time.borrow_mut()
    }

    // =========================================================================
    // Frame State
    // =========================================================================

    /// Check if a frame is scheduled
    pub fn is_frame_scheduled(&self) -> bool {
        self.inner.wake.frame_scheduled.load(Ordering::Acquire)
    }

    /// Get the number of pending transient callbacks
    ///
    /// This is useful for debugging and testing to verify that
    /// all transient callbacks have been processed.
    pub fn transient_callback_count(&self) -> usize {
        self.inner.callbacks.transient.borrow_mut().len()
    }

    /// Get current frame timing (if a frame is active)
    pub fn current_frame(&self) -> Option<FrameTiming> {
        *self.inner.frame.current_frame.borrow_mut()
    }

    /// Set the current frame phase (for rendering pipeline)
    ///
    /// The `current_frame` guard is held for this whole call — it is a
    /// plain field write, never a callback invocation — and must stay that
    /// way: this is exactly the shape `handle_begin_frame`'s retired legacy
    /// callback loop got wrong (issue #1058), where the lock outlived a
    /// `callback(timing)` call and a callback reading `current_frame()`
    /// deadlocked on itself. No user code may run inside this method's
    /// locked scope.
    pub fn set_phase(&self, phase: FramePhase) {
        if let Some(timing) = self.inner.frame.current_frame.borrow_mut().as_mut() {
            timing.phase = phase;
        }
    }

    // =========================================================================
    // Budget and Timing
    // =========================================================================
    //
    // These read the frame's *labeled* target duration (see `UpdateScheduler::new`'s
    // doc) for informational stats only — none of them gate anything. The
    // only thing that ever bounds work here is the `deadline` a caller
    // passes to `drive_frame`, and it bounds `Priority::Idle` alone; see
    // `handle_draw_frame`'s doc for the actual gate.

    /// Check if currently over budget (informational only — does not gate).
    pub fn is_over_budget(&self) -> bool {
        self.inner
            .frame
            .current_frame
            .borrow_mut()
            .as_ref()
            .is_some_and(super::frame::FrameTiming::is_over_budget)
    }

    /// Check if deadline is near, i.e. >80% of the labeled target duration
    /// used (informational only — does not gate; see this section's doc).
    pub fn is_deadline_near(&self) -> bool {
        self.inner
            .frame
            .current_frame
            .borrow_mut()
            .as_ref()
            .is_some_and(super::frame::FrameTiming::is_deadline_near)
    }

    /// Get remaining budget as type-safe Milliseconds
    pub fn remaining_budget(&self) -> Milliseconds {
        self.inner
            .frame
            .current_frame
            .borrow_mut()
            .as_ref()
            .map_or(Milliseconds::ZERO, super::frame::FrameTiming::remaining)
    }

    /// Get remaining budget in milliseconds (raw f64)
    pub fn remaining_budget_ms(&self) -> f64 {
        self.remaining_budget().value()
    }

    /// Snapshot the current frame budget statistics.
    ///
    /// Returns an owned [`FrameBudget`] value rather than a live lock guard
    /// — the caller cannot deadlock the scheduler by holding this past its
    /// own scope. It is one small allocation, not free (`FrameBudget` carries
    /// a bounded rolling-window frame-time history), and the returned value
    /// is a stats snapshot only: it does not gate anything (see
    /// [`drive_frame`](Self::drive_frame) and
    /// [`handle_draw_frame`](Self::handle_draw_frame) for the actual
    /// Idle-slice deadline gate).
    #[must_use]
    pub fn budget_snapshot(&self) -> FrameBudget {
        self.inner.frame.budget.borrow_mut().clone()
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total frame count
    pub fn frame_count(&self) -> u64 {
        self.inner.frame.frame_count.load(Ordering::Relaxed)
    }

    /// Get average FPS from budget statistics
    pub fn avg_fps(&self) -> f64 {
        self.inner.frame.budget.borrow_mut().avg_fps()
    }

    /// Check if last frame was janky
    pub fn is_janky(&self) -> bool {
        self.inner.frame.budget.borrow_mut().is_janky()
    }

    /// Get count of janky frames
    pub fn janky_frame_count(&self) -> u64 {
        self.inner.frame.janky_frame_count.load(Ordering::Relaxed)
    }

    /// Get jank rate as percentage
    pub fn jank_rate(&self) -> f64 {
        let total = self.frame_count();
        if total == 0 {
            0.0
        } else {
            (self.janky_frame_count() as f64 / total as f64) * 100.0
        }
    }

    /// Clear jank statistics
    pub fn clear_jank_stats(&self) {
        self.inner
            .frame
            .janky_frame_count
            .store(0, Ordering::Relaxed);
    }

    // =========================================================================
    // Lifecycle State Management
    // =========================================================================

    /// Get the current application lifecycle state
    ///
    /// Returns the current state of the application as seen by the platform.
    pub fn lifecycle_state(&self) -> AppLifecycleState {
        AppLifecycleState::try_from_u8(self.inner.binding.lifecycle_state.load(Ordering::Acquire))
            .unwrap_or(AppLifecycleState::Detached)
    }

    /// Handle a lifecycle state change from the platform
    ///
    /// This should be called by the platform integration when the app
    /// lifecycle state changes. It will:
    /// 1. Update the internal state
    /// 2. Notify all registered listeners
    /// 3. Adjust frame scheduling behavior accordingly, re-scheduling a
    ///    frame on the disabled→enabled edge
    ///
    /// # Thread affinity
    ///
    /// Listener callbacks fire synchronously, on whatever thread calls this
    /// method — there is no dispatch/queueing here. Production callers are
    /// expected to already be on the UI runtime's owner thread (the platform
    /// event-loop thread that drives `flui-app`'s UI runtime dispatch); this
    /// method does not itself verify that, since the scheduler has no
    /// notion of "UI runtime" or "owner thread" to assert against. A caller with
    /// that context cheaply available should assert it before calling in.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::{AppLifecycleState, UpdateScheduler};
    ///
    /// let scheduler = UpdateScheduler::new();
    ///
    /// // Platform notifies app is going to background
    /// scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    ///
    /// // Check if we should render
    /// if !scheduler.lifecycle_state().should_render() {
    ///     // Skip frame rendering
    /// }
    /// ```
    #[tracing::instrument(skip(self))]
    pub fn handle_app_lifecycle_state_change(&self, new_state: AppLifecycleState) {
        // Atomically swap state and get old value
        let old_state = AppLifecycleState::try_from_u8(
            self.inner
                .binding
                .lifecycle_state
                .swap(new_state as u8, Ordering::AcqRel),
        )
        .unwrap_or(AppLifecycleState::Detached);

        // Auto-toggle frames_enabled. Resumed/Inactive keep rendering active
        // (Inactive means visible-but-unfocused — split screen, modal — still
        // needs to draw). Hidden/Paused/Detached disable the frame loop.
        let should_render = matches!(
            new_state,
            AppLifecycleState::Resumed | AppLifecycleState::Inactive
        );
        // `SeqCst`, not `AcqRel`: this swap and `finish_async_pump`'s own
        // `frame_scheduled` swap+`frames_enabled` read form a
        // store-buffering (Dekker/SB) litmus pair across two threads with
        // no reads-from edge between them -- see `finish_async_pump`'s own
        // doc for the full argument. `AcqRel` alone does not close it.
        let frames_were_enabled = self
            .inner
            .wake
            .frames_enabled
            .swap(should_render, Ordering::SeqCst);

        // Schedule a frame on exactly the disabled→enabled edge, not on every
        // transition that happens to leave frames enabled. Without this leg,
        // an app that was Hidden/Paused/Detached and comes back to
        // Resumed/Inactive never wakes: nothing else re-requests a frame
        // that was never scheduled while frames were off, so the pipeline
        // sits idle until some unrelated event happens to nudge it.
        if !frames_were_enabled && should_render {
            self.request_frame();
        }

        // Only notify if state actually changed
        if old_state != new_state {
            // Notify listeners (clone to avoid holding lock during callbacks)
            let listeners = {
                let listeners = self.inner.callbacks.lifecycle_listeners.borrow_mut();
                listeners
                    .iter()
                    .map(|l| l.callback.clone())
                    .collect::<Vec<_>>()
            };

            for callback in listeners {
                callback(new_state);
            }
        }
    }

    /// Add a lifecycle state change listener
    ///
    /// Returns a `CallbackId` that can be used to remove the listener.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::rc::Rc;
    ///
    /// use flui_scheduler::{AppLifecycleState, UpdateScheduler};
    ///
    /// let scheduler = UpdateScheduler::new();
    ///
    /// let id = scheduler.add_lifecycle_state_listener(Rc::new(|state| {
    ///     println!("App lifecycle changed to: {}", state);
    /// }));
    ///
    /// // Later, remove the listener
    /// scheduler.remove_lifecycle_state_listener(id);
    /// ```
    pub fn add_lifecycle_state_listener(
        &self,
        callback: Rc<dyn Fn(AppLifecycleState)>,
    ) -> CallbackId {
        let id = self.inner.callbacks.id_gen.next();
        self.inner
            .callbacks
            .lifecycle_listeners
            .borrow_mut()
            .push(LifecycleListener { id, callback });
        id
    }

    /// Remove a lifecycle state change listener by ID
    ///
    /// Returns `true` if the listener was found and removed.
    pub fn remove_lifecycle_state_listener(&self, id: CallbackId) -> bool {
        // Same hazard `cancel_frame_callback` guards against: `retain` would
        // drop the removed listener's `Arc<dyn Fn(..)>` (possibly the last
        // reference, running the captured state's `Drop`) while
        // `lifecycle_listeners` is still locked. A listener whose capture
        // re-enters this scheduler (adding or removing another listener) on
        // drop would deadlock. `position` + `remove` finds the single
        // matching entry (ids are unique) with no allocation and no full-Vec
        // move on the common no-match path, and the removed value is
        // dropped only after the guard falls.
        let removed = {
            let mut listeners = self.inner.callbacks.lifecycle_listeners.borrow_mut();
            listeners
                .iter()
                .position(|listener| listener.id == id)
                .map(|index| listeners.remove(index))
        };

        let found = removed.is_some();
        drop(removed);
        found
    }

    /// Check if frames should be scheduled based on lifecycle state
    ///
    /// Returns `false` when the app is hidden, paused, or detached. Thin
    /// alias over [`frames_enabled`](Self::frames_enabled) — the single
    /// atomic `handle_app_lifecycle_state_change` already maintains, rather
    /// than re-deriving the same fact from `lifecycle_state()`.
    pub fn should_schedule_frame(&self) -> bool {
        self.frames_enabled()
    }

    // =========================================================================
    // Frame Completion Futures
    // =========================================================================

    /// Returns a future that completes when the current or next frame ends
    ///
    /// This is useful for scheduling work that should happen after the frame
    /// completes, such as:
    /// - Waiting for layout to be finalized
    /// - Scheduling post-frame cleanup
    /// - Coordinating with async operations
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use flui_scheduler::{FrameOutcome, UpdateScheduler};
    ///
    /// async fn wait_for_frame(scheduler: &UpdateScheduler) {
    ///     // Wait for the current/next frame to complete
    ///     if let Ok(FrameOutcome::Completed { timing, .. }) = scheduler.end_of_frame().await {
    ///         println!(
    ///             "Frame {} completed in {}ms",
    ///             timing.id.get(),
    ///             timing.elapsed().value()
    ///         );
    ///     }
    /// }
    /// ```
    ///
    /// # Demand
    ///
    /// Registering **is** the demand: this schedules a frame on the 0 → 1
    /// transition of the live-waiter set, the same rule Compose's
    /// `BroadcastFrameClock` states for `onNewAwaiters` and the web's
    /// `requestAnimationFrame` implies. So an idle scheduler wakes for this
    /// call, and N registrations inside one frame still cost at most one
    /// wake (`request_frame`'s own false→true swap edge coalesces them).
    ///
    /// The demand honors `frames_enabled`: a scheduler whose owner disabled
    /// frames is not forced awake, and a future registered there resolves
    /// only once frames come back — see
    /// [`set_frames_enabled`](Self::set_frames_enabled).
    ///
    /// Two further consequences worth knowing before calling this:
    ///
    /// * A pending waiter is real frame demand, so it suppresses
    ///   [`execute_idle_callbacks`](Self::execute_idle_callbacks) until the
    ///   frame runs.
    /// * A panicking or absent wake hook leaves durable delivery debt. A later
    ///   frame request or hook installation retries it without discarding waiters.
    ///
    /// # Panics
    ///
    /// Propagates a panic from the `on_frame_scheduled` hook, which this
    /// call reaches whenever it issues a demand. See
    /// [`set_on_frame_scheduled`](Self::set_on_frame_scheduled) for that
    /// hook's contract.
    #[must_use = "registering a completion waiter schedules a frame whether or not the \
                  future is awaited; drop it to cancel"]
    pub fn end_of_frame(&self) -> FrameCompletionFuture {
        let future = FrameCompletionFuture::new();

        let needs_demand = {
            let mut registry = self.inner.frame.completion_waiters.borrow_mut();
            registry.compact_if_due();

            // The predicate is LIVE entries, not `is_empty()`. A tombstone
            // left by a cancelled waiter says nothing about whether a frame
            // is still coming: `frame_scheduled` has a second clearer
            // besides `handle_begin_frame` — the public `finish_async_pump`
            // — so a demand can be revoked with no drain, and `is_empty()`
            // would then be sound only through an untested coupling across
            // the registry, that latch, and `frames_enabled`.
            //
            // `has_live_waiter` reads only the vec already under this
            // guard, and is amortized O(1) rather than O(len): it retires
            // the tombstones it walks past instead of re-walking them on
            // every registration. The cost matters because this runs under
            // the registry mutex, so it blocks registration AND completion.
            //
            // What this buys is the ISSUANCE half of the invariant: after a
            // drain the vec is empty, so the first push demands, and every
            // later push either sees a live entry whose demand postdates
            // that drain or demands itself. SURVIVAL of an issued demand is
            // not decidable here at all -- only `set_frames_enabled`'s
            // enable edge can re-issue one that was revoked.
            let had_live_waiter = registry.has_live_waiter();

            // Register BEFORE demanding, so the registration is the
            // linearization point: demand-first would let a concurrent
            // frame both begin and drain in between, leaving this waiter to
            // miss the frame it just paid for and buy a redundant next one.
            // The ordering is recorded in this crate's ARCHITECTURE.md.
            registry.waiters.push(future.notifier());

            !had_live_waiter
        };
        // The guard is released HERE, before the demand. The hook
        // `schedule_frame_if_enabled` can reach must find every scheduler
        // mutex free, `completion_waiters` included.
        if needs_demand {
            self.schedule_frame_if_enabled();
        }

        future
    }

    /// Internal: notify all frame completion waiters.
    ///
    /// Each waiter's [`Waker`] is taken out from under its own
    /// `notifier.state` lock and called only after that lock is released.
    /// An inline-polling waker — one whose `wake()` immediately re-polls
    /// this SAME [`FrameCompletionFuture`], rather than merely scheduling a
    /// later poll — calls `FrameCompletionFuture::poll`, which locks that
    /// identical `Arc<Mutex<FrameCompletionState>>`; held across the
    /// `wake()` call, that self-relock on the non-reentrant
    /// `parking_lot::Mutex` never returns (issue #1057).
    ///
    /// A waker that panics is caught and traced rather than aborting the
    /// loop: every OTHER waiter still gets its `completed` value set and
    /// its own waker called. The first such panic is re-raised
    /// (`resume_unwind`) only after every waiter has been notified, so a
    /// caller still observes it — matching `end_frame_impl`'s own
    /// catch-then-resume shape for a panicking post-frame callback, just
    /// upstream of it here. A SECOND (or later) waker's panic cannot be
    /// re-raised too -- `resume_unwind` takes one payload -- so it is routed
    /// through [`discard_panic_payload`] instead, retaining the opaque payload
    /// without invoking its possibly-panicking destruction.
    fn notify_frame_completion(
        &self,
        outcome: FrameOutcome,
        preserve_failure: bool,
        failure_signal: Option<&Cell<bool>>,
    ) {
        let waiters = self.inner.frame.completion_waiters.borrow_mut().drain();

        let mut delivery = if let Some(signal) = failure_signal {
            crate::completion_wake::WakeBatch::with_failure_signal(
                "frame completion",
                preserve_failure,
                signal,
            )
        } else {
            crate::completion_wake::WakeBatch::new("frame completion", preserve_failure)
        };
        for notifier in waiters {
            // `upgrade()` at loop-body scope, never inside the
            // `completion_waiters` block above: the temporary `Arc` it
            // produces may be the LAST strong reference by the time this
            // iteration ends, and dropping it then runs
            // `FrameCompletionState::drop` -- caller-owned `Waker` teardown,
            // which the lock order forbids under either mutex.
            //
            // A failed upgrade means the future was dropped, i.e. the wait
            // was CANCELLED. That is an ordinary outcome, not an error, and
            // must stay untraced: it would log at frame rate.
            let Some(state) = notifier.state.upgrade() else {
                continue;
            };

            let waker = {
                let mut state = state.lock();
                state.completed = Some(Ok(outcome));
                // Taken, not read: with the waker moved out, the temporary
                // `Arc`'s own drop at the end of this iteration has no
                // caller code left to run even when it is the last
                // reference.
                state.waker.take()
            };
            let Some(waker) = waker else { continue };

            // A legal and deliberate interleaving: another thread may poll
            // this future between the guard release just above and this
            // `wake()`, observe `completed`, resolve, and drop the task --
            // so this can wake a waker whose task is already gone. Waking
            // after completion is explicitly permitted by `Waker`'s own
            // contract. Do not "fix" it by holding the guard across the
            // wake; that is issue #1057's deadlock.
            delivery.wake(waker);
        }
        delivery.finish(true);
    }

    // =========================================================================
    // Binding Methods (formerly on SchedulerBinding trait)
    // =========================================================================

    /// Get the current scheduler phase (alias for `phase()`)
    pub fn scheduler_phase(&self) -> SchedulerPhase {
        self.phase()
    }

    /// Check if a frame has been scheduled (alias for `is_frame_scheduled()`)
    pub fn has_scheduled_frame(&self) -> bool {
        self.is_frame_scheduled()
    }

    /// Check whether frame scheduling is enabled
    pub fn frames_enabled(&self) -> bool {
        self.inner.wake.frames_enabled.load(Ordering::Acquire)
    }

    /// Enable or disable frame scheduling.
    ///
    /// The disabled → enabled edge re-requests a frame, as
    /// [`handle_app_lifecycle_state_change`](Self::handle_app_lifecycle_state_change)
    /// does. A demand issued while frames
    /// were disabled is silently dropped by
    /// [`schedule_frame_if_enabled`](Self::schedule_frame_if_enabled) with
    /// nothing recording the loss, and a pending
    /// [`end_of_frame`](Self::end_of_frame) waiter left behind that way
    /// waits forever, because every later registration sees it still live
    /// and stays silent. Only a frames-enabled edge can re-issue such a
    /// demand.
    ///
    /// **This setter is the API-surface mirror, not the production
    /// carrier.** It has no production callers; the edge a running app
    /// actually crosses is `handle_app_lifecycle_state_change`'s resume
    /// leg, which has always re-requested and is pinned by
    /// `lifecycle_reenable_edge_schedules_exactly_one_frame`. The
    /// re-request is here so that an embedder toggling frames through the
    /// public setter cannot reach a stranded state the lifecycle path
    /// recovers from.
    pub fn set_frames_enabled(&mut self, enabled: bool) {
        // `SeqCst`: see `finish_async_pump`'s doc for the store-buffering
        // hazard this ordering closes against that method's own swap+load.
        let was_enabled = self
            .inner
            .wake
            .frames_enabled
            .swap(enabled, Ordering::SeqCst);

        if !was_enabled && enabled {
            self.request_frame();
        }
    }

    /// Schedule a frame if frames are enabled.
    ///
    /// Unlike `request_frame()`, this checks `frames_enabled` first. Returns
    /// `true` iff a frame was actually requested (frames were enabled);
    /// `false` when the demand was dropped by the enablement gate.
    pub fn schedule_frame_if_enabled(&self) -> bool {
        if self.inner.wake.frames_enabled.load(Ordering::Acquire) {
            self.request_frame();
            true
        } else {
            false
        }
    }

    /// Schedule a forced frame (ignores `frames_enabled`)
    pub fn schedule_forced_frame(&self) {
        self.request_frame();
    }

    /// Ensure a visual update is scheduled.
    ///
    /// [`SchedulerPhase::Idle`] and
    /// [`SchedulerPhase::PostFrameCallbacks`] request one, through
    /// [`schedule_frame_if_enabled`](Self::schedule_frame_if_enabled) (which
    /// keeps the `frames_enabled` gate); a call from `PostFrameCallbacks`
    /// requests the NEXT frame, since this frame's own pipeline has already
    /// run by that phase. The three mid-frame phases
    /// ([`SchedulerPhase::TransientCallbacks`],
    /// [`SchedulerPhase::MidFrameMicrotasks`],
    /// [`SchedulerPhase::PersistentCallbacks`]) are a no-op **only for the
    /// thread already driving this frame** (see `frame_thread`'s own doc).
    /// A caller on any OTHER thread always requests, regardless of phase:
    /// `UpdateScheduler` is `Send + Sync` and documented as reachable from
    /// any thread, so a lost cross-thread wake would be the worst failure
    /// class this crate names, and only the driving thread's own later
    /// phases are guaranteed to observe what prompted a same-thread call.
    ///
    /// This is not a blanket "the in-flight frame will pick up your
    /// change" promise even for the driving thread: it holds for
    /// `TransientCallbacks`/`MidFrameMicrotasks`, which precede the
    /// pipeline in the frame's slot order, but not for
    /// `PersistentCallbacks`, where the pipeline itself runs — a call made
    /// after paint has already happened would have its demand dropped.
    /// (A raw `handle_begin_frame`/`handle_draw_frame`
    /// sequence outside `drive_frame_impl`'s panic boundary whose callback
    /// panics leaves the phase stuck exactly where it panicked, and this
    /// gate stays silent on that thread until something resets the phase
    /// machine; production always drives frames through
    /// `drive_frame`/`drive_frame_with_lane`, both of which wrap
    /// `drive_frame_impl`.)
    ///
    /// # Return value
    ///
    /// Returns `true` iff a frame was actually requested — the phase gate
    /// passed *and* frames were enabled — and `false` iff the demand was
    /// dropped (the same-thread mid-frame no-op, or `frames_enabled ==
    /// false`). It is **not** the `frame_scheduled` false→true edge: a
    /// caller whose demand coincides with an already-scheduled frame still
    /// gets `true`, because the gate passed. A caller that wants to poke a
    /// per-window redraw (the presentation's `set_on_need_visual_update`
    /// closure) uses this to poke only when the scheduler is actually going
    /// to produce the frame, so a mid-frame same-thread dirty mark no longer
    /// reaches `request_redraw`.
    ///
    /// Spelled out as a `match` with every phase named, not a wildcard arm,
    /// so a phase added to [`SchedulerPhase`] fails to compile here until
    /// it is classified.
    pub fn ensure_visual_update(&self) -> bool {
        ensure_visual_update_impl(&self.inner.wake)
    }

    /// Get the current system frame timestamp
    pub fn current_system_frame_time_stamp(&self) -> Instant {
        self.current_vsync_time().unwrap_or_else(Instant::now)
    }

    /// Add a timings callback for receiving frame performance reports
    pub fn add_timings_callback(&self, callback: TimingsCallback) {
        self.inner
            .binding
            .timings_callbacks
            .borrow_mut()
            .push(callback);
    }

    /// Remove a timings callback
    pub fn remove_timings_callback(&self, callback: &TimingsCallback) {
        // Same hazard as `cancel_frame_callback`/`remove_lifecycle_state_listener`.
        // `extract_if` (stable since Rust 1.87) removes every matching entry
        // in one pass with no full-Vec reallocation or copy of the kept
        // entries, and the removed `Arc<TimingsCallback>`s are dropped only
        // once `timings_callbacks`'s guard has already fallen, not while a
        // re-entrant drop could deadlock on it. Remove-all, not
        // remove-first: unlike the two `CallbackId`-addressed sites above,
        // nothing here guarantees a caller registers a given `Arc` only once.
        let removed: Vec<_> = {
            let mut callbacks = self.inner.binding.timings_callbacks.borrow_mut();
            callbacks
                .extract_if(.., |existing| Rc::ptr_eq(existing, callback))
                .collect()
        };
        drop(removed);
    }

    /// Report pending frame timings to registered callbacks.
    ///
    /// Timings are batched and reported approximately once per second in
    /// release mode, or every ~100ms in debug/profile builds. Call this
    /// from the event loop to flush pending timings.
    ///
    /// Returns the number of timings reported.
    pub fn report_timings(&self) -> usize {
        let timings: Vec<_> = {
            let mut pending = self.inner.binding.pending_timings.borrow_mut();
            if pending.is_empty() {
                return 0;
            }
            pending.drain(..).collect()
        };

        let callbacks = self.inner.binding.timings_callbacks.borrow_mut().clone();
        let count = timings.len();

        for callback in &callbacks {
            callback(&timings);
        }

        *self.inner.binding.last_timings_report.borrow_mut() = Instant::now();
        count
    }

    /// Get the time since the last timings report was sent
    pub fn time_since_last_timings_report(&self) -> Duration {
        self.inner
            .binding
            .last_timings_report
            .borrow_mut()
            .elapsed()
    }

    /// Debug assert: no transient callbacks are pending
    ///
    /// Returns `true` if there are no pending transient callbacks.
    pub fn debug_assert_no_transient_callbacks(&self, _reason: &str) -> bool {
        self.inner.callbacks.transient.borrow_mut().is_empty()
    }

    // =========================================================================
    // Idle Callbacks
    // =========================================================================

    /// Schedule a callback to run when the scheduler is idle.
    ///
    /// Idle callbacks execute when:
    /// 1. No frame is scheduled
    /// 2. The task queue is empty
    /// 3. The app is in `Resumed` state
    ///
    /// The event loop calls `execute_idle_callbacks()` when it has no other
    /// work.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::UpdateScheduler;
    ///
    /// let scheduler = UpdateScheduler::new();
    ///
    /// scheduler.schedule_idle_callback(|| {
    ///     // Do background cleanup work
    /// });
    /// ```
    pub fn schedule_idle_callback(&self, callback: impl FnOnce() + 'static) {
        self.inner
            .callbacks
            .idle
            .borrow_mut()
            .push(Box::new(callback));
    }

    /// Execute all pending idle callbacks.
    ///
    /// Called by the event loop when the scheduler has no other work to do.
    /// Only executes if the scheduler is idle, the task queue is empty,
    /// and the app is in `Resumed` state.
    ///
    /// Returns the number of callbacks executed.
    pub fn execute_idle_callbacks(&self) -> usize {
        // Only run idle callbacks when truly idle
        if self.is_frame_scheduled() || !self.inner.task_queue.is_empty() {
            return 0;
        }
        if !self.lifecycle_state().should_render() {
            return 0;
        }

        let callbacks: Vec<_> = {
            let mut cbs = self.inner.callbacks.idle.borrow_mut();
            cbs.drain(..).collect()
        };

        let count = callbacks.len();
        for callback in callbacks {
            callback();
        }
        count
    }

    /// Check if there are pending idle callbacks.
    pub fn has_idle_callbacks(&self) -> bool {
        !self.inner.callbacks.idle.borrow_mut().is_empty()
    }
}

impl Default for UpdateScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating a scheduler with custom configuration.
///
/// `budget_target` configures ONLY the internal, stats-only [`FrameBudget`]
/// (`avg_fps`/jank tracking via [`UpdateScheduler::budget_snapshot`]) — the
/// scheduler itself never gates on it, and `UpdateScheduler` exposes no way
/// to read it back. This is the sole place in `flui-scheduler` a fixed
/// frame-rate default is declared; see [`UpdateScheduler::new`]'s doc.
#[derive(Debug, Clone)]
pub struct SchedulerBuilder {
    budget_target: FrameDuration,
    task_queue_capacity: Option<usize>,
}

impl SchedulerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            budget_target: FrameDuration::try_from_fps(60).expect("BUG: 60 fps is always valid"),
            task_queue_capacity: None,
        }
    }

    /// Set the stats budget's target FPS (see this type's own doc — this
    /// does not configure a scheduler-wide rate).
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.budget_target = FrameDuration::try_from_fps(fps).expect("fps > 0");
        self
    }

    /// Set the stats budget's target duration directly (see this type's own
    /// doc — this does not configure a scheduler-wide rate).
    pub fn frame_duration(mut self, duration: FrameDuration) -> Self {
        self.budget_target = duration;
        self
    }

    /// Set task queue capacity
    pub fn task_queue_capacity(mut self, capacity: usize) -> Self {
        self.task_queue_capacity = Some(capacity);
        self
    }

    /// Build the scheduler
    pub fn build(self) -> UpdateScheduler {
        let task_queue = self
            .task_queue_capacity
            .map_or_else(TaskQueue::new, TaskQueue::with_capacity);

        UpdateScheduler::with_budget_target_and_task_queue(self.budget_target, task_queue)
    }
}

impl Default for SchedulerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// The lock-discipline / reentrant-registration / lock-then-drop test family
// (issue #1058) lives in its own file: it is one cohesive slice built on a
// single shared oracle that nothing else in this file uses, and together
// with that oracle it outgrew this file's already-large inline test module.
// A sibling module rather than a nested one, so its tests sit beside `tests`
// rather than two segments below it.
#[cfg(test)]
mod lock_discipline_tests;

#[cfg(test)]
mod tests {

    // =========================================================================
    // Async Driver Integration
    // =========================================================================

    /// #1162: a live `end_of_frame` waiter whose demand survives a
    /// disable->enable edge with `frame_scheduled` already latched `true`
    /// must not be silently dropped by a LATER `PumpAsync` cycle that clears
    /// that same latch. The disable->enable edge itself fires no NEW wake
    /// here: `frame_scheduled` was already `true` from the waiter's own
    /// registration and disabling frames never clears it, so
    /// `request_frame_impl`'s own false->true edge does not occur on
    /// re-enable -- which is exactly the trap this pins:
    /// `finish_async_pump` must re-check for a live waiter itself rather
    /// than trust that earlier, silent edge to have covered it.
    ///
    /// Not asserted before the `finish_async_pump()` call: this test pins
    /// that THAT call is what re-issues the demand, not the disable/enable
    /// sequence on its own.
    ///
    /// Red-check (reverted `finish_async_pump`): `(is_frame_scheduled(),
    /// hook_fires) == (false, 1)`, not `(true, 2)`.
    fn finish_async_pump_reissues_a_stranded_live_waiters_demand() {
        let mut scheduler = UpdateScheduler::new();
        let hook_fires = Arc::new(AtomicU64::new(0));
        let hook_fires_for_hook = Arc::clone(&hook_fires);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            hook_fires_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        // Bound, not `let _ =`: an unbound future drops immediately, and a
        // dropped waiter is no longer live -- the whole point being pinned
        // here needs it to survive past this statement.
        let _waiter = scheduler.end_of_frame();
        assert_eq!(hook_fires.load(Ordering::Relaxed), 1);

        scheduler.set_frames_enabled(false);
        scheduler.set_frames_enabled(true);
        assert_eq!(
            hook_fires.load(Ordering::Relaxed),
            1,
            "the re-enable edge must fire no NEW wake here -- frame_scheduled was already \
             true from the waiter's own registration, so request_frame_impl's own \
             false->true edge does not occur on this re-enable"
        );

        scheduler.finish_async_pump();

        assert_eq!(
            (
                scheduler.is_frame_scheduled(),
                hook_fires.load(Ordering::Relaxed)
            ),
            (true, 2),
            "finish_async_pump must re-issue the stranded waiter's demand itself, rather \
             than trust the disable->enable edge to have covered it"
        );
    }

    use super::*;

    /// The platform wake hook fires exactly once per `frame_scheduled`
    /// false→true transition: re-requesting a pending frame stays
    /// silent, and `handle_begin_frame` re-arms the edge. A ticker that
    /// re-registers its callback during the frame therefore wakes the
    /// platform for the NEXT frame — the self-sustaining animation loop.
    fn frame_scheduled_hook_fires_once_per_transition() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let scheduler = UpdateScheduler::new();
        let fired = Arc::new(AtomicUsize::new(0));
        let fired_hook = Arc::clone(&fired);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            fired_hook.fetch_add(1, Ordering::SeqCst);
        })));

        scheduler.request_frame();
        scheduler.request_frame();
        assert_eq!(
            fired.load(Ordering::SeqCst),
            1,
            "a pending frame must not re-wake the platform",
        );

        // A frame runs; during it a ticker re-registers (transient
        // callback) — the cleared edge fires the hook again.
        scheduler.handle_begin_frame(
            Instant::now(),
            &crate::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame"),
        );
        scheduler.schedule_frame_callback(Box::new(|_| {}));
        scheduler.handle_draw_frame();
        assert_eq!(
            fired.load(Ordering::SeqCst),
            2,
            "re-registration after begin_frame must wake the next frame",
        );
    }

    /// A microtask that unconditionally re-enqueues itself must not hang the
    /// frame: `flush_microtasks`'s outer pass loop bounds it to
    /// `MAX_MICROTASK_REENTRY_PASSES` passes, warning once, rather than
    /// looping until the queue empties -- which, for a chain like this one,
    /// it never would (issue #1159). Mirrors
    /// `a_self_reenqueuing_build_task_is_bounded_by_the_reentry_cap_not_hung_forever`
    /// in `tests/update_scheduler_reshape.rs`, but lives here because
    /// `MAX_MICROTASK_REENTRY_PASSES` is crate-private.
    ///
    /// Unlike the Build cap, the run count is exactly the cap, not
    /// `+ 1`: `flush_microtasks` has no trailing, unconditional second sweep
    /// the way `handle_draw_frame` does for `Priority::Idle`.
    fn a_self_reenqueuing_microtask_is_bounded_by_the_reentry_cap_not_hung_forever() {
        use std::sync::atomic::AtomicUsize;

        let scheduler = UpdateScheduler::new();
        let runs = Arc::new(AtomicUsize::new(0));

        // A named fn, not a closure capturing itself: each execution
        // re-enqueues one more instance of itself, unconditionally.
        fn requeue(scheduler: UpdateScheduler, runs: Arc<AtomicUsize>) {
            let next_scheduler = scheduler.clone();
            let next_runs = Arc::clone(&runs);
            scheduler.schedule_microtask(Box::new(move || {
                next_runs.fetch_add(1, Ordering::SeqCst);
                requeue(next_scheduler, next_runs);
            }));
        }
        requeue(scheduler.clone(), Arc::clone(&runs));

        let (_frame_id, log) = flui_testing::log_capture::capture(|| {
            scheduler.execute_frame(
                &crate::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame"),
            )
        });

        assert_eq!(
            runs.load(Ordering::SeqCst),
            MAX_MICROTASK_REENTRY_PASSES,
            "the reentry cap bounds the microtask flush loop to exactly \
             MAX_MICROTASK_REENTRY_PASSES executions -- unlike the Build cap, nothing \
             sweeps up a trailing leftover afterward"
        );
        assert_eq!(
            log.count_containing("reentrant flush"),
            1,
            "the reentry-cap warning must fire exactly once: {log}"
        );
    }

    // Callback Cancellation Tests

    // Lifecycle State Tests

    /// The disabled→enabled edge must actually schedule a frame,
    /// through the real `request_frame` path so `on_frame_scheduled` fires —
    /// otherwise a resumed app never wakes an idle event loop.
    fn lifecycle_reenable_edge_schedules_exactly_one_frame() {
        let scheduler = UpdateScheduler::new();
        let wakes = Arc::new(AtomicU64::new(0));
        let wakes_for_hook = Arc::clone(&wakes);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            wakes_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        // Go to Hidden first: frames_enabled false->false transition here
        // (Resumed -> Hidden) does not schedule (frames are being disabled,
        // not enabled), and consumes no wake.
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert!(!scheduler.is_frame_scheduled());

        // Hidden -> Resumed is the disabled->enabled edge: exactly one wake.
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(
            wakes.load(Ordering::Relaxed),
            1,
            "re-enabling frames after Hidden must schedule exactly one frame"
        );
        assert!(scheduler.is_frame_scheduled());
    }

    // Frame Completion Future Tests

    /// Counts `wake()` calls; shared by several tests below that assert
    /// exactly how many times a stored waker fired.
    struct CountingWaker(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A value whose own `Drop` panics; shared by the tests below that prove
    /// a discarded panic payload's secondary panic-on-drop is contained
    /// rather than left to escape.
    struct PoisonPill;

    impl Drop for PoisonPill {
        fn drop(&mut self) {
            panic!("poison pill dropped");
        }
    }

    /// A waker that panics must not starve waiters registered after it: the
    /// panic is caught, logged, and re-raised only once every waiter has
    /// been notified (issue #1057).
    fn notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics() {
        use std::sync::atomic::AtomicUsize;
        use std::task::Wake;

        struct PanicWaker;
        impl Wake for PanicWaker {
            fn wake(self: Arc<Self>) {
                panic!("waker probe");
            }
        }

        let scheduler = UpdateScheduler::new();
        let mut future_a = scheduler.end_of_frame();
        let mut future_b = scheduler.end_of_frame();

        let panic_waker = Waker::from(Arc::new(PanicWaker));
        let mut cx_a = Context::from_waker(&panic_waker);
        assert!(Pin::new(&mut future_a).poll(&mut cx_a).is_pending());

        let woken_b = Arc::new(CountingWaker(AtomicUsize::new(0)));
        let counting_waker = Waker::from(Arc::clone(&woken_b));
        let mut cx_b = Context::from_waker(&counting_waker);
        assert!(Pin::new(&mut future_b).poll(&mut cx_b).is_pending());

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scheduler.execute_frame(
                &crate::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame"),
            );
        }));
        assert!(unwind.is_err(), "the waker's own panic must propagate");
        assert_eq!(
            unwind
                .as_ref()
                .err()
                .and_then(|payload| flui_foundation::panic::payload_text(&**payload)),
            Some("waker probe"),
            "the original waker panic, not a secondary one"
        );

        assert_eq!(
            woken_b.0.load(Ordering::SeqCst),
            1,
            "the second waiter must still be woken despite the first waker panicking"
        );
        assert_eq!(
            scheduler.phase(),
            SchedulerPhase::Idle,
            "end_frame_impl catches notify_frame_completion's own panic alongside the \
             post-frame callback's, so the phase reset still runs on this clean-pipeline \
             path (issue #1057) instead of leaving the scheduler stuck at PostFrameCallbacks"
        );
    }

    // ── SchedulerInner teardown (#1162) ─────────────────────────────────────

    /// A pending `end_of_frame()` waiter must not hang forever just because
    /// the scheduler that would have resolved it is gone: dropping the last
    /// strong `UpdateScheduler` handle must resolve every registered waiter
    /// with `Err(SchedulerClosed)` and wake it exactly once.
    fn scheduler_drop_resolves_a_pending_waiter_with_scheduler_closed() {
        use std::sync::atomic::AtomicUsize;

        let scheduler = UpdateScheduler::new();
        let mut future = scheduler.end_of_frame();
        let woken = Arc::new(CountingWaker(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&woken));
        assert!(
            Pin::new(&mut future)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );

        drop(scheduler);

        assert_eq!(
            woken.0.load(Ordering::SeqCst),
            1,
            "teardown must wake the waiter exactly once"
        );

        let resolved = Pin::new(&mut future).poll(&mut Context::from_waker(&waker));
        let Poll::Ready(outcome) = resolved else {
            panic!("a dropped scheduler must resolve every pending waiter, not leave it Pending");
        };
        assert!(
            matches!(outcome, Err(SchedulerClosed)),
            "must resolve Err(SchedulerClosed), got {outcome:?}"
        );
    }

    /// The same shape one level up: `end_frame_impl`'s own
    /// `callback_result`/`notify_result` merge must discard a panicking
    /// notify payload through `discard_panic_payload`, not an uncontained
    /// `drop`, when the post-frame callback ALSO panicked and so is what
    /// actually propagates.
    fn end_frame_impl_survives_a_panicking_notify_payloads_own_drop_panic() {
        use std::task::Wake;

        struct PoisonWaker;
        impl Wake for PoisonWaker {
            fn wake(self: Arc<Self>) {
                std::panic::panic_any(PoisonPill);
            }
        }

        let scheduler = UpdateScheduler::new();
        let mut future = scheduler.end_of_frame();
        let waker = Waker::from(Arc::new(PoisonWaker));
        assert!(
            Pin::new(&mut future)
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );

        scheduler.add_post_frame_callback(Box::new(|_timing| panic!("post-frame probe")));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scheduler.execute_frame(
                &crate::OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame"),
            );
        }));

        let payload = unwind.expect_err("the post-frame callback's panic must still propagate");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some("post-frame probe"),
            "the callback panicked first in this frame's own order, so its payload -- not \
             the notify step's poison-pill one -- must be what escapes"
        );
        assert_eq!(
            scheduler.phase(),
            SchedulerPhase::Idle,
            "the phase reset must still run even though discarding the notify side's own \
             panic-on-drop payload could itself have panicked"
        );
    }

    #[test]
    fn scheduler_frame_contract_matrix() {
        crate::table_test::run_table(
            "scheduler_frame_contract_matrix",
            &[
                (
                    "finish_async_pump_reissues_a_stranded_live_waiters_demand",
                    finish_async_pump_reissues_a_stranded_live_waiters_demand as fn(),
                ),
                (
                    "frame_scheduled_hook_fires_once_per_transition",
                    frame_scheduled_hook_fires_once_per_transition as fn(),
                ),
                (
                    "a_self_reenqueuing_microtask_is_bounded_by_the_reentry_cap_not_hung_forever",
                    a_self_reenqueuing_microtask_is_bounded_by_the_reentry_cap_not_hung_forever
                        as fn(),
                ),
                (
                    "lifecycle_reenable_edge_schedules_exactly_one_frame",
                    lifecycle_reenable_edge_schedules_exactly_one_frame as fn(),
                ),
                (
                    "notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics",
                    notify_frame_completion_still_wakes_a_later_waiter_when_an_earlier_waker_panics
                        as fn(),
                ),
                (
                    "scheduler_drop_resolves_a_pending_waiter_with_scheduler_closed",
                    scheduler_drop_resolves_a_pending_waiter_with_scheduler_closed as fn(),
                ),
                (
                    "end_frame_impl_survives_a_panicking_notify_payloads_own_drop_panic",
                    end_frame_impl_survives_a_panicking_notify_payloads_own_drop_panic as fn(),
                ),
            ],
        );
    }

    // Idle Callback Tests

    // =========================================================================
    // Teardown pins (scheduler ui_runtime-ownership) — `WeakUpdateScheduler` must
    // fail closed once its backing scheduler's last strong reference drops,
    // never observe stale state, and never panic.
    // =========================================================================
}
