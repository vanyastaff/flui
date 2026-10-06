//! The realm's owner-local frame state, and post-frame capabilities.
//!
//! [`OwnerFrame`] is what a frame needs from its owner thread that the
//! (still `Send`) [`UpdateScheduler`] cannot hold: the owner-local post-frame
//! queue and the async task store. A realm (or a headless binding) owns
//! exactly one and is the only strong owner; every frame entry point takes it
//! by reference — [`UpdateScheduler::drive_frame`],
//! [`UpdateScheduler::handle_begin_frame`], [`UpdateScheduler::end_frame`],
//! [`UpdateScheduler::execute_frame`] — so no frame can poll or drain
//! "nothing".
//!
//! Shared callbacks remain `Send` and live in the scheduler's synchronized
//! queue, reached through [`PostFrameHandle`]. Owner-local callbacks live in
//! the owner frame's `Rc` queue and are reached through
//! [`LocalPostFrameHandle`], a `!Send` handle holding a `Weak` pointer straight
//! at that queue. The two queues share one registration order.
//!
//! There is no thread-local registry and no "currently active lane" concept:
//! a handle always addresses the one owner frame it was minted from, and the
//! frame drive drains an owner frame by receiving it as an explicit
//! parameter, never by reading ambient state.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use crate::async_driver::{RetirePanic, TaskStore};
use crate::{
    AsyncDriver, CallbackId, FrameTiming, PostFrameCallback, UpdateScheduler, WeakUpdateScheduler,
};

pub(crate) type OwnerPostFrameCallback = Box<dyn FnOnce(&FrameTiming) + 'static>;

pub(crate) struct LocalPostFrameEntry {
    pub(crate) id: CallbackId,
    pub(crate) callback: OwnerPostFrameCallback,
}

struct LocalLaneInner {
    queue: RefCell<Vec<LocalPostFrameEntry>>,
    /// Set by retirement: no callback is admitted afterwards.
    closed: Cell<bool>,
}

/// The realm's owner-local frame state: its post-frame queue and its async
/// tasks.
///
/// Runtime-internal: public only because the realm and the headless bindings
/// live in sibling crates. `!Send + !Sync` through its `Rc` storage, and not
/// `Clone`: its owner — the realm, or a headless binding — holds the only
/// strong reference to the tasks and callbacks, so they are created, run and
/// dropped on the owner thread and never outlive it. Widgets reach it through
/// `Weak` handles ([`AsyncDriver`], [`LocalPostFrameHandle`]).
///
/// Dropping it [retires](Self::retire) whatever is still queued.
#[doc(hidden)]
pub struct OwnerFrame {
    scheduler: WeakUpdateScheduler,
    post_frame: Rc<LocalLaneInner>,
    tasks: Rc<TaskStore>,
}

impl OwnerFrame {
    /// Owner-local frame state for `scheduler`'s frames. Task wakes request a
    /// frame through `scheduler`'s [`FrameWaker`](crate::FrameWaker).
    #[must_use]
    pub fn new(scheduler: &UpdateScheduler) -> Self {
        Self::with_tasks(scheduler, TaskStore::new())
    }

    /// As [`new`](Self::new), issuing task ids from `first`: a test reaches
    /// the identity-exhaustion boundary without spawning `u64::MAX` tasks.
    #[cfg(test)]
    pub(crate) fn with_first_task_id(scheduler: &UpdateScheduler, first: u64) -> Self {
        Self::with_tasks(scheduler, TaskStore::with_first_id(first))
    }

    fn with_tasks(scheduler: &UpdateScheduler, tasks: TaskStore) -> Self {
        let waker = scheduler.frame_waker();
        tasks.set_request_frame(std::sync::Arc::new(move || waker.request_frame()));
        Self {
            scheduler: scheduler.downgrade(),
            post_frame: Rc::new(LocalLaneInner {
                queue: RefCell::new(Vec::new()),
                closed: Cell::new(false),
            }),
            tasks: Rc::new(tasks),
        }
    }

    /// A widget's handle to this frame's tasks: `Weak`, so it keeps nothing
    /// alive past the owner.
    #[must_use]
    pub fn async_driver(&self) -> AsyncDriver {
        AsyncDriver::new(&self.tasks)
    }

    /// A `!Send` handle addressed directly at this frame's post-frame queue.
    #[must_use]
    pub fn local_post_frame_handle(&self) -> LocalPostFrameHandle {
        LocalPostFrameHandle {
            scheduler: self.scheduler.clone(),
            lane: Rc::downgrade(&self.post_frame),
        }
    }

    /// Poll every task whose waker fired since the last poll; returns the
    /// number polled.
    ///
    /// The frame's mid-frame slot calls it from
    /// [`UpdateScheduler::handle_begin_frame`]; a wake that runs no frame
    /// calls it after [`UpdateScheduler::finish_async_pump`]. Never from
    /// build, layout or paint. Tasks are polled in ascending id order; a task
    /// that completes or is cancelled is removed; a task woken during this
    /// call is polled next time — the driver never spins. Cost scales with
    /// ready tasks, not resident ones.
    ///
    /// # Panics
    ///
    /// Propagates a task's poll panic after removing that task and keeping
    /// every unreached sibling indexed for the next poll.
    pub fn poll_ready(&self) -> usize {
        debug_assert!(
            self.scheduler.upgrade().is_none_or(
                |scheduler| scheduler.phase() != crate::SchedulerPhase::PersistentCallbacks
            ),
            "BUG: owner-local tasks must not be polled during build/layout/paint; the \
             poll belongs between the transient and persistent callbacks"
        );
        self.tasks.poll_ready()
    }

    /// Number of tasks this frame holds.
    #[must_use]
    pub fn pending_task_count(&self) -> usize {
        self.tasks.pending_task_count()
    }

    /// Number of live tasks currently indexed as due a poll. A same-batch
    /// duplicate index entry counts twice until the next poll collapses it.
    #[must_use]
    pub fn ready_task_count(&self) -> usize {
        self.tasks.ready_task_count()
    }

    /// Drop every queued post-frame callback, then every task, on this (the
    /// owner) thread, each under its own catch, and admit nothing afterwards.
    ///
    /// A realm's teardown calls it in its own order, before it resumes any
    /// earlier failure, so the captures of both queues are dropped on the
    /// owner exactly once. Returns the first panic a destructor raised; later
    /// ones are retained, never dropped. During an existing unwind the values
    /// are retained without running their destructors. Idempotent; `Drop`
    /// calls it for an owner that did not.
    #[must_use = "the first destructor panic is returned for the owner to raise"]
    pub fn retire(&self) -> Option<RetirePanic> {
        self.post_frame.closed.set(true);
        let callbacks = self.post_frame.queue.take();
        let mut first: Option<RetirePanic> = None;
        for entry in callbacks {
            if std::thread::panicking() {
                std::mem::forget(entry);
                continue;
            }
            if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(entry))) {
                keep_first(&mut first, payload);
            }
        }
        if let Some(payload) = self.tasks.retire() {
            keep_first(&mut first, payload);
        }
        first
    }

    /// Whether this frame belongs to `scheduler`'s frames.
    pub(crate) fn belongs_to(&self, scheduler: &UpdateScheduler) -> bool {
        self.scheduler
            .upgrade()
            .is_some_and(|owner| owner.is_same_instance(scheduler))
    }

    /// Drain the post-frame queue for `scheduler`'s frame drive — but only if
    /// `scheduler` is genuinely the one this frame was made for.
    ///
    /// A queue drained by another scheduler would hand its callbacks a foreign
    /// frame's `FrameTiming`, remove them before their own frame ever runs
    /// them, and interleave `CallbackId`s from another id sequence into a sort
    /// where they carry no meaning. On mismatch (or a since-dropped owning
    /// scheduler) this returns `Err` and leaves the queue untouched.
    pub(crate) fn take_post_frame_queue_for(
        &self,
        scheduler: &UpdateScheduler,
    ) -> Result<Vec<LocalPostFrameEntry>, LocalPostFrameScheduleError> {
        let Some(owner) = self.scheduler.upgrade() else {
            tracing::error!(
                driving_scheduler = scheduler.debug_ptr(),
                "an OwnerFrame's scheduler is already gone; refusing to drain it from this \
                 (necessarily unrelated) frame drive"
            );
            return Err(LocalPostFrameScheduleError::LaneClosed);
        };
        if !owner.is_same_instance(scheduler) {
            tracing::error!(
                owner_frame_scheduler = owner.debug_ptr(),
                driving_scheduler = scheduler.debug_ptr(),
                "an OwnerFrame was handed to a frame drive on a scheduler it does not belong \
                 to; refusing to drain or poll it — its own scheduler's next drive still does"
            );
            return Err(LocalPostFrameScheduleError::WrongScheduler);
        }
        Ok(self.post_frame.queue.take())
    }

    /// Return an uninvoked tail to the already-validated queue. Original IDs
    /// retain FIFO order ahead of newer registrations on the next drain.
    pub(crate) fn restore_post_frame_queue(&self, entries: Vec<LocalPostFrameEntry>) {
        self.post_frame.queue.borrow_mut().extend(entries);
    }
}

fn keep_first(first: &mut Option<RetirePanic>, payload: RetirePanic) {
    if first.is_none() {
        *first = Some(payload);
    } else {
        flui_foundation::panic::retain_opaque_payload(payload);
    }
}

impl Drop for OwnerFrame {
    fn drop(&mut self) {
        if let Some(payload) = self.retire() {
            if std::thread::panicking() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

impl std::fmt::Debug for OwnerFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerFrame")
            .field(
                "post_frame_callbacks",
                &self
                    .post_frame
                    .queue
                    .try_borrow()
                    .map(|queue| queue.len())
                    .ok(),
            )
            .field("tasks", &self.tasks)
            .finish_non_exhaustive()
    }
}

/// Why an owner-local post-frame callback could not be registered, or an
/// owner frame's queue could not be drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocalPostFrameScheduleError {
    /// The owning realm (its owner frame or its scheduler) is gone or retired
    /// — there is no frame left for the callback to observe, and it is
    /// guaranteed never to run.
    #[error("the handle's owner-local lane is closed")]
    LaneClosed,
    /// An owner frame was handed to a frame drive on a different
    /// `UpdateScheduler` than the one it was made for. Draining it anyway
    /// would hand its callbacks a foreign frame's `FrameTiming`, so the drive
    /// refuses to drain it at all; its own scheduler's next drive still
    /// delivers it.
    #[error("the lane belongs to a different UpdateScheduler than the one draining it")]
    WrongScheduler,
}

/// Schedules owner-local work after a completed frame's layout and paint.
///
/// `!Send`: holds a [`Weak`] pointer directly at its owner frame's `Rc`
/// queue, so it can capture non-`Send` state (`Rc`/`RefCell`) in the
/// callbacks it schedules. Moving one to another thread is a compile error,
/// not a runtime check.
#[derive(Clone)]
pub struct LocalPostFrameHandle {
    scheduler: WeakUpdateScheduler,
    lane: Weak<LocalLaneInner>,
}

impl LocalPostFrameHandle {
    /// Schedule an owner-local callback after the next completed frame.
    ///
    /// The callback may capture `Rc`/`RefCell` state. On error (the owning
    /// realm is gone or retired) the callback is dropped without running —
    /// provably: nothing retains it once this call returns `Err`.
    ///
    /// Runs in the same total order as every [`PostFrameHandle::schedule`]
    /// callback registered for this frame — by registration order, across
    /// both handle types, not "all local callbacks, then all shared" or the
    /// reverse.
    pub fn schedule_local(
        &self,
        callback: impl FnOnce(&FrameTiming) + 'static,
    ) -> Result<(), LocalPostFrameScheduleError> {
        let lane = self
            .lane
            .upgrade()
            .filter(|lane| !lane.closed.get())
            .ok_or(LocalPostFrameScheduleError::LaneClosed)?;
        let scheduler = self
            .scheduler
            .upgrade()
            .ok_or(LocalPostFrameScheduleError::LaneClosed)?;
        scheduler.with_post_frame_registration(|id| {
            lane.queue.borrow_mut().push(LocalPostFrameEntry {
                id,
                callback: Box::new(callback),
            });
        });
        Ok(())
    }

    /// The number of owner-local callbacks queued on this handle's owner frame
    /// and not yet drained by a completed frame; `0` once it is closed.
    ///
    /// A self-rescheduling callback (one that queues its successor from
    /// inside its own run) keeps this at `1` after every frame, so a reading
    /// of `0` after a frame is how an owner proves such a loop has stopped.
    ///
    /// A test probe, not part of the documented API: no production path reads
    /// it. It exists so an owner crate's integration tests can observe the
    /// queue without a test-only global counter.
    #[doc(hidden)]
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.lane
            .upgrade()
            .map_or(0, |lane| lane.queue.borrow().len())
    }

    /// Whether this handle targets `other`.
    #[must_use]
    pub fn targets_same_scheduler(&self, other: &UpdateScheduler) -> bool {
        self.scheduler
            .upgrade()
            .is_some_and(|scheduler| scheduler.is_same_instance(other))
    }
}

impl std::fmt::Debug for LocalPostFrameHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostFrameHandle")
            .field("lane_alive", &(self.lane.strong_count() > 0))
            .finish_non_exhaustive()
    }
}

/// Schedules `Send` work after the next completed frame.
///
/// Holds a [`WeakUpdateScheduler`], not a strong `UpdateScheduler`: this handle is
/// `Clone + Send + Sync` and vended into widget capabilities (ADR-0021) that
/// may legitimately outlive the realm that built them, so a surviving handle
/// must fail closed instead of pinning a dead realm's scheduler alive.
#[derive(Clone)]
pub struct PostFrameHandle {
    scheduler: WeakUpdateScheduler,
}

impl PostFrameHandle {
    /// Construct a handle for `Send` post-frame callbacks.
    #[must_use]
    pub fn new(scheduler: &UpdateScheduler) -> Self {
        Self {
            scheduler: scheduler.downgrade(),
        }
    }

    /// Schedule a `Send` callback after the next completed frame.
    ///
    /// If the backing scheduler is already gone (its owning realm has torn
    /// down), the callback is dropped without running and a `tracing::warn!`
    /// is emitted — there is no frame left for it to observe.
    pub fn schedule(&self, callback: impl FnOnce(&FrameTiming) + Send + 'static) {
        let Some(scheduler) = self.scheduler.upgrade() else {
            tracing::warn!(
                "PostFrameHandle::schedule: backing scheduler is gone; dropping callback"
            );
            return;
        };
        let boxed: PostFrameCallback = Box::new(callback);
        scheduler.add_post_frame_callback(boxed);
    }

    /// Whether this handle targets `other`.
    #[must_use]
    pub fn targets_same_scheduler(&self, other: &UpdateScheduler) -> bool {
        self.scheduler
            .upgrade()
            .is_some_and(|scheduler| scheduler.is_same_instance(other))
    }
}

impl std::fmt::Debug for PostFrameHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostFrameHandle").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::*;
    use crate::SchedulerPhase;

    assert_impl_all!(UpdateScheduler: Send, Sync);
    assert_impl_all!(PostFrameHandle: Send, Sync);
    assert_not_impl_any!(OwnerFrame: Send, Sync);
    assert_not_impl_any!(LocalPostFrameHandle: Send, Sync);

    fn assert_batch_depth(
        log: &flui_testing::log_capture::CapturedLog,
        shared: usize,
        local: usize,
        total: usize,
    ) {
        let records: Vec<_> = log
            .records()
            .iter()
            .filter(|record| record.message == "draining post-frame callback batch")
            .collect();
        assert_eq!(records.len(), 1, "exactly one batch trace: {log}");
        let record = records[0];
        assert_eq!(
            record
                .field("shared_callbacks")
                .and_then(|value| value.parse().ok()),
            Some(shared)
        );
        assert_eq!(
            record
                .field("local_callbacks")
                .and_then(|value| value.parse().ok()),
            Some(local)
        );
        assert_eq!(
            record
                .field("total_callbacks")
                .and_then(|value| value.parse().ok()),
            Some(total)
        );
    }

    fn post_frame_panic_restores_idle_and_later_scheduling_works() {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler);
        owner
            .local_post_frame_handle()
            .schedule_local(|_| panic!("post-frame probe"))
            .expect("lane alive");
        assert!(catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame(&owner))).is_err());
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        let fired = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired);
        owner
            .local_post_frame_handle()
            .schedule_local(move |_| callback.set(true))
            .expect("gate remains usable");
        scheduler.execute_frame(&owner);
        assert!(fired.get());
    }

    fn post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work() {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler);
        let log = Arc::new(Mutex::new(Vec::new()));
        let panic_calls = Rc::new(Cell::new(0));
        let counted = panic_calls.clone();
        let reentrant = owner.local_post_frame_handle();
        let reentrant_log = log.clone();
        owner
            .local_post_frame_handle()
            .schedule_local(move |_| {
                counted.set(counted.get() + 1);
                reentrant
                    .schedule_local(move |_| {
                        reentrant_log.lock().expect("log").push(3);
                    })
                    .expect("lane alive");
                panic!("first callback poisons this frame, not the uninvoked queue");
            })
            .expect("lane alive");
        let shared_log = log.clone();
        scheduler.add_post_frame_callback(Box::new(move |_| {
            shared_log.lock().expect("log").push(1);
        }));
        let local_log = log.clone();
        owner
            .local_post_frame_handle()
            .schedule_local(move |_| {
                local_log.lock().expect("log").push(2);
            })
            .expect("lane alive");

        let (panicked, first_batch) = flui_testing::log_capture::capture(|| {
            catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame(&owner)))
        });
        assert!(panicked.is_err());
        assert_batch_depth(&first_batch, 1, 2, 3);
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        assert!(
            log.lock().expect("log").is_empty(),
            "the poisoned frame stops delivery"
        );
        let (_retry_frame, retry_batch) =
            flui_testing::log_capture::capture(|| scheduler.execute_frame(&owner));
        assert_batch_depth(&retry_batch, 1, 2, 3);
        assert_eq!(
            *log.lock().expect("log"),
            [1, 2, 3],
            "uninvoked callbacks retain original registration order across both lanes"
        );
        assert_eq!(
            panic_calls.get(),
            1,
            "the callback that already ran is not retried"
        );
    }

    /// Retirement drops queued owner-local callbacks once, unrun, keeps the
    /// first destructor panic, and closes the queue to later registrations.
    fn retirement_drops_queued_callbacks_and_closes_the_queue() {
        struct Probe {
            drops: Rc<Cell<usize>>,
            panics: bool,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                self.drops.set(self.drops.get() + 1);
                assert!(!self.panics, "callback capture probe");
            }
        }

        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler);
        let handle = owner.local_post_frame_handle();
        let drops = Rc::new(Cell::new(0));
        let ran = Rc::new(Cell::new(false));
        for panics in [true, false] {
            let probe = Probe {
                drops: Rc::clone(&drops),
                panics,
            };
            let ran = Rc::clone(&ran);
            handle
                .schedule_local(move |_| {
                    let _probe = probe;
                    ran.set(true);
                })
                .expect("lane alive");
        }
        let first = owner.retire().expect("a capture's destructor panicked");
        assert_eq!(
            first.downcast_ref::<&str>().copied(),
            Some("callback capture probe")
        );
        assert_eq!(drops.get(), 2, "both captures dropped once");
        assert!(!ran.get(), "a retired callback never runs");
        assert_eq!(
            handle.schedule_local(|_| {}),
            Err(LocalPostFrameScheduleError::LaneClosed)
        );
        scheduler.execute_frame(&owner);
        assert!(!ran.get());
    }

    #[test]
    fn post_frame_panic_matrix() {
        crate::table_test::run_table(
            "post_frame_panic_matrix",
            &[
                (
                    "post_frame_panic_restores_idle_and_later_scheduling_works",
                    post_frame_panic_restores_idle_and_later_scheduling_works as fn(),
                ),
                (
                    "post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work",
                    post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work as fn(),
                ),
                (
                    "retirement_drops_queued_callbacks_and_closes_the_queue",
                    retirement_drops_queued_callbacks_and_closes_the_queue as fn(),
                ),
            ],
        );
    }
}
