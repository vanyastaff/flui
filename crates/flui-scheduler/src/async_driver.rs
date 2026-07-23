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

/// One live task.
struct Task {
    /// The future, absent while it is being polled (moved out so the driver's
    /// lock is not held across user code).
    future: Option<BoxedTask>,
    /// Set by the waker; cleared immediately before each poll.
    ready: Arc<AtomicBool>,
    /// Set by [`TaskToken::drop`]. Checked after a poll returns, so a token
    /// dropped *during* a poll still cancels rather than re-queueing.
    cancelled: Arc<AtomicBool>,
}

/// Shared driver state.
struct Inner {
    /// `BTreeMap`, not `HashMap`: polling order is ascending task id, so a frame
    /// is deterministic and headless tests do not depend on hash seeds.
    tasks: Mutex<BTreeMap<TaskId, Task>>,
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
    ready: Arc<AtomicBool>,
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
            // case it must not wake the event loop for work that can never run.
            let task_is_live = inner.tasks.lock().contains_key(&self.id);
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
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(inner) = self.inner.upgrade() {
            // Dropping the future here runs its destructors on the caller's
            // thread. If the task is mid-poll its slot holds `None`, and
            // `poll_ready` will honour `cancelled` when it tries to re-queue.
            inner.tasks.lock().remove(&self.id);
        }
    }
}

impl Drop for TaskToken {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A frame-driven task driver.
///
/// Cheap to clone; every clone refers to the same task set. Owned by
/// [`Scheduler`](crate::Scheduler), which exposes
/// [`spawn_local`](crate::Scheduler::spawn_local) and
/// [`drive_async_tasks`](crate::Scheduler::drive_async_tasks).
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
                tasks: Mutex::new(BTreeMap::new()),
                next_id: AtomicU64::new(1),
                request_frame: Mutex::new(None),
            }),
        }
    }

    /// Install the binding's "request a frame" hook.
    ///
    /// Called once at wiring time. A driver with no hook still polls whenever a
    /// frame happens to run — headless tests rely on that.
    pub fn set_request_frame<F>(&self, hook: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.inner.request_frame.lock() = Some(Arc::new(hook));
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
        let ready = Arc::new(AtomicBool::new(true));
        let cancelled = Arc::new(AtomicBool::new(false));

        self.inner.tasks.lock().insert(
            id,
            Task {
                future: Some(future),
                ready: Arc::clone(&ready),
                cancelled: Arc::clone(&cancelled),
            },
        );

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
    /// [`Scheduler::drive_async_tasks`](crate::Scheduler::drive_async_tasks), and
    /// so does not trip that method's "never poll during persistent callbacks"
    /// guard: this is a single task polled at its own subscription point, not the
    /// frame's driver step.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local_eager(&self, mut future: BoxedTask) -> Option<TaskToken> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        // Starts NOT ready: we are about to poll it ourselves. A wake landing
        // during that poll flips this to `true` (and requests a frame), so the
        // task is correctly re-armed when we queue it below.
        let ready = Arc::new(AtomicBool::new(false));
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

        self.inner.tasks.lock().insert(
            id,
            Task {
                future: Some(future),
                ready: Arc::clone(&ready),
                cancelled: Arc::clone(&cancelled),
            },
        );

        // A wake that landed *during* the inline poll found no task in the map
        // (we insert only after polling), so `wake_by_ref`'s stale-waker guard
        // suppressed its frame request. Re-request here, now that the task is
        // live, or an already-armed task would wait for a frame nobody asked for.
        if ready.load(Ordering::Acquire) {
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
    /// ([`Scheduler::drive_async_tasks`](crate::Scheduler::drive_async_tasks)).
    /// Never call it from build, layout, or paint.
    ///
    /// Returns the number of tasks polled. Tasks are polled in ascending id
    /// order; a task that completes or is cancelled is removed. A task woken
    /// *during* this call is left `ready` and picked up next frame — the driver
    /// never spins.
    pub fn poll_ready(&self) -> usize {
        // Snapshot the ready ids, then release the lock: a task's `poll` may
        // spawn, cancel, or wake — all of which take this lock.
        let ready_ids: Vec<TaskId> = {
            let tasks = self.inner.tasks.lock();
            tasks
                .iter()
                .filter(|(_, task)| task.ready.load(Ordering::Acquire))
                .map(|(id, _)| *id)
                .collect()
        };

        let mut polled = 0;
        for id in ready_ids {
            // Take the future out so no lock is held across user code.
            let Some((mut future, ready, cancelled)) = ({
                let mut tasks = self.inner.tasks.lock();
                tasks.get_mut(&id).and_then(|task| {
                    // Clear BEFORE polling: a wake landing during the poll must
                    // re-arm the task rather than be swallowed.
                    task.ready.store(false, Ordering::Release);
                    task.future.take().map(|future| {
                        (future, Arc::clone(&task.ready), Arc::clone(&task.cancelled))
                    })
                })
            }) else {
                continue; // cancelled between snapshot and poll
            };

            let waker = Waker::from(Arc::new(TaskWaker {
                id,
                ready,
                cancelled: Arc::clone(&cancelled),
                inner: Arc::downgrade(&self.inner),
            }));
            let mut cx = Context::from_waker(&waker);
            let outcome = future.as_mut().poll(&mut cx);
            polled += 1;

            let mut tasks = self.inner.tasks.lock();
            match outcome {
                Poll::Ready(()) => {
                    tasks.remove(&id);
                }
                Poll::Pending => {
                    if cancelled.load(Ordering::Acquire) {
                        // The token was dropped while we polled; honour it.
                        tasks.remove(&id);
                    } else if let Some(task) = tasks.get_mut(&id) {
                        task.future = Some(future);
                    }
                    // else: `cancel()` already removed the slot; drop the future.
                }
            }
        }

        polled
    }

    /// Number of tasks the driver is holding.
    ///
    /// A count, never a guard — the lock stays private (SP-6).
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.inner.tasks.lock().len()
    }

    /// Number of tasks whose waker has fired since the last poll.
    #[must_use]
    pub fn ready_task_count(&self) -> usize {
        self.inner
            .tasks
            .lock()
            .values()
            .filter(|task| task.ready.load(Ordering::Acquire))
            .count()
    }
}

impl std::fmt::Debug for AsyncDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock`: `{:?}` while the task map is held (e.g. instrumenting a
        // poll) must not deadlock — same discipline as `ExternalBuildScheduler`.
        f.debug_struct("AsyncDriver")
            .field(
                "tasks",
                &self.inner.tasks.try_lock().map(|tasks| tasks.len()),
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
                *self.waker.lock() = Some(cx.waker().clone());
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
            *stored_for_task.lock() = Some(cx.waker().clone());
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
            *child_token_for_task.lock() = Some(token);
        }));

        driver.poll_ready(); // parent runs, spawns child
        assert!(child_token.lock().is_some(), "child was spawned");
        assert!(!spawned.load(Ordering::Acquire), "child not polled yet");

        driver.poll_ready(); // child runs
        assert!(spawned.load(Ordering::Acquire));
    }
}
