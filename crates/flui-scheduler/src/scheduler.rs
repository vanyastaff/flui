//! Main scheduler - coordinates frame lifecycle and task execution
//!
//! The UpdateScheduler is the central orchestrator for FLUI's rendering pipeline,
//! following Flutter's scheduler model with proper phase separation.
//!
//! ## Frame Lifecycle (Flutter-like)
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
//! use flui_scheduler::{Priority, UpdateScheduler};
//!
//! let scheduler = UpdateScheduler::new();
//!
//! // Schedule animation callback (fires during TransientCallbacks)
//! scheduler.schedule_frame_callback(Box::new(|vsync_time| {
//!     // Animation tick - all tickers get same vsync timestamp
//! }));
//!
//! // Add rendering callback (fires during PersistentCallbacks)
//! scheduler.add_persistent_frame_callback(std::sync::Arc::new(|timing| {
//!     // Run build/layout/paint pipeline
//! }));
//!
//! // Execute a frame (typically called by event loop on vsync)
//! let vsync_time = web_time::Instant::now();
//! scheduler.handle_begin_frame(vsync_time);
//! scheduler.handle_draw_frame();
//! ```

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

use dashmap::DashMap;
use parking_lot::Mutex;
use web_time::{Duration, Instant};

use crate::{
    budget::FrameBudget,
    config::{
        PerformanceMode, PerformanceModeRequestHandle, TimingsCallback, adjust_duration_for_epoch,
        time_dilation,
    },
    duration::{FrameDuration, Milliseconds},
    frame::{
        AppLifecycleState, FrameCallback, FrameId, FramePhase, FrameTiming, OneShotFrameCallback,
        PostFrameCallback, RecurringFrameCallback, SchedulerPhase,
    },
    id::{CallbackId, IdGenerator},
    post_frame::{LocalPostFrameEntry, OwnerPostFrameCallback},
    task::{Priority, TaskQueue},
    ticker::TickerProvider,
};

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
const MAX_BUILD_REENTRY_PASSES: usize = 32;

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

/// Cancellable post-frame callback with ID
struct CancellablePostFrameCallback {
    id: CallbackId,
    callback: PostFrameCallback,
}

/// Lifecycle state listener with ID for removal
struct LifecycleListener {
    id: CallbackId,
    callback: Arc<dyn Fn(AppLifecycleState) + Send + Sync>,
}

/// Shared state for frame completion future
struct FrameCompletionState {
    /// Completed frame timing (Some if frame is done)
    completed: Option<FrameTiming>,
    /// Waker to notify when frame completes
    waker: Option<Waker>,
}

/// Future that resolves when a frame completes
///
/// This is returned by `UpdateScheduler::end_of_frame()` and allows awaiting
/// the completion of the current or next frame.
///
/// # Example
///
/// ```rust,no_run
/// use flui_scheduler::UpdateScheduler;
///
/// async fn do_end_of_frame_work(scheduler: &UpdateScheduler) {
///     // Wait for frame to complete
///     let timing = scheduler.end_of_frame().await;
///
///     // Now safe to do post-frame cleanup
///     println!(
///         "Frame {} completed in {}ms",
///         timing.id.get(),
///         timing.elapsed().value()
///     );
/// }
/// ```
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
    type Output = FrameTiming;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock();

        if let Some(timing) = state.completed.take() {
            Poll::Ready(timing)
        } else {
            // Store waker for notification when frame completes
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl FrameCompletionFuture {
    /// Create a new frame completion future
    fn new() -> (Self, Arc<Mutex<FrameCompletionState>>) {
        let state = Arc::new(Mutex::new(FrameCompletionState {
            completed: None,
            waker: None,
        }));
        (
            Self {
                state: state.clone(),
            },
            state,
        )
    }
}

/// Pending frame completion notifier
struct FrameCompletionNotifier {
    state: Arc<Mutex<FrameCompletionState>>,
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
    slot: &'a Mutex<Option<Instant>>,
}

impl Drop for IdleDeadlineGuard<'_> {
    fn drop(&mut self) {
        *self.slot.lock() = None;
    }
}

/// Frame lifecycle and timing state (atomics + guarded fields)
struct FrameState {
    /// Current scheduler phase
    scheduler_phase: AtomicU8,
    /// Current frame timing
    current_frame: Mutex<Option<FrameTiming>>,
    /// VSync timestamp for current frame
    current_vsync_time: Mutex<Option<Instant>>,
    /// Per-phase timing statistics against a caller-chosen target framerate.
    /// This is the only place `flui-scheduler` tracks a frame-duration
    /// value at all — it is stats-only (`UpdateScheduler::budget_snapshot`,
    /// `avg_fps`, `is_janky`), never a gate. There is deliberately no
    /// separate `frame_duration`/`target_fps` field or accessor on
    /// `UpdateScheduler` itself; see `UpdateScheduler::new`'s doc.
    budget: Mutex<FrameBudget>,
    /// Whether a frame is currently scheduled
    frame_scheduled: AtomicBool,
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
    idle_deadline: Mutex<Option<Instant>>,
    /// Pending frame completion futures
    completion_waiters: Mutex<Vec<FrameCompletionNotifier>>,
}

/// Callback registration and cancellation state
struct CallbackState {
    /// Linearizes shared/local post-frame registration with frame snapshots.
    post_frame_registration: Mutex<()>,
    /// Transient callbacks - animation tickers
    transient: Mutex<Vec<CancellableTransientCallback>>,
    /// Cancelled callback IDs (lock-free)
    cancelled: DashMap<CallbackId, ()>,
    /// Callback ID generator
    id_gen: IdGenerator<flui_foundation::markers::FrameCallback>,
    /// Legacy frame callbacks
    frame: Mutex<Vec<FrameCallback>>,
    /// Persistent frame callbacks (every frame)
    persistent: Mutex<Vec<CancellablePersistentCallback>>,
    /// Post-frame callbacks (after frame completes)
    post_frame: Mutex<Vec<CancellablePostFrameCallback>>,
    /// Microtask queue
    microtasks: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
    /// Idle callbacks
    idle: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    /// Lifecycle state change listeners
    lifecycle_listeners: Mutex<Vec<LifecycleListener>>,
}

/// Binding integration state (performance, timings, epoch)
struct BindingState {
    /// Whether frame scheduling is enabled
    frames_enabled: AtomicBool,
    /// Application lifecycle state
    lifecycle_state: AtomicU8,
    /// Epoch start for time dilation
    epoch_start: Mutex<Duration>,
    /// Timings callbacks for performance reporting
    timings_callbacks: Mutex<Vec<TimingsCallback>>,
    /// Pending frame timings awaiting report
    pending_timings: Mutex<Vec<FrameTiming>>,
    /// Last timings report time
    last_timings_report: Mutex<Instant>,
    /// Active performance mode request count
    performance_mode_requests: AtomicU32,
    /// Current performance mode
    current_performance_mode: Mutex<PerformanceMode>,
    /// Platform wake hook, fired on the `frame_scheduled` false->true
    /// transition (Flutter parity: `SchedulerBinding.scheduleFrame` ->
    /// `platformDispatcher.scheduleFrame`). Without it, ticker
    /// re-registration only sets an atomic nobody reads while the
    /// platform sleeps, and animations starve after the first frame.
    on_frame_scheduled: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

/// Every piece of scheduler state, unified behind one allocation.
///
/// Before this type existed, `UpdateScheduler` held five independent `Arc` blobs
/// (`frame`/`callbacks`/`binding`/`task_queue`/`async_driver`) — a "handle"
/// in spirit, but a double indirection wherever code wanted a single owning
/// reference to hand out (`create_ticker` allocated a fresh `Arc<UpdateScheduler>`
/// over the five already-`Arc` fields just to give the ticker something to
/// hold). Collapsing them into one `Arc<SchedulerInner>` makes [`UpdateScheduler`]
/// a plain cheap-clone handle over ONE allocation, and — the reason this
/// exists — makes a true, non-owning [`WeakUpdateScheduler`] possible: a `Weak`
/// over five separate Arcs cannot express "this scheduler is gone", only
/// "this one piece of it is gone".
struct SchedulerInner {
    /// Frame lifecycle and timing
    frame: FrameState,
    /// Callback registration
    callbacks: CallbackState,
    /// Binding integration
    binding: BindingState,
    /// Task queue (priority-based, already internally synchronized)
    task_queue: TaskQueue,
    /// Frame-driven async task driver, polled once per frame by
    /// [`UpdateScheduler::handle_begin_frame`] in the mid-frame slot.
    /// The bindings no longer call it directly.
    async_driver: crate::AsyncDriver,
}

/// Main scheduler for frame and task management
///
/// Implements Flutter-like scheduling with proper phase separation:
/// - TransientCallbacks: Animation tickers
/// - PersistentCallbacks: Rendering pipeline
/// - PostFrameCallbacks: Cleanup
///
/// `UpdateScheduler` is a cheap-clone handle over one `Arc<SchedulerInner>`
/// allocation — cloning bumps one refcount, not five. [`UpdateScheduler::downgrade`]
/// vends a [`WeakUpdateScheduler`] for a handle that must not keep a dead realm's
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
    inner: Arc<SchedulerInner>,
}

/// A non-owning reference to a [`UpdateScheduler`], obtained via [`UpdateScheduler::downgrade`].
///
/// Exists so a handle that must outlive its scheduler's *owner* (a `Ticker`
/// stored on an `AnimationController`, a `PostFrameHandle` vended to a
/// widget capability) can fail closed instead of keeping the whole scheduler
/// — and everything it owns — alive. [`upgrade`](Self::upgrade) returns
/// `None` once the realm that owns the backing `UpdateScheduler` has dropped its
/// last strong reference; every caller here treats that as "silently done",
/// matching a disposed ticker's own short-circuit.
///
/// `Send + Sync`, same as `UpdateScheduler` — cancellation from a foreign thread
/// keeps working exactly as it did when `Ticker` held a strong `Arc<UpdateScheduler>`.
#[derive(Clone)]
pub struct WeakUpdateScheduler {
    inner: std::sync::Weak<SchedulerInner>,
}

impl UpdateScheduler {
    /// Obtain a non-owning [`WeakUpdateScheduler`] over this scheduler.
    #[must_use]
    pub fn downgrade(&self) -> WeakUpdateScheduler {
        WeakUpdateScheduler {
            inner: Arc::downgrade(&self.inner),
        }
    }
}

impl WeakUpdateScheduler {
    /// Upgrade to a strong [`UpdateScheduler`] handle, or `None` if every strong
    /// reference to the backing scheduler has already been dropped (its
    /// owning realm has torn down).
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
                &self.inner.frame.frame_scheduled.load(Ordering::Acquire),
            )
            .field("task_queue", &self.inner.task_queue)
            .finish_non_exhaustive()
    }
}

/// Shared body of [`UpdateScheduler::request_frame`] and the async driver's wake hook.
///
/// Factored out so both callers can hand it plain field references
/// (`&FrameState`, `&BindingState`) rather than duplicating the
/// scheduled-frame coalescing logic. This is a plumbing convenience, not the
/// cycle-avoidance mechanism itself: `UpdateScheduler` collapsed to one
/// `Arc<SchedulerInner>` (see that type's own doc), so the wake hook's actual
/// acyclic guarantee lives in `UpdateScheduler::new`'s `Weak<SchedulerInner>`
/// capture, not in this function's split parameters — a stale earlier
/// version of this doc described the hook capturing `Arc<FrameState>` +
/// `Arc<BindingState>` separately to dodge an `Arc` cycle; that split-Arc
/// scheme predates the single-`Arc` `SchedulerInner` and no longer describes
/// what the hook actually captures.
fn request_frame_impl(frame: &FrameState, binding: &BindingState) {
    let was_scheduled = frame.frame_scheduled.swap(true, Ordering::AcqRel);
    if !was_scheduled {
        let hook = binding.on_frame_scheduled.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
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
    /// A post-construction `Arc::get_mut` on `inner` cannot do this instead:
    /// `get_mut` requires zero weak refs too, and the async-driver wake hook
    /// this constructor installs always holds one (see below), so it would
    /// unconditionally return `None`. Taking the queue as a parameter avoids
    /// needing mutable access to the constructed `Arc` at all.
    fn with_budget_target_and_task_queue(
        budget_target: FrameDuration,
        task_queue: TaskQueue,
    ) -> Self {
        let target_fps = budget_target.fps() as u32;
        let inner = Arc::new(SchedulerInner {
            frame: FrameState {
                scheduler_phase: AtomicU8::new(SchedulerPhase::Idle as u8),
                current_frame: Mutex::new(None),
                current_vsync_time: Mutex::new(None),
                budget: Mutex::new(FrameBudget::new(target_fps)),
                frame_scheduled: AtomicBool::new(false),
                frame_count: AtomicU64::new(0),
                janky_frame_count: AtomicU64::new(0),
                warm_up_done: AtomicBool::new(false),
                idle_deadline: Mutex::new(None),
                completion_waiters: Mutex::new(Vec::new()),
            },
            callbacks: CallbackState {
                post_frame_registration: Mutex::new(()),
                transient: Mutex::new(Vec::new()),
                cancelled: DashMap::new(),
                id_gen: IdGenerator::new(),
                frame: Mutex::new(Vec::new()),
                persistent: Mutex::new(Vec::new()),
                post_frame: Mutex::new(Vec::new()),
                microtasks: Mutex::new(VecDeque::new()),
                idle: Mutex::new(Vec::new()),
                lifecycle_listeners: Mutex::new(Vec::new()),
            },
            binding: BindingState {
                frames_enabled: AtomicBool::new(true),
                lifecycle_state: AtomicU8::new(AppLifecycleState::Resumed as u8),
                epoch_start: Mutex::new(Duration::ZERO),
                timings_callbacks: Mutex::new(Vec::new()),
                pending_timings: Mutex::new(Vec::new()),
                last_timings_report: Mutex::new(Instant::now()),
                performance_mode_requests: AtomicU32::new(0),
                current_performance_mode: Mutex::new(PerformanceMode::Normal),
                on_frame_scheduled: Mutex::new(None),
            },
            task_queue,
            async_driver: crate::AsyncDriver::new(),
        });

        // The driver's wakers request a frame through the scheduler's existing
        // coalescing path (`frame_scheduled` + `on_frame_scheduled`), so an
        // async completion wakes an idle event loop exactly like `setState`.
        // The hook captures a `Weak<SchedulerInner>`, never a strong
        // `UpdateScheduler`/`Arc<SchedulerInner>`, to keep
        // `SchedulerInner → AsyncDriver → hook` acyclic — a strong capture
        // here would leak the entire scheduler for as long as any task is
        // pending. Upgrade failure (the scheduler's last strong ref is
        // already gone) is a silent no-op: there is nothing left to wake.
        let weak_inner = Arc::downgrade(&inner);
        inner.async_driver.set_request_frame(move || {
            if let Some(inner) = weak_inner.upgrade() {
                request_frame_impl(&inner.frame, &inner.binding);
            }
        });

        Self { inner }
    }

    // =========================================================================
    // Phase Management (Flutter-like)
    // =========================================================================

    /// Get current scheduler phase
    pub fn phase(&self) -> SchedulerPhase {
        // Saturating default to Idle on invalid atomic byte (Principle 6:
        // never panic in production paths). Invalid byte is unreachable in
        // normal operation; this is defensive against memory corruption only.
        SchedulerPhase::try_from_u8(self.inner.frame.scheduler_phase.load(Ordering::Acquire))
            .unwrap_or(SchedulerPhase::Idle)
    }

    /// Set scheduler phase with validation
    fn set_scheduler_phase(&self, new_phase: SchedulerPhase) {
        let current =
            SchedulerPhase::try_from_u8(self.inner.frame.scheduler_phase.load(Ordering::Acquire))
                .unwrap_or(SchedulerPhase::Idle);
        debug_assert!(
            current.can_transition_to(new_phase),
            "Invalid phase transition: {current:?} -> {new_phase:?}"
        );
        self.inner
            .frame
            .scheduler_phase
            .store(new_phase as u8, Ordering::Release);
    }

    /// Whether `self` and `other` are clones of the **same** scheduler — the same
    /// callback queues, the same async driver, the same frame.
    ///
    /// `UpdateScheduler` is `Arc`-backed, so this is pointer identity on the shared
    /// inner state. It exists because a capability handed to a widget must be
    /// provably pointed at the realm's own scheduler, not some other one.
    #[must_use]
    pub fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn with_post_frame_registration<R>(
        &self,
        callback: impl FnOnce(CallbackId) -> R,
    ) -> R {
        let _registration = self.inner.callbacks.post_frame_registration.lock();
        callback(self.inner.callbacks.id_gen.next())
    }

    /// Create a FRESH owner-affine local post-frame lane for a binding/runtime.
    ///
    /// Named `new_*`, not `local_post_frame_lane`, so it cannot be confused
    /// with `UiRealm::local_post_frame_lane()` — that one is an ACCESSOR
    /// returning the realm's existing lane; this one is a CONSTRUCTOR that
    /// mints a brand-new, empty one. Both are in scope at every drive site
    /// (a `UiRealm` holds both a `scheduler: UpdateScheduler` and a
    /// `local_post_frame: LocalPostFrameLane` field), so a same-named pair
    /// would let `scheduler.local_post_frame_lane()` compile at a drive site
    /// and silently drain a lane nothing ever schedules into, forever.
    #[doc(hidden)]
    #[must_use]
    pub fn new_local_post_frame_lane(&self) -> crate::LocalPostFrameLane {
        crate::LocalPostFrameLane::new(self)
    }

    /// Check if currently in a frame
    pub fn is_in_frame(&self) -> bool {
        self.phase().is_in_frame()
    }

    // =========================================================================
    // Frame Scheduling (Flutter-like handleBeginFrame/handleDrawFrame)
    // =========================================================================

    /// Handle begin frame - called when vsync signal arrives
    ///
    /// This corresponds to Flutter's `handleBeginFrame`.
    /// Executes transient callbacks (animation tickers) with the vsync
    /// timestamp.
    #[tracing::instrument(skip(self))]
    pub fn handle_begin_frame(&self, vsync_time: Instant) -> FrameId {
        // Store vsync time for all tickers to use
        *self.inner.frame.current_vsync_time.lock() = Some(vsync_time);

        // Create frame timing with vsync timestamp. `FrameTiming` labels its
        // own stats with the persistent budget's target — the only
        // frame-duration value this scheduler tracks anywhere (see
        // `UpdateScheduler::new`'s doc); this does not gate anything.
        let frame_duration = self.inner.frame.budget.lock().frame_duration();
        let mut timing = FrameTiming::with_duration(frame_duration);
        timing.start_time = vsync_time;
        timing.phase = FramePhase::Build;

        let frame_id = timing.id;
        *self.inner.frame.current_frame.lock() = Some(timing);
        self.inner
            .frame
            .frame_scheduled
            .store(false, Ordering::Release);
        self.inner.frame.frame_count.fetch_add(1, Ordering::Relaxed);

        // Phase 1: TransientCallbacks (animation tickers)
        self.set_scheduler_phase(SchedulerPhase::TransientCallbacks);

        // Execute transient callbacks (animations get vsync timestamp)
        // Use drain() instead of take() to preserve Vec capacity across frames
        let transient: Vec<_> = {
            let mut cbs = self.inner.callbacks.transient.lock();
            cbs.drain(..).collect()
        };

        if !transient.is_empty() {
            tracing::debug!(count = transient.len(), "executing transient callbacks");
        }

        for cancellable in transient {
            // Skip if cancelled (DashMap provides lock-free contains_key)
            if self.inner.callbacks.cancelled.contains_key(&cancellable.id) {
                continue;
            }
            (cancellable.callback)(vsync_time);
        }

        // NOTE: Do NOT clear cancelled_callbacks here. Cancellations requested
        // during transient callbacks (e.g. cancelling a post-frame callback)
        // must survive until handle_draw_frame checks them. The single clear()
        // at the end of handle_draw_frame is sufficient.

        // Execute legacy frame callbacks
        let callbacks: Vec<_> = {
            let mut cbs = self.inner.callbacks.frame.lock();
            cbs.drain(..).collect()
        };

        for callback in callbacks {
            if let Some(timing) = self.inner.frame.current_frame.lock().as_ref() {
                callback(timing);
            }
        }

        // Phase 2: MidFrameMicrotasks
        self.set_scheduler_phase(SchedulerPhase::MidFrameMicrotasks);
        self.flush_microtasks();

        // The async-driver step: exactly one poll per frame in the mid-frame
        // microtask slot, after transient callbacks and before persistent ones in
        // `handle_draw_frame`. A future completing here calls `RebuildHandle::schedule()`,
        // whose id the pipeline's `build_scope` drains — so a completion lands in THIS frame.
        //
        // This lives in the scheduler, not in bindings. The contract is "exactly one
        // mid-frame poll on the right `UpdateScheduler` instance". Owning it here enforces
        // both structurally: `HeadlessBinding` drives its binding-local scheduler and
        // production drives the singleton, and neither can forget the step or run it twice.
        self.drive_async_tasks();

        frame_id
    }

    /// Run the frame's **persistent callbacks** and priority task queue.
    ///
    /// Flutter's `handleDrawFrame` persistent phase (`scheduler/binding.dart:1343-1346`).
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
    #[tracing::instrument(skip(self))]
    pub fn handle_draw_frame(&self) {
        // Phase 3: PersistentCallbacks (the pipeline's slot)
        self.set_scheduler_phase(SchedulerPhase::PersistentCallbacks);

        // Reset budget at start of rendering
        self.inner.frame.budget.lock().reset();

        // Execute persistent frame callbacks. Copy FrameTiming once outside the
        // loop to avoid re-locking per callback. Clone callbacks to release the
        // lock before invoking (callbacks may call scheduler methods that take
        // other locks).
        let timing_snapshot = *self.inner.frame.current_frame.lock();
        if let Some(timing) = timing_snapshot {
            let persistent_callbacks: Vec<_> = {
                let cbs = self.inner.callbacks.persistent.lock();
                cbs.iter()
                    .filter(|c| !self.inner.callbacks.cancelled.contains_key(&c.id))
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
        // `TaskQueue::execute_until` snapshots the queue once under a
        // single lock acquisition, then runs the batch OUTSIDE that lock
        // (see its own doc) — it does not loop until the queue is
        // exhausted. A Priority::Animation (or Priority::UserInput) task
        // that itself calls `add_task` with Build-or-higher priority while
        // it runs is invisible to that same snapshot: the freshly-queued
        // work would otherwise sit until the *next* frame's
        // `handle_draw_frame`, silently deferred a whole frame for no
        // reason this method's own doc promises (a deadline bounds Idle
        // work only). Loop until a pass finds nothing new, so reentrant
        // Build work runs THIS frame. Bounded: a task that re-enqueues
        // itself every single pass is a task bug (an unbounded reentrant
        // chain), not a reason to hang this frame — cap the passes and
        // trace loudly if the cap is hit, rather than looping forever.
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
            .lock()
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Close the frame: run its **post-frame callbacks**, record timing, notify
    /// waiters, and return to [`SchedulerPhase::Idle`].
    ///
    /// Flutter's `handleDrawFrame` post-frame phase
    /// (`scheduler/binding.dart:1349-1358`). Called **after** the pipeline has
    /// committed layout and paint, so a post-frame callback observes this frame's
    /// geometry — the contract `HeroController` depends on
    /// (`heroes.dart:968`).
    ///
    /// A callback registered *from* a post-frame callback runs on the **next**
    /// frame: the queue is drained into a local buffer before any callback is
    /// invoked, so re-registrations land in the now-empty queue. Flutter behaves
    /// the same way (`scheduler/binding.dart:1350-1351`).
    ///
    /// Each callback runs **exactly once** — the queue is drained, not iterated.
    ///
    /// This drains the **shared** post-frame queue only. A caller that also owns
    /// a [`crate::LocalPostFrameLane`] must use
    /// [`end_frame_with_lane`](Self::end_frame_with_lane) instead, or that lane's
    /// callbacks never run — there is no ambient lookup that finds it for you
    /// (see the `post_frame` module docs for why).
    ///
    /// # Panics
    ///
    /// Debug-asserts an illegal phase transition unless the scheduler is in
    /// `PersistentCallbacks` (i.e. `handle_draw_frame` ran). To finish a frame
    /// *without* running its post-frame callbacks, use
    /// [`abort_frame`](Self::abort_frame).
    #[tracing::instrument(skip(self))]
    pub fn end_frame(&self) {
        self.end_frame_impl(None);
    }

    /// [`end_frame`](Self::end_frame), additionally draining `lane`'s owner-local
    /// queue into the **same** total order as the shared queue — one
    /// `CallbackId` sequence, interleaving well-defined: both queues are
    /// combined into one snapshot and sorted by registration order before any
    /// callback runs, so it does not matter which queue a given callback landed
    /// in, only when it was registered. A widget author reads this on
    /// [`crate::LocalPostFrameHandle::schedule_local`], not here.
    #[tracing::instrument(skip(self, lane))]
    pub fn end_frame_with_lane(&self, lane: &crate::LocalPostFrameLane) {
        self.end_frame_impl(Some(lane));
    }

    fn end_frame_impl(&self, lane: Option<&crate::LocalPostFrameLane>) {
        // Phase 4: PostFrameCallbacks
        self.set_scheduler_phase(SchedulerPhase::PostFrameCallbacks);

        let timing = self.inner.frame.current_frame.lock().take();

        if let Some(timing) = timing {
            // Record timing and check for jank
            let elapsed = timing.elapsed();
            self.inner
                .frame
                .budget
                .lock()
                .record_frame_duration(elapsed);

            if timing.is_janky() {
                self.inner
                    .frame
                    .janky_frame_count
                    .fetch_add(1, Ordering::Relaxed);
            }

            // Record timing for batched reporting
            self.inner.binding.pending_timings.lock().push(timing);

            // Drain BEFORE invoking: a post-frame callback that registers another
            // one must not have it run in this same frame. `lane.take_queue()`
            // runs in here, inside the "a frame was actually open" branch, not
            // unconditionally at the top of this function — taking the lane's
            // queue on a no-open-frame call (no preceding `handle_begin_frame`/
            // `handle_draw_frame`, or a second `end_frame_with_lane` call with
            // nothing new to close) would silently drop every already-queued
            // local entry: they'd be taken out of the lane here, never folded
            // into `callbacks` below since that only happens inside this same
            // `Some(timing)` arm, and then dropped un-run when this function
            // returns. Keeping the take() and the invoke() in the same branch
            // means a queue that isn't drained this call is simply left alone,
            // still in the lane, for whenever a real frame next closes.
            let mut callbacks: Vec<LocalPostFrameEntry> = {
                let _registration = self.inner.callbacks.post_frame_registration.lock();
                let mut cbs = self.inner.callbacks.post_frame.lock();
                let mut snapshot: Vec<_> = cbs
                    .drain(..)
                    .map(|entry| LocalPostFrameEntry {
                        id: entry.id,
                        callback: entry.callback as OwnerPostFrameCallback,
                    })
                    .collect();
                if let Some(lane) = lane {
                    snapshot.extend(lane.take_queue());
                }
                snapshot
            };

            callbacks.sort_unstable_by_key(|entry| entry.id.get());
            let callback_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for entry in callbacks {
                    if self.inner.callbacks.cancelled.contains_key(&entry.id) {
                        continue;
                    }
                    (entry.callback)(&timing);
                }
            }));

            // Clear processed cancellations
            self.inner.callbacks.cancelled.clear();

            // Notify frame completion futures
            self.notify_frame_completion(&timing);

            if let Err(payload) = callback_result {
                self.inner
                    .frame
                    .scheduler_phase
                    .store(SchedulerPhase::Idle as u8, Ordering::Release);
                *self.inner.frame.current_vsync_time.lock() = None;
                std::panic::resume_unwind(payload);
            }
        }

        // Return to idle
        self.set_scheduler_phase(SchedulerPhase::Idle);
        *self.inner.frame.current_vsync_time.lock() = None;
    }

    /// Abandon the open frame: return to [`SchedulerPhase::Idle`] **without**
    /// running its post-frame callbacks.
    ///
    /// Flutter's `finally { _schedulerPhase = idle; _currentFrameTimeStamp = null; }`
    /// (`scheduler/binding.dart:1364-1374`): when a persistent callback throws, the
    /// post-frame loop is skipped but the phase is still reset. The queued
    /// callbacks survive and run on the next completed frame.
    ///
    /// # Who calls this, and why it is not a `Drop` guard
    ///
    /// [`drive_frame`](Self::drive_frame) **does** catch a panicking pipeline: it
    /// `catch_unwind`s, calls this, then `resume_unwind`s. So this runs *between*
    /// the catch and the resume — the panic payload is already captured and nothing
    /// here executes during unwinding. A caller that drives a frame by hand
    /// (`handle_begin_frame` + `handle_draw_frame`) and panics must call this itself.
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
    ///
    /// Idempotent: a no-op when no frame is open.
    pub fn abort_frame(&self) {
        if self.phase() == SchedulerPhase::Idle {
            return;
        }

        let timing = self.inner.frame.current_frame.lock().take();

        // Raw store: this is the deliberate exception to the forward-only phase
        // machine. `set_scheduler_phase` would `debug_assert!` on
        // `PersistentCallbacks -> Idle`.
        self.inner
            .frame
            .scheduler_phase
            .store(SchedulerPhase::Idle as u8, Ordering::Release);
        *self.inner.frame.current_vsync_time.lock() = None;
        self.inner.callbacks.cancelled.clear();

        if let Some(timing) = timing {
            self.notify_frame_completion(&timing);
        }

        tracing::warn!("frame aborted; its post-frame callbacks were not run");
    }

    /// The one shared frame ordering: **begin → persistent → pipeline → post-frame → idle.**
    ///
    /// Every frame driver goes through here — `HeadlessBinding::pump_frame` on its
    /// binding-local scheduler, and the desktop / android / wasm runners on
    /// the realm's own owned `UpdateScheduler` (`UiRealm.scheduler`, in flui-app —
    /// there is no process-global scheduler singleton any more) — so
    /// headless and production cannot drift.
    ///
    /// `pipeline` is the binding's build → layout → compositing → paint step. It
    /// runs in the [`SchedulerPhase::PersistentCallbacks`] slot without being
    /// registered as a callback: FLUI's bindings own their element tree by value,
    /// so no `Fn` closure could drive it the way Flutter's `drawFrame` does.
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
    /// A pipeline that **panics** is an abandoned frame. The panic is caught,
    /// [`abort_frame`](Self::abort_frame) resets the phase **without running any
    /// post-frame callback**, and the panic is then resumed unchanged. The queued
    /// callbacks survive to the next completed frame. This is Flutter's
    /// `try { persistent; postFrame } finally { phase = idle; }`
    /// (`scheduler/binding.dart:1341-1374`), where a throwing persistent callback
    /// skips the post-frame loop but still resets the phase.
    ///
    /// The recovery runs *between* `catch_unwind` and `resume_unwind` — the panic
    /// payload is already captured, so nothing here executes during unwinding, and
    /// no `Drop` guard is involved for THIS part (see `abort_frame` for why a guard
    /// would `abort` the process here specifically — the earlier `IdleDeadlineGuard`
    /// is a different, narrower guard over a plain field, not the phase machine).
    /// Without this, a panicking pipeline would leave the
    /// frame open at `PersistentCallbacks` and the *next* `handle_begin_frame`
    /// would attempt the illegal `PersistentCallbacks -> TransientCallbacks`
    /// transition.
    ///
    /// Under `panic = "abort"` nothing is caught and the process dies with the
    /// frame open, which is moot.
    pub fn drive_frame<R>(
        &self,
        vsync_time: Instant,
        deadline: IdleDeadline,
        pipeline: impl FnOnce() -> R,
    ) -> R {
        self.drive_frame_impl(vsync_time, deadline, pipeline, None)
    }

    /// [`drive_frame`](Self::drive_frame), additionally draining `lane`'s
    /// owner-local queue into the same total order as the shared queue when the
    /// frame completes. See [`end_frame_with_lane`](Self::end_frame_with_lane)
    /// for the ordering guarantee.
    pub fn drive_frame_with_lane<R>(
        &self,
        vsync_time: Instant,
        deadline: IdleDeadline,
        pipeline: impl FnOnce() -> R,
        lane: &crate::LocalPostFrameLane,
    ) -> R {
        self.drive_frame_impl(vsync_time, deadline, pipeline, Some(lane))
    }

    fn drive_frame_impl<R>(
        &self,
        vsync_time: Instant,
        deadline: IdleDeadline,
        pipeline: impl FnOnce() -> R,
        lane: Option<&crate::LocalPostFrameLane>,
    ) -> R {
        use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

        *self.inner.frame.idle_deadline.lock() = Some(deadline.0);
        let idle_deadline_guard = IdleDeadlineGuard {
            slot: &self.inner.frame.idle_deadline,
        };

        self.handle_begin_frame(vsync_time);
        self.handle_draw_frame();
        // The deadline only ever needs to be visible for this frame's own
        // `handle_draw_frame` call, immediately above; drop the guard now
        // (rather than leaving it to this function's own end) so a caller
        // that later drives `handle_begin_frame`/`handle_draw_frame` by hand
        // never inherits a stale deadline, and so it is visibly gone before
        // `pipeline` runs. The guard's `Drop` — not this explicit call alone
        // — is what still clears the field if `handle_begin_frame` or
        // `handle_draw_frame` above panics; see this method's own doc.
        drop(idle_deadline_guard);

        match catch_unwind(AssertUnwindSafe(pipeline)) {
            Ok(result) => {
                match lane {
                    Some(lane) => self.end_frame_with_lane(lane),
                    None => self.end_frame(),
                }
                result
            }
            Err(payload) => {
                self.abort_frame();
                resume_unwind(payload)
            }
        }
    }

    /// Execute a complete frame (convenience method)
    ///
    /// Calls handle_begin_frame and handle_draw_frame in sequence.
    /// Use this for simple cases; for proper vsync integration,
    /// call handle_begin_frame and handle_draw_frame separately.
    #[tracing::instrument(skip(self))]
    pub fn execute_frame(&self) -> FrameId {
        // begin → persistent → end. The non-pipeline convenience path (warm-up
        // frames, tests); `drive_frame` is the same sequence with a pipeline in
        // the persistent slot. Preserves this method's original behavior: the
        // frame completes and its post-frame callbacks run.
        let vsync_time = Instant::now();
        let frame_id = self.handle_begin_frame(vsync_time);
        self.handle_draw_frame();
        self.end_frame();
        frame_id
    }

    /// [`execute_frame`](Self::execute_frame), additionally draining `lane`'s
    /// owner-local queue in the same total order as the shared queue.
    #[tracing::instrument(skip(self, lane))]
    pub fn execute_frame_with_lane(&self, lane: &crate::LocalPostFrameLane) -> FrameId {
        let vsync_time = Instant::now();
        let frame_id = self.handle_begin_frame(vsync_time);
        self.handle_draw_frame();
        self.end_frame_with_lane(lane);
        frame_id
    }

    /// Schedule a warm-up frame (synchronous, no vsync wait)
    ///
    /// This forces an immediate frame to be processed, useful for:
    /// - App initialization
    /// - Reducing first-frame jank
    /// - Forcing immediate layout updates
    #[tracing::instrument(skip(self))]
    pub fn schedule_warm_up_frame(&self) {
        if self.inner.frame.warm_up_done.load(Ordering::Acquire) {
            return;
        }

        // Execute frame immediately without vsync
        self.execute_frame();
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
    pub fn schedule_frame_callback(&self, callback: OneShotFrameCallback) -> CallbackId {
        let id = self.inner.callbacks.id_gen.next();
        self.inner
            .callbacks
            .transient
            .lock()
            .push(CancellableTransientCallback { id, callback });
        // Registering a tick demands a frame to run it in (Flutter parity:
        // `scheduleFrameCallback` calls `scheduleFrame`). `request_frame`
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
        // First, try to remove from pending callbacks
        let mut callbacks = self.inner.callbacks.transient.lock();
        let original_len = callbacks.len();
        callbacks.retain(|c| c.id != id);

        if callbacks.len() < original_len {
            return true; // Found and removed
        }

        // If not found, mark as cancelled (in case it's about to be executed)
        self.inner.callbacks.cancelled.insert(id, ());
        false
    }

    /// Schedule a legacy frame callback
    pub fn schedule_frame(&self, callback: FrameCallback) {
        self.inner.callbacks.frame.lock().push(callback);
        self.request_frame();
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
        request_frame_impl(&self.inner.frame, &self.inner.binding);
    }

    // =========================================================================
    // Async Task Driver
    // =========================================================================

    /// The frame-driven task driver.
    ///
    /// Clone it to spawn tasks from elsewhere; every clone shares one task set.
    #[must_use]
    pub fn async_driver(&self) -> &crate::AsyncDriver {
        &self.inner.async_driver
    }

    /// Queue `future` for polling on the frame thread, and request a frame.
    ///
    /// Thin forwarder to [`AsyncDriver::spawn_local`](crate::AsyncDriver::spawn_local).
    /// Dropping the returned [`TaskToken`](crate::TaskToken) cancels the task.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local(&self, future: crate::BoxedTask) -> crate::TaskToken {
        self.inner.async_driver.spawn_local(future)
    }

    /// Spawn `future`, polling it once inline (Flutter's synchronous-`.then`
    /// window). `None` when it completed on that first poll.
    ///
    /// Thin forwarder to [`AsyncDriver::spawn_local_eager`](crate::AsyncDriver::spawn_local_eager).
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local_eager(&self, future: crate::BoxedTask) -> Option<crate::TaskToken> {
        self.inner.async_driver.spawn_local_eager(future)
    }

    /// **The** async-driver step of a frame — the single call site both frame
    /// drivers use (`HeadlessBinding::pump_frame` and `UiRealm::draw_frame`).
    ///
    /// Polls every task whose waker fired, on the calling thread. A future
    /// completing here calls `RebuildHandle::schedule()`, whose id the *same*
    /// frame's `build_scope` then drains — so a completion is observed without
    /// waiting an extra frame.
    ///
    /// # Where this sits in a frame
    ///
    /// Flutter's `SchedulerPhase.midFrameMicrotasks`: after the frame's transient
    /// callbacks (animation ticks), before its persistent callbacks (build →
    /// layout → paint). Both bindings call it in exactly that slot, between
    /// `vsync.tick_all` and `build_scope`.
    ///
    /// **Called by [`handle_begin_frame`](Self::handle_begin_frame), once per
    /// frame**. Previously, each binding called it directly. The real invariant is
    /// *one mid-frame poll per frame, on the right `UpdateScheduler` instance*. Moving
    /// the call into the scheduler enforces both structurally and lets the pipeline
    /// take the persistent slot it always semantically occupied.
    ///
    /// Still `pub`: a test may drive one poll directly. It does not mutate the
    /// phase (the machine is strictly forward-only — `MidFrameMicrotasks -> Idle`
    /// is not a legal transition). It asserts the invariant that actually
    /// matters: **never poll while the scheduler is running persistent
    /// callbacks**, i.e. inside build / layout / paint.
    ///
    /// Returns the number of tasks polled.
    pub fn drive_async_tasks(&self) -> usize {
        debug_assert_ne!(
            self.phase(),
            SchedulerPhase::PersistentCallbacks,
            "BUG: the async driver must not poll during build/layout/paint; the \
             driver step belongs between the transient and persistent callbacks"
        );
        self.inner.async_driver.poll_ready()
    }

    /// Clears the `frame_scheduled` latch for a wake that will call
    /// [`drive_async_tasks`](Self::drive_async_tasks) WITHOUT a surrounding
    /// [`handle_begin_frame`](Self::handle_begin_frame) — the frames-disabled
    /// `PumpAsync` path (`ADR-0035`'s `wake_action`), which deliberately
    /// never begins/draws a real frame.
    ///
    /// # Why this exists
    ///
    /// `on_frame_scheduled` fires only on `frame_scheduled`'s false→true
    /// transition, and the *only* place that ever clears the latch back to
    /// `false` is `handle_begin_frame`, unconditionally, at the top of every
    /// real frame. A `PumpAsync` wake has no frame to do that clearing —
    /// without this call, a `request_frame()` made before entering
    /// `PumpAsync` (a task's initial spawn, or an earlier wake) leaves the
    /// latch `true` forever: a LATER, independent wake (a network
    /// response's `Waker::wake()`, arriving on a different thread after this
    /// pump cycle already returned) finds the latch already set, never
    /// transitions it, and the hook never fires again — the platform loop is
    /// never told to wake, and the future silently stops advancing with no
    /// visible error.
    ///
    /// # Call before, not after, `drive_async_tasks`
    ///
    /// Call this immediately **before** `drive_async_tasks`, mirroring
    /// `handle_begin_frame`'s clear-at-the-top order — not after. A polled
    /// future may legitimately re-arm itself synchronously (call
    /// `Waker::wake_by_ref` from inside its own `poll`, wanting to be polled
    /// again next cycle); that call sets `frame_scheduled` back to `true`
    /// *during* `drive_async_tasks`. Clearing the latch again *afterward*
    /// would silently erase that signal — the exact starvation this method
    /// exists to prevent, just for a self-waking task instead of an
    /// externally-woken one.
    pub fn finish_async_pump(&self) {
        self.inner
            .frame
            .frame_scheduled
            .store(false, Ordering::Release);
    }

    /// Number of tasks the async driver holds.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.inner.async_driver.pending_task_count()
    }

    /// Install the platform wake hook fired when a frame is first
    /// scheduled (Flutter parity: `SchedulerBinding.scheduleFrame` →
    /// `platformDispatcher.scheduleFrame`).
    ///
    /// The hook runs on whichever thread schedules the frame and may run
    /// while callers hold their own locks — it must only touch wake
    /// machinery (e.g. `request_redraw`), never re-enter the scheduler.
    pub fn set_on_frame_scheduled(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.inner.binding.on_frame_scheduled.lock() = hook;
    }

    /// Add a persistent frame callback.
    ///
    /// Fires every frame during PersistentCallbacks phase. Use for the
    /// rendering pipeline (build/layout/paint).
    ///
    /// Flutter parity at [`binding.dart:773`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart):
    /// "Persistent frame callbacks cannot be unregistered. Once registered,
    /// they are called for every frame for the lifetime of the application."
    /// Returns `()` — no removal handle.
    pub fn add_persistent_frame_callback(&self, callback: RecurringFrameCallback) {
        let id = self.inner.callbacks.id_gen.next();
        self.inner
            .callbacks
            .persistent
            .lock()
            .push(CancellablePersistentCallback { id, callback });
    }

    /// Add a post-frame callback.
    ///
    /// Fires once after the current/next frame completes.
    ///
    /// Flutter parity at [`binding.dart:802`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/binding.dart):
    /// "Post-frame callbacks ... are called exactly once" and cannot be
    /// cancelled before they fire. Returns `()` — no cancellation handle.
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) {
        self.with_post_frame_registration(|id| {
            self.inner
                .callbacks
                .post_frame
                .lock()
                .push(CancellablePostFrameCallback { id, callback });
        });
    }

    // =========================================================================
    // Microtask Queue
    // =========================================================================

    /// Schedule a microtask
    ///
    /// Microtasks are executed during MidFrameMicrotasks phase,
    /// after animations but before rendering.
    pub fn schedule_microtask(&self, task: Box<dyn FnOnce() + Send>) {
        self.inner.callbacks.microtasks.lock().push_back(task);
    }

    /// Flush all pending microtasks
    fn flush_microtasks(&self) {
        loop {
            let task = self.inner.callbacks.microtasks.lock().pop_front();
            match task {
                Some(t) => t(),
                None => break,
            }
        }
    }

    // =========================================================================
    // Task Queue
    // =========================================================================

    /// Add a task with priority
    pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
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
        *self.inner.frame.current_vsync_time.lock()
    }

    // =========================================================================
    // Frame State
    // =========================================================================

    /// Check if a frame is scheduled
    pub fn is_frame_scheduled(&self) -> bool {
        self.inner.frame.frame_scheduled.load(Ordering::Acquire)
    }

    /// Get the number of pending transient callbacks
    ///
    /// This is useful for debugging and testing to verify that
    /// all transient callbacks have been processed.
    pub fn transient_callback_count(&self) -> usize {
        self.inner.callbacks.transient.lock().len()
    }

    /// Get current frame timing (if a frame is active)
    pub fn current_frame(&self) -> Option<FrameTiming> {
        *self.inner.frame.current_frame.lock()
    }

    /// Set the current frame phase (for rendering pipeline)
    pub fn set_phase(&self, phase: FramePhase) {
        if let Some(timing) = self.inner.frame.current_frame.lock().as_mut() {
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
            .lock()
            .as_ref()
            .is_some_and(super::frame::FrameTiming::is_over_budget)
    }

    /// Check if deadline is near, i.e. >80% of the labeled target duration
    /// used (informational only — does not gate; see this section's doc).
    pub fn is_deadline_near(&self) -> bool {
        self.inner
            .frame
            .current_frame
            .lock()
            .as_ref()
            .is_some_and(super::frame::FrameTiming::is_deadline_near)
    }

    /// Get remaining budget as type-safe Milliseconds
    pub fn remaining_budget(&self) -> Milliseconds {
        self.inner
            .frame
            .current_frame
            .lock()
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
        self.inner.frame.budget.lock().clone()
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
        self.inner.frame.budget.lock().avg_fps()
    }

    /// Check if last frame was janky
    pub fn is_janky(&self) -> bool {
        self.inner.frame.budget.lock().is_janky()
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
    /// expected to already be on the realm's owner thread (the platform
    /// event-loop thread that drives `flui-app`'s realm dispatch); this
    /// method does not itself verify that, since the scheduler has no
    /// notion of "realm" or "owner thread" to assert against. A caller with
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

        // Auto-toggle frames_enabled per Flutter parity at
        // binding.dart:414-441. Resumed/Inactive keep rendering active
        // (Inactive means visible-but-unfocused — split screen, modal — still
        // needs to draw). Hidden/Paused/Detached disable the frame loop.
        let should_render = matches!(
            new_state,
            AppLifecycleState::Resumed | AppLifecycleState::Inactive
        );
        let frames_were_enabled = self
            .inner
            .binding
            .frames_enabled
            .swap(should_render, Ordering::AcqRel);

        // Flutter's `_setFramesEnabledState(true)` (binding.dart @ 3.44.0)
        // schedules a frame on exactly the disabled→enabled edge —
        // `SchedulerBinding.scheduleFrame()` is called there, not on every
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
                let listeners = self.inner.callbacks.lifecycle_listeners.lock();
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
    /// use std::sync::Arc;
    ///
    /// use flui_scheduler::{AppLifecycleState, UpdateScheduler};
    ///
    /// let scheduler = UpdateScheduler::new();
    ///
    /// let id = scheduler.add_lifecycle_state_listener(Arc::new(|state| {
    ///     println!("App lifecycle changed to: {}", state);
    /// }));
    ///
    /// // Later, remove the listener
    /// scheduler.remove_lifecycle_state_listener(id);
    /// ```
    pub fn add_lifecycle_state_listener(
        &self,
        callback: Arc<dyn Fn(AppLifecycleState) + Send + Sync>,
    ) -> CallbackId {
        let id = self.inner.callbacks.id_gen.next();
        self.inner
            .callbacks
            .lifecycle_listeners
            .lock()
            .push(LifecycleListener { id, callback });
        id
    }

    /// Remove a lifecycle state change listener by ID
    ///
    /// Returns `true` if the listener was found and removed.
    pub fn remove_lifecycle_state_listener(&self, id: CallbackId) -> bool {
        let mut listeners = self.inner.callbacks.lifecycle_listeners.lock();
        let original_len = listeners.len();
        listeners.retain(|l| l.id != id);
        listeners.len() < original_len
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
    /// use flui_scheduler::UpdateScheduler;
    ///
    /// async fn wait_for_frame(scheduler: &UpdateScheduler) {
    ///     // Wait for the current/next frame to complete
    ///     let timing = scheduler.end_of_frame().await;
    ///
    ///     println!(
    ///         "Frame {} completed in {}ms",
    ///         timing.id.get(),
    ///         timing.elapsed().value()
    ///     );
    /// }
    /// ```
    ///
    /// # Flutter Equivalent
    ///
    /// This is similar to Flutter's `SchedulerBinding.endOfFrame` Future.
    pub fn end_of_frame(&self) -> FrameCompletionFuture {
        let (future, state) = FrameCompletionFuture::new();
        self.inner
            .frame
            .completion_waiters
            .lock()
            .push(FrameCompletionNotifier { state });
        future
    }

    /// Internal: Notify all frame completion waiters
    fn notify_frame_completion(&self, timing: &FrameTiming) {
        let waiters: Vec<_> = {
            let mut waiters = self.inner.frame.completion_waiters.lock();
            waiters.drain(..).collect()
        };

        for notifier in waiters {
            let mut state = notifier.state.lock();
            state.completed = Some(*timing);
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }
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
        self.inner.binding.frames_enabled.load(Ordering::Acquire)
    }

    /// Enable or disable frame scheduling
    pub fn set_frames_enabled(&mut self, enabled: bool) {
        self.inner
            .binding
            .frames_enabled
            .store(enabled, Ordering::Release);
    }

    /// Schedule a frame if frames are enabled
    ///
    /// Unlike `request_frame()`, this checks `frames_enabled` first.
    pub fn schedule_frame_if_enabled(&self) {
        if self.inner.binding.frames_enabled.load(Ordering::Acquire) {
            self.request_frame();
        }
    }

    /// Schedule a forced frame (ignores `frames_enabled`)
    pub fn schedule_forced_frame(&self) {
        self.request_frame();
    }

    /// Ensure a visual update is scheduled
    ///
    /// Calls `schedule_frame_if_enabled()` to guarantee a frame will be
    /// processed.
    pub fn ensure_visual_update(&self) {
        self.schedule_frame_if_enabled();
    }

    /// Reset the epoch for time dilation calculations
    ///
    /// Called when time dilation changes to avoid large time jumps.
    pub fn reset_epoch(&self) {
        *self.inner.binding.epoch_start.lock() = Duration::ZERO;
    }

    /// Set the process-wide time dilation factor and reset this scheduler's
    /// epoch, so the change takes effect without a large time jump on the
    /// next frame. Flutter parity: `binding.dart::timeDilation`'s setter.
    ///
    /// Delegates validation and storage to
    /// [`config::set_time_dilation`](crate::config::set_time_dilation) —
    /// the atomic itself stays process-wide (a data-plane debug knob), only
    /// the epoch-reset reach is scheduler-instance-scoped now that nothing in
    /// this crate resolves a scheduler singleton to perform it.
    ///
    /// # Errors
    ///
    /// See [`config::set_time_dilation`](crate::config::set_time_dilation).
    pub fn set_time_dilation(&self, value: f64) -> Result<(), crate::config::InvalidTimeDilation> {
        crate::config::set_time_dilation(value)?;
        self.reset_epoch();
        Ok(())
    }

    /// Get the current frame timestamp adjusted for epoch and time dilation
    pub fn current_frame_time_stamp(&self) -> Duration {
        let epoch = *self.inner.binding.epoch_start.lock();
        adjust_duration_for_epoch(epoch, Duration::ZERO)
    }

    /// Get the current system frame timestamp
    pub fn current_system_frame_time_stamp(&self) -> Instant {
        self.current_vsync_time().unwrap_or_else(Instant::now)
    }

    /// Adjust a duration for the current epoch and time dilation
    pub fn adjust_for_epoch(&self, raw: Duration) -> Duration {
        let epoch = *self.inner.binding.epoch_start.lock();
        adjust_duration_for_epoch(raw, epoch)
    }

    /// Request a performance mode
    ///
    /// Returns a handle that releases the request when dropped.
    pub fn request_performance_mode(&self, _mode: PerformanceMode) -> PerformanceModeRequestHandle {
        self.inner
            .binding
            .performance_mode_requests
            .fetch_add(1, Ordering::AcqRel);

        // Weak, not a strong clone: this handle is an external RAII object
        // the caller may hold arbitrarily long, unlike `Ticker`'s callback
        // (which lives inside the scheduler's OWN transient queue and would
        // form a cycle). A dead scheduler has nothing left to release the
        // request from, so upgrade failure is a silent no-op.
        let weak = self.downgrade();
        PerformanceModeRequestHandle::new(move || {
            if let Some(scheduler) = weak.upgrade() {
                scheduler
                    .inner
                    .binding
                    .performance_mode_requests
                    .fetch_sub(1, Ordering::AcqRel);
            }
        })
    }

    /// Add a timings callback for receiving frame performance reports
    pub fn add_timings_callback(&self, callback: TimingsCallback) {
        self.inner.binding.timings_callbacks.lock().push(callback);
    }

    /// Remove a timings callback
    pub fn remove_timings_callback(&self, callback: &TimingsCallback) {
        let mut callbacks = self.inner.binding.timings_callbacks.lock();
        callbacks.retain(|c| !Arc::ptr_eq(c, callback));
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
            let mut pending = self.inner.binding.pending_timings.lock();
            if pending.is_empty() {
                return 0;
            }
            pending.drain(..).collect()
        };

        let callbacks = self.inner.binding.timings_callbacks.lock().clone();
        let count = timings.len();

        for callback in &callbacks {
            callback(&timings);
        }

        *self.inner.binding.last_timings_report.lock() = Instant::now();
        count
    }

    /// Get the time since the last timings report was sent
    pub fn time_since_last_timings_report(&self) -> Duration {
        self.inner.binding.last_timings_report.lock().elapsed()
    }

    /// Get the current performance mode
    ///
    /// The mode is determined by the highest-priority active request.
    pub fn current_performance_mode(&self) -> PerformanceMode {
        *self.inner.binding.current_performance_mode.lock()
    }

    /// Set the current performance mode directly
    ///
    /// This is typically called internally when performance mode requests
    /// change, but can also be called by the platform integration layer.
    pub fn set_performance_mode(&self, mode: PerformanceMode) {
        *self.inner.binding.current_performance_mode.lock() = mode;
    }

    /// Debug assert: no transient callbacks are pending
    ///
    /// Returns `true` if there are no pending transient callbacks.
    pub fn debug_assert_no_transient_callbacks(&self, _reason: &str) -> bool {
        self.inner.callbacks.transient.lock().is_empty()
    }

    /// Debug assert: no pending performance mode requests
    ///
    /// Returns `true` if all performance mode requests have been released.
    pub fn debug_assert_no_pending_performance_mode_requests(&self, _reason: &str) -> bool {
        self.inner
            .binding
            .performance_mode_requests
            .load(Ordering::Acquire)
            == 0
    }

    /// Debug assert: no time dilation is active
    ///
    /// Returns `true` if time dilation is at the default value (1.0).
    pub fn debug_assert_no_time_dilation(&self, _reason: &str) -> bool {
        (time_dilation() - 1.0).abs() < f64::EPSILON
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
    pub fn schedule_idle_callback(&self, callback: impl FnOnce() + Send + 'static) {
        self.inner.callbacks.idle.lock().push(Box::new(callback));
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
            let mut cbs = self.inner.callbacks.idle.lock();
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
        !self.inner.callbacks.idle.lock().is_empty()
    }

    /// Get the number of active performance mode requests.
    pub fn performance_mode_request_count(&self) -> u32 {
        self.inner
            .binding
            .performance_mode_requests
            .load(Ordering::Acquire)
    }

    /// Get the number of pending frame completion waiters (for testing).
    #[cfg(test)]
    fn completion_waiter_count(&self) -> usize {
        self.inner.frame.completion_waiters.lock().len()
    }
}

impl Default for UpdateScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TickerProvider for UpdateScheduler {
    /// Vend an auto-scheduling [`Ticker`](crate::ticker::Ticker) attached to
    /// this scheduler.
    ///
    /// Flutter parity: [`ticker.dart:248`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `Ticker createTicker(TickerCallback)`. The vended ticker self-registers
    /// a transient frame callback on `start`/`unmute` and cancels it on
    /// `stop`/`mute`/`dispose`.
    ///
    /// The vended ticker stores only a [`WeakUpdateScheduler`] (see
    /// [`Ticker::new_with_scheduler`](crate::ticker::Ticker::new_with_scheduler)) —
    /// no allocation beyond the ticker itself, and no strong reference back to
    /// this scheduler's `SchedulerInner`. A strong `Arc<UpdateScheduler>` here would
    /// recreate a live cycle: this scheduler's own transient-callback queue
    /// stores the ticker's re-scheduling closure, which would then hold a
    /// strong ref back to the scheduler that owns that very queue.
    fn create_ticker(&self, on_tick: crate::ticker::TickerCallback) -> crate::ticker::Ticker {
        let mut ticker = crate::ticker::Ticker::new_with_scheduler(self);
        ticker.set_pending_callback(on_tick);
        ticker
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

#[cfg(test)]
mod tests {

    // =========================================================================
    // Async Driver Integration
    // =========================================================================

    /// Requirement 1: tasks are polled in the `MidFrameMicrotasks` phase.
    ///
    /// `handle_begin_frame` performs the poll itself in that phase —
    /// the bindings no longer call `drive_async_tasks` directly. Replaces
    /// `handle_begin_frame_does_not_poll_async_tasks`, which pinned the old
    /// binding-owned contract.
    #[test]
    fn handle_begin_frame_polls_async_tasks_once_in_the_mid_frame_phase() {
        use std::sync::atomic::AtomicUsize;
        let scheduler = UpdateScheduler::new();
        let observed = Arc::new(Mutex::new(None));
        let polls = Arc::new(AtomicUsize::new(0));
        let observed_for_task = Arc::clone(&observed);
        let polls_for_task = Arc::clone(&polls);
        let probe = scheduler.clone();

        let _token = scheduler.spawn_local(Box::pin(async move {
            polls_for_task.fetch_add(1, Ordering::Release);
            *observed_for_task.lock() = Some(probe.phase());
        }));

        scheduler.handle_begin_frame(Instant::now());

        assert_eq!(
            polls.load(Ordering::Acquire),
            1,
            "exactly one driver poll per frame, performed by the scheduler"
        );
        assert_eq!(
            *observed.lock(),
            Some(SchedulerPhase::MidFrameMicrotasks),
            "the future must be polled in the microtask phase — never during \
             persistent callbacks, i.e. never inside build/layout/paint"
        );
        assert_eq!(
            scheduler.phase(),
            SchedulerPhase::MidFrameMicrotasks,
            "the poll must not advance the phase"
        );
    }

    /// The poll happens on **this** scheduler instance, not on a global. Headless
    /// bindings own a binding-local `UpdateScheduler`; production drives the singleton.
    /// A driver step on the wrong instance would be a divergence between the two.
    #[test]
    fn each_scheduler_instance_polls_only_its_own_async_driver() {
        let a = UpdateScheduler::new();
        let b = UpdateScheduler::new();
        let polled_a = Arc::new(AtomicBool::new(false));
        let polled_a_task = Arc::clone(&polled_a);
        let _token = a.spawn_local(Box::pin(async move {
            polled_a_task.store(true, Ordering::Release);
        }));

        b.handle_begin_frame(Instant::now());
        assert!(
            !polled_a.load(Ordering::Acquire),
            "driving `b`'s frame must not poll `a`'s task"
        );

        a.handle_begin_frame(Instant::now());
        assert!(polled_a.load(Ordering::Acquire));
    }

    /// Requirement 6: polling during build/layout/paint is a `BUG:` panic in
    /// debug. A persistent frame callback runs in `PersistentCallbacks`, so
    /// calling the driver step from one must trip the guard.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "BUG: the async driver must not poll during build/layout/paint")]
    fn drive_async_tasks_panics_if_called_during_persistent_callbacks() {
        let scheduler = UpdateScheduler::new();
        let probe = scheduler.clone();
        scheduler.add_persistent_frame_callback(Arc::new(move |_timing: &FrameTiming| {
            probe.drive_async_tasks();
        }));

        scheduler.handle_begin_frame(Instant::now());
        scheduler.handle_draw_frame();
    }

    /// The driver step is callable from the bindings' `Idle` phase — FLUI's
    /// binding frame path is decoupled from the scheduler's phase machine.
    #[test]
    fn drive_async_tasks_is_callable_outside_a_scheduler_frame() {
        let scheduler = UpdateScheduler::new();
        let polled = Arc::new(AtomicBool::new(false));
        let polled_for_task = Arc::clone(&polled);
        let _token = scheduler.spawn_local(Box::pin(async move {
            polled_for_task.store(true, Ordering::Release);
        }));

        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        assert_eq!(scheduler.drive_async_tasks(), 1);
        assert!(polled.load(Ordering::Acquire));
    }

    /// A task's waker requests a frame through the scheduler's existing
    /// coalescing path (`frame_scheduled` + `on_frame_scheduled`).
    #[test]
    fn async_task_wake_requests_a_frame_through_the_scheduler_hook() {
        let scheduler = UpdateScheduler::new();
        let wakes = Arc::new(AtomicU64::new(0));
        let wakes_for_hook = Arc::clone(&wakes);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            wakes_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        let stored: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let stored_for_task = Arc::clone(&stored);
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            *stored_for_task.lock() = Some(cx.waker().clone());
            std::task::Poll::<()>::Pending
        })));
        // `spawn_local` requested the frame that will first poll the task.
        assert_eq!(wakes.load(Ordering::Relaxed), 1);

        // Consume the pending-frame flag as a real frame would.
        scheduler.handle_begin_frame(Instant::now());
        scheduler.drive_async_tasks();
        let waker = stored.lock().clone().expect("waker stored");

        waker.wake_by_ref();
        waker.wake_by_ref();
        waker.wake_by_ref();
        assert_eq!(
            wakes.load(Ordering::Relaxed),
            2,
            "repeated wakes between frames request exactly one frame"
        );
        assert!(scheduler.is_frame_scheduled());
    }

    /// A `PumpAsync` wake (frames disabled, so no `handle_begin_frame` ever
    /// runs to clear `frame_scheduled`) must not starve a LATER, independent
    /// wake. Shape: spawn -> simulate a `PumpAsync` cycle
    /// (`finish_async_pump` + `drive_async_tasks`, no `handle_begin_frame`)
    /// -> an external wake arriving afterward must still re-fire
    /// `on_frame_scheduled`.
    ///
    /// Red-check: remove the `finish_async_pump()` call below and this
    /// fails — `hook_fires` stays at 1 after the external wake, because
    /// `frame_scheduled` was never cleared and the false→true edge the hook
    /// needs never occurs.
    #[test]
    fn finish_async_pump_lets_a_later_independent_wake_refire_the_hook() {
        use std::task::Waker;

        let scheduler = UpdateScheduler::new();
        let hook_fires = Arc::new(AtomicU64::new(0));
        let hook_fires_for_hook = Arc::clone(&hook_fires);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            hook_fires_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        let stored: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let stored_for_task = Arc::clone(&stored);
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            *stored_for_task.lock() = Some(cx.waker().clone());
            std::task::Poll::<()>::Pending
        })));
        // `spawn_local` requested the frame that will first poll the task.
        assert_eq!(hook_fires.load(Ordering::Relaxed), 1);

        // Simulate a `PumpAsync` wake cycle: no `handle_begin_frame` runs
        // (frames are disabled), just the async-pump sequence a `PumpAsync`
        // arm performs.
        scheduler.finish_async_pump();
        scheduler.drive_async_tasks();
        assert!(
            !scheduler.is_frame_scheduled(),
            "the pump must consume the latch that triggered it, not leave it latched"
        );

        // A LATER, independent wake (e.g. a network response completing on
        // a different thread, well after this pump cycle returned) must
        // still be able to re-fire the hook.
        let waker = stored
            .lock()
            .clone()
            .expect("waker stored by the poll above");
        waker.wake_by_ref();

        assert_eq!(
            hook_fires.load(Ordering::Relaxed),
            2,
            "a wake arriving after a PumpAsync cycle must re-fire on_frame_scheduled, not find \
             the frame_scheduled latch already set and silently coalesce away"
        );
    }

    /// Requirement 7: the microtask queue keeps its existing behavior — the
    /// driver step does not drain or reorder it.
    #[test]
    fn drive_async_tasks_does_not_disturb_microtasks() {
        let scheduler = UpdateScheduler::new();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_for_task = Arc::clone(&ran);
        scheduler.schedule_microtask(Box::new(move || {
            ran_for_task.store(true, Ordering::Release);
        }));

        scheduler.drive_async_tasks();
        assert!(
            !ran.load(Ordering::Acquire),
            "the driver step must not drain the microtask queue"
        );

        scheduler.handle_begin_frame(Instant::now());
        assert!(
            ran.load(Ordering::Acquire),
            "handle_begin_frame still flushes"
        );
    }
    use super::*;

    #[test]
    fn test_scheduler_phase_lifecycle() {
        let scheduler = UpdateScheduler::new();
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);

        let vsync = Instant::now();
        scheduler.handle_begin_frame(vsync);

        // After begin_frame, we should be in MidFrameMicrotasks
        // (TransientCallbacks already executed)
        assert_eq!(scheduler.phase(), SchedulerPhase::MidFrameMicrotasks);

        // `handle_draw_frame` no longer finishes the frame. It hands the
        // `PersistentCallbacks` slot to the caller's pipeline.
        scheduler.handle_draw_frame();
        assert_eq!(scheduler.phase(), SchedulerPhase::PersistentCallbacks);

        scheduler.end_frame();
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    }

    #[test]
    fn test_transient_callback_receives_vsync() {
        let scheduler = UpdateScheduler::new();
        let received_time = Arc::new(Mutex::new(None));

        let rt = Arc::clone(&received_time);
        scheduler.schedule_frame_callback(Box::new(move |vsync_time| {
            *rt.lock() = Some(vsync_time);
        }));

        let vsync = Instant::now();
        scheduler.handle_begin_frame(vsync);
        scheduler.handle_draw_frame();

        let received = received_time.lock().unwrap();
        assert_eq!(received, vsync);
    }

    /// The platform wake hook fires exactly once per `frame_scheduled`
    /// false→true transition: re-requesting a pending frame stays
    /// silent, and `handle_begin_frame` re-arms the edge. A ticker that
    /// re-registers its callback during the frame therefore wakes the
    /// platform for the NEXT frame — the self-sustaining animation loop.
    #[test]
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
        scheduler.handle_begin_frame(Instant::now());
        scheduler.schedule_frame_callback(Box::new(|_| {}));
        scheduler.handle_draw_frame();
        assert_eq!(
            fired.load(Ordering::SeqCst),
            2,
            "re-registration after begin_frame must wake the next frame",
        );
    }

    #[test]
    fn test_microtask_execution() {
        let scheduler = UpdateScheduler::new();
        let executed = Arc::new(Mutex::new(false));

        let e = Arc::clone(&executed);
        scheduler.schedule_microtask(Box::new(move || {
            *e.lock() = true;
        }));

        scheduler.execute_frame();
        assert!(*executed.lock());
    }

    #[test]
    fn test_scheduler_frame_lifecycle() {
        let scheduler = UpdateScheduler::new();
        assert!(!scheduler.is_frame_scheduled());

        scheduler.request_frame();
        assert!(scheduler.is_frame_scheduled());

        let frame_id = scheduler.execute_frame();
        assert!(frame_id.get() > 0);
        assert!(!scheduler.is_frame_scheduled());
    }

    #[test]
    fn test_task_execution_priority() {
        let scheduler = UpdateScheduler::new();
        let counter = Arc::new(Mutex::new(Vec::new()));

        let c1 = Arc::clone(&counter);
        scheduler.add_task(Priority::Idle, move || c1.lock().push(4));

        let c2 = Arc::clone(&counter);
        scheduler.add_task(Priority::UserInput, move || c2.lock().push(1));

        let c3 = Arc::clone(&counter);
        scheduler.add_task(Priority::Build, move || c3.lock().push(3));

        let c4 = Arc::clone(&counter);
        scheduler.add_task(Priority::Animation, move || c4.lock().push(2));

        scheduler.execute_frame();

        assert_eq!(*counter.lock(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_post_frame_callback() {
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        scheduler.add_post_frame_callback(Box::new(move |_| {
            *c.lock() = true;
        }));

        scheduler.execute_frame();
        assert!(*called.lock());
    }

    #[test]
    fn test_persistent_callback() {
        let scheduler = UpdateScheduler::new();
        let count = Arc::new(Mutex::new(0));

        let c = Arc::clone(&count);
        scheduler.add_persistent_frame_callback(Arc::new(move |_| {
            *c.lock() += 1;
        }));

        scheduler.execute_frame();
        scheduler.execute_frame();
        scheduler.execute_frame();

        assert_eq!(*count.lock(), 3);
    }

    #[test]
    fn test_frame_count() {
        let scheduler = UpdateScheduler::new();

        assert_eq!(scheduler.frame_count(), 0);

        scheduler.execute_frame();
        assert_eq!(scheduler.frame_count(), 1);

        scheduler.execute_frame();
        assert_eq!(scheduler.frame_count(), 2);
    }

    #[test]
    fn test_scheduler_builder() {
        // `target_fps` configures only the internal stats budget's label —
        // UpdateScheduler itself exposes no `target_fps()` accessor; read it
        // back through `budget_snapshot()` instead.
        let scheduler = SchedulerBuilder::new()
            .target_fps(120)
            .task_queue_capacity(100)
            .build();

        assert!((scheduler.budget_snapshot().target_fps() as i32 - 120).abs() <= 1);
    }

    #[test]
    fn test_warm_up_frame() {
        let scheduler = UpdateScheduler::new();

        assert_eq!(scheduler.frame_count(), 0);

        scheduler.schedule_warm_up_frame();
        assert_eq!(scheduler.frame_count(), 1);

        // Second call should be no-op
        scheduler.schedule_warm_up_frame();
        assert_eq!(scheduler.frame_count(), 1);
    }

    // Callback Cancellation Tests

    #[test]
    fn test_cancel_transient_callback() {
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        let id = scheduler.schedule_frame_callback(Box::new(move |_| {
            *c.lock() = true;
        }));

        // Cancel before frame executes
        assert!(scheduler.cancel_frame_callback(id));

        scheduler.execute_frame();

        // Callback should NOT have been called
        assert!(!*called.lock());
    }

    #[test]
    fn test_persistent_callback_fires_every_frame() {
        // Flutter parity: binding.dart:773 "Persistent frame callbacks
        // cannot be unregistered. Once registered, they are called for every
        // frame for the lifetime of the application." FLUI previously
        // diverged with `remove_persistent_frame_callback`; reverted to
        // strict Flutter contract.
        let scheduler = UpdateScheduler::new();
        let count = Arc::new(Mutex::new(0));

        let c = Arc::clone(&count);
        scheduler.add_persistent_frame_callback(Arc::new(move |_| {
            *c.lock() += 1;
        }));

        // Persistent fires every frame, no way to remove.
        scheduler.execute_frame();
        scheduler.execute_frame();
        scheduler.execute_frame();
        assert_eq!(*count.lock(), 3);
    }

    #[test]
    fn test_post_frame_callback_fires_exactly_once() {
        // Flutter parity: binding.dart:802 "Post-frame callbacks ... are
        // called exactly once" and cannot be cancelled before they fire.
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(0));

        let c = Arc::clone(&called);
        scheduler.add_post_frame_callback(Box::new(move |_| {
            *c.lock() += 1;
        }));

        scheduler.execute_frame();
        scheduler.execute_frame();

        // Post-frame fires exactly once even across multiple frames.
        assert_eq!(*called.lock(), 1);
    }

    #[test]
    fn test_callback_id_uniqueness() {
        let scheduler = UpdateScheduler::new();

        let id1 = scheduler.schedule_frame_callback(Box::new(|_| {}));
        let id2 = scheduler.schedule_frame_callback(Box::new(|_| {}));
        // persistent + post-frame no longer return CallbackId; only the
        // transient schedule_frame_callback does. Both transient IDs differ.
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_cancel_nonexistent_callback() {
        let scheduler = UpdateScheduler::new();

        // Create an ID for a callback
        let id = scheduler.schedule_frame_callback(Box::new(|_| {}));

        // Execute frame (callback consumed)
        scheduler.execute_frame();

        // Trying to cancel after execution returns false
        assert!(!scheduler.cancel_frame_callback(id));
    }

    // Lifecycle State Tests

    #[test]
    fn test_lifecycle_state_default() {
        let scheduler = UpdateScheduler::new();

        // Default state should be Resumed
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Resumed);
        assert!(scheduler.should_schedule_frame());
    }

    #[test]
    fn test_lifecycle_state_change() {
        let scheduler = UpdateScheduler::new();

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Hidden);
        assert!(!scheduler.should_schedule_frame());

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Resumed);
        assert!(scheduler.should_schedule_frame());
    }

    #[test]
    fn test_lifecycle_listener() {
        let scheduler = UpdateScheduler::new();
        let received_state = Arc::new(Mutex::new(None));

        let rs = Arc::clone(&received_state);
        let id = scheduler.add_lifecycle_state_listener(Arc::new(move |state| {
            *rs.lock() = Some(state);
        }));

        // Change state - listener should be called
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Inactive);
        assert_eq!(*received_state.lock(), Some(AppLifecycleState::Inactive));

        // Remove listener
        assert!(scheduler.remove_lifecycle_state_listener(id));

        // Change state again - listener should NOT be called
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        // State in listener callback should still be Inactive (not updated)
        assert_eq!(*received_state.lock(), Some(AppLifecycleState::Inactive));
    }

    #[test]
    fn test_lifecycle_listener_not_called_on_same_state() {
        let scheduler = UpdateScheduler::new();
        let call_count = Arc::new(Mutex::new(0));

        let cc = Arc::clone(&call_count);
        scheduler.add_lifecycle_state_listener(Arc::new(move |_| {
            *cc.lock() += 1;
        }));

        // Change to same state (Resumed -> Resumed) should NOT notify
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(*call_count.lock(), 0);

        // Change to different state should notify
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(*call_count.lock(), 1);
    }

    /// Flutter parity leg (binding.dart `_setFramesEnabledState(true)` @
    /// 3.44.0): the disabled→enabled edge must actually schedule a frame,
    /// through the real `request_frame` path so `on_frame_scheduled` fires —
    /// otherwise a resumed app never wakes an idle event loop.
    #[test]
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

    /// Resumed -> Inactive keeps `frames_enabled` true -> true (Inactive
    /// still renders — visible-but-unfocused). No edge, so no frame is
    /// (re)scheduled by the lifecycle handler itself.
    #[test]
    fn lifecycle_resumed_to_inactive_schedules_nothing() {
        let scheduler = UpdateScheduler::new();
        let wakes = Arc::new(AtomicU64::new(0));
        let wakes_for_hook = Arc::clone(&wakes);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            wakes_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Inactive);
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert!(!scheduler.is_frame_scheduled());
    }

    /// A repeated same-state transition is a no-op on every axis: no
    /// listener call (already covered above), and — the frames_enabled edge
    /// this test targets — no frame (re)scheduled either.
    #[test]
    fn lifecycle_repeated_same_state_schedules_nothing() {
        let scheduler = UpdateScheduler::new();
        let wakes = Arc::new(AtomicU64::new(0));
        let wakes_for_hook = Arc::clone(&wakes);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            wakes_for_hook.fetch_add(1, Ordering::Relaxed);
        })));

        // Resumed -> Resumed: already enabled, not an edge.
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert!(!scheduler.is_frame_scheduled());
    }

    #[test]
    fn test_app_lifecycle_state_properties() {
        // Resumed
        assert!(AppLifecycleState::Resumed.is_visible());
        assert!(AppLifecycleState::Resumed.is_focused());
        assert!(AppLifecycleState::Resumed.should_animate());
        assert!(AppLifecycleState::Resumed.should_render());
        assert!(!AppLifecycleState::Resumed.should_save_state());

        // Inactive
        assert!(AppLifecycleState::Inactive.is_visible());
        assert!(!AppLifecycleState::Inactive.is_focused());
        assert!(!AppLifecycleState::Inactive.should_animate());
        assert!(AppLifecycleState::Inactive.can_animate());
        assert!(AppLifecycleState::Inactive.should_render());

        // Hidden
        assert!(!AppLifecycleState::Hidden.is_visible());
        assert!(!AppLifecycleState::Hidden.should_render());
        assert!(AppLifecycleState::Hidden.should_release_resources());

        // Paused
        assert!(!AppLifecycleState::Paused.should_render());
        assert!(AppLifecycleState::Paused.should_save_state());

        // Detached
        assert!(AppLifecycleState::Detached.should_save_state());
        assert!(AppLifecycleState::Detached.should_release_resources());
    }

    // Frame Completion Future Tests

    #[test]
    fn test_end_of_frame_future_creation() {
        let scheduler = UpdateScheduler::new();

        // Should be able to create multiple futures
        let _future1 = scheduler.end_of_frame();
        let _future2 = scheduler.end_of_frame();

        // Both futures should be registered
        assert_eq!(scheduler.completion_waiter_count(), 2);
    }

    #[test]
    fn test_end_of_frame_completes_after_frame() {
        use std::task::Waker;

        let scheduler = UpdateScheduler::new();

        // Create a future
        let mut future = scheduler.end_of_frame();

        // Create a no-op waker for polling
        let mut cx = Context::from_waker(Waker::noop());

        // Should be pending before frame
        let result = Pin::new(&mut future).poll(&mut cx);
        assert!(result.is_pending());

        // Execute a frame
        scheduler.execute_frame();

        // Create a new future (old one was already notified)
        let mut future2 = scheduler.end_of_frame();

        // Should still be pending (new future, next frame not executed)
        let result2 = Pin::new(&mut future2).poll(&mut cx);
        assert!(result2.is_pending());

        // Execute another frame
        scheduler.execute_frame();

        // Now should be ready
        let result3 = Pin::new(&mut future2).poll(&mut cx);
        assert!(result3.is_ready());

        if let Poll::Ready(timing) = result3 {
            assert!(timing.id.get() > 0);
        }
    }

    #[test]
    fn test_multiple_waiters_notified() {
        use std::task::Waker;

        let scheduler = UpdateScheduler::new();

        let mut cx = Context::from_waker(Waker::noop());

        // Create multiple futures
        let mut future1 = scheduler.end_of_frame();
        let mut future2 = scheduler.end_of_frame();
        let mut future3 = scheduler.end_of_frame();

        // Poll all to register wakers
        let _ = Pin::new(&mut future1).poll(&mut cx);
        let _ = Pin::new(&mut future2).poll(&mut cx);
        let _ = Pin::new(&mut future3).poll(&mut cx);

        // Execute frame
        scheduler.execute_frame();

        // All should be ready now
        let r1 = Pin::new(&mut future1).poll(&mut cx);
        let r2 = Pin::new(&mut future2).poll(&mut cx);
        let r3 = Pin::new(&mut future3).poll(&mut cx);

        assert!(r1.is_ready());
        assert!(r2.is_ready());
        assert!(r3.is_ready());
    }

    // Idle Callback Tests

    #[test]
    fn test_idle_callback_execution() {
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        scheduler.schedule_idle_callback(move || {
            *c.lock() = true;
        });

        assert!(scheduler.has_idle_callbacks());

        // Execute idle callbacks (no frame scheduled, queue empty, Resumed state)
        let count = scheduler.execute_idle_callbacks();
        assert_eq!(count, 1);
        assert!(*called.lock());
        assert!(!scheduler.has_idle_callbacks());
    }

    #[test]
    fn test_idle_callbacks_skip_when_frame_scheduled() {
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        scheduler.schedule_idle_callback(move || {
            *c.lock() = true;
        });

        // Schedule a frame — idle callbacks should NOT run
        scheduler.request_frame();
        let count = scheduler.execute_idle_callbacks();
        assert_eq!(count, 0);
        assert!(!*called.lock());
    }

    #[test]
    fn test_idle_callbacks_skip_when_hidden() {
        let scheduler = UpdateScheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        scheduler.schedule_idle_callback(move || {
            *c.lock() = true;
        });

        // Hide app — idle callbacks should NOT run
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        let count = scheduler.execute_idle_callbacks();
        assert_eq!(count, 0);
        assert!(!*called.lock());
    }

    #[test]
    fn test_multiple_idle_callbacks() {
        let scheduler = UpdateScheduler::new();
        let counter = Arc::new(Mutex::new(0));

        for _ in 0..5 {
            let c = Arc::clone(&counter);
            scheduler.schedule_idle_callback(move || {
                *c.lock() += 1;
            });
        }

        let count = scheduler.execute_idle_callbacks();
        assert_eq!(count, 5);
        assert_eq!(*counter.lock(), 5);
    }

    // =========================================================================
    // Teardown pins (scheduler realm-ownership) — `WeakUpdateScheduler` must
    // fail closed once its backing scheduler's last strong reference drops,
    // never observe stale state, and never panic.
    // =========================================================================

    /// After every strong `UpdateScheduler` reference is dropped, a retained
    /// `WeakUpdateScheduler::upgrade()` must return `None` — this is the whole
    /// point of Q1's shape: a realm's teardown must be observable by every
    /// handle it vended, not just its own owner.
    #[test]
    fn weak_scheduler_upgrade_returns_none_after_the_scheduler_drops() {
        let scheduler = UpdateScheduler::new();
        let weak = scheduler.downgrade();

        assert!(
            weak.upgrade().is_some(),
            "precondition: the scheduler is still alive"
        );

        drop(scheduler);

        assert!(
            weak.upgrade().is_none(),
            "WeakUpdateScheduler::upgrade() must return None once the backing \
             scheduler's last strong reference is gone"
        );
    }

    /// A ticker attached via `new_with_scheduler` must not keep the
    /// scheduler alive (no strong cycle): dropping every other strong
    /// reference to the scheduler while the ticker is still running must
    /// let the scheduler's inner allocation actually go away.
    #[test]
    fn ticker_does_not_keep_a_dropped_schedulers_inner_state_alive() {
        let scheduler = UpdateScheduler::new();
        let weak = scheduler.downgrade();
        let mut ticker = crate::ticker::Ticker::new_with_scheduler(&scheduler);
        let _future = ticker.start(|_| {});

        // The ticker's auto-schedule registered a transient callback whose
        // closure captures a WEAK scheduler — if it captured a strong one
        // instead (the earlier shared-singleton shape), this drop would not be the last
        // strong reference and the assertion below would fail.
        drop(scheduler);

        assert!(
            weak.upgrade().is_none(),
            "a live, started ticker must not pin the scheduler's inner \
             state alive via a strong capture in its own transient-callback \
             queue entry"
        );

        // The ticker itself must remain safely disposable afterward.
        ticker.dispose();
    }
}
