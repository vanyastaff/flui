//! Task queue with priority-based scheduling
//!
//! Manages execution order of tasks within a frame based on priority.
//!
//! ## Key Types
//!
//! - [`Priority`] - Task priority enum (UserInput, Animation, Build, Idle)
//! - [`Task`] - A scheduled task with priority and callback
//! - [`TaskQueue`] - Priority-based task queue
//!
//! ## Priority Levels
//!
//! 1. **UserInput** (highest) - Mouse, keyboard, touch events
//! 2. **Animation** - Animation tickers, interpolations
//! 3. **Build** - Widget tree rebuilds
//! 4. **Idle** (lowest) - Background work, GC, telemetry

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

/// Generate the next unique task ID using a global atomic counter.
fn next_task_id() -> Option<TaskId> {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    next_task_id_with_counter(&COUNTER)
}

fn next_task_id_with_counter(counter: &AtomicUsize) -> Option<TaskId> {
    counter
        .try_update(
            AtomicOrdering::Relaxed,
            AtomicOrdering::Relaxed,
            |current| current.checked_add(1),
        )
        .ok()
        .map(TaskId::zip)
}

/// Task priority levels (higher value = higher priority).
///
/// Internal `#[repr(u8)]` discriminants are 0/1/2/3 for compact atomic
/// storage and exhaustive matching. The widely spaced numeric values
/// (0/50000/100000/200000 for `Idle/Build/Animation/UserInput`) are exposed
/// via [`Priority::numeric_value`], leaving room for relative-priority
/// offsets. FLUI deliberately uses a closed 4-variant enum rather than an
/// open priority class with offset arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum Priority {
    /// Background/idle work (GC, telemetry). Numeric value 0.
    Idle = 0,
    /// Normal UI updates (widget rebuilds). Sits between
    /// Idle and Animation. Numeric value 50000.
    #[default]
    Build = 1,
    /// Animations and transitions. Numeric value 100000.
    Animation = 2,
    /// User input events (must be immediate). Numeric value 200000.
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

    /// Spaced numeric value for this priority (`idle=0`, `build=50000`,
    /// `animation=100000`, `input=200000`).
    ///
    /// Use this when interoperating with code expecting a numeric priority
    /// scheme or when computing relative-priority offsets.
    #[inline]
    pub const fn numeric_value(self) -> u32 {
        match self {
            Priority::Idle => 0,
            Priority::Build => 50_000,
            Priority::Animation => 100_000,
            Priority::UserInput => 200_000,
        }
    }

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

impl TryFrom<u8> for Priority {
    type Error = u8;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_u8(value).ok_or(value)
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

/// Unique task identifier from `flui_foundation`.
pub use flui_foundation::TaskId;

/// A scheduled task with priority
pub struct Task {
    id: TaskId,
    priority: Priority,
    callback: Box<dyn FnOnce()>,
}

impl Task {
    /// Create a new task
    ///
    /// # Panics
    ///
    /// Panics when the process-wide task counter exhausts identities
    /// `1..usize::MAX`. `usize::MAX` is reserved for permanent exhaustion;
    /// catching the panic never permits another admission through that counter.
    /// On exhausted admission, the rejected callback is retained without running
    /// its destructor, preserving the capacity failure. This policy applies only
    /// to rejected admission; accepted callbacks retain their ordinary lifetime.
    pub fn new<F>(priority: Priority, callback: F) -> Self
    where
        F: FnOnce() + 'static,
    {
        Self::new_with_allocator(priority, callback, next_task_id)
    }

    fn new_with_allocator<F>(
        priority: Priority,
        callback: F,
        allocate: impl FnOnce() -> Option<TaskId>,
    ) -> Self
    where
        F: FnOnce() + 'static,
    {
        let Some(id) = allocate() else {
            std::mem::forget(callback);
            panic!("Task identities exhausted; refusing identity reuse");
        };
        Self {
            id,
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

/// One FIFO lane per [`Priority`], indexed by [`Priority::as_u8`].
type Lanes = [VecDeque<Task>; Priority::ALL.len()];

/// What one `TaskQueue::drain` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TaskDrain {
    /// Tasks that ran.
    pub(crate) executed: usize,
    /// Whether eligible tasks queued before the drain began were left queued:
    /// the run limit or the admission check stopped it first.
    pub(crate) deferred: bool,
}

/// Priority-based task queue
///
/// Tasks are executed in priority order:
/// UserInput > Animation > Build > Idle,
/// and in admission order within a priority.
///
/// ## Task Addition
///
/// ```rust
/// use flui_scheduler::task::TaskQueue;
///
/// let queue = TaskQueue::new();
/// queue.add(flui_scheduler::Priority::Animation, || {});
/// ```
#[derive(Clone)]
pub struct TaskQueue {
    lanes: Rc<RefCell<Lanes>>,
}

impl std::fmt::Debug for TaskQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The lanes hold opaque task closures; report the length.
        f.debug_struct("TaskQueue")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl TaskQueue {
    /// Detach scheduler-owned work before terminal callback retirement.
    pub(crate) fn detach_for_retirement(&self) -> impl Iterator<Item = Task> {
        self.lanes.take().into_iter().flatten()
    }

    /// Create a new task queue
    pub fn new() -> Self {
        Self {
            lanes: Rc::new(RefCell::new(Default::default())),
        }
    }

    /// Create a task queue with room for `capacity` tasks of each priority
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lanes: Rc::new(RefCell::new(std::array::from_fn(|_| {
                VecDeque::with_capacity(capacity)
            }))),
        }
    }

    /// Add a task to the queue
    pub fn add_task(&self, task: Task) {
        self.lanes.borrow_mut()[usize::from(task.priority.as_u8())].push_back(task);
    }

    /// Add a task with priority
    pub fn add<F>(&self, priority: Priority, callback: F)
    where
        F: FnOnce() + 'static,
    {
        self.add_task(Task::new(priority, callback));
    }

    /// Remove the next task: the oldest of the highest priority.
    pub fn pop(&self) -> Option<Task> {
        self.lanes
            .borrow_mut()
            .iter_mut()
            .rev()
            .find_map(VecDeque::pop_front)
    }

    /// The priority of the task [`Self::pop`] would return
    pub fn peek_priority(&self) -> Option<Priority> {
        Priority::ALL
            .into_iter()
            .rev()
            .find(|priority| !self.lanes.borrow()[usize::from(priority.as_u8())].is_empty())
    }

    /// Get number of pending tasks
    pub fn len(&self) -> usize {
        self.lanes.borrow().iter().map(VecDeque::len).sum()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.lanes.borrow().iter().all(VecDeque::is_empty)
    }

    /// Execute the tasks at or above `min_priority` that were queued when this
    /// call started, highest priority first.
    ///
    /// Returns the number of tasks executed; a task queued while it runs waits
    /// for the next call, and a panicking task loses only itself.
    pub fn execute_until(&self, min_priority: Priority) -> usize {
        self.drain(min_priority, usize::MAX, |_| true).executed
    }

    /// Execute, one at a time and outside the queue's borrow, the tasks at or
    /// above `min_priority` that were queued when this call started: highest
    /// priority first, oldest first within a priority, at most `limit` of
    /// them, each only if `admit` accepts its priority when its turn comes.
    ///
    /// The tasks a drain may run are fixed when it starts, by each lane's
    /// newest task id (ids grow in admission order). A task queued while the
    /// drain runs has a larger id and waits for the next drain, so a task that
    /// re-queues itself, at any priority, cannot run twice in one drain or
    /// push older tasks out of it. A task that panics loses only itself: the
    /// tasks behind it are still queued when the panic propagates.
    ///
    /// The first task `admit` refuses ends the drain; the frame uses it to
    /// stop `Priority::Idle` work at its deadline.
    pub(crate) fn drain(
        &self,
        min_priority: Priority,
        limit: usize,
        mut admit: impl FnMut(Priority) -> bool,
    ) -> TaskDrain {
        let watermarks: [Option<TaskId>; Priority::ALL.len()] = {
            let lanes = self.lanes.borrow();
            std::array::from_fn(|lane| lanes[lane].back().map(Task::id))
        };
        let mut drain = TaskDrain::default();
        loop {
            let next = {
                let lanes = self.lanes.borrow();
                Priority::ALL.into_iter().rev().find(|priority| {
                    let lane = usize::from(priority.as_u8());
                    *priority >= min_priority
                        && lanes[lane]
                            .front()
                            .zip(watermarks[lane])
                            .is_some_and(|(task, watermark)| task.id.get() <= watermark.get())
                })
            };
            let Some(priority) = next else {
                return drain;
            };
            if drain.executed == limit || !admit(priority) {
                drain.deferred = true;
                return drain;
            }
            let task = self.lanes.borrow_mut()[usize::from(priority.as_u8())]
                .pop_front()
                .expect("BUG: the lane's front was found under the previous borrow");
            task.execute();
            drain.executed += 1;
        }
    }

    /// Execute the tasks of exactly `priority` that were queued when this call
    /// started, oldest first, one at a time.
    ///
    /// Returns number of tasks executed
    pub fn execute_priority(&self, priority: Priority) -> usize {
        self.drain(priority, usize::MAX, |candidate| candidate == priority)
            .executed
    }

    /// Execute every task that was queued when this call started.
    ///
    /// Returns number of tasks executed
    pub fn execute_all(&self) -> usize {
        self.execute_until(Priority::Idle)
    }

    /// Get count of tasks at each priority level
    pub fn count_by_priority(&self) -> PriorityCount {
        let lanes = self.lanes.borrow();
        let count = |priority: Priority| lanes[usize::from(priority.as_u8())].len();
        PriorityCount {
            user_input: count(Priority::UserInput),
            animation: count(Priority::Animation),
            build: count(Priority::Build),
            idle: count(Priority::Idle),
        }
    }

    /// Test-only probe: `true` if this queue's borrow is currently free.
    ///
    /// Backs `flui-scheduler`'s scheduler-wide lock-discipline oracle
    /// (`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held`),
    /// so a callback that calls `add_task`/`execute_until` from inside another
    /// callback can be observed not re-entering this queue's borrow.
    #[cfg(test)]
    pub(crate) fn is_unlocked(&self) -> bool {
        self.lanes.try_borrow_mut().is_ok()
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
    use std::cell::Cell;

    use super::*;

    /// A panicking task must not take its queued siblings down with it: a
    /// single-lock batch drain would already have removed every matching
    /// task from the heap before running any of them, so a panic partway
    /// through the batch loses the rest silently. Popping one task at a
    /// time (issue #1057) means only the panicking task itself is ever
    /// removed before it runs.
    fn priority_task_panic_preserves_same_priority_tail() {
        let queue = TaskQueue::new();
        let ran: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));

        let ran_before = Rc::clone(&ran);
        queue.add(Priority::Build, move || ran_before.borrow_mut().push(1));
        queue.add(Priority::Build, || panic!("build task probe"));
        let ran_after = Rc::clone(&ran);
        queue.add(Priority::Build, move || ran_after.borrow_mut().push(3));

        let queue_len_before = queue.len();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            queue.execute_until(Priority::Build)
        }));
        assert!(unwind.is_err(), "the panic must propagate");

        assert_eq!(
            *ran.borrow(),
            vec![1],
            "only the task queued before the panic ran"
        );
        assert_eq!(
            queue.len(),
            queue_len_before - 2,
            "the panicking task is gone; the task queued after it is still queued"
        );

        // The next call resumes exactly where the panic left off.
        let executed = queue.execute_until(Priority::Build);
        assert_eq!(executed, 1);
        assert_eq!(*ran.borrow(), vec![1, 3]);
        assert_eq!(queue.len(), queue_len_before - 3);
    }

    fn exhausted_task_ids_preserve_priority_fifo() {
        struct RejectedCapture(Rc<AtomicUsize>);
        impl Drop for RejectedCapture {
            fn drop(&mut self) {
                self.0.fetch_add(1, AtomicOrdering::Relaxed);
            }
        }

        let counter = AtomicUsize::new(usize::MAX - 2);
        let queue = TaskQueue::new();
        let ran = Rc::new(RefCell::new(Vec::new()));
        for value in [1, 2] {
            let output = Rc::clone(&ran);
            queue.add_task(Task::new_with_allocator(
                Priority::Build,
                move || output.borrow_mut().push(value),
                || next_task_id_with_counter(&counter),
            ));
        }
        let rejected_drops = Rc::new(AtomicUsize::new(0));
        for _ in 0..8 {
            let capture = RejectedCapture(Rc::clone(&rejected_drops));
            let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                queue.add_task(Task::new_with_allocator(
                    Priority::Build,
                    move || drop(capture),
                    || next_task_id_with_counter(&counter),
                ));
            }))
            .expect_err("exhausted task identities must refuse priority admission");
            assert_eq!(
                refusal.downcast_ref::<&str>().copied(),
                Some("Task identities exhausted; refusing identity reuse")
            );
            assert_eq!(queue.len(), 2);
        }
        assert_eq!(rejected_drops.load(AtomicOrdering::Relaxed), 0);
        assert_eq!(queue.execute_until(Priority::Build), 2);
        assert_eq!(*ran.borrow(), vec![1, 2]);
        assert!(queue.is_empty());
        for _ in 0..4 {
            assert!(next_task_id_with_counter(&counter).is_none());
        }

        let output = Rc::clone(&ran);
        queue.add(Priority::Build, move || output.borrow_mut().push(3));
        let output = Rc::clone(&ran);
        queue.add(Priority::Build, move || output.borrow_mut().push(4));
        assert_eq!(queue.execute_until(Priority::Build), 2);
        assert_eq!(*ran.borrow(), vec![1, 2, 3, 4]);
    }

    /// A higher-priority task that re-queues itself waits for the next drain
    /// instead of taking every turn from older, lower-priority work.
    fn a_requeueing_input_task_does_not_starve_older_builds() {
        fn requeue(queue: TaskQueue, runs: Rc<Cell<u32>>) {
            let next = queue.clone();
            queue.add(Priority::UserInput, move || {
                runs.set(runs.get() + 1);
                requeue(next, runs);
            });
        }
        let queue = TaskQueue::new();
        let input_runs = Rc::new(Cell::new(0));
        requeue(queue.clone(), Rc::clone(&input_runs));
        let built = Rc::new(Cell::new(false));
        let build = Rc::clone(&built);
        queue.add(Priority::Build, move || build.set(true));

        assert_eq!(queue.execute_until(Priority::Build), 2);
        assert_eq!(
            input_runs.get(),
            1,
            "the re-queued copy waits for the next drain"
        );
        assert!(built.get(), "the older Build task ran in the same drain");
    }

    #[test]
    fn execute_until_leaves_later_same_priority_tasks_queued_when_one_panics() {
        crate::table_test::run_table(
            "task_queue_failure_and_ordering",
            &[
                (
                    "priority_task_panic_preserves_same_priority_tail",
                    priority_task_panic_preserves_same_priority_tail as fn(),
                ),
                (
                    "exhausted_task_ids_preserve_priority_fifo",
                    exhausted_task_ids_preserve_priority_fifo as fn(),
                ),
                (
                    "a_requeueing_input_task_does_not_starve_older_builds",
                    a_requeueing_input_task_does_not_starve_older_builds as fn(),
                ),
            ],
        );
    }
}
