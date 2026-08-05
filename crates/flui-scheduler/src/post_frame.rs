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

    /// Drain this lane's queue for the frame drive.
    ///
    /// Called with the lane passed explicitly by [`UpdateScheduler::end_frame_with_lane`]
    /// (and the `execute_frame_with_lane`/`drive_frame_with_lane` convenience
    /// wrappers) — drain-by-parameter, never an ambient lookup.
    pub(crate) fn take_queue(&self) -> Vec<LocalPostFrameEntry> {
        self.inner.queue.take()
    }
}

impl std::fmt::Debug for LocalPostFrameLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostFrameLane")
            .field("pending", &self.inner.queue.borrow().len())
            .finish_non_exhaustive()
    }
}

/// Why an owner-local post-frame callback could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocalPostFrameScheduleError {
    /// The owning [`LocalPostFrameLane`] (or its backing scheduler) has
    /// already been dropped — there is no frame left for the callback to
    /// observe, and it is guaranteed never to run.
    #[error("the handle's owner-local lane is closed")]
    LaneClosed,
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

    #[test]
    fn mixed_shared_and_local_callbacks_keep_total_registration_order() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let log = Arc::new(Mutex::new(Vec::new()));

        let shared = Arc::clone(&log);
        scheduler.add_post_frame_callback(Box::new(move |_| {
            shared.lock().expect("log mutex").push(1);
        }));
        let local = Arc::clone(&log);
        lane.local_handle()
            .schedule_local(move |_| local.lock().expect("log mutex").push(2))
            .expect("lane is alive");
        let shared = Arc::clone(&log);
        scheduler.add_post_frame_callback(Box::new(move |_| {
            shared.lock().expect("log mutex").push(3);
        }));
        scheduler.execute_frame_with_lane(&lane);

        assert_eq!(*log.lock().expect("log mutex"), [1, 2, 3]);
    }

    /// A local callback that re-registers another LOCAL callback (the direct
    /// same-lane case, distinct from `local_then_shared` below): the queue is
    /// taken from the lane before any callback in this frame's snapshot runs,
    /// so the re-registration lands in the lane's now-empty queue and fires
    /// only on the *next* drive, never in the frame that scheduled it. A
    /// `!Send` `LocalPostFrameHandle` cannot be captured into a `Send`-bound
    /// shared callback at all (a compile error, not a runtime check) — that
    /// structurally rules out the "shared schedules local" direction the
    /// thread-local ticket registry used to have to arbitrate at runtime.
    #[test]
    fn local_then_local_nested_registration_defers() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let handle = lane.local_handle();
        let fired = Rc::new(Cell::new(0));
        let nested_handle = handle.clone();
        let nested = Rc::clone(&fired);
        handle
            .schedule_local(move |_| {
                nested_handle
                    .schedule_local(move |_| {
                        nested.set(nested.get() + 1);
                    })
                    .expect("lane outlives this frame");
            })
            .expect("lane alive");
        scheduler.execute_frame_with_lane(&lane);
        assert_eq!(fired.get(), 0, "re-registration must not run this frame");
        scheduler.execute_frame_with_lane(&lane);
        assert_eq!(fired.get(), 1, "it must run on the very next frame");
    }

    #[test]
    fn local_then_shared_nested_registration_defers() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let nested_scheduler = scheduler.clone();
        let nested = Arc::clone(&fired);
        lane.local_handle()
            .schedule_local(move |_| {
                nested_scheduler.add_post_frame_callback(Box::new(move |_| {
                    nested.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }));
            })
            .expect("lane is alive");
        scheduler.execute_frame_with_lane(&lane);
        assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 0);
        scheduler.execute_frame_with_lane(&lane);
        assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Two lanes on the same scheduler, both with handles held concurrently:
    /// each handle addresses its own lane directly (a `Weak` straight at its
    /// `LocalLaneInner`), so there is no "active lane" ambiguity to resolve —
    /// this is the property that let the thread-local ticket registry retire.
    #[test]
    fn concurrent_lanes_on_one_scheduler_never_cross_queues() {
        let scheduler = UpdateScheduler::new();
        let lane_a = scheduler.local_post_frame_lane();
        let lane_b = scheduler.local_post_frame_lane();
        let fired_a = Rc::new(Cell::new(0));
        let fired_b = Rc::new(Cell::new(0));

        let fired = Rc::clone(&fired_a);
        lane_a
            .local_handle()
            .schedule_local(move |_| fired.set(1))
            .expect("lane A alive");
        let fired = Rc::clone(&fired_b);
        lane_b
            .local_handle()
            .schedule_local(move |_| fired.set(1))
            .expect("lane B alive");

        // Draining lane A must not touch lane B's still-queued callback.
        scheduler.execute_frame_with_lane(&lane_a);
        assert_eq!(fired_a.get(), 1);
        assert_eq!(fired_b.get(), 0);

        scheduler.execute_frame_with_lane(&lane_b);
        assert_eq!(fired_b.get(), 1);
    }

    #[test]
    fn other_scheduler_lane_is_untouched_by_a_different_scheduler_drive() {
        let scheduler_a = UpdateScheduler::new();
        let scheduler_b = UpdateScheduler::new();
        let lane_b = scheduler_b.local_post_frame_lane();
        let fired = Rc::new(Cell::new(0));

        let probe = Rc::clone(&fired);
        lane_b
            .local_handle()
            .schedule_local(move |_| probe.set(1))
            .expect("lane B alive");

        // Scheduler A completing a frame has no lane of its own here and must
        // not observe lane B's queue.
        scheduler_a.execute_frame();
        assert_eq!(fired.get(), 0);

        scheduler_b.execute_frame_with_lane(&lane_b);
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn dropped_lane_is_a_typed_error_and_the_callback_never_runs() {
        struct DropProbe(Rc<Cell<bool>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let handle = lane.local_handle();
        let dropped = Rc::new(Cell::new(false));
        let ran = Rc::new(Cell::new(false));

        let probe = DropProbe(Rc::clone(&dropped));
        let ran_flag = Rc::clone(&ran);
        handle
            .schedule_local(move |_| {
                ran_flag.set(true);
                drop(probe);
            })
            .expect("lane alive");

        drop(lane);
        assert!(dropped.get(), "queued capture must drop with its dead lane");

        // The handle itself now fails closed — a typed error, not a panic or
        // silent success.
        assert_eq!(
            handle.schedule_local(|_| {}),
            Err(LocalPostFrameScheduleError::LaneClosed)
        );
        // And the original callback is provably never invoked: it was
        // dropped, not run, when its lane died.
        assert!(!ran.get());
    }

    #[test]
    fn dead_scheduler_is_also_a_typed_error() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let handle = lane.local_handle();
        drop(scheduler);
        assert_eq!(
            handle.schedule_local(|_| {}),
            Err(LocalPostFrameScheduleError::LaneClosed)
        );
    }

    #[test]
    fn post_frame_panic_restores_idle_and_later_scheduling_works() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
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

    #[test]
    fn aborted_pipeline_retains_local_callback_for_next_completed_frame() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let fired = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired);
        lane.local_handle()
            .schedule_local(move |_| callback.set(true))
            .expect("lane alive");
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let now = crate::Instant::now();
                scheduler.drive_frame_with_lane(
                    now,
                    crate::IdleDeadline::far_future(now),
                    || panic!("pipeline probe"),
                    &lane,
                );
            }))
            .is_err()
        );
        assert!(!fired.get());
        scheduler.execute_frame_with_lane(&lane);
        assert!(fired.get());
    }
}
