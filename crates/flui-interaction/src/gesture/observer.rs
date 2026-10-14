//! Owner-level observation of arena decisions; the contract is on
//! [`GestureObserver`].

use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt,
    num::NonZeroU64,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    sync::Weak as SyncWeak,
};

use flui_foundation::IdExhausted;
use flui_platform_api::{EventTime, PlatformWindow};
use smallvec::SmallVec;

use crate::events::CancelReason;
use crate::gesture::arena::{CompositionKind, GestureArenaMember};
use crate::ids::PointerId;
use crate::retain::{Owned, Retain};

#[cfg(doc)]
use crate::gesture::arena::GestureArena;

/// Receives the decisions of the arena it is registered with through
/// [`GestureArena::observe`].
///
/// An observer receives every [`GestureObservation`] the arena commits: a
/// member joining a contest, the contest closing, a claim, each win and
/// rejection, a cancellation with its cause and a deadline that elapsed.
/// Observations describe decisions after the fact. Arena members still learn
/// their own verdicts through [`GestureArenaMember`]; an observer cannot change
/// one.
///
/// # Delivery
///
/// The arena queues an observation when it commits the decision and delivers
/// the queue after the operation that made it: at the end of the outermost
/// arena call, or, for a [`GestureBinding`](crate::GestureBinding)'s arena,
/// when the binding's outermost dispatch returns. No arena or binding borrow is
/// held while an observer runs, so an observer may call back into either.
///
/// - A pass delivers only the observations queued when it started. What an
///   observer causes reentrantly waits for the next pass; when the arena has a
///   window to wake, it asks that window for a frame, whose dispatch runs it.
/// - Every observer call is panic-contained. The observer stays registered and
///   later observations still reach it; the first failure of a pass is the one
///   reported (on the `flui_interaction::gesture::observer` tracing target),
///   and later ones are counted, never reported in its place.
/// - Each observation goes to the observers registered when its delivery
///   began. One replaced through [`ObserverHandle::replace`] or removed by
///   dropping its handle inside a callback stops receiving from the next
///   observation.
/// - An outgoing observer is destroyed after the call in flight returns, under
///   containment: a panic while destroying it is contained separately and
///   never replaces the first failure.
///
/// With no observer registered the arena records nothing and allocates nothing
/// for observation.
pub trait GestureObserver {
    /// One committed decision, delivered after the operation that made it.
    fn observe(&self, observation: &GestureObservation);
}

impl<F: Fn(&GestureObservation)> GestureObserver for F {
    fn observe(&self, observation: &GestureObservation) {
        self(observation);
    }
}

/// One contest of the arena: a pointer and the arena generation its Down
/// opened. A reused pointer ID opens a different contest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GestureContest {
    pointer: PointerId,
    generation: NonZeroU64,
}

impl GestureContest {
    pub(crate) fn new(pointer: PointerId, generation: u64) -> Self {
        Self {
            pointer,
            generation: NonZeroU64::new(generation).expect("BUG: arena generations start at 1"),
        }
    }

    /// The contact the contest is for.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }
}

/// The arena member an observation names, held weakly: observing a decision
/// never keeps a recognizer alive.
#[derive(Clone)]
pub struct ObservedMember(Weak<dyn GestureArenaMember>);

impl ObservedMember {
    pub(crate) fn new(member: Weak<dyn GestureArenaMember>) -> Self {
        Self(member)
    }

    /// Whether this is `member`, compared by allocation, never by type or
    /// label.
    #[must_use]
    pub fn is<M: ?Sized>(&self, member: &Rc<M>) -> bool {
        std::ptr::addr_eq(self.0.as_ptr(), Rc::as_ptr(member))
    }
}

impl fmt::Debug for ObservedMember {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObservedMember({:p})", self.0.as_ptr())
    }
}

/// Why a contest ended with no winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContestCancel {
    /// The contact was cancelled for this reason.
    Pointer(CancelReason),
    /// The contest was abandoned without a pointer Cancel: lifecycle loss or
    /// an explicit [`GestureArena::abandon`].
    Abandoned,
}

/// One decision of the arena, in the order the arena committed it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum GestureObservation {
    /// A member joined a contest.
    Joined {
        /// The contest joined.
        contest: GestureContest,
        /// The member.
        member: ObservedMember,
        /// The innermost composition the member joined under; `None` when it
        /// joined uncomposed. [`GestureArena::composition_path`] lists the
        /// enclosing ones.
        composition: Option<CompositionKind>,
    },
    /// The contest stopped admitting members.
    Closed {
        /// The contest closed.
        contest: GestureContest,
    },
    /// A current member claimed the contest. A claim made while the contest is
    /// open, or blocked by a composition, wins later or not at all.
    Claimed {
        /// The contest claimed.
        contest: GestureContest,
        /// The claimant.
        member: ObservedMember,
    },
    /// A member won the contest.
    Won {
        /// The contest won.
        contest: GestureContest,
        /// The winner.
        member: ObservedMember,
    },
    /// A member lost or withdrew from the contest.
    Rejected {
        /// The contest.
        contest: GestureContest,
        /// The rejected member.
        member: ObservedMember,
    },
    /// The contest ended without a winner; its members' rejections follow.
    Cancelled {
        /// The contest cancelled.
        contest: GestureContest,
        /// Why.
        cause: ContestCancel,
    },
    /// A member's deadline elapsed and the arena polled it; any decision it
    /// made follows.
    DeadlineElapsed {
        /// The contact whose contest the member was polled for.
        pointer: PointerId,
        /// The member polled.
        member: ObservedMember,
        /// When the arena polled it.
        now: EventTime,
    },
}

struct Registration {
    id: u64,
    observer: Rc<dyn GestureObserver>,
}

/// The arena's observers and its queue of undelivered observations.
#[derive(Default)]
pub(crate) struct ObserverHub {
    registrations: RefCell<Vec<Registration>>,
    queue: RefCell<VecDeque<GestureObservation>>,
    /// The last registration id issued; `u64::MAX` once exhausted.
    last_id: Cell<u64>,
    /// Owners (a dispatching binding) that defer delivery until they return.
    holds: Cell<usize>,
    delivering: Cell<bool>,
    /// Observers replaced or removed during a pass, destroyed after the call
    /// in flight.
    outgoing: RefCell<Vec<Rc<dyn GestureObserver>>>,
    wake: RefCell<Option<SyncWeak<dyn PlatformWindow>>>,
}

impl ObserverHub {
    /// Queue one observation if anyone observes; build nothing otherwise.
    pub(crate) fn record(&self, observation: impl FnOnce() -> GestureObservation) {
        if self.registrations.borrow().is_empty() {
            return;
        }
        let observation = observation();
        self.queue.borrow_mut().push_back(observation);
    }

    pub(crate) fn set_wake(&self, window: SyncWeak<dyn PlatformWindow>) {
        *self.wake.borrow_mut() = Some(window);
    }

    #[cfg(feature = "testing")]
    pub(crate) fn set_last_id(&self, last: u64) {
        self.last_id.set(last);
    }

    pub(crate) fn register(
        self: &Rc<Self>,
        observer: Rc<dyn GestureObserver>,
    ) -> Result<ObserverHandle, IdExhausted> {
        let id = self.last_id.get().checked_add(1).ok_or(IdExhausted)?;
        self.last_id.set(id);
        self.registrations
            .borrow_mut()
            .push(Registration { id, observer });
        Ok(ObserverHandle {
            hub: Rc::downgrade(self),
            id,
        })
    }

    /// Defer delivery until the returned guard drops; the owner then calls
    /// [`Self::deliver`].
    pub(crate) fn hold(&self) -> DeliveryHold<'_> {
        self.holds.set(self.holds.get() + 1);
        DeliveryHold { hub: self }
    }

    fn take_out(&self, id: u64, replacement: Option<Rc<dyn GestureObserver>>) {
        let outgoing = {
            let mut registrations = self.registrations.borrow_mut();
            let Some(index) = registrations.iter().position(|entry| entry.id == id) else {
                drop(registrations);
                Retire::new().retire(replacement);
                return;
            };
            match replacement {
                Some(observer) => std::mem::replace(&mut registrations[index].observer, observer),
                None => registrations.remove(index).observer,
            }
        };
        if self.registrations.borrow().is_empty() {
            // Nobody is left to receive the queued tail; its members are weak.
            drop(std::mem::take(&mut *self.queue.borrow_mut()));
        }
        if self.delivering.get() {
            self.outgoing.borrow_mut().push(outgoing);
        } else {
            Retire::new().retire(Some(outgoing));
        }
    }

    /// Run one delivery pass unless an owner holds delivery or a pass is
    /// already running.
    pub(crate) fn deliver(&self) {
        if self.delivering.get() || self.holds.get() != 0 || self.queue.borrow().is_empty() {
            return;
        }
        self.delivering.set(true);
        let mut failures = Failures::default();
        let retire = Retire::new();
        let batch = self.queue.borrow().len();
        for _ in 0..batch {
            let Some(observation) = self.queue.borrow_mut().pop_front() else {
                break;
            };
            let observers: SmallVec<[Rc<dyn GestureObserver>; 4]> = self
                .registrations
                .borrow()
                .iter()
                .map(|entry| Rc::clone(&entry.observer))
                .collect();
            for observer in observers {
                if let Err(payload) =
                    catch_unwind(AssertUnwindSafe(|| observer.observe(&observation)))
                {
                    failures.capture(payload);
                }
                retire.retire(Some(observer));
                let outgoing = std::mem::take(&mut *self.outgoing.borrow_mut());
                for observer in outgoing {
                    retire.retire(Some(observer));
                }
            }
        }
        // Diagnostics are user code too: report while the pass still owns
        // delivery, so an observation they cause waits for the next pass.
        failures.report(&retire);
        self.delivering.set(false);
        if !self.queue.borrow().is_empty() {
            self.request_wake();
        }
    }

    fn request_wake(&self) {
        let window = self.wake.borrow().as_ref().and_then(SyncWeak::upgrade);
        if let Some(window) = window {
            let woke = catch_unwind(AssertUnwindSafe(|| window.request_redraw()));
            match woke {
                Ok(()) => drop(window),
                Err(payload) => {
                    Owned(window).retain();
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
        }
    }
}

/// Defers a hub's delivery while an owner dispatches.
pub(crate) struct DeliveryHold<'a> {
    hub: &'a ObserverHub,
}

impl Drop for DeliveryHold<'_> {
    fn drop(&mut self) {
        self.hub.holds.set(self.hub.holds.get() - 1);
    }
}

/// Observer failures of one pass; the first is the one reported.
#[derive(Default)]
struct Failures {
    first: Option<Box<dyn Any + Send>>,
    later: usize,
}

impl Failures {
    fn capture(&mut self, payload: Box<dyn Any + Send>) {
        if self.first.is_none() {
            self.first = Some(payload);
        } else {
            self.later += 1;
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    fn report(self, retire: &Retire) {
        let Some(first) = self.first else {
            return;
        };
        let panic = flui_foundation::panic::payload_text(first.as_ref()).unwrap_or("<opaque>");
        let later = self.later;
        let destroyed = retire.failed.get();
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            tracing::error!(
                target: "flui_interaction::gesture::observer",
                panic,
                later_failures = later,
                failed_retirements = destroyed,
                "a gesture observer panicked; it stays registered and later observations continue"
            );
        })) {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
        flui_foundation::panic::retain_opaque_payload(first);
    }
}

/// Destroys outgoing observers under containment. After a failed destruction,
/// or while the thread unwinds, the rest are retained instead.
struct Retire {
    failed: Cell<usize>,
}

impl Retire {
    fn new() -> Self {
        Self {
            failed: Cell::new(0),
        }
    }

    fn retire(&self, observer: Option<Rc<dyn GestureObserver>>) {
        let Some(observer) = observer else {
            return;
        };
        if self.failed.get() != 0 || std::thread::panicking() {
            observer.retain();
            return;
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(observer))) {
            self.failed.set(self.failed.get() + 1);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

/// One observer registration. Dropping it removes this registration only;
/// see [`GestureArena::observe`].
#[must_use = "dropping the handle removes the observer"]
pub struct ObserverHandle {
    hub: Weak<ObserverHub>,
    id: u64,
}

impl ObserverHandle {
    /// Replace this registration's observer. The replacement receives from
    /// the next observation; the outgoing observer is destroyed after any call
    /// in flight returns.
    pub fn replace(&self, observer: impl GestureObserver + 'static) {
        let observer: Rc<dyn GestureObserver> = Rc::new(observer);
        match self.hub.upgrade() {
            Some(hub) => hub.take_out(self.id, Some(observer)),
            None => Retire::new().retire(Some(observer)),
        }
    }
}

impl fmt::Debug for ObserverHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            hub.take_out(self.id, None);
        }
    }
}
