//! Scheduler binding - glue between scheduling and the Flutter-like binding system.
//!
//! This module provides `SchedulerBinding`, a mixin-like trait that provides
//! frame scheduling, task execution, and animation coordination following
//! Flutter's `SchedulerBinding` pattern.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `SchedulerBinding` mixin:
//!
//! ```dart
//! mixin SchedulerBinding on BindingBase {
//!   void scheduleFrame();
//!   void scheduleForcedFrame();
//!   void ensureVisualUpdate();
//!   int scheduleFrameCallback(FrameCallback callback);
//!   void cancelFrameCallbackWithId(int id);
//!   void addPersistentFrameCallback(FrameCallback callback);
//!   void addPostFrameCallback(FrameCallback callback);
//!   Future<void> get endOfFrame;
//!   void handleBeginFrame(Duration? rawTimeStamp);
//!   void handleDrawFrame();
//!   // ... etc
//! }
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_scheduler::binding::SchedulerBinding;
//! use flui_scheduler::Scheduler;
//!
//! // Get the scheduler instance
//! let scheduler = Scheduler::instance();
//!
//! // Schedule a frame callback
//! let id = scheduler.schedule_frame_callback_with_id(Box::new(|timestamp| {
//!     println!("Frame callback at {:?}", timestamp);
//! }));
//!
//! // Cancel if needed
//! scheduler.cancel_frame_callback_with_id(id);
//!
//! // Force a visual update
//! scheduler.ensure_visual_update();
//! ```

use crate::frame::{
    AppLifecycleState, FrameTiming, PersistentFrameCallback, PostFrameCallback, SchedulerPhase,
    TransientFrameCallback,
};
use crate::id::{CallbackIdMarker, TypedId};
use crate::scheduler::{FrameCompletionFuture, Scheduler};
use crate::task::Priority;
use flui_foundation::{BindingBase, HasInstance};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use web_time::{Duration, Instant};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Callback ID type for frame callbacks (matches Flutter's int id)
pub type FrameCallbackId = i32;

/// Frame callback signature matching Flutter's `FrameCallback`
///
/// The `timeStamp` is the time since the scheduler's epoch, not wall-clock time.
/// This ensures all animations are synchronized to a common time base.
pub type FrameCallback = Box<dyn FnOnce(Duration) + Send>;

/// Task callback signature matching Flutter's `TaskCallback<T>`
pub type TaskCallback<T> = Box<dyn FnOnce() -> T + Send>;

/// Timings callback for receiving FrameTiming reports from the engine
///
/// Callbacks receive batched FrameTiming data approximately once per second
/// in release mode, or every ~100ms in debug/profile builds.
pub type TimingsCallback = Arc<dyn Fn(&[FrameTiming]) + Send + Sync>;

/// Scheduling strategy callback
///
/// Called to determine whether a task at a given priority should run.
/// Returns `true` if the task should execute, `false` to defer.
pub type SchedulingStrategy = Box<dyn Fn(i32, &Scheduler) -> bool + Send + Sync>;

/// Default scheduling strategy - runs tasks when not over budget
pub fn default_scheduling_strategy(priority: i32, scheduler: &Scheduler) -> bool {
    // Always run high priority tasks (Animation = 100000, UserInput = 1000000)
    if priority >= 100000 {
        return true;
    }

    // Run lower priority tasks only if we have budget remaining
    !scheduler.is_over_budget()
}

// ============================================================================
// Time Dilation
// ============================================================================

/// Global time dilation factor for animations
///
/// This slows down animations by the given factor to help with development.
/// A value of 1.0 means normal speed, 2.0 means half speed, etc.
///
/// # Thread Safety
///
/// This uses atomic operations and is safe to access from any thread.
static TIME_DILATION: AtomicU64 = AtomicU64::new(0x3FF0000000000000); // 1.0 as f64 bits

/// Get the current time dilation factor
///
/// # Example
///
/// ```rust
/// use flui_scheduler::binding::time_dilation;
///
/// let dilation = time_dilation();
/// assert_eq!(dilation, 1.0); // Default is normal speed
/// ```
#[inline]
pub fn time_dilation() -> f64 {
    f64::from_bits(TIME_DILATION.load(Ordering::Relaxed))
}

/// Set the time dilation factor
///
/// # Panics
///
/// Panics if `value` is not positive (must be > 0.0).
///
/// # Example
///
/// ```rust
/// use flui_scheduler::binding::{set_time_dilation, time_dilation};
///
/// // Slow down animations to 50% speed
/// set_time_dilation(2.0);
/// assert_eq!(time_dilation(), 2.0);
///
/// // Reset to normal
/// set_time_dilation(1.0);
/// ```
pub fn set_time_dilation(value: f64) {
    assert!(value > 0.0, "timeDilation must be positive");

    let old_bits = TIME_DILATION.load(Ordering::Relaxed);
    let old_value = f64::from_bits(old_bits);

    if (old_value - value).abs() < f64::EPSILON {
        return;
    }

    // If scheduler is initialized, reset epoch first
    if <Scheduler as BindingBase>::is_initialized() {
        Scheduler::instance().reset_epoch();
    }

    TIME_DILATION.store(value.to_bits(), Ordering::Relaxed);
}

// ============================================================================
// Performance Mode
// ============================================================================

/// Performance mode for the Dart VM / Rust runtime
///
/// This hints to the runtime about expected workload patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PerformanceMode {
    /// Normal operation - no special optimizations
    #[default]
    Normal,

    /// Latency-optimized mode for interactive scenarios
    ///
    /// Hints that low latency is more important than throughput.
    /// The runtime may disable some background optimizations.
    Latency,

    /// Throughput-optimized mode for batch processing
    ///
    /// Hints that throughput is more important than latency.
    /// The runtime may batch operations more aggressively.
    Throughput,

    /// Battery-saving mode for background operation
    ///
    /// Hints that power consumption should be minimized.
    /// The runtime may reduce polling frequency and defer work.
    LowPower,
}

// ============================================================================
// Service Extensions
// ============================================================================

/// Service extension identifiers for the scheduler.
///
/// These constants are used when registering service extensions for debugging
/// and development tools. In Flutter, service extensions allow external tools
/// to interact with the running application.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::binding::SchedulerServiceExtensions;
///
/// let ext = SchedulerServiceExtensions::TimeDilation;
/// assert_eq!(ext.name(), "timeDilation");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum SchedulerServiceExtensions {
    /// Service extension for controlling time dilation.
    ///
    /// When called, changes the value of [`time_dilation`], which determines
    /// the factor by which to slow down animations for development help.
    TimeDilation,
}

impl SchedulerServiceExtensions {
    /// Get the string name of this service extension.
    ///
    /// This matches the naming convention used in Flutter's service extensions.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TimeDilation => "timeDilation",
        }
    }

    /// Get all available service extensions.
    #[inline]
    pub const fn all() -> &'static [Self] {
        &[Self::TimeDilation]
    }
}

impl std::fmt::Display for SchedulerServiceExtensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Handle for a performance mode request
///
/// When dropped, the performance mode request is released.
/// Multiple handles can be active; the highest-priority mode wins.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::binding::{PerformanceModeRequestHandle, PerformanceMode};
/// use flui_scheduler::{Scheduler, SchedulerBinding};
///
/// let scheduler = Scheduler::new();
///
/// // Request latency mode for an interactive operation
/// let handle = scheduler.request_performance_mode(PerformanceMode::Latency);
///
/// // Do latency-sensitive work...
///
/// // Mode is released when handle is dropped
/// drop(handle);
/// ```
pub struct PerformanceModeRequestHandle {
    cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl PerformanceModeRequestHandle {
    /// Create a new handle with a cleanup callback
    pub(crate) fn new(cleanup: impl FnOnce() + Send + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    /// Dispose of this handle, releasing the performance mode request
    ///
    /// This is called automatically when the handle is dropped.
    pub fn dispose(mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for PerformanceModeRequestHandle {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

// ============================================================================
// SchedulerBinding Trait
// ============================================================================

/// Scheduler binding trait - provides frame scheduling and task execution
///
/// This trait corresponds to Flutter's `SchedulerBinding` mixin and provides:
/// - Frame callback scheduling (transient, persistent, post-frame)
/// - Task scheduling with priorities
/// - VSync coordination
/// - Time dilation for debugging
/// - Performance mode requests
/// - Frame timing statistics
///
/// # Implementation
///
/// The `Scheduler` struct implements this trait directly. Other bindings
/// can delegate to it or compose it.
pub trait SchedulerBinding: Send + Sync {
    // ========================================================================
    // Core Properties
    // ========================================================================

    /// Get the current scheduler phase
    fn scheduler_phase(&self) -> SchedulerPhase;

    /// Whether a frame has been scheduled
    fn has_scheduled_frame(&self) -> bool;

    /// Whether frames are enabled (app is visible)
    fn frames_enabled(&self) -> bool;

    /// Set whether frames are enabled
    fn set_frames_enabled(&mut self, enabled: bool);

    /// Get the current lifecycle state
    fn lifecycle_state(&self) -> AppLifecycleState;

    // ========================================================================
    // Epoch and Time
    // ========================================================================

    /// Get the current frame timestamp (time since epoch)
    ///
    /// This is the adjusted timestamp that accounts for time dilation.
    fn current_frame_time_stamp(&self) -> Duration;

    /// Get the raw system timestamp for the current frame
    ///
    /// This is the unadjusted timestamp from the system.
    fn current_system_frame_time_stamp(&self) -> Instant;

    /// Reset the epoch to now
    ///
    /// This resets the time origin used for frame timestamps.
    /// Called automatically when time dilation changes.
    fn reset_epoch(&self);

    // ========================================================================
    // Frame Scheduling
    // ========================================================================

    /// Schedule a frame to be rendered
    ///
    /// This schedules a frame if one is not already scheduled.
    /// The frame will be rendered on the next vsync.
    fn schedule_frame(&self);

    /// Schedule a frame unconditionally
    ///
    /// This schedules a frame even if the app is not visible or
    /// frames are disabled. Use sparingly.
    fn schedule_forced_frame(&self);

    /// Ensure a visual update will happen
    ///
    /// This is a convenience method that schedules a frame if needed
    /// and ensures persistent callbacks will be invoked.
    fn ensure_visual_update(&self);

    /// Schedule a warm-up frame
    ///
    /// Executes a frame immediately without waiting for vsync.
    /// Used to reduce first-frame jank.
    fn schedule_warm_up_frame(&self);

    // ========================================================================
    // Transient Frame Callbacks
    // ========================================================================

    /// Schedule a transient frame callback
    ///
    /// The callback is called once on the next frame with the frame timestamp.
    /// Returns an ID that can be used to cancel the callback.
    ///
    /// # Arguments
    ///
    /// * `callback` - The callback to invoke
    /// * `rescheduling` - Whether this is a reschedule from within a callback
    fn schedule_frame_callback_fn(
        &self,
        callback: FrameCallback,
        rescheduling: bool,
    ) -> FrameCallbackId;

    /// Schedule a transient frame callback (convenience method)
    fn schedule_frame_callback(&self, callback: FrameCallback) -> FrameCallbackId {
        self.schedule_frame_callback_fn(callback, false)
    }

    /// Cancel a transient frame callback by ID
    fn cancel_frame_callback_with_id(&self, id: FrameCallbackId);

    /// Get the number of pending transient callbacks
    fn transient_callback_count(&self) -> usize;

    // ========================================================================
    // Persistent Frame Callbacks
    // ========================================================================

    /// Add a persistent frame callback
    ///
    /// Persistent callbacks are called every frame during the
    /// `PersistentCallbacks` phase. They are used for the rendering pipeline.
    fn add_persistent_frame_callback(&self, callback: PersistentFrameCallback);

    // ========================================================================
    // Post-Frame Callbacks
    // ========================================================================

    /// Add a post-frame callback
    ///
    /// Post-frame callbacks are called once after the current frame completes.
    /// They are used for cleanup and scheduling work for the next frame.
    ///
    /// # Arguments
    ///
    /// * `callback` - The callback to invoke
    /// * `debug_label` - Optional label for debugging
    fn add_post_frame_callback_with_label(
        &self,
        callback: PostFrameCallback,
        debug_label: Option<&str>,
    );

    /// Add a post-frame callback (convenience method)
    fn add_post_frame_callback(&self, callback: PostFrameCallback) {
        self.add_post_frame_callback_with_label(callback, None);
    }

    // ========================================================================
    // End of Frame Future
    // ========================================================================

    /// Get a future that completes when the current frame ends
    ///
    /// If no frame is in progress, the future completes on the next frame.
    fn end_of_frame(&self) -> FrameCompletionFuture;

    // ========================================================================
    // Task Scheduling
    // ========================================================================

    /// Schedule a task with the given priority
    ///
    /// Tasks are executed during the frame when budget allows.
    fn schedule_task<T: Send + 'static>(
        &self,
        callback: TaskCallback<T>,
        priority: Priority,
        debug_label: Option<&str>,
    );

    // ========================================================================
    // Frame Handling
    // ========================================================================

    /// Handle the begin frame signal from the platform
    ///
    /// This processes transient callbacks with the given timestamp.
    fn handle_begin_frame(&self, raw_time_stamp: Option<Duration>);

    /// Handle the draw frame signal from the platform
    ///
    /// This processes persistent and post-frame callbacks.
    fn handle_draw_frame(&self);

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Handle a lifecycle state change from the platform
    fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState);

    // ========================================================================
    // Timings
    // ========================================================================

    /// Add a callback to receive frame timing data
    fn add_timings_callback(&self, callback: TimingsCallback);

    /// Remove a timings callback
    fn remove_timings_callback(&self, callback: &TimingsCallback);

    // ========================================================================
    // Performance Mode
    // ========================================================================

    /// Request a specific performance mode
    ///
    /// Returns a handle that releases the request when dropped.
    fn request_performance_mode(&self, mode: PerformanceMode) -> PerformanceModeRequestHandle;

    // ========================================================================
    // Debugging
    // ========================================================================

    /// Assert that there are no pending transient callbacks
    ///
    /// Used in tests to verify cleanup.
    fn debug_assert_no_transient_callbacks(&self, reason: &str) -> bool;

    /// Assert that there are no pending performance mode requests
    fn debug_assert_no_pending_performance_mode_requests(&self, reason: &str) -> bool;

    /// Assert that time dilation is 1.0
    fn debug_assert_no_time_dilation(&self, reason: &str) -> bool;
}

// ============================================================================
// SchedulerBinding Implementation for Scheduler
// ============================================================================

/// Internal state for SchedulerBinding implementation
///
/// This is now stored per-Scheduler instance instead of globally,
/// ensuring proper test isolation and multiple scheduler support.
pub(crate) struct SchedulerBindingState {
    /// Whether frames are enabled
    frames_enabled: bool,

    /// Epoch start time
    epoch_start: Duration,

    /// Last raw timestamp
    last_raw_time_stamp: Duration,

    /// Current frame raw timestamp
    current_raw_time_stamp: Option<Duration>,

    /// Timings callbacks
    timings_callbacks: Vec<TimingsCallback>,

    /// Pending frame timings (batched for callbacks)
    pending_timings: Vec<FrameTiming>,

    /// Last timings report time
    last_timings_report: Instant,

    /// Performance mode request count
    performance_mode_requests: u32,

    /// Current performance mode
    current_performance_mode: PerformanceMode,

    /// Frame callback ID counter
    next_frame_callback_id: i32,

    /// Mapping from legacy ID to internal CallbackId
    frame_callback_ids: std::collections::HashMap<FrameCallbackId, TypedId<CallbackIdMarker>>,
}

impl Default for SchedulerBindingState {
    fn default() -> Self {
        Self {
            frames_enabled: true,
            epoch_start: Duration::ZERO,
            last_raw_time_stamp: Duration::ZERO,
            current_raw_time_stamp: None,
            timings_callbacks: Vec::new(),
            pending_timings: Vec::new(),
            last_timings_report: Instant::now(),
            performance_mode_requests: 0,
            current_performance_mode: PerformanceMode::Normal,
            next_frame_callback_id: 0,
            frame_callback_ids: std::collections::HashMap::new(),
        }
    }
}

impl SchedulerBindingState {
    /// Create a new binding state instance
    pub fn new() -> Self {
        Self::default()
    }
}

impl SchedulerBinding for Scheduler {
    fn scheduler_phase(&self) -> SchedulerPhase {
        self.phase()
    }

    fn has_scheduled_frame(&self) -> bool {
        self.is_frame_scheduled()
    }

    fn frames_enabled(&self) -> bool {
        self.binding_state.lock().frames_enabled
    }

    fn set_frames_enabled(&mut self, enabled: bool) {
        let mut state = self.binding_state.lock();
        if state.frames_enabled == enabled {
            return;
        }
        state.frames_enabled = enabled;
        drop(state);

        if enabled {
            self.request_frame();
        }
    }

    fn lifecycle_state(&self) -> AppLifecycleState {
        Scheduler::lifecycle_state(self)
    }

    fn current_frame_time_stamp(&self) -> Duration {
        let state = self.binding_state.lock();
        if let Some(raw) = state.current_raw_time_stamp {
            drop(state);
            self.adjust_for_epoch(raw)
        } else {
            Duration::ZERO
        }
    }

    fn current_system_frame_time_stamp(&self) -> Instant {
        self.current_vsync_time().unwrap_or_else(Instant::now)
    }

    fn reset_epoch(&self) {
        let mut state = self.binding_state.lock();
        state.epoch_start = state.last_raw_time_stamp;
    }

    fn schedule_frame(&self) {
        if !self.binding_state.lock().frames_enabled {
            return;
        }

        if !self.should_schedule_frame() {
            return;
        }

        self.request_frame();
    }

    fn schedule_forced_frame(&self) {
        // Force scheduling even if frames are disabled
        self.request_frame();
    }

    fn ensure_visual_update(&self) {
        match self.phase() {
            SchedulerPhase::Idle | SchedulerPhase::PostFrameCallbacks => {
                self.request_frame();
            }
            _ => {
                // Already in a frame, nothing to do
            }
        }
    }

    fn schedule_warm_up_frame(&self) {
        Scheduler::schedule_warm_up_frame(self);
    }

    fn schedule_frame_callback_fn(
        &self,
        callback: FrameCallback,
        _rescheduling: bool,
    ) -> FrameCallbackId {
        let mut state = self.binding_state.lock();
        let legacy_id = state.next_frame_callback_id;
        state.next_frame_callback_id = state.next_frame_callback_id.wrapping_add(1);

        // Convert to internal callback format
        let epoch_start = state.epoch_start;
        drop(state);

        let internal_callback: TransientFrameCallback = Box::new(move |vsync_time: Instant| {
            // Convert Instant to Duration from epoch
            let raw_duration = vsync_time.elapsed();
            let adjusted = adjust_duration_for_epoch(raw_duration, epoch_start);
            callback(adjusted);
        });

        let internal_id = self.schedule_frame_callback(internal_callback);

        // Store mapping
        self.binding_state
            .lock()
            .frame_callback_ids
            .insert(legacy_id, internal_id);

        legacy_id
    }

    fn cancel_frame_callback_with_id(&self, id: FrameCallbackId) {
        let mut state = self.binding_state.lock();
        if let Some(internal_id) = state.frame_callback_ids.remove(&id) {
            drop(state);
            self.cancel_frame_callback(internal_id);
        }
    }

    fn transient_callback_count(&self) -> usize {
        Scheduler::transient_callback_count(self)
    }

    fn add_persistent_frame_callback(&self, callback: PersistentFrameCallback) {
        Scheduler::add_persistent_frame_callback(self, callback);
    }

    fn add_post_frame_callback_with_label(
        &self,
        callback: PostFrameCallback,
        _debug_label: Option<&str>,
    ) {
        Scheduler::add_post_frame_callback(self, callback);
    }

    fn end_of_frame(&self) -> FrameCompletionFuture {
        Scheduler::end_of_frame(self)
    }

    fn schedule_task<T: Send + 'static>(
        &self,
        callback: TaskCallback<T>,
        priority: Priority,
        _debug_label: Option<&str>,
    ) {
        self.add_task(priority, move || {
            let _ = callback();
        });
    }

    fn handle_begin_frame(&self, raw_time_stamp: Option<Duration>) {
        let raw = raw_time_stamp.unwrap_or_else(|| {
            // Use current time if not provided
            Duration::from_secs_f64(Instant::now().elapsed().as_secs_f64())
        });

        {
            let mut state = self.binding_state.lock();
            state.current_raw_time_stamp = Some(raw);
            state.last_raw_time_stamp = raw;
        }

        // Call internal handler with Instant
        Scheduler::handle_begin_frame(self, Instant::now());
    }

    fn handle_draw_frame(&self) {
        Scheduler::handle_draw_frame(self);

        // Record timing for callbacks
        if let Some(timing) = self.current_frame() {
            let mut state = self.binding_state.lock();
            state.pending_timings.push(timing);

            // Report timings periodically (every ~100ms in debug, ~1s in release)
            #[cfg(debug_assertions)]
            let report_interval = Duration::from_millis(100);
            #[cfg(not(debug_assertions))]
            let report_interval = Duration::from_secs(1);

            if state.last_timings_report.elapsed() >= report_interval {
                let timings = std::mem::take(&mut state.pending_timings);
                let callbacks = state.timings_callbacks.clone();
                state.last_timings_report = Instant::now();
                drop(state);

                // Invoke callbacks outside lock
                for callback in callbacks {
                    callback(&timings);
                }
            }
        }

        // Clear current raw timestamp
        self.binding_state.lock().current_raw_time_stamp = None;
    }

    fn handle_app_lifecycle_state_changed(&self, state: AppLifecycleState) {
        self.handle_app_lifecycle_state_change(state);
    }

    fn add_timings_callback(&self, callback: TimingsCallback) {
        self.binding_state.lock().timings_callbacks.push(callback);
    }

    fn remove_timings_callback(&self, callback: &TimingsCallback) {
        let mut state = self.binding_state.lock();
        state
            .timings_callbacks
            .retain(|c| !Arc::ptr_eq(c, callback));
    }

    fn request_performance_mode(&self, mode: PerformanceMode) -> PerformanceModeRequestHandle {
        let binding_state = Arc::clone(&self.binding_state);

        {
            let mut state = binding_state.lock();
            state.performance_mode_requests += 1;

            // Use highest priority mode
            if mode as u8 > state.current_performance_mode as u8 {
                state.current_performance_mode = mode;
            }
        }

        PerformanceModeRequestHandle::new(move || {
            let mut state = binding_state.lock();
            state.performance_mode_requests = state.performance_mode_requests.saturating_sub(1);

            // Reset to normal if no more requests
            if state.performance_mode_requests == 0 {
                state.current_performance_mode = PerformanceMode::Normal;
            }
        })
    }

    fn debug_assert_no_transient_callbacks(&self, reason: &str) -> bool {
        let count = self.transient_callback_count();
        if count > 0 {
            tracing::error!(
                "Found {} transient callbacks when expecting none: {}",
                count,
                reason
            );
            return false;
        }
        true
    }

    fn debug_assert_no_pending_performance_mode_requests(&self, reason: &str) -> bool {
        let requests = self.binding_state.lock().performance_mode_requests;
        if requests > 0 {
            tracing::error!(
                "Found {} performance mode requests when expecting none: {}",
                requests,
                reason
            );
            return false;
        }
        true
    }

    fn debug_assert_no_time_dilation(&self, reason: &str) -> bool {
        let dilation = time_dilation();
        if (dilation - 1.0).abs() > f64::EPSILON {
            tracing::error!(
                "Time dilation is {} when expecting 1.0: {}",
                dilation,
                reason
            );
            return false;
        }
        true
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Adjust a duration for the epoch and time dilation
fn adjust_duration_for_epoch(raw: Duration, epoch_start: Duration) -> Duration {
    let since_epoch = raw.saturating_sub(epoch_start);
    let dilation = time_dilation();

    if (dilation - 1.0).abs() < f64::EPSILON {
        since_epoch
    } else {
        Duration::from_secs_f64(since_epoch.as_secs_f64() / dilation)
    }
}

impl Scheduler {
    /// Adjust a raw timestamp for the epoch
    pub fn adjust_for_epoch(&self, raw: Duration) -> Duration {
        let epoch_start = self.binding_state.lock().epoch_start;
        adjust_duration_for_epoch(raw, epoch_start)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_dilation() {
        // Reset to default
        set_time_dilation(1.0);
        assert!((time_dilation() - 1.0).abs() < f64::EPSILON);

        // Set to 2x (half speed)
        set_time_dilation(2.0);
        assert!((time_dilation() - 2.0).abs() < f64::EPSILON);

        // Reset
        set_time_dilation(1.0);
    }

    #[test]
    #[should_panic(expected = "timeDilation must be positive")]
    fn test_time_dilation_zero() {
        set_time_dilation(0.0);
    }

    #[test]
    #[should_panic(expected = "timeDilation must be positive")]
    fn test_time_dilation_negative() {
        set_time_dilation(-1.0);
    }

    #[test]
    fn test_performance_mode_handle() {
        let scheduler = Scheduler::new();

        // Request latency mode
        let handle = scheduler.request_performance_mode(PerformanceMode::Latency);

        // Check request is active (using instance state, not global)
        assert_eq!(scheduler.binding_state.lock().performance_mode_requests, 1);

        // Drop handle
        drop(handle);

        // Check request is released
        assert_eq!(scheduler.binding_state.lock().performance_mode_requests, 0);
    }

    #[test]
    fn test_binding_state_isolation() {
        // Test that each scheduler has its own binding state
        let scheduler1 = Scheduler::new();
        let scheduler2 = Scheduler::new();

        // Request performance mode on scheduler1
        let _handle = scheduler1.request_performance_mode(PerformanceMode::Latency);

        // Verify scheduler1 has the request
        assert_eq!(scheduler1.binding_state.lock().performance_mode_requests, 1);

        // Verify scheduler2 is unaffected (proper isolation)
        assert_eq!(scheduler2.binding_state.lock().performance_mode_requests, 0);
    }

    #[test]
    fn test_default_scheduling_strategy() {
        let scheduler = Scheduler::new();

        // High priority should always run (Animation = 2, UserInput = 3)
        // Our strategy uses 100000 as threshold for "high priority"
        assert!(default_scheduling_strategy(100000, &scheduler));
        assert!(default_scheduling_strategy(1000000, &scheduler));

        // Lower priority depends on budget
        // (without active frame, we're not over budget)
        assert!(default_scheduling_strategy(0, &scheduler));
    }

    #[test]
    fn test_adjust_for_epoch() {
        let raw = Duration::from_secs(10);
        let epoch = Duration::from_secs(5);

        // Without dilation
        set_time_dilation(1.0);
        let adjusted = adjust_duration_for_epoch(raw, epoch);
        assert_eq!(adjusted, Duration::from_secs(5));

        // With 2x dilation (half speed)
        set_time_dilation(2.0);
        let adjusted = adjust_duration_for_epoch(raw, epoch);
        assert!((adjusted.as_secs_f64() - 2.5).abs() < 0.001);

        // Reset
        set_time_dilation(1.0);
    }
}
