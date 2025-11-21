//! Main scheduler - coordinates frame lifecycle and task execution
//!
//! The Scheduler is the central orchestrator for FLUI's rendering pipeline.
//! It manages:
//! - Frame scheduling (vsync coordination)
//! - Task queue execution
//! - Animation tickers
//! - Frame budgets

use crate::budget::FrameBudget;
use crate::frame::{
    FrameCallback, FrameId, FramePhase, FrameTiming, PersistentFrameCallback, PostFrameCallback,
};
use crate::task::{Priority, TaskQueue};
use crate::ticker::TickerProvider;
use instant::Instant;
use parking_lot::Mutex;
use std::sync::Arc;

/// Main scheduler for frame and task management
#[derive(Clone)]
pub struct Scheduler {
    /// Current frame timing
    current_frame: Arc<Mutex<Option<FrameTiming>>>,

    /// Task queue (priority-based)
    task_queue: TaskQueue,

    /// Frame callbacks (executed at frame start, one-time)
    frame_callbacks: Arc<Mutex<Vec<FrameCallback>>>,

    /// Persistent frame callbacks (executed every frame)
    persistent_frame_callbacks: Arc<Mutex<Vec<PersistentFrameCallback>>>,

    /// Post-frame callbacks (executed after frame completes)
    post_frame_callbacks: Arc<Mutex<Vec<PostFrameCallback>>>,

    /// Target FPS (60 by default)
    target_fps: u32,

    /// Frame budget management
    budget: Arc<Mutex<FrameBudget>>,

    /// Whether a frame is currently scheduled
    frame_scheduled: Arc<Mutex<bool>>,
}

impl Scheduler {
    /// Create a new scheduler with 60 FPS target
    pub fn new() -> Self {
        Self::with_target_fps(60)
    }

    /// Create a scheduler with custom target FPS
    pub fn with_target_fps(target_fps: u32) -> Self {
        Self {
            current_frame: Arc::new(Mutex::new(None)),
            task_queue: TaskQueue::new(),
            frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            persistent_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            target_fps,
            budget: Arc::new(Mutex::new(FrameBudget::new(target_fps))),
            frame_scheduled: Arc::new(Mutex::new(false)),
        }
    }

    /// Set target FPS
    pub fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps;
        *self.budget.lock() = FrameBudget::new(fps);
    }

    /// Get target FPS
    pub fn target_fps(&self) -> u32 {
        self.target_fps
    }

    /// Get task queue reference
    pub fn task_queue(&self) -> &TaskQueue {
        &self.task_queue
    }

    /// Add a task with priority
    pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.task_queue.add(priority, callback);
    }

    /// Schedule a frame callback
    ///
    /// The callback will be executed at the start of the next frame only.
    pub fn schedule_frame(&self, callback: FrameCallback) {
        self.frame_callbacks.lock().push(callback);
        *self.frame_scheduled.lock() = true;
    }

    /// Add a persistent frame callback
    ///
    /// The callback will be executed at the start of every frame.
    /// This is useful for rebuilds, animations, and other per-frame work.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// scheduler.add_persistent_frame_callback(Box::new(|timing| {
    ///     // Process rebuild queue every frame
    ///     pipeline.flush_rebuild_queue();
    /// }));
    /// ```
    pub fn add_persistent_frame_callback(&self, callback: PersistentFrameCallback) {
        self.persistent_frame_callbacks.lock().push(callback);
    }

    /// Add a post-frame callback
    ///
    /// The callback will be executed after the frame completes.
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) {
        self.post_frame_callbacks.lock().push(callback);
    }

    /// Check if a frame is scheduled
    pub fn is_frame_scheduled(&self) -> bool {
        *self.frame_scheduled.lock()
    }

    /// Begin a new frame
    ///
    /// This should be called by the event loop when a frame starts (e.g., at vsync).
    pub fn begin_frame(&self) -> FrameId {
        let mut timing = FrameTiming::new(self.target_fps);
        timing.phase = FramePhase::Build;

        let frame_id = timing.id;
        *self.current_frame.lock() = Some(timing);
        *self.frame_scheduled.lock() = false;

        // Execute persistent frame callbacks (every frame)
        // Clone the callbacks so we don't hold the lock during execution
        let persistent_callbacks = {
            let cbs = self.persistent_frame_callbacks.lock();
            cbs.clone()
        };

        for callback in persistent_callbacks.iter() {
            if let Some(timing) = self.current_frame.lock().as_ref() {
                callback(timing);
            }
        }

        // Execute one-time frame callbacks
        let callbacks = {
            let mut cbs = self.frame_callbacks.lock();
            std::mem::take(&mut *cbs)
        };

        for callback in callbacks {
            if let Some(timing) = self.current_frame.lock().as_ref() {
                callback(timing);
            }
        }

        // Reset budget
        self.budget.lock().reset();

        frame_id
    }

    /// End the current frame
    ///
    /// This should be called after all rendering work is complete.
    pub fn end_frame(&self) {
        let timing = self.current_frame.lock().take();

        if let Some(mut timing) = timing {
            timing.phase = FramePhase::Idle;

            // Record final timing
            self.budget.lock().record_frame_time(timing.elapsed_ms());

            // Execute post-frame callbacks
            let callbacks = {
                let mut cbs = self.post_frame_callbacks.lock();
                std::mem::take(&mut *cbs)
            };

            for callback in callbacks {
                callback(&timing);
            }
        }
    }

    /// Execute the current frame
    ///
    /// This is a convenience method that:
    /// 1. Begins the frame
    /// 2. Executes all high-priority tasks
    /// 3. Ends the frame
    ///
    /// Returns the frame ID.
    pub fn execute_frame(&self) -> FrameId {
        let frame_id = self.begin_frame();

        // Execute all UserInput and Animation tasks immediately
        self.task_queue.execute_until(Priority::Animation);

        // Execute Build tasks if budget allows
        if !self.is_over_budget() {
            self.task_queue.execute_until(Priority::Build);
        }

        // Execute Idle tasks if significant budget remains
        if self.remaining_budget_ms() > 5.0 {
            // Leave 5ms buffer for compositing
            self.task_queue.execute_until(Priority::Idle);
        }

        self.end_frame();
        frame_id
    }

    /// Set the current frame phase
    pub fn set_phase(&self, phase: FramePhase) {
        if let Some(timing) = self.current_frame.lock().as_mut() {
            timing.phase = phase;
        }
    }

    /// Get current frame timing (if a frame is active)
    pub fn current_frame(&self) -> Option<FrameTiming> {
        *self.current_frame.lock()
    }

    /// Check if currently over budget
    pub fn is_over_budget(&self) -> bool {
        self.current_frame
            .lock()
            .as_ref()
            .is_some_and(|t| t.is_over_budget())
    }

    /// Get remaining budget in milliseconds
    pub fn remaining_budget_ms(&self) -> f64 {
        self.current_frame
            .lock()
            .as_ref()
            .map_or(0.0, |t| t.remaining_budget_ms())
    }

    /// Get frame budget reference
    pub fn budget(&self) -> Arc<Mutex<FrameBudget>> {
        Arc::clone(&self.budget)
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Binding trait for scheduler access
///
/// This allows different parts of the framework to access the scheduler
/// without tight coupling.
pub trait SchedulerBinding: Send + Sync {
    /// Get reference to the scheduler
    fn scheduler(&self) -> &Scheduler;

    /// Schedule a frame
    fn schedule_frame(&self) {
        self.scheduler().schedule_frame(Box::new(|_| {}));
    }

    /// Add a task
    fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.scheduler().add_task(priority, callback);
    }
}

impl TickerProvider for Scheduler {
    fn schedule_tick(&self, callback: Box<dyn FnOnce(f64) + Send>) {
        // Schedule as animation priority
        self.add_task(Priority::Animation, move || {
            let elapsed = Instant::now().elapsed().as_secs_f64();
            callback(elapsed);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_frame_lifecycle() {
        let scheduler = Scheduler::new();
        assert!(!scheduler.is_frame_scheduled());

        // Schedule a frame
        scheduler.schedule_frame(Box::new(|_| {}));
        assert!(scheduler.is_frame_scheduled());

        // Execute frame
        let frame_id = scheduler.execute_frame();
        assert!(frame_id.as_u64() > 0);
        assert!(!scheduler.is_frame_scheduled());
    }

    #[test]
    fn test_task_execution_priority() {
        let scheduler = Scheduler::new();
        let counter = Arc::new(Mutex::new(Vec::new()));

        // Add tasks in various priorities
        let c1 = Arc::clone(&counter);
        scheduler.add_task(Priority::Idle, move || c1.lock().push(4));

        let c2 = Arc::clone(&counter);
        scheduler.add_task(Priority::UserInput, move || c2.lock().push(1));

        let c3 = Arc::clone(&counter);
        scheduler.add_task(Priority::Build, move || c3.lock().push(3));

        let c4 = Arc::clone(&counter);
        scheduler.add_task(Priority::Animation, move || c4.lock().push(2));

        // Execute frame
        scheduler.execute_frame();

        // Should execute in priority order
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
}
