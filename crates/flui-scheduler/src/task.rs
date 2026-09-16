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

use std::{cmp::Ordering, collections::BinaryHeap, sync::Arc};

use parking_lot::Mutex;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

/// Generate the next unique task ID using a global atomic counter.
fn next_task_id() -> TaskId {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let value = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    TaskId::zip(value)
}

/// Task priority levels (higher value = higher priority).
///
/// Internal `#[repr(u8)]` discriminants are 0/1/2/3 for compact atomic
/// storage and exhaustive matching. The Flutter-faithful numeric values
/// (0/50000/100000/200000 — matching [`priority.dart:11-54`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/priority.dart)
/// `idle/animation/touch` with `Build` inserted between Idle and
/// Animation) are exposed via [`Priority::numeric_value`]. FLUI deliberately
/// uses a closed 4-variant enum instead of Flutter's open class +
/// `operator +/-` offset arithmetic — the offset arithmetic has zero usages
/// in the Flutter framework code itself (verified via repo-wide grep).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum Priority {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Background/idle work (GC, telemetry). Flutter `priority.dart::idle` = 0.
    Idle = 0,
    /// Normal UI updates (widget rebuilds). FLUI-only — inserted between
    /// Idle and Animation. Maps to Flutter numeric 50000.
    #[default]
    Build = 1,
    /// Animations and transitions. Flutter `priority.dart::animation` = 100000.
    Animation = 2,
    /// User input events (must be immediate). Flutter
    /// `priority.dart::touch` = 200000.
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

    /// Flutter-faithful numeric value for this priority.
    ///
    /// Maps to Flutter `priority.dart` numeric layout — see [`Priority`] doc
    /// comment for full mapping. Use this when interoperating with code
    /// expecting Flutter's `idle=0/animation=100000/touch=200000` scheme
    /// or when computing relative-priority offsets.
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
            id: next_task_id(),
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
    queue: Arc<Mutex<BinaryHeap<PriorityTask>>>,
    /// Lock-free mirror of the BinaryHeap length.
    ///
    /// Write-through on push / pop / drain operations. Allows callers like
    /// `UpdateScheduler::is_over_budget` to check queue depth without acquiring
    /// the queue lock. Per Gjengset *Rust Atomics and Locks* Ch 3:
    /// Acquire/Release ordering is sufficient because readers don't need
    /// total ordering across multiple atomics — they only care about a
    /// fresh observation of this single counter.
    len: Arc<AtomicUsize>,
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
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            len: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a task queue with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::with_capacity(capacity))),
            len: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Add a task to the queue
    pub fn add_task(&self, task: Task) {
        let mut queue = self.queue.lock();
        queue.push(PriorityTask(task));
        // Update the atomic len mirror BEFORE releasing the heap mutex.
        // Otherwise concurrent observers see a window where the heap
        // contains the new task but `len()` still reads the old value:
        // updating the atomic outside the critical section creates a
        // TOCTOU gap between the heap mutation and the atomic mirror update.
        self.len.fetch_add(1, AtomicOrdering::AcqRel);
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
        let mut queue = self.queue.lock();
        let popped = queue.pop().map(|pt| pt.0);
        if popped.is_some() {
            // Decrement inside the critical section — matches add_task
            // ordering so observers don't see len > heap-size or vice versa.
            self.len.fetch_sub(1, AtomicOrdering::AcqRel);
        }
        popped
    }

    /// Peek at the next task without removing it
    pub fn peek_priority(&self) -> Option<Priority> {
        self.queue.lock().peek().map(|pt| pt.0.priority)
    }

    /// Get number of pending tasks (lock-free).
    ///
    /// Reads the atomic length mirror; no lock acquisition. See [`TaskQueue::len`]
    /// field docs for ordering rationale.
    pub fn len(&self) -> usize {
        self.len.load(AtomicOrdering::Acquire)
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
        let mut budget = self.queue.lock().len();
        let mut executed = 0usize;
        while budget > 0 {
            let task = {
                let mut queue = self.queue.lock();
                match queue.peek() {
                    Some(pt) if pt.0.priority >= min_priority => {
                        let popped = queue.pop().expect(
                            "BUG: peek returned Some under the same lock, so pop must succeed",
                        );
                        // Decrement inside the critical section — matches
                        // add_task / pop ordering.
                        self.len.fetch_sub(1, AtomicOrdering::AcqRel);
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
            let mut queue = self.queue.lock();
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
            if !batch.is_empty() {
                self.len.fetch_sub(batch.len(), AtomicOrdering::AcqRel);
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
            let mut queue = self.queue.lock();
            let mut batch = Vec::with_capacity(queue.len());
            while let Some(pt) = queue.pop() {
                batch.push(pt.0);
            }
            // Decrement atomic len inside the critical section.
            if !batch.is_empty() {
                self.len.fetch_sub(batch.len(), AtomicOrdering::AcqRel);
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
            let queue = self.queue.lock();
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
        self.queue.try_lock().is_some()
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

    /// A panicking task must not take its queued siblings down with it: a
    /// single-lock batch drain would already have removed every matching
    /// task from the heap before running any of them, so a panic partway
    /// through the batch loses the rest silently. Popping one task at a
    /// time (issue #1057) means only the panicking task itself is ever
    /// removed before it runs.
    #[test]
    fn execute_until_leaves_later_same_priority_tasks_queued_when_one_panics() {
        let queue = TaskQueue::new();
        let ran: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));

        let ran_before = Arc::clone(&ran);
        queue.add(Priority::Build, move || ran_before.lock().push(1));
        queue.add(Priority::Build, || panic!("build task probe"));
        let ran_after = Arc::clone(&ran);
        queue.add(Priority::Build, move || ran_after.lock().push(3));

        let queue_len_before = queue.len();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            queue.execute_until(Priority::Build)
        }));
        assert!(unwind.is_err(), "the panic must propagate");

        assert_eq!(
            *ran.lock(),
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
        assert_eq!(*ran.lock(), vec![1, 3]);
        assert_eq!(queue.len(), queue_len_before - 3);
    }

    /// A task reentrantly enqueued by an earlier task in the SAME pass must
    /// not be lost when a LATER task in that pass panics. An id-watermark
    /// implementation tried first set a too-high-id pop aside in a local
    /// `deferred` buffer to keep scanning past it — and that buffer is
    /// itself dropped on unwind before ever making it back into the live
    /// heap, losing reentrant work the pre-#1057 batch drain never lost
    /// (nothing there ever left the heap except what was already
    /// executing). The count-budget version this replaced it with has no
    /// such buffer to lose anything from: A runs and enqueues X at a LOWER
    /// priority than B (so X cannot displace B's own turn — see the sibling
    /// test below for what happens when it can), B panics, and X — never
    /// popped at all this call — is simply still sitting in the live heap
    /// afterward.
    #[test]
    fn execute_until_does_not_drop_a_reentrantly_enqueued_task_when_a_later_task_panics() {
        let queue = TaskQueue::new();
        let x_ran = Arc::new(Mutex::new(false));

        let reentrant_queue = queue.clone();
        let x_ran_for_a = Arc::clone(&x_ran);
        queue.add(Priority::Build, move || {
            let x_ran = Arc::clone(&x_ran_for_a);
            reentrant_queue.add(Priority::Idle, move || {
                *x_ran.lock() = true;
            });
        }); // A
        queue.add(Priority::Build, || panic!("build task probe")); // B, higher priority than X

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            queue.execute_until(Priority::Build)
        }));
        assert!(unwind.is_err(), "B's panic must propagate");

        assert_eq!(
            queue.len(),
            1,
            "X, enqueued reentrantly by A, must still be queued after B panics -- not \
             dropped along with a local buffer that never survives an unwind"
        );

        let executed = queue.execute_until(Priority::Idle);
        assert_eq!(executed, 1, "X must run on the very next call");
        assert!(*x_ran.lock());
    }

    /// The count budget's own accepted trade-off, named rather than
    /// silently assumed correct: a reentrant task of a HIGHER priority than
    /// a still-queued sibling does not defer to it (an id watermark's own
    /// behavior) — it DISPLACES it, consuming the budget slot the sibling
    /// would otherwise have used. A runs and enqueues X at a HIGHER
    /// priority than B; X pops (and runs) in A's own call instead of B,
    /// leaving B for the NEXT call rather than this one. No work is lost
    /// either way -- B simply moves a whole call later than a naive
    /// "reentrant work is always deferred" reading would expect.
    #[test]
    fn execute_until_lets_a_higher_priority_reentrant_task_displace_a_lower_priority_sibling() {
        let queue = TaskQueue::new();
        let ran: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let reentrant_queue = queue.clone();
        let ran_for_a = Arc::clone(&ran);
        queue.add(Priority::Build, move || {
            ran_for_a.lock().push("a");
            let ran = Arc::clone(&ran_for_a);
            reentrant_queue.add(Priority::UserInput, move || ran.lock().push("x"));
        }); // A
        let ran_for_b = Arc::clone(&ran);
        queue.add(Priority::Build, move || ran_for_b.lock().push("b")); // B

        let executed = queue.execute_until(Priority::Build);
        assert_eq!(
            executed, 2,
            "A and X run this call; B's budget slot went to X instead"
        );
        assert_eq!(*ran.lock(), vec!["a", "x"]);
        assert_eq!(queue.len(), 1, "B is displaced, not lost");

        let executed = queue.execute_until(Priority::Build);
        assert_eq!(executed, 1);
        assert_eq!(
            *ran.lock(),
            vec!["a", "x", "b"],
            "B still runs, one call later"
        );
    }

    /// The pin for "nothing popped may live in a local across user code": a
    /// task enqueued reentrantly by A runs to completion even though a later
    /// sibling in the same pass panics. A pass that parked above-budget pops
    /// in a local buffer and re-pushed them afterwards would drop X together
    /// with that buffer during B's unwind — X would never run at all.
    #[test]
    fn a_reentrantly_enqueued_task_survives_a_later_siblings_panic() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let queue = TaskQueue::new();
        let ran: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let reentrant_queue = queue.clone();
        let ran_for_a = Arc::clone(&ran);
        queue.add(Priority::Build, move || {
            ran_for_a.lock().push("a");
            let ran = Arc::clone(&ran_for_a);
            reentrant_queue.add(Priority::UserInput, move || ran.lock().push("x"));
        }); // A
        queue.add(Priority::Build, || panic!("b panics")); // B

        // Pass 1 (budget 2): A runs and enqueues X; X outranks B and takes
        // the second slot; B is displaced to the next pass, not lost.
        let executed = queue.execute_until(Priority::Build);
        assert_eq!(executed, 2);
        assert_eq!(
            *ran.lock(),
            vec!["a", "x"],
            "X ran in the pass that enqueued it"
        );
        assert_eq!(queue.len(), 1, "B is still queued");

        // Pass 2: B panics. The panic propagates; nothing that already ran
        // is repeated and nothing queued is lost.
        let outcome = catch_unwind(AssertUnwindSafe(|| queue.execute_until(Priority::Build)));
        assert!(outcome.is_err(), "B's panic propagates to the caller");
        assert_eq!(
            queue.len(),
            0,
            "B consumed its own slot and nothing else remained"
        );
        assert_eq!(
            *ran.lock(),
            vec!["a", "x"],
            "X ran exactly once overall; a later sibling's panic never touches it"
        );
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
