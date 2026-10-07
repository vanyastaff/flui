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
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
    sync::Arc,
};

use smallvec::SmallVec;
use tracing::instrument;
use web_time::Instant;

use crate::__runtime::{CloseMode, ClosePanic, CloseTombstone};
use crate::ids::PointerId;
use crate::retain::Retain;
use flui_foundation::{MonotonicClock, SystemClock};

mod composition;
use composition::{BranchPosition, CompositionBranch};
pub use composition::{CompositionError, GestureBranches, GestureCompetition};

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
///
/// ```rust
/// use flui_interaction::arena::{GestureArena, GestureArenaMember, GestureDisposition};
/// use flui_interaction::PointerId;
///
/// struct MyRecognizer;
///
/// impl GestureArenaMember for MyRecognizer {
///     fn accept_gesture(&self, pointer: PointerId) {
///         // Handle winning the arena
///     }
///     fn reject_gesture(&self, pointer: PointerId) {
///         // Handle losing the arena
///     }
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::new(core::num::NonZeroU64::MIN);
/// let recognizer = std::rc::Rc::new(MyRecognizer);
/// let entry = arena.add(pointer, &recognizer);
/// entry.resolve(GestureDisposition::Accepted);
/// arena.close(pointer);
/// ```
///
pub trait GestureArenaMember {
    /// Accept the gesture for this pointer.
    ///
    /// Called when this recognizer wins the arena for the given pointer.
    fn accept_gesture(&self, pointer: PointerId);

    /// Reject the gesture for this pointer.
    ///
    /// Called when another recognizer wins the arena, or this recognizer
    /// explicitly rejects the gesture.
    fn reject_gesture(&self, pointer: PointerId);

    /// Advance any time-based deadline this member owns (e.g. a long-press
    /// hold timer).
    ///
    /// Called once per frame by the binding's deadline tick so a deadline can
    /// elapse while the pointer is held still — without a further pointer event
    /// to drive it. The default is a no-op; only deadline-driven recognizers
    /// (long press) override it. Implementations must be idempotent across
    /// frames (firing at most once per deadline).
    fn poll_deadline(&self, now: Instant) {
        let _ = now;
    }

    /// The armed deadline, queried without invoking callbacks or calling the arena.
    /// The arena derives pending work from this one query and polls only due members.
    fn deadline(&self) -> Option<Instant> {
        None
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
/// use flui_interaction::arena::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::arena::GestureArenaMember;
///
/// struct R;
/// impl GestureArenaMember for R {
///     fn accept_gesture(&self, _: PointerId) {}
///     fn reject_gesture(&self, _: PointerId) {}
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ArenaGeneration(u64);

struct ArenaSlot {
    pointer: PointerId,
    generation: ArenaGeneration,
    data: RefCell<ArenaEntryData>,
}

impl ArenaSlot {
    fn new(pointer: PointerId, generation: ArenaGeneration) -> Self {
        Self {
            pointer,
            generation,
            data: RefCell::new(ArenaEntryData::new()),
        }
    }
}

#[derive(Clone)]
/// A stale-safe handle for one member in one arena generation.
///
/// Resolving a handle after its pointer ID has been reused cannot affect the
/// replacement arena.
pub struct GestureArenaEntry {
    arena: GestureArena,
    pointer: PointerId,
    generation: ArenaGeneration,
    slot: Weak<ArenaSlot>,
    member: Weak<dyn GestureArenaMember>,
}

/// Registration of one recognizer whose timer must outlive arena resolution.
///
/// Primary-pointer deadlines are scheduled independently from the
/// gesture arena: a lone long press can win the arena's default resolution on
/// Down and still fire after its hold timeout. FLUI drives those timers from
/// the owner frame clock, so this token keeps only the polling registration
/// alive. The registry itself stores a weak recognizer identity and therefore
/// cannot keep an unmounted recognizer alive or form an arena cycle.
pub(crate) struct GestureDeadlineRegistration {
    registry: Weak<DeadlineRegistry>,
    id: u64,
}

impl Drop for GestureDeadlineRegistration {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.unregister(self.id);
        }
    }
}

struct DeadlineWatcher {
    id: u64,
    pointer: PointerId,
    member: Weak<dyn GestureArenaMember>,
}

struct DeadlinePoll {
    registration: Option<u64>,
    pointer: PointerId,
    member: Weak<dyn GestureArenaMember>,
}

struct DeadlineRegistry {
    next_id: Cell<u64>,
    watchers: RefCell<Vec<DeadlineWatcher>>,
}

impl DeadlineRegistry {
    fn new() -> Self {
        Self {
            next_id: Cell::new(1),
            watchers: RefCell::new(Vec::new()),
        }
    }

    fn register(
        self: &Rc<Self>,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
    ) -> GestureDeadlineRegistration {
        let id = self.next_id.get();
        self.next_id.set(
            id.checked_add(1)
                .expect("BUG: gesture deadline registration ID exhausted"),
        );
        self.watchers.borrow_mut().push(DeadlineWatcher {
            id,
            pointer,
            member: Rc::downgrade(member),
        });
        GestureDeadlineRegistration {
            registry: Rc::downgrade(self),
            id,
        }
    }

    fn unregister(&self, id: u64) {
        self.watchers
            .borrow_mut()
            .retain(|watcher| watcher.id != id);
    }

    fn contains(&self, id: u64) -> bool {
        self.watchers
            .borrow()
            .iter()
            .any(|watcher| watcher.id == id)
    }

    fn snapshot(&self) -> SmallVec<[DeadlinePoll; 8]> {
        let mut watchers = self.watchers.borrow_mut();
        let mut live = SmallVec::new();
        watchers.retain(|watcher| {
            if watcher.member.strong_count() != 0 {
                live.push(DeadlinePoll {
                    registration: Some(watcher.id),
                    pointer: watcher.pointer,
                    member: watcher.member.clone(),
                });
                true
            } else {
                false
            }
        });
        live
    }
}

impl GestureArenaEntry {
    /// Create a new arena entry handle.
    fn new(
        arena: GestureArena,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        member: &Rc<dyn GestureArenaMember>,
    ) -> Self {
        Self {
            arena,
            pointer,
            generation: slot.generation,
            slot: Rc::downgrade(slot),
            member: Rc::downgrade(member),
        }
    }

    /// Resolve this entry with the given disposition.
    ///
    /// Call with [`GestureDisposition::Accepted`] to claim victory, or
    /// [`GestureDisposition::Rejected`] to admit defeat.
    ///
    /// It's safe to call this on an arena that has already been resolved.
    pub fn resolve(&self, disposition: GestureDisposition) {
        let (Some(slot), Some(member)) = (self.slot.upgrade(), self.member.upgrade()) else {
            return;
        };
        self.arena
            .resolve_entry(self.pointer, &slot, member, disposition);
    }

    /// Withdraw an already locally retired member without rejecting its later contact.
    pub(crate) fn reject_without_self(&self) {
        let (Some(slot), Some(member)) = (self.slot.upgrade(), self.member.upgrade()) else {
            return;
        };
        self.arena.resolve_entry_notifying(
            self.pointer,
            &slot,
            member,
            GestureDisposition::Rejected,
            Some(&self.member),
        );
    }

    /// Retire membership without executing user code from an owner's destructor.
    /// The weak identity remains comparable after its owner starts dropping.
    pub(crate) fn withdraw_deferred(&self) {
        if self.arena.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        if slot.generation != self.generation || !self.arena.is_live_slot(self.pointer, &slot) {
            return;
        }
        let follow_up = slot.data.borrow_mut().withdraw(&self.member);
        match follow_up {
            ArenaFollowUp::RemoveEmpty => {
                slot.data.borrow_mut().is_resolved = true;
                self.arena.remove_exact_slot(self.pointer, &slot);
            }
            ArenaFollowUp::DeferDefault | ArenaFollowUp::ResolveInFavorOf(_) => {
                self.arena.queue_default_resolution(self.pointer, &slot);
            }
            ArenaFollowUp::None => {}
        }
    }

    /// Hold this exact arena generation against a pointer-up sweep.
    pub fn hold(&self) {
        if self.arena.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            GestureArena::hold_slot(&slot);
        }
    }

    /// Release a hold on this exact arena generation.
    pub fn release(&self) {
        if self.arena.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            self.arena.release_slot(&slot);
        }
    }

    /// Release this exact generation's retained hold without notifying members.
    /// Owners call this for their held tokens before silent withdrawal in Drop.
    pub(crate) fn release_deferred(&self) {
        if self.arena.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        if slot.generation != self.generation || !self.arena.is_live_slot(self.pointer, &slot) {
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
            self.arena.queue_default_resolution(self.pointer, &slot);
        }
    }

    /// Sweep this exact arena generation.
    pub fn sweep(&self) {
        if self.arena.owner_closed.get() {
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
        if self.arena.owner_closed.get() {
            return;
        }
        if let Some(slot) = self.slot.upgrade() {
            self.arena.abandon_slot(&slot);
        }
    }

    /// Cancel the contact after this member has retired its local state.
    /// Its own stale rejection must not reach a reentrantly admitted contact.
    pub(crate) fn abandon_without_self(&self) {
        if self.arena.owner_closed.get() {
            return;
        }
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        if !self.arena.remove_exact_slot(slot.pointer, &slot) {
            return;
        }
        let mut pending = slot.data.borrow_mut().resolve(None);
        pending.retain(|(member, _)| !Weak::ptr_eq(member, &self.member));
        GestureArena::dispatch_pending(pending, slot.pointer);
    }

    /// Get the pointer ID for this entry.
    #[inline]
    pub fn pointer(&self) -> PointerId {
        self.pointer
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> Option<Rc<dyn GestureArenaMember>> {
        if self.arena.owner_closed.get() {
            return None;
        }
        self.member.upgrade()
    }
}

impl std::fmt::Debug for GestureArenaEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaEntry")
            .field("pointer", &self.pointer)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// ArenaEntryData (internal)
// ============================================================================

/// Arena entry for a single pointer.
///
/// Tracks which recognizers are competing for this pointer.
///
/// # Performance Optimization
///
/// Uses SmallVec with inline capacity of 4 to avoid heap allocations
/// for typical gesture scenarios (tap, drag, long-press, double-tap).
/// Most interactions have 2-3 competing recognizers.
struct ArenaEntryData {
    /// Members competing in this arena.
    /// Inline capacity: 4 (avoids heap for most cases).
    members: SmallVec<[Weak<dyn GestureArenaMember>; 4]>,
    branches: SmallVec<[(Weak<dyn GestureArenaMember>, CompositionBranch); 4]>,
    requested: SmallVec<[Weak<dyn GestureArenaMember>; 4]>,
    /// Whether the arena is still open for new members.
    /// When open, accepts are stored as eager_winner instead of resolving
    /// immediately.
    is_open: bool,
    /// Whether this entry is held open (waiting for more information).
    is_held: bool,
    /// Whether arena has been resolved.
    is_resolved: bool,
    /// Eager winner - first recognizer to accept while arena is open.
    /// When arena closes, eager winner wins immediately.
    eager_winner: Option<Weak<dyn GestureArenaMember>>,
    /// Whether sweep is pending (requested while held).
    has_pending_sweep: bool,
}

impl std::fmt::Debug for ArenaEntryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaEntryData")
            .field("member_count", &self.members.len())
            .field("is_open", &self.is_open)
            .field("is_held", &self.is_held)
            .field("is_resolved", &self.is_resolved)
            .field("has_eager_winner", &self.eager_winner.is_some())
            .field("has_pending_sweep", &self.has_pending_sweep)
            .finish()
    }
}

/// Member callbacks deferred out of the borrowed state.
///
/// Arena resolution must never invoke `accept_gesture`/`reject_gesture`
/// while the per-entry borrow is held: a member's handler may call back
/// into the arena (e.g. `reject_gesture` -> `state.reject()` ->
/// `arena.resolve`), which needs to borrow the same entry. Internal `ArenaEntryData` mutators
/// therefore return the pending notifications; the public `GestureArena`
/// methods dispatch them after releasing the borrow.
/// Pending verdicts keep only weak ownership: an earlier callback may release
/// a later member, so each member is upgraded immediately before its callback.
type PendingNotifications = SmallVec<[(Weak<dyn GestureArenaMember>, GestureDisposition); 4]>;

enum ArenaFollowUp {
    None,
    RemoveEmpty,
    DeferDefault,
    ResolveInFavorOf(Weak<dyn GestureArenaMember>),
}

impl ArenaEntryData {
    fn new() -> Self {
        Self {
            members: SmallVec::new(),
            branches: SmallVec::new(),
            requested: SmallVec::new(),
            is_open: true,
            is_held: false,
            is_resolved: false,
            eager_winner: None,
            has_pending_sweep: false,
        }
    }

    /// Close membership and report the resolution work the manager must run.
    fn close(&mut self) -> ArenaFollowUp {
        if !self.is_open || self.is_resolved {
            return ArenaFollowUp::None;
        }
        self.is_open = false;
        self.prune_departed();
        self.follow_up()
    }

    /// Accept gesture for a member.
    /// If arena is open, store as eager winner. If closed, resolve immediately.
    ///
    /// Only a current member can win: an accept from a member that already
    /// withdrew (or never joined) is ignored, since resolving in its favour
    /// would reject every remaining member and accept no one.
    #[must_use]
    ///
    /// The candidate is borrowed: the caller still owns it, so an ignored
    /// candidate that is its own last owner is dropped after the slot borrow is
    /// released, never inside it.
    fn accept(&mut self, member: &Rc<dyn GestureArenaMember>) -> ArenaFollowUp {
        self.prune_departed();
        let identity = Rc::downgrade(member);
        if self.is_resolved
            || !self
                .members
                .iter()
                .any(|entry| Weak::ptr_eq(entry, &identity))
        {
            return ArenaFollowUp::None;
        }

        if self.is_open {
            // Store as eager winner - will win when arena closes
            if !self
                .requested
                .iter()
                .any(|request| Weak::ptr_eq(request, &identity))
            {
                self.requested.push(identity.clone());
            }
            if self.eager_winner.is_none() && !self.is_blocked(&identity) {
                self.eager_winner = Some(identity);
            }
            // If already have eager winner, ignore subsequent accepts
            ArenaFollowUp::None
        } else {
            if self.is_blocked(&identity) {
                if !self
                    .requested
                    .iter()
                    .any(|request| Weak::ptr_eq(request, &identity))
                {
                    self.requested.push(identity);
                }
                ArenaFollowUp::None
            } else {
                ArenaFollowUp::ResolveInFavorOf(identity)
            }
        }
    }

    /// Reject gesture for a member.
    #[must_use]
    fn reject(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
    ) -> (PendingNotifications, ArenaFollowUp) {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return (pending, ArenaFollowUp::None);
        }

        let Some(index) = self
            .members
            .iter()
            .position(|entry| Weak::ptr_eq(entry, &Rc::downgrade(member)))
        else {
            return (pending, ArenaFollowUp::None);
        };
        let rejected = self.members.remove(index);

        // Remove from eager winner if it was this member
        if let Some(ref eager) = self.eager_winner
            && Weak::ptr_eq(eager, &Rc::downgrade(member))
        {
            self.eager_winner = None;
        }

        // Defer the member's rejection callback (dispatched after the entry
        // borrow is released to permit arena reentry).
        pending.push((rejected, GestureDisposition::Rejected));
        self.prune_departed();

        let follow_up = if self.is_open {
            ArenaFollowUp::None
        } else {
            self.follow_up()
        };
        (pending, follow_up)
    }

    fn follow_up(&self) -> ArenaFollowUp {
        if self.is_resolved || self.is_open {
            return ArenaFollowUp::None;
        }

        if self.members.len() == 1 && !self.is_blocked(&self.members[0]) {
            ArenaFollowUp::DeferDefault
        } else if self.members.is_empty() {
            ArenaFollowUp::RemoveEmpty
        } else if let Some(eager) = self
            .eager_winner
            .clone()
            .filter(|eager| !self.is_blocked(eager))
        {
            ArenaFollowUp::ResolveInFavorOf(eager)
        } else if let Some(request) = self
            .requested
            .iter()
            .find(|request| !self.is_blocked(request))
        {
            ArenaFollowUp::ResolveInFavorOf(request.clone())
        } else {
            ArenaFollowUp::None
        }
    }

    fn withdraw(&mut self, member: &Weak<dyn GestureArenaMember>) -> ArenaFollowUp {
        if self.is_resolved {
            return ArenaFollowUp::None;
        }
        let Some(index) = self
            .members
            .iter()
            .position(|entry| Weak::ptr_eq(entry, member))
        else {
            return ArenaFollowUp::None;
        };
        self.members.remove(index);
        if self
            .eager_winner
            .as_ref()
            .is_some_and(|eager| Weak::ptr_eq(eager, member))
        {
            self.eager_winner = None;
        }
        self.prune_departed();
        self.follow_up()
    }

    fn prune_departed(&mut self) {
        self.members.retain(|member| member.strong_count() != 0);
        self.branches
            .retain(|(member, _)| self.members.iter().any(|live| Weak::ptr_eq(live, member)));
        self.requested
            .retain(|member| self.members.iter().any(|live| Weak::ptr_eq(live, member)));
        if self
            .eager_winner
            .as_ref()
            .is_some_and(|member| member.strong_count() == 0)
        {
            self.eager_winner = None;
        }
    }

    fn is_blocked(&self, member: &Weak<dyn GestureArenaMember>) -> bool {
        let Some((_, branch)) = self
            .branches
            .iter()
            .find(|(candidate, _)| Weak::ptr_eq(candidate, member))
        else {
            return false;
        };
        self.branches.iter().any(|(_, other)| branch.blocks(other))
    }

    fn has_blocked_fallback(&mut self) -> bool {
        self.prune_departed();
        self.members.iter().any(|member| self.is_blocked(member))
    }

    /// Add a member to this arena.
    fn add(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
        branch: Option<&CompositionBranch>,
    ) -> bool {
        if self.is_open && !self.is_resolved {
            if branch.is_some()
                && self
                    .members
                    .iter()
                    .any(|existing| Weak::ptr_eq(existing, &Rc::downgrade(member)))
            {
                return false;
            }
            self.members.push(Rc::downgrade(member));
            if let Some(branch) = branch {
                self.branches.push((Rc::downgrade(member), branch.clone()));
            }
            true
        } else {
            false
        }
    }

    /// Hold the arena open (delay resolution).
    fn hold(&mut self) {
        self.is_held = true;
    }

    /// Release the hold on this arena.
    fn release(&mut self) {
        self.is_held = false;
    }

    /// Resolve the arena with a single winner.
    ///
    /// Losers are reported in registration order before winner callbacks.
    #[must_use]
    fn resolve(&mut self, winner: Option<&Rc<dyn GestureArenaMember>>) -> PendingNotifications {
        self.resolve_weak(winner.map(Rc::downgrade).as_ref())
    }

    fn resolve_weak(
        &mut self,
        winner: Option<&Weak<dyn GestureArenaMember>>,
    ) -> PendingNotifications {
        if self.is_resolved {
            return PendingNotifications::new();
        }

        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        let mut losers = PendingNotifications::new();
        let mut accepted = PendingNotifications::new();

        // Explicit/eager resolution rejects every loser before
        // accepting the winner. This ordering is observable when callbacks
        // re-enter or panic.
        for member in members {
            let is_winner = winner
                .as_ref()
                .is_some_and(|winner| Weak::ptr_eq(&member, winner));
            if is_winner {
                accepted.push((member, GestureDisposition::Accepted));
            } else {
                losers.push((member, GestureDisposition::Rejected));
            }
        }

        losers.extend(accepted);
        losers
    }

    /// Sweep ordering is intentionally different from an explicit
    /// resolution: the front member is accepted first, then later members are
    /// rejected in registration order.
    #[must_use]
    fn sweep(&mut self) -> PendingNotifications {
        let mut pending = PendingNotifications::new();
        if self.is_resolved {
            return pending;
        }
        self.prune_departed();
        if let Some(request) = self
            .requested
            .iter()
            .find(|request| {
                !self.is_blocked(request)
                    && self.branches.iter().any(|(member, branch)| {
                        Weak::ptr_eq(member, request)
                            && branch.position == BranchPosition::Second
                            && *branch.competition == GestureCompetition::RequireFirstFailure
                    })
            })
            .cloned()
        {
            return self.resolve_weak(Some(&request));
        }
        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        let mut live = members
            .into_iter()
            .filter(|member| member.strong_count() != 0);
        if let Some(winner) = live.next() {
            pending.push((winner, GestureDisposition::Accepted));
            pending.extend(live.map(|member| (member, GestureDisposition::Rejected)));
        }
        pending
    }
}

// ============================================================================
// SweepModel
// ============================================================================

/// Who owns the close/sweep lifecycle of an arena.
///
/// The contract is that the *binding* — not a recognizer — drives
/// `close(pointer)` on pointer-down and `sweep(pointer)` on pointer-up.
///
/// - [`SelfDriven`](Self::SelfDriven) — a low-level recognizer owns its private
///   arena lifecycle, so [`RecognizerBase::stop_tracking`] sweeps on up. This
///   model is for standalone recognizer use and focused recognizer tests, never
///   a presentation widget subtree.
/// - [`BindingDriven`](Self::BindingDriven) — a binding owns the arena and runs
///   the close/sweep lifecycle after routing each pointer event to the hit-test
///   path. Recognizers below it must *not* self-sweep: a tap's own
///   `stop_tracking → sweep` on the first up would force-resolve a shared entry
///   to the front member before a double-tap (or a peer detector) could
///   complete.
///
/// The model is immutable per arena, like the clock; every clone observes it.
///
/// [`RecognizerBase::stop_tracking`]: crate::recognizers::RecognizerBase::stop_tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepModel {
    /// A standalone recognizer owns the lifecycle of its private arena.
    SelfDriven,
    /// A binding owns the lifecycle (shared arena).
    BindingDriven,
}

/// Run the binding-owned close/sweep lifecycle for a single pointer event.
///
/// On `PointerDown` the arena is closed; on `PointerUp` it is swept. Every
/// other event — including `PointerCancel` — is a no-op for the arena
/// lifecycle (recognizers self-reject on cancel; sweeping on cancel would
/// force the first member to win an interrupted gesture). Callers run
/// their own route (hit-test dispatch) step *first*, then call this kernel
/// — the route-before-sweep order is load-bearing (it lets a double-tap's
/// first-up `hold` run before the sweep, so the sweep observes the hold
/// and defers). A held arena leaves the pointer's active slot on that sweep,
/// so the pointer's next Down opens a fresh arena.
///
/// No workspace code calls it: `GestureBinding` runs the same sequence inline
/// (`binding.rs`). Kept public for standalone arena users and tests.
pub fn run_pointer_lifecycle(arena: &GestureArena, event: &crate::events::PointerEvent) {
    use crate::events::PointerEvent;
    match event {
        PointerEvent::Down(data) => arena.close(data.pointer.id),
        PointerEvent::Up(data) => arena.sweep(data.pointer.id),
        _ => {}
    }
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
/// use flui_interaction::arena::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::arena::GestureArenaMember;
///
/// // A minimal recogniser that counts accepts/rejects. Use a real
/// // `TapGestureRecognizer` / `DragGestureRecognizer` in production —
/// // this is the minimum surface to participate in the arena.
/// #[derive(Debug)]
/// struct Counter(AtomicUsize, AtomicUsize);
/// impl GestureArenaMember for Counter {
///     fn accept_gesture(&self, _: PointerId) { self.0.fetch_add(1, Ordering::Relaxed); }
///     fn reject_gesture(&self, _: PointerId) { self.1.fetch_add(1, Ordering::Relaxed); }
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
    branch: Option<CompositionBranch>,
    owner_closed: Rc<Cell<bool>>,
    close_mode: CloseTombstone,
    /// Active exact-generation slots, visited in ascending pointer order.
    entries: Rc<RefCell<BTreeMap<PointerId, Rc<ArenaSlot>>>>,
    /// Held arenas detached from the active pointer map during an Up
    /// transaction. Exact entry tokens can still release these generations,
    /// while a reused pointer ID opens a fresh active slot. Retained slots
    /// are visited in ascending generation order.
    retained: Rc<RefCell<BTreeMap<ArenaGeneration, Rc<ArenaSlot>>>>,
    /// Typed queue of deferred single-member resolutions.
    deferred: Rc<RefCell<VecDeque<DeferredResolution>>>,
    /// Owner-frame deadline polling, independent from arena-slot lifetime.
    deadlines: Rc<DeadlineRegistry>,
    next_generation: Rc<Cell<u64>>,
    /// The time source deadline-driven recognizers read `now()` from. Defaults
    /// to the OS clock; a headless frame driver injects a `ManualClock` so a
    /// deadline (e.g. long-press) elapses deterministically without sleeping.
    clock: Arc<dyn MonotonicClock>,
    /// Who owns the close/sweep lifecycle. Immutable per arena (like the clock);
    /// recognizers read it to decide whether `stop_tracking` should self-sweep.
    sweep_model: SweepModel,
}

struct DeferredResolution {
    pointer: PointerId,
    generation: ArenaGeneration,
    slot: Weak<ArenaSlot>,
}

pub(crate) struct DetachedArenaBatch {
    pointer: PointerId,
    slots: SmallVec<[Rc<ArenaSlot>; 2]>,
}

impl GestureArena {
    pub(crate) fn close_tombstone(&self) -> CloseTombstone {
        self.close_mode.clone()
    }

    fn allocate_slot(&self, pointer: PointerId) -> Rc<ArenaSlot> {
        let generation = self.next_generation.get();
        self.next_generation.set(
            generation
                .checked_add(1)
                .expect("BUG: gesture arena generation exhausted"),
        );
        Rc::new(ArenaSlot::new(pointer, ArenaGeneration(generation)))
    }

    fn current_slot(&self, pointer: PointerId) -> Option<Rc<ArenaSlot>> {
        self.entries.borrow().get(&pointer).cloned()
    }

    fn remove_current_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        let removed = {
            let mut entries = self.entries.borrow_mut();
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
            let mut retained = self.retained.borrow_mut();
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
        self.remove_current_slot(pointer, slot) || self.remove_retained_slot(slot)
    }

    fn is_live_slot(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) -> bool {
        self.entries
            .borrow()
            .get(&pointer)
            .is_some_and(|entry| Rc::ptr_eq(entry, slot))
            || self
                .retained
                .borrow()
                .get(&slot.generation)
                .is_some_and(|entry| Rc::ptr_eq(entry, slot))
    }

    fn queue_default_resolution(&self, pointer: PointerId, slot: &Rc<ArenaSlot>) {
        if self.is_live_slot(pointer, slot) {
            self.deferred.borrow_mut().push_back(DeferredResolution {
                pointer,
                generation: slot.generation,
                slot: Rc::downgrade(slot),
            });
        }
    }

    fn collect_follow_up(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        follow_up: ArenaFollowUp,
    ) -> PendingNotifications {
        match follow_up {
            ArenaFollowUp::None => PendingNotifications::new(),
            ArenaFollowUp::RemoveEmpty => {
                slot.data.borrow_mut().is_resolved = true;
                self.remove_exact_slot(pointer, slot);
                PendingNotifications::new()
            }
            ArenaFollowUp::DeferDefault => {
                self.queue_default_resolution(pointer, slot);
                PendingNotifications::new()
            }
            ArenaFollowUp::ResolveInFavorOf(winner) => {
                let winner = winner.upgrade();
                let pending = slot.data.borrow_mut().resolve(winner.as_ref());
                self.remove_exact_slot(pointer, slot);
                pending
            }
        }
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
        Self {
            branch: None,
            entries: Rc::new(RefCell::new(BTreeMap::new())),
            retained: Rc::new(RefCell::new(BTreeMap::new())),
            deferred: Rc::new(RefCell::new(VecDeque::new())),
            deadlines: Rc::new(DeadlineRegistry::new()),
            next_generation: Rc::new(Cell::new(1)),
            owner_closed: Rc::new(Cell::new(false)),
            close_mode: CloseTombstone::default(),
            clock,
            sweep_model: SweepModel::SelfDriven,
        }
    }

    /// Create a binding-owned gesture arena driven by the given clock.
    ///
    /// The returned arena answers [`SweepModel::BindingDriven`], so recognizers
    /// added to it never self-sweep in `stop_tracking` — the binding runs the
    /// close/sweep lifecycle via [`run_pointer_lifecycle`] after routing each
    /// pointer event. This is the arena a [`HeadlessBinding`](https://docs.rs/flui-testing)
    /// or production `GestureBinding` hands down to a subtree.
    #[inline]
    pub fn binding_driven(clock: Arc<dyn MonotonicClock>) -> Self {
        Self {
            branch: None,
            entries: Rc::new(RefCell::new(BTreeMap::new())),
            retained: Rc::new(RefCell::new(BTreeMap::new())),
            deferred: Rc::new(RefCell::new(VecDeque::new())),
            deadlines: Rc::new(DeadlineRegistry::new()),
            next_generation: Rc::new(Cell::new(1)),
            owner_closed: Rc::new(Cell::new(false)),
            close_mode: CloseTombstone::default(),
            clock,
            sweep_model: SweepModel::BindingDriven,
        }
    }

    /// Create a gesture arena with pre-allocated deferred-resolution capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            branch: None,
            entries: Rc::new(RefCell::new(BTreeMap::new())),
            retained: Rc::new(RefCell::new(BTreeMap::new())),
            deferred: Rc::new(RefCell::new(VecDeque::with_capacity(capacity))),
            deadlines: Rc::new(DeadlineRegistry::new()),
            next_generation: Rc::new(Cell::new(1)),
            owner_closed: Rc::new(Cell::new(false)),
            close_mode: CloseTombstone::default(),
            clock: Arc::new(SystemClock),
            sweep_model: SweepModel::SelfDriven,
        }
    }

    /// Who owns this arena's close/sweep lifecycle.
    ///
    /// [`SelfDriven`](SweepModel::SelfDriven) for a low-level private arena
    /// (the recognizer sweeps itself);
    /// [`BindingDriven`](SweepModel::BindingDriven) for a binding-owned shared
    /// presentation arena.
    #[inline]
    pub fn sweep_model(&self) -> SweepModel {
        self.sweep_model
    }

    /// The current instant on this arena's clock — the time a deadline-driven
    /// recognizer compares its captured down-time against.
    #[inline]
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Register a time-based recognizer with the owner frame clock.
    ///
    /// The returned token controls the active lifetime. Arena resolution does
    /// not remove it; the recognizer drops it when its deadline fires or the
    /// pointer sequence terminates.
    pub(crate) fn register_deadline_member(
        &self,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
    ) -> GestureDeadlineRegistration {
        self.deadlines.register(pointer, member)
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
    /// use flui_interaction::arena::{GestureArena, GestureDisposition};
    /// use flui_interaction::ids::PointerId;
    /// use flui_interaction::arena::GestureArenaMember;
    ///
    /// struct R;
    /// impl GestureArenaMember for R {
    ///     fn accept_gesture(&self, _: PointerId) {}
    ///     fn reject_gesture(&self, _: PointerId) {}
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
            event = %crate::observability::GestureEvent::RecognizerAdded,
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
        self.try_add_erased(pointer, member)
            .unwrap_or_else(|| GestureArenaEntry {
                arena: self.clone(),
                pointer,
                generation: ArenaGeneration(0),
                slot: Weak::new(),
                member: Rc::downgrade(member),
            })
    }

    pub(crate) fn try_add_erased(
        &self,
        pointer: PointerId,
        member: &Rc<dyn GestureArenaMember>,
    ) -> Option<GestureArenaEntry> {
        if self.owner_closed.get() {
            return None;
        }

        let slot = match self.current_slot(pointer) {
            Some(slot) => slot,
            None => {
                let slot = self.allocate_slot(pointer);
                self.entries.borrow_mut().insert(pointer, Rc::clone(&slot));
                slot
            }
        };
        let admitted = slot.data.borrow_mut().add(member, self.branch.as_ref());
        if !admitted {
            return None;
        }

        Some(GestureArenaEntry::new(self.clone(), pointer, &slot, member))
    }

    /// Close the arena for a pointer (no more members can be added).
    ///
    /// Called after the framework finishes dispatching the pointer down event.
    ///
    /// If there's an eager winner, they win immediately.
    /// If there's only one member, it wins automatically.
    /// Otherwise, waits for members to accept/reject.
    #[instrument(
        name = "arena.close",
        level = "debug",
        skip(self),
        fields(
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::ArenaClosed,
        )
    )]
    pub fn close(&self, pointer: PointerId) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let follow_up = slot.data.borrow_mut().close();
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        Self::dispatch_pending(pending, pointer);
    }

    /// Dispatch deferred member notifications after the per-entry borrow has
    /// been released. Keeping member callbacks out of the borrowed state is
    /// what makes the arena re-entrancy-safe (a handler may call back into the
    /// arena). See [`PendingNotifications`].
    #[inline]
    fn dispatch_pending(pending: PendingNotifications, pointer: PointerId) {
        if let Some(payload) = Self::dispatch_pending_capturing(pending, pointer) {
            resume_unwind(payload);
        }
    }

    fn dispatch_pending_capturing(
        pending: PendingNotifications,
        pointer: PointerId,
    ) -> Option<Box<dyn Any + Send>> {
        let mut first_panic = None;
        Self::dispatch_pending_into(pending, pointer, &mut first_panic);
        first_panic
    }

    fn dispatch_pending_into(
        pending: PendingNotifications,
        pointer: PointerId,
        first_panic: &mut Option<Box<dyn Any + Send>>,
    ) {
        for (member, disposition) in pending {
            let Some(member) = member.upgrade() else {
                continue;
            };
            let candidate = catch_unwind(AssertUnwindSafe(|| match disposition {
                GestureDisposition::Accepted => member.accept_gesture(pointer),
                GestureDisposition::Rejected => member.reject_gesture(pointer),
            }))
            .err();
            Self::preserve_first_panic(first_panic, candidate, pointer);
            if first_panic.is_some() || std::thread::panicking() {
                member.retain();
            } else {
                let drop_candidate = catch_unwind(AssertUnwindSafe(|| drop(member))).err();
                Self::preserve_first_panic(first_panic, drop_candidate, pointer);
            }
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
        self.resolve_entry_notifying(pointer, slot, member, disposition, None);
    }

    fn resolve_entry_notifying(
        &self,
        pointer: PointerId,
        slot: &Rc<ArenaSlot>,
        member: Rc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
        excluded: Option<&Weak<dyn GestureArenaMember>>,
    ) {
        if self.owner_closed.get() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(member);
            failure.finish();
            return;
        }
        let (mut pending, follow_up) = {
            let mut entry = slot.data.borrow_mut();
            match disposition {
                GestureDisposition::Accepted => {
                    let follow_up = entry.accept(&member);
                    (PendingNotifications::new(), follow_up)
                }
                GestureDisposition::Rejected => entry.reject(&member),
            }
        };
        pending.extend(self.collect_follow_up(pointer, slot, follow_up));
        if let Some(excluded) = excluded {
            pending.retain(|(member, _)| !Weak::ptr_eq(member, excluded));
        }
        Self::dispatch_with_candidate(pending, pointer, Some(member));
    }

    /// Drop the caller's candidate after the slot borrow is released and
    /// before any callback runs, so a panicking callback can never leave it as
    /// the last owner to be destroyed during that unwind. A panic from its own
    /// destructor is held and resumed once the callbacks were dispatched.
    fn retire_candidate(
        candidate: Option<Rc<dyn GestureArenaMember>>,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        if std::thread::panicking() {
            candidate.retain();
            return None;
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(candidate))).err()
    }

    fn dispatch_with_candidate(
        pending: PendingNotifications,
        pointer: PointerId,
        candidate: Option<Rc<dyn GestureArenaMember>>,
    ) {
        let mut first_panic = Self::retire_candidate(candidate);
        Self::dispatch_pending_into(pending, pointer, &mut first_panic);
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
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
        if self.owner_closed.get() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(member);
            failure.finish();
            return;
        }
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let follow_up = slot.data.borrow_mut().accept(&member);
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        Self::dispatch_with_candidate(pending, pointer, Some(member));
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

    fn release_slot(&self, slot: &Rc<ArenaSlot>) {
        let should_sweep = {
            let mut entry = slot.data.borrow_mut();
            entry.release();
            std::mem::take(&mut entry.has_pending_sweep)
        };
        if should_sweep {
            self.sweep_slot(slot);
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
            event = %crate::observability::GestureEvent::ArenaResolved,
        )
    )]
    pub fn resolve(&self, pointer: PointerId, winner: Option<&Rc<dyn GestureArenaMember>>) {
        let winner = winner.cloned();
        if self.owner_closed.get() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
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
                Self::dispatch_with_candidate(pending, pointer, winner);
                return;
            }
        }
        let pending = slot.data.borrow_mut().resolve(winner.as_ref());
        self.remove_exact_slot(pointer, &slot);
        Self::dispatch_with_candidate(pending, pointer, winner);
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
            event = %crate::observability::GestureEvent::ArenaSwept,
        )
    )]
    pub fn sweep(&self, pointer: PointerId) {
        if let Some(slot) = self.current_slot(pointer) {
            self.sweep_slot(&slot);
        }
    }

    fn sweep_slot(&self, slot: &Rc<ArenaSlot>) {
        let pending = {
            let mut entry = slot.data.borrow_mut();
            if entry.is_held || entry.has_blocked_fallback() {
                // The pointer is up: its held generation leaves the active
                // map, so the pointer's next Down opens a fresh arena instead
                // of being refused by this closed one. Exact entry handles and
                // `release(pointer)` still reach it among the retained slots.
                entry.has_pending_sweep = true;
                drop(entry);
                self.remove_current_slot(slot.pointer, slot);
                self.retained
                    .borrow_mut()
                    .insert(slot.generation, Rc::clone(slot));
                return;
            }
            entry.sweep()
        };
        self.remove_exact_slot(slot.pointer, slot);
        Self::dispatch_pending(pending, slot.pointer);
    }

    fn abandon_slot(&self, slot: &Rc<ArenaSlot>) {
        // Leave the maps first, so a reentrant rejection callback cannot reach
        // this generation again.
        if !self.remove_exact_slot(slot.pointer, slot) {
            return;
        }
        let pending = slot.data.borrow_mut().resolve(None);
        Self::dispatch_pending(pending, slot.pointer);
    }

    /// Tear down one interrupted pointer sequence without choosing a winner.
    ///
    /// Lifecycle loss and explicit cache invalidation can arrive without a
    /// matching pointer Cancel. Remove the slot before notifying recognizers,
    /// so re-entrancy or a panicking rejection callback cannot leave a stale
    /// competition behind.
    pub fn abandon(&self, pointer: PointerId) {
        let batch = self.detach(pointer);
        Self::abandon_detached(batch);
    }

    /// Remove every generation for `pointer` from maps visible to new input.
    /// The returned owned batch keeps members alive until the caller finishes
    /// its causal route/lifecycle transaction.
    pub(crate) fn detach(&self, pointer: PointerId) -> DetachedArenaBatch {
        let mut slots = SmallVec::new();
        let active = self.entries.borrow_mut().remove(&pointer);
        if let Some(slot) = active {
            slots.push(slot);
        }

        let mut retained_generations: SmallVec<[ArenaGeneration; 2]> = self
            .retained
            .borrow()
            .iter()
            .filter(|(_, slot)| slot.pointer == pointer)
            .map(|(generation, _)| *generation)
            .collect();
        retained_generations.sort_unstable_by_key(|generation| generation.0);
        for generation in retained_generations {
            let retained = self.retained.borrow_mut().remove(&generation);
            if let Some(slot) = retained {
                slots.push(slot);
            }
        }

        DetachedArenaBatch { pointer, slots }
    }

    /// Run pointer-up sweep semantics over an already detached batch.
    pub(crate) fn sweep_detached(&self, batch: DetachedArenaBatch) {
        let mut first_panic = None;
        for slot in batch.slots {
            let pending = {
                let mut entry = slot.data.borrow_mut();
                if entry.is_held || entry.has_blocked_fallback() {
                    entry.has_pending_sweep = true;
                    self.retained
                        .borrow_mut()
                        .insert(slot.generation, Rc::clone(&slot));
                    continue;
                }
                entry.sweep()
            };
            Self::dispatch_pending_into(pending, batch.pointer, &mut first_panic);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Reject every member of an already detached batch without a winner.
    pub(crate) fn abandon_detached(batch: DetachedArenaBatch) {
        let mut first_panic = None;
        for slot in batch.slots {
            let pending = slot.data.borrow_mut().resolve(None);
            Self::dispatch_pending_into(pending, batch.pointer, &mut first_panic);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    pub(crate) fn close_owner(&self, mode: CloseMode) {
        let mut failure = ClosePanic::for_close(mode, self.close_mode.clone());
        self.owner_closed.set(true);
        let mut pointers: Vec<_> = self.entries.borrow().keys().copied().collect();
        pointers.extend(self.retained.borrow().values().map(|slot| slot.pointer));
        pointers.sort_unstable();
        pointers.dedup();
        let batches: Vec<_> = pointers
            .into_iter()
            .map(|pointer| self.detach(pointer))
            .collect();
        self.deferred.borrow_mut().clear();
        for batch in batches {
            for slot in batch.slots {
                if failure.preserving() {
                    failure.retire(crate::retain::Owned(slot));
                    continue;
                }
                let pending = slot.data.borrow_mut().resolve(None);
                for (member, disposition) in pending {
                    let Some(member) = member.upgrade() else {
                        continue;
                    };
                    failure.run(|| match disposition {
                        GestureDisposition::Accepted => member.accept_gesture(batch.pointer),
                        GestureDisposition::Rejected => member.reject_gesture(batch.pointer),
                    });
                    failure.retire(member);
                }
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
        let mut pointers: Vec<PointerId> = self.entries.borrow().keys().copied().collect();
        pointers.extend(self.retained.borrow().values().map(|slot| slot.pointer));
        pointers.sort_unstable();
        pointers.dedup();
        let batches: Vec<_> = pointers
            .into_iter()
            .map(|pointer| self.detach(pointer))
            .collect();

        let mut first_panic = None;
        for batch in batches {
            for slot in batch.slots {
                let pending = slot.data.borrow_mut().resolve(None);
                Self::dispatch_pending_into(pending, batch.pointer, &mut first_panic);
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    fn preserve_first_panic(
        first: &mut Option<Box<dyn Any + Send>>,
        candidate: Option<Box<dyn Any + Send>>,
        _pointer: PointerId,
    ) {
        let Some(candidate) = candidate else {
            return;
        };
        if first.is_none() {
            *first = Some(candidate);
        } else {
            std::mem::forget(candidate);
        }
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
                if !members
                    .iter()
                    .any(|existing| Weak::ptr_eq(&existing.member, member))
                {
                    members.push(DeadlinePoll {
                        registration: None,
                        pointer: slot.pointer,
                        member: member.clone(),
                    });
                }
            }
        };
        for slot in self.entries.borrow().values() {
            collect_slot(slot);
        }
        for slot in self.retained.borrow().values() {
            collect_slot(slot);
        }
        for poll in self.deadlines.snapshot() {
            if !members
                .iter()
                .any(|existing| Weak::ptr_eq(&existing.member, &poll.member))
            {
                members.push(poll);
            }
        }
        members
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
    /// Complexity: O(P + M) where P is the number of open arenas and M the
    /// total active members — both bounded by the simultaneous-pointer cap.
    pub fn poll_deadlines(&self) {
        if self.owner_closed.get() {
            return;
        }
        let mut first_panic = None;
        let now = self.clock.now();
        for poll in self.deadline_members_snapshot() {
            if poll
                .registration
                .is_some_and(|id| !self.deadlines.contains(id))
            {
                continue;
            }
            let Some(member) = poll.member.upgrade() else {
                continue;
            };
            let candidate = catch_unwind(AssertUnwindSafe(|| {
                if member.deadline().is_some_and(|due| due <= now) {
                    member.poll_deadline(now);
                }
            }))
            .err();
            Self::preserve_first_panic(&mut first_panic, candidate, poll.pointer);
            if first_panic.is_some() || std::thread::panicking() {
                member.retain();
            } else {
                let candidate = catch_unwind(AssertUnwindSafe(|| drop(member))).err();
                Self::preserve_first_panic(&mut first_panic, candidate, poll.pointer);
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Whether any live member has an armed time-based deadline (see
    /// [`GestureArenaMember::deadline`]).
    ///
    /// The frame driver queries this once per frame, beside
    /// [`poll_deadlines`](Self::poll_deadlines), to keep producing frames
    /// while a deadline is pending. Members are snapshotted out of the
    /// per-entry borrows before querying — the same discipline `poll_deadlines`
    /// follows — and the predicate itself is a pure state read.
    pub fn has_pending_deadlines(&self) -> bool {
        self.next_deadline().is_some()
    }

    /// The earliest instant any live member's armed deadline will fire, if
    /// any — the wall-clock-wake counterpart of
    /// [`has_pending_deadlines`](Self::has_pending_deadlines). A caller
    /// computing a `ControlFlow::WaitUntil` target reads this instead of the
    /// boolean so a fully idle-but-armed presentation (nothing dirty, no
    /// running animation) still wakes at the right instant to resolve the
    /// deadline, rather than only lazily on the next unrelated event. Same
    /// snapshot/borrowing discipline as `has_pending_deadlines`.
    pub fn next_deadline(&self) -> Option<Instant> {
        if self.owner_closed.get() {
            return None;
        }
        let mut earliest = None;
        let mut first_panic = None;
        for poll in self.deadline_members_snapshot() {
            if poll
                .registration
                .is_some_and(|id| !self.deadlines.contains(id))
            {
                continue;
            }
            let Some(member) = poll.member.upgrade() else {
                continue;
            };
            let candidate = match catch_unwind(AssertUnwindSafe(|| member.deadline())) {
                Ok(Some(due)) => {
                    earliest = Some(earliest.map_or(due, |previous: Instant| previous.min(due)));
                    None
                }
                Ok(None) => None,
                Err(payload) => Some(payload),
            };
            Self::preserve_first_panic(&mut first_panic, candidate, poll.pointer);
            if first_panic.is_some() || std::thread::panicking() {
                member.retain();
            } else {
                let candidate = catch_unwind(AssertUnwindSafe(|| drop(member))).err();
                Self::preserve_first_panic(&mut first_panic, candidate, poll.pointer);
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
        earliest
    }

    /// Get the number of active arenas.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.borrow().len() + self.retained.borrow().len()
    }

    /// Check if arena is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty() && self.retained.borrow().is_empty()
    }

    /// Check if an arena exists for a pointer.
    #[inline]
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.entries.borrow().contains_key(&pointer)
            || self
                .retained
                .borrow()
                .values()
                .any(|slot| slot.pointer == pointer)
    }

    /// Whether `pointer` currently has a slot accepting this sequence's arena
    /// lifecycle (excluding held generations retained from earlier contacts).
    pub(crate) fn has_active(&self, pointer: PointerId) -> bool {
        self.entries.borrow().contains_key(&pointer)
    }

    fn inspection_slot(&self, pointer: PointerId) -> Option<Rc<ArenaSlot>> {
        self.current_slot(pointer).or_else(|| {
            self.retained
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
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.borrow().eager_winner.is_some())
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
            .entries
            .borrow()
            .values()
            .chain(self.retained.borrow().values())
            .cloned()
            .collect();
        for slot in slots {
            let follow_up = {
                let mut entry = slot.data.borrow_mut();
                entry.prune_departed();
                if entry.has_pending_sweep
                    && !entry.is_held
                    && !entry.is_resolved
                    && !entry.has_blocked_fallback()
                {
                    ArenaFollowUp::DeferDefault
                } else {
                    entry.follow_up()
                }
            };
            match follow_up {
                ArenaFollowUp::DeferDefault => self.queue_default_resolution(slot.pointer, &slot),
                ArenaFollowUp::RemoveEmpty => {
                    self.remove_exact_slot(slot.pointer, &slot);
                }
                _ => {}
            }
        }
        let queued = std::mem::take(&mut *self.deferred.borrow_mut());
        let mut resolved = 0;
        let mut first_panic = None;

        for token in queued {
            let Some(slot) = token.slot.upgrade() else {
                continue;
            };
            if slot.generation != token.generation || !self.is_live_slot(token.pointer, &slot) {
                continue;
            }

            let pending = {
                let mut entry = slot.data.borrow_mut();
                entry.prune_departed();
                if entry.has_pending_sweep && !entry.is_held && !entry.has_blocked_fallback() {
                    entry.has_pending_sweep = false;
                    entry.sweep()
                } else {
                    let winner = match entry.follow_up() {
                        ArenaFollowUp::DeferDefault => entry.members[0].upgrade(),
                        ArenaFollowUp::ResolveInFavorOf(winner) => winner.upgrade(),
                        ArenaFollowUp::None | ArenaFollowUp::RemoveEmpty => continue,
                    };
                    entry.resolve(winner.as_ref())
                }
            };
            self.remove_exact_slot(token.pointer, &slot);
            resolved += 1;
            Self::dispatch_pending_into(pending, token.pointer, &mut first_panic);
        }

        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
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
            .field("active_arenas", &self.entries.borrow().len())
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
        fn accept_gesture(&self, _pointer: PointerId) {
            *self.accepted.lock() = true;
        }

        fn reject_gesture(&self, _pointer: PointerId) {
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
        fn accept_gesture(&self, _pointer: PointerId) {}

        fn reject_gesture(&self, pointer: PointerId) {
            *self.rejected.lock() = true;
            // Re-enter the arena from inside the reject callback.
            self.arena.resolve(pointer, None);
        }
    }

    struct OrderedMember {
        name: &'static str,
        calls: Rc<Mutex<Vec<&'static str>>>,
        panic_on_accept: bool,
    }

    impl GestureArenaMember for OrderedMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            self.calls.lock().push(self.name);
            assert!(!self.panic_on_accept, "winner callback panic");
        }

        fn reject_gesture(&self, _pointer: PointerId) {
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
