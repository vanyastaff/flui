//! Gesture Arena - Conflict resolution between competing gesture recognizers
//!
//! When multiple gesture recognizers compete for the same pointer (e.g., a tap
//! and a drag recognizer both want to handle the same touch), the GestureArena
//! determines which recognizer wins.
//!
//! # Architecture
//!
//! The arena follows a lifecycle:
//!
//! ```text
//! 1. Pointer Down → Create arena entry (Open state)
//! 2. Recognizers add themselves to arena
//! 3. Arena can be held (Held state) if recognizers need more time
//! 4. Arena closes (Closed state) - no more members
//! 5. Recognizers compete (accept/reject)
//! 6. Arena resolves winner (Resolved state)
//! 7. Winner receives all future events for that pointer
//! 8. Pointer Up → Sweep (cleanup)
//! ```
//!
//! Runtime deadline decisions borrow the presentation's recovery custody through
//! verdict delivery and snapshot retirement. Committed member notifications are
//! delivery; destruction of an opaque member establishes no valid postcondition.
//! The outermost standalone operation owns recovery and resumes its first
//! authoritative unwind after accepted notifications and safe retirement finish.
//! During synchronous reentry, public calls through the same arena join that
//! active operation: they relay typed failure receipts instead of unwinding
//! through the enclosing callback's unrelated delivery classification. The outer
//! standalone call finishes recovery; runtime sampling consumes its owned receipt.
//! Nested receipts are imported before an escaping enclosing callback is
//! classified. A passive arena-local stack carries custody without borrowing arena
//! state across callouts. It ends with the operation; later independent calls
//! retire normally.
//!
//! # GestureArenaEntry Handle Pattern
//!
//! When adding a member to the arena, you receive a [`GestureArenaEntry`]
//! handle. This handle is the preferred way for recognizers to resolve
//! themselves:
//!
//! ```rust
//! use flui_interaction::{GestureArena, GestureDisposition, PointerId, TapGestureRecognizer};
//! let arena = GestureArena::new();
//! let pointer = PointerId::try_from(1_u64)?;
//! let my_recognizer = TapGestureRecognizer::builder(arena.clone()).build();
//! let entry = arena.add(pointer, &my_recognizer);
//! // Later, when the recognizer decides:
//! entry.resolve(GestureDisposition::Accepted);
//! arena.close(pointer);
//! # Ok::<(), std::num::TryFromIntError>(())
//! ```
//!
//! This pattern allows recognizers to resolve themselves without needing
//! a reference back to the arena.
//!
//! # Type System Features
//!
//! - **Newtype IDs**: Type-safe `PointerId` prevents mixing with other IDs
//! - **SmallVec**: Inline storage avoids heap allocation for typical cases
//! - **Exact generations**: stale entry handles cannot resolve a reused pointer
//! - **Owner affinity**: executable callbacks never acquire a cross-thread API

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
    sync::Arc,
};

use flui_platform::EventTime;
use smallvec::SmallVec;
use tracing::instrument;

use crate::close::{CloseMode, ClosePanic, CloseTombstone};
use crate::ids::PointerId;
use crate::retain::Retain;
use flui_foundation::{
    IdMint, InputSequenceId, MonotonicClock, SystemClock,
    panic::{FailureClass, RecoveryScope},
};

mod composition;
mod frame;
use composition::BranchLink;
pub(crate) use composition::CompositionNode;
pub use composition::{
    CompositionBranch, CompositionError, CompositionKind, GestureBranches, GestureComposition,
    SequencedBranch,
};
mod deadline;
use deadline::{DeadlinePoll, DeadlineRegistry};
pub use deadline::{GestureDeadlineError, GestureDeadlineRegistration, GestureDeadlineUpdate};
mod ownership;
pub(crate) use ownership::MotionFrame;
use ownership::PanClaimNode;
pub use ownership::{MotionRole, PanClaim};
mod entry;
use entry::{ArenaEntryData, ArenaFollowUp, PendingNotifications, Resolution};
mod recovery;
use recovery::ArenaRecovery;

use crate::gesture::settings::GestureSettingsProvider;
use crate::pointer::pointer_capture::ContactFeeder;
use flui_foundation::geometry::Offset;

/// What a joining recognizer tells the arena about one admission.
#[derive(Clone, Default)]
pub(crate) struct AdmissionContext {
    /// How the member takes part in the contact's ownership.
    pub(crate) role: ContactRole,
    /// The route target that delivered the Down to the member.
    pub(crate) feeder: Option<ContactFeeder>,
    /// How the member sees displacement, for the ownership table.
    pub(crate) frame: MotionFrame,
}

/// How a member takes part in its contact's ownership (ADR-0208).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ContactRole {
    /// It does not move: a tap or a long press, which a hold beats.
    #[default]
    Discrete,
    /// It moves as `MotionRole` says, for the ownership table.
    Moving(MotionRole),
    /// It holds the contact on Down (rule 4) and does not move.
    Hold,
}

impl ContactRole {
    /// How the member moves, if it does.
    pub(crate) const fn motion(self) -> Option<MotionRole> {
        match self {
            Self::Moving(role) => Some(role),
            Self::Discrete | Self::Hold => None,
        }
    }
}

/// The rejections a hold's admission committed, owed to the members it beat
/// (ADR-0208 rule 4). The admitting recognizer records its contact before it
/// delivers them, so a beaten member's callback that reenters the arena or
/// fails finds the hold already tracking that contest.
#[must_use = "the members a hold beat are owed their rejections"]
pub(crate) struct HeldOut {
    pending: PendingNotifications,
    contest: GestureContest,
}

/// Everything the arena records about one member of one contact.
#[derive(Clone)]
struct Admission {
    branch: Option<BranchLink>,
    pan_claim: Option<Rc<PanClaimNode>>,
    role: ContactRole,
    /// The member cannot use this contact's motion yet, so the ownership
    /// table passes it by (a two-contact scale on its lone contact).
    dormant: bool,
    feeder: Option<ContactFeeder>,
    frame: MotionFrame,
}

/// One member's vote on its contact.
#[derive(Clone, Copy, PartialEq)]
enum Vote {
    Accept,
    Reject,
    /// The member crossed its slop with this displacement since Down.
    ClaimMotion(Offset<f64>),
}

impl From<GestureDisposition> for Vote {
    fn from(disposition: GestureDisposition) -> Self {
        match disposition {
            GestureDisposition::Accepted => Self::Accept,
            GestureDisposition::Rejected => Self::Reject,
        }
    }
}

/// A successor kept admitted after its hold won the contact, until the
/// [`SequencedBranch`] hands the contact to it or the contact ends.
#[derive(Clone)]
struct Carried {
    node: Weak<CompositionNode>,
    /// The resolved contest that carried it, as observers name it.
    contest: GestureContest,
    member: Weak<dyn GestureArenaMember>,
    requested: bool,
    feeder: Option<ContactFeeder>,
}

/// A committed verdict and the exact contest it settles.
type ContestVerdict = (
    Weak<dyn GestureArenaMember>,
    GestureDisposition,
    GestureContest,
);

/// Verdicts owed to carried successors, each for the contest that carried it.
type CarriedVerdicts = SmallVec<[ContestVerdict; 2]>;

use crate::gesture::observer::{
    ContestCancel, GestureContest, GestureObservation, GestureObserver, ObservedMember,
    ObserverHandle, ObserverHub, RejectionReason,
};

// ============================================================================
// GestureDisposition enum
// ============================================================================

/// Gesture disposition - how a recognizer voted in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GestureDisposition {
    /// Recognizer wants to handle this gesture.
    Accepted,
    /// Recognizer does not want to handle this gesture.
    Rejected,
}

impl GestureDisposition {
    /// Returns `true` if accepted.
    #[inline]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Returns `true` if rejected.
    #[inline]
    pub const fn is_rejected(self) -> bool {
        matches!(self, Self::Rejected)
    }
}

// ============================================================================
// GestureArenaMember trait
// ============================================================================

/// Trait for objects that can participate in gesture arena.
///
/// Implemented by all gesture recognizers.
///
/// # Custom Recognizers
///
/// External members implement this trait directly, including their own deadline
/// query and polling hook. The arena holds members weakly; their owner keeps them alive.
/// Runtime-hosted timers publish changes through
/// [`GestureArena::register_deadline_member`] and its weak update handle after
/// committing recognizer state. This also permits explicit rearm after a failed
/// open poll, whose side effects cannot safely be repeated automatically.
///
/// A verdict names the exact contest it settles: a pointer ID is reused by
/// every contact of a mouse, and a contest a hold kept past its Up can settle
/// while the same pointer's next contact is down. A member that outlives one
/// contact compares the contest with its entry's
/// ([`GestureArenaEntry::contest`]) rather than the pointer.
///
/// ```rust
/// use std::cell::RefCell;
/// use flui_interaction::gesture::{
///     GestureArena, GestureArenaEntry, GestureArenaMember, GestureContest, GestureDisposition,
/// };
/// use flui_interaction::PointerId;
///
/// #[derive(Default)]
/// struct MyRecognizer {
///     entry: RefCell<Option<GestureArenaEntry>>,
/// }
///
/// impl GestureArenaMember for MyRecognizer {
///     fn accept_gesture(&self, contest: GestureContest) {
///         let mine = self.entry.borrow().as_ref().is_some_and(|entry| entry.contest() == contest);
///         assert!(mine, "this member joined only one contest");
///     }
///     fn reject_gesture(&self, contest: GestureContest) {
///         // Handle losing the arena
///     }
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::new(core::num::NonZeroU64::MIN);
/// let recognizer = std::rc::Rc::new(MyRecognizer::default());
/// let entry = arena.add(pointer, &recognizer);
/// *recognizer.entry.borrow_mut() = Some(entry.clone());
/// entry.resolve(GestureDisposition::Accepted);
/// arena.close(pointer);
/// ```
///
pub trait GestureArenaMember: member_type::MemberTypeName {
    /// Accept the gesture for `contest`.
    ///
    /// Called when this recognizer wins that contest.
    fn accept_gesture(&self, contest: GestureContest);

    /// Reject the gesture for `contest`.
    ///
    /// Called when another recognizer wins that contest, or this recognizer
    /// explicitly rejects the gesture.
    fn reject_gesture(&self, contest: GestureContest);

    /// Advance any time-based deadline this member owns (e.g. a long-press
    /// hold timer).
    ///
    /// Called once per frame by the binding's deadline tick so a deadline can
    /// elapse while the pointer is held still — without a further pointer event
    /// to drive it. The default is a no-op; only deadline-driven recognizers
    /// (long press) override it. Implementations must be idempotent across
    /// frames (firing at most once per deadline).
    fn poll_deadline(&self, now: EventTime) {
        let _ = now;
    }

    /// The armed deadline, queried without invoking callbacks or calling the arena.
    /// The arena derives pending work from this one query and polls only due members.
    /// Runtime scheduling getters read an owner snapshot instead of invoking it.
    /// A runtime query failure gets one automatic re-probe after 100 ms, then
    /// waits for an explicit update revision; standalone queries still propagate.
    fn deadline(&self) -> Option<EventTime> {
        None
    }

    /// [`poll_deadline`](Self::poll_deadline) under the arena's failure
    /// custody, which the arena calls in its place.
    ///
    /// The default runs `poll_deadline` and returns `true`; the arena contains
    /// a panic escaping it. Override it when the deadline commits a transition
    /// and then calls user code (a long press fires its callbacks): commit the
    /// state first, run each callback through
    /// [`RecoveryScope::run`](flui_foundation::panic::RecoveryScope::run) so a
    /// panic in one is caught once and the callbacks after it still run, and
    /// retire values whose destructors may run user code through
    /// [`RecoveryScope::retire`](flui_foundation::panic::RecoveryScope::retire).
    ///
    /// Return `true` once the transition this poll owed has committed, even
    /// if a callback after it failed, so the arena does not poll it again; an
    /// older failure in the same scope cannot change that answer. `false`
    /// parks the deadline until its registration publishes an update.
    fn poll_deadline_with_recovery(
        &self,
        now: EventTime,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) -> bool {
        // The owning arena contains this open hook. A second catch here would
        // classify its escaped payload before an earlier nested arena receipt.
        let _ = recovery;
        self.poll_deadline(now);
        true
    }
}

/// `member`'s concrete type name.
pub(crate) fn member_type_name(member: &dyn GestureArenaMember) -> &'static str {
    member_type::MemberTypeName::member_type_name(member)
}

/// The concrete type name of an arena member: a supertrait every type
/// implements, so a member never implements it and a recording reads the
/// name through the member's vtable.
mod member_type {
    /// The member's concrete type name, which an input recording's decision
    /// row carries ([`ObservedMember::type_name`](crate::gesture::ObservedMember::type_name)).
    /// A description, not an identity.
    pub trait MemberTypeName {
        /// The implementing type's name.
        fn member_type_name(&self) -> &'static str;
    }

    impl<T> MemberTypeName for T {
        fn member_type_name(&self) -> &'static str {
            core::any::type_name::<T>()
        }
    }
}

// ============================================================================
// GestureArenaEntry - Handle pattern for resolving gestures
// ============================================================================

/// A handle to an arena entry for a specific member.
///
/// This is returned by [`GestureArena::add`] and provides a convenient way
/// for gesture recognizers to resolve themselves without needing a reference
/// back to the arena.
///
/// # Example
///
/// ```rust
/// use std::rc::Rc;
///
/// use flui_interaction::gesture::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::gesture::{GestureArenaMember, GestureContest};
///
/// struct R;
/// impl GestureArenaMember for R {
///     fn accept_gesture(&self, _: GestureContest) {}
///     fn reject_gesture(&self, _: GestureContest) {}
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::new(core::num::NonZeroU64::MIN);
/// let recognizer: Rc<R> = Rc::new(R);
///
/// let entry = arena.add(pointer, &recognizer);
///
/// // Later, when the recogniser decides:
/// entry.resolve(GestureDisposition::Accepted);
/// ```
///
/// The handle is owner-affine because gesture callbacks are owner-local.
/// Multiple calls to `resolve` are safe; stale or already-resolved entries are
/// no-ops.
/// Reentrant calls join the arena's surrounding recovery operation. An outermost
/// standalone operation propagates its first authoritative unwind; runtime
/// sampling contains it as an owned failure receipt (see the module contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ArenaGeneration(u64);

struct ArenaSlot {
    pointer: PointerId,
    /// The input sequence of the Down that opened this contest, when a
    /// binding dispatched it.
    sequence: Option<InputSequenceId>,
    generation: ArenaGeneration,
    data: RefCell<ArenaEntryData>,
}

impl ArenaSlot {
    fn new(
        pointer: PointerId,
        sequence: Option<InputSequenceId>,
        generation: ArenaGeneration,
    ) -> Self {
        Self {
            pointer,
            sequence,
            generation,
            data: RefCell::new(ArenaEntryData::new()),
        }
    }

    fn contest(&self) -> GestureContest {
        GestureContest::new(self.pointer, self.sequence, self.generation)
    }
}

/// Run one outermost arena operation, then deliver the decisions it queued
/// to `observers`, including when it fails: what it committed before the
/// failure is still a fact, and nothing else would deliver or wake for it.
///
/// A clock failure stamping a verdict is raised here too, after delivery,
/// in the order it happened. The hub keeps one only while no failure of the
/// operation preceded it, so a kept clock failure, stamped before the
/// operation began or inside it, precedes the operation's own failure.
fn observe_after(observers: &ObserverHub, operation: impl FnOnce()) {
    let earlier = observers.take_clock_failure();
    let hold = observers.hold();
    let outcome = catch_unwind(AssertUnwindSafe(operation));
    drop(hold);
    observers.deliver();
    let during = observers.take_clock_failure();
    let mut failures = earlier.into_iter().chain(during).chain(outcome.err());
    if let Some(first) = failures.next() {
        failures.for_each(flui_foundation::panic::retain_opaque_payload);
        resume_unwind(first);
    }
}

/// Queue a contest's cancellation and the rejections it committed.
fn record_cancel(
    observers: &ObserverHub,
    recovery: &ArenaRecovery,
    slot: &ArenaSlot,
    cause: ContestCancel,
    pending: &PendingNotifications,
) {
    observers.record(|| GestureObservation::Cancelled {
        contest: slot.contest(),
        cause,
    });
    record_verdicts(
        observers,
        recovery,
        slot,
        pending,
        RejectionReason::Cancelled,
    );
}

/// Queue the verdicts a resolution committed, losers and winner in the order
/// their members are notified. A loser was defeated when the batch has a
/// winner, and rejected for `unwon` otherwise. `recovery` says whether the
/// operation stamping them has already failed.
fn record_verdicts(
    observers: &ObserverHub,
    recovery: &ArenaRecovery,
    slot: &ArenaSlot,
    pending: &PendingNotifications,
    unwon: RejectionReason,
) {
    let reason = if pending
        .iter()
        .any(|(_, disposition)| *disposition == GestureDisposition::Accepted)
    {
        RejectionReason::Defeated
    } else {
        unwon
    };
    for (member, disposition) in pending {
        observers.record(|| {
            GestureObservation::verdict(
                slot.contest(),
                ObservedMember::new(member.clone()),
                observers.now(recovery.preserving()),
                *disposition,
                reason,
            )
        });
    }
}

#[derive(Clone)]
/// A stale-safe handle for one member in one arena generation.
///
/// Resolving a handle after its pointer ID has been reused cannot affect the
/// replacement arena.
pub struct GestureArenaEntry {
    arena: GestureArena,
    contest: GestureContest,
    slot: Weak<ArenaSlot>,
    member: Weak<dyn GestureArenaMember>,
}

impl GestureArenaEntry {
    /// Create a new arena entry handle.
    fn new(arena: GestureArena, slot: &Rc<ArenaSlot>, member: &Rc<dyn GestureArenaMember>) -> Self {
        Self {
            arena,
            contest: slot.contest(),
            slot: Rc::downgrade(slot),
            member: Rc::downgrade(member),
        }
    }

    /// The exact contest this entry joined: the one its member's verdicts
    /// name ([`GestureArenaMember::accept_gesture`]).
    #[must_use]
    pub fn contest(&self) -> GestureContest {
        self.contest
    }

    /// The input sequence of the Down that opened this entry's contest, when
    /// a binding dispatched it.
    pub(crate) fn sequence(&self) -> Option<InputSequenceId> {
        self.contest.sequence()
    }

    /// Whether the ownership table passes this member by on this contact
    /// (ADR-0208 §2): `true` while the member cannot use the contact's motion
    /// — a scale that needs two contacts and holds this one alone — so a
    /// moving competitor that crosses its slop takes the contact instead of
    /// losing it to a member that would never start. The member keeps its
    /// role and its place in the arena, and may still claim by resolving.
    pub(crate) fn set_motion_dormant(&self, dormant: bool) {
        if let Some(slot) = self.slot.upgrade()
            && let Some(member) = self.member.upgrade()
        {
            slot.data
                .borrow_mut()
                .set_dormant(&Rc::downgrade(&member), dormant);
        }
    }

    /// Resolve this entry with the given disposition.
    ///
    /// Call with [`GestureDisposition::Accepted`] to claim victory, or
    /// [`GestureDisposition::Rejected`] to admit defeat.
    ///
    /// It's safe to call this on an arena that has already been resolved.
    pub fn resolve(&self, disposition: GestureDisposition) {
        self.arena.run_owned_recovery(|scope| {
            self.resolve_with_recovery(disposition, scope);
        });
    }

    pub(crate) fn resolve_with_recovery(
        &self,
        disposition: GestureDisposition,
        recovery: &mut RecoveryScope<'_>,
    ) {
        let (Some(slot), Some(member)) = (self.slot.upgrade(), self.member.upgrade()) else {
            return;
        };
        self.arena.resolve_entry_notifying(
            self.contest.pointer(),
            &slot,
            member,
            disposition,
            None,
            recovery,
        );
    }

    /// Claim the contact for a moving member that crossed its slop, with the
    /// root-space `displacement` accumulated since Down (ADR-0208 §2).
    ///
    /// The arena gives the contact to the owner the ownership table picks
    /// among the members admitted with a [`MotionRole`]: possibly another
    /// member, in which case this one is rejected. A member admitted without a
    /// role claims as [`Self::resolve`] with `Accepted` does.
    pub fn claim_motion(&self, displacement: Offset<f64>) {
        self.arena.run_owned_recovery(|scope| {
            self.claim_motion_with_recovery(displacement, scope);
        });
    }

    pub(crate) fn claim_motion_with_recovery(
        &self,
        displacement: Offset<f64>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        let (Some(slot), Some(member)) = (self.slot.upgrade(), self.member.upgrade()) else {
            return;
        };
        self.arena.resolve_entry_notifying(
            self.contest.pointer(),
            &slot,
            member,
            Vote::ClaimMotion(displacement),
            None,
            recovery,
        );
    }

    /// Withdraw an already locally retired member without rejecting its later contact.
    pub(crate) fn reject_without_self(&self) {
        self.arena.run_owned_recovery(|scope| {
            self.reject_without_self_with_recovery(scope);
        });
    }

    pub(crate) fn reject_without_self_with_recovery(&self, recovery: &mut RecoveryScope<'_>) {
        let (Some(slot), Some(member)) = (self.slot.upgrade(), self.member.upgrade()) else {
            return;
        };
        self.arena.resolve_entry_notifying(
            self.contest.pointer(),
            &slot,
            member,
            GestureDisposition::Rejected,
            Some(&self.member),
            recovery,
        );
    }

    /// Retire membership without executing user code from an owner's destructor.
    /// The weak identity remains comparable after its owner starts dropping.
    pub(crate) fn withdraw_deferred(&self) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        if slot.generation != self.contest.generation()
            || !self.arena.is_live_slot(self.contest.pointer(), &slot)
        {
            return;
        }
        let follow_up = slot.data.borrow_mut().withdraw(&self.member);
        self.arena
            .shared
            .deadlines
            .withdraw_member_generation(&self.member, slot.generation);
        match follow_up {
            ArenaFollowUp::RemoveEmpty => {
                slot.data.borrow_mut().is_resolved = true;
                self.arena.remove_exact_slot(self.contest.pointer(), &slot);
            }
            ArenaFollowUp::DeferDefault | ArenaFollowUp::ResolveInFavorOf(_) => {
                self.arena
                    .queue_default_resolution(self.contest.pointer(), &slot);
            }
            ArenaFollowUp::None => {}
        }
    }

    /// Hold this exact arena generation against a pointer-up sweep.
    pub fn hold(&self) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            GestureArena::hold_slot(&slot);
        }
    }

    /// Release a hold on this exact arena generation.
    pub fn release(&self) {
        self.arena.run_owned_recovery(|scope| {
            self.release_with_recovery(scope);
        });
    }

    pub(crate) fn release_with_recovery(&self, recovery: &mut RecoveryScope<'_>) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            self.arena.release_slot_with_recovery(&slot, recovery);
        }
    }

    /// Release this exact generation's retained hold without notifying members.
    /// Owners call this for their held tokens before silent withdrawal in Drop.
    pub(crate) fn release_deferred(&self) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        if slot.generation != self.contest.generation()
            || !self.arena.is_live_slot(self.contest.pointer(), &slot)
        {
            return;
        }
        let should_queue = {
            let mut entry = slot.data.borrow_mut();
            if entry.is_resolved {
                return;
            }
            entry.release();
            entry.has_pending_sweep
                || matches!(
                    entry.follow_up(),
                    ArenaFollowUp::DeferDefault | ArenaFollowUp::ResolveInFavorOf(_)
                )
        };
        if should_queue {
            self.arena
                .queue_default_resolution(self.contest.pointer(), &slot);
        }
    }

    /// Sweep this exact arena generation.
    pub fn sweep(&self) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            self.arena.sweep_slot(&slot);
        }
    }

    /// End this exact arena generation without a winner: every member still
    /// in it is rejected.
    ///
    /// A cancelled contact uses this rather than [`Self::sweep`]: a sweep has
    /// pointer-up semantics and awards the arena to its first member, which
    /// would accept a gesture for a contact that no longer exists. A
    /// generation already resolved or gone is left alone.
    pub fn abandon(&self) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            self.arena.abandon_slot(&slot);
        }
    }

    /// Cancel the contact after this member has retired its local state.
    /// Its own stale rejection must not reach a reentrantly admitted contact.
    pub(crate) fn abandon_without_self_with_recovery(&self, recovery: &mut RecoveryScope<'_>) {
        if self.arena.shared.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        let mut pending = None;
        recovery.run_classified(FailureClass::Unknown, |_| {
            if self.arena.remove_exact_slot(slot.pointer, &slot) {
                let mut notifications = slot.data.borrow_mut().resolve(None, Resolution::Terminal);
                self.arena
                    .record_cancel(&slot, ContestCancel::Abandoned, &notifications);
                notifications.retain(|(member, _)| !Weak::ptr_eq(member, &self.member));
                pending = Some(notifications);
            }
        });
        if let Some(pending) = pending {
            GestureArena::dispatch_pending_with_recovery(
                &self.arena.shared.recovery,
                pending,
                slot.contest(),
                recovery,
            );
        }
    }

    /// Get the pointer ID for this entry.
    #[inline]
    pub fn pointer(&self) -> PointerId {
        self.contest.pointer()
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> Option<Rc<dyn GestureArenaMember>> {
        if self.arena.shared.owner_closed.get() {
            return None;
        }
        self.member.upgrade()
    }
}

impl std::fmt::Debug for GestureArenaEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaEntry")
            .field("contest", &self.contest)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// SweepModel
// ============================================================================

/// Who owns the close/sweep lifecycle of an arena.
///
/// For a shared arena, the binding closes admissions on pointer-down and
/// detaches exact contact generations before terminal routing. Pointer-up
/// sweeps that detached batch; cancellation abandons it without a winner.
///
/// - [`SelfDriven`](Self::SelfDriven) — a low-level recognizer owns its private
///   arena lifecycle, so finishing pointer-up tracking sweeps its exact entry. This
///   model is for standalone recognizer use and focused recognizer tests, never
///   a presentation widget subtree.
/// - [`BindingDriven`](Self::BindingDriven) — a binding owns the arena and runs
///   lifecycle for exact contact generations around pointer routing.
///   Recognizers below it must *not* self-sweep: a tap's own
///   sweep on the first up would force-resolve a shared entry
///   to the front member before a double-tap (or a peer detector) could
///   complete.
///
/// The model is immutable per arena, like the clock; every clone observes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepModel {
    /// A standalone recognizer owns the lifecycle of its private arena.
    SelfDriven,
    /// A binding owns the lifecycle (shared arena).
    BindingDriven,
}

// ============================================================================
// GestureArena
// ============================================================================

/// The Gesture Arena.
///
/// Manages conflict resolution between competing gesture recognizers.
///
/// The arena is intentionally owner-affine: recognizer callbacks may capture
/// owner-local UI state. Internal maps provide keyed storage, not a promise of
/// cross-thread callback dispatch.
///
/// # Example
///
/// ```rust
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::rc::Rc;
///
/// use flui_interaction::gesture::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::gesture::{GestureArenaMember, GestureContest};
///
/// // A minimal recogniser that counts accepts/rejects. Use a real
/// // `TapGestureRecognizer` / `DragGestureRecognizer` in production —
/// // this is the minimum surface to participate in the arena.
/// #[derive(Debug)]
/// struct Counter(AtomicUsize, AtomicUsize);
/// impl GestureArenaMember for Counter {
///     fn accept_gesture(&self, _: GestureContest) { self.0.fetch_add(1, Ordering::Relaxed); }
///     fn reject_gesture(&self, _: GestureContest) { self.1.fetch_add(1, Ordering::Relaxed); }
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::new(core::num::NonZeroU64::MIN);
/// let tap = Rc::new(Counter(AtomicUsize::new(0), AtomicUsize::new(0)));
/// let drag = Rc::new(Counter(AtomicUsize::new(0), AtomicUsize::new(0)));
///
/// // Add recognisers to the arena — returns an entry handle.
/// let tap_entry = arena.add(pointer, &tap);
/// let drag_entry = arena.add(pointer, &drag);
///
/// // Close the arena once pointer-down dispatch finishes.
/// arena.close(pointer);
///
/// // Resolvers call the entry handle; the arena notifies members.
/// tap_entry.resolve(GestureDisposition::Accepted);
/// drag_entry.resolve(GestureDisposition::Rejected);
///
/// assert_eq!(tap.0.load(Ordering::Relaxed), 1);   // accepted
/// assert_eq!(drag.1.load(Ordering::Relaxed), 1);  // rejected
/// ```
#[derive(Clone)]
pub struct GestureArena {
    /// The composition branch this handle admits members into.
    branch: Option<BranchLink>,
    /// The innermost [`PanClaim`] region this handle admits members in.
    pan_claim: Option<Rc<PanClaimNode>>,
    /// The profiles compositions through this handle are checked against.
    settings: GestureSettingsProvider,
    /// The arena itself, which every clone and composition branch shares.
    shared: Rc<ArenaShared>,
}

/// The state every handle of one arena shares.
struct ArenaShared {
    /// Successors their hold's win kept admitted, until hand-off or the end
    /// of their contact.
    carried: RefCell<SmallVec<[Carried; 2]>>,
    owner_closed: Cell<bool>,
    close_mode: CloseTombstone,
    /// Active exact-generation slots, visited in ascending pointer order.
    entries: RefCell<BTreeMap<PointerId, Rc<ArenaSlot>>>,
    /// Held arenas detached from the active pointer map during an Up
    /// transaction. Exact entry tokens can still release these generations,
    /// while a reused pointer ID opens a fresh active slot. Retained slots
    /// are visited in ascending generation order.
    retained: RefCell<BTreeMap<ArenaGeneration, Rc<ArenaSlot>>>,
    /// Typed queue of deferred single-member resolutions.
    deferred: RefCell<VecDeque<DeferredResolution>>,
    /// Owner-frame deadline polling, independent from arena-slot lifetime;
    /// a detached batch keeps it past the arena.
    deadlines: Rc<DeadlineRegistry>,
    /// Borrow-free custody shared by nested public operations in this arena.
    recovery: Rc<ArenaRecovery>,
    next_generation: Cell<u64>,
    /// What the arena knows about the contacts its binding delivers.
    contacts: ContactLedger,
    /// The time source deadline-driven recognizers read `now()` from. Defaults
    /// to the OS clock; a headless frame driver injects a `ManualClock` so a
    /// deadline (e.g. long-press) elapses deterministically without sleeping.
    clock: Arc<dyn MonotonicClock>,
    /// Who owns the close/sweep lifecycle. Immutable per arena (like the clock);
    /// recognizers read it to decide whether finishing tracking should self-sweep.
    sweep_model: SweepModel,
    /// Observers of this arena's decisions and their undelivered queue.
    observers: Rc<ObserverHub>,
    /// The pointer frames open on this arena.
    frames: frame::PointerFrames,
}

/// When a binding's wheel or trackpad latch ends, as the packet being
/// dispatched leaves it ([`GestureArena::dispatched_latch_end`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScrollLatchEnd {
    /// It lapses at this event time, its idle time after the packet, unless
    /// another packet of its source comes first and moves the time on: the
    /// preset's contactless idle for a phase-less wheel, its longer phased
    /// idle for a phased source that may never send its end phase.
    IdleAt(EventTime),
    /// This packet is its last: an end phase, a cancellation or the end of
    /// platform momentum. The next packet begins a new latch.
    WithThisPacket,
}

/// The input a binding is dispatching, and the sequence it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchedInput {
    /// An event of a contact (or of a native pan/zoom lease on its pointer):
    /// a contest the pointer opens meanwhile decides this sequence.
    Contact {
        pointer: PointerId,
        sequence: InputSequenceId,
    },
    /// A wheel or trackpad packet of a latch, and when the latch ends.
    Latch {
        sequence: InputSequenceId,
        end: Option<ScrollLatchEnd>,
    },
    /// Contactless input: an assistive action, keyboard or programmatic
    /// scrolling, or a pan/zoom lease with no pointer.
    Contactless(InputSequenceId),
}

impl DispatchedInput {
    pub(crate) const fn sequence(self) -> InputSequenceId {
        match self {
            Self::Contact { sequence, .. }
            | Self::Latch { sequence, .. }
            | Self::Contactless(sequence) => sequence,
        }
    }
}

/// The one cell naming the input a binding is dispatching; the binding's
/// haptics host reads it live to tag its requests.
pub(crate) type DispatchedInputCell = Rc<Cell<Option<DispatchedInput>>>;

/// Per-contact state the arena keeps beside its competitions.
#[derive(Default)]
struct ContactLedger {
    /// The input its binding is dispatching; `None` outside a binding's
    /// dispatch.
    dispatched: DispatchedInputCell,
    /// The issuer of every sequence identity: the binding's own mint once a
    /// binding drives this arena, a private one for a standalone arena.
    sequences: RefCell<Rc<IdMint<InputSequenceId>>>,
    /// Contacts a moving member won and owns until they end: a latched
    /// scroll or drag, under which hover freezes (ADR-0208 rule 6).
    motion_latches: RefCell<SmallVec<[PointerId; 2]>>,
}

struct DeferredResolution {
    pointer: PointerId,
    generation: ArenaGeneration,
    slot: Weak<ArenaSlot>,
}

pub(crate) struct DetachedArenaBatch {
    slots: SmallVec<[Rc<ArenaSlot>; 2]>,
    /// Rejections owed to the successors carried for the ended contact.
    carried: CarriedVerdicts,
    deadlines: Rc<DeadlineRegistry>,
    recovery: Rc<ArenaRecovery>,
    observers: Rc<ObserverHub>,
}

impl GestureArena {
    /// Own recovery outside sampling, or join an enclosing arena operation.
    /// Nested calls relay receipts; only the outermost standalone owner resumes.
    /// Observers run after the outermost operation returns, never inside one.
    pub(crate) fn run_owned_recovery(&self, action: impl FnOnce(&mut RecoveryScope<'_>)) {
        observe_after(&self.shared.observers, || {
            self.shared.recovery.standalone(action);
        });
    }

    /// The observer hub an owner holds while it dispatches (see
    /// [`ObserverHub::hold`]).
    pub(crate) fn observers(&self) -> &Rc<ObserverHub> {
        &self.shared.observers
    }

    /// Register `observer` for this arena's decisions, shared by every clone
    /// and composition branch of it.
    ///
    /// The observer receives each [`GestureObservation`] after the operation
    /// that made it, until the returned handle drops; see
    /// [`GestureObserver`] for the delivery rules.
    ///
    /// ```rust
    /// use std::{cell::RefCell, rc::Rc};
    /// use flui_interaction::gesture::{GestureArena, GestureObservation};
    /// use flui_interaction::{GestureArenaMember, GestureContest, PointerId};
    ///
    /// struct Member;
    /// impl GestureArenaMember for Member {
    ///     fn accept_gesture(&self, _: GestureContest) {}
    ///     fn reject_gesture(&self, _: GestureContest) {}
    /// }
    ///
    /// let arena = GestureArena::new();
    /// let wins = Rc::new(RefCell::new(0));
    /// let seen = Rc::clone(&wins);
    /// let _handle = arena.observe(move |observation: &GestureObservation| {
    ///     if matches!(observation, GestureObservation::Won { .. }) {
    ///         *seen.borrow_mut() += 1;
    ///     }
    /// })?;
    /// let pointer = PointerId::try_from(1_u64).expect("nonzero");
    /// let member = Rc::new(Member);
    /// let _entry = arena.add(pointer, &member);
    /// arena.close(pointer);
    /// arena.drain_deferred_resolutions();
    /// assert_eq!(*wins.borrow(), 1);
    /// # Ok::<(), flui_interaction::IdExhausted>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`IdExhausted`](flui_foundation::IdExhausted) once every registration
    /// identity was issued; the arena refuses permanently rather than reissue
    /// one.
    pub fn observe(
        &self,
        observer: impl GestureObserver + 'static,
    ) -> Result<ObserverHandle, flui_foundation::IdExhausted> {
        self.shared.observers.register(Rc::new(observer))
    }

    /// This arena with its next observer registration identity `last + 1`,
    /// so a test can reach exhaustion without registering every identity
    /// before it.
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn with_observer_ids_after(self, last: u64) -> Self {
        self.shared.observers.issue_ids_after(last);
        self
    }

    /// A member asks to win: record an admitted claim, then apply it. A claim
    /// made crossing a slop carries the `motion` since Down, for the
    /// ownership table.
    fn claim(
        &self,
        slot: &ArenaSlot,
        entry: &mut ArenaEntryData,
        member: &Rc<dyn GestureArenaMember>,
        motion: Option<Offset<f64>>,
    ) -> ArenaFollowUp {
        entry.prune_departed();
        let identity = Rc::downgrade(member);
        if !entry.is_resolved
            && entry
                .members
                .iter()
                .any(|live| Weak::ptr_eq(live, &identity))
        {
            self.shared
                .observers
                .record(|| GestureObservation::Claimed {
                    contest: slot.contest(),
                    member: ObservedMember::new(identity),
                });
        }
        match motion {
            Some(displacement) => entry.claim_motion(member, displacement),
            None => entry.accept(member),
        }
    }

    /// Queue the verdict a carried successor received; a rejection names
    /// `reason`.
    fn record_carried(
        &self,
        carried: &Carried,
        disposition: GestureDisposition,
        reason: RejectionReason,
    ) {
        self.shared.observers.record(|| {
            GestureObservation::verdict(
                carried.contest,
                ObservedMember::new(carried.member.clone()),
                self.shared.observers.now(self.shared.recovery.preserving()),
                disposition,
                reason,
            )
        });
    }

    /// Queue a contest's cancellation and the rejections it committed.
    fn record_cancel(
        &self,
        slot: &ArenaSlot,
        cause: ContestCancel,
        pending: &PendingNotifications,
    ) {
        record_cancel(
            &self.shared.observers,
            &self.shared.recovery,
            slot,
            cause,
            pending,
        );
    }

    /// Queue the verdicts a resolution committed, in delivery order; losers
    /// of a batch with no winner were rejected for `unwon`.
    fn record_verdicts(
        &self,
        slot: &ArenaSlot,
        pending: &PendingNotifications,
        unwon: RejectionReason,
    ) {
        record_verdicts(
            &self.shared.observers,
            &self.shared.recovery,
            slot,
            pending,
            unwon,
        );
    }

    pub(crate) fn with_recovery_context(
        &self,
        scope: &mut RecoveryScope<'_>,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) {
        self.shared.recovery.scoped(scope, action);
    }

    pub(crate) fn invoke_with_recovery(
        &self,
        class: FailureClass,
        scope: &mut RecoveryScope<'_>,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) -> bool {
        self.shared.recovery.invoke(class, scope, action)
    }

    pub(crate) fn close_tombstone(&self) -> CloseTombstone {
        self.shared.close_mode.clone()
    }

    /// Whether a moving member owns a continuing contact: a latched scroll
    /// or drag, under which hover freezes (ADR-0208 rule 6).
    /// `live` says whether a pointer's contact still continues, so a latch
    /// whose contact retired without reaching its binding's Up never counts.
    pub(crate) fn is_motion_latched(&self, live: impl Fn(PointerId) -> bool) -> bool {
        self.shared
            .contacts
            .motion_latches
            .borrow()
            .iter()
            .any(|pointer| live(*pointer))
    }

    /// `pointer`'s contact ended: its motion latch, if any, is released.
    pub(crate) fn release_motion_latch(&self, pointer: PointerId) {
        self.shared
            .contacts
            .motion_latches
            .borrow_mut()
            .retain(|latched| *latched != pointer);
    }

    fn allocate_slot(&self, pointer: PointerId) -> Rc<ArenaSlot> {
        // A fresh contact of this pointer owns no motion yet.
        self.release_motion_latch(pointer);
        let generation = self.shared.next_generation.get();
        self.shared.next_generation.set(
            generation
                .checked_add(1)
                .expect("BUG: gesture arena generation exhausted"),
        );
        let sequence = match self.shared.contacts.dispatched.get() {
            Some(DispatchedInput::Contact {
                pointer: dispatched,
                sequence,
            }) if dispatched == pointer => Some(sequence),
            _ => None,
        };
        Rc::new(ArenaSlot::new(
            pointer,
            sequence,
            ArenaGeneration(generation),
        ))
    }

    /// The input a binding is dispatching. Returns the value it replaces,
    /// for the binding to restore.
    pub(crate) fn set_dispatched(&self, input: Option<DispatchedInput>) -> Option<DispatchedInput> {
        self.shared.contacts.dispatched.replace(input)
    }

    /// The live cell naming the input a binding is dispatching.
    pub(crate) fn dispatched_cell(&self) -> DispatchedInputCell {
        Rc::clone(&self.shared.contacts.dispatched)
    }

    /// Share `mint` as this arena's issuer of sequence identities: the
    /// binding driving the arena and the arena then never issue one id twice.
    pub(crate) fn share_sequence_mint(&self, mint: Rc<IdMint<InputSequenceId>>) {
        let retired = self.shared.contacts.sequences.replace(mint);
        drop(retired);
    }

    /// The input sequence the binding is dispatching an event of: the
    /// contact's for a pointer event, the latch's for a wheel or trackpad
    /// packet, the action's for an assistive action. `None` outside a
    /// binding's dispatch.
    ///
    /// A scroll consumer reads it to name the sequence its deltas belong to
    /// (ADR-0209 §1).
    #[must_use]
    pub fn dispatched_sequence(&self) -> Option<InputSequenceId> {
        self.shared
            .contacts
            .dispatched
            .get()
            .map(DispatchedInput::sequence)
    }

    /// When the binding's latch of the wheel or trackpad packet being
    /// dispatched ends: the event time it lapses at unless another packet
    /// of its source comes first, or with this packet. `None` outside such
    /// a dispatch, and for a contact or an assistive action, whose ends the
    /// binding reports otherwise.
    ///
    /// A scroll consumer ends the sequence of its deltas there (ADR-0209
    /// §5), so the latch and the consumer's sequence end together and the
    /// next packet's new latch is a new sequence for both.
    #[must_use]
    pub fn dispatched_latch_end(&self) -> Option<ScrollLatchEnd> {
        match self.shared.contacts.dispatched.get()? {
            DispatchedInput::Latch { end, .. } => end,
            DispatchedInput::Contact { .. } | DispatchedInput::Contactless(_) => None,
        }
    }

    /// Issue the identity of a contactless input sequence from the issuer the
    /// binding driving this arena shares, so it never collides with a
    /// contact or latch id: a programmatic scroll, a reveal, keyboard
    /// scrolling or an auto-scroll.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`](flui_foundation::IdExhausted) once every identity was
    /// issued; the issuer refuses permanently rather than reissue one.
    pub fn begin_contactless_sequence(
        &self,
    ) -> Result<InputSequenceId, flui_foundation::IdExhausted> {
        let mint = Rc::clone(&self.shared.contacts.sequences.borrow());
        mint.next()
    }

    fn current_slot(&self, pointer: PointerId) -> Option<Rc<ArenaSlot>> {
        self.shared.entries.borrow().get(&pointer).cloned()
    }

    fn remove_current_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        let removed = {
            let mut entries = self.shared.entries.borrow_mut();
            if entries
                .get(&pointer)
                .is_some_and(|entry| Rc::ptr_eq(entry, slot))
            {
                entries.remove(&pointer)
            } else {
                None
            }
        };
        removed.is_some()
    }

    fn remove_retained_slot(&self, slot: &Rc<ArenaSlot>) -> bool {
        let removed = {
            let mut retained = self.shared.retained.borrow_mut();
            if retained
                .get(&slot.generation)
                .is_some_and(|entry| Rc::ptr_eq(entry, slot))
            {
                retained.remove(&slot.generation)
            } else {
                None
            }
        };
        removed.is_some()
    }

    fn remove_exact_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        let removed = self.remove_current_slot(pointer, slot) || self.remove_retained_slot(slot);
        if removed {
            self.shared.deadlines.withdraw_generation(slot.generation);
        }
        removed
    }

    fn is_live_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        self.shared
            .entries
            .borrow()
            .get(&pointer)
            .is_some_and(|entry| Rc::ptr_eq(entry, slot))
            || self
                .shared
                .retained
                .borrow()
                .get(&slot.generation)
                .is_some_and(|entry| Rc::ptr_eq(entry, slot))
    }

    fn queue_default_resolution(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) {
        if self.is_live_slot(pointer, slot) {
            self.shared
                .deferred
                .borrow_mut()
                .push_back(DeferredResolution {
                    pointer,
                    generation: slot.generation,
                    slot: Rc::downgrade(slot),
                });
        }
    }

    /// Whether `slot` is `pointer`'s active generation: its contact has not
    /// reached its terminal event, so a verdict hands a continuing contact over.
    fn is_active_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        self.shared
            .entries
            .borrow()
            .get(&pointer)
            .is_some_and(|entry| Rc::ptr_eq(entry, slot))
    }

    fn collect_follow_up(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        follow_up: ArenaFollowUp,
    ) -> PendingNotifications {
        // Successors whose hold left fail with it, which can change the verdict.
        let mut pending = slot.data.borrow_mut().reject_orphans();
        self.record_verdicts(slot, &pending, RejectionReason::HoldFailed);
        let follow_up = if pending.is_empty() {
            follow_up
        } else {
            slot.data.borrow().follow_up()
        };
        match follow_up {
            ArenaFollowUp::None => {}
            ArenaFollowUp::RemoveEmpty => {
                slot.data.borrow_mut().is_resolved = true;
                self.remove_exact_slot(pointer, slot);
            }
            ArenaFollowUp::DeferDefault => {
                self.queue_default_resolution(pointer, slot);
            }
            ArenaFollowUp::ResolveInFavorOf(winner) => {
                pending.extend(self.resolve_slot(
                    pointer,
                    slot,
                    winner.upgrade().as_ref(),
                    Resolution::Claimed,
                ));
            }
        }
        pending
    }

    /// Resolve `slot` for `winner`, adopting the successors its win carries
    /// while the contact continues. A contact past its terminal event
    /// resolves as [`Resolution::Terminal`] whatever `resolution` says.
    fn resolve_slot(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        winner: Option<&Rc<dyn GestureArenaMember>>,
        resolution: Resolution,
    ) -> PendingNotifications {
        let resolution = if self.is_active_slot(pointer, slot) {
            resolution
        } else {
            Resolution::Terminal
        };
        let (pending, carried) = {
            let mut data = slot.data.borrow_mut();
            // A moving member that wins a continuing contact latches it, by
            // claim or as the lone member.
            let latches = resolution != Resolution::Terminal
                && winner.is_some_and(|winner| data.moves(&Rc::downgrade(winner)));
            if latches
                && !self
                    .shared
                    .contacts
                    .motion_latches
                    .borrow()
                    .contains(&pointer)
            {
                self.shared
                    .contacts
                    .motion_latches
                    .borrow_mut()
                    .push(pointer);
            }
            let pending = data.resolve(winner, resolution);
            (pending, std::mem::take(&mut data.carried))
        };
        self.remove_exact_slot(pointer, slot);
        self.record_verdicts(slot, &pending, RejectionReason::Unresolved);
        self.shared
            .carried
            .borrow_mut()
            .extend(carried.into_iter().map(|carried| Carried {
                node: carried.node,
                contest: slot.contest(),
                member: carried.member,
                requested: carried.requested,
                feeder: carried.feeder,
            }));
        pending
    }

    /// Give a hold's won contact to its earliest live successor that asked
    /// for it, or else its earliest live successor.
    fn hand_off_carried(&self, node: &Rc<CompositionNode>, pointer: PointerId) -> bool {
        if self.shared.owner_closed.get() {
            return false;
        }
        let mut successors: SmallVec<[Carried; 2]> = SmallVec::new();
        self.shared.carried.borrow_mut().retain(|carried| {
            let mine = carried.contest.pointer() == pointer
                && carried
                    .node
                    .upgrade()
                    .is_some_and(|candidate| Rc::ptr_eq(&candidate, node));
            if mine {
                successors.push(carried.clone());
            }
            !mine
        });
        let live = |carried: &&Carried| carried.member.strong_count() != 0;
        let Some(chosen) = successors
            .iter()
            .filter(live)
            .find(|carried| carried.requested)
            .or_else(|| successors.iter().find(live))
            .cloned()
        else {
            return false;
        };
        let mut verdicts = CarriedVerdicts::new();
        for carried in successors {
            if !Weak::ptr_eq(&carried.member, &chosen.member) {
                self.record_carried(
                    &carried,
                    GestureDisposition::Rejected,
                    RejectionReason::Defeated,
                );
                verdicts.push((
                    carried.member,
                    GestureDisposition::Rejected,
                    carried.contest,
                ));
            }
        }
        self.record_carried(
            &chosen,
            GestureDisposition::Accepted,
            RejectionReason::Defeated,
        );
        if let Some(feeder) = &chosen.feeder {
            feeder.hand_off();
        }
        verdicts.push((chosen.member, GestureDisposition::Accepted, chosen.contest));
        self.deliver(verdicts);
        true
    }

    /// Remove the carried successors `ended` selects: their contact reached
    /// its end.
    fn take_carried(&self, ended: impl Fn(&Carried) -> bool) -> CarriedVerdicts {
        let taken: SmallVec<[Carried; 2]> = {
            let mut carried = self.shared.carried.borrow_mut();
            let taken = carried.iter().filter(|c| ended(c)).cloned().collect();
            carried.retain(|c| !ended(c));
            taken
        };
        taken
            .into_iter()
            .map(|carried| {
                self.record_carried(
                    &carried,
                    GestureDisposition::Rejected,
                    RejectionReason::Unresolved,
                );
                (
                    carried.member,
                    GestureDisposition::Rejected,
                    carried.contest,
                )
            })
            .collect()
    }

    /// Create a new gesture arena driven by the real OS clock.
    #[inline]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// Create a gesture arena with an explicit time source.
    ///
    /// Production uses [`new`](Self::new) (the OS clock); a headless frame driver
    /// passes a [`ManualClock`](flui_foundation::ManualClock) it advances per frame
    /// so deadline-driven recognizers resolve deterministically with no sleep.
    #[inline]
    pub fn with_clock(clock: Arc<dyn MonotonicClock>) -> Self {
        Self::assemble(clock, SweepModel::SelfDriven, VecDeque::new())
    }

    fn assemble(
        clock: Arc<dyn MonotonicClock>,
        sweep_model: SweepModel,
        deferred: VecDeque<DeferredResolution>,
    ) -> Self {
        let recovery = Rc::new(ArenaRecovery::default());
        Self {
            branch: None,
            pan_claim: None,
            settings: GestureSettingsProvider::default(),
            shared: Rc::new(ArenaShared {
                carried: RefCell::new(SmallVec::new()),
                entries: RefCell::new(BTreeMap::new()),
                retained: RefCell::new(BTreeMap::new()),
                deferred: RefCell::new(deferred),
                deadlines: Rc::new(DeadlineRegistry::new(Rc::clone(&recovery))),
                recovery,
                next_generation: Cell::new(1),
                contacts: ContactLedger::default(),
                owner_closed: Cell::new(false),
                close_mode: CloseTombstone::default(),
                observers: Rc::new(ObserverHub::with_clock(Arc::clone(&clock))),
                clock,
                sweep_model,
                frames: frame::PointerFrames::default(),
            }),
        }
    }

    /// This arena with compositions checked against `settings`
    /// ([`CompositionError::DelayCeilingExceeded`]). The handle shares every
    /// contact, branch and claim with `self`.
    #[must_use]
    pub fn with_settings(&self, settings: impl Into<GestureSettingsProvider>) -> Self {
        Self {
            settings: settings.into(),
            ..self.clone()
        }
    }

    /// This arena admitting members inside a region that declares `claim`
    /// (ADR-0208 rule 1): axial members outside the region may claim only
    /// the axes every enclosing region admits from free members inside it.
    #[must_use]
    pub fn with_pan_claim(&self, claim: PanClaim) -> Self {
        Self {
            pan_claim: Some(PanClaimNode::nested(claim, self.pan_claim.clone())),
            ..self.clone()
        }
    }

    /// Whether `self` and `other` admit members alike: into one arena, through
    /// one composition branch and inside one [`PanClaim`] region. The settings
    /// compositions are checked against may differ. A widget that built its
    /// recognizers with one handle rebuilds them when its scope hands it a
    /// handle that does not admit like it.
    #[must_use]
    pub fn admits_like(&self, other: &Self) -> bool {
        let same_branch = match (&self.branch, &other.branch) {
            (None, None) => true,
            (Some(ours), Some(theirs)) => {
                Rc::ptr_eq(&ours.node, &theirs.node) && ours.position == theirs.position
            }
            _ => false,
        };
        let same_claim = match (&self.pan_claim, &other.pan_claim) {
            (None, None) => true,
            (Some(ours), Some(theirs)) => Rc::ptr_eq(ours, theirs),
            _ => false,
        };
        Rc::ptr_eq(&self.shared, &other.shared) && same_branch && same_claim
    }

    /// Create a binding-owned gesture arena driven by the given clock.
    ///
    /// The returned arena answers [`SweepModel::BindingDriven`], so recognizers
    /// added to it never self-sweep when finishing tracking — the binding runs the
    /// exact contact lifecycle around routing each pointer event. On Up it
    /// detaches the contact before callbacks and sweeps that detached generation;
    /// on Cancel it abandons that generation without granting a winner. Composed
    /// standalone drivers use [`crate::GestureBinding`] too, rather than finishing
    /// an event by its reusable pointer ID after callbacks. This is the arena a
    /// [`HeadlessBinding`](https://docs.rs/flui-testing)
    /// or production `GestureBinding` hands down to a subtree.
    #[inline]
    pub fn binding_driven(clock: Arc<dyn MonotonicClock>) -> Self {
        Self::assemble(clock, SweepModel::BindingDriven, VecDeque::new())
    }

    /// Create a gesture arena with pre-allocated deferred-resolution capacity.
    ///
    /// This reserves queue storage; it imposes no contact or member admission
    /// limit. Standalone callers own admission and must close each Down after
    /// routing, sweep each Up (abandon on Cancel), drain deferred resolutions,
    /// and release held entry handles. Mounted consumers use `GestureBinding`,
    /// which owns normal progression and its simultaneous-contact limit.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::assemble(
            Arc::new(SystemClock),
            SweepModel::SelfDriven,
            VecDeque::with_capacity(capacity),
        )
    }

    /// Who owns this arena's close/sweep lifecycle.
    ///
    /// [`SelfDriven`](SweepModel::SelfDriven) for a low-level private arena
    /// (the recognizer sweeps itself);
    /// [`BindingDriven`](SweepModel::BindingDriven) for a binding-owned shared
    /// presentation arena.
    #[inline]
    pub fn sweep_model(&self) -> SweepModel {
        self.shared.sweep_model
    }

    /// The interaction clock: this arena's clock read on the event timeline
    /// (ADR-0207 §2). Every deadline a member arms is an [`EventTime`]
    /// compared with this reading; nothing in the arena reads the wall clock.
    #[inline]
    pub fn now(&self) -> EventTime {
        crate::events::interaction_now(self.shared.clock.as_ref())
    }

    /// The instant `time` names on this arena's clock, for velocity samples
    /// and gesture details that keep [`web_time::Instant`]s.
    #[inline]
    pub(crate) fn instant_at(&self, time: EventTime) -> web_time::Instant {
        // A sample past the last representable instant orders at the epoch:
        // no clock reading produces one.
        crate::events::instant_at(self.shared.clock.as_ref(), time)
            .unwrap_or_else(|| self.shared.clock.epoch())
    }

    /// The current reading as an instant, for a recognizer that only stamps
    /// velocity samples and arms no deadline from it.
    #[inline]
    pub(crate) fn sample_now(&self) -> web_time::Instant {
        self.shared.clock.now()
    }

    /// Register a time-based recognizer with the owner frame clock.
    ///
    /// The returned token controls the active lifetime. Arena resolution does
    /// not remove it; the recognizer drops it when its deadline fires or the
    /// pointer sequence terminates.
    ///
    /// Registration is inert: publish timing with the token's update handle
    /// after committing contact state. One registration covers the member's
    /// aggregate query and poll hook, even across several pointer memberships.
    ///
    /// # Errors
    /// Refuses a closed arena, a duplicate member registration or exhausted IDs.
    pub fn register_deadline_member(
        &self,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
    ) -> Result<GestureDeadlineRegistration, GestureDeadlineError> {
        let weak = Rc::downgrade(member);
        let generations = self
            .deadline_members_snapshot()
            .into_iter()
            .find(|poll| Weak::ptr_eq(&poll.member, &weak))
            .map_or_else(SmallVec::new, |poll| poll.generations);
        let contest = self.current_slot(pointer).map(|slot| slot.contest());
        self.shared.deadlines.register(contest, member, generations)
    }

    /// Register a deadline source that joins no contest with the owner frame
    /// clock: a timer of contactless input, such as the idle end of a wheel
    /// sequence's consumer. It is never offered a gesture; the arena polls
    /// it when its published deadline is due, and wakes the owner for it as
    /// it does for a recognizer's.
    ///
    /// Registration is inert, as with
    /// [`register_deadline_member`](Self::register_deadline_member): publish
    /// timing with the token's update handle.
    ///
    /// # Errors
    /// Refuses a closed arena, a duplicate member registration or exhausted IDs.
    pub fn register_deadline(
        &self,
        member: &Rc<dyn GestureArenaMember>,
    ) -> Result<GestureDeadlineRegistration, GestureDeadlineError> {
        self.shared
            .deadlines
            .register(None, member, SmallVec::new())
    }

    pub(crate) fn set_deadline_wake(
        &self,
        window: std::sync::Weak<dyn flui_platform::PlatformWindow>,
    ) {
        self.shared.observers.set_wake(window.clone());
        self.shared.deadlines.set_wake(window);
    }

    /// Add a member to the arena for a specific pointer.
    ///
    /// Returns a [`GestureArenaEntry`] handle that can be used to resolve
    /// the gesture later. This is the preferred pattern for recognizers.
    ///
    /// Creates a new arena entry if one doesn't exist for this pointer.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::rc::Rc;
    ///
    /// use flui_interaction::gesture::{GestureArena, GestureDisposition};
    /// use flui_interaction::ids::PointerId;
    /// use flui_interaction::gesture::{GestureArenaMember, GestureContest};
    ///
    /// struct R;
    /// impl GestureArenaMember for R {
    ///     fn accept_gesture(&self, _: GestureContest) {}
    ///     fn reject_gesture(&self, _: GestureContest) {}
    /// }
    ///
    /// let arena = GestureArena::new();
    /// let pointer = PointerId::new(core::num::NonZeroU64::MIN);
    /// let recognizer: Rc<R> = Rc::new(R);
    /// let entry = arena.add(pointer, &recognizer);
    /// // Resolve the gesture via the entry handle.
    /// entry.resolve(GestureDisposition::Accepted);
    /// ```
    #[instrument(
        name = "arena.add",
        level = "debug",
        skip(self, member),
        fields(
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::RecognizerAdded,
        )
    )]
    pub fn add<M: GestureArenaMember + 'static>(
        &self,
        pointer: PointerId,
        member: &Rc<M>,
    ) -> GestureArenaEntry {
        let member: Rc<dyn GestureArenaMember> = member.clone();
        self.add_erased(pointer, &member)
    }

    pub(crate) fn add_erased(
        &self,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
    ) -> GestureArenaEntry {
        let Some((entry, held_out)) =
            self.try_add_erased(pointer, member, AdmissionContext::default())
        else {
            return GestureArenaEntry {
                arena: self.clone(),
                // Generations are issued from 1: a refused entry names no
                // contest any verdict settles.
                contest: GestureContest::new(pointer, None, ArenaGeneration(0)),
                slot: Weak::new(),
                member: Rc::downgrade(member),
            };
        };
        self.deliver_held_out(held_out);
        entry
    }

    pub(crate) fn try_add_erased(
        &self,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
        context: AdmissionContext,
    ) -> Option<(GestureArenaEntry, HeldOut)> {
        if self.shared.owner_closed.get() {
            return None;
        }

        let slot = if let Some(slot) = self.current_slot(pointer) {
            slot
        } else {
            let slot = self.allocate_slot(pointer);
            self.shared
                .entries
                .borrow_mut()
                .insert(pointer, Rc::clone(&slot));
            slot
        };
        let rejected = slot.data.borrow_mut().add(
            member,
            self.branch.as_ref(),
            self.pan_claim.as_ref(),
            context,
        )?;
        self.shared
            .deadlines
            .admit_generation(member, slot.generation, slot.contest());
        self.shared.observers.record(|| GestureObservation::Joined {
            contest: slot.contest(),
            member: ObservedMember::new(Rc::downgrade(member)),
            composition: self.observed_composition(),
        });
        let entry = GestureArenaEntry::new(self.clone(), &slot, member);
        if !rejected.is_empty() {
            // A hold joined: the members it beats lose now, after its own
            // admission committed (ADR-0208 rule 4).
            self.record_verdicts(&slot, &rejected, RejectionReason::HeldOut);
            for (loser, _) in &rejected {
                self.shared
                    .deadlines
                    .withdraw_member_generation(loser, slot.generation);
            }
        }
        let held_out = HeldOut {
            pending: rejected,
            contest: slot.contest(),
        };
        Some((entry, held_out))
    }

    /// Tell the members a hold beat that they lost.
    pub(crate) fn deliver_held_out(&self, held_out: HeldOut) {
        if !held_out.pending.is_empty() {
            self.dispatch_pending(held_out.pending, held_out.contest);
        }
    }

    /// [`Self::deliver_held_out`] inside the caller's recovery operation.
    pub(crate) fn deliver_held_out_with_recovery(
        &self,
        held_out: HeldOut,
        recovery: &mut RecoveryScope<'_>,
    ) {
        Self::dispatch_pending_with_recovery(
            &self.shared.recovery,
            held_out.pending,
            held_out.contest,
            recovery,
        );
    }

    /// Close the arena for a pointer (no more members can be added).
    ///
    /// Called after the framework finishes dispatching the pointer down event.
    ///
    /// A lone eligible member is queued for [`Self::drain_deferred_resolutions`].
    /// An explicit acceptance through its entry after closure can settle it
    /// immediately. With competing members, an eligible eager winner resolves
    /// at closure; otherwise the arena waits for a member's decision.
    #[instrument(
        name = "arena.close",
        level = "debug",
        skip(self),
        fields(
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::ArenaClosed,
        )
    )]
    pub fn close(&self, pointer: PointerId) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let (closing, follow_up) = {
            let mut entry = slot.data.borrow_mut();
            (entry.is_open && !entry.is_resolved, entry.close())
        };
        if closing {
            self.shared.observers.record(|| GestureObservation::Closed {
                contest: slot.contest(),
            });
        }
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        self.dispatch_pending(pending, slot.contest());
    }

    /// Dispatch deferred member notifications after the per-entry borrow has
    /// been released. Keeping member callbacks out of the borrowed state is
    /// what makes the arena re-entrancy-safe (a handler may call back into the
    /// arena). See [`PendingNotifications`].
    #[inline]
    fn dispatch_pending(&self, pending: PendingNotifications, contest: GestureContest) {
        self.run_owned_recovery(|scope| {
            Self::dispatch_pending_with_recovery(&self.shared.recovery, pending, contest, scope);
        });
    }

    /// Deliver verdicts that each name their own contest.
    fn deliver(&self, verdicts: CarriedVerdicts) {
        self.run_owned_recovery(|scope| {
            Self::deliver_with_recovery(&self.shared.recovery, verdicts, scope);
        });
    }

    /// Deliver one contest's verdicts.
    fn dispatch_pending_with_recovery(
        context: &ArenaRecovery,
        pending: PendingNotifications,
        contest: GestureContest,
        recovery: &mut RecoveryScope<'_>,
    ) {
        Self::deliver_with_recovery(
            context,
            pending
                .into_iter()
                .map(|(member, disposition)| (member, disposition, contest)),
            recovery,
        );
    }

    /// Verdicts are committed; their opaque snapshots have separate retirement custody.
    fn deliver_with_recovery(
        context: &ArenaRecovery,
        verdicts: impl IntoIterator<Item = ContestVerdict>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        for (member, disposition, contest) in verdicts {
            let Some(member) = member.upgrade() else {
                continue;
            };
            context.invoke(FailureClass::Delivery, recovery, |_| match disposition {
                GestureDisposition::Accepted => member.accept_gesture(contest),
                GestureDisposition::Rejected => member.reject_gesture(contest),
            });
            Self::retire_member_with_recovery(context, member, recovery);
        }
    }

    fn retire_member_with_recovery(
        context: &ArenaRecovery,
        member: Rc<dyn GestureArenaMember>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        context.drain(recovery);
        if recovery.has_failure() || context.preserving() {
            member.retain();
        } else {
            context.invoke(FailureClass::Unknown, recovery, |_| drop(member));
        }
    }

    /// Internal method: resolve an entry with given disposition.
    ///
    /// Called by [`GestureArenaEntry::resolve`].
    fn resolve_entry(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        member: Rc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
    ) {
        self.run_owned_recovery(|scope| {
            self.resolve_entry_notifying(pointer, slot, member, disposition, None, scope);
        });
    }

    fn resolve_entry_notifying(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        member: Rc<dyn GestureArenaMember>,
        vote: impl Into<Vote>,
        excluded: Option<&Weak<dyn GestureArenaMember>>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        let vote = vote.into();
        if self.shared.owner_closed.get() {
            if self.shared.close_mode.mode() == CloseMode::PreservingFailure {
                member.retain();
            } else {
                Self::retire_member_with_recovery(&self.shared.recovery, member, recovery);
            }
            return;
        }
        let mut notifications = None;
        recovery.run_classified(FailureClass::Unknown, |_| {
            let (mut pending, follow_up) = {
                let mut entry = slot.data.borrow_mut();
                match vote {
                    Vote::Accept => (
                        PendingNotifications::new(),
                        self.claim(slot, &mut entry, &member, None),
                    ),
                    Vote::ClaimMotion(displacement) => (
                        PendingNotifications::new(),
                        self.claim(slot, &mut entry, &member, Some(displacement)),
                    ),
                    Vote::Reject => entry.reject(&member),
                }
            };
            self.record_verdicts(slot, &pending, RejectionReason::Withdrew);
            if vote == Vote::Reject {
                self.shared
                    .deadlines
                    .withdraw_member_generation(&Rc::downgrade(&member), slot.generation);
            }
            pending.extend(self.collect_follow_up(pointer, slot, follow_up));
            if let Some(excluded) = excluded {
                pending.retain(|(member, _)| !Weak::ptr_eq(member, excluded));
            }
            notifications = Some(pending);
        });
        if let Some(pending) = notifications {
            Self::dispatch_with_recovery(
                &self.shared.recovery,
                pending,
                slot.contest(),
                Some(member),
                recovery,
            );
        } else {
            Self::retire_member_with_recovery(&self.shared.recovery, member, recovery);
        }
    }

    fn dispatch_with_candidate(
        &self,
        pending: PendingNotifications,
        contest: GestureContest,
        candidate: Option<Rc<dyn GestureArenaMember>>,
    ) {
        self.run_owned_recovery(|scope| {
            Self::dispatch_with_recovery(&self.shared.recovery, pending, contest, candidate, scope);
        });
    }

    fn dispatch_with_recovery(
        context: &ArenaRecovery,
        pending: PendingNotifications,
        contest: GestureContest,
        candidate: Option<Rc<dyn GestureArenaMember>>,
        recovery: &mut RecoveryScope<'_>,
    ) {
        // Release the caller's snapshot before callbacks can release its owner.
        if let Some(candidate) = candidate {
            Self::retire_member_with_recovery(context, candidate, recovery);
        }
        Self::dispatch_pending_with_recovery(context, pending, contest, recovery);
    }

    /// Accept gesture for a member - the member wants to handle this gesture.
    ///
    /// If arena is open, stores as eager winner (wins when arena closes).
    /// If arena is closed, resolves immediately in favor of this member.
    /// The caller keeps ownership; the arena retains only weak membership.
    ///
    /// # Note
    ///
    /// Prefer using [`GestureArenaEntry::resolve`] instead of this method.
    pub fn accept(&self, pointer: PointerId, member: &Rc<dyn GestureArenaMember>) {
        let member = Rc::clone(member);
        if self.shared.owner_closed.get() {
            let mut failure = ClosePanic::for_rejection(self.shared.close_mode.mode());
            failure.retire(member);
            failure.finish();
            return;
        }
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let follow_up = self.claim(&slot, &mut slot.data.borrow_mut(), &member, None);
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        self.dispatch_with_candidate(pending, slot.contest(), Some(member));
    }

    /// Reject gesture for a member - the member doesn't want this gesture.
    ///
    /// Removes the member from the arena and notifies them.
    /// If only one member remains and arena is closed, they win.
    ///
    /// # Note
    ///
    /// Prefer using [`GestureArenaEntry::resolve`] instead of this method.
    pub fn reject(&self, pointer: PointerId, member: &Rc<dyn GestureArenaMember>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        self.resolve_entry(pointer, &slot, member.clone(), GestureDisposition::Rejected);
    }

    fn hold_slot(slot: &Rc<ArenaSlot>) {
        let mut entry = slot.data.borrow_mut();
        if !entry.is_resolved {
            entry.hold();
        }
    }

    fn release_slot_with_recovery(&self, slot: &Rc<ArenaSlot>, recovery: &mut RecoveryScope<'_>) {
        let mut should_sweep = false;
        recovery.run_classified(FailureClass::Unknown, |_| {
            let mut entry = slot.data.borrow_mut();
            entry.release();
            should_sweep = std::mem::take(&mut entry.has_pending_sweep);
        });
        if should_sweep {
            self.sweep_slot_with_recovery(slot, recovery);
        }
    }

    /// Resolve the arena with a specific winner.
    ///
    /// Winner receives `accept_gesture()`, all others receive
    /// `reject_gesture()`.
    /// The winner is borrowed; its caller remains an owner during delivery.
    ///
    /// # Note
    ///
    /// Prefer using [`GestureArenaEntry::resolve`] instead of this method.
    #[instrument(
        name = "arena.resolve",
        level = "debug",
        skip(self, winner),
        fields(
            pointer = ?pointer,
            has_winner = winner.is_some(),
            event = %crate::gesture::observability::GestureEvent::ArenaResolved,
        )
    )]
    pub fn resolve(&self, pointer: PointerId, winner: Option<&Rc<dyn GestureArenaMember>>) {
        let winner = winner.cloned();
        if self.shared.owner_closed.get() {
            let mut failure = ClosePanic::for_rejection(self.shared.close_mode.mode());
            failure.retire(winner);
            failure.finish();
            return;
        }
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        if let Some(candidate) = &winner {
            let follow_up = {
                let mut entry = slot.data.borrow_mut();
                entry.prune_departed();
                entry
                    .is_blocked(&Rc::downgrade(candidate))
                    .then(|| entry.accept(candidate))
            };
            if let Some(follow_up) = follow_up {
                let pending = self.collect_follow_up(pointer, &slot, follow_up);
                self.dispatch_with_candidate(pending, slot.contest(), winner);
                return;
            }
        }
        let pending = self.resolve_slot(pointer, &slot, winner.as_ref(), Resolution::Claimed);
        self.dispatch_with_candidate(pending, slot.contest(), winner);
    }

    /// Withdraw a single member from the arena, leaving the others to keep
    /// competing.
    ///
    /// Unlike [`resolve`](Self::resolve) with no winner — which resolves the
    /// whole entry and rejects *every* member — this removes only `member`.
    /// When exactly one member remains in a closed arena, that member wins
    /// (the caller is withdrawn without rejecting its competitors).
    pub fn reject_member(&self, pointer: PointerId, member: &Rc<dyn GestureArenaMember>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        self.resolve_entry(pointer, &slot, member.clone(), GestureDisposition::Rejected);
    }

    /// Sweep - remove resolved arenas for a pointer.
    ///
    /// Called when pointer is released to clean up.
    /// Forces resolution if arena is still open (first member wins).
    /// If arena is held, sweep is deferred until release().
    #[instrument(
        name = "arena.sweep",
        level = "debug",
        skip(self),
        fields(
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::ArenaSwept,
        )
    )]
    pub fn sweep(&self, pointer: PointerId) {
        if let Some(slot) = self.current_slot(pointer) {
            self.sweep_slot(&slot);
        } else {
            // A resolved contact ends: successors its hold carried never won.
            self.deliver(self.take_carried(|carried| carried.contest.pointer() == pointer));
        }
    }

    fn sweep_slot(&self, slot: &Rc<ArenaSlot>) {
        self.run_owned_recovery(|scope| self.sweep_slot_with_recovery(slot, scope));
    }

    fn sweep_slot_with_recovery(&self, slot: &Rc<ArenaSlot>, recovery: &mut RecoveryScope<'_>) {
        let mut pending = None;
        recovery.run_classified(FailureClass::Unknown, |_| {
            let mut entry = slot.data.borrow_mut();
            if entry.is_held || entry.has_blocked_fallback() {
                // The pointer is up: its held generation leaves the active
                // map, so the pointer's next Down opens a fresh arena instead
                // of being refused by this closed one. Exact entry handles and
                // `release(pointer)` still reach it among the retained slots.
                entry.has_pending_sweep = true;
                drop(entry);
                self.remove_current_slot(slot.pointer, slot);
                self.shared
                    .retained
                    .borrow_mut()
                    .insert(slot.generation, Rc::clone(slot));
                return;
            }
            let swept = entry.sweep();
            drop(entry);
            self.remove_exact_slot(slot.pointer, slot);
            self.record_verdicts(slot, &swept, RejectionReason::Unresolved);
            // Only this contest's own successors end with it: a retained
            // generation sweeps while the pointer's next contact, and the
            // successors its hold carries, may be down.
            let contest = slot.contest();
            let carried = self.take_carried(|carried| carried.contest == contest);
            pending = Some((swept, carried));
        });
        if let Some((swept, carried)) = pending {
            Self::dispatch_pending_with_recovery(
                &self.shared.recovery,
                swept,
                slot.contest(),
                recovery,
            );
            Self::deliver_with_recovery(&self.shared.recovery, carried, recovery);
        }
    }

    fn abandon_slot(&self, slot: &Rc<ArenaSlot>) {
        // Leave the maps first, so a reentrant rejection callback cannot reach
        // this generation again.
        if !self.remove_exact_slot(slot.pointer, slot) {
            return;
        }
        let pending = slot.data.borrow_mut().resolve(None, Resolution::Terminal);
        self.record_cancel(slot, ContestCancel::Abandoned, &pending);
        self.dispatch_pending(pending, slot.contest());
    }

    /// Tear down one interrupted pointer sequence without choosing a winner.
    ///
    /// Lifecycle loss and explicit cache invalidation can arrive without a
    /// matching pointer Cancel. Remove the slot before notifying recognizers,
    /// so re-entrancy or a panicking rejection callback cannot leave a stale
    /// competition behind.
    pub fn abandon(&self, pointer: PointerId) {
        let batch = self.detach(pointer);
        Self::abandon_detached(batch, ContestCancel::Abandoned);
    }

    /// Remove every generation for `pointer` from maps visible to new input.
    /// The returned owned batch keeps members alive until the caller finishes
    /// its causal route/lifecycle transaction.
    pub(crate) fn detach(&self, pointer: PointerId) -> DetachedArenaBatch {
        let mut slots = SmallVec::new();
        let active = self.shared.entries.borrow_mut().remove(&pointer);
        if let Some(slot) = active {
            slots.push(slot);
        }

        let mut retained_generations: SmallVec<[ArenaGeneration; 2]> = self
            .shared
            .retained
            .borrow()
            .iter()
            .filter(|(_, slot)| slot.pointer == pointer)
            .map(|(generation, _)| *generation)
            .collect();
        retained_generations.sort_unstable_by_key(|generation| generation.0);
        for generation in retained_generations {
            let retained = self.shared.retained.borrow_mut().remove(&generation);
            if let Some(slot) = retained {
                slots.push(slot);
            }
        }

        DetachedArenaBatch {
            slots,
            carried: self.take_carried(|carried| carried.contest.pointer() == pointer),
            deadlines: Rc::clone(&self.shared.deadlines),
            recovery: Rc::clone(&self.shared.recovery),
            observers: Rc::clone(&self.shared.observers),
        }
    }

    /// Run pointer-up sweep semantics over an already detached batch.
    pub(crate) fn sweep_detached(&self, batch: DetachedArenaBatch) {
        self.run_owned_recovery(|scope| {
            Self::deliver_with_recovery(&self.shared.recovery, batch.carried, scope);
            for slot in batch.slots {
                let pending = {
                    let mut entry = slot.data.borrow_mut();
                    if entry.is_held || entry.has_blocked_fallback() {
                        entry.has_pending_sweep = true;
                        self.shared
                            .retained
                            .borrow_mut()
                            .insert(slot.generation, Rc::clone(&slot));
                        continue;
                    }
                    entry.sweep()
                };
                batch.deadlines.withdraw_generation(slot.generation);
                self.record_verdicts(&slot, &pending, RejectionReason::Unresolved);
                Self::dispatch_pending_with_recovery(
                    &self.shared.recovery,
                    pending,
                    slot.contest(),
                    scope,
                );
            }
        });
    }

    /// Reject every member of an already detached batch without a winner.
    pub(crate) fn abandon_detached(batch: DetachedArenaBatch, cause: ContestCancel) {
        for slot in &batch.slots {
            batch.deadlines.withdraw_generation(slot.generation);
        }
        let context = Rc::clone(&batch.recovery);
        let observers = Rc::clone(&batch.observers);
        observe_after(&observers, || {
            context.standalone(|scope| {
                Self::deliver_with_recovery(&context, batch.carried, scope);
                for slot in batch.slots {
                    let pending = slot.data.borrow_mut().resolve(None, Resolution::Terminal);
                    record_cancel(&observers, &context, &slot, cause, &pending);
                    Self::dispatch_pending_with_recovery(&context, pending, slot.contest(), scope);
                }
            });
        });
    }

    /// Revoke the owner's authority and detach every generation without
    /// running member code; [`Self::retire_owner`] settles what it returns.
    pub(crate) fn withdraw_owner(&self) -> Vec<DetachedArenaBatch> {
        self.shared.owner_closed.set(true);
        self.shared.deadlines.close();
        let mut pointers: Vec<_> = self.shared.entries.borrow().keys().copied().collect();
        pointers.extend(
            self.shared
                .retained
                .borrow()
                .values()
                .map(|slot| slot.pointer),
        );
        pointers.sort_unstable();
        pointers.dedup();
        let batches: Vec<_> = pointers
            .into_iter()
            .map(|pointer| self.detach(pointer))
            .collect();
        self.shared.deferred.borrow_mut().clear();
        batches
    }

    /// Reject the members of generations [`Self::withdraw_owner`] detached,
    /// or retain them under a preserving close.
    pub(crate) fn retire_owner(&self, batches: Vec<DetachedArenaBatch>, mode: CloseMode) {
        fn notify(failure: &mut ClosePanic, verdicts: impl IntoIterator<Item = ContestVerdict>) {
            for (member, disposition, contest) in verdicts {
                let Some(member) = member.upgrade() else {
                    continue;
                };
                failure.run(|| match disposition {
                    GestureDisposition::Accepted => member.accept_gesture(contest),
                    GestureDisposition::Rejected => member.reject_gesture(contest),
                });
                failure.retire(member);
            }
        }
        let mut failure = ClosePanic::for_close(mode, self.shared.close_mode.clone());
        for batch in batches {
            // Carried successors hold only weak identities; a preserving close
            // has nothing of theirs to retain.
            if !failure.preserving() {
                notify(&mut failure, batch.carried);
            }
            for slot in batch.slots {
                if failure.preserving() {
                    failure.retire(crate::retain::Owned(slot));
                    continue;
                }
                let contest = slot.contest();
                let pending = slot.data.borrow_mut().resolve(None, Resolution::Terminal);
                notify(
                    &mut failure,
                    pending
                        .into_iter()
                        .map(|(member, disposition)| (member, disposition, contest)),
                );
                failure.retire(crate::retain::Owned(slot));
            }
        }
        failure.finish();
    }

    /// Tear down every interrupted pointer sequence without choosing winners.
    ///
    /// All slots are detached first. Recognizer notifications keep their
    /// normal stop-on-unwind behavior while the binding still ends with no
    /// live arena state.
    pub(crate) fn abandon_all(&self) {
        let mut pointers: Vec<PointerId> = self.shared.entries.borrow().keys().copied().collect();
        pointers.extend(
            self.shared
                .retained
                .borrow()
                .values()
                .map(|slot| slot.pointer),
        );
        pointers.sort_unstable();
        pointers.dedup();
        let batches: Vec<_> = pointers
            .into_iter()
            .map(|pointer| self.detach(pointer))
            .collect();

        for batch in &batches {
            for slot in &batch.slots {
                batch.deadlines.withdraw_generation(slot.generation);
            }
        }
        self.run_owned_recovery(|scope| {
            for batch in batches {
                Self::deliver_with_recovery(&self.shared.recovery, batch.carried, scope);
                for slot in batch.slots {
                    let pending = slot.data.borrow_mut().resolve(None, Resolution::Terminal);
                    self.record_cancel(&slot, ContestCancel::Abandoned, &pending);
                    Self::dispatch_pending_with_recovery(
                        &self.shared.recovery,
                        pending,
                        slot.contest(),
                        scope,
                    );
                }
            }
        });
    }

    /// Snapshot every recognizer whose deadline state is visible to a frame.
    ///
    /// Arena entries, held generations detached during pointer-up, and
    /// explicit timer registrations all participate. Exact recognizer
    /// identities are de-duplicated so a timer-owning competitor is queried
    /// or polled once.
    fn deadline_members_snapshot(&self) -> SmallVec<[DeadlinePoll; 8]> {
        let mut members: SmallVec<[DeadlinePoll; 8]> = SmallVec::new();
        let mut collect_slot = |slot: &ArenaSlot| {
            for member in &slot.data.borrow().members {
                if let Some(existing) = members
                    .iter_mut()
                    .find(|existing| Weak::ptr_eq(&existing.member, member))
                {
                    if !existing.generations.contains(&slot.generation) {
                        existing.generations.push(slot.generation);
                    }
                } else {
                    members.push(DeadlinePoll {
                        registration: None,
                        contest: Some(slot.contest()),
                        member: member.clone(),
                        generations: smallvec::smallvec![slot.generation],
                    });
                }
            }
        };
        for slot in self.shared.entries.borrow().values() {
            collect_slot(slot);
        }
        for slot in self.shared.retained.borrow().values() {
            collect_slot(slot);
        }
        for poll in self.shared.deadlines.snapshot() {
            if let Some(existing) = members
                .iter_mut()
                .find(|existing| Weak::ptr_eq(&existing.member, &poll.member))
            {
                existing.registration = poll.registration;
            } else {
                members.push(poll);
            }
        }
        members
    }

    pub(crate) fn deadlines_with_recovery(
        &self,
        now: EventTime,
        poll_due: bool,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) {
        if !self.shared.owner_closed.get() {
            self.shared.deadlines.refresh(
                self.deadline_members_snapshot(),
                now,
                poll_due,
                &self.shared.observers,
                recovery,
            );
        }
    }

    pub(crate) fn next_deadline_snapshot(&self) -> Option<EventTime> {
        self.shared.deadlines.next_snapshot()
    }

    pub(crate) fn has_pending_deadlines_snapshot(&self) -> bool {
        self.shared.deadlines.has_pending_snapshot()
    }

    /// Poll every active member's time-based deadline (e.g. long-press hold).
    ///
    /// Call once per frame from the UI thread. Members are snapshotted out of
    /// the per-entry borrows *before* polling, because a deadline hook may fire
    /// user callbacks and re-enter the arena to resolve — invoking it under the
    /// entry borrow would prevent arena reentry.
    /// Explicit deadline registrations remain visible after arena resolution,
    /// since the timers' lifetime is independent from the arena.
    /// Exact recognizer identities are de-duplicated, so a registered member
    /// that is also still competing is polled only once.
    /// Active slots are visited in ascending pointer identity order, followed
    /// by retained slots in generation order and explicit timer registrations
    /// in registration order.
    ///
    /// Snapshot complexity: O(P + M * U), where P counts active and retained
    /// slots, M counts memberships and timer registrations, and U counts unique
    /// member identities. Identity deduplication scans the collected members.
    /// The standalone arena imposes no admission cap;
    /// the binding limits admitted simultaneous contacts, not members per contact.
    pub fn poll_deadlines(&self) {
        if self.shared.owner_closed.get() {
            return;
        }
        self.run_owned_recovery(|scope| {
            let mut now = None;
            self.invoke_with_recovery(FailureClass::Unknown, scope, |_| {
                now = Some(self.now());
            });
            let Some(now) = now else { return };
            for poll in self.deadline_members_snapshot() {
                if poll
                    .registration
                    .is_some_and(|id| !self.shared.deadlines.contains(id))
                {
                    continue;
                }
                let Some(member) = poll.member.upgrade() else {
                    continue;
                };
                self.invoke_with_recovery(FailureClass::Unknown, scope, |_| {
                    if member.deadline().is_some_and(|due| due <= now) {
                        self.shared
                            .observers
                            .record(|| GestureObservation::DeadlineElapsed {
                                contest: poll.contest,
                                member: ObservedMember::new(poll.member.clone()),
                                now,
                            });
                        member.poll_deadline(now);
                    }
                });
                Self::retire_member_with_recovery(&self.shared.recovery, member, scope);
            }
        });
    }

    /// Whether any live member has an armed time-based deadline (see
    /// [`GestureArenaMember::deadline`]).
    ///
    /// The frame driver queries this once per frame, beside
    /// [`poll_deadlines`](Self::poll_deadlines), to keep producing frames
    /// while a deadline is pending. Members are snapshotted out of the
    /// per-entry borrows before querying — the same discipline `poll_deadlines`
    /// follows. The member's query is contractually pure, but remains foreign
    /// code that may unwind; this standalone entry finishes healthy queries
    /// before resuming the first failure. Runtime host getters use cached
    /// observation instead.
    pub fn has_pending_deadlines(&self) -> bool {
        self.next_deadline().is_some()
    }

    /// The earliest event time any live member's armed deadline will fire
    /// at, if any (ADR-0207 §2) — the wake counterpart of
    /// [`has_pending_deadlines`](Self::has_pending_deadlines). A caller
    /// computing a `ControlFlow::WaitUntil` target reads this instead of the
    /// boolean so a fully idle-but-armed presentation (nothing dirty, no
    /// running animation) still wakes at the right instant to resolve the
    /// deadline, rather than only lazily on the next unrelated event. Same
    /// snapshot/borrowing discipline as `has_pending_deadlines`.
    pub fn next_deadline(&self) -> Option<EventTime> {
        if self.shared.owner_closed.get() {
            return None;
        }
        let mut earliest = None;
        self.run_owned_recovery(|scope| {
            for poll in self.deadline_members_snapshot() {
                if poll
                    .registration
                    .is_some_and(|id| !self.shared.deadlines.contains(id))
                {
                    continue;
                }
                let Some(member) = poll.member.upgrade() else {
                    continue;
                };
                self.invoke_with_recovery(FailureClass::Unknown, scope, |_| {
                    if let Some(due) = member.deadline() {
                        earliest =
                            Some(earliest.map_or(due, |previous: EventTime| previous.min(due)));
                    }
                });
                Self::retire_member_with_recovery(&self.shared.recovery, member, scope);
            }
        });
        earliest
    }

    /// Get the number of active arenas.
    #[inline]
    pub fn len(&self) -> usize {
        self.shared.entries.borrow().len() + self.shared.retained.borrow().len()
    }

    /// Check if arena is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.shared.entries.borrow().is_empty() && self.shared.retained.borrow().is_empty()
    }

    /// Check if an arena exists for a pointer.
    #[inline]
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.shared.entries.borrow().contains_key(&pointer)
            || self
                .shared
                .retained
                .borrow()
                .values()
                .any(|slot| slot.pointer == pointer)
    }

    /// Whether `pointer` currently has a slot accepting this sequence's arena
    /// lifecycle (excluding held generations retained from earlier contacts).
    pub(crate) fn has_active(&self, pointer: PointerId) -> bool {
        self.shared.entries.borrow().contains_key(&pointer)
    }

    fn inspection_slot(&self, pointer: PointerId) -> Option<Rc<ArenaSlot>> {
        self.current_slot(pointer).or_else(|| {
            self.shared
                .retained
                .borrow()
                .values()
                .find(|slot| slot.pointer == pointer)
                .cloned()
        })
    }

    /// Check if an arena is held.
    pub fn is_held(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.borrow().is_held)
    }

    /// Check if an arena is open (accepting new members).
    pub fn is_open(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.borrow().is_open)
    }

    /// Check if an arena has an eager winner.
    pub fn has_eager_winner(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer).is_some_and(|slot| {
            let entry = slot.data.borrow();
            entry
                .eager_winner
                .as_ref()
                .is_some_and(|winner| !entry.is_blocked(winner))
        })
    }

    /// Check if sweep is pending for an arena.
    pub fn has_pending_sweep(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.borrow().has_pending_sweep)
    }

    /// Get the number of members in an arena.
    pub fn member_count(&self, pointer: PointerId) -> usize {
        self.inspection_slot(pointer)
            .map_or(0, |slot| slot.data.borrow().members.len())
    }

    /// Drain default, eager and released pointer-up decisions without losing debt.
    ///
    /// This is a typed owner-boundary queue, not an arbitrary closure
    /// executor. Each token carries the exact arena generation; rejection,
    /// explicit resolution, teardown, or pointer-ID reuse makes it stale.
    pub fn drain_deferred_resolutions(&self) -> usize {
        let slots: SmallVec<[Rc<ArenaSlot>; 8]> = self
            .shared
            .entries
            .borrow()
            .values()
            .chain(self.shared.retained.borrow().values())
            .cloned()
            .collect();
        let mut orphaned: SmallVec<[(GestureContest, PendingNotifications); 2]> = SmallVec::new();
        for slot in slots {
            let (orphans, follow_up) = {
                let mut entry = slot.data.borrow_mut();
                entry.prune_departed();
                let orphans = entry.reject_orphans();
                let follow_up = if entry.has_pending_sweep
                    && !entry.is_held
                    && !entry.is_resolved
                    && !entry.has_blocked_fallback()
                {
                    ArenaFollowUp::DeferDefault
                } else {
                    entry.follow_up()
                };
                (orphans, follow_up)
            };
            if !orphans.is_empty() {
                self.record_verdicts(&slot, &orphans, RejectionReason::HoldFailed);
                orphaned.push((slot.contest(), orphans));
            }
            match follow_up {
                ArenaFollowUp::DeferDefault => self.queue_default_resolution(slot.pointer, &slot),
                ArenaFollowUp::RemoveEmpty => {
                    self.remove_exact_slot(slot.pointer, &slot);
                }
                _ => {}
            }
        }
        let queued = std::mem::take(&mut *self.shared.deferred.borrow_mut());
        let mut resolved = 0;
        self.run_owned_recovery(|scope| {
            for (contest, orphans) in orphaned {
                Self::dispatch_pending_with_recovery(
                    &self.shared.recovery,
                    orphans,
                    contest,
                    scope,
                );
            }
            for token in queued {
                let Some(slot) = token.slot.upgrade() else {
                    continue;
                };
                if slot.generation != token.generation || !self.is_live_slot(token.pointer, &slot) {
                    continue;
                }

                enum Verdict {
                    Swept(PendingNotifications),
                    Winner(Option<Rc<dyn GestureArenaMember>>, Resolution),
                }
                let verdict = {
                    let mut entry = slot.data.borrow_mut();
                    entry.prune_departed();
                    if entry.has_pending_sweep && !entry.is_held && !entry.has_blocked_fallback() {
                        entry.has_pending_sweep = false;
                        Verdict::Swept(entry.sweep())
                    } else {
                        match entry.follow_up() {
                            ArenaFollowUp::DeferDefault => {
                                Verdict::Winner(entry.default_winner(), Resolution::Default)
                            }
                            ArenaFollowUp::ResolveInFavorOf(winner) => {
                                Verdict::Winner(winner.upgrade(), Resolution::Claimed)
                            }
                            ArenaFollowUp::None | ArenaFollowUp::RemoveEmpty => continue,
                        }
                    }
                };
                let pending = match verdict {
                    Verdict::Swept(pending) => {
                        self.remove_exact_slot(token.pointer, &slot);
                        self.record_verdicts(&slot, &pending, RejectionReason::Unresolved);
                        pending
                    }
                    Verdict::Winner(winner, resolution) => {
                        self.resolve_slot(token.pointer, &slot, winner.as_ref(), resolution)
                    }
                };
                resolved += 1;
                Self::dispatch_pending_with_recovery(
                    &self.shared.recovery,
                    pending,
                    slot.contest(),
                    scope,
                );
            }
        });
        resolved
    }
}

impl Default for GestureArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArena")
            .field("active_arenas", &self.shared.entries.borrow().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    static_assertions::assert_not_impl_any!(GestureArena: Send, Sync);
    static_assertions::assert_not_impl_any!(GestureArenaEntry: Send, Sync);

    // Mock arena member for testing - implement sealed trait
    struct MockMember {
        accepted: Rc<Mutex<bool>>,
        rejected: Rc<Mutex<bool>>,
    }

    impl MockMember {
        fn new() -> Self {
            Self {
                accepted: Rc::new(Mutex::new(false)),
                rejected: Rc::new(Mutex::new(false)),
            }
        }

        fn was_accepted(&self) -> bool {
            *self.accepted.lock()
        }

        fn was_rejected(&self) -> bool {
            *self.rejected.lock()
        }
    }

    impl GestureArenaMember for MockMember {
        fn accept_gesture(&self, _: GestureContest) {
            *self.accepted.lock() = true;
        }

        fn reject_gesture(&self, _: GestureContest) {
            *self.rejected.lock() = true;
        }
    }

    /// A member whose `reject_gesture` re-enters the arena — the real
    /// long-press / drag pattern (`reject_gesture -> state.reject() ->
    /// arena.resolve`). Before member notifications were deferred out of the
    /// locked region, this re-entry deadlocked on the non-reentrant per-entry
    /// `Mutex`.
    struct ReentrantMember {
        arena: GestureArena,
        rejected: Rc<Mutex<bool>>,
    }

    impl GestureArenaMember for ReentrantMember {
        fn accept_gesture(&self, _: GestureContest) {}

        fn reject_gesture(&self, contest: GestureContest) {
            *self.rejected.lock() = true;
            // Re-enter the arena from inside the reject callback.
            self.arena.resolve(contest.pointer(), None);
        }
    }

    struct OrderedMember {
        name: &'static str,
        calls: Rc<Mutex<Vec<&'static str>>>,
        panic_on_accept: bool,
    }

    impl GestureArenaMember for OrderedMember {
        fn accept_gesture(&self, _: GestureContest) {
            self.calls.lock().push(self.name);
            assert!(!self.panic_on_accept, "winner callback panic");
        }

        fn reject_gesture(&self, _: GestureContest) {
            self.calls.lock().push(self.name);
        }
    }

    // Arena failure and reentrancy matrix.
    #[test]
    fn arena_failure_and_reentrancy_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "explicit_resolution_removes_slot_rejects_losers_then_finishes_panicking_winner",
                explicit_resolution_removes_slot_rejects_losers_then_finishes_panicking_winner,
            ),
            (
                "reject_gesture_reentering_arena_does_not_deadlock",
                reject_gesture_reentering_arena_does_not_deadlock,
            ),
            (
                "stale_entry_cannot_resolve_a_reused_pointer_slot",
                stale_entry_cannot_resolve_a_reused_pointer_slot,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn reject_gesture_reentering_arena_does_not_deadlock() {
        use std::{sync::mpsc, time::Duration};

        // Run the arena work on a worker thread; a deadlock manifests as the
        // worker never reporting, caught by the receive timeout.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let arena = GestureArena::new();
            let pointer = PointerId::new(core::num::NonZeroU64::MIN);
            let reentrant = Rc::new(ReentrantMember {
                arena: arena.clone(),
                rejected: Rc::new(Mutex::new(false)),
            });
            let winner = Rc::new(MockMember::new());
            arena.add(pointer, &reentrant);
            arena.add(pointer, &winner);
            arena.close(pointer);
            // Resolve for `winner`; `reentrant` is rejected and its callback
            // re-enters the arena. Must complete without hanging.
            let candidate: Rc<dyn GestureArenaMember> = winner.clone();
            arena.resolve(pointer, Some(&candidate));
            let _ = tx.send((*reentrant.rejected.lock(), winner.was_accepted()));
        });

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((rejected, accepted)) => {
                assert!(rejected, "reentrant member should have been rejected");
                assert!(accepted, "winner should have been accepted");
            }
            Err(err) => panic!("arena deadlocked on reentrant reject_gesture: {err}"),
        }
    }

    // Arena resolution rules: deferred lone default winner, first eager winner.
    #[test]
    fn arena_resolution_rules_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "close_defers_a_lone_default_winner",
                close_defers_a_lone_default_winner,
            ),
            ("test_first_eager_winner_wins", test_first_eager_winner_wins),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn close_defers_a_lone_default_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(core::num::NonZeroU64::MIN);
        let member = Rc::new(MockMember::new());
        arena.add(pointer, &member);

        arena.close(pointer);

        assert!(
            !member.was_accepted(),
            "close alone must not resolve the final member; resolution is deferred"
        );
        assert!(arena.contains(pointer));
    }

    fn explicit_resolution_removes_slot_rejects_losers_then_finishes_panicking_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(core::num::NonZeroU64::MIN);
        let calls = Rc::new(Mutex::new(Vec::new()));
        let winner = Rc::new(OrderedMember {
            name: "winner.accept",
            calls: Rc::clone(&calls),
            panic_on_accept: true,
        });
        let loser = Rc::new(OrderedMember {
            name: "loser.reject",
            calls: Rc::clone(&calls),
            panic_on_accept: false,
        });
        arena.add(pointer, &winner);
        arena.add(pointer, &loser);
        arena.close(pointer);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let candidate: Rc<dyn GestureArenaMember> = winner.clone();
            arena.resolve(pointer, Some(&candidate));
        }));

        assert!(unwind.is_err(), "the earliest callback panic must resume");
        assert_eq!(
            calls.lock().as_slice(),
            ["loser.reject", "winner.accept"],
            "all losers are rejected in registration order before the winner is accepted"
        );
        assert!(
            !arena.contains(pointer),
            "the exact slot must be gone before any member callback"
        );
    }

    fn stale_entry_cannot_resolve_a_reused_pointer_slot() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(core::num::NonZeroU64::MIN);
        let old = Rc::new(MockMember::new());
        let stale_entry = arena.add(pointer, &old);
        arena.close(pointer);
        arena.sweep(pointer);

        let fresh = Rc::new(MockMember::new());
        let competitor = Rc::new(MockMember::new());
        arena.add(pointer, &fresh);
        arena.add(pointer, &competitor);
        arena.close(pointer);

        stale_entry.resolve(GestureDisposition::Accepted);

        assert!(arena.contains(pointer));
        assert!(!fresh.was_rejected());
        assert!(!competitor.was_rejected());
    }

    // ========================================================================
    // Eager Winner tests
    // ========================================================================

    fn test_first_eager_winner_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(core::num::NonZeroU64::MIN);

        let member1 = Rc::new(MockMember::new());
        let member2 = Rc::new(MockMember::new());

        let entry1 = arena.add(pointer, &member1);
        let entry2 = arena.add(pointer, &member2);

        // Both accept while arena is open - first wins
        entry1.resolve(GestureDisposition::Accepted);
        entry2.resolve(GestureDisposition::Accepted); // Ignored, already have eager winner

        arena.close(pointer);

        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
    }

    // ========================================================================
    // Pending Sweep tests
    // ========================================================================

    // ========================================================================
    // GestureArenaEntry tests
    // ========================================================================

    // ========================================================================
    // SweepModel
    // ========================================================================
}
