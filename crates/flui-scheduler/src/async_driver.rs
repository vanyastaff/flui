//! [`AsyncDriver`] — the frame-driven, owner-local task driver.
//!
//! # What this is
//!
//! A single-threaded executor with no runtime, no thread pool, and no
//! dependency beyond [`std::future`] / [`std::task`]. Futures are polled on the
//! **owner thread**, in the gap between a frame's transient callbacks
//! (animation ticks) and its persistent callbacks (build → layout → paint) —
//! [`SchedulerPhase::MidFrameMicrotasks`](crate::SchedulerPhase::MidFrameMicrotasks) —
//! or by a background pump that runs no frame.
//!
//! # Ownership
//!
//! The tasks live in a `TaskStore` owned by the UI runtime's
//! [`OwnerFrame`](crate::OwnerFrame), which holds the only strong reference.
//! An [`AsyncDriver`] is a `Weak` handle to that store: a widget state keeps
//! one, and even a leaked one keeps no task — and none of a task's captures —
//! alive after the UI runtime. Because the futures never leave the owner thread
//! they need not be `Send`: a task may hold `Rc` state, and its captures are
//! created, polled and dropped on the owner thread.
//!
//! Spawning through a handle whose UI runtime is gone drops the future at once,
//! on the calling (owner) thread, and returns an already-cancelled
//! [`TaskToken`].
//!
//! # Waking
//!
//! A task's [`Waker`] is `Send + Sync` and reaches only the store's
//! cross-thread half (`WakeShared`), through a `Weak`: a ready queue and the
//! frame-request hook. It sets a per-task `ready` flag and — **only on the
//! `false → true` transition** — queues the task's id and asks for a frame
//! through the hook ([`OwnerFrame::new`](crate::OwnerFrame::new) installs the
//! scheduler's [`FrameWaker`](crate::FrameWaker)). So a burst of wakes between
//! frames costs one readiness transition and one successfully delivered frame
//! request. A failed or absent hook retains delivery debt for the next wake or
//! hook installation, including a wake through another clone of the same
//! waker. A wake after the UI runtime is gone upgrades nothing and does nothing.
//!
//! Waking is legal from any thread. Polling is not: it happens only inside
//! [`OwnerFrame::drive_frame`](crate::OwnerFrame::drive_frame) or
//! [`OwnerFrame::pump_background`](crate::OwnerFrame::pump_background), on the owner.
//!
//! # Readiness index
//!
//! Readiness discovery is separate from task ownership (issue #1056): the
//! store keeps `tasks` (every live task, keyed by id, owner-local) beside
//! `ready` (the ids due a poll, in wake-arrival order, in the cross-thread
//! half), so a pump on an idle driver touches neither a dormant task's slot
//! nor its atomic flag — it only drains `ready`, which is empty.
//!
//! **Invariant:** `ready == true` (a task's own flag) implies its id is in
//! `ready` **or** in the in-flight pump's own unprocessed tail (the ids
//! `PumpGuard` has not reached yet). Every path that sets the flag `true` also
//! pushes the id: [`spawn_local`](AsyncDriver::spawn_local) (seeds the flag
//! `true` and pushes); the waker's `false → true` edge (pushes unless the task
//! is retired or cancelled, or not yet admitted);
//! [`spawn_local_eager`](AsyncDriver::spawn_local_eager) (pushes iff a wake
//! armed the task during its inline poll, once it is admitted); and a pump's
//! own unreached remainder, restored by `PumpGuard::drop` on panic and
//! consumed normally on success.
//!
//! `ready` is **never proactively purged** on cancel or retirement, nor
//! scrubbed for an id whose task then panics — such an entry goes stale for at
//! most one pump and self-heals via the pump's "id not found in `tasks` ⇒
//! skip" arm, which is cheaper than an O(R) scan on every cancel. A waker that
//! read the task as live just before the owner retired it can likewise push
//! one stale id and request one spurious frame. A stale entry can also be a
//! same-id **duplicate** — a cross-thread wake racing an eager spawn's
//! admission can push the id while the spawn pushes it too — so the pump
//! sorts and `dedup()`s its drained batch rather than assuming the flag
//! makes a duplicate impossible.
//!
//! A same-id duplicate can also **straddle a drain**: if one push lands
//! before a pump's swap (drained and polled this pump) and the other lands
//! after (queued for the next one), the next pump finds a *live* task whose
//! own flag reads `false` — not a ghost, so it is polled rather than
//! skipped. That poll is spurious (nothing new is actually ready), but
//! harmless and self-limiting: `Future::poll` may be called at any time and
//! must handle it, and the extra entry does not recur.
//!
//! Churn without an intervening pump is bounded by the spawns and wakes
//! since the last drain — one entry per event, 8 bytes each — and a pump
//! drains all of it. That bound is per gap between pumps, not a global cap: a
//! tight `spawn_local` + immediate `cancel()` loop, with no pump between
//! iterations, leaves one stale id per pair even though the pending count is
//! `0` throughout — the next pump still clears every one of them.
//!
//! # Cancellation and retirement
//!
//! [`TaskToken`] cancels on drop: the future is removed from the driver and
//! dropped, so it is never polled again and its destructors run. During an
//! existing unwind the detached future is retained instead: running opaque
//! destruction then could abort the process. A `Waker` held by a cancelled or
//! retired task is inert — it finds the task's flags closed and does nothing.
//!
//! The UI runtime's teardown retires every remaining task explicitly
//! ([`OwnerFrame::retire`](crate::OwnerFrame::retire)), on the owner thread,
//! each future dropped under its own catch, the first panic kept.
//!
//! # What it never does
//!
//! Touch an element tree, a render tree, or a pipeline. A task that wants a
//! rebuild calls `RebuildHandle::schedule()`, which only writes to the
//! build owner's inbox.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Wake, Waker};

use parking_lot::Mutex;

/// A future the driver polls on the owner thread.
///
/// Not `Send`: the driver never moves it off the thread that spawned it, so
/// it may hold `Rc` state. Only its [`Waker`] crosses threads.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + 'static>>;

/// Frame-request hook: "please schedule another frame".
type RequestFrame = Arc<dyn Fn() + Send + Sync>;

/// Monotonic task id. Never reused, so a stale [`Waker`] cannot resurrect or
/// mis-target a later task.
type TaskId = u64;

/// A panic payload kept from a retirement.
pub(crate) type RetirePanic = Box<dyn Any + Send>;

/// One task's cross-thread flags: what its waker and its token read.
struct TaskFlags {
    /// `true` between a wake and the poll that clears it.
    ready: AtomicBool,
    /// Set by [`TaskToken`]'s cancellation and by a poll that panicked.
    /// Checked after a poll returns, so a token dropped *during* a poll still
    /// cancels rather than re-queueing.
    cancelled: AtomicBool,
    /// Set once the task has left the store for any reason, so its waker
    /// stops queueing ids and requesting frames.
    retired: AtomicBool,
    /// `false` only while `spawn_local_eager` polls the task inline, before
    /// it is in the store: a wake then only arms `ready`, and the spawn
    /// queues the id and requests the frame once the task is admitted.
    admitted: AtomicBool,
}

impl TaskFlags {
    fn new(ready: bool, admitted: bool) -> Self {
        Self {
            ready: AtomicBool::new(ready),
            cancelled: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            admitted: AtomicBool::new(admitted),
        }
    }

    fn closed() -> Self {
        Self {
            ready: AtomicBool::new(false),
            cancelled: AtomicBool::new(true),
            retired: AtomicBool::new(true),
            admitted: AtomicBool::new(true),
        }
    }
}

/// One live task.
struct Task {
    /// The future, absent while it is being polled (moved out so no borrow of
    /// the task map is held across user code).
    future: Option<BoxedTask>,
    /// One identity for the task lifetime; cloning it for a poll never allocates.
    waker: Waker,
    flags: Arc<TaskFlags>,
}

/// The store's cross-thread half: everything a [`Waker`] may reach.
struct WakeShared {
    /// Ids whose waker fired since the last drain, in wake-arrival order.
    /// Sorted and deduplicated once per pump, outside the lock.
    ready: Mutex<Vec<TaskId>>,
    request_frame: Mutex<Option<RequestFrame>>,
    wake_delivery: crate::wake_delivery::WakeDelivery,
}

impl WakeShared {
    /// Ask for a frame, if a hook is installed. No lock is held while the
    /// hook runs.
    fn request_frame(&self, fresh: bool) {
        self.wake_delivery
            .request(|| fresh, || self.request_frame.lock().clone());
    }
}

/// The UI runtime's task set: owner-local, reached through [`AsyncDriver`]'s
/// `Weak` and owned only by [`OwnerFrame`](crate::OwnerFrame).
pub(crate) struct TaskStore {
    /// `BTreeMap`, not `HashMap`: a `HashMap`'s hash-seed-dependent
    /// iteration is worth avoiding wherever this map is walked directly.
    tasks: RefCell<BTreeMap<TaskId, Task>>,
    /// An always-empty scratch buffer, ping-ponged with `ready` at the start
    /// of every pump (`mem::swap`, not `mem::take`) purely so `ready` always
    /// starts a pump already capacity-warmed. Without it, a self-waking task
    /// pushing its own id back during the pump would regrow `ready` from
    /// empty every pump instead of reaching zero allocations.
    spare: RefCell<Vec<TaskId>>,
    /// The next id to issue. `u64::MAX` is reserved for permanent exhaustion.
    next_id: Cell<TaskId>,
    /// Set by retirement: no task is admitted afterwards.
    closed: Cell<bool>,
    execution_failure: RefCell<Option<RcWeak<Cell<bool>>>>,
    shared: Arc<WakeShared>,
}

impl TaskStore {
    pub(crate) fn execution_failure_slot(&self) -> &RefCell<Option<RcWeak<Cell<bool>>>> {
        &self.execution_failure
    }
    pub(crate) fn new() -> Self {
        Self::with_first_id(1)
    }

    pub(crate) fn with_first_id(first: TaskId) -> Self {
        Self {
            tasks: RefCell::new(BTreeMap::new()),
            spare: RefCell::new(Vec::new()),
            next_id: Cell::new(first),
            closed: Cell::new(false),
            execution_failure: RefCell::new(None),
            shared: Arc::new(WakeShared {
                ready: Mutex::new(Vec::new()),
                request_frame: Mutex::new(None),
                wake_delivery: crate::wake_delivery::WakeDelivery::default(),
            }),
        }
    }

    fn next_task_id(&self) -> Option<TaskId> {
        let current = self.next_id.get();
        let next = current.checked_add(1)?;
        self.next_id.set(next);
        Some(current)
    }

    /// Install the "request a frame" hook; installation retries undelivered
    /// demand. The displaced hook is dropped only after the lock is released:
    /// its captures are user code that may re-enter the driver.
    ///
    /// A retired store refuses the hook: it is released at once, under
    /// [`release_opaque`]'s policy, rather than kept past the UI runtime.
    pub(crate) fn set_request_frame(&self, hook: RequestFrame) {
        if self.closed.get() {
            if let Err(payload) = release_opaque(hook) {
                resume_unwind(payload);
            }
            return;
        }
        let previous = { self.shared.request_frame.lock().replace(hook) };
        drop(previous);
        self.shared.request_frame(false);
    }

    fn new_waker(&self, id: TaskId, flags: &Arc<TaskFlags>) -> Waker {
        Waker::from(Arc::new(TaskWaker {
            id,
            flags: Arc::clone(flags),
            shared: Arc::downgrade(&self.shared),
        }))
    }

    fn token(self: &Rc<Self>, id: TaskId, flags: Arc<TaskFlags>) -> TaskToken {
        TaskToken {
            id,
            flags,
            store: Rc::downgrade(self),
        }
    }

    fn spawn(self: &Rc<Self>, future: BoxedTask) -> TaskToken {
        let Some(id) = self.next_task_id() else {
            // Capacity refusal is authoritative over rejected opaque destruction.
            mem::forget(future);
            panic!("AsyncDriver task identities exhausted; refusing identity reuse");
        };
        let flags = Arc::new(TaskFlags::new(true, true));
        let waker = self.new_waker(id, &flags);
        self.tasks.borrow_mut().insert(
            id,
            Task {
                future: Some(future),
                waker,
                flags: Arc::clone(&flags),
            },
        );
        // Starts ready (see `spawn_local`'s doc): index it immediately.
        self.shared.ready.lock().push(id);
        let token = self.token(id, flags);
        // Establish rollback ownership before calling the host hook. If the
        // hook fails, token destruction detaches the unreturned task.
        self.shared.request_frame(true);
        token
    }

    fn spawn_eager(self: &Rc<Self>, mut future: BoxedTask) -> Option<TaskToken> {
        let Some(id) = self.next_task_id() else {
            mem::forget(future);
            panic!("AsyncDriver task identities exhausted; refusing identity reuse");
        };
        // Starts NOT ready and NOT admitted: we are about to poll it
        // ourselves. A wake landing during that poll only arms `ready`; once
        // the task is inserted below, an armed task is queued and its frame
        // requested, exactly once, here.
        let flags = Arc::new(TaskFlags::new(false, false));
        let waker = self.new_waker(id, &flags);
        let mut cx = Context::from_waker(&waker);

        let outcome = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(payload) => {
                flags.cancelled.store(true, Ordering::Release);
                flags.retired.store(true, Ordering::Release);
                mem::forget(future);
                resume_unwind(payload);
            }
        };
        if outcome.is_ready() {
            flags.retired.store(true, Ordering::Release);
            crate::scheduler::execution::retire_with_execution_custody(
                &self.execution_failure,
                future,
            );
            return None;
        }

        // Polling is user code and may retire the owner through another
        // handle. A pending future must never reopen that closed store.
        if self.closed.get() {
            flags.cancelled.store(true, Ordering::Release);
            flags.retired.store(true, Ordering::Release);
            crate::scheduler::execution::retire_with_execution_custody(
                &self.execution_failure,
                future,
            );
            return Some(TaskToken::refused());
        }

        self.tasks.borrow_mut().insert(
            id,
            Task {
                future: Some(future),
                waker,
                flags: Arc::clone(&flags),
            },
        );
        // `SeqCst` pairs with the waker's `ready` swap and `admitted` load: a
        // wake either armed `ready` before this load (queued here) or sees
        // `admitted` (queues itself). It may do both; the pump deduplicates.
        flags.admitted.store(true, Ordering::SeqCst);
        let armed = flags.ready.load(Ordering::SeqCst);
        if armed {
            self.shared.ready.lock().push(id);
        }
        let token = self.token(id, flags);
        if armed {
            self.shared.request_frame(true);
        }
        Some(token)
    }

    /// Remove `id`'s slot, marking it retired. The caller drops the returned
    /// task (and so its future) after the map borrow is released.
    fn remove(&self, id: TaskId) -> Option<Task> {
        let removed = self.tasks.borrow_mut().remove(&id);
        if let Some(task) = &removed {
            task.flags.retired.store(true, Ordering::Release);
        }
        removed
    }

    pub(crate) fn poll_ready(&self) -> usize {
        // Drain the ready index, then release every lock and borrow: a task's
        // `poll` may spawn, cancel, or wake. Swap with `spare`, not
        // `mem::take` `ready` directly — see `spare`'s doc for why.
        let mut remaining: Vec<TaskId> = {
            let mut ready = self.shared.ready.lock();
            self.shared.wake_delivery.consume(|| {});
            let mut spare = self.spare.borrow_mut();
            mem::swap(&mut *ready, &mut *spare);
            mem::take(&mut *spare)
        };

        // Ascending order keeps polling deterministic; `dedup` (adjacent-only,
        // hence the sort first) collapses a same-id double push.
        remaining.sort_unstable();
        remaining.dedup();

        let mut guard = PumpGuard {
            store: self,
            remaining,
            cursor: 0,
            in_flight: None,
            done: false,
        };

        let mut polled = 0;
        while guard.cursor < guard.remaining.len() {
            let id = guard.remaining[guard.cursor];
            // Advance before polling, so `cursor` always means "ids already
            // consumed" — including the one about to be polled — no matter
            // where a panic below unwinds from.
            guard.cursor += 1;

            // Take the future out so no borrow is held across user code.
            let taken = {
                let mut tasks = self.tasks.borrow_mut();
                tasks.get_mut(&id).and_then(|task| {
                    // Clear BEFORE polling: a wake landing during the poll must
                    // re-arm the task rather than be swallowed.
                    task.flags.ready.store(false, Ordering::Release);
                    task.future
                        .take()
                        .map(|future| (future, task.waker.clone(), Arc::clone(&task.flags)))
                })
            };
            let Some((mut future, waker, flags)) = taken else {
                // Stale id: cancelled, retired, or a same-pump duplicate whose
                // first occurrence already took the slot.
                continue;
            };

            // Armed for the poll only: if `future.poll` panics, `PumpGuard`
            // removes this slot on unwind (issue #1057).
            guard.in_flight = Some(id);

            let mut cx = Context::from_waker(&waker);
            // Borrow the future into the recovery boundary: it must remain
            // owned outside the closure so poll unwinding cannot drop it.
            let outcome = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(payload) => {
                    flags.cancelled.store(true, Ordering::Release);
                    mem::forget(future);
                    // PumpGuard removes the empty slot and restores siblings;
                    // none of its recovery values run user destruction.
                    resume_unwind(payload);
                }
            };
            guard.in_flight = None;
            polled += 1;

            let finished = match outcome {
                Poll::Ready(()) => {
                    drop(self.remove(id));
                    Some(future)
                }
                Poll::Pending if flags.cancelled.load(Ordering::Acquire) => {
                    // The token was dropped while we polled; honour it.
                    drop(self.remove(id));
                    Some(future)
                }
                Poll::Pending => {
                    let mut tasks = self.tasks.borrow_mut();
                    if let Some(task) = tasks.get_mut(&id) {
                        task.future = Some(future);
                        None
                    } else {
                        // `cancel()` already removed the slot; drop the future.
                        Some(future)
                    }
                }
            };
            // The finished future's destructor is user code (it may hold a
            // nested `TaskToken`) and runs with no borrow held.
            crate::scheduler::execution::retire_with_execution_custody(
                &self.execution_failure,
                finished,
            );
        }

        // `done` first: if anything below ever panicked, `PumpGuard::drop`
        // would see `done == true` and never `split_off` a consumed batch.
        guard.done = true;
        let buffer = mem::take(&mut guard.remaining);
        recycle(&mut self.spare.borrow_mut(), buffer);

        polled
    }

    pub(crate) fn pending_task_count(&self) -> usize {
        self.tasks.borrow().len()
    }

    pub(crate) fn ready_task_count(&self) -> usize {
        let ready = self.shared.ready.lock();
        let tasks = self.tasks.borrow();
        ready
            .iter()
            .filter(|id| {
                tasks
                    .get(id)
                    .is_some_and(|task| task.flags.ready.load(Ordering::Acquire))
            })
            .count()
    }

    /// Close admission and wake delivery, returning ownership without running
    /// any opaque destructor. All task flags are closed before user code can
    /// reenter. Repeated calls detach nothing.
    pub(crate) fn detach_for_retirement(&self) -> RetiringTasks {
        self.closed.set(true);
        let tasks = mem::take(&mut *self.tasks.borrow_mut());
        for task in tasks.values() {
            task.flags.cancelled.store(true, Ordering::Release);
            task.flags.retired.store(true, Ordering::Release);
        }
        let hook = self.shared.request_frame.lock().take();
        self.shared.ready.lock().clear();
        RetiringTasks { tasks, hook }
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn is_unlocked(&self) -> bool {
        self.tasks.try_borrow_mut().is_ok()
            && self.spare.try_borrow_mut().is_ok()
            && self.shared.ready.try_lock().is_some()
            && self.shared.request_frame.try_lock().is_some()
            && self.shared.wake_delivery.is_unlocked()
    }
}

/// Outgoing ownership detached from the closed store before user code runs.
pub(crate) struct RetiringTasks {
    tasks: BTreeMap<TaskId, Task>,
    hook: Option<RequestFrame>,
}

impl RetiringTasks {
    /// Drop tasks then the hook, preserving the first failure. During an
    /// existing unwind retain opaque values: an outer catch cannot rescue
    /// double-panicking aggregate drop glue.
    pub(crate) fn retire_preserving_failure(
        self,
        preserve_failure: bool,
        failure_signal: &Cell<bool>,
    ) -> Option<RetirePanic> {
        self.retire_impl(preserve_failure, Some(failure_signal))
    }

    fn retire_impl(
        self,
        preserve_failure: bool,
        failure_signal: Option<&Cell<bool>>,
    ) -> Option<RetirePanic> {
        let mut first: Option<RetirePanic> = None;
        let mut retire = |value| {
            if preserve_failure
                || first.is_some()
                || failure_signal.is_some_and(Cell::get)
                || std::thread::panicking()
            {
                mem::forget(value);
            } else if let Err(payload) = release_opaque(value) {
                if let Some(signal) = failure_signal {
                    signal.set(true);
                }
                if first.is_none() {
                    first = Some(payload);
                } else {
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
        };
        for (_, mut task) in self.tasks {
            if let Some(future) = task.future.take() {
                retire(future);
            }
        }
        // Its captures die here, not with the last outstanding waker.
        if let Some(hook) = self.hook {
            // A hook has a different envelope type than a task future.
            if preserve_failure
                || first.is_some()
                || failure_signal.is_some_and(Cell::get)
                || std::thread::panicking()
            {
                mem::forget(hook);
            } else if let Err(payload) = release_opaque(hook) {
                if let Some(signal) = failure_signal {
                    signal.set(true);
                }
                first = Some(payload);
            }
        }
        first
    }
}

/// The `Waker` payload for one task: `Send + Sync`, reaching only the
/// store's cross-thread half, through a `Weak`.
struct TaskWaker {
    id: TaskId,
    flags: Arc<TaskFlags>,
    /// `Weak`: a waker a worker keeps must not keep the UI runtime's wake state
    /// alive, and nothing on this side may reach the owner-local tasks.
    shared: Weak<WakeShared>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if self.flags.cancelled.load(Ordering::Acquire)
            || self.flags.retired.load(Ordering::Acquire)
        {
            return;
        }
        let fresh = !self.flags.ready.swap(true, Ordering::SeqCst);
        if !self.flags.admitted.load(Ordering::SeqCst) {
            // Mid inline poll: the eager spawn queues the armed task.
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if fresh {
            shared.ready.lock().push(self.id);
        }
        shared.request_frame(fresh);
    }
}

/// Cancels its task when dropped.
///
/// Hold it for as long as the task should run. `ViewState::dispose` dropping one
/// is what stops an in-flight `FutureBuilder` subscription. Owner-local, like
/// the driver that issued it.
#[derive(Debug)]
pub struct TaskToken {
    id: TaskId,
    flags: Arc<TaskFlags>,
    store: RcWeak<TaskStore>,
}

impl std::fmt::Debug for TaskFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskFlags")
            .field("ready", &self.ready.load(Ordering::Relaxed))
            .field("cancelled", &self.cancelled.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for TaskStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskStore")
            .field(
                "tasks",
                &self.tasks.try_borrow().map(|tasks| tasks.len()).ok(),
            )
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}

impl TaskToken {
    /// A token for a task that was never admitted: already cancelled, and
    /// reaching no store.
    fn refused() -> Self {
        Self {
            id: 0,
            flags: Arc::new(TaskFlags::closed()),
            store: RcWeak::new(),
        }
    }

    /// The driver-unique id of the task, for diagnostics. `0` for a task the
    /// driver refused because its UI runtime was gone.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether the task has been cancelled: explicitly, by its UI runtime's
    /// teardown, or because its driver's UI runtime was already gone when it was
    /// spawned.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flags.cancelled.load(Ordering::Acquire)
    }

    /// Cancel now rather than at drop. Idempotent.
    ///
    /// The removed future is dropped only once the task map's borrow has been
    /// released. A future's destructor is user code: it may cancel another
    /// task from this same driver (a nested [`TaskToken`]), spawn cleanup
    /// work, or wake a sibling.
    ///
    /// # Panics
    ///
    /// Propagates a panic raised by the removed future's own destructor —
    /// called directly, this is an ordinary panic with an ordinary
    /// backtrace. During an existing unwind this type's `Drop` detaches and
    /// retains the future instead of running its destructor.
    pub fn cancel(&self) {
        self.flags.cancelled.store(true, Ordering::Release);
        if let Some(store) = self.store.upgrade() {
            // If the task is mid-poll its slot holds `None`, and the pump
            // honours `cancelled` when it tries to re-queue.
            let removed = store.remove(self.id);
            crate::scheduler::execution::retire_with_execution_custody(
                &store.execution_failure,
                removed,
            );
        }
    }
}

impl Drop for TaskToken {
    /// Cancels the task, same as an explicit [`cancel`](Self::cancel) call —
    /// except while the thread is already unwinding.
    ///
    /// During an existing unwind even catching the future's Drop cannot
    /// contain two of its fields panicking in succession. Detach its slot and
    /// retain the opaque future without invoking any user destruction. This
    /// also retains any resources and nested tokens it owns; cancellation of
    /// this task is guaranteed, recursive destruction of its captures is not.
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.flags.cancelled.store(true, Ordering::Release);
            if let Some(store) = self.store.upgrade()
                && let Some(mut task) = store.remove(self.id)
                && let Some(future) = task.future.take()
            {
                mem::forget(future);
            }
        } else {
            self.cancel();
        }
    }
}

/// Warm `spare` with a drained batch's own allocation (clear and reuse `buf`,
/// never drop it).
fn recycle(spare: &mut Vec<TaskId>, mut buf: Vec<TaskId>) {
    buf.clear();
    *spare = buf;
}

/// Owns one pump's whole ready-id batch until the pump returns normally.
///
/// If a poll panics, this guard's `Drop` removes the slot whose future was
/// taken for that poll and restores every id the pump has not yet reached
/// (`remaining[cursor..]`) into `ready`, so the next pump still indexes them.
/// The slot it removes holds `future: None` (taken before the poll), so the
/// removal runs no user code: only framework-owned waker and flag
/// reference-count decrements.
struct PumpGuard<'a> {
    store: &'a TaskStore,
    /// This pump's whole ready batch, sorted and deduplicated, consumed
    /// front-to-back via `cursor`.
    remaining: Vec<TaskId>,
    /// The index of the next (not yet polled) id.
    cursor: usize,
    /// The id whose future is between "taken out of its slot" and "outcome
    /// applied", if any.
    in_flight: Option<TaskId>,
    /// Set just before the pump's normal return.
    done: bool,
}

impl Drop for PumpGuard<'_> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Some(id) = self.in_flight.take() {
            drop(self.store.remove(id));
        }
        let mut tail = self.remaining.split_off(self.cursor);
        recycle(
            &mut self.store.spare.borrow_mut(),
            mem::take(&mut self.remaining),
        );
        self.store.shared.ready.lock().append(&mut tail);
    }
}

/// A widget's handle to its UI runtime's frame-driven tasks.
///
/// Acquired in `init_state` through `LifecycleContext::async_driver`. Cheap to
/// clone; every clone reaches the same task set. Owner-local: it may spawn
/// futures holding `Rc` state, so it never leaves the owner thread. A task
/// that must be completed from another thread is woken through its
/// [`Waker`]; a worker that needs a frame for any other reason uses a
/// [`FrameWaker`](crate::FrameWaker).
///
/// A `Weak` handle: the UI runtime's [`OwnerFrame`](crate::OwnerFrame) owns the
/// tasks, so a handle that outlives its UI runtime holds nothing, and spawning
/// through it drops the future at once.
#[derive(Clone)]
pub struct AsyncDriver {
    store: RcWeak<TaskStore>,
}

impl AsyncDriver {
    pub(crate) fn new(store: &Rc<TaskStore>) -> Self {
        Self {
            store: Rc::downgrade(store),
        }
    }

    /// Replace the "request a frame" hook. A test probe: production installs
    /// the hook only through [`OwnerFrame::new`](crate::OwnerFrame::new), so a
    /// widget cannot replace its UI runtime's frame hook.
    ///
    /// Installation retries undelivered demand. The displaced `Arc` is
    /// dropped only after the hook lock is released, since its captured state
    /// is user code that may re-enter the driver. Once the UI runtime is gone or
    /// retired the hook is refused and dropped at once (retained instead
    /// during an existing unwind).
    ///
    /// # Panics
    ///
    /// Propagates a panic from a refused hook's destructor.
    #[cfg(any(test, feature = "testing"))]
    #[doc(hidden)]
    pub fn set_request_frame<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        match self.store.upgrade() {
            Some(store) => store.set_request_frame(Arc::new(hook)),
            None => {
                if let Err(payload) = release_opaque(hook) {
                    resume_unwind(payload);
                }
            }
        }
    }

    /// Queue `future` for polling on the owner thread, and request a frame.
    ///
    /// The task starts ready, so the **next** frame polls it — an
    /// already-complete future finishes on that frame without a wake.
    ///
    /// Dropping the returned [`TaskToken`] cancels the task.
    ///
    /// If this handle's UI runtime is gone, the future is dropped here, at once,
    /// and the returned token is already cancelled.
    ///
    /// # Panics
    ///
    /// Panics when this driver's task counter exhausts identities `1..u64::MAX`.
    /// `u64::MAX` is reserved for permanent exhaustion, shared by both spawn
    /// methods and all driver clones. Catching the panic or removing tasks never
    /// permits another admission. On exhausted admission, the rejected future is
    /// retained without polling it or running its destructor, preserving the
    /// capacity failure. Accepted futures retain their ordinary lifetime policy.
    ///
    /// Propagates a panic from a refused future's destructor.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local(&self, future: BoxedTask) -> TaskToken {
        match self.store.upgrade() {
            Some(store) if !store.closed.get() => store.spawn(future),
            Some(store) => refuse(future, Some(&store.execution_failure)),
            None => refuse(future, None),
        }
    }

    /// Spawn `future` and poll it **once, inline, right now**.
    ///
    /// Returns `None` when that first poll completed the future — nothing is
    /// queued and no frame is requested. Returns `Some(token)` when it is
    /// pending, in which case the task is queued exactly as
    /// [`spawn_local`](Self::spawn_local) would have left it after its first
    /// frame.
    ///
    /// # Why this exists
    ///
    /// A future builder subscribes to its future in `init_state`; an
    /// already-complete future must run its completion **inline** there, so it
    /// never shows a `waiting` state. `spawn_local` cannot reproduce it: a
    /// subscription is created in `ViewState::init_state`, which runs inside
    /// `build_scope`, and the frame's driver step already ran *before*
    /// `build_scope`.
    ///
    /// The inline poll runs user code during the build phase, deliberately: a
    /// single task polled at its own subscription point, not the frame's
    /// driver step.
    ///
    /// If this handle's UI runtime is gone, the future is dropped unpolled and the
    /// returned token is already cancelled.
    ///
    /// # Panics
    ///
    /// As [`spawn_local`](Self::spawn_local), plus a panic from the inline poll.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local_eager(&self, future: BoxedTask) -> Option<TaskToken> {
        match self.store.upgrade() {
            Some(store) if !store.closed.get() => store.spawn_eager(future),
            Some(store) => Some(refuse(future, Some(&store.execution_failure))),
            None => Some(refuse(future, None)),
        }
    }

    /// Number of tasks the UI runtime is holding; `0` once the UI runtime is gone.
    ///
    /// A count, never a guard.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.store
            .upgrade()
            .map_or(0, |store| store.pending_task_count())
    }

    /// Diagnostic: `true` if none of this driver's locks or borrows is
    /// currently held — never blocks, and never exposes a guard.
    ///
    /// Backs the lock-discipline oracles that prove a callback calling
    /// `spawn_local`/`spawn_local_eager` from inside another callback does not
    /// deadlock or hit a `BorrowMutError`. Gated behind the `testing` feature
    /// (on by default in this crate's own test builds) so a downstream crate's
    /// reentrancy test can reach it through a dev-dependency edge.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.store.upgrade().is_none_or(|store| store.is_unlocked())
    }
}

/// Release an opaque owned value — a future or a hook, whose drop glue is
/// user code — under the retention policy shared by cancellation and
/// retirement: during an existing unwind it is retained (forgotten), since a
/// destructor panicking then aborts the process; otherwise it is dropped
/// under a catch and the panic, if any, handed back.
fn release_opaque<T>(value: T) -> Result<(), RetirePanic> {
    if std::thread::panicking() {
        mem::forget(value);
        return Ok(());
    }
    catch_unwind(AssertUnwindSafe(move || drop(value)))
}

/// Release a future a dead driver cannot admit, on the calling (owner)
/// thread, and hand back a cancelled token.
///
/// Ownership is made safe first ([`release_opaque`]); the diagnostic runs
/// afterwards under its own catch, so a panicking subscriber can neither
/// abort an unwind nor replace the destructor's panic. A diagnostic panic is
/// retained, never raised.
fn refuse(
    future: BoxedTask,
    failure_slot: Option<&RefCell<Option<RcWeak<Cell<bool>>>>>,
) -> TaskToken {
    let token = TaskToken::refused();
    let released = if let Some(slot) = failure_slot {
        catch_unwind(AssertUnwindSafe(|| {
            crate::scheduler::execution::retire_with_execution_custody(slot, future);
        }))
    } else {
        release_opaque(future)
    };
    let diagnostic = catch_unwind(|| {
        tracing::warn!(
            "AsyncDriver: the ui_runtime that owned this driver is gone; dropping the spawned future"
        );
    });
    if let Err(payload) = diagnostic {
        flui_foundation::panic::retain_opaque_payload(payload);
    }
    if let Err(payload) = released {
        resume_unwind(payload);
    }
    token
}

impl std::fmt::Debug for AsyncDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let store = self.store.upgrade();
        f.debug_struct("AsyncDriver")
            .field("alive", &store.is_some())
            .field(
                "tasks",
                &store.and_then(|store| store.tasks.try_borrow().map(|tasks| tasks.len()).ok()),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OwnerFrame, UpdateScheduler};
    use std::sync::atomic::AtomicUsize;

    /// A future that reports `Pending` until `ready` flips, recording polls.
    struct Controlled {
        polls: Arc<AtomicUsize>,
        finish: Arc<AtomicBool>,
        waker: Arc<Mutex<Option<Waker>>>,
    }

    impl Future for Controlled {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            self.polls.fetch_add(1, Ordering::Relaxed);
            if self.finish.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                let _prev = self.waker.lock().replace(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    /// `(future, poll counter, finish flag, stored waker)`.
    type ControlledParts = (
        Controlled,
        Arc<AtomicUsize>,
        Arc<AtomicBool>,
        Arc<Mutex<Option<Waker>>>,
    );

    fn controlled() -> ControlledParts {
        let polls = Arc::new(AtomicUsize::new(0));
        let finish = Arc::new(AtomicBool::new(false));
        let waker = Arc::new(Mutex::new(None));
        (
            Controlled {
                polls: Arc::clone(&polls),
                finish: Arc::clone(&finish),
                waker: Arc::clone(&waker),
            },
            polls,
            finish,
            waker,
        )
    }

    /// A fresh scheduler and the owner frame that holds its tasks.
    fn owner_frame() -> (UpdateScheduler, OwnerFrame) {
        let scheduler = UpdateScheduler::new();
        let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
        (scheduler, frame)
    }

    fn exhausted_task_ids_preserve_cancellation_and_progress() {
        struct Rejected {
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        }
        impl Future for Rejected {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
                self.polls.fetch_add(1, Ordering::Relaxed);
                Poll::Ready(())
            }
        }
        impl Drop for Rejected {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }

        for eager_first in [false, true] {
            let scheduler = UpdateScheduler::new();
            let frame = OwnerFrame::with_first_task_id(&scheduler, u64::MAX - 2)
                .expect("the scheduler has no live owner frame");
            let driver = frame.async_driver();
            let alias = driver.clone();
            let mut accepted = Vec::new();
            for eager in [eager_first, !eager_first] {
                let (future, polls, finish, waker) = controlled();
                let token = if eager {
                    let token = driver
                        .spawn_local_eager(Box::pin(future))
                        .expect("controlled eager future remains pending");
                    waker
                        .lock()
                        .as_ref()
                        .expect("inline poll stores waker")
                        .wake_by_ref();
                    token
                } else {
                    driver.spawn_local(Box::pin(future))
                };
                accepted.push((token, polls, finish, waker));
            }
            assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 2);
            assert_eq!(driver.pending_task_count(), 2);

            let rejected_polls = Arc::new(AtomicUsize::new(0));
            let rejected_drops = Arc::new(AtomicUsize::new(0));
            for _ in 0..4 {
                for eager in [false, true] {
                    let future = Box::pin(Rejected {
                        polls: Arc::clone(&rejected_polls),
                        drops: Arc::clone(&rejected_drops),
                    });
                    let refusal = catch_unwind(AssertUnwindSafe(|| {
                        if eager {
                            alias.spawn_local_eager(future)
                        } else {
                            Some(alias.spawn_local(future))
                        }
                    }))
                    .expect_err("exhausted driver must refuse both task admission paths");
                    assert_eq!(
                        refusal.downcast_ref::<&str>().copied(),
                        Some("AsyncDriver task identities exhausted; refusing identity reuse")
                    );
                    assert_eq!(driver.pending_task_count(), 2);
                    assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 0);
                }
            }
            assert_eq!(rejected_polls.load(Ordering::Relaxed), 0);
            assert_eq!(rejected_drops.load(Ordering::Relaxed), 0);

            let (first, _, finish, waker) = &accepted[0];
            finish.store(true, Ordering::Release);
            waker
                .lock()
                .as_ref()
                .expect("first pending task stores waker")
                .wake_by_ref();
            assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 1);
            first.cancel();
            assert_eq!(driver.pending_task_count(), 1);
            let (_, _, finish, waker) = &accepted[1];
            finish.store(true, Ordering::Release);
            waker
                .lock()
                .as_ref()
                .expect("sibling stores waker")
                .wake_by_ref();
            assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 1);
            assert_eq!(driver.pending_task_count(), 0);
            for eager in [false, true] {
                let refusal = catch_unwind(AssertUnwindSafe(|| {
                    if eager {
                        driver.spawn_local_eager(Box::pin(async {}))
                    } else {
                        Some(driver.spawn_local(Box::pin(async {})))
                    }
                }));
                assert!(
                    refusal.is_err(),
                    "removal must not reset exhausted identities"
                );
            }
        }

        let (_scheduler, fresh) = owner_frame();
        let completed = Arc::new(AtomicBool::new(false));
        let output = Arc::clone(&completed);
        let _token = fresh.async_driver().spawn_local(Box::pin(async move {
            output.store(true, Ordering::Release);
        }));
        assert_eq!(fresh.pump_background(|| {}).expect("live owner turn"), 1);
        assert!(completed.load(Ordering::Acquire));
        assert_eq!(fresh.async_driver().pending_task_count(), 0);
    }

    /// A future that panics on its very first poll.
    struct PanicsOnPoll;

    impl Future for PanicsOnPoll {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            panic!("poll probe");
        }
    }

    /// A panic inside `future.poll` must not leave a zombie slot behind
    /// (issue #1057).
    fn async_driver_poll_panic_does_not_leave_a_zombie_slot() {
        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let before = frame.async_driver().pending_task_count();

        let _token = driver.spawn_local(Box::pin(PanicsOnPoll));
        assert_eq!(frame.async_driver().pending_task_count(), before + 1);

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            frame.pump_background(|| {}).expect("live owner turn")
        }));
        assert!(
            unwind.is_err(),
            "the panic must propagate out of poll_ready"
        );

        assert_eq!(
            frame.async_driver().pending_task_count(),
            before,
            "the panicking task's slot must be removed, not left as a zombie"
        );

        // A later poll must not touch the removed slot.
        assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 0);
        assert_eq!(frame.async_driver().pending_task_count(), before);
    }

    fn async_driver_coalesces_repeated_wakes_into_one_frame_request() {
        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let (task, _polls, _finish, waker) = controlled();
        let _token = driver.spawn_local(Box::pin(task));
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "spawn requests the frame that will poll the task"
        );

        frame.pump_background(|| {}).expect("live owner turn");
        let waker = waker.lock().clone().expect("waker stored");

        for _ in 0..5 {
            waker.wake_by_ref();
        }

        assert_eq!(
            frames.load(Ordering::Relaxed),
            2,
            "five wakes between frames must request exactly one more frame"
        );
        assert_eq!(frame.ready_task_count(), 1);

        // After a poll clears `ready`, the next wake requests again.
        frame.pump_background(|| {}).expect("live owner turn");
        waker.wake_by_ref();
        assert_eq!(frames.load(Ordering::Relaxed), 3);
    }

    /// A wake from a worker thread arms the task and requests a frame, but the
    /// future is polled only when the owner thread pumps.
    fn async_driver_wake_from_another_thread_polls_on_the_driving_thread() {
        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let polled_on = Rc::new(RefCell::new(Vec::<std::thread::ThreadId>::new()));
        let polled_on_for_task = Rc::clone(&polled_on);
        let (task, polls, finish, waker) = controlled();
        let _token = driver.spawn_local(Box::pin(async move {
            polled_on_for_task
                .borrow_mut()
                .push(std::thread::current().id());
            task.await;
        }));

        frame.pump_background(|| {}).expect("live owner turn");
        let polls_after_first = polls.load(Ordering::Relaxed);
        let waker = waker.lock().clone().expect("waker");

        finish.store(true, Ordering::Release);
        let worker = std::thread::spawn(move || {
            waker.wake_by_ref();
            std::thread::current().id()
        });
        let worker_id = worker.join().expect("worker");

        assert_eq!(
            polls.load(Ordering::Relaxed),
            polls_after_first,
            "waking must not poll on the worker thread"
        );
        assert_eq!(frame.ready_task_count(), 1);

        frame.pump_background(|| {}).expect("live owner turn");
        assert_eq!(polls.load(Ordering::Relaxed), polls_after_first + 1);

        let threads = polled_on.borrow().clone();
        assert!(
            threads.iter().all(|id| *id != worker_id),
            "the future was polled only on the driving thread"
        );
    }

    /// Tasks are polled in ascending spawn order, so a frame is reproducible.
    fn async_driver_polls_in_deterministic_spawn_order() {
        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut tokens = Vec::new();

        for index in 0..8 {
            let order = Rc::clone(&order);
            tokens.push(driver.spawn_local(Box::pin(async move {
                order.borrow_mut().push(index);
            })));
        }

        frame.pump_background(|| {}).expect("live owner turn");
        assert_eq!(*order.borrow(), (0..8).collect::<Vec<_>>());
        // Retain cancellation tokens until every task has been observed.
        drop(tokens);
    }

    /// A task woken *during* the poll is picked up next frame, not spun on.
    fn async_driver_self_wake_defers_to_the_next_frame() {
        let (_scheduler, frame) = owner_frame();
        let polls = Rc::new(Cell::new(0_usize));
        let polls_for_task = Rc::clone(&polls);

        let _token = frame
            .async_driver()
            .spawn_local(Box::pin(std::future::poll_fn(move |cx| {
                polls_for_task.set(polls_for_task.get() + 1);
                cx.waker().wake_by_ref(); // immediate self-wake
                Poll::<()>::Pending
            })));

        assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 1);
        assert_eq!(polls.get(), 1, "one poll, no spin");
        assert_eq!(frame.ready_task_count(), 1, "re-armed for the next frame");

        assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 1);
        assert_eq!(polls.get(), 2);
    }

    /// Three tasks spawn ready (ascending id); the middle one panics on its
    /// first poll. The third — never reached this pump — must still be
    /// indexed as ready after the unwind, and the very next pump must poll
    /// exactly it, with no external wake needed.
    fn panic_mid_pump_keeps_unreached_siblings_indexed() {
        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let third_polls = Rc::new(Cell::new(0_usize));
        let third_polls_for_task = Rc::clone(&third_polls);

        let _first = driver.spawn_local(Box::pin(async {}));
        let _second = driver.spawn_local(Box::pin(PanicsOnPoll));
        let _third = driver.spawn_local(Box::pin(std::future::poll_fn(move |_cx| {
            third_polls_for_task.set(third_polls_for_task.get() + 1);
            Poll::<()>::Pending
        })));

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            frame.pump_background(|| {}).expect("live owner turn")
        }));
        assert!(
            unwind.is_err(),
            "the panic must propagate out of poll_ready"
        );

        assert_eq!(
            third_polls.get(),
            0,
            "the third task must not have been reached in the aborted pump"
        );
        assert_eq!(
            frame.async_driver().pending_task_count(),
            1,
            "the completed first and the panicking second are both gone; only the third remains"
        );
        assert_eq!(
            frame.ready_task_count(),
            1,
            "the unreached third task must still be indexed as ready after the unwind"
        );

        assert_eq!(
            frame.pump_background(|| {}).expect("live owner turn"),
            1,
            "the next pump must poll exactly the stranded third task"
        );
        assert_eq!(third_polls.get(), 1);
        assert_eq!(
            frame.async_driver().pending_task_count(),
            1,
            "third stays pending, never woken again"
        );
        assert_eq!(frame.ready_task_count(), 0);
    }

    /// Called directly (not via `Drop`, and not while already unwinding),
    /// `cancel()` must propagate a panic raised by the removed future's own
    /// destructor.
    fn cancel_propagates_the_removed_futures_panic() {
        struct PanicsOnDrop;
        impl Drop for PanicsOnDrop {
            fn drop(&mut self) {
                panic!("cancel-propagation probe");
            }
        }

        let (_scheduler, frame) = owner_frame();
        let token = frame.async_driver().spawn_local(Box::pin(async move {
            let _payload = PanicsOnDrop;
            std::future::pending::<()>().await;
        }));
        // Poll once so the async block actually starts executing (and so
        // constructs `_payload`) before it is cancelled.
        assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 1);

        let unwind = catch_unwind(AssertUnwindSafe(|| token.cancel()));
        assert!(
            unwind.is_err(),
            "cancel() must propagate the removed future's destructor panic"
        );
    }

    /// A failed destructor closes failure custody; later opaque futures are
    /// retained rather than risking aggregate double-panic destruction.
    fn retirement_retains_the_tail_and_keeps_the_first_panic() {
        struct Probe {
            drops: Rc<Cell<usize>>,
            panic_with: Option<&'static str>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
                if let Some(message) = self.panic_with {
                    panic!("{message}");
                }
            }
        }

        let (_scheduler, frame) = owner_frame();
        let driver = frame.async_driver();
        let drops = Rc::new(Cell::new(0));
        let tokens: Vec<_> = [None, Some("first"), None, Some("second")]
            .into_iter()
            .map(|panic_with| {
                let probe = Probe {
                    drops: Rc::clone(&drops),
                    panic_with,
                };
                driver.spawn_local(Box::pin(async move {
                    let _probe = probe;
                    std::future::pending::<()>().await;
                }))
            })
            .collect();
        assert_eq!(frame.pump_background(|| {}).expect("live owner turn"), 4);

        let first = frame.retire().expect("a destructor panicked");
        assert_eq!(
            first.downcast_ref::<String>().map(String::as_str),
            Some("first")
        );
        assert_eq!(
            drops.get(),
            2,
            "the healthy prefix and first failing capture retire"
        );
        assert!(tokens.iter().all(TaskToken::is_cancelled));
        assert!(frame.retire().is_none(), "retirement is idempotent");
        drop(tokens);
        assert_eq!(drops.get(), 2);

        let late = driver.spawn_local(Box::pin(async {}));
        assert!(late.is_cancelled(), "a retired store admits nothing");
        assert_eq!(frame.async_driver().pending_task_count(), 0);
    }

    /// A scheduler has one live owner frame, so the owner a frame drive polls
    /// is the only one a task can be admitted to: a second owner would hold
    /// ready tasks that no frame polls and no demand is left to request.
    fn a_scheduler_has_one_live_owner_frame() {
        let (scheduler, first) = owner_frame();
        assert_eq!(
            OwnerFrame::new(&scheduler).err(),
            Some(crate::OwnerFrameError::AlreadyOwned)
        );
        let polled = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&polled);
        let _token = first.async_driver().spawn_local(Box::pin(async move {
            counter.fetch_add(1, Ordering::Relaxed);
        }));
        first
            .drive_frame(
                crate::Instant::now(),
                crate::IdleDeadline::far_future(crate::Instant::now()),
                || {},
                || {},
            )
            .expect("live owner frame");
        assert_eq!(polled.load(Ordering::Relaxed), 1, "the one owner is polled");

        let _ = first.retire();
        assert_eq!(
            OwnerFrame::new(&scheduler).err(),
            Some(crate::OwnerFrameError::AlreadyOwned),
            "a retired owner still holds the slot until it drops"
        );
        drop(first);
        let second = OwnerFrame::new(&scheduler).expect("the slot frees when the owner drops");
        let _token = second.async_driver().spawn_local(Box::pin(async {}));
        assert!(scheduler.is_frame_scheduled());
        second
            .drive_frame(
                crate::Instant::now(),
                crate::IdleDeadline::far_future(crate::Instant::now()),
                || {},
                || {},
            )
            .expect("live owner frame");
        assert_eq!(second.async_driver().pending_task_count(), 0);
    }

    #[test]
    fn async_driver_failure_and_ordering_matrix() {
        crate::table_test::run_table(
            "async_driver_failure_and_ordering_matrix",
            &[
                (
                    "exhausted_task_ids_preserve_cancellation_and_progress",
                    exhausted_task_ids_preserve_cancellation_and_progress as fn(),
                ),
                (
                    "async_driver_poll_panic_does_not_leave_a_zombie_slot",
                    async_driver_poll_panic_does_not_leave_a_zombie_slot as fn(),
                ),
                (
                    "async_driver_coalesces_repeated_wakes_into_one_frame_request",
                    async_driver_coalesces_repeated_wakes_into_one_frame_request as fn(),
                ),
                (
                    "async_driver_wake_from_another_thread_polls_on_the_driving_thread",
                    async_driver_wake_from_another_thread_polls_on_the_driving_thread as fn(),
                ),
                (
                    "async_driver_polls_in_deterministic_spawn_order",
                    async_driver_polls_in_deterministic_spawn_order as fn(),
                ),
                (
                    "async_driver_self_wake_defers_to_the_next_frame",
                    async_driver_self_wake_defers_to_the_next_frame as fn(),
                ),
                (
                    "panic_mid_pump_keeps_unreached_siblings_indexed",
                    panic_mid_pump_keeps_unreached_siblings_indexed as fn(),
                ),
                (
                    "cancel_propagates_the_removed_futures_panic",
                    cancel_propagates_the_removed_futures_panic as fn(),
                ),
                (
                    "retirement_retains_the_tail_and_keeps_the_first_panic",
                    retirement_retains_the_tail_and_keeps_the_first_panic as fn(),
                ),
                (
                    "a_scheduler_has_one_live_owner_frame",
                    a_scheduler_has_one_live_owner_frame as fn(),
                ),
            ],
        );
    }
}
