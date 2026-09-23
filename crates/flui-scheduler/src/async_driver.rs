//! [`AsyncDriver`] — the frame-driven task driver.
//!
//! # What this is
//!
//! A single-threaded executor with no runtime, no thread pool, and no
//! dependency beyond [`std::future`] / [`std::task`]. Futures are polled on the
//! **frame thread**, in the gap between a frame's transient callbacks
//! (animation ticks) and its persistent callbacks (build → layout → paint) —
//! Flutter's `SchedulerPhase.midFrameMicrotasks`.
//!
//! That is Flutter parity, not a compromise: a Dart `Future` completes on the UI
//! isolate's event loop, and `FutureBuilder`'s callbacks run there. Polling on
//! the frame thread reproduces it, and keeps an async runtime out of
//! `flui-view` / `flui-widgets` / `flui-app`.
//!
//! # Waking
//!
//! A task's [`Waker`] sets a per-task `ready` flag and — **only on the
//! `false → true` transition** — asks the binding for a frame through the
//! scheduler's existing `request_frame` hook. So a burst of wakes between frames
//! costs one flag write and one frame request, exactly as
//! `ExternalBuildScheduler` coalesces rebuild requests.
//!
//! Waking is legal from any thread. Polling is not: it happens only inside
//! [`AsyncDriver::poll_ready`], which the binding calls once per frame.
//!
//! # Readiness index
//!
//! Readiness discovery is separate from task ownership (issue #1056):
//! `TaskStore` keeps `tasks` (every live task, keyed by id) beside `ready`
//! (the ids due a poll, in wake-arrival order), so `poll_ready` on an idle
//! driver touches neither a dormant task's slot nor its atomic flag — it
//! only drains `ready`, which is empty.
//!
//! **Invariant:** `ready == true` (a task's own flag) implies its id is in
//! `store.ready` **or** in the in-flight pump's own unprocessed tail (the
//! ids `PumpGuard` has not reached yet). Every path that sets the flag
//! `true` also pushes the id, in the same locked section:
//! [`spawn_local`](AsyncDriver::spawn_local) (seeds the flag `true` and
//! pushes); `TaskWaker::wake_by_ref`'s `false → true` edge (pushes iff the
//! task is still live); [`spawn_local_eager`](AsyncDriver::spawn_local_eager)
//! (pushes iff a wake landed during the inline poll); and a pump's own
//! unreached remainder, restored by `PumpGuard::drop` on panic and consumed
//! normally on success.
//!
//! `store.ready` is **never proactively purged** on cancel, nor scrubbed for
//! an id whose task then panics — both go stale for at most one pump and
//! self-heal via `poll_ready`'s "id not found in `tasks` ⇒ skip" arm, which
//! is cheaper than an O(R) scan on every cancel. A stale entry can also be a
//! same-id **duplicate**: `spawn_local_eager`'s inline poll and a concurrent
//! `wake_by_ref` can both observe the armed flag and each push the id before
//! either sees the other's write (no pump ran between them to clear it), so
//! `poll_ready` sorts and `dedup()`s its drained batch rather than assuming
//! the flag makes a duplicate impossible.
//!
//! A same-id duplicate can also **straddle a drain**: if one push lands
//! before a pump's swap (drained and polled this pump) and the other lands
//! after (queued for the next one), the next pump finds a *live* task whose
//! own flag reads `false` — not a ghost, so it is polled rather than
//! skipped. That poll is spurious (nothing new is actually ready), but
//! harmless and self-limiting: `Future::poll` may be called at any time and
//! must handle it, and the extra entry does not recur — nothing re-pushes an
//! id no new event woke.
//!
//! Churn without an intervening pump is bounded by the spawns and wakes
//! since the last drain — one entry per event, 8 bytes each, two for an
//! eager spawn that races its own wake (the duplicate above) — and a pump
//! drains all of it. That bound is per gap between pumps, not a global
//! cap: a tight
//! `spawn_local` + immediate `cancel()` loop, with no pump running between
//! iterations, leaves one stale id per pair even though
//! `pending_task_count()` is `0` throughout — the next pump still clears
//! every one of them via the "not found" skip arm.
//!
//! # Cancellation
//!
//! [`TaskToken`] cancels on drop: the future is removed from the driver and
//! dropped, so it is never polled again and its destructors run. A `Waker` held
//! by a cancelled task is inert — it sets a flag nobody reads and finds no task
//! to poll. This is real cancellation, not Dart's "ignore the late callback".
//!
//! # What it never does
//!
//! Touch an element tree, a render tree, or a pipeline. A task that wants a
//! rebuild calls `RebuildHandle::schedule()`, which only writes to the
//! build owner's inbox.

use std::collections::BTreeMap;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, Wake, Waker};

use parking_lot::Mutex;

/// A future the driver polls on the frame thread.
///
/// `Send` because the driver is shared across threads (a worker may wake a
/// task); the future itself is only ever polled on the frame thread.
pub type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Frame-request hook: the binding's "please schedule another frame".
type RequestFrame = Arc<dyn Fn() + Send + Sync>;

/// Monotonic task id. Never reused, so a stale [`Waker`] cannot resurrect or
/// mis-target a later task.
type TaskId = u64;

/// A task's own readiness flag: `true` between a wake and the poll that
/// clears it.
///
/// Wraps `AtomicBool` instead of using one directly so `load` can be counted
/// under `#[cfg(test)]` — the discriminating oracle for
/// `an_empty_pump_touches_no_dormant_task_flags`: an O(N) filter-scan (issue
/// #1056's actual bug) calls `.load()` once per *resident* task per pump
/// regardless of readiness, while draining `store.ready` never reads a
/// dormant task's own flag to discover it — only `wake_by_ref` (arming) and
/// `poll_ready`'s clear-before-poll (consuming) touch it, neither of which
/// fires for a task nothing wakes. The allocation-oracle test cannot make
/// this same distinction: collecting zero ready ids allocates nothing
/// whether it comes from a full scan or an empty drain. The counting path
/// does not exist outside `cfg(test)`, so this costs nothing in a normal
/// build.
struct ReadyFlag(AtomicBool);

impl ReadyFlag {
    fn new(value: bool) -> Self {
        Self(AtomicBool::new(value))
    }

    fn load(&self, order: Ordering) -> bool {
        #[cfg(test)]
        READY_FLAG_LOAD_COUNT.with(|count| count.set(count.get() + 1));
        self.0.load(order)
    }

    fn store(&self, value: bool, order: Ordering) {
        self.0.store(value, order);
    }

    fn swap(&self, value: bool, order: Ordering) -> bool {
        self.0.swap(value, order)
    }
}

#[cfg(test)]
thread_local! {
    /// Counts calls to [`ReadyFlag::load`] on this thread, reset before a
    /// measured pump loop by `reset_ready_flag_load_count`. Per-thread, not
    /// process-global, for the same reason `frame_telemetry_allocation.rs`'s
    /// counting allocator is per-thread: this binary's other `#[test]`
    /// functions may run concurrently under bare `cargo test`, and a shared
    /// counter would charge their unrelated flag reads to this measurement.
    static READY_FLAG_LOAD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn ready_flag_load_count() -> usize {
    READY_FLAG_LOAD_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_ready_flag_load_count() {
    READY_FLAG_LOAD_COUNT.with(|count| count.set(0));
}

/// One live task.
struct Task {
    /// The future, absent while it is being polled (moved out so the driver's
    /// lock is not held across user code).
    future: Option<BoxedTask>,
    /// Set by the waker; cleared immediately before each poll.
    ready: Arc<ReadyFlag>,
    /// Set by [`TaskToken::drop`]. Checked after a poll returns, so a token
    /// dropped *during* a poll still cancels rather than re-queueing.
    cancelled: Arc<AtomicBool>,
}

/// Task ownership plus the independent index of which tasks are due a poll.
///
/// Separating `ready` from `tasks` (issue #1056) is what lets `poll_ready`
/// scale with ready work instead of resident tasks: draining an empty `ready`
/// touches no entry in `tasks` at all. See the module's "Readiness index"
/// doc for the invariant this pair must hold.
struct TaskStore {
    /// `BTreeMap`, not `HashMap`: a lookup/removal by id needs no ordering
    /// now that poll order comes from sorting `ready` at drain, but a
    /// `HashMap`'s hash-seed-dependent iteration is still worth avoiding
    /// wherever this map is walked directly (e.g. `pending_task_count`).
    tasks: BTreeMap<TaskId, Task>,
    /// Ids whose waker fired since the last drain, in wake-arrival order.
    /// Sorted and deduplicated once per pump, outside the lock — see
    /// [`AsyncDriver::poll_ready`].
    ready: Vec<TaskId>,
    /// An always-empty scratch buffer, ping-ponged with `ready` at the start
    /// of every pump (`mem::swap`, not `mem::take`) purely so `ready` always
    /// starts a pump already capacity-warmed. Without this second buffer,
    /// draining `ready` via `mem::take` would install a fresh, zero-capacity
    /// `Vec` in its place: fine between pumps, but a self-waking task pushes
    /// its own id back into that same zero-capacity `ready` *during* this
    /// very pump (its `wake_by_ref` fires synchronously inside `poll`), so a
    /// steady `R` would regrow `ready` from empty every single pump instead
    /// of reaching zero allocations. [`recycle`] is what warms this buffer
    /// back up for the pump after next.
    spare: Vec<TaskId>,
}

/// Shared driver state.
struct Inner {
    store: Mutex<TaskStore>,
    next_id: AtomicU64,
    request_frame: Mutex<Option<RequestFrame>>,
}

impl Inner {
    /// Ask the binding for a frame, if a hook is installed.
    ///
    /// The lock is released before the hook runs: a hook that re-enters the
    /// driver (or takes the binding's own locks) must not deadlock against us.
    fn request_frame(&self) {
        let hook = self.request_frame.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
}

/// The `Waker` payload for one task.
struct TaskWaker {
    id: TaskId,
    ready: Arc<ReadyFlag>,
    cancelled: Arc<AtomicBool>,
    /// `Weak`, because the task's future holds this waker and `Inner` holds the
    /// future — an `Arc` here would leak the whole driver.
    inner: Weak<Inner>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }

        // Coalescing: only the first wake since the last poll requests a frame.
        if !self.ready.swap(true, Ordering::AcqRel)
            && let Some(inner) = self.inner.upgrade()
        {
            // A stale waker may outlive a completed/cancelled task. In that
            // case it must not wake the event loop for work that can never
            // run, and must not index an id no task owns any more —
            // `contains_key` and the push happen in the same locked section.
            let task_is_live = {
                let mut store = inner.store.lock();
                let is_live = store.tasks.contains_key(&self.id);
                if is_live {
                    store.ready.push(self.id);
                }
                is_live
            };
            if task_is_live {
                inner.request_frame();
            }
        }
    }
}

/// Cancels its task when dropped.
///
/// Hold it for as long as the task should run. `ViewState::dispose` dropping one
/// is what stops an in-flight `FutureBuilder` subscription.
#[derive(Debug)]
pub struct TaskToken {
    id: TaskId,
    cancelled: Arc<AtomicBool>,
    inner: Weak<Inner>,
}

impl TaskToken {
    /// The driver-unique id of the task, for diagnostics.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether the task has been cancelled (by dropping a previous token is
    /// impossible — a token is unique — so this only reports explicit cancel).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Cancel now rather than at drop. Idempotent.
    ///
    /// The removed task — and so the user future it owns — is dropped only
    /// once `Inner::store`'s lock has been released. A future's destructor is user
    /// code: it may cancel another task from this same driver (a nested
    /// [`TaskToken`]), spawn cleanup work, or wake a sibling, all of which
    /// take this same lock. Never held while that runs, or a reentrant
    /// destructor deadlocks on the non-reentrant mutex.
    ///
    /// # Panics
    ///
    /// Propagates a panic raised by the removed future's own destructor —
    /// called directly, this is an ordinary panic with an ordinary
    /// backtrace. This type's `Drop` impl is the one caller that must NOT
    /// let this propagate unconditionally: see its own doc for why it
    /// contains this same panic instead, while already unwinding.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(inner) = self.inner.upgrade() {
            // Dropping the future here runs its destructors on the caller's
            // thread. If the task is mid-poll its slot holds `None`, and
            // `poll_ready` will honour `cancelled` when it tries to re-queue.
            //
            // Extracted in its own block so the lock guard (a statement
            // temporary) is released at the `}` — before the removed task,
            // now a named binding, is explicitly dropped below.
            let removed = { inner.store.lock().tasks.remove(&self.id) };
            drop(removed);
        }
    }
}

impl Drop for TaskToken {
    /// Cancels the task, same as an explicit [`cancel`](Self::cancel) call —
    /// except while the thread is already unwinding.
    ///
    /// `cancel()` drops the removed future outside this driver's lock (see
    /// its own doc), but that future's destructor is still arbitrary user
    /// code that can itself panic. Reached through an ordinary `drop()`,
    /// that panic propagates like any other — the dominant path
    /// (`ViewState::dispose` → `FutureBuilder::unsubscribe` → `dispose`)
    /// already runs inside its own `catch_unwind` one level up
    /// (`on_unmount`'s hook-panic recovery), so propagating here reports it
    /// through that same accounting rather than swallowing it a second
    /// time. Reached instead while THIS thread is already unwinding from a
    /// separate panic (`std::thread::panicking()`) — concretely, a held
    /// token dropping out of a thread-local registry during thread teardown
    /// mid-unwind (`flui-app`'s `PENDING_SECONDARY_WINDOW_OPENS`) —
    /// propagating would be a double panic, which `abort`s the process with
    /// no diagnostic for either failure. So this one case is contained and
    /// logged instead, matching std's own Drop convention — and the caught
    /// payload itself is discarded through `panic_payload::discard_panic_payload`,
    /// not dropped bare, since the payload can just as well own a type
    /// whose own `Drop` panics too.
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.cancel()))
            {
                tracing::error!(
                    task_id = self.id,
                    payload = flui_foundation::panic::payload_text(&*payload)
                        .unwrap_or("<non-string panic payload>"),
                    "TaskToken::cancel panicked while already unwinding; not re-raising"
                );
                // The payload itself may own a type whose own `Drop` panics;
                // dropping it bare here would let that second panic escape
                // uncontained -- during this already-unwinding path, the
                // exact double panic (abort, no diagnostic) this whole
                // branch exists to prevent.
                crate::panic_payload::discard_panic_payload(
                    payload,
                    "TaskToken::drop (cancel panic, traced above)",
                );
            }
        } else {
            self.cancel();
        }
    }
}

/// Warm `store.spare` with a drained batch's own allocation (clear and
/// reuse `buf`, never drop it) — see `TaskStore`'s `spare` field doc for why
/// the next pump needs this buffer already warm. Called on both
/// `poll_ready`'s normal return and [`PumpGuard::drop`]'s unwind path;
/// `store.ready` itself is untouched here, since it already holds exactly
/// the ids this pump discovered as ready for the next one.
fn recycle(store: &mut TaskStore, mut buf: Vec<TaskId>) {
    buf.clear();
    store.spare = buf;
}

/// Owns one pump's whole ready-id batch until [`AsyncDriver::poll_ready`]
/// returns normally.
///
/// `poll_ready` drains `store.ready` into this guard's `remaining` up front
/// and processes it, one id at a time, with no lock held across a poll. If a
/// poll (or a completed/cancelled task's own destructor, which runs just
/// after) panics, this guard's `Drop` restores every id the pump has not
/// yet reached (`remaining[cursor..]`) back into `store.ready`, so the next
/// pump still indexes them. An O(N)-scan design never needed this: it
/// re-derives readiness from ground truth on every call and cannot strand a
/// sibling. Owning the batch (rather than, say, re-deriving it) is what an
/// index-based design owes back in return for not scanning.
///
/// This `Drop`'s own `self.inner.store.lock()` never contends with a
/// destructor it triggered. On unwind, Rust drops the *current* scope's
/// locals first (the panicking loop iteration's `future`, waker, and
/// `cancelled`, none of which hold this lock) before propagating to
/// `poll_ready`'s own frame, where this guard (declared outside the loop)
/// finally drops last: a nested `TaskToken` a panicking future owns finishes
/// its own `cancel()` (lock acquired, then released) before this `Drop`
/// body ever runs. And the slot this removes holds `future: None` (taken
/// before the poll), so `tasks.remove` itself runs no user code: only two
/// `Arc<AtomicBool>` reference-count decrements.
struct PumpGuard<'a> {
    inner: &'a Inner,
    /// This pump's whole ready batch, sorted and deduplicated, consumed
    /// front-to-back via `cursor`.
    remaining: Vec<TaskId>,
    /// Ids already consumed from `remaining` — the index of the next
    /// (not yet polled) id, or `remaining.len()` once the pump is done.
    cursor: usize,
    /// The id whose future is between "taken out of its slot" and "outcome
    /// applied", if any. Set right before a poll and cleared right after —
    /// a completed/cancelled future's own destructor runs later, with this
    /// already `None`, since its slot's fate was already decided under lock.
    in_flight: Option<TaskId>,
    /// Set just before `poll_ready`'s normal return, so `Drop` on the happy
    /// path — the common case — does nothing.
    done: bool,
}

impl Drop for PumpGuard<'_> {
    fn drop(&mut self) {
        if self.done {
            return;
        }

        let mut store = self.inner.store.lock();
        // Only a panic *during* `future.poll` leaves a slot whose future was
        // taken but whose outcome was never applied — remove it, the same
        // zombie-slot hazard of issue #1057. A panic in a completed future's
        // own destructor runs with `in_flight` already `None`: that slot's
        // fate was already committed under lock before the destructor ran,
        // so there is nothing left here to undo for it.
        if let Some(id) = self.in_flight.take() {
            store.tasks.remove(&id);
        }
        // Every id this pump had not yet reached, `cursor` already having
        // been advanced past whichever id panicked — restore them so the
        // next pump still indexes them instead of losing them silently.
        let mut tail = self.remaining.split_off(self.cursor);
        recycle(&mut store, mem::take(&mut self.remaining));
        store.ready.append(&mut tail);
    }
}

/// A frame-driven task driver.
///
/// Cheap to clone; every clone refers to the same task set. Owned by
/// [`UpdateScheduler`](crate::UpdateScheduler), which exposes
/// [`spawn_local`](crate::UpdateScheduler::spawn_local) and
/// [`drive_async_tasks`](crate::UpdateScheduler::drive_async_tasks).
#[derive(Clone)]
pub struct AsyncDriver {
    inner: Arc<Inner>,
}

impl Default for AsyncDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncDriver {
    /// An empty driver with no frame-request hook.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                store: Mutex::new(TaskStore {
                    tasks: BTreeMap::new(),
                    ready: Vec::new(),
                    spare: Vec::new(),
                }),
                next_id: AtomicU64::new(1),
                request_frame: Mutex::new(None),
            }),
        }
    }

    /// Install the binding's "request a frame" hook.
    ///
    /// Called once at wiring time. A driver with no hook still polls whenever a
    /// frame happens to run — headless tests rely on that.
    ///
    /// Nothing enforces "once": a later call replaces the hook, and the
    /// displaced `Arc` is dropped only after `request_frame`'s lock is
    /// released — the same discipline [`TaskToken::cancel`] uses — since a
    /// hook's captured state is user code that may re-enter the driver.
    pub fn set_request_frame<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let previous = { self.inner.request_frame.lock().replace(Arc::new(hook)) };
        drop(previous);
    }

    /// Queue `future` for polling on the frame thread, and request a frame.
    ///
    /// The task starts `ready`, so the **next** [`poll_ready`](Self::poll_ready)
    /// polls it — an already-complete future finishes on that frame without a
    /// wake.
    ///
    /// Dropping the returned [`TaskToken`] cancels the task.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local(&self, future: BoxedTask) -> TaskToken {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let ready = Arc::new(ReadyFlag::new(true));
        let cancelled = Arc::new(AtomicBool::new(false));

        {
            let mut store = self.inner.store.lock();
            store.tasks.insert(
                id,
                Task {
                    future: Some(future),
                    ready: Arc::clone(&ready),
                    cancelled: Arc::clone(&cancelled),
                },
            );
            // Starts ready (see this method's doc): index it immediately, in
            // the same locked section as the insert.
            store.ready.push(id);
        }

        // A freshly spawned task needs a frame to be polled in.
        self.inner.request_frame();

        TaskToken {
            id,
            cancelled,
            inner: Arc::downgrade(&self.inner),
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
    /// Flutter's `_FutureBuilderState._subscribe` calls `future.then(...)`, and a
    /// `SynchronousFuture` runs that callback **inline**, so an
    /// already-complete future never shows `ConnectionState.waiting`
    /// (`'gives expected snapshot with SynchronousFuture'`). The Rust analogue is
    /// a future that is `Ready` on its first poll.
    ///
    /// `spawn_local` cannot reproduce it: a subscription is created in
    /// `ViewState::init_state`, which runs inside `build_scope`, and the frame's
    /// driver step already ran *before* `build_scope`. The task would first be
    /// polled on the next frame, so the first build would show `Waiting`.
    ///
    /// The inline poll runs user code during the build phase — exactly as Dart's
    /// synchronous `.then` does. It does **not** go through
    /// [`UpdateScheduler::drive_async_tasks`](crate::UpdateScheduler::drive_async_tasks), and
    /// so does not trip that method's "never poll during persistent callbacks"
    /// guard: this is a single task polled at its own subscription point, not the
    /// frame's driver step.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local_eager(&self, mut future: BoxedTask) -> Option<TaskToken> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        // Starts NOT ready: we are about to poll it ourselves. A wake landing
        // during that poll flips this to `true` (and requests a frame), so the
        // task is correctly re-armed when we queue it below.
        let ready = Arc::new(ReadyFlag::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        let waker = Waker::from(Arc::new(TaskWaker {
            id,
            ready: Arc::clone(&ready),
            cancelled: Arc::clone(&cancelled),
            inner: Arc::downgrade(&self.inner),
        }));
        let mut cx = Context::from_waker(&waker);

        if future.as_mut().poll(&mut cx).is_ready() {
            return None;
        }

        // A wake that landed *during* the inline poll found no task in the map
        // (we insert only after polling), so `wake_by_ref`'s stale-waker guard
        // suppressed its frame request AND its index push. Insert and — iff a
        // wake already armed it — index it, in the same locked section, then
        // request the frame outside the lock the same way `spawn_local` does.
        let armed = {
            let mut store = self.inner.store.lock();
            store.tasks.insert(
                id,
                Task {
                    future: Some(future),
                    ready: Arc::clone(&ready),
                    cancelled: Arc::clone(&cancelled),
                },
            );
            let armed = ready.load(Ordering::Acquire);
            if armed {
                store.ready.push(id);
            }
            armed
        };

        if armed {
            self.inner.request_frame();
        }

        Some(TaskToken {
            id,
            cancelled,
            inner: Arc::downgrade(&self.inner),
        })
    }

    /// Poll every task whose waker fired since the last frame.
    ///
    /// Called exactly once per frame, from the binding's async-driver step
    /// ([`UpdateScheduler::drive_async_tasks`](crate::UpdateScheduler::drive_async_tasks)).
    /// Never call it from build, layout, or paint.
    ///
    /// Returns the number of tasks polled. Tasks are polled in ascending id
    /// order; a task that completes or is cancelled is removed. A task woken
    /// *during* this call is left `ready` and picked up next frame — the driver
    /// never spins. Cost scales with **ready** tasks, not resident ones
    /// (issue #1056): an idle driver drains an empty index and touches no
    /// dormant task at all.
    pub fn poll_ready(&self) -> usize {
        // Drain the ready index, then release the lock: a task's `poll` may
        // spawn, cancel, or wake — all of which take this lock. Swap with
        // `spare`, not `mem::take` `ready` directly — see `TaskStore`'s
        // `spare` field doc for why the swap matters.
        let mut remaining: Vec<TaskId> = {
            let mut guard = self.inner.store.lock();
            // One `DerefMut` first: two separate `&mut store.field` calls
            // through the `MutexGuard`'s `deref_mut` are each their own
            // opaque borrow, which the compiler cannot prove disjoint.
            let store: &mut TaskStore = &mut guard;
            mem::swap(&mut store.ready, &mut store.spare);
            mem::take(&mut store.spare)
        };

        // Outside the lock: exclusively this thread's data now, so an
        // O(R log R) sort touches no shared state. Ascending order keeps
        // polling deterministic (module doc); `dedup` (adjacent-only, hence
        // the sort first) collapses the rare same-id double push a
        // `spawn_local_eager` inline poll can race against a concurrent
        // `wake_by_ref` before any pump has run to clear the flag between
        // them — without it, the second entry would poll an already-handled
        // task a second time in the same pump.
        remaining.sort_unstable();
        remaining.dedup();

        let mut guard = PumpGuard {
            inner: &self.inner,
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
            // where a panic below unwinds from (`PumpGuard::drop` relies on
            // this to restore exactly the unreached tail).
            guard.cursor += 1;

            // Take the future out so no lock is held across user code.
            let Some((mut future, ready, cancelled)) = ({
                let mut store = self.inner.store.lock();
                store.tasks.get_mut(&id).and_then(|task| {
                    // Clear BEFORE polling: a wake landing during the poll must
                    // re-arm the task rather than be swallowed.
                    task.ready.store(false, Ordering::Release);
                    task.future.take().map(|future| {
                        (future, Arc::clone(&task.ready), Arc::clone(&task.cancelled))
                    })
                })
            }) else {
                // Stale id: cancelled, or a same-pump duplicate whose first
                // occurrence already took the slot. Self-heals — nothing to
                // restore for an id that no longer owns a task.
                continue;
            };

            // Armed for the poll only: if `future.poll` panics, `PumpGuard`
            // removes this slot on unwind (issue #1057) — it already holds
            // `future: None` (taken above), so nothing else would ever
            // revisit it otherwise.
            guard.in_flight = Some(id);

            let waker = Waker::from(Arc::new(TaskWaker {
                id,
                ready,
                cancelled: Arc::clone(&cancelled),
                inner: Arc::downgrade(&self.inner),
            }));
            let mut cx = Context::from_waker(&waker);
            let outcome = future.as_mut().poll(&mut cx);
            guard.in_flight = None;
            polled += 1;

            let mut store = self.inner.store.lock();
            match outcome {
                Poll::Ready(()) => {
                    store.tasks.remove(&id);
                }
                Poll::Pending => {
                    if cancelled.load(Ordering::Acquire) {
                        // The token was dropped while we polled; honour it.
                        store.tasks.remove(&id);
                    } else if let Some(task) = store.tasks.get_mut(&id) {
                        task.future = Some(future);
                    }
                    // else: `cancel()` already removed the slot; drop the future.
                }
            }
            // `store`'s lock guard, declared after `future`, drops first at
            // this iteration's end — releasing the lock — before `future`
            // itself drops (only reached on the `Ready`/cancelled arms; the
            // `Pending`-and-live arm moved it into the map). That destructor
            // is user code (it may hold a nested `TaskToken`, per #1038) and
            // must never run under this lock.
        }

        {
            let mut store = self.inner.store.lock();
            // `done` first: if anything below this line ever panicked,
            // `PumpGuard::drop` would already see `done == true` and return
            // immediately, so it can never attempt its own `split_off` on a
            // `remaining` this block is mid-consuming — unreachable by this
            // ordering, not merely because `recycle`/`mem::take` happen not
            // to panic today.
            guard.done = true;
            recycle(&mut store, mem::take(&mut guard.remaining));
        }

        polled
    }

    /// Number of tasks the driver is holding.
    ///
    /// A count, never a guard — the lock stays private.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.inner.store.lock().tasks.len()
    }

    /// Number of tasks currently indexed as due a poll.
    ///
    /// R lookups into `tasks` (O(R log N)), never a walk of it, so this
    /// still scales with ready work rather than resident tasks. Exact per
    /// *physical* index entry: an entry counts once if its task is live and
    /// armed (still present *and* that task's own flag reads `true`), so a
    /// ghost left by a cancel or a self-woken-then-panicked id contributes
    /// nothing. A same-batch duplicate from the eager inline-poll/wake race
    /// (module doc, "Readiness index") is *not* collapsed here — both
    /// entries are physically present and both reference the same live,
    /// still-armed task, so this counts it twice until the next pump's
    /// `dedup()` reduces the batch to one.
    #[must_use]
    pub fn ready_task_count(&self) -> usize {
        let store = self.inner.store.lock();
        store
            .ready
            .iter()
            .filter(|id| {
                store
                    .tasks
                    .get(id)
                    .is_some_and(|task| task.ready.load(Ordering::Acquire))
            })
            .count()
    }

    /// Diagnostic: `true` if both of this driver's locks are currently
    /// free — never blocks, and never exposes a guard (this crate
    /// hands out no lock guard from any public signature).
    ///
    /// Backs `flui-scheduler`'s own scheduler-wide lock-discipline oracle
    /// (`scheduler/lock_discipline_tests.rs`'s `assert_no_scheduler_lock_held`)
    /// so a callback that calls `spawn_local`/`spawn_local_eager` from
    /// inside another callback can be observed NOT deadlocking against this
    /// driver's own mutexes. Gated behind the `testing` feature (on by
    /// default in this crate's own test builds via `cfg(test)`) rather than
    /// unconditionally public, so a downstream crate's own reentrancy test
    /// — a nested `TaskToken` owned by a task's future, cancelled from
    /// inside that future's own destructor — can reach it too, through an
    /// explicit `dev-dependencies` edge with `features = ["testing"]`
    /// (see `crates/flui-view/Cargo.toml`), without this ever becoming part
    /// of the crate's unconditional public surface.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.inner.store.try_lock().is_some() && self.inner.request_frame.try_lock().is_some()
    }
}

impl std::fmt::Debug for AsyncDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock`: `{:?}` while the task map is held (e.g. instrumenting a
        // poll) must not deadlock — same discipline as `ExternalBuildScheduler`.
        f.debug_struct("AsyncDriver")
            .field(
                "tasks",
                &self.inner.store.try_lock().map(|store| store.tasks.len()),
            )
            .field(
                "has_request_frame",
                &self
                    .inner
                    .request_frame
                    .try_lock()
                    .map(|hook| hook.is_some()),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::task::Poll;

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

    // ── 1. ready future completes on the next poll ──────────────────────────

    #[test]
    fn async_driver_polls_and_completes_a_ready_future() {
        let driver = AsyncDriver::new();
        let done = Arc::new(AtomicBool::new(false));
        let done_for_task = Arc::clone(&done);

        let _token = driver.spawn_local(Box::pin(async move {
            done_for_task.store(true, Ordering::Release);
        }));

        assert_eq!(driver.pending_task_count(), 1, "queued, not yet polled");
        assert!(!done.load(Ordering::Acquire), "spawn must not poll inline");

        assert_eq!(driver.poll_ready(), 1);
        assert!(done.load(Ordering::Acquire));
        assert_eq!(driver.pending_task_count(), 0, "completed task is removed");
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
    /// (issue #1057): the slot's `future` field is already `None` (taken
    /// before polling) and nothing else on the normal path ever revisits
    /// it, so without the guard `pending_task_count` would count this task
    /// forever, and a later `poll_ready` would silently skip over it
    /// (not `ready`, so never selected) rather than ever making progress
    /// or erroring.
    #[test]
    fn async_driver_poll_panic_does_not_leave_a_zombie_slot() {
        let driver = AsyncDriver::new();
        let before = driver.pending_task_count();

        let _token = driver.spawn_local(Box::pin(PanicsOnPoll));
        assert_eq!(driver.pending_task_count(), before + 1);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.poll_ready()));
        assert!(
            unwind.is_err(),
            "the panic must propagate out of poll_ready"
        );

        assert_eq!(
            driver.pending_task_count(),
            before,
            "the panicking task's slot must be removed, not left as a zombie"
        );

        // A later poll must not touch the removed slot.
        assert_eq!(driver.poll_ready(), 0);
        assert_eq!(driver.pending_task_count(), before);
    }

    /// A pending task is re-queued and not re-polled until woken.
    #[test]
    fn async_driver_does_not_repoll_a_pending_task_until_it_is_woken() {
        let driver = AsyncDriver::new();
        let (task, polls, finish, waker) = controlled();
        let _token = driver.spawn_local(Box::pin(task));

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 1);

        // No wake ⇒ no poll.
        assert_eq!(driver.poll_ready(), 0);
        assert_eq!(polls.load(Ordering::Relaxed), 1);

        finish.store(true, Ordering::Release);
        waker.lock().as_ref().expect("waker stored").wake_by_ref();

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 2);
        assert_eq!(driver.pending_task_count(), 0);
    }

    // ── eager spawn (the SynchronousFuture window) ──────────────────────────

    /// An already-ready future completes on the inline poll: nothing is queued,
    /// no token, and the driver never sees it. This is what lets a
    /// `FutureBuilder` skip `Waiting` for a synchronously-complete future.
    #[test]
    fn async_driver_spawn_eager_completes_a_ready_future_inline() {
        let driver = AsyncDriver::new();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let done = Arc::new(AtomicBool::new(false));
        let done_for_task = Arc::clone(&done);
        let token = driver.spawn_local_eager(Box::pin(async move {
            done_for_task.store(true, Ordering::Release);
        }));

        assert!(token.is_none(), "a ready future needs no token");
        assert!(
            done.load(Ordering::Acquire),
            "completed inline, at spawn time"
        );
        assert_eq!(driver.pending_task_count(), 0);
        assert_eq!(
            frames.load(Ordering::Relaxed),
            0,
            "an inline completion requests no frame"
        );
        assert_eq!(driver.poll_ready(), 0);
    }

    /// A pending future is queued after the inline poll, and is NOT re-polled
    /// until woken — the inline poll counts as its first poll.
    #[test]
    fn async_driver_spawn_eager_queues_a_pending_future_already_polled_once() {
        let driver = AsyncDriver::new();
        let (task, polls, finish, waker) = controlled();
        let token = driver.spawn_local_eager(Box::pin(task));

        assert!(token.is_some());
        assert_eq!(polls.load(Ordering::Relaxed), 1, "polled once, inline");
        assert_eq!(driver.pending_task_count(), 1);
        assert_eq!(driver.ready_task_count(), 0, "not armed: no wake yet");

        assert_eq!(driver.poll_ready(), 0, "no wake ⇒ no poll");
        assert_eq!(polls.load(Ordering::Relaxed), 1);

        finish.store(true, Ordering::Release);
        waker.lock().as_ref().expect("waker").wake_by_ref();
        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 2);
        assert_eq!(driver.pending_task_count(), 0);
    }

    /// A wake landing *during* the inline poll finds no task in the map, so the
    /// stale-waker guard suppresses its frame request. `spawn_local_eager` must
    /// re-request once the task is live, or the armed task would wait forever.
    #[test]
    fn async_driver_spawn_eager_requests_a_frame_for_a_wake_during_the_inline_poll() {
        let driver = AsyncDriver::new();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_task = Arc::clone(&polls);
        let token = driver.spawn_local_eager(Box::pin(std::future::poll_fn(move |cx| {
            // Self-wake on the very first (inline) poll, then stay pending once.
            if polls_for_task.fetch_add(1, Ordering::Relaxed) == 0 {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(())
        })));

        assert!(token.is_some());
        assert_eq!(driver.ready_task_count(), 1, "armed by the inline wake");
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "the frame request must be re-issued once the task is live"
        );

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(driver.pending_task_count(), 0);
    }

    /// Dropping an eager token cancels, exactly like `spawn_local`'s.
    #[test]
    fn async_driver_spawn_eager_token_cancels_on_drop() {
        let driver = AsyncDriver::new();
        let (task, polls, _finish, _waker) = controlled();
        let token = driver.spawn_local_eager(Box::pin(task)).expect("pending");

        drop(token);
        assert_eq!(driver.pending_task_count(), 0);
        assert_eq!(driver.poll_ready(), 0);
        assert_eq!(polls.load(Ordering::Relaxed), 1, "only the inline poll");
    }

    // ── 2. wake coalescing + frame requests ─────────────────────────────────

    #[test]
    fn async_driver_coalesces_repeated_wakes_into_one_frame_request() {
        let driver = AsyncDriver::new();
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

        driver.poll_ready();
        let waker = waker.lock().clone().expect("waker stored");

        for _ in 0..5 {
            waker.wake_by_ref();
        }

        assert_eq!(
            frames.load(Ordering::Relaxed),
            2,
            "five wakes between frames must request exactly one more frame"
        );
        assert_eq!(driver.ready_task_count(), 1);

        // After a poll clears `ready`, the next wake requests again.
        driver.poll_ready();
        waker.wake_by_ref();
        assert_eq!(frames.load(Ordering::Relaxed), 3);
    }

    // ── 3. cancellation ─────────────────────────────────────────────────────

    #[test]
    fn async_driver_dropping_the_token_cancels_and_never_polls_again() {
        let driver = AsyncDriver::new();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });
        let (task, polls, _finish, waker) = controlled();
        let token = driver.spawn_local(Box::pin(task));
        assert_eq!(frames.load(Ordering::Relaxed), 1, "spawn requests a frame");

        driver.poll_ready();
        assert_eq!(polls.load(Ordering::Relaxed), 1);

        drop(token);
        assert_eq!(driver.pending_task_count(), 0, "the future is dropped");

        // A waker held by the cancelled task is inert.
        waker.lock().as_ref().expect("waker").wake_by_ref();
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a stale waker for a cancelled task must not request another frame"
        );
        assert_eq!(driver.poll_ready(), 0);
        assert_eq!(polls.load(Ordering::Relaxed), 1, "never polled again");
    }

    /// A waker held after a task completed is inert too: no future remains, so
    /// waking it must not request a useless frame.
    #[test]
    fn async_driver_waker_after_completion_is_inert() {
        let driver = AsyncDriver::new();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let stored = Arc::new(Mutex::new(None::<Waker>));
        let stored_for_task = Arc::clone(&stored);
        let _token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            let _prev = stored_for_task.lock().replace(cx.waker().clone());
            Poll::Ready(())
        })));
        assert_eq!(frames.load(Ordering::Relaxed), 1, "spawn requests a frame");

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(driver.pending_task_count(), 0);

        stored.lock().as_ref().expect("waker").wake_by_ref();
        assert_eq!(
            frames.load(Ordering::Relaxed),
            1,
            "a stale waker for a completed task must not request another frame"
        );
        assert_eq!(driver.poll_ready(), 0);
    }

    /// The future's destructor runs at cancellation — real cancellation, not
    /// "ignore the late callback".
    #[test]
    fn async_driver_cancellation_drops_the_future() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let driver = AsyncDriver::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(Arc::clone(&dropped));

        let token = driver.spawn_local(Box::pin(async move {
            let _flag = flag;
            std::future::pending::<()>().await;
        }));
        driver.poll_ready();
        assert!(!dropped.load(Ordering::Acquire));

        drop(token);
        assert!(dropped.load(Ordering::Acquire), "future dropped on cancel");
    }

    #[test]
    fn async_driver_explicit_cancel_is_idempotent() {
        let driver = AsyncDriver::new();
        let (task, _polls, _finish, _waker) = controlled();
        let token = driver.spawn_local(Box::pin(task));

        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
        assert_eq!(driver.pending_task_count(), 0);
        drop(token); // must not panic
    }

    // ── 4. cross-thread wake ────────────────────────────────────────────────

    /// A wake from a worker thread arms the task and requests a frame, but the
    /// future is polled only when the frame thread calls `poll_ready`.
    #[test]
    fn async_driver_wake_from_another_thread_polls_on_the_driving_thread() {
        let driver = AsyncDriver::new();
        let frames = Arc::new(AtomicUsize::new(0));
        let frames_for_hook = Arc::clone(&frames);
        driver.set_request_frame(move || {
            frames_for_hook.fetch_add(1, Ordering::Relaxed);
        });

        let polled_on = Arc::new(Mutex::new(Vec::<std::thread::ThreadId>::new()));
        let polled_on_for_task = Arc::clone(&polled_on);
        let (task, polls, finish, waker) = controlled();
        let _token = driver.spawn_local(Box::pin(async move {
            polled_on_for_task.lock().push(std::thread::current().id());
            task.await;
        }));

        driver.poll_ready();
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
        assert_eq!(driver.ready_task_count(), 1);

        driver.poll_ready();
        assert_eq!(polls.load(Ordering::Relaxed), polls_after_first + 1);

        let threads = polled_on.lock().clone();
        assert!(
            threads.iter().all(|id| *id != worker_id),
            "the future was polled only on the driving thread"
        );
    }

    // ── determinism / re-entrancy ───────────────────────────────────────────

    /// Tasks are polled in ascending spawn order, so a frame is reproducible.
    #[test]
    fn async_driver_polls_in_deterministic_spawn_order() {
        let driver = AsyncDriver::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tokens = Vec::new();

        for index in 0..8 {
            let order = Arc::clone(&order);
            tokens.push(driver.spawn_local(Box::pin(async move {
                order.lock().push(index);
            })));
        }

        driver.poll_ready();
        assert_eq!(*order.lock(), (0..8).collect::<Vec<_>>());
    }

    /// A task woken *during* the poll is picked up next frame, not spun on.
    #[test]
    fn async_driver_self_wake_defers_to_the_next_frame() {
        let driver = AsyncDriver::new();
        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_task = Arc::clone(&polls);

        let _token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            polls_for_task.fetch_add(1, Ordering::Relaxed);
            cx.waker().wake_by_ref(); // immediate self-wake
            Poll::Pending
        })));

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 1, "one poll, no spin");
        assert_eq!(driver.ready_task_count(), 1, "re-armed for the next frame");

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 2);
    }

    /// A task may spawn another task without deadlocking the driver's lock
    /// (`poll_ready` releases it across user code).
    #[test]
    fn async_driver_task_may_spawn_during_poll() {
        let driver = AsyncDriver::new();
        let inner = driver.clone();
        let spawned = Arc::new(AtomicBool::new(false));
        let spawned_for_task = Arc::clone(&spawned);
        // The child's token must outlive the parent, or dropping it at the end
        // of the parent's body would cancel the child before it ever ran.
        let child_token: Arc<Mutex<Option<TaskToken>>> = Arc::new(Mutex::new(None));
        let child_token_for_task = Arc::clone(&child_token);

        let _parent = driver.spawn_local(Box::pin(async move {
            let token = inner.spawn_local(Box::pin(async move {
                spawned_for_task.store(true, Ordering::Release);
            }));
            let _prev = child_token_for_task.lock().replace(token);
        }));

        driver.poll_ready(); // parent runs, spawns child
        assert!(child_token.lock().is_some(), "child was spawned");
        assert!(!spawned.load(Ordering::Acquire), "child not polled yet");

        driver.poll_ready(); // child runs
        assert!(spawned.load(Ordering::Acquire));
    }

    // ── readiness index (issue #1056) ───────────────────────────────────────

    /// The stranding fix this rewrite exists for: three tasks spawn ready
    /// (ascending id); the middle one panics on its first poll. The third —
    /// never reached this pump — must still be indexed as ready after the
    /// unwind, and the very next pump must poll exactly it, with no
    /// external wake needed. `tests/frame_panic_recovery.rs`'s
    /// `async_future_poll_panic_closes_the_frame` is this same fixture
    /// end-to-end, through `UpdateScheduler`; this is the driver-level unit.
    #[test]
    fn panic_mid_pump_keeps_unreached_siblings_indexed() {
        let driver = AsyncDriver::new();
        let third_polls = Arc::new(AtomicUsize::new(0));
        let third_polls_for_task = Arc::clone(&third_polls);

        let _first = driver.spawn_local(Box::pin(async {}));
        let _second = driver.spawn_local(Box::pin(PanicsOnPoll));
        let _third = driver.spawn_local(Box::pin(std::future::poll_fn(move |_cx| {
            third_polls_for_task.fetch_add(1, Ordering::Relaxed);
            Poll::<()>::Pending
        })));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.poll_ready()));
        assert!(
            unwind.is_err(),
            "the panic must propagate out of poll_ready"
        );

        assert_eq!(
            third_polls.load(Ordering::Relaxed),
            0,
            "the third task must not have been reached in the aborted pump"
        );
        assert_eq!(
            driver.pending_task_count(),
            1,
            "the completed first and the panicking second are both gone; only the third remains"
        );
        assert_eq!(
            driver.ready_task_count(),
            1,
            "the unreached third task must still be indexed as ready after the unwind"
        );

        assert_eq!(
            driver.poll_ready(),
            1,
            "the next pump must poll exactly the stranded third task"
        );
        assert_eq!(third_polls.load(Ordering::Relaxed), 1);
        assert_eq!(
            driver.pending_task_count(),
            1,
            "third stays pending, never woken again"
        );
        assert_eq!(driver.ready_task_count(), 0);
    }

    /// A panic in a **completed** future's own destructor is a second,
    /// distinct unwind site from a mid-poll panic —
    /// it runs at the loop body's closing brace, *after* `PumpGuard`'s
    /// `in_flight` has already been cleared and the `Ready` outcome already
    /// committed under lock. An `in_flight`-gated `Drop` sees `in_flight ==
    /// None` here and restores nothing, stranding the third task through
    /// this other door — the same tail-restore must fire regardless of
    /// which of the two sites panicked.
    #[test]
    fn panic_in_a_completed_futures_destructor_keeps_unreached_siblings_indexed() {
        struct PanicsOnDrop;
        impl Drop for PanicsOnDrop {
            fn drop(&mut self) {
                panic!("destructor probe");
            }
        }

        /// Completes on its very first poll; only the struct's own drop glue
        /// (running when the whole boxed future is finally deallocated, not
        /// during `poll`) panics.
        struct ReadyThenPanicsOnDrop {
            _payload: PanicsOnDrop,
        }
        impl Future for ReadyThenPanicsOnDrop {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
                Poll::Ready(())
            }
        }

        let driver = AsyncDriver::new();
        let third_polls = Arc::new(AtomicUsize::new(0));
        let third_polls_for_task = Arc::clone(&third_polls);

        let _first = driver.spawn_local(Box::pin(async {}));
        let _second = driver.spawn_local(Box::pin(ReadyThenPanicsOnDrop {
            _payload: PanicsOnDrop,
        }));
        let _third = driver.spawn_local(Box::pin(std::future::poll_fn(move |_cx| {
            third_polls_for_task.fetch_add(1, Ordering::Relaxed);
            Poll::<()>::Pending
        })));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.poll_ready()));
        assert!(unwind.is_err(), "the destructor panic must propagate");

        assert_eq!(
            third_polls.load(Ordering::Relaxed),
            0,
            "the third task must not have been reached in the aborted pump"
        );
        assert_eq!(
            driver.ready_task_count(),
            1,
            "the unreached third task must still be indexed after the unwind"
        );

        assert_eq!(
            driver.poll_ready(),
            1,
            "the next pump must poll exactly the stranded third task"
        );
        assert_eq!(third_polls.load(Ordering::Relaxed), 1);
    }

    /// Sorting the drained batch — not trusting wake-arrival order — is what
    /// keeps polling deterministic now that readiness is discovered from an
    /// index rather than a full scan: waking five tasks in descending id
    /// order must still poll them ascending.
    #[test]
    fn poll_ready_visits_ready_ids_ascending_even_when_woken_in_reverse() {
        let driver = AsyncDriver::new();
        let order: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let mut tokens = Vec::new();
        let mut wakers = Vec::new();

        for index in 0..5u32 {
            let order_for_task = Arc::clone(&order);
            let waker_slot: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
            let waker_slot_for_task = Arc::clone(&waker_slot);
            tokens.push(driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
                order_for_task.lock().push(index);
                let _prev = waker_slot_for_task.lock().replace(cx.waker().clone());
                Poll::<()>::Pending
            }))));
            wakers.push(waker_slot);
        }

        // First pump: all five are ready by construction (spawn seeds
        // `ready`); poll once each to stash a waker per task, then discard
        // that trivially-ascending record.
        driver.poll_ready();
        // Swap the recorded order out and drop it after the guard releases
        // (`u32` has no significant Drop, but this keeps the shape uniform
        // with every other lock site here: nothing drops while its own
        // guard is still held).
        let discarded_order = std::mem::take(&mut *order.lock());
        drop(discarded_order);

        // Wake descending: the last-spawned task's waker first.
        for waker_slot in wakers.iter().rev() {
            waker_slot
                .lock()
                .as_ref()
                .expect("waker stored")
                .wake_by_ref();
        }

        driver.poll_ready();
        assert_eq!(
            *order.lock(),
            (0..5).collect::<Vec<_>>(),
            "poll order must be ascending task id, independent of wake order"
        );
    }

    /// A task that wakes itself and then panics mid-poll pushes its own id
    /// into the ready index (the wake lands while it is still present in
    /// `tasks`) even though `PumpGuard` removes its slot on unwind — a
    /// stale, self-healing entry, not a hazard the old O(N) scan lacked
    /// (module doc, "Readiness index").
    #[test]
    fn self_wake_during_a_poll_that_then_panics_leaves_a_stale_id_that_self_heals() {
        struct SelfWakeThenPanic;
        impl Future for SelfWakeThenPanic {
            type Output = ();
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
                cx.waker().wake_by_ref();
                panic!("self-wake-then-panic probe");
            }
        }

        let driver = AsyncDriver::new();
        let _token = driver.spawn_local(Box::pin(SelfWakeThenPanic));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.poll_ready()));
        assert!(
            unwind.is_err(),
            "the panic must propagate out of poll_ready"
        );

        assert_eq!(
            driver.pending_task_count(),
            0,
            "the panicking task's own slot is removed"
        );
        assert_eq!(
            driver.ready_task_count(),
            0,
            "the self-wake pushed a now-dangling id into the ready index, but the exact \
             count does not see it: its task is already gone"
        );
        assert_eq!(
            driver.inner.store.lock().ready.len(),
            1,
            "the dangling id is still physically in the index -- only the exact count \
             hides it"
        );

        // The next pump finds no task behind this id (the "not found" skip
        // arm) and moves on rather than erroring or reviving it.
        assert_eq!(driver.poll_ready(), 0, "no live task behind the stale id");
        assert_eq!(
            driver.ready_task_count(),
            0,
            "the stale id does not survive a second pump"
        );
        assert_eq!(
            driver.inner.store.lock().ready.len(),
            0,
            "the pump physically drained the stale entry, not just hidden it"
        );
    }

    /// Cancelling a task that is ready but not yet polled leaves its id in
    /// the ready index — cancellation does not proactively scrub it — and
    /// the next pump's "not found" skip arm cleans it up rather than
    /// reviving or erroring on it.
    #[test]
    fn cancelling_a_ready_but_unpolled_task_self_heals_on_the_next_pump() {
        let driver = AsyncDriver::new();
        let token = driver.spawn_local(Box::pin(std::future::pending::<()>()));

        assert_eq!(
            driver.ready_task_count(),
            1,
            "starts ready, per spawn_local's contract"
        );

        token.cancel();
        assert_eq!(
            driver.pending_task_count(),
            0,
            "cancelled: the task itself is gone"
        );
        assert_eq!(
            driver.ready_task_count(),
            0,
            "the ready index still holds the id (cancellation does not proactively scrub \
             it), but the exact count does not see it: its task is already gone"
        );
        assert_eq!(
            driver.inner.store.lock().ready.len(),
            1,
            "the cancelled id is still physically in the index -- only the exact count \
             hides it"
        );

        assert_eq!(driver.poll_ready(), 0, "no live task behind the stale id");
        assert_eq!(driver.ready_task_count(), 0, "self-healed by the next pump");
        assert_eq!(
            driver.inner.store.lock().ready.len(),
            0,
            "the pump physically drained the stale entry, not just hidden it"
        );
    }

    /// A future that cancels its *own* task mid-poll — dropping the
    /// [`TaskToken`] `spawn_local` handed back, stashed in a cell set just
    /// after spawning — must have its slot removed exactly once, never be
    /// polled again, and must not deadlock. The `is_unlocked()` check right
    /// before the self-drop turns a guard-order regression (`poll_ready`
    /// still holding `Inner::store` while polling) into an immediate,
    /// readable assertion failure instead of an actual hang on the token's
    /// own blocking `cancel()` lock call.
    #[test]
    fn poll_ready_self_cancel_during_pending_removes_slot_once() {
        struct DropOnce(Arc<AtomicUsize>);
        impl Drop for DropOnce {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Release);
            }
        }

        /// Bundles the token with its drop counter so dropping ONE value
        /// proves the token itself dropped exactly once — a `DropOnce`
        /// created fresh as a body-scoped local would only count how many
        /// times the `if let` body ran, not whether the token it sits beside
        /// ever actually dropped.
        #[expect(
            dead_code,
            reason = "both fields exist only for their Drop side effects when Held itself drops"
        )]
        struct Held(TaskToken, DropOnce);

        let driver = AsyncDriver::new();
        let driver_for_task = driver.clone();
        let own_token: Arc<Mutex<Option<Held>>> = Arc::new(Mutex::new(None));
        let own_token_for_task = Arc::clone(&own_token);
        let drop_count = Arc::new(AtomicUsize::new(0));
        let drop_count_for_task = Arc::clone(&drop_count);
        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_task = Arc::clone(&polls);

        let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |_cx| {
            polls_for_task.fetch_add(1, Ordering::Relaxed);
            // Named binding, not an `if let` scrutinee: `own_token_for_task`'s
            // guard (a `let` statement's own temporary) releases here, before
            // `held`'s drop below. The assertion under test is about the
            // DRIVER's store lock, not this one; the extraction is shaped this
            // way so the line does not carry the very scrutinee shape
            // `clippy::significant_drop_in_scrutinee` rejects.
            let held = own_token_for_task.lock().take();
            if let Some(held) = held {
                assert!(
                    driver_for_task.is_unlocked(),
                    "AsyncDriver's store lock must be free while polling: dropping this \
                     task's own TaskToken re-enters cancel(), which would deadlock on a \
                     lock poll_ready still held rather than failing loudly"
                );
                drop(held); // self-cancel, mid-poll
            }
            Poll::Pending
        })));
        let _prev = own_token
            .lock()
            .replace(Held(token, DropOnce(Arc::clone(&drop_count_for_task))));

        assert_eq!(driver.poll_ready(), 1);
        assert_eq!(polls.load(Ordering::Relaxed), 1, "polled exactly once");
        assert_eq!(
            drop_count.load(Ordering::Relaxed),
            1,
            "the self-held token was dropped exactly once"
        );
        assert_eq!(
            driver.pending_task_count(),
            0,
            "the self-cancelled slot must not be resurrected by the outcome match"
        );

        // A later pump must not resurrect or re-poll the self-cancelled task.
        assert_eq!(driver.poll_ready(), 0);
        assert_eq!(polls.load(Ordering::Relaxed), 1, "never re-polled");
        assert_eq!(driver.pending_task_count(), 0);
    }

    /// The deterministic complexity oracle the allocation-oracle integration
    /// test cannot be: at R=0, an O(N) filter-scan and an O(R) drain both
    /// collect zero ready ids, so neither allocates and the allocation test
    /// cannot tell them apart. This counts every [`ReadyFlag::load`] on this
    /// thread instead — `poll_ready`'s drain never reads a dormant task's own
    /// flag to discover it (only `wake_by_ref` arms it and this method's own
    /// clear-before-poll consumes it, neither of which fires for a task
    /// nothing wakes), while an O(N) scan calls `.load()` once per resident
    /// task per pump regardless of readiness. Revert target: replace the
    /// drain step with `store.tasks.iter().filter(|(_, task)|
    /// task.ready.load(Ordering::Acquire))...` (this file's shape before
    /// issue #1056) — reddens this test with a nonzero count.
    #[test]
    fn an_empty_pump_touches_no_dormant_task_flags() {
        let driver = AsyncDriver::new();
        let mut tokens = Vec::with_capacity(1_000);
        for _ in 0..1_000 {
            tokens.push(driver.spawn_local(Box::pin(std::future::pending::<()>())));
        }

        // Warm-up: the first pump polls every freshly spawned task once
        // (spawn seeds `ready`), making all of them dormant. Excluded from
        // the measured window.
        assert_eq!(driver.poll_ready(), 1_000);
        assert_eq!(driver.ready_task_count(), 0);

        reset_ready_flag_load_count();
        for _ in 0..20 {
            assert_eq!(driver.poll_ready(), 0, "R=0 must poll nothing");
        }

        assert_eq!(
            ready_flag_load_count(),
            0,
            "an empty pump must not load a single dormant task's own ready flag -- it \
             drains the ready index, never scans task storage (issue #1056)"
        );

        drop(tokens);
    }

    /// The eager/wake race (module doc, "Readiness index"): a
    /// `spawn_local_eager` inline poll and a concurrent `wake_by_ref` can
    /// each push the SAME id into `store.ready` before either observes the
    /// other's write, since no pump runs between them to clear the flag.
    /// This injects that duplicate deterministically — reaching into
    /// `inner.store` directly, which only this lib test module can do —
    /// rather than trying to time a real race. Revert target: delete
    /// `dedup()` from `poll_ready`'s drain step — reddens this test at 2
    /// polls instead of 1.
    #[test]
    fn poll_ready_dedups_an_eager_double_push_of_the_same_id() {
        let driver = AsyncDriver::new();
        let polls = Arc::new(AtomicUsize::new(0));
        let polls_for_task = Arc::clone(&polls);

        let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |_cx| {
            polls_for_task.fetch_add(1, Ordering::Relaxed);
            Poll::<()>::Pending
        })));

        // `spawn_local` already pushed this id once; push it a second time,
        // exactly as a concurrent `wake_by_ref` racing an eager inline poll
        // would land two entries for the same id in the same batch.
        driver.inner.store.lock().ready.push(token.id());

        assert_eq!(
            driver.poll_ready(),
            1,
            "the duplicate must be deduped, not double-polled"
        );
        assert_eq!(polls.load(Ordering::Relaxed), 1, "polled exactly once");
        assert_eq!(
            driver.pending_task_count(),
            1,
            "still pending after the one legitimate poll"
        );

        drop(token);
    }

    // ── cancellation must not hold the task lock across a user destructor ──
    // (#1038)

    /// `TaskToken::cancel` must drop the removed future only after releasing
    /// `Inner::store`. A destructor is user code; running it under the lock
    /// makes any reentrant destructor (see the two tests below) deadlock.
    #[test]
    fn cancel_drops_the_future_outside_the_task_lock() {
        struct Probe {
            inner: Weak<Inner>,
            unlocked: Arc<AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                let inner = self
                    .inner
                    .upgrade()
                    .expect("the driver outlives the task being cancelled");
                self.unlocked
                    .store(inner.store.try_lock().is_some(), Ordering::Release);
            }
        }

        let driver = AsyncDriver::new();
        let unlocked = Arc::new(AtomicBool::new(false));
        let probe = Probe {
            inner: Arc::downgrade(&driver.inner),
            unlocked: Arc::clone(&unlocked),
        };

        let token = driver.spawn_local(Box::pin(async move {
            let _probe = probe;
            std::future::pending::<()>().await;
        }));
        assert_eq!(driver.poll_ready(), 1);

        token.cancel();

        assert!(
            unlocked.load(Ordering::Acquire),
            "future destructor ran under the task mutex"
        );
    }

    // ── `TaskToken` panic policy: `cancel()` propagates, `Drop` contains
    // only while already unwinding ──

    /// Called directly (not via `Drop`, and not while already unwinding),
    /// `cancel()` must propagate a panic raised by the removed future's own
    /// destructor. The dominant caller (`ViewState::dispose` →
    /// `FutureBuilder::unsubscribe` → `dispose`) already runs inside
    /// `on_unmount`'s own `catch_unwind`-based hook-panic recovery;
    /// `cancel()` swallowing this panic itself would hide it from that
    /// accounting instead of reporting through it.
    #[test]
    fn cancel_propagates_the_removed_futures_panic() {
        struct PanicsOnDrop;
        impl Drop for PanicsOnDrop {
            fn drop(&mut self) {
                panic!("cancel-propagation probe");
            }
        }

        let driver = AsyncDriver::new();
        let token = driver.spawn_local(Box::pin(async move {
            let _payload = PanicsOnDrop;
            std::future::pending::<()>().await;
        }));
        // Poll once so the async block actually starts executing (and so
        // constructs `_payload`) before it is cancelled — an unpolled
        // future's body never ran, so there would be nothing to panic.
        assert_eq!(driver.poll_ready(), 1);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| token.cancel()));
        assert!(
            unwind.is_err(),
            "cancel() must propagate the removed future's destructor panic"
        );
    }

    /// `Drop for TaskToken`, reached while this thread is already unwinding
    /// from an unrelated panic, must CONTAIN a panic `cancel()` raises
    /// rather than let it become a double panic. Reverting the `Drop` impl
    /// to an unconditional `self.cancel();` `abort`s this whole test
    /// process before the outer `catch_unwind` below can even return —
    /// reaching the final assertion is itself the proof.
    #[test]
    fn drop_while_already_unwinding_contains_the_cancel_panic_instead_of_aborting() {
        struct PanicsOnDrop;
        impl Drop for PanicsOnDrop {
            fn drop(&mut self) {
                panic!("drop-while-unwinding probe");
            }
        }

        let driver = AsyncDriver::new();
        let token = driver.spawn_local(Box::pin(async move {
            let _payload = PanicsOnDrop;
            std::future::pending::<()>().await;
        }));
        // Poll once so `_payload` is actually constructed before the token
        // is dropped — see `cancel_propagates_the_removed_futures_panic`.
        assert_eq!(driver.poll_ready(), 1);

        // `token`'s `Drop` runs while this closure is unwinding from the
        // panic below, i.e. with `std::thread::panicking()` already `true`.
        let outer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _token = token;
            panic!("outer probe");
        }));

        assert!(
            outer.is_err(),
            "the outer panic must still propagate; only the token's own \
             nested cancel panic is contained"
        );
    }

    /// The panic PAYLOAD `cancel()` raises can itself own a type whose own
    /// `Drop` panics — a payload built via `std::panic::panic_any`, not the
    /// default string-message panic. `Drop for TaskToken`'s already-
    /// unwinding branch must discard that payload through
    /// `discard_panic_payload`, not a bare `drop`: a bare drop here would
    /// let the payload's second panic escape uncontained while this thread
    /// is already unwinding from the outer probe below — a double panic,
    /// which `abort`s the whole process (empirically confirmed against a
    /// bare-drop version of this exact shape: "panic in a destructor
    /// during cleanup … aborting").
    #[test]
    fn drop_while_already_unwinding_contains_a_payload_whose_own_drop_panics() {
        struct PayloadPanicsOnDrop;
        impl Drop for PayloadPanicsOnDrop {
            fn drop(&mut self) {
                panic!("payload-drop probe");
            }
        }

        struct PanicsWithPayloadOnDrop;
        impl Drop for PanicsWithPayloadOnDrop {
            fn drop(&mut self) {
                std::panic::panic_any(PayloadPanicsOnDrop);
            }
        }

        let driver = AsyncDriver::new();
        let token = driver.spawn_local(Box::pin(async move {
            let _payload = PanicsWithPayloadOnDrop;
            std::future::pending::<()>().await;
        }));
        assert_eq!(driver.poll_ready(), 1);

        // `token`'s `Drop` runs while this closure is unwinding: `cancel()`'s
        // internal `catch_unwind` catches a `Box<PayloadPanicsOnDrop>`
        // payload whose own `Drop` also panics — exactly the shape
        // `discard_panic_payload` exists to contain.
        let outer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _token = token;
            panic!("outer probe");
        }));

        assert!(
            outer.is_err(),
            "the outer panic must still propagate; reaching this line at all \
             is the proof the payload's own drop panic did not abort the process"
        );
    }

    /// The public-API reproducer from #1038: a future that owns a second
    /// [`TaskToken`] from the same driver. Cancelling the outer task drops
    /// the inner future, whose `_child` field drops the nested token, which
    /// re-enters `cancel()` — and so `Inner::tasks.lock()` — from inside the
    /// outer cancellation's own destructor. Run off-thread with a bounded
    /// wait: a deadlocked thread is leaked, not joined, and nextest gives
    /// this test its own process, so a leaked thread costs nothing.
    #[test]
    fn cancelling_a_future_that_owns_another_token_terminates() {
        let driver = AsyncDriver::new();
        let child = driver.spawn_local(Box::pin(std::future::pending::<()>()));
        let parent = driver.spawn_local(Box::pin(async move {
            let _child = child;
            std::future::pending::<()>().await;
        }));
        driver.poll_ready();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            parent.cancel();
            let _ = done_tx.send(());
        });

        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect(
                "parent.cancel() deadlocked: the nested TaskToken::drop tried to \
                 re-lock Inner::tasks while cancel()'s own guard was still held",
            );
        // Termination alone would also be satisfied by a cancel that never
        // reached the child; the empty driver pins that the nested token's
        // own cancellation ran to completion.
        assert_eq!(
            driver.pending_task_count(),
            0,
            "the parent's destructor must have cancelled the child it owned"
        );
    }

    /// A cancelled future's destructor may re-enter the driver three
    /// different ways — querying `pending_task_count`, spawning a new task,
    /// and waking a sibling's stored [`Waker`] — none of which may deadlock
    /// on `Inner::tasks`.
    #[test]
    fn destructor_reentry_through_query_spawn_and_sibling_wake_does_not_deadlock() {
        let driver = AsyncDriver::new();

        // A sibling task that stores its waker on every poll and stays
        // pending until told to finish.
        let (sibling, sibling_polls, sibling_finish, sibling_waker) = controlled();
        // `ManuallyDrop`, not a plain binding: if the cancel below ever
        // deadlocks again, the `expect` panics after 5 s and this thread
        // unwinds — and a live `TaskToken` on the unwinding thread would run
        // `cancel()` and block on the very guard the leaked thread still
        // holds, turning a bounded failure into a hang. It is released only
        // after the wait succeeds.
        let sibling_token = std::mem::ManuallyDrop::new(driver.spawn_local(Box::pin(sibling)));
        driver.poll_ready();
        assert_eq!(sibling_polls.load(Ordering::Relaxed), 1, "waker stored");

        let spawned_ran = Arc::new(AtomicBool::new(false));
        let spawned_token: Arc<Mutex<Option<TaskToken>>> = Arc::new(Mutex::new(None));

        /// Re-enters the driver from `Drop`: queries, spawns, and wakes a
        /// sibling task, all of which lock `Inner::tasks`.
        struct Reentrant {
            driver: AsyncDriver,
            spawned_ran: Arc<AtomicBool>,
            spawned_token: Arc<Mutex<Option<TaskToken>>>,
            sibling_waker: Arc<Mutex<Option<Waker>>>,
        }

        impl Drop for Reentrant {
            fn drop(&mut self) {
                // (a) query
                let _ = self.driver.pending_task_count();
                // (b) spawn — keep the token alive, or it would be cancelled
                // (and dropped) again right here.
                let ran = Arc::clone(&self.spawned_ran);
                let token = self
                    .driver
                    .spawn_local(Box::pin(async move { ran.store(true, Ordering::Release) }));
                let _prev = self.spawned_token.lock().replace(token);
                // (c) wake a sibling task.
                if let Some(waker) = self.sibling_waker.lock().as_ref() {
                    waker.wake_by_ref();
                }
            }
        }

        let reentrant = Reentrant {
            driver: driver.clone(),
            spawned_ran: Arc::clone(&spawned_ran),
            spawned_token: Arc::clone(&spawned_token),
            sibling_waker: Arc::clone(&sibling_waker),
        };
        let outer = driver.spawn_local(Box::pin(async move {
            let _reentrant = reentrant;
            std::future::pending::<()>().await;
        }));
        driver.poll_ready();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            outer.cancel();
            let _ = done_tx.send(());
        });

        done_rx.recv_timeout(std::time::Duration::from_secs(5)).expect(
            "cancel() deadlocked: destructor reentry (query/spawn/wake) blocked on the task mutex",
        );
        let _sibling_token = std::mem::ManuallyDrop::into_inner(sibling_token);

        assert!(
            spawned_token.lock().is_some(),
            "the reentrant spawn_local did not register a task"
        );
        assert_eq!(
            driver.pending_task_count(),
            2,
            "the sibling and the reentrant-spawned task remain; the cancelled \
             outer task is gone"
        );

        // The sibling must be armed by the reentrant wake, and the
        // reentrant-spawned task starts armed by construction: both poll
        // (and, since the sibling is told to finish, both complete) on the
        // very next frame.
        sibling_finish.store(true, Ordering::Release);
        assert_eq!(
            driver.poll_ready(),
            2,
            "the woken sibling and the reentrant-spawned task both poll this frame"
        );
        assert_eq!(
            sibling_polls.load(Ordering::Relaxed),
            2,
            "the sibling was actually polled again, not just left marked ready"
        );
        assert!(
            spawned_ran.load(Ordering::Acquire),
            "the reentrant-spawned task ran"
        );
    }

    /// `set_request_frame`'s doc says "called once at wiring time", but
    /// nothing enforces that, and it has the identical hazard shape as
    /// `UpdateScheduler::set_on_frame_scheduled`: a single-slot `Option<Arc<dyn
    /// Fn>>` hook, replaced with a bare assignment that drops the displaced
    /// `Arc` while the guard is still live.
    #[test]
    fn replacing_the_request_frame_hook_drops_the_previous_hook_outside_the_lock() {
        struct Probe {
            inner: Weak<Inner>,
            unlocked: Arc<AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                let inner = self
                    .inner
                    .upgrade()
                    .expect("the driver outlives the hook it holds");
                self.unlocked
                    .store(inner.request_frame.try_lock().is_some(), Ordering::Release);
            }
        }

        let driver = AsyncDriver::new();
        let unlocked = Arc::new(AtomicBool::new(false));
        let probe = Probe {
            inner: Arc::downgrade(&driver.inner),
            unlocked: Arc::clone(&unlocked),
        };

        driver.set_request_frame(move || {
            let _keep_alive = &probe;
        });
        driver.set_request_frame(|| {});

        assert!(
            unlocked.load(Ordering::Acquire),
            "the previous request-frame hook's destructor ran under its own mutex"
        );
    }
}
