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
    /// the tasks already queued when THIS call started.
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
    /// # The id watermark
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
    /// method). Task ids are minted from one monotonic counter
    /// (`next_task_id`), so this call reads the highest id currently
    /// queued ONCE, under the first lock acquisition, before running
    /// anything, and then only pops a task whose id is `<=` that watermark.
    /// A task enqueued reentrantly during this call always gets a strictly
    /// greater id and is left queued for the NEXT call to see — exactly
    /// what the batched snapshot this replaced also guaranteed, since it
    /// popped its whole matching run into a `Vec` before invoking any of
    /// it. Because ids are independent of priority, a freshly-enqueued
    /// task of the SAME or a HIGHER priority than something still-eligible
    /// and lower in this call's own watermarked set does not stop the scan
    /// early: it is popped, set aside, and the scan continues past it —
    /// only the top's priority against `min_priority` ends the scan, same
    /// as before.
    ///
    /// Preserves the priority-threshold semantics exactly: stops the
    /// moment the highest remaining ELIGIBLE priority no longer meets
    /// `min_priority`.
    ///
    /// Returns the number of tasks executed before returning — a panic
    /// propagates past this method with that count short of the full
    /// matching run; the caller observes the panic, not a partial count.
    pub fn execute_until(&self, min_priority: Priority) -> usize {
        // Read the watermark under its own lock acquisition, before
        // anything runs. `None` means the queue was empty at entry -- no
        // watermark, nothing to do.
        let watermark = {
            let queue = self.queue.lock();
            queue.iter().map(|pt| pt.0.id.get()).max()
        };
        let Some(watermark) = watermark else {
            return 0;
        };

        let mut executed = 0usize;
        // Tasks popped past because they exceed `watermark` (reentrant
        // additions from a task this call already ran) -- re-queued once,
        // after the scan, rather than immediately: pushing back into the
        // SAME heap we are still popping from would let a later iteration
        // of this very call see it again if it happens to sort back to the
        // top, defeating the watermark.
        let mut deferred: Vec<PriorityTask> = Vec::new();
        loop {
            let task = {
                let mut queue = self.queue.lock();
                loop {
                    match queue.peek() {
                        Some(pt) if pt.0.priority >= min_priority => {
                            let popped = queue.pop().expect(
                                "BUG: peek returned Some under the same lock, so pop must \
                                 succeed",
                            );
                            // Decrement inside the critical section —
                            // matches add_task / pop ordering, regardless
                            // of whether this pop turns out to be eligible
                            // or deferred: either way it left the heap here.
                            self.len.fetch_sub(1, AtomicOrdering::AcqRel);
                            if popped.0.id.get() <= watermark {
                                break Some(popped.0);
                            }
                            deferred.push(popped);
                        }
                        _ => break None,
                    }
                }
            };
            let Some(task) = task else {
                break;
            };
            task.execute();
            executed += 1;
        }

        if !deferred.is_empty() {
            let mut queue = self.queue.lock();
            let count = deferred.len();
            for pt in deferred {
                queue.push(pt);
            }
            // Re-added inside the critical section — matches add_task's
            // own ordering.
            self.len.fetch_add(count, AtomicOrdering::AcqRel);
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

    /// Clear all pending tasks
    pub fn clear(&self) {
        let mut queue = self.queue.lock();
        let cleared = queue.len();
        queue.clear();
        if cleared > 0 {
            self.len.fetch_sub(cleared, AtomicOrdering::AcqRel);
        }
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
