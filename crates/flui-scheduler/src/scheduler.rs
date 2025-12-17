//! Main scheduler - coordinates frame lifecycle and task execution
//!
//! The Scheduler is the central orchestrator for FLUI's rendering pipeline,
//! following Flutter's SchedulerBinding model with proper phase separation.
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
//! use flui_scheduler::{Scheduler, Priority};
//! use flui_scheduler::traits::AnimationPriority;
//!
//! let scheduler = Scheduler::new();
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

use crate::budget::FrameBudget;
use crate::duration::{FrameDuration, Milliseconds};
use crate::frame::{
    AppLifecycleState, FrameCallback, FrameId, FramePhase, FrameTiming, PersistentFrameCallback,
    PostFrameCallback, SchedulerPhase, TransientFrameCallback,
};
use crate::id::{CallbackIdMarker, IdGenerator, TypedId};
use crate::task::{Priority, TaskQueue};
use crate::ticker::TickerProvider;
use crate::traits::PriorityLevel;
use crate::vsync::VsyncScheduler;
use dashmap::DashMap;
use flui_foundation::{impl_binding_singleton, BindingBase};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use web_time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier for callbacks (transient, persistent, post-frame)
pub type CallbackId = TypedId<CallbackIdMarker>;

/// Cancellable transient callback with ID
struct CancellableTransientCallback {
    id: CallbackId,
    callback: TransientFrameCallback,
}

/// Cancellable persistent callback with ID
struct CancellablePersistentCallback {
    id: CallbackId,
    callback: PersistentFrameCallback,
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
/// This is returned by `Scheduler::end_of_frame()` and allows awaiting
/// the completion of the current or next frame.
///
/// # Example
///
/// ```rust,ignore
/// use flui_scheduler::Scheduler;
///
/// async fn do_end_of_frame_work(scheduler: &Scheduler) {
///     // Wait for frame to complete
///     let timing = scheduler.end_of_frame().await;
///
///     // Now safe to do post-frame cleanup
///     println!("Frame {} completed in {}ms",
///         timing.id.as_u64(),
///         timing.elapsed().value());
/// }
/// ```
pub struct FrameCompletionFuture {
    state: Arc<Mutex<FrameCompletionState>>,
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

/// Frame skip policy - determines behavior when catching up from frame drops
///
/// When the application falls behind (missing vsync signals), this policy
/// determines whether to render every frame (potentially falling further behind)
/// or skip frames to catch up.
///
/// # Flutter Comparison
///
/// Flutter uses a similar approach with `timeDilation` and frame scheduling.
/// The key insight is that animations should advance by real elapsed time,
/// but rendering can be skipped if we're behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum FrameSkipPolicy {
    /// Never skip frames - always render, even if behind
    ///
    /// Use this for applications where every frame matters (video playback).
    /// May cause increasing latency if consistently over budget.
    Never = 0,

    /// Skip frames if more than one frame behind (default)
    ///
    /// This balances responsiveness with animation smoothness.
    /// Animations advance by real time, skipped frames just don't render.
    #[default]
    CatchUp = 1,

    /// Aggressively skip to latest vsync
    ///
    /// Only render the most recent frame, skipping all intermediate ones.
    /// Best for input latency, but animations may appear to "jump".
    SkipToLatest = 2,

    /// Limit skips to a maximum count
    ///
    /// Skip up to N frames, then render regardless. Prevents animations
    /// from jumping too far ahead after a long stall.
    LimitedSkip = 3,
}

impl FrameSkipPolicy {
    /// Try to convert from u8 representation
    ///
    /// Returns `None` if the value is not a valid discriminant.
    #[inline]
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Never),
            1 => Some(Self::CatchUp),
            2 => Some(Self::SkipToLatest),
            3 => Some(Self::LimitedSkip),
            _ => None,
        }
    }

    /// Convert from u8 representation (for atomic storage)
    ///
    /// # Panics
    /// Panics if the value is not a valid FrameSkipPolicy discriminant.
    /// For fallible conversion, use [`try_from_u8`](Self::try_from_u8).
    #[inline]
    pub const fn from_u8(value: u8) -> Self {
        match Self::try_from_u8(value) {
            Some(v) => v,
            None => panic!("Invalid FrameSkipPolicy value"),
        }
    }

    /// Calculate how many frames to skip given elapsed time since last frame
    ///
    /// # Arguments
    /// * `elapsed_ms` - Time since last frame was rendered
    /// * `frame_budget_ms` - Target frame duration (e.g., 16.67ms for 60fps)
    /// * `max_skip` - Maximum frames to skip for `LimitedSkip` policy
    ///
    /// # Returns
    /// Number of frames to skip (0 means render this frame)
    pub fn frames_to_skip(self, elapsed_ms: f64, frame_budget_ms: f64, max_skip: u32) -> u32 {
        if frame_budget_ms <= 0.0 || elapsed_ms <= 0.0 {
            return 0;
        }

        // How many frame intervals have passed?
        let frames_behind = (elapsed_ms / frame_budget_ms).floor() as u32;

        match self {
            Self::Never => 0,
            Self::CatchUp => {
                // Skip if more than 1 frame behind
                if frames_behind > 1 {
                    frames_behind.saturating_sub(1)
                } else {
                    0
                }
            }
            Self::SkipToLatest => {
                // Skip all but the latest
                frames_behind.saturating_sub(1)
            }
            Self::LimitedSkip => {
                // Skip up to max_skip frames
                let skip = frames_behind.saturating_sub(1);
                skip.min(max_skip)
            }
        }
    }

    /// Check if this policy allows skipping
    #[inline]
    pub const fn allows_skip(self) -> bool {
        !matches!(self, Self::Never)
    }

    /// Get a description of this policy
    pub const fn description(self) -> &'static str {
        match self {
            Self::Never => "Never skip frames",
            Self::CatchUp => "Skip to catch up when behind",
            Self::SkipToLatest => "Always skip to latest frame",
            Self::LimitedSkip => "Skip up to a maximum count",
        }
    }
}

/// Main scheduler for frame and task management
///
/// Implements Flutter-like scheduling with proper phase separation:
/// - TransientCallbacks: Animation tickers
/// - PersistentCallbacks: Rendering pipeline
/// - PostFrameCallbacks: Cleanup
///
/// ## Callback Cancellation
///
/// All callback registration methods return a `CallbackId` that can be used to cancel:
///
/// ```rust
/// use flui_scheduler::Scheduler;
///
/// let scheduler = Scheduler::new();
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
pub struct Scheduler {
    /// Current scheduler phase (Flutter-like state machine)
    /// Optimized: AtomicU8 for lock-free phase transitions
    scheduler_phase: Arc<AtomicU8>,

    /// Current frame timing
    current_frame: Arc<Mutex<Option<FrameTiming>>>,

    /// VSync timestamp for current frame (all tickers use this)
    current_vsync_time: Arc<Mutex<Option<Instant>>>,

    /// Task queue (priority-based)
    task_queue: TaskQueue,

    /// Transient callbacks - animation tickers (one-time, fires during TransientCallbacks)
    transient_callbacks: Arc<Mutex<Vec<CancellableTransientCallback>>>,

    /// Cancelled callback IDs (checked before execution)
    /// Optimized: DashMap for lock-free concurrent access
    cancelled_callbacks: Arc<DashMap<CallbackId, ()>>,

    /// Callback ID generator
    callback_id_gen: Arc<IdGenerator<CallbackIdMarker>>,

    /// Frame callbacks (legacy, executed after transient)
    frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,

    /// Persistent frame callbacks (executed every frame during PersistentCallbacks)
    persistent_callbacks: Arc<Mutex<Vec<CancellablePersistentCallback>>>,

    /// Post-frame callbacks (executed after frame completes)
    post_frame_callbacks: Arc<Mutex<Vec<CancellablePostFrameCallback>>>,

    /// Microtask queue (executed during MidFrameMicrotasks)
    #[allow(clippy::type_complexity)]
    microtasks: Arc<Mutex<VecDeque<Box<dyn FnOnce() + Send>>>>,

    /// Frame duration configuration
    frame_duration: Arc<Mutex<FrameDuration>>,

    /// Frame budget management
    budget: Arc<Mutex<FrameBudget>>,

    /// Whether a frame is currently scheduled
    /// Optimized: AtomicBool for lock-free access
    frame_scheduled: Arc<AtomicBool>,

    /// Frame counter
    /// Optimized: AtomicU64 for lock-free increment
    frame_count: Arc<AtomicU64>,

    /// VSync scheduler (optional integration)
    vsync: Arc<Mutex<Option<VsyncScheduler>>>,

    /// Jank tracking - frames that exceeded budget
    janky_frames: Arc<Mutex<Vec<FrameId>>>,

    /// Whether warm-up frame was executed
    /// Optimized: AtomicBool for lock-free access
    warm_up_done: Arc<AtomicBool>,

    /// Frame skip policy for catching up
    /// Optimized: AtomicU8 for lock-free access
    frame_skip_policy: Arc<AtomicU8>,

    /// Maximum frames to skip (for LimitedSkip policy)
    max_frame_skip: Arc<Mutex<u32>>,

    /// Last frame end time (for skip calculation)
    last_frame_end: Arc<Mutex<Option<Instant>>>,

    /// Skipped frame counter
    /// Optimized: AtomicU64 for lock-free increment
    skipped_frames: Arc<AtomicU64>,

    /// Current application lifecycle state
    /// Optimized: AtomicU8 for lock-free access
    lifecycle_state: Arc<AtomicU8>,

    /// Lifecycle state change listeners
    lifecycle_listeners: Arc<Mutex<Vec<LifecycleListener>>>,

    /// Pending frame completion futures waiting to be notified
    frame_completion_waiters: Arc<Mutex<Vec<FrameCompletionNotifier>>>,

    /// Binding state for SchedulerBinding trait implementation
    /// Stored per-instance for proper test isolation
    pub(crate) binding_state: Arc<Mutex<crate::binding::SchedulerBindingState>>,
}

impl Scheduler {
    /// Create a new scheduler with 60 FPS target
    pub fn new() -> Self {
        Self::with_frame_duration(FrameDuration::FPS_60)
    }

    /// Create a scheduler with custom target FPS
    pub fn with_target_fps(target_fps: u32) -> Self {
        Self::with_frame_duration(FrameDuration::from_fps(target_fps))
    }

    /// Create a scheduler with specific frame duration
    pub fn with_frame_duration(frame_duration: FrameDuration) -> Self {
        let target_fps = frame_duration.fps() as u32;
        Self {
            scheduler_phase: Arc::new(AtomicU8::new(SchedulerPhase::Idle as u8)),
            current_frame: Arc::new(Mutex::new(None)),
            current_vsync_time: Arc::new(Mutex::new(None)),
            task_queue: TaskQueue::new(),
            transient_callbacks: Arc::new(Mutex::new(Vec::new())),
            cancelled_callbacks: Arc::new(DashMap::new()),
            callback_id_gen: Arc::new(IdGenerator::new()),
            frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            persistent_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            microtasks: Arc::new(Mutex::new(VecDeque::new())),
            frame_duration: Arc::new(Mutex::new(frame_duration)),
            budget: Arc::new(Mutex::new(FrameBudget::new(target_fps))),
            frame_scheduled: Arc::new(AtomicBool::new(false)),
            frame_count: Arc::new(AtomicU64::new(0)),
            vsync: Arc::new(Mutex::new(None)),
            janky_frames: Arc::new(Mutex::new(Vec::new())),
            warm_up_done: Arc::new(AtomicBool::new(false)),
            frame_skip_policy: Arc::new(AtomicU8::new(FrameSkipPolicy::default() as u8)),
            max_frame_skip: Arc::new(Mutex::new(3)), // Default max 3 frame skips
            last_frame_end: Arc::new(Mutex::new(None)),
            skipped_frames: Arc::new(AtomicU64::new(0)),
            lifecycle_state: Arc::new(AtomicU8::new(AppLifecycleState::Resumed as u8)),
            lifecycle_listeners: Arc::new(Mutex::new(Vec::new())),
            frame_completion_waiters: Arc::new(Mutex::new(Vec::new())),
            binding_state: Arc::new(Mutex::new(crate::binding::SchedulerBindingState::new())),
        }
    }

    // =========================================================================
    // Phase Management (Flutter-like)
    // =========================================================================

    /// Get current scheduler phase
    pub fn phase(&self) -> SchedulerPhase {
        SchedulerPhase::from_u8(self.scheduler_phase.load(Ordering::Acquire))
    }

    /// Set scheduler phase with validation
    fn set_scheduler_phase(&self, new_phase: SchedulerPhase) {
        let current = SchedulerPhase::from_u8(self.scheduler_phase.load(Ordering::Acquire));
        debug_assert!(
            current.can_transition_to(new_phase),
            "Invalid phase transition: {:?} -> {:?}",
            current,
            new_phase
        );
        self.scheduler_phase
            .store(new_phase as u8, Ordering::Release);
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
    /// Executes transient callbacks (animation tickers) with the vsync timestamp.
    pub fn handle_begin_frame(&self, vsync_time: Instant) -> FrameId {
        // Store vsync time for all tickers to use
        *self.current_vsync_time.lock() = Some(vsync_time);

        // Create frame timing with vsync timestamp
        let frame_duration = *self.frame_duration.lock();
        let mut timing = FrameTiming::with_duration(frame_duration);
        timing.start_time = vsync_time;
        timing.phase = FramePhase::Build;

        let frame_id = timing.id;
        *self.current_frame.lock() = Some(timing);
        self.frame_scheduled.store(false, Ordering::Release);
        self.frame_count.fetch_add(1, Ordering::Relaxed);

        // Phase 1: TransientCallbacks (animation tickers)
        self.set_scheduler_phase(SchedulerPhase::TransientCallbacks);

        // Execute transient callbacks (animations get vsync timestamp)
        let transient = {
            let mut cbs = self.transient_callbacks.lock();
            std::mem::take(&mut *cbs)
        };

        for cancellable in transient {
            // Skip if cancelled (DashMap provides lock-free contains_key)
            if self.cancelled_callbacks.contains_key(&cancellable.id) {
                continue;
            }
            (cancellable.callback)(vsync_time);
        }

        // Clear processed cancellations
        self.cancelled_callbacks.clear();

        // Execute legacy frame callbacks
        let callbacks = {
            let mut cbs = self.frame_callbacks.lock();
            std::mem::take(&mut *cbs)
        };

        for callback in callbacks {
            if let Some(timing) = self.current_frame.lock().as_ref() {
                callback(timing);
            }
        }

        // Phase 2: MidFrameMicrotasks
        self.set_scheduler_phase(SchedulerPhase::MidFrameMicrotasks);
        self.flush_microtasks();

        frame_id
    }

    /// Handle draw frame - called after begin frame to run rendering pipeline
    ///
    /// This corresponds to Flutter's `handleDrawFrame`.
    /// Executes persistent callbacks (rendering pipeline).
    pub fn handle_draw_frame(&self) {
        // Phase 3: PersistentCallbacks (rendering pipeline)
        self.set_scheduler_phase(SchedulerPhase::PersistentCallbacks);

        // Reset budget at start of rendering
        self.budget.lock().reset();

        // Execute persistent frame callbacks (rendering pipeline)
        // Note: We check cancelled_callbacks for each persistent callback
        // This allows removing callbacks between frames
        let persistent_callbacks = {
            let cbs = self.persistent_callbacks.lock();
            cbs.iter()
                .map(|c| (c.id, c.callback.clone()))
                .collect::<Vec<_>>()
        };

        for (id, callback) in persistent_callbacks.iter() {
            // DashMap provides lock-free contains_key
            if self.cancelled_callbacks.contains_key(id) {
                continue;
            }
            if let Some(timing) = self.current_frame.lock().as_ref() {
                callback(timing);
            }
        }

        // Execute priority tasks (always execute all - don't skip UI updates!)
        self.task_queue.execute_all();

        // Phase 4: PostFrameCallbacks
        self.set_scheduler_phase(SchedulerPhase::PostFrameCallbacks);

        let timing = self.current_frame.lock().take();

        if let Some(timing) = timing {
            // Record timing and check for jank
            let elapsed = timing.elapsed();
            self.budget.lock().record_frame_duration(elapsed);

            if timing.is_janky() {
                self.janky_frames.lock().push(timing.id);
            }

            // Execute post-frame callbacks
            let callbacks = {
                let mut cbs = self.post_frame_callbacks.lock();
                std::mem::take(&mut *cbs)
            };

            for cancellable in callbacks {
                // DashMap provides lock-free contains_key
                if self.cancelled_callbacks.contains_key(&cancellable.id) {
                    continue;
                }
                (cancellable.callback)(&timing);
            }

            // Clear processed cancellations
            self.cancelled_callbacks.clear();

            // Notify frame completion futures
            self.notify_frame_completion(&timing);
        }

        // Return to idle
        self.set_scheduler_phase(SchedulerPhase::Idle);
        *self.current_vsync_time.lock() = None;

        // Record frame end time for skip calculations
        *self.last_frame_end.lock() = Some(Instant::now());
    }

    /// Execute a complete frame (convenience method)
    ///
    /// Calls handle_begin_frame and handle_draw_frame in sequence.
    /// Use this for simple cases; for proper vsync integration,
    /// call handle_begin_frame and handle_draw_frame separately.
    pub fn execute_frame(&self) -> FrameId {
        let vsync_time = Instant::now();
        let frame_id = self.handle_begin_frame(vsync_time);
        self.handle_draw_frame();
        frame_id
    }

    /// Schedule a warm-up frame (synchronous, no vsync wait)
    ///
    /// This forces an immediate frame to be processed, useful for:
    /// - App initialization
    /// - Reducing first-frame jank
    /// - Forcing immediate layout updates
    pub fn schedule_warm_up_frame(&self) {
        if self.warm_up_done.load(Ordering::Acquire) {
            return;
        }

        // Execute frame immediately without vsync
        self.execute_frame();
        self.warm_up_done.store(true, Ordering::Release);
    }

    // =========================================================================
    // Callback Registration
    // =========================================================================

    /// Schedule a transient frame callback (animation)
    ///
    /// The callback receives the vsync timestamp and fires during TransientCallbacks phase.
    /// This is the correct way for tickers to receive frame timing.
    ///
    /// Returns a `CallbackId` that can be used to cancel the callback before it fires.
    pub fn schedule_frame_callback(&self, callback: TransientFrameCallback) -> CallbackId {
        let id = self.callback_id_gen.next();
        self.transient_callbacks
            .lock()
            .push(CancellableTransientCallback { id, callback });
        self.frame_scheduled.store(true, Ordering::Release);
        id
    }

    /// Cancel a transient frame callback by ID
    ///
    /// Returns `true` if the callback was found and cancelled, `false` if it was
    /// already executed or not found.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::Scheduler;
    ///
    /// let scheduler = Scheduler::new();
    /// let id = scheduler.schedule_frame_callback(Box::new(|_| {
    ///     println!("This won't be called!");
    /// }));
    /// scheduler.cancel_frame_callback(id);
    /// ```
    pub fn cancel_frame_callback(&self, id: CallbackId) -> bool {
        // First, try to remove from pending callbacks
        let mut callbacks = self.transient_callbacks.lock();
        let original_len = callbacks.len();
        callbacks.retain(|c| c.id != id);

        if callbacks.len() < original_len {
            return true; // Found and removed
        }

        // If not found, mark as cancelled (in case it's about to be executed)
        self.cancelled_callbacks.insert(id, ());
        false
    }

    /// Schedule a legacy frame callback
    pub fn schedule_frame(&self, callback: FrameCallback) {
        self.frame_callbacks.lock().push(callback);
        self.frame_scheduled.store(true, Ordering::Release);
    }

    /// Request a frame (without callback)
    pub fn request_frame(&self) {
        self.frame_scheduled.store(true, Ordering::Release);
    }

    /// Add a persistent frame callback
    ///
    /// Fires every frame during PersistentCallbacks phase.
    /// Use for the rendering pipeline (build/layout/paint).
    ///
    /// Returns a `CallbackId` that can be used to remove the callback.
    pub fn add_persistent_frame_callback(&self, callback: PersistentFrameCallback) -> CallbackId {
        let id = self.callback_id_gen.next();
        self.persistent_callbacks
            .lock()
            .push(CancellablePersistentCallback { id, callback });
        id
    }

    /// Remove a persistent frame callback by ID
    ///
    /// Returns `true` if the callback was found and removed.
    pub fn remove_persistent_frame_callback(&self, id: CallbackId) -> bool {
        let mut callbacks = self.persistent_callbacks.lock();
        let original_len = callbacks.len();
        callbacks.retain(|c| c.id != id);
        callbacks.len() < original_len
    }

    /// Add a post-frame callback
    ///
    /// Fires once after the current/next frame completes.
    ///
    /// Returns a `CallbackId` that can be used to cancel the callback.
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) -> CallbackId {
        let id = self.callback_id_gen.next();
        self.post_frame_callbacks
            .lock()
            .push(CancellablePostFrameCallback { id, callback });
        id
    }

    /// Cancel a post-frame callback by ID
    ///
    /// Returns `true` if the callback was found and cancelled.
    pub fn cancel_post_frame_callback(&self, id: CallbackId) -> bool {
        let mut callbacks = self.post_frame_callbacks.lock();
        let original_len = callbacks.len();
        callbacks.retain(|c| c.id != id);

        if callbacks.len() < original_len {
            return true;
        }

        self.cancelled_callbacks.insert(id, ());
        false
    }

    // =========================================================================
    // Microtask Queue
    // =========================================================================

    /// Schedule a microtask
    ///
    /// Microtasks are executed during MidFrameMicrotasks phase,
    /// after animations but before rendering.
    pub fn schedule_microtask(&self, task: Box<dyn FnOnce() + Send>) {
        self.microtasks.lock().push_back(task);
    }

    /// Flush all pending microtasks
    fn flush_microtasks(&self) {
        loop {
            let task = self.microtasks.lock().pop_front();
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
        self.task_queue.add(priority, callback);
    }

    /// Add a task with compile-time priority checking
    pub fn add_task_typed<P: PriorityLevel>(&self, callback: impl FnOnce() + Send + 'static) {
        self.task_queue.add_typed::<P>(callback);
    }

    /// Get task queue reference
    pub fn task_queue(&self) -> &TaskQueue {
        &self.task_queue
    }

    // =========================================================================
    // Configuration
    // =========================================================================

    /// Set target FPS
    pub fn set_target_fps(&self, fps: u32) {
        let frame_duration = FrameDuration::from_fps(fps);
        *self.frame_duration.lock() = frame_duration;
        *self.budget.lock() = FrameBudget::new(fps);
    }

    /// Set frame duration directly
    pub fn set_frame_duration(&self, frame_duration: FrameDuration) {
        *self.frame_duration.lock() = frame_duration;
        *self.budget.lock() = FrameBudget::new(frame_duration.fps() as u32);
    }

    /// Get target FPS
    pub fn target_fps(&self) -> u32 {
        self.frame_duration.lock().fps() as u32
    }

    /// Get frame duration configuration
    pub fn frame_duration(&self) -> FrameDuration {
        *self.frame_duration.lock()
    }

    // =========================================================================
    // VSync Integration
    // =========================================================================

    /// Set VSync scheduler for integration
    pub fn set_vsync(&self, vsync: VsyncScheduler) {
        *self.vsync.lock() = Some(vsync);
    }

    /// Get VSync scheduler reference
    pub fn has_vsync(&self) -> bool {
        self.vsync.lock().is_some()
    }

    /// Get current vsync timestamp (if in frame)
    pub fn current_vsync_time(&self) -> Option<Instant> {
        *self.current_vsync_time.lock()
    }

    // =========================================================================
    // Frame State
    // =========================================================================

    /// Check if a frame is scheduled
    pub fn is_frame_scheduled(&self) -> bool {
        self.frame_scheduled.load(Ordering::Acquire)
    }

    /// Get the number of pending transient callbacks
    ///
    /// This is useful for debugging and testing to verify that
    /// all transient callbacks have been processed.
    pub fn transient_callback_count(&self) -> usize {
        self.transient_callbacks.lock().len()
    }

    /// Get current frame timing (if a frame is active)
    pub fn current_frame(&self) -> Option<FrameTiming> {
        *self.current_frame.lock()
    }

    /// Set the current frame phase (for rendering pipeline)
    pub fn set_phase(&self, phase: FramePhase) {
        if let Some(timing) = self.current_frame.lock().as_mut() {
            timing.phase = phase;
        }
    }

    /// Begin a new frame (legacy - use handle_begin_frame for new code)
    pub fn begin_frame(&self) -> FrameId {
        self.handle_begin_frame(Instant::now())
    }

    /// End the current frame (legacy - use handle_draw_frame for new code)
    pub fn end_frame(&self) {
        // Only complete if we're in a frame
        if self.phase() != SchedulerPhase::Idle {
            // Skip to post-frame if needed
            if self.phase() != SchedulerPhase::PostFrameCallbacks {
                // Force transition (skip validation for legacy API)
                self.scheduler_phase
                    .store(SchedulerPhase::PostFrameCallbacks as u8, Ordering::Release);
            }

            let timing = self.current_frame.lock().take();
            if let Some(timing) = timing {
                self.budget.lock().record_frame_duration(timing.elapsed());

                let callbacks = {
                    let mut cbs = self.post_frame_callbacks.lock();
                    std::mem::take(&mut *cbs)
                };

                for cancellable in callbacks {
                    // DashMap provides lock-free contains_key
                    if self.cancelled_callbacks.contains_key(&cancellable.id) {
                        continue;
                    }
                    (cancellable.callback)(&timing);
                }

                self.cancelled_callbacks.clear();

                // Notify frame completion futures
                self.notify_frame_completion(&timing);
            }

            self.scheduler_phase
                .store(SchedulerPhase::Idle as u8, Ordering::Release);
        }
    }

    // =========================================================================
    // Budget and Timing
    // =========================================================================

    /// Check if currently over budget
    pub fn is_over_budget(&self) -> bool {
        self.current_frame
            .lock()
            .as_ref()
            .is_some_and(|t| t.is_over_budget())
    }

    /// Check if deadline is near (>80% budget used)
    pub fn is_deadline_near(&self) -> bool {
        self.current_frame
            .lock()
            .as_ref()
            .is_some_and(|t| t.is_deadline_near())
    }

    /// Get remaining budget as type-safe Milliseconds
    pub fn remaining_budget(&self) -> Milliseconds {
        self.current_frame
            .lock()
            .as_ref()
            .map_or(Milliseconds::ZERO, |t| t.remaining())
    }

    /// Get remaining budget in milliseconds (raw f64)
    pub fn remaining_budget_ms(&self) -> f64 {
        self.remaining_budget().value()
    }

    /// Get frame budget reference
    pub fn budget(&self) -> Arc<Mutex<FrameBudget>> {
        Arc::clone(&self.budget)
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::Relaxed)
    }

    /// Get average FPS from budget statistics
    pub fn avg_fps(&self) -> f64 {
        self.budget.lock().avg_fps()
    }

    /// Check if last frame was janky
    pub fn is_janky(&self) -> bool {
        self.budget.lock().is_janky()
    }

    /// Get count of janky frames
    pub fn janky_frame_count(&self) -> usize {
        self.janky_frames.lock().len()
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
        self.janky_frames.lock().clear();
    }

    // =========================================================================
    // Frame Skip Policy
    // =========================================================================

    /// Set frame skip policy
    pub fn set_frame_skip_policy(&self, policy: FrameSkipPolicy) {
        self.frame_skip_policy
            .store(policy as u8, Ordering::Release);
    }

    /// Get current frame skip policy
    pub fn frame_skip_policy(&self) -> FrameSkipPolicy {
        FrameSkipPolicy::from_u8(self.frame_skip_policy.load(Ordering::Acquire))
    }

    /// Set maximum frames to skip (for LimitedSkip policy)
    pub fn set_max_frame_skip(&self, max: u32) {
        *self.max_frame_skip.lock() = max;
    }

    /// Get maximum frames to skip
    pub fn max_frame_skip(&self) -> u32 {
        *self.max_frame_skip.lock()
    }

    /// Get count of skipped frames
    pub fn skipped_frame_count(&self) -> u64 {
        self.skipped_frames.load(Ordering::Relaxed)
    }

    /// Calculate frames to skip based on current policy
    ///
    /// Call this before rendering to determine if this frame should be skipped.
    /// Returns the number of frames to skip (0 means render this frame).
    pub fn should_skip_frames(&self) -> u32 {
        let last_end = *self.last_frame_end.lock();
        let Some(last) = last_end else {
            return 0; // First frame, don't skip
        };

        let elapsed_ms = last.elapsed().as_secs_f64() * 1000.0;
        let frame_budget_ms = self.frame_duration.lock().as_ms().value();
        let policy = FrameSkipPolicy::from_u8(self.frame_skip_policy.load(Ordering::Acquire));
        let max_skip = *self.max_frame_skip.lock();

        policy.frames_to_skip(elapsed_ms, frame_budget_ms, max_skip)
    }

    /// Check if current frame should be skipped and record skip if so
    ///
    /// Returns `true` if this frame should be skipped, `false` if it should be rendered.
    /// If skipping, increments the skipped frame counter.
    pub fn check_and_skip_frame(&self) -> bool {
        let skip_count = self.should_skip_frames();

        if skip_count > 0 {
            self.skipped_frames
                .fetch_add(skip_count as u64, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Clear skip statistics
    pub fn clear_skip_stats(&self) {
        self.skipped_frames.store(0, Ordering::Relaxed);
    }

    /// Get skip rate as percentage
    pub fn skip_rate(&self) -> f64 {
        let total = self.frame_count() + self.skipped_frame_count();
        if total == 0 {
            0.0
        } else {
            (self.skipped_frame_count() as f64 / total as f64) * 100.0
        }
    }

    // =========================================================================
    // Lifecycle State Management
    // =========================================================================

    /// Get the current application lifecycle state
    ///
    /// Returns the current state of the application as seen by the platform.
    pub fn lifecycle_state(&self) -> AppLifecycleState {
        AppLifecycleState::from_u8(self.lifecycle_state.load(Ordering::Acquire))
    }

    /// Handle a lifecycle state change from the platform
    ///
    /// This should be called by the platform integration when the app
    /// lifecycle state changes. It will:
    /// 1. Update the internal state
    /// 2. Notify all registered listeners
    /// 3. Adjust frame scheduling behavior accordingly
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::{Scheduler, AppLifecycleState};
    ///
    /// let scheduler = Scheduler::new();
    ///
    /// // Platform notifies app is going to background
    /// scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    ///
    /// // Check if we should render
    /// if !scheduler.lifecycle_state().should_render() {
    ///     // Skip frame rendering
    /// }
    /// ```
    pub fn handle_app_lifecycle_state_change(&self, new_state: AppLifecycleState) {
        // Atomically swap state and get old value
        let old_state = AppLifecycleState::from_u8(
            self.lifecycle_state.swap(new_state as u8, Ordering::AcqRel),
        );

        // Only notify if state actually changed
        if old_state != new_state {
            // Notify listeners (clone to avoid holding lock during callbacks)
            let listeners = {
                let listeners = self.lifecycle_listeners.lock();
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
    /// use flui_scheduler::{Scheduler, AppLifecycleState};
    /// use std::sync::Arc;
    ///
    /// let scheduler = Scheduler::new();
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
        let id = self.callback_id_gen.next();
        self.lifecycle_listeners
            .lock()
            .push(LifecycleListener { id, callback });
        id
    }

    /// Remove a lifecycle state change listener by ID
    ///
    /// Returns `true` if the listener was found and removed.
    pub fn remove_lifecycle_state_listener(&self, id: CallbackId) -> bool {
        let mut listeners = self.lifecycle_listeners.lock();
        let original_len = listeners.len();
        listeners.retain(|l| l.id != id);
        listeners.len() < original_len
    }

    /// Check if frames should be scheduled based on lifecycle state
    ///
    /// Returns `false` when the app is hidden, paused, or detached.
    pub fn should_schedule_frame(&self) -> bool {
        self.lifecycle_state().should_render()
    }

    /// Check if animations should run based on lifecycle state
    ///
    /// Returns `true` only when the app is resumed (visible and focused).
    pub fn should_run_animations(&self) -> bool {
        self.lifecycle_state().should_animate()
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
    /// ```rust,ignore
    /// async fn wait_for_frame(scheduler: &Scheduler) {
    ///     // Wait for the current/next frame to complete
    ///     let timing = scheduler.end_of_frame().await;
    ///
    ///     println!("Frame {} completed in {}ms",
    ///         timing.id.as_u64(),
    ///         timing.elapsed().value());
    /// }
    /// ```
    ///
    /// # Flutter Equivalent
    ///
    /// This is similar to Flutter's `SchedulerBinding.endOfFrame` Future.
    pub fn end_of_frame(&self) -> FrameCompletionFuture {
        let (future, state) = FrameCompletionFuture::new();
        self.frame_completion_waiters
            .lock()
            .push(FrameCompletionNotifier { state });
        future
    }

    /// Internal: Notify all frame completion waiters
    fn notify_frame_completion(&self, timing: &FrameTiming) {
        let waiters = {
            let mut waiters = self.frame_completion_waiters.lock();
            std::mem::take(&mut *waiters)
        };

        for notifier in waiters {
            let mut state = notifier.state.lock();
            state.completed = Some(*timing);
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

// Implement BindingBase trait for singleton pattern
impl BindingBase for Scheduler {
    fn init_instances(&mut self) {
        tracing::debug!("Scheduler initialized");
    }
}

// Implement singleton pattern via macro
impl_binding_singleton!(Scheduler);

impl TickerProvider for Scheduler {
    fn schedule_tick(&self, callback: Box<dyn FnOnce(f64) + Send>) {
        // Schedule as transient callback for next frame
        self.schedule_frame_callback(Box::new(move |_vsync_time| {
            // Pass 0.0 as elapsed time since the callback was just scheduled.
            // Individual Tickers (like ScheduledTicker) track their own start times
            // and compute elapsed time internally. This matches Flutter's behavior
            // where TickerProvider just schedules the tick, not computes elapsed.
            callback(0.0);
        }));
    }
}

/// Builder for creating a scheduler with custom configuration
#[derive(Debug, Clone)]
pub struct SchedulerBuilder {
    frame_duration: FrameDuration,
    task_queue_capacity: Option<usize>,
    vsync_refresh_rate: Option<u32>,
}

impl SchedulerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            frame_duration: FrameDuration::FPS_60,
            task_queue_capacity: None,
            vsync_refresh_rate: None,
        }
    }

    /// Set target FPS
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.frame_duration = FrameDuration::from_fps(fps);
        self
    }

    /// Set frame duration
    pub fn frame_duration(mut self, duration: FrameDuration) -> Self {
        self.frame_duration = duration;
        self
    }

    /// Set task queue capacity
    pub fn task_queue_capacity(mut self, capacity: usize) -> Self {
        self.task_queue_capacity = Some(capacity);
        self
    }

    /// Set VSync refresh rate (creates integrated VsyncScheduler)
    pub fn vsync_refresh_rate(mut self, refresh_rate: u32) -> Self {
        self.vsync_refresh_rate = Some(refresh_rate);
        self
    }

    /// Build the scheduler
    pub fn build(self) -> Scheduler {
        let mut scheduler = Scheduler::with_frame_duration(self.frame_duration);

        if let Some(capacity) = self.task_queue_capacity {
            scheduler.task_queue = TaskQueue::with_capacity(capacity);
        }

        if let Some(refresh_rate) = self.vsync_refresh_rate {
            scheduler.set_vsync(VsyncScheduler::new(refresh_rate));
        }

        scheduler
    }
}

impl Default for SchedulerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{AnimationPriority, BuildPriority, IdlePriority, UserInputPriority};

    #[test]
    fn test_scheduler_phase_lifecycle() {
        let scheduler = Scheduler::new();
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);

        let vsync = Instant::now();
        scheduler.handle_begin_frame(vsync);

        // After begin_frame, we should be in MidFrameMicrotasks
        // (TransientCallbacks already executed)
        assert_eq!(scheduler.phase(), SchedulerPhase::MidFrameMicrotasks);

        scheduler.handle_draw_frame();
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    }

    #[test]
    fn test_transient_callback_receives_vsync() {
        let scheduler = Scheduler::new();
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

    #[test]
    fn test_microtask_execution() {
        let scheduler = Scheduler::new();
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
        let scheduler = Scheduler::new();
        assert!(!scheduler.is_frame_scheduled());

        scheduler.request_frame();
        assert!(scheduler.is_frame_scheduled());

        let frame_id = scheduler.execute_frame();
        assert!(frame_id.as_u64() > 0);
        assert!(!scheduler.is_frame_scheduled());
    }

    #[test]
    fn test_task_execution_priority() {
        let scheduler = Scheduler::new();
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
    fn test_typed_task_execution() {
        let scheduler = Scheduler::new();
        let counter = Arc::new(Mutex::new(Vec::new()));

        let c1 = Arc::clone(&counter);
        scheduler.add_task_typed::<IdlePriority>(move || c1.lock().push(4));

        let c2 = Arc::clone(&counter);
        scheduler.add_task_typed::<UserInputPriority>(move || c2.lock().push(1));

        let c3 = Arc::clone(&counter);
        scheduler.add_task_typed::<BuildPriority>(move || c3.lock().push(3));

        let c4 = Arc::clone(&counter);
        scheduler.add_task_typed::<AnimationPriority>(move || c4.lock().push(2));

        scheduler.execute_frame();

        assert_eq!(*counter.lock(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_post_frame_callback() {
        let scheduler = Scheduler::new();
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
        let scheduler = Scheduler::new();
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
        let scheduler = Scheduler::new();

        assert_eq!(scheduler.frame_count(), 0);

        scheduler.execute_frame();
        assert_eq!(scheduler.frame_count(), 1);

        scheduler.execute_frame();
        assert_eq!(scheduler.frame_count(), 2);
    }

    #[test]
    fn test_scheduler_builder() {
        let scheduler = SchedulerBuilder::new()
            .target_fps(120)
            .task_queue_capacity(100)
            .build();

        assert!((scheduler.target_fps() as i32 - 120).abs() <= 1);
    }

    #[test]
    fn test_warm_up_frame() {
        let scheduler = Scheduler::new();

        assert_eq!(scheduler.frame_count(), 0);

        scheduler.schedule_warm_up_frame();
        assert_eq!(scheduler.frame_count(), 1);

        // Second call should be no-op
        scheduler.schedule_warm_up_frame();
        assert_eq!(scheduler.frame_count(), 1);
    }

    #[test]
    fn test_frame_duration_setting() {
        let scheduler = Scheduler::new();

        scheduler.set_frame_duration(FrameDuration::FPS_144);
        assert!((scheduler.target_fps() as i32 - 144).abs() <= 1);

        scheduler.set_target_fps(30);
        assert!((scheduler.target_fps() as i32 - 30).abs() <= 1);
    }

    // Frame Skip Policy Tests

    #[test]
    fn test_frame_skip_policy_never() {
        // Never policy should never skip frames
        assert_eq!(FrameSkipPolicy::Never.frames_to_skip(100.0, 16.67, 3), 0);
        assert_eq!(FrameSkipPolicy::Never.frames_to_skip(1000.0, 16.67, 3), 0);
    }

    #[test]
    fn test_frame_skip_policy_catch_up() {
        // Under budget - no skip
        assert_eq!(FrameSkipPolicy::CatchUp.frames_to_skip(15.0, 16.67, 3), 0);

        // Slightly over budget - no skip (only 1 frame behind)
        assert_eq!(FrameSkipPolicy::CatchUp.frames_to_skip(20.0, 16.67, 3), 0);

        // 2 frames behind - skip 1
        assert_eq!(FrameSkipPolicy::CatchUp.frames_to_skip(40.0, 16.67, 3), 1);

        // 3 frames behind - skip 2
        assert_eq!(FrameSkipPolicy::CatchUp.frames_to_skip(60.0, 16.67, 3), 2);
    }

    #[test]
    fn test_frame_skip_policy_skip_to_latest() {
        // 50ms elapsed / 16.67ms per frame = 2.99 = floor to 2 frames behind
        // Skip 2-1 = 1 frame (keep latest)
        assert_eq!(
            FrameSkipPolicy::SkipToLatest.frames_to_skip(50.0, 16.67, 3),
            1
        );

        // 100ms = 5.99 = floor to 5 frames behind, skip 4
        assert_eq!(
            FrameSkipPolicy::SkipToLatest.frames_to_skip(100.0, 16.67, 3),
            4
        );
    }

    #[test]
    fn test_frame_skip_policy_limited_skip() {
        // 50ms = 2.99 = floor to 2 frames behind, skip 1
        assert_eq!(
            FrameSkipPolicy::LimitedSkip.frames_to_skip(50.0, 16.67, 3),
            1
        );

        // 200ms = 11.99 = floor to 11 frames behind, skip 10, but max 3
        assert_eq!(
            FrameSkipPolicy::LimitedSkip.frames_to_skip(200.0, 16.67, 3),
            3
        );
    }

    #[test]
    fn test_scheduler_frame_skip_policy() {
        let scheduler = Scheduler::new();

        assert_eq!(scheduler.frame_skip_policy(), FrameSkipPolicy::CatchUp);

        scheduler.set_frame_skip_policy(FrameSkipPolicy::Never);
        assert_eq!(scheduler.frame_skip_policy(), FrameSkipPolicy::Never);

        scheduler.set_max_frame_skip(5);
        assert_eq!(scheduler.max_frame_skip(), 5);
    }

    #[test]
    fn test_skipped_frame_counter() {
        let scheduler = Scheduler::new();

        assert_eq!(scheduler.skipped_frame_count(), 0);

        // Execute a frame to set last_frame_end
        scheduler.execute_frame();

        // Should not skip immediately after a frame
        assert!(!scheduler.check_and_skip_frame());
    }

    // Callback Cancellation Tests

    #[test]
    fn test_cancel_transient_callback() {
        let scheduler = Scheduler::new();
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
    fn test_cancel_persistent_callback() {
        let scheduler = Scheduler::new();
        let count = Arc::new(Mutex::new(0));

        let c = Arc::clone(&count);
        let id = scheduler.add_persistent_frame_callback(Arc::new(move |_| {
            *c.lock() += 1;
        }));

        // Execute once - should fire
        scheduler.execute_frame();
        assert_eq!(*count.lock(), 1);

        // Remove the callback
        assert!(scheduler.remove_persistent_frame_callback(id));

        // Execute again - should NOT fire
        scheduler.execute_frame();
        assert_eq!(*count.lock(), 1);
    }

    #[test]
    fn test_cancel_post_frame_callback() {
        let scheduler = Scheduler::new();
        let called = Arc::new(Mutex::new(false));

        let c = Arc::clone(&called);
        let id = scheduler.add_post_frame_callback(Box::new(move |_| {
            *c.lock() = true;
        }));

        // Cancel before frame executes
        assert!(scheduler.cancel_post_frame_callback(id));

        scheduler.execute_frame();

        // Callback should NOT have been called
        assert!(!*called.lock());
    }

    #[test]
    fn test_callback_id_uniqueness() {
        let scheduler = Scheduler::new();

        let id1 = scheduler.schedule_frame_callback(Box::new(|_| {}));
        let id2 = scheduler.schedule_frame_callback(Box::new(|_| {}));
        let id3 = scheduler.add_persistent_frame_callback(Arc::new(|_| {}));
        let id4 = scheduler.add_post_frame_callback(Box::new(|_| {}));

        // All IDs should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id3, id4);
    }

    #[test]
    fn test_cancel_nonexistent_callback() {
        let scheduler = Scheduler::new();

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
        let scheduler = Scheduler::new();

        // Default state should be Resumed
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Resumed);
        assert!(scheduler.should_schedule_frame());
        assert!(scheduler.should_run_animations());
    }

    #[test]
    fn test_lifecycle_state_change() {
        let scheduler = Scheduler::new();

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Hidden);
        assert!(!scheduler.should_schedule_frame());
        assert!(!scheduler.should_run_animations());

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Resumed);
        assert!(scheduler.should_schedule_frame());
    }

    #[test]
    fn test_lifecycle_listener() {
        let scheduler = Scheduler::new();
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
        let scheduler = Scheduler::new();
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
        let scheduler = Scheduler::new();

        // Should be able to create multiple futures
        let _future1 = scheduler.end_of_frame();
        let _future2 = scheduler.end_of_frame();

        // Both futures should be registered
        assert_eq!(scheduler.frame_completion_waiters.lock().len(), 2);
    }

    #[test]
    fn test_end_of_frame_completes_after_frame() {
        use std::task::{RawWaker, RawWakerVTable, Waker};

        let scheduler = Scheduler::new();

        // Create a future
        let mut future = scheduler.end_of_frame();

        // Create a no-op waker for polling
        fn noop_waker() -> Waker {
            fn clone(_: *const ()) -> RawWaker {
                RawWaker::new(std::ptr::null(), &VTABLE)
            }
            fn wake(_: *const ()) {}
            fn wake_by_ref(_: *const ()) {}
            fn drop(_: *const ()) {}
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
            unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
        }

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

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
            assert!(timing.id.as_u64() > 0);
        }
    }

    #[test]
    fn test_multiple_waiters_notified() {
        use std::task::{RawWaker, RawWakerVTable, Waker};

        let scheduler = Scheduler::new();

        fn noop_waker() -> Waker {
            fn clone(_: *const ()) -> RawWaker {
                RawWaker::new(std::ptr::null(), &VTABLE)
            }
            fn wake(_: *const ()) {}
            fn wake_by_ref(_: *const ()) {}
            fn drop(_: *const ()) {}
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
            unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
        }

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

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
}
