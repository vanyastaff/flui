//! Scheduler binding - wraps flui-scheduler for framework integration
//!
//! This is a thin wrapper around `flui_scheduler::Scheduler` that implements
//! the `BindingBase` trait for consistency with other bindings.
//!
//! ## Features
//!
//! The scheduler binding provides access to:
//! - **Frame scheduling**: Schedule callbacks for animation and rendering
//! - **Callback cancellation**: Cancel scheduled callbacks by ID
//! - **Lifecycle state**: Track application lifecycle for resource management
//! - **Frame completion futures**: Await frame completion asynchronously
//! - **Task queue**: Priority-based task execution
//! - **VSync coordination**: Smooth frame pacing

use super::BindingBase;
use flui_scheduler::{
    AppLifecycleState, CallbackId, FrameCompletionFuture, FrameId, FrameTiming,
    PersistentFrameCallback, PostFrameCallback, Priority, Scheduler, TransientFrameCallback,
};
use std::sync::Arc;

/// Scheduler binding wrapper
///
/// # Architecture
///
/// ```text
/// SchedulerBinding (wrapper) → flui_scheduler::Scheduler (production implementation)
/// ```
///
/// This provides a consistent API with other bindings while delegating all
/// scheduling logic to the production flui-scheduler crate.
///
/// # Features
///
/// - **Callback Cancellation**: All callback registration methods return a `CallbackId`
///   that can be used to cancel the callback before it fires.
/// - **Lifecycle State**: Track and respond to application lifecycle changes
///   (resumed, inactive, hidden, paused, detached).
/// - **Frame Completion Futures**: Use `end_of_frame()` to get a future that
///   resolves when the current/next frame completes.
///
/// # Thread-Safety
///
/// The underlying Scheduler is fully thread-safe and can be accessed from any thread.
///
/// # Example
///
/// ```rust
/// use flui_app::binding::SchedulerBinding;
/// use flui_scheduler::AppLifecycleState;
/// use std::sync::Arc;
///
/// let mut binding = SchedulerBinding::new();
///
/// // Schedule a frame callback (returns ID for cancellation)
/// let callback_id = binding.schedule_frame_callback(Box::new(|vsync_time| {
///     println!("Frame at {:?}", vsync_time);
/// }));
///
/// // Can cancel if needed
/// binding.cancel_frame_callback(callback_id);
///
/// // Listen for lifecycle changes
/// binding.add_lifecycle_listener(Arc::new(|state| {
///     if !state.should_render() {
///         println!("App going to background, pausing rendering");
///     }
/// }));
/// ```
pub struct SchedulerBinding {
    scheduler: Scheduler,
}

impl SchedulerBinding {
    /// Create a new SchedulerBinding with 60 FPS target
    pub fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
        }
    }

    /// Create a SchedulerBinding with custom target FPS
    pub fn with_target_fps(target_fps: u32) -> Self {
        Self {
            scheduler: Scheduler::with_target_fps(target_fps),
        }
    }

    /// Get reference to the underlying Scheduler
    ///
    /// This provides full access to the production scheduler's features:
    /// - Frame scheduling and callbacks
    /// - Task queue with priority levels
    /// - Frame budget management
    /// - VSync coordination
    /// - Lifecycle state management
    /// - Frame completion futures
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Consume and get the underlying Scheduler
    pub fn into_scheduler(self) -> Scheduler {
        self.scheduler
    }

    // =========================================================================
    // Frame Scheduling
    // =========================================================================

    /// Schedule a transient frame callback (animation)
    ///
    /// The callback receives the vsync timestamp and fires during the
    /// TransientCallbacks phase. Returns a `CallbackId` for cancellation.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_app::binding::SchedulerBinding;
    /// let binding = SchedulerBinding::new();
    /// let id = binding.schedule_frame_callback(Box::new(|vsync_time| {
    ///     // Animation tick
    /// }));
    ///
    /// // Cancel if needed
    /// binding.cancel_frame_callback(id);
    /// ```
    #[inline]
    pub fn schedule_frame_callback(&self, callback: TransientFrameCallback) -> CallbackId {
        self.scheduler.schedule_frame_callback(callback)
    }

    /// Cancel a transient frame callback by ID
    ///
    /// Returns `true` if the callback was found and cancelled.
    #[inline]
    pub fn cancel_frame_callback(&self, id: CallbackId) -> bool {
        self.scheduler.cancel_frame_callback(id)
    }

    /// Add a persistent frame callback (rendering pipeline)
    ///
    /// Fires every frame during PersistentCallbacks phase.
    /// Returns a `CallbackId` for removal.
    #[inline]
    pub fn add_persistent_frame_callback(&self, callback: PersistentFrameCallback) -> CallbackId {
        self.scheduler.add_persistent_frame_callback(callback)
    }

    /// Remove a persistent frame callback by ID
    #[inline]
    pub fn remove_persistent_frame_callback(&self, id: CallbackId) -> bool {
        self.scheduler.remove_persistent_frame_callback(id)
    }

    /// Add a post-frame callback (cleanup)
    ///
    /// Fires once after the current/next frame completes.
    /// Returns a `CallbackId` for cancellation.
    #[inline]
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) -> CallbackId {
        self.scheduler.add_post_frame_callback(callback)
    }

    /// Cancel a post-frame callback by ID
    #[inline]
    pub fn cancel_post_frame_callback(&self, id: CallbackId) -> bool {
        self.scheduler.cancel_post_frame_callback(id)
    }

    /// Request a frame to be scheduled
    #[inline]
    pub fn request_frame(&self) {
        self.scheduler.request_frame();
    }

    /// Execute a complete frame
    #[inline]
    pub fn execute_frame(&self) -> FrameId {
        self.scheduler.execute_frame()
    }

    // =========================================================================
    // Lifecycle State
    // =========================================================================

    /// Get the current application lifecycle state
    #[inline]
    pub fn lifecycle_state(&self) -> AppLifecycleState {
        self.scheduler.lifecycle_state()
    }

    /// Handle a lifecycle state change from the platform
    ///
    /// Call this when the platform notifies of state changes (e.g., app going
    /// to background). This will:
    /// 1. Update the internal state
    /// 2. Notify all registered listeners
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_app::binding::SchedulerBinding;
    /// # use flui_scheduler::AppLifecycleState;
    /// let binding = SchedulerBinding::new();
    ///
    /// // Platform notifies app is going to background
    /// binding.handle_lifecycle_state_change(AppLifecycleState::Hidden);
    ///
    /// // Check if we should render
    /// if !binding.should_schedule_frame() {
    ///     // Skip frame rendering
    /// }
    /// ```
    #[inline]
    pub fn handle_lifecycle_state_change(&self, state: AppLifecycleState) {
        self.scheduler.handle_app_lifecycle_state_change(state);
    }

    /// Add a lifecycle state change listener
    ///
    /// Returns a `CallbackId` for removal.
    #[inline]
    pub fn add_lifecycle_listener(
        &self,
        callback: Arc<dyn Fn(AppLifecycleState) + Send + Sync>,
    ) -> CallbackId {
        self.scheduler.add_lifecycle_state_listener(callback)
    }

    /// Remove a lifecycle state change listener
    #[inline]
    pub fn remove_lifecycle_listener(&self, id: CallbackId) -> bool {
        self.scheduler.remove_lifecycle_state_listener(id)
    }

    /// Check if frames should be scheduled based on lifecycle state
    ///
    /// Returns `false` when the app is hidden, paused, or detached.
    #[inline]
    pub fn should_schedule_frame(&self) -> bool {
        self.scheduler.should_schedule_frame()
    }

    /// Check if animations should run based on lifecycle state
    ///
    /// Returns `true` only when the app is resumed (visible and focused).
    #[inline]
    pub fn should_run_animations(&self) -> bool {
        self.scheduler.should_run_animations()
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
    /// async fn wait_for_layout(binding: &SchedulerBinding) {
    ///     let timing = binding.end_of_frame().await;
    ///     println!("Frame completed in {}ms", timing.elapsed().value());
    /// }
    /// ```
    #[inline]
    pub fn end_of_frame(&self) -> FrameCompletionFuture {
        self.scheduler.end_of_frame()
    }

    // =========================================================================
    // Task Queue
    // =========================================================================

    /// Add a task with priority
    #[inline]
    pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.scheduler.add_task(priority, callback);
    }

    // =========================================================================
    // Frame State
    // =========================================================================

    /// Check if a frame is scheduled
    #[inline]
    pub fn is_frame_scheduled(&self) -> bool {
        self.scheduler.is_frame_scheduled()
    }

    /// Get current frame timing (if a frame is active)
    #[inline]
    pub fn current_frame(&self) -> Option<FrameTiming> {
        self.scheduler.current_frame()
    }

    /// Get target FPS
    #[inline]
    pub fn target_fps(&self) -> u32 {
        self.scheduler.target_fps()
    }

    /// Set target FPS
    #[inline]
    pub fn set_target_fps(&self, fps: u32) {
        self.scheduler.set_target_fps(fps);
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total frame count
    #[inline]
    pub fn frame_count(&self) -> u64 {
        self.scheduler.frame_count()
    }

    /// Get average FPS
    #[inline]
    pub fn avg_fps(&self) -> f64 {
        self.scheduler.avg_fps()
    }

    /// Check if last frame was janky (exceeded budget)
    #[inline]
    pub fn is_janky(&self) -> bool {
        self.scheduler.is_janky()
    }

    /// Get jank rate as percentage
    #[inline]
    pub fn jank_rate(&self) -> f64 {
        self.scheduler.jank_rate()
    }
}

impl Default for SchedulerBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for SchedulerBinding {
    fn init(&mut self) {
        tracing::debug!(
            target_fps = self.scheduler.target_fps(),
            lifecycle_state = %self.scheduler.lifecycle_state(),
            "SchedulerBinding initialized with flui-scheduler"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_binding_creation() {
        let binding = SchedulerBinding::new();
        assert_eq!(binding.scheduler().target_fps(), 60);
    }

    #[test]
    fn test_custom_fps() {
        let binding = SchedulerBinding::with_target_fps(120);
        assert_eq!(binding.scheduler().target_fps(), 120);
    }

    #[test]
    fn test_scheduler_access() {
        let binding = SchedulerBinding::new();
        let scheduler = binding.scheduler();

        // Should have production scheduler features
        assert!(!scheduler.is_frame_scheduled());
        assert_eq!(scheduler.target_fps(), 60);
    }

    #[test]
    fn test_callback_cancellation() {
        let binding = SchedulerBinding::new();

        // Schedule a callback
        let id = binding.schedule_frame_callback(Box::new(|_| {}));

        // Cancel it
        assert!(binding.cancel_frame_callback(id));

        // Second cancel should return false (already cancelled)
        assert!(!binding.cancel_frame_callback(id));
    }

    #[test]
    fn test_persistent_callback() {
        let binding = SchedulerBinding::new();

        let id = binding.add_persistent_frame_callback(Arc::new(|_| {}));

        // Remove it
        assert!(binding.remove_persistent_frame_callback(id));

        // Second remove should return false
        assert!(!binding.remove_persistent_frame_callback(id));
    }

    #[test]
    fn test_lifecycle_state() {
        let binding = SchedulerBinding::new();

        // Default is Resumed
        assert_eq!(binding.lifecycle_state(), AppLifecycleState::Resumed);
        assert!(binding.should_schedule_frame());
        assert!(binding.should_run_animations());

        // Go to background
        binding.handle_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(binding.lifecycle_state(), AppLifecycleState::Hidden);
        assert!(!binding.should_schedule_frame());
        assert!(!binding.should_run_animations());

        // Resume
        binding.handle_lifecycle_state_change(AppLifecycleState::Resumed);
        assert!(binding.should_schedule_frame());
    }

    #[test]
    fn test_lifecycle_listener() {
        use parking_lot::Mutex;

        let binding = SchedulerBinding::new();
        let received = Arc::new(Mutex::new(None));

        let r = Arc::clone(&received);
        let id = binding.add_lifecycle_listener(Arc::new(move |state| {
            *r.lock() = Some(state);
        }));

        // Change state
        binding.handle_lifecycle_state_change(AppLifecycleState::Inactive);
        assert_eq!(*received.lock(), Some(AppLifecycleState::Inactive));

        // Remove listener
        assert!(binding.remove_lifecycle_listener(id));

        // Change again - should not be received
        binding.handle_lifecycle_state_change(AppLifecycleState::Hidden);
        assert_eq!(*received.lock(), Some(AppLifecycleState::Inactive));
    }

    #[test]
    fn test_end_of_frame_future() {
        let binding = SchedulerBinding::new();

        // Should be able to create a future
        let _future = binding.end_of_frame();

        // Execute a frame
        let frame_id = binding.execute_frame();
        assert!(frame_id.as_u64() > 0);
    }

    #[test]
    fn test_task_queue() {
        use parking_lot::Mutex;

        let binding = SchedulerBinding::new();
        let executed = Arc::new(Mutex::new(false));

        let e = Arc::clone(&executed);
        binding.add_task(Priority::Animation, move || {
            *e.lock() = true;
        });

        // Execute frame to run tasks
        binding.execute_frame();

        assert!(*executed.lock());
    }

    #[test]
    fn test_statistics() {
        let binding = SchedulerBinding::new();

        assert_eq!(binding.frame_count(), 0);

        binding.execute_frame();
        assert_eq!(binding.frame_count(), 1);

        binding.execute_frame();
        assert_eq!(binding.frame_count(), 2);
    }
}
