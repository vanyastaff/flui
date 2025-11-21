//! Task queue with priority-based scheduling
//!
//! Manages execution order of tasks within a frame based on priority.
//!
//! ## Priority Levels
//!
//! 1. **UserInput** (highest) - Mouse, keyboard, touch events
//! 2. **Animation** - Animation tickers, interpolations
//! 3. **Build** - Widget tree rebuilds
//! 4. **Idle** (lowest) - Background work, GC, telemetry

use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

/// Task priority levels (higher value = higher priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    /// Background/idle work (GC, telemetry)
    Idle = 0,
    /// Normal UI updates (widget rebuilds)
    Build = 1,
    /// Animations and transitions
    Animation = 2,
    /// User input events (must be immediate)
    UserInput = 3,
}

impl Priority {
    /// Get priority as numeric value for comparison
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Build => write!(f, "Build"),
            Self::Animation => write!(f, "Animation"),
            Self::UserInput => write!(f, "UserInput"),
        }
    }
}

/// Unique task identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, AtomicOrdering::Relaxed))
    }

    /// Get raw task ID
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// A scheduled task with priority
pub struct Task {
    id: TaskId,
    priority: Priority,
    callback: Box<dyn FnOnce() + Send>,
}

impl Task {
    /// Create a new task
    pub fn new<F>(priority: Priority, callback: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self {
            id: TaskId::new(),
            priority,
            callback: Box::new(callback),
        }
    }

    /// Get task ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get task priority
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Execute the task
    pub fn execute(self) {
        (self.callback)();
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}

/// Wrapper for priority queue ordering (higher priority first)
struct PriorityTask(Task);

impl PartialEq for PriorityTask {
    fn eq(&self, other: &Self) -> bool {
        self.0.priority == other.0.priority && self.0.id == other.0.id
    }
}

impl Eq for PriorityTask {}

impl PartialOrd for PriorityTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then by task ID (FIFO within priority)
        match self.0.priority.cmp(&other.0.priority) {
            Ordering::Equal => other.0.id.0.cmp(&self.0.id.0), // Earlier ID first
            ord => ord,
        }
    }
}

/// Priority-based task queue
///
/// Tasks are executed in priority order:
/// UserInput > Animation > Build > Idle
pub struct TaskQueue {
    queue: Arc<Mutex<BinaryHeap<PriorityTask>>>,
}

impl TaskQueue {
    /// Create a new task queue
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    /// Add a task to the queue
    pub fn add_task(&self, task: Task) {
        self.queue.lock().push(PriorityTask(task));
    }

    /// Add a task with priority
    pub fn add<F>(&self, priority: Priority, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.add_task(Task::new(priority, callback));
    }

    /// Get the next task (highest priority)
    pub fn pop(&self) -> Option<Task> {
        self.queue.lock().pop().map(|pt| pt.0)
    }

    /// Get number of pending tasks
    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    /// Execute all tasks up to a certain priority
    ///
    /// Returns number of tasks executed
    pub fn execute_until(&self, min_priority: Priority) -> usize {
        let mut executed = 0;

        loop {
            let task = {
                let mut queue = self.queue.lock();
                if let Some(pt) = queue.peek() {
                    if pt.0.priority >= min_priority {
                        queue.pop().map(|pt| pt.0)
                    } else {
                        break; // Lower priority task, stop
                    }
                } else {
                    break; // Empty queue
                }
            };

            if let Some(task) = task {
                task.execute();
                executed += 1;
            }
        }

        executed
    }

    /// Execute all pending tasks
    ///
    /// Returns number of tasks executed
    pub fn execute_all(&self) -> usize {
        let mut executed = 0;

        while let Some(task) = self.pop() {
            task.execute();
            executed += 1;
        }

        executed
    }

    /// Clear all pending tasks
    pub fn clear(&self) {
        self.queue.lock().clear();
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for TaskQueue {
    fn clone(&self) -> Self {
        Self {
            queue: Arc::clone(&self.queue),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::UserInput > Priority::Animation);
        assert!(Priority::Animation > Priority::Build);
        assert!(Priority::Build > Priority::Idle);
    }

    #[test]
    fn test_task_queue_priority() {
        let queue = TaskQueue::new();

        // Add tasks in random priority order
        queue.add(Priority::Idle, || {});
        queue.add(Priority::UserInput, || {});
        queue.add(Priority::Build, || {});
        queue.add(Priority::Animation, || {});

        // Should execute in priority order
        assert_eq!(queue.pop().unwrap().priority(), Priority::UserInput);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Animation);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Build);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Idle);
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_execute_until() {
        let queue = TaskQueue::new();
        let counter = Arc::new(Mutex::new(0));

        // Add various priority tasks
        for _ in 0..3 {
            let c = Arc::clone(&counter);
            queue.add(Priority::UserInput, move || *c.lock() += 1);
        }
        for _ in 0..2 {
            let c = Arc::clone(&counter);
            queue.add(Priority::Build, move || *c.lock() += 1);
        }

        // Execute only UserInput priority
        let executed = queue.execute_until(Priority::UserInput);
        assert_eq!(executed, 3);
        assert_eq!(*counter.lock(), 3);
        assert_eq!(queue.len(), 2); // Build tasks remain
    }
}
