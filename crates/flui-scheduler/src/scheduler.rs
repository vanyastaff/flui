//! Main scheduler - coordinates frame lifecycle and task execution
//!
//! The Scheduler is the central orchestrator for FLUI's rendering pipeline.
//! It manages:
//! - Frame scheduling (vsync coordination)
//! - Task queue execution
//! - Animation tickers
//! - Frame budgets
//!
//! ## Type-Safe Task Addition
//!
//! ```rust
//! use flui_scheduler::{Scheduler, Priority};
//! use flui_scheduler::traits::AnimationPriority;
//!
//! let scheduler = Scheduler::new();
//!
//! // Runtime priority
//! scheduler.add_task(Priority::Animation, || {
//!     println!("Animation task!");
//! });
//!
//! // Compile-time priority (type-safe)
//! scheduler.add_task_typed::<AnimationPriority>(|| {
//!     println!("Also animation!");
//! });
//! ```

use crate::budget::FrameBudget;
use crate::duration::{FrameDuration, Milliseconds};
use crate::frame::{
    FrameCallback, FrameId, FramePhase, FrameTiming, PersistentFrameCallback, PostFrameCallback,
};
use crate::task::{Priority, TaskQueue};
use crate::ticker::TickerProvider;
use crate::traits::PriorityLevel;
use parking_lot::Mutex;
use std::sync::Arc;
use web_time::Instant;

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

    /// Frame duration configuration
    frame_duration: Arc<Mutex<FrameDuration>>,

    /// Frame budget management
    budget: Arc<Mutex<FrameBudget>>,

    /// Whether a frame is currently scheduled
    frame_scheduled: Arc<Mutex<bool>>,

    /// Frame counter
    frame_count: Arc<Mutex<u64>>,
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
            current_frame: Arc::new(Mutex::new(None)),
            task_queue: TaskQueue::new(),
            frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            persistent_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            post_frame_callbacks: Arc::new(Mutex::new(Vec::new())),
            frame_duration: Arc::new(Mutex::new(frame_duration)),
            budget: Arc::new(Mutex::new(FrameBudget::new(target_fps))),
            frame_scheduled: Arc::new(Mutex::new(false)),
            frame_count: Arc::new(Mutex::new(0)),
        }
    }

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

    /// Get task queue reference
    pub fn task_queue(&self) -> &TaskQueue {
        &self.task_queue
    }

    /// Add a task with priority
    pub fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.task_queue.add(priority, callback);
    }

    /// Add a task with compile-time priority checking
    pub fn add_task_typed<P: PriorityLevel>(&self, callback: impl FnOnce() + Send + 'static) {
        self.task_queue.add_typed::<P>(callback);
    }

    /// Schedule a frame callback
    ///
    /// The callback will be executed at the start of the next frame only.
    pub fn schedule_frame(&self, callback: FrameCallback) {
        self.frame_callbacks.lock().push(callback);
        *self.frame_scheduled.lock() = true;
    }

    /// Schedule a frame (without callback)
    pub fn request_frame(&self) {
        *self.frame_scheduled.lock() = true;
    }

    /// Add a persistent frame callback
    ///
    /// The callback will be executed at the start of every frame.
    /// This is useful for rebuilds, animations, and other per-frame work.
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
        let frame_duration = *self.frame_duration.lock();
        let mut timing = FrameTiming::with_duration(frame_duration);
        timing.phase = FramePhase::Build;

        let frame_id = timing.id;
        *self.current_frame.lock() = Some(timing);
        *self.frame_scheduled.lock() = false;

        // Increment frame counter
        *self.frame_count.lock() += 1;

        // Execute persistent frame callbacks (every frame)
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
            self.budget.lock().record_frame_duration(timing.elapsed());

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
        if self.remaining_budget().value() > 5.0 {
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

    /// Get total frame count
    pub fn frame_count(&self) -> u64 {
        *self.frame_count.lock()
    }

    /// Get average FPS from budget statistics
    pub fn avg_fps(&self) -> f64 {
        self.budget.lock().avg_fps()
    }

    /// Check if last frame was janky
    pub fn is_janky(&self) -> bool {
        self.budget.lock().is_janky()
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
        self.scheduler().request_frame();
    }

    /// Add a task
    fn add_task(&self, priority: Priority, callback: impl FnOnce() + Send + 'static) {
        self.scheduler().add_task(priority, callback);
    }

    /// Add a typed task
    fn add_task_typed<P: PriorityLevel>(&self, callback: impl FnOnce() + Send + 'static) {
        self.scheduler().add_task_typed::<P>(callback);
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

/// Builder for creating a scheduler with custom configuration
#[derive(Debug, Clone)]
pub struct SchedulerBuilder {
    frame_duration: FrameDuration,
    task_queue_capacity: Option<usize>,
}

impl SchedulerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            frame_duration: FrameDuration::FPS_60,
            task_queue_capacity: None,
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

    /// Build the scheduler
    pub fn build(self) -> Scheduler {
        let mut scheduler = Scheduler::with_frame_duration(self.frame_duration);
        if let Some(capacity) = self.task_queue_capacity {
            scheduler.task_queue = TaskQueue::with_capacity(capacity);
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
    fn test_scheduler_frame_lifecycle() {
        let scheduler = Scheduler::new();
        assert!(!scheduler.is_frame_scheduled());

        // Schedule a frame
        scheduler.request_frame();
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

        // Allow for rounding due to float conversions
        assert!((scheduler.target_fps() as i32 - 120).abs() <= 1);
    }

    #[test]
    fn test_frame_duration_setting() {
        let scheduler = Scheduler::new();

        scheduler.set_frame_duration(FrameDuration::FPS_144);
        // Allow for rounding due to float conversions
        assert!((scheduler.target_fps() as i32 - 144).abs() <= 1);

        scheduler.set_target_fps(30);
        assert!((scheduler.target_fps() as i32 - 30).abs() <= 1);
    }
}
