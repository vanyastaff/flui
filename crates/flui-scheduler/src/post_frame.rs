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
        let lane = scheduler.new_local_post_frame_lane();
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
        let lane = scheduler.new_local_post_frame_lane();
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
        let lane = scheduler.new_local_post_frame_lane();
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
        let lane_a = scheduler.new_local_post_frame_lane();
        let lane_b = scheduler.new_local_post_frame_lane();
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

    /// The weak case: A is driven with NO lane parameter at all, so it never
    /// even looks at B's lane. `draining_the_wrong_schedulers_lane_leaves_it_untouched`
    /// below is the strong case this one does not cover — B's lane handed to
    /// A's drive AS THE PARAMETER, which is the actual misuse
    /// `take_queue_for`'s identity check exists to refuse.
    #[test]
    fn other_scheduler_lane_is_untouched_by_a_different_scheduler_drive() {
        let scheduler_a = UpdateScheduler::new();
        let scheduler_b = UpdateScheduler::new();
        let lane_b = scheduler_b.new_local_post_frame_lane();
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

    /// The retired thread-local ticket registry filtered a lane's drain by
    /// `scheduler_identity`; `take_queue_for` restores that check for the
    /// direct-parameter shape the new design uses. With per-realm schedulers
    /// now real (every `UiRealm` mints its own via `new_local_post_frame_lane`),
    /// a binding wiring the wrong realm's lane into a drive call is a
    /// reachable mistake, not a theoretical one — this is the exact
    /// misuse `other_scheduler_lane_is_untouched_by_a_different_scheduler_drive`
    /// above does not exercise (that one never hands B's lane to A's drive at
    /// all).
    ///
    /// Mutant this kills: dropping the `is_same_instance` check from
    /// `take_queue_for` (or calling the private `take_queue` directly, as the
    /// pre-fix code did) — B's callback would run inside A's frame, with A's
    /// `FrameTiming`, be removed from B's queue, and never fire again.
    #[test]
    fn draining_the_wrong_schedulers_lane_leaves_it_untouched() {
        let scheduler_a = UpdateScheduler::new();
        let scheduler_b = UpdateScheduler::new();
        let lane_b = scheduler_b.new_local_post_frame_lane();
        let fired_b = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired_b);
        lane_b
            .local_handle()
            .schedule_local(move |_| callback.set(true))
            .expect("lane alive");

        // A's own shared-queue callback, so the exploit can also prove the
        // mismatch does not collaterally break A's real work.
        let fired_a = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cb_a = Arc::clone(&fired_a);
        scheduler_a.add_post_frame_callback(Box::new(move |_| {
            cb_a.store(true, std::sync::atomic::Ordering::SeqCst);
        }));

        // The mistake: hand B's lane to A's drive.
        scheduler_a.execute_frame_with_lane(&lane_b);
        assert!(!fired_b.get(), "B's callback must NOT run inside A's frame");
        assert!(
            fired_a.load(std::sync::atomic::Ordering::SeqCst),
            "A's own shared-queue callback must be unaffected by the mismatch"
        );

        // Not just "didn't run" — still there, unremoved: B's own next frame
        // must still deliver it.
        scheduler_b.execute_frame_with_lane(&lane_b);
        assert!(
            fired_b.get(),
            "B's own frame must still run its own lane's callback \
             (proves the entry was refused, not silently dropped)"
        );
    }

    /// The mismatch is a typed, observable error — not merely an absence of
    /// effect — and the correct scheduler is unaffected by a prior failed
    /// attempt from the wrong one.
    #[test]
    fn take_queue_for_the_wrong_scheduler_is_a_typed_error() {
        let scheduler_a = UpdateScheduler::new();
        let scheduler_b = UpdateScheduler::new();
        let lane_b = scheduler_b.new_local_post_frame_lane();

        assert!(matches!(
            lane_b.take_queue_for(&scheduler_a),
            Err(LocalPostFrameScheduleError::WrongScheduler)
        ));

        lane_b
            .local_handle()
            .schedule_local(|_| {})
            .expect("lane alive");
        assert!(
            lane_b.take_queue_for(&scheduler_b).is_ok(),
            "the failed attempt from the wrong scheduler must not have corrupted the lane"
        );
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
        let lane = scheduler.new_local_post_frame_lane();
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
        let lane = scheduler.new_local_post_frame_lane();
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

    #[test]
    fn aborted_pipeline_retains_local_callback_for_next_completed_frame() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.new_local_post_frame_lane();
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

    /// `end_frame_with_lane` on a scheduler with no open frame must not
    /// silently drop the lane's queued entries. The mutant this kills: taking
    /// `lane.take_queue()` unconditionally, ahead of the `if let Some(timing)`
    /// guard, instead of inside it — the taken entries would then never be
    /// folded into `callbacks` (that only happens inside the same
    /// `Some(timing)` arm) and would be dropped, un-run, when the call
    /// returns.
    ///
    /// `#[cfg(not(debug_assertions))]`, deliberately: reaching this call with
    /// no open frame ALSO means the current phase cannot legally transition
    /// to `PostFrameCallbacks` (`current_frame` and the phase machine are
    /// always advanced together — see `handle_begin_frame`/`handle_draw_frame`
    /// — so the only way to decouple them is a misuse call, e.g. a second
    /// `end_frame_with_lane` after the frame it was meant to close already
    /// closed). `set_scheduler_phase`'s own `debug_assert!` catches that
    /// misuse first in a debug/test build, panicking before this function
    /// ever reaches `current_frame.lock().take()` — which is exactly why the
    /// reviewer who asked for this test also called the bug "not reachable
    /// today". It IS reachable the moment `debug_assertions` are off (a
    /// release build, where that assert compiles out), which is what this
    /// test pins; it cannot run under the debug-assertions-on profile this
    /// workspace tests with, and is gated accordingly rather than pretending
    /// otherwise.
    #[test]
    #[cfg(not(debug_assertions))]
    fn end_frame_with_lane_without_an_open_frame_does_not_drop_the_queued_entry() {
        let scheduler = UpdateScheduler::new();
        let lane = scheduler.new_local_post_frame_lane();
        let fired = Rc::new(Cell::new(false));
        let callback = Rc::clone(&fired);
        lane.local_handle()
            .schedule_local(move |_| callback.set(true))
            .expect("lane alive");

        // No frame is open: nothing runs, and nothing must be lost.
        scheduler.end_frame_with_lane(&lane);
        assert!(!fired.get(), "no open frame means nothing to close");

        // The entry must still be queued for the next REAL frame close.
        scheduler.execute_frame_with_lane(&lane);
        assert!(
            fired.get(),
            "the queued entry must survive a no-open-frame end_frame_with_lane call"
        );
    }
}
