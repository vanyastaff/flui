//! Binding-scoped post-frame capabilities.
//!
//! Shared callbacks remain `Send` and live in the scheduler's synchronized queue,
//! reached through [`PostFrameHandle`]. Owner-local callbacks live in
//! [`LocalPostFrameLane`]'s `Rc` queue and are reached through
//! [`LocalPostFrameHandle`], a `!Send` handle holding a `Weak` pointer straight at
//! its lane's storage.
//!
//! There is no thread-local registry and no "currently active lane" concept:
//! earlier revisions resolved a `Send`-safe ticket against a thread-local map and
//! an activation stack, because a `Clone + Send + Sync` handle cannot hold an
//! `Rc` directly. `LocalPostFrameHandle` is `!Send` precisely so it CAN hold that
//! `Rc` (as a `Weak`) directly — it always addresses the one lane it was minted
//! from, so nothing needs to look up "the" active lane on the calling thread, and
//! there is no other lane it could be confused with. The frame drive drains a
//! lane by receiving it as an explicit parameter
//! ([`UpdateScheduler::end_frame_with_lane`] and friends), never by reading
//! ambient state.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::{CallbackId, FrameTiming, PostFrameCallback, UpdateScheduler, WeakUpdateScheduler};

pub(crate) type OwnerPostFrameCallback = Box<dyn FnOnce(&FrameTiming) + 'static>;

pub(crate) struct LocalPostFrameEntry {
    pub(crate) id: CallbackId,
    pub(crate) callback: OwnerPostFrameCallback,
}

struct LocalLaneInner {
    queue: RefCell<Vec<LocalPostFrameEntry>>,
}

/// Owner-affine queue for post-frame callbacks that are not required to be `Send`.
///
/// This runtime-internal type is public only because bindings live in sibling
/// crates. It is intentionally absent from the prelude and structurally
/// `!Send + !Sync` through its `Rc` storage. `Clone` shares the same
/// underlying queue (an `Rc` clone, not a fresh lane) — the same shape
/// `UpdateScheduler`'s own `Clone` has over its `Arc`.
#[derive(Clone)]
#[doc(hidden)]
pub struct LocalPostFrameLane {
    scheduler: WeakUpdateScheduler,
    inner: Rc<LocalLaneInner>,
}

impl LocalPostFrameLane {
    pub(crate) fn new(scheduler: &UpdateScheduler) -> Self {
        Self {
            scheduler: scheduler.downgrade(),
            inner: Rc::new(LocalLaneInner {
                queue: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Create a `!Send` handle addressed directly at this lane's storage.
    ///
    /// The returned handle never needs "is this lane active" resolution: it
    /// holds a `Weak` pointer straight at [`LocalLaneInner`], so scheduling
    /// through it always reaches the correct queue, regardless of what else is
    /// happening on the owner thread.
    #[must_use]
    pub fn local_handle(&self) -> LocalPostFrameHandle {
        LocalPostFrameHandle {
            scheduler: self.scheduler.clone(),
            lane: Rc::downgrade(&self.inner),
        }
    }

    /// Drain this lane's queue, unconditionally. Private: every caller must
    /// go through [`take_queue_for`](Self::take_queue_for), which verifies
    /// the lane actually belongs to the scheduler asking to drain it before
    /// ever reaching this.
    fn take_queue(&self) -> Vec<LocalPostFrameEntry> {
        self.inner.queue.take()
    }

    /// Return an uninvoked tail to the already-validated source lane.
    /// Original IDs retain FIFO order ahead of newer registrations on the next drain.
    pub(crate) fn restore_queue(&self, entries: Vec<LocalPostFrameEntry>) {
        self.inner.queue.borrow_mut().extend(entries);
    }

    /// Drain this lane's queue for `scheduler`'s frame drive — but only if
    /// `scheduler` is genuinely the one this lane was minted from.
    ///
    /// Called with the lane passed explicitly by [`UpdateScheduler::end_frame_with_lane`]
    /// (and the `execute_frame_with_lane`/`drive_frame_with_lane` convenience
    /// wrappers) — drain-by-parameter, never an ambient lookup. The identity
    /// check restores what the retired thread-local ticket registry's
    /// `scheduler_identity` filter used to guarantee: a lane minted by
    /// scheduler B, handed by mistake to scheduler A's drive, must not be
    /// drained by A. Draining it anyway would hand B's callbacks A's
    /// `FrameTiming`, remove them before B's own frame ever runs them, and
    /// interleave B's `CallbackId`s — which mean nothing in A's registration
    /// sequence — into A's sort, corrupting the one-total-order guarantee
    /// this slice exists to provide. On mismatch (or a since-dropped owning
    /// scheduler) this returns `Err` and leaves the lane's queue untouched,
    /// still there for its own scheduler's next drive.
    pub(crate) fn take_queue_for(
        &self,
        scheduler: &UpdateScheduler,
    ) -> Result<Vec<LocalPostFrameEntry>, LocalPostFrameScheduleError> {
        let Some(owner) = self.scheduler.upgrade() else {
            tracing::error!(
                driving_scheduler = scheduler.debug_ptr(),
                "a LocalPostFrameLane's owning scheduler is already gone; refusing to \
                 drain it from this (necessarily unrelated) frame drive"
            );
            return Err(LocalPostFrameScheduleError::LaneClosed);
        };
        if !owner.is_same_instance(scheduler) {
            tracing::error!(
                lane_owner_scheduler = owner.debug_ptr(),
                driving_scheduler = scheduler.debug_ptr(),
                "a LocalPostFrameLane was handed to a frame drive on a scheduler that does \
                 not own it; refusing to drain it — its own scheduler's next drive still \
                 delivers it"
            );
            return Err(LocalPostFrameScheduleError::WrongScheduler);
        }
        Ok(self.take_queue())
    }
}

impl std::fmt::Debug for LocalPostFrameLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostFrameLane")
            .field("pending", &self.inner.queue.borrow().len())
            .finish_non_exhaustive()
    }
}

/// Why an owner-local post-frame callback could not be registered, or a
/// lane's queue could not be drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocalPostFrameScheduleError {
    /// The owning [`LocalPostFrameLane`] (or its backing scheduler) has
    /// already been dropped — there is no frame left for the callback to
    /// observe, and it is guaranteed never to run.
    #[error("the handle's owner-local lane is closed")]
    LaneClosed,
    /// A [`LocalPostFrameLane`] was handed to a frame drive
    /// (`end_frame_with_lane`/`execute_frame_with_lane`/`drive_frame_with_lane`)
    /// on a different `UpdateScheduler` than the one it was minted from.
    /// Draining it anyway would hand its callbacks a foreign frame's
    /// `FrameTiming`, remove them before their own scheduler's frame ever
    /// runs, and interleave `CallbackId`s from a different id sequence into
    /// a sort where they carry no meaning — so the drive refuses to drain it
    /// at all. The lane's queue is untouched: its own scheduler's next drive
    /// still delivers it.
    #[error("the lane belongs to a different UpdateScheduler than the one draining it")]
    WrongScheduler,
}

/// Schedules owner-local work after a completed frame's layout and paint.
///
/// `!Send`: holds a [`Weak`] pointer directly at its lane's `Rc` storage, so it
/// can capture non-`Send` state (`Rc`/`RefCell`) in the callbacks it schedules.
/// Moving one to another thread is a compile error, not a runtime check — see
/// the module docs for why that structural guarantee replaces the older
/// thread-local ticket registry outright rather than augmenting it.
#[derive(Clone)]
pub struct LocalPostFrameHandle {
    scheduler: WeakUpdateScheduler,
    lane: Weak<LocalLaneInner>,
}

impl LocalPostFrameHandle {
    /// Schedule an owner-local callback after the next completed frame.
    ///
    /// The callback may capture `Rc`/`RefCell` state. On error (the owning
    /// lane or its scheduler is gone) the callback is dropped without
    /// running — provably: nothing retains it once this call returns `Err`.
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

    /// The number of owner-local callbacks queued on this handle's lane and
    /// not yet drained by a completed frame; `0` once the lane is closed.
    ///
    /// A self-rescheduling callback (one that queues its successor from
    /// inside its own run) keeps this at `1` after every frame, so a reading
    /// of `0` after a frame is how an owner proves such a loop has stopped.
    ///
    /// A test probe, not part of the documented API: no production path reads
    /// it. It exists so an owner crate's integration tests can observe the
    /// lane without a test-only global counter.
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
    assert_not_impl_any!(LocalPostFrameLane: Send, Sync);
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
        let lane = scheduler.new_local_post_frame_lane();
        lane.local_handle()
            .schedule_local(|_| panic!("post-frame probe"))
            .expect("lane alive");
        assert!(
            catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame_with_lane(&lane))).is_err()
        );
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        let fired = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired);
        lane.local_handle()
            .schedule_local(move |_| callback.set(true))
            .expect("gate remains usable");
        scheduler.execute_frame_with_lane(&lane);
        assert!(fired.get());
    }

    fn post_frame_panic_preserves_uninvoked_mixed_tail_before_reentrant_work() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.new_local_post_frame_lane();
        let log = Arc::new(Mutex::new(Vec::new()));
        let panic_calls = Rc::new(Cell::new(0));
        let counted = panic_calls.clone();
        let reentrant = lane.local_handle();
        let reentrant_log = log.clone();
        lane.local_handle()
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
        lane.local_handle()
            .schedule_local(move |_| {
                local_log.lock().expect("log").push(2);
            })
            .expect("lane alive");

        let (panicked, first_batch) = flui_testing::log_capture::capture(|| {
            catch_unwind(AssertUnwindSafe(|| {
                scheduler.execute_frame_with_lane(&lane)
            }))
        });
        assert!(panicked.is_err());
        assert_batch_depth(&first_batch, 1, 2, 3);
        assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
        assert!(
            log.lock().expect("log").is_empty(),
            "the poisoned frame stops delivery"
        );
        let (_retry_frame, retry_batch) =
            flui_testing::log_capture::capture(|| scheduler.execute_frame_with_lane(&lane));
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
            ],
        );
    }
}
