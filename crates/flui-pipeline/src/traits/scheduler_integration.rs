//! Scheduler integration trait
//!
//! Provides a bridge between the pipeline and flui-scheduler.
//! This allows the pipeline to request frames, add tasks, and
//! access frame timing without directly depending on scheduler internals.
//!
//! # Design Philosophy
//!
//! This trait follows **Dependency Inversion Principle (DIP)**:
//! - High-level pipeline modules depend on this abstraction
//! - Low-level scheduler details are hidden behind the trait
//! - Types from `flui-scheduler` are re-exported, not duplicated

use std::time::Duration;

// Re-export scheduler types - single source of truth
pub use flui_scheduler::{FrameTiming, Priority};

/// Scheduler integration trait
///
/// Provides pipeline access to scheduler functionality without
/// tight coupling to the concrete scheduler implementation.
///
/// # Implementors
///
/// - `flui_app::SchedulerBinding` - Production implementation
/// - [`NoopScheduler`] - Testing (no-op)
/// - [`RecordingScheduler`] - Testing (records operations)
///
/// # Example
///
/// ```rust,ignore
/// use flui_pipeline::traits::{SchedulerIntegration, Priority};
///
/// struct MyPipeline<S: SchedulerIntegration> {
///     scheduler: S,
/// }
///
/// impl<S: SchedulerIntegration> MyPipeline<S> {
///     fn request_rebuild(&self) {
///         self.scheduler.request_frame();
///     }
///
///     fn schedule_animation(&self, callback: impl FnOnce() + Send + 'static) {
///         self.scheduler.add_task(Priority::Animation, Box::new(callback));
///     }
/// }
/// ```
pub trait SchedulerIntegration: Send + Sync {
    // =========================================================================
    // Frame Scheduling
    // =========================================================================

    /// Request a new frame to be scheduled
    ///
    /// Called when the pipeline has dirty elements that need processing.
    /// The scheduler will call back to execute the frame at the next vsync.
    fn request_frame(&self);

    /// Check if a frame is currently scheduled
    fn is_frame_scheduled(&self) -> bool;

    /// Cancel any scheduled frame
    fn cancel_frame(&self);

    // =========================================================================
    // Task Management
    // =========================================================================

    /// Add a task with priority
    ///
    /// Tasks are executed during frame processing based on priority
    /// and available time budget.
    fn add_task(&self, priority: Priority, task: Box<dyn FnOnce() + Send>);

    /// Add a task that runs every frame
    ///
    /// Persistent tasks are useful for:
    /// - Flushing rebuild queues
    /// - Animation updates
    /// - Debug overlays
    fn add_persistent_task(&self, task: Box<dyn Fn() + Send + Sync>);

    // =========================================================================
    // Timing
    // =========================================================================

    /// Get target frames per second
    fn target_fps(&self) -> u32;

    /// Set target frames per second
    fn set_target_fps(&mut self, fps: u32);

    /// Get frame budget duration
    fn frame_budget(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.target_fps() as f64)
    }

    /// Get current frame timing (if in a frame)
    fn current_frame_timing(&self) -> Option<&FrameTiming>;

    /// Check if currently over budget
    fn is_over_budget(&self) -> bool {
        self.current_frame_timing()
            .map(|t| t.is_over_budget())
            .unwrap_or(false)
    }

    /// Get remaining budget in current frame (milliseconds)
    fn remaining_budget_ms(&self) -> f64 {
        self.current_frame_timing()
            .map(|t| t.remaining_budget_ms())
            .unwrap_or(0.0)
    }
}

// =============================================================================
// Test Implementations
// =============================================================================

/// A no-op scheduler for testing
///
/// All operations are no-ops except timing which returns defaults.
#[derive(Debug, Default)]
pub struct NoopScheduler {
    target_fps: u32,
}

impl NoopScheduler {
    /// Create a new no-op scheduler
    pub fn new() -> Self {
        Self { target_fps: 60 }
    }
}

impl SchedulerIntegration for NoopScheduler {
    fn request_frame(&self) {}

    fn is_frame_scheduled(&self) -> bool {
        false
    }

    fn cancel_frame(&self) {}

    fn add_task(&self, _priority: Priority, _task: Box<dyn FnOnce() + Send>) {}

    fn add_persistent_task(&self, _task: Box<dyn Fn() + Send + Sync>) {}

    fn target_fps(&self) -> u32 {
        self.target_fps
    }

    fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps;
    }

    fn current_frame_timing(&self) -> Option<&FrameTiming> {
        None
    }
}

/// A recording scheduler for testing
///
/// Records all operations for verification in tests.
#[derive(Debug)]
pub struct RecordingScheduler {
    target_fps: u32,
    frame_requested: std::sync::atomic::AtomicBool,
    task_count: std::sync::atomic::AtomicUsize,
}

impl Default for RecordingScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingScheduler {
    /// Create a new recording scheduler
    pub fn new() -> Self {
        Self {
            target_fps: 60,
            frame_requested: std::sync::atomic::AtomicBool::new(false),
            task_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Check if frame was requested
    pub fn was_frame_requested(&self) -> bool {
        self.frame_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get number of tasks added
    pub fn tasks_added(&self) -> usize {
        self.task_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Reset all recordings
    pub fn reset(&self) {
        self.frame_requested
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.task_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

impl SchedulerIntegration for RecordingScheduler {
    fn request_frame(&self) {
        self.frame_requested
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_frame_scheduled(&self) -> bool {
        self.frame_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn cancel_frame(&self) {
        self.frame_requested
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    fn add_task(&self, _priority: Priority, _task: Box<dyn FnOnce() + Send>) {
        self.task_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn add_persistent_task(&self, _task: Box<dyn Fn() + Send + Sync>) {
        self.task_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn target_fps(&self) -> u32 {
        self.target_fps
    }

    fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps;
    }

    fn current_frame_timing(&self) -> Option<&FrameTiming> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        // Priority from flui-scheduler: higher value = higher priority
        assert!(Priority::UserInput > Priority::Animation);
        assert!(Priority::Animation > Priority::Build);
        assert!(Priority::Build > Priority::Idle);
    }

    #[test]
    fn test_noop_scheduler() {
        let scheduler = NoopScheduler::new();

        assert_eq!(scheduler.target_fps(), 60);
        assert!(!scheduler.is_frame_scheduled());

        scheduler.request_frame();
        assert!(!scheduler.is_frame_scheduled()); // No-op
    }

    #[test]
    fn test_recording_scheduler() {
        let scheduler = RecordingScheduler::new();

        assert!(!scheduler.was_frame_requested());
        assert_eq!(scheduler.tasks_added(), 0);

        scheduler.request_frame();
        assert!(scheduler.was_frame_requested());

        scheduler.add_task(Priority::Build, Box::new(|| {}));
        assert_eq!(scheduler.tasks_added(), 1);

        scheduler.reset();
        assert!(!scheduler.was_frame_requested());
        assert_eq!(scheduler.tasks_added(), 0);
    }
}
