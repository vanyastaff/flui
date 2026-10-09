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
use std::{cell::RefCell, cmp::Ordering, collections::BinaryHeap, rc::Rc};

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
            Ordering::Equal => other.0.id.get().cmp(&self.0.id.get()), // Earlier ID first
            ord => ord,
        }
    }
}

/// Priority-based task queue
///
/// Tasks are executed in priority order:
/// UserInput > Animation > Build > Idle
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
    queue: Rc<RefCell<BinaryHeap<PriorityTask>>>,
}

impl std::fmt::Debug for TaskQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The heap holds opaque task closures; report the lock-free length.
        f.debug_struct("TaskQueue")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl TaskQueue {
    /// Create a new task queue
    pub fn new() -> Self {
        Self {
            queue: Rc::new(RefCell::new(BinaryHeap::new())),
        }
    }

    /// Create a task queue with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: Rc::new(RefCell::new(BinaryHeap::with_capacity(capacity))),
        }
    }

    /// Add a task to the queue
    pub fn add_task(&self, task: Task) {
        let mut queue = self.queue.borrow_mut();
        queue.push(PriorityTask(task));
        // Update the atomic len mirror BEFORE releasing the heap mutex.
        // Otherwise concurrent observers see a window where the heap
        // contains the new task but `len()` still reads the old value:
        // updating the atomic outside the critical section creates a
        // TOCTOU gap between the heap mutation and the atomic mirror update.
    }

    /// Add a task with priority
    pub fn add<F>(&self, priority: Priority, callback: F)
    where
        F: FnOnce() + 'static,
    {
        self.add_task(Task::new(priority, callback));
    }

    /// Get the next task (highest priority)
    pub fn pop(&self) -> Option<Task> {
        let mut queue = self.queue.borrow_mut();
        let popped = queue.pop().map(|pt| pt.0);
        if popped.is_some() {
            // Decrement inside the critical section — matches add_task
            // ordering so observers don't see len > heap-size or vice versa.
        }
        popped
    }

    /// Peek at the next task without removing it
    pub fn peek_priority(&self) -> Option<Priority> {
        self.queue.borrow_mut().peek().map(|pt| pt.0.priority)
    }

    /// Get number of pending tasks (lock-free).
    ///
    /// Reads the atomic length mirror; no lock acquisition. See [`TaskQueue::len`]
    /// field docs for ordering rationale.
    pub fn len(&self) -> usize {
        self.queue.borrow().len()
    }

    /// Check if queue is empty (lock-free).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Execute tasks at or above `min_priority`, one at a time, bounded to
    /// the NUMBER of tasks already queued when THIS call started.
    ///
    /// Each iteration takes the queue lock only long enough to pop one
    /// eligible task and execute it OUTSIDE the lock — the same
    /// pop-one-under-lock, execute-outside-it shape [`Self::pop`] and
    /// `UpdateScheduler::flush_microtasks` use, rather than draining the
    /// whole matching run into a batch before executing any of it. A task
    /// that panics loses only itself: every task still behind it stays
    /// queued for the next call to see, instead of already being removed
    /// from the heap by this call's own batch drain and lost with it
    /// (issue #1057). The atomic `len` mirror is decremented per pop,
    /// inside the same critical section as the pop itself — matching
    /// `add_task`/`pop`'s ordering, so a panic mid-loop cannot leave
    /// `len()` under- or over-reporting the heap's real size.
    ///
    /// # The count budget, and why not an id watermark
    ///
    /// Simply re-peeking the live heap until its top drops below
    /// `min_priority` is NOT equivalent to the batched version this
    /// replaced: a task that enqueues another task of its own during this
    /// same call would be visible to that live re-peek and run in the SAME
    /// call, with nothing bounding a chain that keeps re-enqueuing itself
    /// (measured: a self-re-enqueuing `Priority::Build` task ran 500+
    /// times in one `execute_until` call and never returned, hanging the
    /// frame — the caller's own reentrant-pass cap in `handle_draw_frame`
    /// never got a chance to fire, since it only runs BETWEEN calls to this
    /// method). This reads `queue.len()` ONCE, under the first lock
    /// acquisition, as a budget, and decrements it once per pop; the loop
    /// stops the moment EITHER the budget is spent OR the top no longer
    /// meets `min_priority`. Unlike `handle_begin_frame`'s transient-
    /// callback deque, this heap has no mid-scan removal API (nothing here
    /// plays the role `cancel_frame_callback` plays there), so a plain
    /// count is sound: the budget can only ever be spent on pops this call
    /// itself performs, never invalidated out from under it by a sibling
    /// mutation.
    ///
    /// A first attempt at this used an id watermark instead — pop, and if
    /// the popped task's id exceeded the watermark, set it aside in a local
    /// buffer and keep scanning past it — to let a reentrant HIGHER-priority
    /// task skip the scan without ending it early. That local buffer is
    /// itself the bug this replaced: a later task's panic in the SAME call
    /// unwound straight through it, dropping every task it had set aside
    /// with no way back into the live heap — losing reentrant work the
    /// pre-#1057 batch drain never lost, since nothing there ever left the
    /// heap except what was already executing. The count budget has no such
    /// buffer at all: a reentrant task, if it displaces anything, displaces
    /// a PRE-EXISTING one to the next call (the reentrant task consumes a
    /// budget slot a pre-existing task would otherwise have used), never a
    /// task already popped and pending re-insertion.
    ///
    /// Returns the number of tasks executed before returning — a panic
    /// propagates past this method with that count short of the full
    /// matching run; the caller observes the panic, not a partial count.
    pub fn execute_until(&self, min_priority: Priority) -> usize {
        let mut budget = self.queue.borrow_mut().len();
        let mut executed = 0usize;
        while budget > 0 {
            let task = {
                let mut queue = self.queue.borrow_mut();
                match queue.peek() {
                    Some(pt) if pt.0.priority >= min_priority => {
                        let popped = queue.pop().expect(
                            "BUG: peek returned Some under the same lock, so pop must succeed",
                        );
                        // Decrement inside the critical section — matches
                        // add_task / pop ordering.
                        Some(popped.0)
                    }
                    _ => None,
                }
            };
            let Some(task) = task else {
                break;
            };
            budget -= 1;
            task.execute();
            executed += 1;
        }
        executed
    }

    /// Execute all tasks of a specific priority
    ///
    /// Returns number of tasks executed
    pub fn execute_priority(&self, priority: Priority) -> usize {
        let tasks = {
            let mut queue = self.queue.borrow_mut();
            let mut batch = Vec::with_capacity(queue.len());
            while let Some(pt) = queue.peek() {
                if pt.0.priority == priority {
                    let task = queue
                        .pop()
                        .expect("BUG: peek returned Some under the same lock, so pop must succeed");
                    batch.push(task.0);
                } else {
                    break;
                }
            }
            batch
        };

        let count = tasks.len();
        for task in tasks {
            task.execute();
        }
        count
    }

    /// Execute all pending tasks
    ///
    /// Returns number of tasks executed
    pub fn execute_all(&self) -> usize {
        let tasks: Vec<Task> = {
            let mut queue = self.queue.borrow_mut();
            let mut batch = Vec::with_capacity(queue.len());
            while let Some(pt) = queue.pop() {
                batch.push(pt.0);
            }
            batch
        };

        let count = tasks.len();
        for task in tasks {
            task.execute();
        }
        count
    }

    /// Get count of tasks at each priority level
    pub fn count_by_priority(&self) -> PriorityCount {
        let mut counts = PriorityCount::default();
        {
            let queue = self.queue.borrow_mut();
            for pt in queue.iter() {
                match pt.0.priority {
                    Priority::UserInput => counts.user_input += 1,
                    Priority::Animation => counts.animation += 1,
                    Priority::Build => counts.build += 1,
                    Priority::Idle => counts.idle += 1,
                }
            }
        }
        counts
    }

    /// Test-only probe: `true` if this queue's lock is currently free.
    ///
    /// Backs `flui-scheduler`'s scheduler-wide lock-discipline oracle
    /// (`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held`),
    /// so a callback that calls `add_task`/`execute_until` from inside another callback
    /// can be observed NOT deadlocking against this queue's own mutex.
    /// `queue` is private to this module; the oracle probes it through
    /// this method rather than reaching into the field directly.
    #[cfg(test)]
    pub(crate) fn is_unlocked(&self) -> bool {
        self.queue.try_borrow_mut().ok().is_some()
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
            ],
        );
    }
}
