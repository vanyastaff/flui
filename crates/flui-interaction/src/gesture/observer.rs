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
    sync::{Arc, Weak as SyncWeak},
};

use flui_foundation::{IdExhausted, IdMint, InputSequenceId, MonotonicClock};
use flui_platform_api::{EventTime, PlatformWindow};
use smallvec::SmallVec;

use crate::events::CancelReason;
use crate::gesture::arena::{
    ArenaGeneration, CompositionBranch, CompositionKind, CompositionNode, GestureArenaMember,
    GestureDisposition,
};
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

/// One contest of the arena: a pointer, the input sequence its Down started
/// and the arena generation that Down opened. A reused pointer ID opens a
/// different contest, with a different sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GestureContest {
    pointer: PointerId,
    sequence: Option<InputSequenceId>,
    generation: ArenaGeneration,
}

impl GestureContest {
    pub(crate) const fn new(
        pointer: PointerId,
        sequence: Option<InputSequenceId>,
        generation: ArenaGeneration,
    ) -> Self {
        Self {
            pointer,
            sequence,
            generation,
        }
    }

    /// The contact the contest is for.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }

    /// The input sequence the contest decides: the one a
    /// [`GestureBinding`](crate::GestureBinding) minted at the contact's Down.
    /// `None` in an arena no binding drives, whose contacts carry no sequence.
    #[must_use]
    pub const fn sequence(self) -> Option<InputSequenceId> {
        self.sequence
    }

    pub(crate) const fn generation(self) -> ArenaGeneration {
        self.generation
    }
}

/// The arena member an observation names, held weakly: observing a decision
/// never keeps a recognizer alive.
///
/// Two observed members are equal when they name the same member allocation,
/// never by type or label; an observer that keeps one keeps the allocation
/// from being reused for another member.
#[derive(Clone)]
pub struct ObservedMember {
    member: Weak<dyn GestureArenaMember>,
    /// Read when the decision is committed: a member may retire before the
    /// observation is delivered.
    type_name: Option<&'static str>,
}

impl ObservedMember {
    pub(crate) fn new(member: Weak<dyn GestureArenaMember>) -> Self {
        // Through the `dyn` member, so the name is the member's own type's,
        // not the `Rc`'s.
        let type_name = member
            .upgrade()
            .map(|member| super::arena::member_type_name(&*member));
        Self { member, type_name }
    }

    /// Whether this is `member`, compared by allocation, never by type or
    /// label.
    #[must_use]
    pub fn is<M: ?Sized>(&self, member: &Rc<M>) -> bool {
        std::ptr::addr_eq(self.member.as_ptr(), Rc::as_ptr(member))
    }

    /// Whether the member was dropped since; it can then join no contest.
    #[must_use]
    pub fn is_retired(&self) -> bool {
        self.member.strong_count() == 0
    }

    /// The member's concrete type name, read when the decision was committed
    /// (`None` if it had already retired): a human-readable annotation, never
    /// an identity.
    #[must_use]
    pub fn type_name(&self) -> Option<&'static str> {
        self.type_name
    }
}

impl PartialEq for ObservedMember {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.member, &other.member)
    }
}

impl Eq for ObservedMember {}

impl fmt::Debug for ObservedMember {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObservedMember({:p})", self.member.as_ptr())
    }
}

/// One composition a member joined under, held weakly: observing a join never
/// keeps a composition alive.
///
/// Two observed compositions are the same node when
/// [`is_same_node`](Self::is_same_node) says so, compared by allocation; the
/// members whose paths share a node are the two branches of one relation.
#[derive(Clone)]
pub struct ObservedComposition {
    node: Weak<CompositionNode>,
    kind: CompositionKind,
    branch: CompositionBranch,
}

impl ObservedComposition {
    pub(crate) fn new(node: &Rc<CompositionNode>, branch: CompositionBranch) -> Self {
        Self {
            node: Rc::downgrade(node),
            kind: node.kind,
            branch,
        }
    }

    /// The relation this node is.
    #[must_use]
    pub fn kind(&self) -> CompositionKind {
        self.kind
    }

    /// The branch of this node the member joined through.
    #[must_use]
    pub fn branch(&self) -> CompositionBranch {
        self.branch
    }

    /// Whether every arena handle of this composition was dropped since; no
    /// member can join under it again.
    #[must_use]
    pub fn is_retired(&self) -> bool {
        self.node.strong_count() == 0
    }

    /// Whether `other` names the same composition node, whichever branch it
    /// took through it.
    #[must_use]
    pub fn is_same_node(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.node, &other.node)
    }
}

impl PartialEq for ObservedComposition {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_node(other) && self.branch == other.branch
    }
}

impl Eq for ObservedComposition {}

impl fmt::Debug for ObservedComposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservedComposition")
            .field("node", &self.node.as_ptr())
            .field("kind", &self.kind)
            .field("branch", &self.branch)
            .finish()
    }
}

/// Why a member was rejected from a contest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RejectionReason {
    /// A hold joined the contact on Down and beat this member, which has no
    /// motion role (ADR-0208 rule 4).
    HeldOut,
    /// The member rejected itself.
    Withdrew,
    /// Another member won the contest, or the contact was handed to another
    /// successor of the same hold.
    Defeated,
    /// The contest was cancelled; the preceding
    /// [`GestureObservation::Cancelled`] names the cause.
    Cancelled,
    /// The member was a [`GestureArena::sequenced`] successor and its hold
    /// left the contest.
    HoldFailed,
    /// The contest settled without choosing any member: the contact ended
    /// with no eligible competitor, or before a won hold handed its contact
    /// to this successor.
    Unresolved,
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
        /// The compositions the member joined under, innermost first, as
        /// [`GestureArena::composition_path`] lists them; empty when it
        /// joined uncomposed.
        composition: Vec<ObservedComposition>,
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
        /// When the arena committed the verdict, on its event timeline.
        at: EventTime,
    },
    /// A member lost or withdrew from the contest.
    Rejected {
        /// The contest.
        contest: GestureContest,
        /// The rejected member.
        member: ObservedMember,
        /// When the arena committed the verdict, on its event timeline.
        at: EventTime,
        /// Why.
        reason: RejectionReason,
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
        /// The contest the member was polled for, with its pointer and
        /// sequence: the latest one it joined. `None` only for a member that
        /// registered its deadline for a contact with no contest open and has
        /// joined none since.
        contest: Option<GestureContest>,
        /// The member polled.
        member: ObservedMember,
        /// When the arena polled it.
        now: EventTime,
    },
}

impl GestureObservation {
    /// `member`'s verdict in `contest` at `at`; a rejection names `reason`.
    pub(crate) fn verdict(
        contest: GestureContest,
        member: ObservedMember,
        at: EventTime,
        disposition: GestureDisposition,
        reason: RejectionReason,
    ) -> Self {
        match disposition {
            GestureDisposition::Accepted => Self::Won {
                contest,
                member,
                at,
            },
            GestureDisposition::Rejected => Self::Rejected {
                contest,
                member,
                at,
                reason,
            },
        }
    }
}

/// One observer registration of a hub, never reissued by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegistrationId(NonZeroU64);

struct Registration {
    id: RegistrationId,
    observer: Rc<dyn GestureObserver>,
}

/// The arena's observers and its queue of undelivered observations.
#[derive(Default)]
pub(crate) struct ObserverHub {
    registrations: RefCell<Vec<Registration>>,
    queue: RefCell<VecDeque<GestureObservation>>,
    /// Issues registration ids; refuses permanently once exhausted.
    ids: IdMint<NonZeroU64>,
    /// Owners (a dispatching binding) that defer delivery until they return.
    holds: Cell<usize>,
    delivering: Cell<bool>,
    /// Observers replaced or removed during a pass, destroyed after the call
    /// in flight.
    outgoing: RefCell<Vec<Rc<dyn GestureObserver>>>,
    wake: RefCell<Option<SyncWeak<dyn PlatformWindow>>>,
    /// The arena's clock, read when a verdict is committed.
    clock: Option<Arc<dyn MonotonicClock>>,
    /// The first failure of that clock, raised by the arena operation once
    /// the verdicts it committed are delivered.
    clock_failure: RefCell<Option<Box<dyn Any + Send>>>,
}

impl ObserverHub {
    /// A hub stamping verdicts on `clock`'s event timeline.
    pub(crate) fn with_clock(clock: Arc<dyn MonotonicClock>) -> Self {
        Self {
            clock: Some(clock),
            ..Self::default()
        }
    }

    /// Now on the arena's event timeline: when a verdict being recorded was
    /// committed.
    ///
    /// The verdict is already committed when it is stamped, so a failing
    /// clock cannot unwind past it: the failure is kept for
    /// [`Self::take_clock_failure`] and the verdict is stamped as a hub
    /// without a clock stamps it.
    pub(crate) fn now(&self) -> EventTime {
        let Some(clock) = self.clock.as_deref() else {
            return EventTime::from_nanos(0);
        };
        match catch_unwind(AssertUnwindSafe(|| crate::events::interaction_now(clock))) {
            Ok(now) => now,
            Err(payload) => {
                let mut first = self.clock_failure.borrow_mut();
                if first.is_none() {
                    *first = Some(payload);
                } else {
                    drop(first);
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
                EventTime::from_nanos(0)
            }
        }
    }

    /// The clock failure [`Self::now`] kept, for the operation to raise.
    pub(crate) fn take_clock_failure(&self) -> Option<Box<dyn Any + Send>> {
        self.clock_failure.borrow_mut().take()
    }

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
    pub(crate) fn issue_ids_after(&self, last: u64) {
        self.ids.reserve_through(last);
    }

    pub(crate) fn register(
        self: &Rc<Self>,
        observer: Rc<dyn GestureObserver>,
    ) -> Result<ObserverHandle, IdExhausted> {
        let id = RegistrationId(self.ids.next()?);
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

    fn take_out(&self, id: RegistrationId, replacement: Option<Rc<dyn GestureObserver>>) {
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
    id: RegistrationId,
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
