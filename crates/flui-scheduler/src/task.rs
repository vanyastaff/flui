//! Task queue with priority-based scheduling
//!
//! Manages execution order of tasks within a frame based on priority.
//!
//! ## Key Types
//!
//! - [`Priority`] - Task priority enum (UserInput, Animation, Build, Idle)
//! - [`Task`] - A scheduled task with priority and callback
//! - [`TypedTask`] - Compile-time priority checking via phantom types
//! - [`TaskQueue`] - Priority-based task queue
//!
//! ## Priority Levels
//!
//! 1. **UserInput** (highest) - Mouse, keyboard, touch events
//! 2. **Animation** - Animation tickers, interpolations
//! 3. **Build** - Widget tree rebuilds
//! 4. **Idle** (lowest) - Background work, GC, telemetry
//!
//! ## Type-Safe Task Creation
//!
//! ```rust
//! use flui_scheduler::task::{TypedTask, Priority};
//! use flui_scheduler::traits::UserInputPriority;
//!
//! // Type-safe task creation with compile-time priority
//! let task = TypedTask::<UserInputPriority>::new(|| {
//!     println!("High priority task!");
//! });
//!
//! assert_eq!(task.priority(), Priority::UserInput);
//! ```

use crate::id::{TaskIdMarker, TypedId};
use crate::traits::PriorityLevel;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Task priority levels (higher value = higher priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum Priority {
    /// Background/idle work (GC, telemetry)
    Idle = 0,
    /// Normal UI updates (widget rebuilds)
    #[default]
    Build = 1,
    /// Animations and transitions
    Animation = 2,
    /// User input events (must be immediate)
    UserInput = 3,
}

impl Priority {
    /// All priority levels in order from lowest to highest
    pub const ALL: [Priority; 4] = [
        Priority::Idle,
        Priority::Build,
        Priority::Animation,
        Priority::UserInput,
    ];

    /// Get priority as numeric value for comparison
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Create from u8 value
    ///
    /// Returns `None` if the value is out of range.
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Idle),
            1 => Some(Self::Build),
            2 => Some(Self::Animation),
            3 => Some(Self::UserInput),
            _ => None,
        }
    }

    /// Check if this is the highest priority
    #[inline]
    pub const fn is_highest(self) -> bool {
        matches!(self, Self::UserInput)
    }

    /// Check if this is the lowest priority
    #[inline]
    pub const fn is_lowest(self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Get the next higher priority, if any
    #[inline]
    pub const fn higher(self) -> Option<Self> {
        match self {
            Self::Idle => Some(Self::Build),
            Self::Build => Some(Self::Animation),
            Self::Animation => Some(Self::UserInput),
            Self::UserInput => None,
        }
    }

    /// Get the next lower priority, if any
    #[inline]
    pub const fn lower(self) -> Option<Self> {
        match self {
            Self::UserInput => Some(Self::Animation),
            Self::Animation => Some(Self::Build),
            Self::Build => Some(Self::Idle),
            Self::Idle => None,
        }
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

/// Unique task identifier using type-safe ID
pub type TaskId = TypedId<TaskIdMarker>;

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
    #[inline]
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get task priority
    #[inline]
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

/// A task with compile-time priority checking
///
/// Uses the typestate pattern to encode priority in the type system.
/// This allows compile-time verification of task priorities.
///
/// ## Example
///
/// ```rust
/// use flui_scheduler::task::TypedTask;
/// use flui_scheduler::traits::{UserInputPriority, IdlePriority};
///
/// fn process_input_task(task: TypedTask<UserInputPriority>) {
///     task.execute();
/// }
///
/// // This compiles
/// let input_task = TypedTask::<UserInputPriority>::new(|| {});
/// process_input_task(input_task);
///
/// // This would NOT compile:
/// // let idle_task = TypedTask::<IdlePriority>::new(|| {});
/// // process_input_task(idle_task); // Type error!
/// ```
pub struct TypedTask<P: PriorityLevel> {
    id: TaskId,
    callback: Box<dyn FnOnce() + Send>,
    _priority: PhantomData<P>,
}

impl<P: PriorityLevel> TypedTask<P> {
    /// Create a new typed task
    pub fn new<F>(callback: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self {
            id: TaskId::new(),
            callback: Box::new(callback),
            _priority: PhantomData,
        }
    }

    /// Get task ID
    #[inline]
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get task priority (from type)
    #[inline]
    pub fn priority(&self) -> Priority {
        P::VALUE
    }

    /// Get priority name
    #[inline]
    pub fn priority_name(&self) -> &'static str {
        P::NAME
    }

    /// Execute the task
    pub fn execute(self) {
        (self.callback)();
    }

    /// Convert to untyped Task
    pub fn into_task(self) -> Task {
        Task {
            id: self.id,
            priority: P::VALUE,
            callback: self.callback,
        }
    }
}

impl<P: PriorityLevel> std::fmt::Debug for TypedTask<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedTask")
            .field("id", &self.id)
            .field("priority", &P::NAME)
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
            Ordering::Equal => other.0.id.as_u64().cmp(&self.0.id.as_u64()), // Earlier ID first
            ord => ord,
        }
    }
}

/// Priority-based task queue
///
/// Tasks are executed in priority order:
/// UserInput > Animation > Build > Idle
///
/// ## Type-Safe Task Addition
///
/// ```rust
/// use flui_scheduler::task::TaskQueue;
/// use flui_scheduler::traits::AnimationPriority;
///
/// let queue = TaskQueue::new();
///
/// // Add with runtime priority
/// queue.add(flui_scheduler::Priority::Animation, || {});
///
/// // Add with type-safe priority
/// queue.add_typed::<AnimationPriority>(|| {});
/// ```
#[derive(Clone)]
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

    /// Create a task queue with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::with_capacity(capacity))),
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

    /// Add a typed task (compile-time priority checking)
    pub fn add_typed<P: PriorityLevel>(&self, callback: impl FnOnce() + Send + 'static) {
        self.add_task(Task::new(P::VALUE, callback));
    }

    /// Add a TypedTask to the queue
    pub fn add_typed_task<P: PriorityLevel>(&self, task: TypedTask<P>) {
        self.add_task(task.into_task());
    }

    /// Get the next task (highest priority)
    pub fn pop(&self) -> Option<Task> {
        self.queue.lock().pop().map(|pt| pt.0)
    }

    /// Peek at the next task without removing it
    pub fn peek_priority(&self) -> Option<Priority> {
        self.queue.lock().peek().map(|pt| pt.0.priority)
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

    /// Execute all tasks of a specific priority
    ///
    /// Returns number of tasks executed
    pub fn execute_priority(&self, priority: Priority) -> usize {
        let mut executed = 0;

        loop {
            let task = {
                let mut queue = self.queue.lock();
                if let Some(pt) = queue.peek() {
                    if pt.0.priority == priority {
                        queue.pop().map(|pt| pt.0)
                    } else {
                        break;
                    }
                } else {
                    break;
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

    /// Get count of tasks at each priority level
    pub fn count_by_priority(&self) -> PriorityCount {
        let queue = self.queue.lock();
        let mut counts = PriorityCount::default();

        for pt in queue.iter() {
            match pt.0.priority {
                Priority::UserInput => counts.user_input += 1,
                Priority::Animation => counts.animation += 1,
                Priority::Build => counts.build += 1,
                Priority::Idle => counts.idle += 1,
            }
        }

        counts
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Count of tasks at each priority level
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PriorityCount {
    /// Number of UserInput priority tasks
    pub user_input: usize,
    /// Number of Animation priority tasks
    pub animation: usize,
    /// Number of Build priority tasks
    pub build: usize,
    /// Number of Idle priority tasks
    pub idle: usize,
}

impl PriorityCount {
    /// Total number of tasks
    #[inline]
    pub fn total(&self) -> usize {
        self.user_input + self.animation + self.build + self.idle
    }

    /// Check if there are any high-priority tasks (UserInput or Animation)
    #[inline]
    pub fn has_high_priority(&self) -> bool {
        self.user_input > 0 || self.animation > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{AnimationPriority, BuildPriority, IdlePriority, UserInputPriority};

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::UserInput > Priority::Animation);
        assert!(Priority::Animation > Priority::Build);
        assert!(Priority::Build > Priority::Idle);
    }

    #[test]
    fn test_priority_navigation() {
        assert_eq!(Priority::Idle.higher(), Some(Priority::Build));
        assert_eq!(Priority::Build.higher(), Some(Priority::Animation));
        assert_eq!(Priority::Animation.higher(), Some(Priority::UserInput));
        assert_eq!(Priority::UserInput.higher(), None);

        assert_eq!(Priority::UserInput.lower(), Some(Priority::Animation));
        assert_eq!(Priority::Idle.lower(), None);
    }

    #[test]
    fn test_priority_from_u8() {
        assert_eq!(Priority::from_u8(0), Some(Priority::Idle));
        assert_eq!(Priority::from_u8(1), Some(Priority::Build));
        assert_eq!(Priority::from_u8(2), Some(Priority::Animation));
        assert_eq!(Priority::from_u8(3), Some(Priority::UserInput));
        assert_eq!(Priority::from_u8(4), None);
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
    fn test_typed_task() {
        let task = TypedTask::<UserInputPriority>::new(|| {});
        assert_eq!(task.priority(), Priority::UserInput);
        assert_eq!(task.priority_name(), "UserInput");
    }

    #[test]
    fn test_typed_task_queue() {
        let queue = TaskQueue::new();

        queue.add_typed::<IdlePriority>(|| {});
        queue.add_typed::<UserInputPriority>(|| {});
        queue.add_typed::<BuildPriority>(|| {});
        queue.add_typed::<AnimationPriority>(|| {});

        assert_eq!(queue.pop().unwrap().priority(), Priority::UserInput);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Animation);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Build);
        assert_eq!(queue.pop().unwrap().priority(), Priority::Idle);
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

    #[test]
    fn test_priority_count() {
        let queue = TaskQueue::new();

        queue.add(Priority::UserInput, || {});
        queue.add(Priority::UserInput, || {});
        queue.add(Priority::Animation, || {});
        queue.add(Priority::Build, || {});
        queue.add(Priority::Idle, || {});
        queue.add(Priority::Idle, || {});
        queue.add(Priority::Idle, || {});

        let counts = queue.count_by_priority();
        assert_eq!(counts.user_input, 2);
        assert_eq!(counts.animation, 1);
        assert_eq!(counts.build, 1);
        assert_eq!(counts.idle, 3);
        assert_eq!(counts.total(), 7);
        assert!(counts.has_high_priority());
    }
}
