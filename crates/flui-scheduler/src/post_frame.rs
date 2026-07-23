//! Binding-scoped post-frame capabilities.
//!
//! Shared callbacks remain `Send` and live in the scheduler's synchronized queue.
//! Owner-local callbacks live in [`LocalPostFrameLane`]'s `Rc` queue and are only
//! reachable while that lane is active on its owner thread. Handles carry a
//! Send-safe identity ticket, never an `Rc` or a non-`Send` callback.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, ThreadId};

use crate::{CallbackId, FrameTiming, PostFrameCallback, Scheduler};

static NEXT_LANE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LaneId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LaneTicket {
    lane_id: LaneId,
    owner: ThreadId,
    scheduler_identity: u64,
}

pub(crate) type OwnerPostFrameCallback = Box<dyn FnOnce(&FrameTiming) + 'static>;

pub(crate) struct LocalPostFrameEntry {
    pub(crate) id: CallbackId,
    pub(crate) callback: OwnerPostFrameCallback,
}

struct LocalLaneInner {
    ticket: LaneTicket,
    scheduler: Scheduler,
    queue: RefCell<Vec<LocalPostFrameEntry>>,
}

thread_local! {
    static LOCAL_LANES: RefCell<HashMap<LaneId, Weak<LocalLaneInner>>> =
        RefCell::new(HashMap::new());
    static ACTIVE_LANES: RefCell<Vec<LaneTicket>> = const { RefCell::new(Vec::new()) };
}

/// Owner-affine queue for post-frame callbacks that are not required to be `Send`.
///
/// This runtime-internal type is public only because bindings live in sibling
/// crates. It is intentionally absent from the prelude and structurally
/// `!Send + !Sync` through its `Rc` storage.
#[doc(hidden)]
pub struct LocalPostFrameLane {
    inner: Rc<LocalLaneInner>,
}

impl LocalPostFrameLane {
    pub(crate) fn new(scheduler: &Scheduler) -> Self {
        let lane_id = LaneId(
            NEXT_LANE_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
                .expect("BUG: LocalPostFrameLane identity space exhausted"),
        );
        let inner = Rc::new(LocalLaneInner {
            ticket: LaneTicket {
                lane_id,
                owner: thread::current().id(),
                scheduler_identity: scheduler.identity(),
            },
            scheduler: scheduler.clone(),
            queue: RefCell::new(Vec::new()),
        });
        LOCAL_LANES.with(|registry| {
            registry.borrow_mut().insert(lane_id, Rc::downgrade(&inner));
        });
        Self { inner }
    }

    /// Create a `Send + Sync` handle carrying this lane's identity ticket.
    #[must_use]
    pub fn post_frame_handle(&self) -> PostFrameHandle {
        PostFrameHandle {
            scheduler: self.inner.scheduler.clone(),
            local_lane: Some(self.inner.ticket),
        }
    }

    /// Activate this lane for the dynamic extent of `callback`.
    pub fn enter<R>(&self, callback: impl FnOnce() -> R) -> R {
        ACTIVE_LANES.with(|active| active.borrow_mut().push(self.inner.ticket));
        let _activation = LocalLaneActivation {
            ticket: self.inner.ticket,
            _borrow: PhantomData,
        };
        callback()
    }
}

impl std::fmt::Debug for LocalPostFrameLane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalPostFrameLane")
            .field("lane_id", &self.inner.ticket.lane_id)
            .field("owner", &self.inner.ticket.owner)
            .finish_non_exhaustive()
    }
}

impl Drop for LocalPostFrameLane {
    fn drop(&mut self) {
        let removed = LOCAL_LANES
            .try_with(|registry| registry.borrow_mut().remove(&self.inner.ticket.lane_id))
            .ok()
            .flatten();
        drop(removed);
        let queued = self.inner.queue.take();
        drop(queued);
    }
}

struct LocalLaneActivation<'lane> {
    ticket: LaneTicket,
    _borrow: PhantomData<&'lane LocalPostFrameLane>,
}

impl Drop for LocalLaneActivation<'_> {
    fn drop(&mut self) {
        let _ = ACTIVE_LANES.try_with(|active| {
            let popped = active.borrow_mut().pop();
            if popped != Some(self.ticket) {
                tracing::error!("local post-frame activation stack was not LIFO");
            }
        });
    }
}

/// Why an owner-local post-frame callback could not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LocalPostFrameScheduleError {
    /// This handle only supports `Send` callbacks.
    #[error("this PostFrameHandle has no owner-local lane")]
    NoLocalLane,
    /// The handle was used away from its lane's owner thread.
    #[error("owner-local post-frame callback scheduled from the wrong thread")]
    WrongThread,
    /// Another lane is active, or this lane is not currently active.
    #[error("the handle's owner-local lane is not the active lane")]
    InactiveLane,
    /// The owning lane has already been dropped.
    #[error("the handle's owner-local lane is closed")]
    LaneClosed,
}

/// Schedules work after a completed frame's layout and paint.
#[derive(Clone)]
pub struct PostFrameHandle {
    scheduler: Scheduler,
    local_lane: Option<LaneTicket>,
}

impl PostFrameHandle {
    /// Construct a handle for `Send` post-frame callbacks.
    #[must_use]
    pub fn new(scheduler: &Scheduler) -> Self {
        Self {
            scheduler: scheduler.clone(),
            local_lane: None,
        }
    }

    /// Schedule a `Send` callback after the next completed frame.
    pub fn schedule(&self, callback: impl FnOnce(&FrameTiming) + Send + 'static) {
        let boxed: PostFrameCallback = Box::new(callback);
        self.scheduler.add_post_frame_callback(boxed);
    }

    /// Schedule an owner-local callback after the next completed frame.
    ///
    /// The callback may capture `Rc`/`RefCell` state. Registration succeeds only
    /// while this handle's lane is the active top scope on its owner thread. On
    /// error the callback is dropped without running; stale handles cannot
    /// recreate a lane.
    pub fn schedule_local(
        &self,
        callback: impl FnOnce(&FrameTiming) + 'static,
    ) -> Result<(), LocalPostFrameScheduleError> {
        let ticket = self
            .local_lane
            .ok_or(LocalPostFrameScheduleError::NoLocalLane)?;
        if thread::current().id() != ticket.owner {
            return Err(LocalPostFrameScheduleError::WrongThread);
        }
        let lane = LOCAL_LANES.with(|registry| {
            registry
                .borrow()
                .get(&ticket.lane_id)
                .and_then(Weak::upgrade)
        });
        let Some(lane) = lane else {
            return Err(LocalPostFrameScheduleError::LaneClosed);
        };
        let is_active = ACTIVE_LANES.with(|active| active.borrow().last().copied() == Some(ticket));
        if !is_active {
            return Err(LocalPostFrameScheduleError::InactiveLane);
        }
        self.scheduler.with_post_frame_registration(|id| {
            lane.queue.borrow_mut().push(LocalPostFrameEntry {
                id,
                callback: Box::new(callback),
            });
        });
        Ok(())
    }

    /// Whether this handle targets `other`.
    #[must_use]
    pub fn targets_same_scheduler(&self, other: &Scheduler) -> bool {
        self.scheduler.is_same_instance(other)
    }
}

impl std::fmt::Debug for PostFrameHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostFrameHandle")
            .field("has_local_lane", &self.local_lane.is_some())
            .finish_non_exhaustive()
    }
}

pub(crate) fn drain_active_lane(scheduler_identity: u64) -> Vec<LocalPostFrameEntry> {
    let ticket = ACTIVE_LANES.with(|active| active.borrow().last().copied());
    let Some(ticket) = ticket.filter(|ticket| ticket.scheduler_identity == scheduler_identity)
    else {
        return Vec::new();
    };
    let lane = LOCAL_LANES.with(|registry| {
        registry
            .borrow()
            .get(&ticket.lane_id)
            .and_then(Weak::upgrade)
    });
    lane.map_or_else(Vec::new, |lane| lane.queue.take())
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

    assert_impl_all!(Scheduler: Send, Sync);
    assert_impl_all!(PostFrameHandle: Send, Sync);
    assert_not_impl_any!(LocalPostFrameLane: Send, Sync);

    #[test]
    fn mixed_shared_and_local_callbacks_keep_total_registration_order() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let log = Arc::new(Mutex::new(Vec::new()));

        lane.enter(|| {
            let shared = Arc::clone(&log);
            scheduler.add_post_frame_callback(Box::new(move |_| {
                shared.lock().expect("log mutex").push(1);
            }));
            let local = Arc::clone(&log);
            lane.post_frame_handle()
                .schedule_local(move |_| local.lock().expect("log mutex").push(2))
                .expect("lane is active");
            let shared = Arc::clone(&log);
            scheduler.add_post_frame_callback(Box::new(move |_| {
                shared.lock().expect("log mutex").push(3);
            }));
            scheduler.execute_frame();
        });

        assert_eq!(*log.lock().expect("log mutex"), [1, 2, 3]);
    }

    #[test]
    fn shared_then_local_nested_registration_defers() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let handle = lane.post_frame_handle();
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        lane.enter(|| {
            let nested = Arc::clone(&fired);
            scheduler.add_post_frame_callback(Box::new(move |_| {
                handle
                    .schedule_local(move |_| {
                        nested.fetch_add(1, Ordering::SeqCst);
                    })
                    .expect("lane remains active for the whole frame");
            }));
            scheduler.execute_frame();
            assert_eq!(fired.load(Ordering::SeqCst), 0);
            scheduler.execute_frame();
        });
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn local_then_shared_nested_registration_defers() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        lane.enter(|| {
            let nested_scheduler = scheduler.clone();
            let nested = Arc::clone(&fired);
            lane.post_frame_handle()
                .schedule_local(move |_| {
                    nested_scheduler.add_post_frame_callback(Box::new(move |_| {
                        nested.fetch_add(1, Ordering::SeqCst);
                    }));
                })
                .expect("lane is active");
            scheduler.execute_frame();
            assert_eq!(fired.load(Ordering::SeqCst), 0);
            scheduler.execute_frame();
        });
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn active_lane_and_scheduler_identity_isolate_local_queues() {
        let scheduler_a = Scheduler::new();
        let scheduler_b = Scheduler::new();
        let lane_a = scheduler_a.local_post_frame_lane();
        let lane_b = scheduler_a.local_post_frame_lane();
        let lane_other_scheduler = scheduler_b.local_post_frame_lane();
        let fired_a = Rc::new(Cell::new(0));
        let fired_b = Rc::new(Cell::new(0));
        let fired_other = Rc::new(Cell::new(0));

        lane_a.enter(|| {
            let fired = Rc::clone(&fired_a);
            lane_a
                .post_frame_handle()
                .schedule_local(move |_| fired.set(1))
                .expect("lane A active");
        });
        lane_b.enter(|| {
            let fired = Rc::clone(&fired_b);
            lane_b
                .post_frame_handle()
                .schedule_local(move |_| fired.set(1))
                .expect("lane B active");
            scheduler_a.execute_frame();
        });
        assert_eq!(fired_a.get(), 0);
        assert_eq!(fired_b.get(), 1);

        lane_other_scheduler.enter(|| {
            let fired = Rc::clone(&fired_other);
            lane_other_scheduler
                .post_frame_handle()
                .schedule_local(move |_| fired.set(1))
                .expect("other scheduler lane active");
            scheduler_a.execute_frame();
            assert_eq!(fired_other.get(), 0);
            scheduler_b.execute_frame();
        });
        assert_eq!(fired_other.get(), 1);
    }

    #[test]
    fn nested_different_lane_rejects_inactive_outer_ticket() {
        let scheduler = Scheduler::new();
        let lane_a = scheduler.local_post_frame_lane();
        let lane_b = scheduler.local_post_frame_lane();
        let handle_a = lane_a.post_frame_handle();
        lane_a.enter(|| {
            lane_b.enter(|| {
                assert_eq!(
                    handle_a.schedule_local(|_| {}),
                    Err(LocalPostFrameScheduleError::InactiveLane)
                );
            });
        });
    }

    #[test]
    fn wrong_thread_and_stale_lane_are_typed_errors() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let handle = lane.post_frame_handle();
        let threaded = handle.clone();
        let wrong_thread = std::thread::spawn(move || threaded.schedule_local(|_| {}))
            .join()
            .expect("worker should not panic");
        assert_eq!(wrong_thread, Err(LocalPostFrameScheduleError::WrongThread));
        drop(lane);
        assert_eq!(
            handle.schedule_local(|_| {}),
            Err(LocalPostFrameScheduleError::LaneClosed)
        );
    }

    #[test]
    fn missing_lane_is_a_typed_error() {
        let scheduler = Scheduler::new();
        assert_eq!(
            PostFrameHandle::new(&scheduler).schedule_local(|_| {}),
            Err(LocalPostFrameScheduleError::NoLocalLane)
        );
    }

    #[test]
    fn dropping_lane_drops_queued_owner_capture() {
        struct DropProbe(Rc<Cell<bool>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let dropped = Rc::new(Cell::new(false));
        lane.enter(|| {
            let probe = DropProbe(Rc::clone(&dropped));
            lane.post_frame_handle()
                .schedule_local(move |_| drop(probe))
                .expect("lane active");
        });
        drop(lane);
        assert!(dropped.get());
    }

    #[test]
    fn post_frame_panic_restores_idle_and_later_scheduling_works() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        lane.enter(|| {
            lane.post_frame_handle()
                .schedule_local(|_| panic!("post-frame probe"))
                .expect("lane active");
            assert!(catch_unwind(AssertUnwindSafe(|| scheduler.execute_frame())).is_err());
            assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
            let fired = Rc::new(Cell::new(false));
            let callback = Rc::clone(&fired);
            lane.post_frame_handle()
                .schedule_local(move |_| callback.set(true))
                .expect("gate remains usable");
            scheduler.execute_frame();
            assert!(fired.get());
        });
    }

    #[test]
    fn aborted_pipeline_retains_local_callback_for_next_completed_frame() {
        let scheduler = Scheduler::new();
        let lane = scheduler.local_post_frame_lane();
        let fired = Rc::new(Cell::new(false));
        lane.enter(|| {
            let callback = Rc::clone(&fired);
            lane.post_frame_handle()
                .schedule_local(move |_| callback.set(true))
                .expect("lane active");
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    scheduler.drive_frame(crate::Instant::now(), || panic!("pipeline probe"));
                }))
                .is_err()
            );
            assert!(!fired.get());
            scheduler.execute_frame();
        });
        assert!(fired.get());
    }
}
