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
//! ```rust,ignore
//! let entry = arena.add(pointer, my_recognizer.clone());
//! // Later, when the recognizer decides:
//! entry.resolve(GestureDisposition::Accepted);
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
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/GestureArenaManager-class.html>

// Submodules — these are part of the crate's public surface (they're
// referenced from recognizer code) so they're `pub` rather than `pub(crate)`.
pub mod signal_resolver;
pub mod team;

pub use signal_resolver::{PointerSignalResolver, SignalPriority};
pub use team::{GestureArenaTeam, TeamEntry};

use std::{
    any::Any,
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;
use tracing::instrument;

use crate::ids::PointerId;
use flui_foundation::{MonotonicClock, SystemClock};

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
/// To create a custom gesture recognizer, implement [`CustomGestureRecognizer`]
/// instead of this trait directly. The blanket implementation will
/// automatically provide `GestureArenaMember` for your type.
///
/// ```rust,ignore
/// use flui_interaction::sealed::CustomGestureRecognizer;
///
/// struct MyRecognizer { /* ... */ }
///
/// impl CustomGestureRecognizer for MyRecognizer {
///     fn on_arena_accept(&self, pointer: PointerId) {
///         // Handle winning the arena
///     }
///     fn on_arena_reject(&self, pointer: PointerId) {
///         // Handle losing the arena
///     }
/// }
///
/// // MyRecognizer now implements GestureArenaMember automatically!
/// let arena = GestureArena::new();
/// let entry = arena.add(pointer, Arc::new(MyRecognizer { /* ... */ }));
/// // Later: entry.resolve(GestureDisposition::Accepted);
/// ```
///
/// [`CustomGestureRecognizer`]: crate::sealed::CustomGestureRecognizer
pub trait GestureArenaMember: crate::sealed::arena_member::Sealed {
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
    fn poll_deadline(&self) {}
}

// ============================================================================
// Blanket implementation for CustomGestureRecognizer
// ============================================================================

/// Blanket implementation: any `CustomGestureRecognizer` automatically
/// implements `GestureArenaMember`.
impl<T: crate::sealed::CustomGestureRecognizer> GestureArenaMember for T {
    #[inline]
    fn accept_gesture(&self, pointer: PointerId) {
        self.on_arena_accept(pointer);
    }

    #[inline]
    fn reject_gesture(&self, pointer: PointerId) {
        self.on_arena_reject(pointer);
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
/// use std::sync::Arc;
///
/// use flui_interaction::arena::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::sealed::CustomGestureRecognizer;
///
/// struct R;
/// impl CustomGestureRecognizer for R {
///     fn on_arena_accept(&self, _: PointerId) {}
///     fn on_arena_reject(&self, _: PointerId) {}
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::PRIMARY;
/// let recognizer: Arc<R> = Arc::new(R);
///
/// let entry = arena.add(pointer, recognizer);
///
/// // Later, when the recogniser decides:
/// entry.resolve(GestureDisposition::Accepted);
/// ```
///
/// The handle is owner-affine because gesture callbacks are owner-local.
/// Multiple calls to `resolve` are safe; stale or already-resolved entries are
/// no-ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ArenaGeneration(u64);

struct ArenaSlot {
    pointer: PointerId,
    generation: ArenaGeneration,
    data: Mutex<ArenaEntryData>,
}

impl ArenaSlot {
    fn new(pointer: PointerId, generation: ArenaGeneration) -> Self {
        Self {
            pointer,
            generation,
            data: Mutex::new(ArenaEntryData::new()),
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
/// Flutter's primary-pointer deadlines are scheduled independently from the
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
    member: Arc<dyn GestureArenaMember>,
}

struct DeadlineRegistry {
    next_id: AtomicU64,
    watchers: Mutex<Vec<DeadlineWatcher>>,
}

impl DeadlineRegistry {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            watchers: Mutex::new(Vec::new()),
        }
    }

    fn register(
        self: &Arc<Self>,
        pointer: PointerId,
        member: &Arc<dyn GestureArenaMember>,
    ) -> GestureDeadlineRegistration {
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .unwrap_or_else(|_| panic!("BUG: gesture deadline registration ID exhausted"));
        self.watchers.lock().push(DeadlineWatcher {
            id,
            pointer,
            member: Arc::downgrade(member),
        });
        GestureDeadlineRegistration {
            registry: Arc::downgrade(self),
            id,
        }
    }

    fn unregister(&self, id: u64) {
        self.watchers.lock().retain(|watcher| watcher.id != id);
    }

    fn contains(&self, id: u64) -> bool {
        self.watchers.lock().iter().any(|watcher| watcher.id == id)
    }

    fn snapshot(&self) -> SmallVec<[DeadlinePoll; 8]> {
        let mut watchers = self.watchers.lock();
        let mut live = SmallVec::new();
        watchers.retain(|watcher| {
            if let Some(member) = watcher.member.upgrade() {
                live.push(DeadlinePoll {
                    registration: Some(watcher.id),
                    pointer: watcher.pointer,
                    member,
                });
                true
            } else {
                false
            }
        });
        live
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.watchers.lock().len()
    }
}

impl GestureArenaEntry {
    /// Create a new arena entry handle.
    fn new(
        arena: GestureArena,
        pointer: PointerId,
        slot: &Arc<ArenaSlot>,
        member: &Arc<dyn GestureArenaMember>,
    ) -> Self {
        Self {
            arena,
            pointer,
            generation: slot.generation,
            slot: Arc::downgrade(slot),
            member: Arc::downgrade(member),
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

    /// Hold this exact arena generation against a pointer-up sweep.
    pub fn hold(&self) {
        if let Some(slot) = self.slot.upgrade() {
            GestureArena::hold_slot(&slot);
        }
    }

    /// Release a hold on this exact arena generation.
    pub fn release(&self) {
        if let Some(slot) = self.slot.upgrade() {
            self.arena.release_slot(&slot);
        }
    }

    /// Sweep this exact arena generation.
    pub fn sweep(&self) {
        if let Some(slot) = self.slot.upgrade() {
            self.arena.sweep_slot(&slot);
        }
    }

    /// Get the pointer ID for this entry.
    #[inline]
    pub fn pointer(&self) -> PointerId {
        self.pointer
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> Option<Arc<dyn GestureArenaMember>> {
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
    members: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,
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
    eager_winner: Option<Arc<dyn GestureArenaMember>>,
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

/// Member callbacks deferred out of the locked region.
///
/// Arena resolution must never invoke `accept_gesture`/`reject_gesture`
/// while the per-entry `Mutex` is held: a member's handler may call back
/// into the arena (e.g. `reject_gesture` -> `state.reject()` ->
/// `arena.resolve`), which re-locks the same entry and deadlocks under the
/// non-reentrant `parking_lot::Mutex`. Internal `ArenaEntryData` mutators
/// therefore return the pending notifications; the public `GestureArena`
/// methods dispatch them after releasing the lock.
type PendingNotifications = SmallVec<[(Arc<dyn GestureArenaMember>, GestureDisposition); 4]>;

enum ArenaFollowUp {
    None,
    RemoveEmpty,
    DeferDefault,
    ResolveInFavorOf(Arc<dyn GestureArenaMember>),
}

impl ArenaEntryData {
    fn new() -> Self {
        Self {
            members: SmallVec::new(),
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
        self.follow_up()
    }

    /// Accept gesture for a member.
    /// If arena is open, store as eager winner. If closed, resolve immediately.
    #[must_use]
    fn accept(&mut self, member: Arc<dyn GestureArenaMember>) -> ArenaFollowUp {
        if self.is_resolved {
            return ArenaFollowUp::None;
        }

        if self.is_open {
            // Store as eager winner - will win when arena closes
            if self.eager_winner.is_none() {
                self.eager_winner = Some(member);
            }
            // If already have eager winner, ignore subsequent accepts
            ArenaFollowUp::None
        } else {
            ArenaFollowUp::ResolveInFavorOf(member)
        }
    }

    /// Reject gesture for a member.
    #[must_use]
    fn reject(
        &mut self,
        member: Arc<dyn GestureArenaMember>,
    ) -> (PendingNotifications, ArenaFollowUp) {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return (pending, ArenaFollowUp::None);
        }

        let Some(index) = self
            .members
            .iter()
            .position(|entry| Arc::ptr_eq(entry, &member))
        else {
            return (pending, ArenaFollowUp::None);
        };
        let rejected = self.members.remove(index);

        // Remove from eager winner if it was this member
        if let Some(ref eager) = self.eager_winner
            && Arc::ptr_eq(eager, &member)
        {
            self.eager_winner = None;
        }

        // Defer the member's rejection callback (dispatched after the entry
        // lock is released to avoid arena re-entrancy deadlock).
        pending.push((rejected, GestureDisposition::Rejected));

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

        if self.members.len() == 1 {
            ArenaFollowUp::DeferDefault
        } else if self.members.is_empty() {
            ArenaFollowUp::RemoveEmpty
        } else if let Some(eager) = self.eager_winner.clone() {
            ArenaFollowUp::ResolveInFavorOf(eager)
        } else {
            ArenaFollowUp::None
        }
    }

    /// Add a member to this arena.
    fn add(&mut self, member: Arc<dyn GestureArenaMember>) {
        if self.is_open && !self.is_resolved {
            self.members.push(member);
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
    fn resolve(&mut self, winner: Option<Arc<dyn GestureArenaMember>>) -> PendingNotifications {
        if self.is_resolved {
            return PendingNotifications::new();
        }

        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        let mut losers = PendingNotifications::new();
        let mut accepted = PendingNotifications::new();

        // Flutter's explicit/eager resolution rejects every loser before
        // accepting the winner. This ordering is observable when callbacks
        // re-enter or panic.
        for member in members {
            let is_winner = winner
                .as_ref()
                .is_some_and(|winner| Arc::ptr_eq(&member, winner));
            if is_winner {
                accepted.push((member, GestureDisposition::Accepted));
            } else {
                losers.push((member, GestureDisposition::Rejected));
            }
        }

        losers.extend(accepted);
        losers
    }

    /// Resolve the arena with multiple winners (team resolution).
    #[must_use]
    fn resolve_team(&mut self, winners: &[Arc<dyn GestureArenaMember>]) -> PendingNotifications {
        if self.is_resolved {
            return PendingNotifications::new();
        }

        self.is_resolved = true;
        let members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        let mut losers = PendingNotifications::new();
        let mut accepted = PendingNotifications::new();

        // Reject all losers before accepting any team member.
        for member in members {
            let is_winner = winners.iter().any(|winner| Arc::ptr_eq(&member, winner));
            if is_winner {
                accepted.push((member, GestureDisposition::Accepted));
            } else {
                losers.push((member, GestureDisposition::Rejected));
            }
        }

        losers.extend(accepted);
        losers
    }

    /// Flutter sweep ordering is intentionally different from an explicit
    /// resolution: the front member is accepted first, then later members are
    /// rejected in registration order.
    #[must_use]
    fn sweep(&mut self) -> PendingNotifications {
        let mut pending = PendingNotifications::new();
        if self.is_resolved {
            return pending;
        }
        self.is_resolved = true;
        let mut members = std::mem::take(&mut self.members);
        self.eager_winner = None;
        if members.is_empty() {
            return pending;
        }
        let winner = members.remove(0);
        pending.push((winner, GestureDisposition::Accepted));
        pending.extend(
            members
                .into_iter()
                .map(|member| (member, GestureDisposition::Rejected)),
        );
        pending
    }
}

// ============================================================================
// SweepModel
// ============================================================================

/// Who owns the close/sweep lifecycle of an arena.
///
/// This is the FLUI-native encoding of the Flutter contract that the
/// *binding* — not a recognizer — drives `close(pointer)` on pointer-down and
/// `sweep(pointer)` on pointer-up (`gestures/binding.dart` `handleEvent`).
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
/// The model is immutable per arena, like the clock; it rides on the
/// `Arc`-backed handle, so every clone observes it.
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
/// Mirrors Flutter's `GestureBinding.handleEvent` (`gestures/binding.dart`):
/// on `PointerDown` the arena is closed; on `PointerUp` it is swept. Every
/// other event — including `PointerCancel` — is a no-op for the arena
/// lifecycle (recognizers self-reject on cancel; sweeping on cancel would
/// force the first member to win an interrupted gesture). Callers run
/// their own route (hit-test dispatch) step *first*, then call this kernel
/// — the route-before-sweep order is load-bearing (it lets a double-tap's
/// first-up `hold` run before the sweep, so the sweep observes the hold
/// and defers).
/// Shared by the headless binding and the production `GestureBinding`.
pub fn run_pointer_lifecycle(arena: &GestureArena, event: &crate::events::PointerEvent) {
    use crate::events::PointerEvent;
    let pointer = crate::events::extract_pointer_id(event);
    match event {
        PointerEvent::Down(_) => arena.close(pointer),
        PointerEvent::Up(_) => arena.sweep(pointer),
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
/// use std::sync::Arc;
///
/// use flui_interaction::arena::{GestureArena, GestureDisposition};
/// use flui_interaction::ids::PointerId;
/// use flui_interaction::sealed::CustomGestureRecognizer;
///
/// // A minimal recogniser that counts accepts/rejects. Use a real
/// // `TapGestureRecognizer` / `DragGestureRecognizer` in production —
/// // this is the minimum surface to participate in the arena.
/// #[derive(Debug)]
/// struct Counter(AtomicUsize, AtomicUsize);
/// impl CustomGestureRecognizer for Counter {
///     fn on_arena_accept(&self, _: PointerId) { self.0.fetch_add(1, Ordering::Relaxed); }
///     fn on_arena_reject(&self, _: PointerId) { self.1.fetch_add(1, Ordering::Relaxed); }
/// }
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::PRIMARY;
/// let tap = Arc::new(Counter(AtomicUsize::new(0), AtomicUsize::new(0)));
/// let drag = Arc::new(Counter(AtomicUsize::new(0), AtomicUsize::new(0)));
///
/// // Add recognisers to the arena — returns an entry handle.
/// let tap_entry = arena.add(pointer, tap.clone());
/// let drag_entry = arena.add(pointer, drag.clone());
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
    /// Map from pointer ID to the active exact-generation arena slot.
    entries: Arc<DashMap<PointerId, Arc<ArenaSlot>>>,
    /// Held arenas detached from the active pointer map during an Up
    /// transaction. Exact entry tokens can still release these generations,
    /// while a reused pointer ID opens a fresh active slot.
    retained: Arc<DashMap<ArenaGeneration, Arc<ArenaSlot>>>,
    /// Typed replacement for Flutter's single-member microtask closure queue.
    deferred: Arc<Mutex<VecDeque<DeferredResolution>>>,
    /// Owner-frame deadline polling, independent from arena-slot lifetime.
    deadlines: Arc<DeadlineRegistry>,
    next_generation: Arc<AtomicU64>,
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
    slots: SmallVec<[Arc<ArenaSlot>; 2]>,
}

impl GestureArena {
    fn allocate_slot(&self, pointer: PointerId) -> Arc<ArenaSlot> {
        let generation = ArenaGeneration(
            self.next_generation
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
                    generation.checked_add(1)
                })
                .unwrap_or_else(|_| panic!("BUG: gesture arena generation exhausted")),
        );
        Arc::new(ArenaSlot::new(pointer, generation))
    }

    fn current_slot(&self, pointer: PointerId) -> Option<Arc<ArenaSlot>> {
        self.entries
            .get(&pointer)
            .map(|entry| Arc::clone(entry.value()))
    }

    fn remove_current_slot(&self, pointer: PointerId, slot: &Arc<ArenaSlot>) -> bool {
        use dashmap::mapref::entry::Entry;

        match self.entries.entry(pointer) {
            Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), slot) => {
                entry.remove();
                true
            }
            _ => false,
        }
    }

    fn remove_retained_slot(&self, slot: &Arc<ArenaSlot>) -> bool {
        use dashmap::mapref::entry::Entry;

        match self.retained.entry(slot.generation) {
            Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), slot) => {
                entry.remove();
                true
            }
            _ => false,
        }
    }

    fn remove_exact_slot(&self, pointer: PointerId, slot: &Arc<ArenaSlot>) -> bool {
        self.remove_current_slot(pointer, slot) || self.remove_retained_slot(slot)
    }

    fn is_live_slot(&self, pointer: PointerId, slot: &Arc<ArenaSlot>) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry| Arc::ptr_eq(entry.value(), slot))
            || self
                .retained
                .get(&slot.generation)
                .is_some_and(|entry| Arc::ptr_eq(entry.value(), slot))
    }

    fn queue_default_resolution(&self, pointer: PointerId, slot: &Arc<ArenaSlot>) {
        if self.is_live_slot(pointer, slot) {
            self.deferred.lock().push_back(DeferredResolution {
                pointer,
                generation: slot.generation,
                slot: Arc::downgrade(slot),
            });
        }
    }

    fn collect_follow_up(
        &self,
        pointer: PointerId,
        slot: &Arc<ArenaSlot>,
        follow_up: ArenaFollowUp,
    ) -> PendingNotifications {
        match follow_up {
            ArenaFollowUp::None => PendingNotifications::new(),
            ArenaFollowUp::RemoveEmpty => {
                slot.data.lock().is_resolved = true;
                self.remove_exact_slot(pointer, slot);
                PendingNotifications::new()
            }
            ArenaFollowUp::DeferDefault => {
                self.queue_default_resolution(pointer, slot);
                PendingNotifications::new()
            }
            ArenaFollowUp::ResolveInFavorOf(winner) => {
                let pending = slot.data.lock().resolve(Some(winner));
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
            entries: Arc::new(DashMap::new()),
            retained: Arc::new(DashMap::new()),
            deferred: Arc::new(Mutex::new(VecDeque::new())),
            deadlines: Arc::new(DeadlineRegistry::new()),
            next_generation: Arc::new(AtomicU64::new(1)),
            clock,
            sweep_model: SweepModel::SelfDriven,
        }
    }

    /// Create a binding-owned gesture arena driven by the given clock.
    ///
    /// The returned arena answers [`SweepModel::BindingDriven`], so recognizers
    /// added to it never self-sweep in `stop_tracking` — the binding runs the
    /// close/sweep lifecycle via [`run_pointer_lifecycle`] after routing each
    /// pointer event. This is the arena a [`HeadlessBinding`](https://docs.rs/flui-binding)
    /// or production `GestureBinding` hands down to a subtree.
    #[inline]
    pub fn binding_driven(clock: Arc<dyn MonotonicClock>) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            retained: Arc::new(DashMap::new()),
            deferred: Arc::new(Mutex::new(VecDeque::new())),
            deadlines: Arc::new(DeadlineRegistry::new()),
            next_generation: Arc::new(AtomicU64::new(1)),
            clock,
            sweep_model: SweepModel::BindingDriven,
        }
    }

    /// Create a gesture arena with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(DashMap::with_capacity(capacity)),
            retained: Arc::new(DashMap::with_capacity(capacity)),
            deferred: Arc::new(Mutex::new(VecDeque::new())),
            deadlines: Arc::new(DeadlineRegistry::new()),
            next_generation: Arc::new(AtomicU64::new(1)),
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
        member: &Arc<dyn GestureArenaMember>,
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
    /// use std::sync::Arc;
    ///
    /// use flui_interaction::arena::{GestureArena, GestureDisposition};
    /// use flui_interaction::ids::PointerId;
    /// use flui_interaction::sealed::CustomGestureRecognizer;
    ///
    /// struct R;
    /// impl CustomGestureRecognizer for R {
    ///     fn on_arena_accept(&self, _: PointerId) {}
    ///     fn on_arena_reject(&self, _: PointerId) {}
    /// }
    ///
    /// let arena = GestureArena::new();
    /// let pointer = PointerId::PRIMARY;
    /// let recognizer: Arc<R> = Arc::new(R);
    /// let entry = arena.add(pointer, recognizer);
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
    pub fn add(
        &self,
        pointer: PointerId,
        member: Arc<dyn GestureArenaMember>,
    ) -> GestureArenaEntry {
        use dashmap::mapref::entry::Entry;

        // Keep the occupied shard guard until membership is recorded. This
        // makes slot selection and membership insertion one transaction even
        // if a future embedding widens the current owner-local boundary.
        let slot = match self.entries.entry(pointer) {
            Entry::Occupied(entry) => {
                let slot = Arc::clone(entry.get());
                slot.data.lock().add(member.clone());
                slot
            }
            Entry::Vacant(entry) => {
                let slot = self.allocate_slot(pointer);
                slot.data.lock().add(member.clone());
                entry.insert(Arc::clone(&slot));
                slot
            }
        };

        GestureArenaEntry::new(self.clone(), pointer, &slot, &member)
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
        let follow_up = slot.data.lock().close();
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        Self::dispatch_pending(pending, pointer);
    }

    /// Dispatch deferred member notifications after the per-entry `Mutex` has
    /// been released. Keeping member callbacks out of the locked region is
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
        for (member, disposition) in pending {
            let candidate = catch_unwind(AssertUnwindSafe(|| match disposition {
                GestureDisposition::Accepted => member.accept_gesture(pointer),
                GestureDisposition::Rejected => member.reject_gesture(pointer),
            }))
            .err();
            Self::preserve_first_panic(&mut first_panic, candidate, pointer);
            let drop_candidate = catch_unwind(AssertUnwindSafe(|| drop(member))).err();
            Self::preserve_first_panic(&mut first_panic, drop_candidate, pointer);
        }
        first_panic
    }

    /// Internal method: resolve an entry with given disposition.
    ///
    /// Called by [`GestureArenaEntry::resolve`].
    fn resolve_entry(
        &self,
        pointer: PointerId,
        slot: &Arc<ArenaSlot>,
        member: Arc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
    ) {
        let (mut pending, follow_up) = {
            let mut entry = slot.data.lock();
            match disposition {
                GestureDisposition::Accepted => (PendingNotifications::new(), entry.accept(member)),
                GestureDisposition::Rejected => entry.reject(member),
            }
        };
        pending.extend(self.collect_follow_up(pointer, slot, follow_up));
        Self::dispatch_pending(pending, pointer);
    }

    /// Accept gesture for a member - the member wants to handle this gesture.
    ///
    /// If arena is open, stores as eager winner (wins when arena closes).
    /// If arena is closed, resolves immediately in favor of this member.
    ///
    /// # Note
    ///
    /// Prefer using [`GestureArenaEntry::resolve`] instead of this method.
    pub fn accept(&self, pointer: PointerId, member: Arc<dyn GestureArenaMember>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let follow_up = slot.data.lock().accept(member);
        let pending = self.collect_follow_up(pointer, &slot, follow_up);
        Self::dispatch_pending(pending, pointer);
    }

    /// Reject gesture for a member - the member doesn't want this gesture.
    ///
    /// Removes the member from the arena and notifies them.
    /// If only one member remains and arena is closed, they win.
    ///
    /// # Note
    ///
    /// Prefer using [`GestureArenaEntry::resolve`] instead of this method.
    pub fn reject(&self, pointer: PointerId, member: &Arc<dyn GestureArenaMember>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        self.resolve_entry(pointer, &slot, member.clone(), GestureDisposition::Rejected);
    }

    /// Hold the arena open for a pointer (delay resolution).
    ///
    /// Used when a recognizer needs more time to decide.
    pub fn hold(&self, pointer: PointerId) {
        if let Some(slot) = self.current_slot(pointer) {
            slot.data.lock().hold();
        }
    }

    fn hold_slot(slot: &Arc<ArenaSlot>) {
        let mut entry = slot.data.lock();
        if !entry.is_resolved {
            entry.hold();
        }
    }

    /// Release the hold on an arena.
    ///
    /// If a sweep was attempted while held, the deferred sweep runs now.
    /// Releasing never closes membership; `close` always does that during
    /// Down dispatch, independent of the hold state.
    pub fn release(&self, pointer: PointerId) {
        let retained = self
            .retained
            .iter()
            .filter(|entry| entry.value().pointer == pointer)
            .min_by_key(|entry| entry.key().0)
            .map(|entry| Arc::clone(entry.value()));
        if let Some(slot) = retained.or_else(|| self.current_slot(pointer)) {
            self.release_slot(&slot);
        }
    }

    fn release_slot(&self, slot: &Arc<ArenaSlot>) {
        let should_sweep = {
            let mut entry = slot.data.lock();
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
    pub fn resolve(&self, pointer: PointerId, winner: Option<Arc<dyn GestureArenaMember>>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let pending = slot.data.lock().resolve(winner);
        self.remove_exact_slot(pointer, &slot);
        Self::dispatch_pending(pending, pointer);
    }

    /// Withdraw a single member from the arena, leaving the others to keep
    /// competing.
    ///
    /// Unlike [`resolve`](Self::resolve) with no winner — which resolves the
    /// whole entry and rejects *every* member — this removes only `member`.
    /// When exactly one member remains in a closed arena, that member wins
    /// (Flutter parity: `GestureArenaEntry.resolve(rejected)` withdraws the
    /// caller without rejecting its competitors).
    pub fn reject_member(&self, pointer: PointerId, member: &Arc<dyn GestureArenaMember>) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        self.resolve_entry(pointer, &slot, member.clone(), GestureDisposition::Rejected);
    }

    /// Resolve the arena with multiple winners.
    ///
    /// All specified winners receive `accept_gesture()`.
    /// This is useful when multiple gestures should be recognized
    /// simultaneously.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Both tap and double-tap can be recognized
    /// arena.resolve_team(pointer, &[tap_recognizer, double_tap_recognizer]);
    /// ```
    pub fn resolve_team(&self, pointer: PointerId, winners: &[Arc<dyn GestureArenaMember>]) {
        let Some(slot) = self.current_slot(pointer) else {
            return;
        };
        let pending = slot.data.lock().resolve_team(winners);
        self.remove_exact_slot(pointer, &slot);
        Self::dispatch_pending(pending, pointer);
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

    fn sweep_slot(&self, slot: &Arc<ArenaSlot>) {
        let pending = {
            let mut entry = slot.data.lock();
            if entry.is_held {
                entry.has_pending_sweep = true;
                if !self
                    .entries
                    .get(&slot.pointer)
                    .is_some_and(|current| Arc::ptr_eq(current.value(), slot))
                {
                    self.retained.insert(slot.generation, Arc::clone(slot));
                }
                return;
            }
            entry.sweep()
        };
        self.remove_exact_slot(slot.pointer, slot);
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
        if let Some((_, slot)) = self.entries.remove(&pointer) {
            slots.push(slot);
        }

        let mut retained_generations: SmallVec<[ArenaGeneration; 2]> = self
            .retained
            .iter()
            .filter(|entry| entry.value().pointer == pointer)
            .map(|entry| *entry.key())
            .collect();
        retained_generations.sort_unstable_by_key(|generation| generation.0);
        for generation in retained_generations {
            if let Some((_, slot)) = self.retained.remove(&generation) {
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
                let mut entry = slot.data.lock();
                if entry.is_held {
                    entry.has_pending_sweep = true;
                    self.retained.insert(slot.generation, Arc::clone(&slot));
                    continue;
                }
                entry.sweep()
            };
            let candidate = Self::dispatch_pending_capturing(pending, batch.pointer);
            Self::preserve_first_panic(&mut first_panic, candidate, batch.pointer);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Reject every member of an already detached batch without a winner.
    pub(crate) fn abandon_detached(batch: DetachedArenaBatch) {
        let mut first_panic = None;
        for slot in batch.slots {
            let pending = slot.data.lock().resolve(None);
            let candidate = Self::dispatch_pending_capturing(pending, batch.pointer);
            Self::preserve_first_panic(&mut first_panic, candidate, batch.pointer);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Tear down every interrupted pointer sequence without choosing winners.
    ///
    /// All slots are detached first. Recognizer notifications keep their
    /// normal stop-on-unwind behavior while the binding still ends with no
    /// live arena state.
    pub(crate) fn abandon_all(&self) {
        let mut pointers: Vec<PointerId> = self.entries.iter().map(|entry| *entry.key()).collect();
        pointers.extend(self.retained.iter().map(|entry| entry.value().pointer));
        pointers.sort_unstable();
        pointers.dedup();
        let batches: Vec<_> = pointers
            .into_iter()
            .map(|pointer| self.detach(pointer))
            .collect();

        let mut first_panic = None;
        for batch in batches {
            for slot in batch.slots {
                let pending = slot.data.lock().resolve(None);
                let candidate = Self::dispatch_pending_capturing(pending, batch.pointer);
                Self::preserve_first_panic(&mut first_panic, candidate, batch.pointer);
            }
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    fn preserve_first_panic(
        first: &mut Option<Box<dyn Any + Send>>,
        candidate: Option<Box<dyn Any + Send>>,
        pointer: PointerId,
    ) {
        let Some(candidate) = candidate else {
            return;
        };
        if first.is_none() {
            *first = Some(candidate);
        } else {
            tracing::error!(?pointer, "arena phase panicked after an earlier failure");
            std::mem::forget(candidate);
        }
    }

    /// Poll every active member's time-based deadline (e.g. long-press hold).
    ///
    /// Call once per frame from the UI thread. Members are snapshotted out of
    /// the per-entry locks *before* polling, because a deadline hook may fire
    /// user callbacks and re-enter the arena to resolve — invoking it under the
    /// entry lock would re-introduce the arena re-entrancy deadlock. A member
    /// Explicit deadline registrations remain visible after arena resolution,
    /// matching Flutter timers whose lifetime is independent from the arena.
    /// Exact recognizer identities are de-duplicated, so a registered member
    /// that is also still competing is polled only once.
    ///
    /// Complexity: O(P + M) where P is the number of open arenas and M the
    /// total active members — both bounded by the simultaneous-pointer cap.
    pub fn poll_deadlines(&self) {
        let mut members: SmallVec<[DeadlinePoll; 8]> = SmallVec::new();
        for entry in self.entries.iter() {
            let pointer = entry.value().pointer;
            for member in &entry.value().data.lock().members {
                if !members
                    .iter()
                    .any(|existing| Arc::ptr_eq(&existing.member, member))
                {
                    members.push(DeadlinePoll {
                        registration: None,
                        pointer,
                        member: Arc::clone(member),
                    });
                }
            }
        }
        for entry in self.retained.iter() {
            let pointer = entry.value().pointer;
            for member in &entry.value().data.lock().members {
                if !members
                    .iter()
                    .any(|existing| Arc::ptr_eq(&existing.member, member))
                {
                    members.push(DeadlinePoll {
                        registration: None,
                        pointer,
                        member: Arc::clone(member),
                    });
                }
            }
        }
        for poll in self.deadlines.snapshot() {
            if !members
                .iter()
                .any(|existing| Arc::ptr_eq(&existing.member, &poll.member))
            {
                members.push(poll);
            }
        }

        let mut first_panic = None;
        for poll in members {
            if poll
                .registration
                .is_some_and(|id| !self.deadlines.contains(id))
            {
                continue;
            }
            let candidate = catch_unwind(AssertUnwindSafe(|| poll.member.poll_deadline())).err();
            Self::preserve_first_panic(&mut first_panic, candidate, poll.pointer);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    #[cfg(test)]
    pub(crate) fn deadline_member_count(&self) -> usize {
        self.deadlines.len()
    }

    /// Get the number of active arenas.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len() + self.retained.len()
    }

    /// Check if arena is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.retained.is_empty()
    }

    /// Check if an arena exists for a pointer.
    #[inline]
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.entries.contains_key(&pointer)
            || self
                .retained
                .iter()
                .any(|entry| entry.value().pointer == pointer)
    }

    /// Whether `pointer` currently has a slot accepting this sequence's arena
    /// lifecycle (excluding held generations retained from earlier contacts).
    pub(crate) fn has_active(&self, pointer: PointerId) -> bool {
        self.entries.contains_key(&pointer)
    }

    fn inspection_slot(&self, pointer: PointerId) -> Option<Arc<ArenaSlot>> {
        self.current_slot(pointer).or_else(|| {
            self.retained
                .iter()
                .filter(|entry| entry.value().pointer == pointer)
                .min_by_key(|entry| entry.key().0)
                .map(|entry| Arc::clone(entry.value()))
        })
    }

    /// Check if an arena is held.
    pub fn is_held(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.lock().is_held)
    }

    /// Check if an arena is open (accepting new members).
    pub fn is_open(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.lock().is_open)
    }

    /// Check if an arena has an eager winner.
    pub fn has_eager_winner(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.lock().eager_winner.is_some())
    }

    /// Check if sweep is pending for an arena.
    pub fn has_pending_sweep(&self, pointer: PointerId) -> bool {
        self.inspection_slot(pointer)
            .is_some_and(|slot| slot.data.lock().has_pending_sweep)
    }

    /// Get the number of members in an arena.
    pub fn member_count(&self, pointer: PointerId) -> usize {
        self.inspection_slot(pointer)
            .map_or(0, |slot| slot.data.lock().members.len())
    }

    /// Drain single-member default resolutions queued by `close`/`reject`.
    ///
    /// This is a typed owner-boundary queue, not an arbitrary closure
    /// executor. Each token carries the exact arena generation; rejection,
    /// explicit resolution, teardown, or pointer-ID reuse makes it stale.
    pub fn drain_deferred_resolutions(&self) -> usize {
        let queued = std::mem::take(&mut *self.deferred.lock());
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
                let mut entry = slot.data.lock();
                if entry.is_open || entry.is_resolved || entry.members.len() != 1 {
                    continue;
                }
                let winner = entry.members[0].clone();
                entry.resolve(Some(winner))
            };
            self.remove_exact_slot(token.pointer, &slot);
            resolved += 1;
            let candidate = Self::dispatch_pending_capturing(pending, token.pointer);
            Self::preserve_first_panic(&mut first_panic, candidate, token.pointer);
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
            .field("active_arenas", &self.entries.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_not_impl_any!(GestureArena: Send, Sync);
    static_assertions::assert_not_impl_any!(GestureArenaEntry: Send, Sync);

    // Mock arena member for testing - implement sealed trait
    struct MockMember {
        accepted: Arc<Mutex<bool>>,
        rejected: Arc<Mutex<bool>>,
    }

    // Implement the sealed trait
    impl crate::sealed::arena_member::Sealed for MockMember {}

    impl MockMember {
        fn new() -> Self {
            Self {
                accepted: Arc::new(Mutex::new(false)),
                rejected: Arc::new(Mutex::new(false)),
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
        rejected: Arc<Mutex<bool>>,
    }

    impl crate::sealed::arena_member::Sealed for ReentrantMember {}

    impl GestureArenaMember for ReentrantMember {
        fn accept_gesture(&self, _pointer: PointerId) {}

        fn reject_gesture(&self, pointer: PointerId) {
            *self.rejected.lock() = true;
            // Re-enter the arena from inside the reject callback.
            self.arena.resolve(pointer, None);
        }
    }

    struct PanickingAcceptMember;

    impl crate::sealed::arena_member::Sealed for PanickingAcceptMember {}

    impl GestureArenaMember for PanickingAcceptMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            panic!("accept panic");
        }

        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    struct PanickingAcceptAndDropMember;

    impl crate::sealed::arena_member::Sealed for PanickingAcceptAndDropMember {}

    impl GestureArenaMember for PanickingAcceptAndDropMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            panic!("callback panic wins");
        }

        fn reject_gesture(&self, _pointer: PointerId) {}
    }

    impl Drop for PanickingAcceptAndDropMember {
        fn drop(&mut self) {
            panic!("member drop panic");
        }
    }

    struct OrderedMember {
        name: &'static str,
        calls: Arc<Mutex<Vec<&'static str>>>,
        panic_on_accept: bool,
    }

    struct ReentrantAddMember {
        arena: GestureArena,
        fresh: Arc<MockMember>,
    }

    impl crate::sealed::arena_member::Sealed for ReentrantAddMember {}

    impl GestureArenaMember for ReentrantAddMember {
        fn accept_gesture(&self, _pointer: PointerId) {}

        fn reject_gesture(&self, pointer: PointerId) {
            self.arena.add(pointer, self.fresh.clone());
        }
    }

    impl crate::sealed::arena_member::Sealed for OrderedMember {}

    impl GestureArenaMember for OrderedMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            self.calls.lock().push(self.name);
            assert!(!self.panic_on_accept, "winner callback panic");
        }

        fn reject_gesture(&self, _pointer: PointerId) {
            self.calls.lock().push(self.name);
        }
    }

    #[test]
    fn reject_gesture_reentering_arena_does_not_deadlock() {
        use std::{sync::mpsc, time::Duration};

        // Run the arena work on a worker thread; a deadlock manifests as the
        // worker never reporting, caught by the receive timeout.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let arena = GestureArena::new();
            let pointer = PointerId::PRIMARY;
            let reentrant = Arc::new(ReentrantMember {
                arena: arena.clone(),
                rejected: Arc::new(Mutex::new(false)),
            });
            let winner = Arc::new(MockMember::new());
            arena.add(pointer, reentrant.clone());
            arena.add(pointer, winner.clone());
            arena.close(pointer);
            // Resolve for `winner`; `reentrant` is rejected and its callback
            // re-enters the arena. Must complete without hanging.
            arena.resolve(pointer, Some(winner.clone()));
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

    #[test]
    fn test_arena_single_member_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());

        let _entry = arena.add(pointer, member.clone());
        arena.close(pointer);

        assert!(!member.was_accepted());
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(member.was_accepted());
        assert!(!member.was_rejected());
    }

    #[test]
    fn close_marks_a_held_arena_closed() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        arena.add(pointer, Arc::new(MockMember::new()));
        arena.hold(pointer);

        arena.close(pointer);

        assert!(arena.is_held(pointer));
        assert!(
            !arena.is_open(pointer),
            "hold defers only sweep; it must not keep membership open"
        );
    }

    #[test]
    fn close_defers_a_lone_default_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        arena.close(pointer);

        assert!(
            !member.was_accepted(),
            "Flutter resolves the final member from a deferred microtask boundary"
        );
        assert!(arena.contains(pointer));
    }

    #[test]
    fn explicit_resolution_removes_slot_rejects_losers_then_finishes_panicking_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let winner = Arc::new(OrderedMember {
            name: "winner.accept",
            calls: Arc::clone(&calls),
            panic_on_accept: true,
        });
        let loser = Arc::new(OrderedMember {
            name: "loser.reject",
            calls: Arc::clone(&calls),
            panic_on_accept: false,
        });
        arena.add(pointer, winner.clone());
        arena.add(pointer, loser);
        arena.close(pointer);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.resolve(pointer, Some(winner));
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

    #[test]
    fn sweep_finishes_peers_and_preserves_callback_panic_over_member_drop_panic() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let peer = Arc::new(MockMember::new());
        arena.add(pointer, Arc::new(PanickingAcceptAndDropMember));
        arena.add(pointer, peer.clone());
        arena.close(pointer);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.sweep(pointer);
        }));
        let payload = unwind.expect_err("the first callback panic must resume");

        assert_eq!(payload.downcast_ref::<&str>(), Some(&"callback panic wins"));
        assert!(peer.was_rejected(), "later peers must still be notified");
        assert!(arena.is_empty());
    }

    #[test]
    fn stale_entry_cannot_resolve_a_reused_pointer_slot() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let old = Arc::new(MockMember::new());
        let stale_entry = arena.add(pointer, old);
        arena.close(pointer);
        arena.sweep(pointer);

        let fresh = Arc::new(MockMember::new());
        let competitor = Arc::new(MockMember::new());
        arena.add(pointer, fresh.clone());
        arena.add(pointer, competitor.clone());
        arena.close(pointer);

        stale_entry.resolve(GestureDisposition::Accepted);

        assert!(arena.contains(pointer));
        assert!(!fresh.was_rejected());
        assert!(!competitor.was_rejected());
    }

    #[test]
    fn rejecting_the_lone_member_cancels_its_deferred_default_win() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());
        let entry = arena.add(pointer, member.clone());
        arena.close(pointer);

        entry.resolve(GestureDisposition::Rejected);

        assert!(member.was_rejected());
        assert_eq!(arena.drain_deferred_resolutions(), 0);
        assert!(!member.was_accepted());
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn deferred_token_never_resolves_a_reused_pointer_generation() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let old = Arc::new(MockMember::new());
        arena.add(pointer, old.clone());
        arena.close(pointer);
        arena.abandon(pointer);

        let fresh = Arc::new(MockMember::new());
        arena.add(pointer, fresh.clone());
        arena.close(pointer);

        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(!old.was_accepted());
        assert!(fresh.was_accepted());
    }

    #[test]
    fn resolution_callback_adds_only_to_a_fresh_pointer_generation() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let fresh = Arc::new(MockMember::new());
        let reentrant = Arc::new(ReentrantAddMember {
            arena: arena.clone(),
            fresh: fresh.clone(),
        });
        let winner = Arc::new(MockMember::new());
        arena.add(pointer, reentrant);
        arena.add(pointer, winner.clone());
        arena.close(pointer);

        arena.resolve(pointer, Some(winner));

        assert_eq!(arena.member_count(pointer), 1);
        assert!(!fresh.was_rejected());
        assert!(arena.is_open(pointer));
    }

    #[test]
    fn test_arena_entry_resolve_accepted() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let _entry2 = arena.add(pointer, member2.clone());

        arena.close(pointer);

        // member1 resolves via entry handle
        entry1.resolve(GestureDisposition::Accepted);

        assert!(member1.was_accepted());
        assert!(!member1.was_rejected());

        assert!(!member2.was_accepted());
        assert!(member2.was_rejected());
    }

    #[test]
    fn test_arena_entry_resolve_rejected() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let _entry2 = arena.add(pointer, member2.clone());

        arena.close(pointer);

        // member1 rejects via entry handle
        entry1.resolve(GestureDisposition::Rejected);

        assert!(!member1.was_accepted());
        assert!(member1.was_rejected());

        // member2 wins by default at the deferred owner boundary.
        assert!(!member2.was_accepted());
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(member2.was_accepted());
        assert!(!member2.was_rejected());
    }

    #[test]
    fn test_arena_resolve_with_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        arena.add(pointer, member1.clone());
        arena.add(pointer, member2.clone());

        // member1 wins
        arena.resolve(pointer, Some(member1.clone()));

        assert!(member1.was_accepted());
        assert!(!member1.was_rejected());

        assert!(!member2.was_accepted());
        assert!(member2.was_rejected());
    }

    #[test]
    fn test_arena_hold_and_release() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member.clone());
        arena.hold(pointer);
        arena.close(pointer);

        // Close queues the default winner; hold affects only sweep.
        assert!(!member.was_accepted());
        assert!(arena.is_held(pointer));

        arena.release(pointer);
        assert!(
            !member.was_accepted(),
            "release is not a resolution boundary"
        );
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(member.was_accepted());
    }

    #[test]
    fn test_arena_sweep() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member.clone());
        assert!(arena.contains(pointer));

        arena.sweep(pointer);

        // Member should win (first member wins on sweep)
        assert!(member.was_accepted());
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn test_arena_is_empty() {
        let arena = GestureArena::new();
        assert!(arena.is_empty());

        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member);
        assert!(!arena.is_empty());

        arena.sweep(pointer);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_arena_member_count() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        assert_eq!(arena.member_count(pointer), 0);

        arena.add(pointer, Arc::new(MockMember::new()));
        assert_eq!(arena.member_count(pointer), 1);

        arena.add(pointer, Arc::new(MockMember::new()));
        assert_eq!(arena.member_count(pointer), 2);
    }

    #[test]
    fn test_gesture_disposition() {
        assert!(GestureDisposition::Accepted.is_accepted());
        assert!(!GestureDisposition::Accepted.is_rejected());

        assert!(GestureDisposition::Rejected.is_rejected());
        assert!(!GestureDisposition::Rejected.is_accepted());
    }

    #[test]
    fn test_arena_resolve_team() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());
        let member3 = Arc::new(MockMember::new());

        arena.add(pointer, member1.clone());
        arena.add(pointer, member2.clone());
        arena.add(pointer, member3.clone());

        // Resolve with multiple winners
        arena.resolve_team(pointer, &[member1.clone(), member2.clone()]);

        assert!(member1.was_accepted());
        assert!(member2.was_accepted());
        assert!(member3.was_rejected());

        assert!(
            !arena.contains(pointer),
            "explicit team resolution removes the exact slot"
        );
    }

    #[test]
    fn test_reject_member_leaves_a_competitor_to_win() {
        // Two members compete in a closed arena; one withdraws via
        // `reject_member`. Withdrawal must reject ONLY the bowing-out member —
        // the sole survivor then wins. Regression guard: a self-reject used to
        // resolve the whole entry with no winner, rejecting every competitor
        // (so e.g. a tap exceeding its slop silently killed the drag it raced).
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let bowing_out = Arc::new(MockMember::new());
        let survivor = Arc::new(MockMember::new());
        arena.add(pointer, bowing_out.clone());
        arena.add(pointer, survivor.clone());
        arena.close(pointer); // 2 members, no winner — stays open to compete

        let withdrawing: Arc<dyn GestureArenaMember> = bowing_out.clone();
        arena.reject_member(pointer, &withdrawing);

        assert!(
            bowing_out.was_rejected(),
            "the withdrawing member is rejected"
        );
        assert!(!survivor.was_accepted(), "default win is deferred");
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(
            survivor.was_accepted(),
            "the sole remaining member wins — withdrawal must not reject competitors",
        );
        assert!(!survivor.was_rejected(), "the survivor is not rejected");
    }

    #[test]
    fn test_reject_member_keeps_a_three_way_competition_open() {
        // With THREE members competing, one withdrawing must leave the other two
        // STILL competing — never force-resolve to the front member. Guards the
        // Withdrawing must NOT tear down an unresolved entry that still has
        // rivals (the latent bug a recogniser's `stop_tracking()`→`sweep()`
        // would have caused for 3+ members).
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let bowing_out = Arc::new(MockMember::new());
        let rival_a = Arc::new(MockMember::new());
        let rival_b = Arc::new(MockMember::new());
        arena.add(pointer, bowing_out.clone());
        arena.add(pointer, rival_a.clone());
        arena.add(pointer, rival_b.clone());
        arena.close(pointer); // 3 members, unresolved

        let withdrawing: Arc<dyn GestureArenaMember> = bowing_out.clone();
        arena.reject_member(pointer, &withdrawing);

        assert!(
            bowing_out.was_rejected(),
            "the withdrawing member is rejected"
        );
        assert!(
            !rival_a.was_accepted() && !rival_a.was_rejected(),
            "rival A keeps competing — not resolved either way",
        );
        assert!(
            !rival_b.was_accepted() && !rival_b.was_rejected(),
            "rival B keeps competing — not resolved either way",
        );
        assert_eq!(arena.member_count(pointer), 2);
        assert!(
            !arena.is_empty(),
            "the entry survives — two rivals are still in it"
        );
    }

    // ========================================================================
    // Eager Winner tests
    // ========================================================================

    #[test]
    fn test_eager_winner_wins_on_close() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let _entry2 = arena.add(pointer, member2.clone());

        // Arena is open, accept stores as eager winner
        assert!(arena.is_open(pointer));
        entry1.resolve(GestureDisposition::Accepted);
        assert!(arena.has_eager_winner(pointer));

        // Close arena - eager winner should win
        arena.close(pointer);

        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn test_first_eager_winner_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let entry2 = arena.add(pointer, member2.clone());

        // Both accept while arena is open - first wins
        entry1.resolve(GestureDisposition::Accepted);
        entry2.resolve(GestureDisposition::Accepted); // Ignored, already have eager winner

        arena.close(pointer);

        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
    }

    #[test]
    fn test_accept_after_close_resolves_immediately() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let _entry2 = arena.add(pointer, member2.clone());

        // Close arena first (no eager winner, no single member - stays unresolved)
        arena.close(pointer);
        assert!(!arena.is_open(pointer));
        assert_eq!(arena.member_count(pointer), 2);

        // Accept after close resolves immediately
        entry1.resolve(GestureDisposition::Accepted);

        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn test_reject_removes_eager_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());

        let entry1 = arena.add(pointer, member1.clone());
        let _entry2 = arena.add(pointer, member2.clone());

        // member1 accepts (becomes eager winner)
        entry1.resolve(GestureDisposition::Accepted);
        assert!(arena.has_eager_winner(pointer));

        // member1 rejects (removes eager winner)
        entry1.resolve(GestureDisposition::Rejected);
        assert!(!arena.has_eager_winner(pointer));
        assert!(member1.was_rejected());

        // Close - member2 is only one left, wins
        arena.close(pointer);
        assert!(!member2.was_accepted());
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(member2.was_accepted());
    }

    #[test]
    fn test_is_open_false_after_close() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member);

        assert!(arena.is_open(pointer));
        arena.close(pointer);
        assert!(!arena.is_open(pointer));
    }

    // ========================================================================
    // Pending Sweep tests
    // ========================================================================

    #[test]
    fn test_sweep_deferred_when_held() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.hold(pointer);

        // Sweep while held - should be deferred
        arena.sweep(pointer);
        assert!(arena.contains(pointer)); // Still there
        assert!(arena.has_pending_sweep(pointer));

        // Release triggers deferred sweep
        arena.release(pointer);
        assert!(!arena.contains(pointer)); // Now removed
    }

    #[test]
    fn deferred_release_removes_the_slot_before_notifying_members() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        arena.add(pointer, Arc::new(PanickingAcceptMember));
        arena.hold(pointer);
        arena.sweep(pointer);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.release(pointer);
        }));

        assert!(unwind.is_err(), "the member panic must propagate");
        assert!(
            !arena.contains(pointer),
            "a deferred sweep must remove its slot before arbitrary callbacks"
        );
    }

    #[test]
    fn test_sweep_immediate_when_not_held() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // Sweep when not held - immediate
        arena.sweep(pointer);
        assert!(!arena.contains(pointer));
        assert!(member.was_accepted()); // First member wins on sweep
    }

    #[test]
    fn test_pending_sweep_cleared_on_release() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member);
        arena.hold(pointer);
        arena.sweep(pointer);

        assert!(arena.has_pending_sweep(pointer));

        arena.release(pointer);
        // Arena should be removed, can't check pending_sweep anymore
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn release_does_not_close_an_open_arena() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.hold(pointer);

        // Arena still open and held
        assert!(arena.is_open(pointer));
        assert!(arena.is_held(pointer));

        arena.release(pointer);

        assert!(arena.is_open(pointer));
        assert!(!arena.is_held(pointer));
        assert!(!member.was_accepted());
    }

    // ========================================================================
    // GestureArenaEntry tests
    // ========================================================================

    #[test]
    fn test_entry_pointer_accessor() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(43).expect("nonzero pointer id");
        let member = Arc::new(MockMember::new());

        let entry = arena.add(pointer, member);

        assert_eq!(entry.pointer(), pointer);
    }

    #[test]
    fn test_entry_member_accessor() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());
        let member_dyn: Arc<dyn GestureArenaMember> = member.clone();

        let entry = arena.add(pointer, member_dyn.clone());

        let entry_member = entry.member().expect("active member");
        assert!(Arc::ptr_eq(&entry_member, &member_dyn));
    }

    #[test]
    fn test_entry_debug_impl() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(124).expect("nonzero pointer id");
        let member = Arc::new(MockMember::new());

        let entry = arena.add(pointer, member);
        let debug = format!("{entry:?}");

        assert!(debug.contains("GestureArenaEntry"));
        assert!(debug.contains("pointer"));
    }

    #[test]
    fn test_entry_resolve_multiple_times_is_safe() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        let entry = arena.add(pointer, member.clone());

        arena.close(pointer);

        // Resolve multiple times should be safe
        entry.resolve(GestureDisposition::Accepted);
        entry.resolve(GestureDisposition::Accepted);
        entry.resolve(GestureDisposition::Rejected); // Should be ignored

        assert!(member.was_accepted());
    }

    // ========================================================================
    // SweepModel
    // ========================================================================

    #[test]
    fn new_arena_is_self_driven_binding_arena_is_binding_driven() {
        assert_eq!(GestureArena::new().sweep_model(), SweepModel::SelfDriven);
        assert_eq!(
            GestureArena::with_capacity(4).sweep_model(),
            SweepModel::SelfDriven
        );
        let binding = GestureArena::binding_driven(Arc::new(SystemClock));
        assert_eq!(binding.sweep_model(), SweepModel::BindingDriven);
        // The model rides on the Arc-backed handle: every clone observes it.
        assert_eq!(binding.clone().sweep_model(), SweepModel::BindingDriven);
    }

    #[test]
    fn generation_exhaustion_panics_instead_of_reusing_an_old_token() {
        let arena = GestureArena::new();
        arena
            .next_generation
            .store(u64::MAX, std::sync::atomic::Ordering::Relaxed);

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            arena.add(PointerId::PRIMARY, Arc::new(MockMember::new()));
        }));

        let payload = unwind.expect_err("generation exhaustion must be explicit");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"BUG: gesture arena generation exhausted")
        );
        assert!(arena.is_empty());
    }

    #[test]
    fn run_pointer_lifecycle_closes_on_down_and_sweeps_on_up() {
        use crate::events::{PointerType, make_down_event, make_up_event};
        use flui_types::{Offset, geometry::px};

        let arena = GestureArena::binding_driven(Arc::new(SystemClock));
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // Down closes the arena and queues the lone default winner.
        let down = make_down_event(Offset::new(px(1.0), px(1.0)), PointerType::Touch);
        run_pointer_lifecycle(&arena, &down);
        assert!(!arena.is_open(pointer), "down must close the arena");
        assert!(
            !member.was_accepted(),
            "close must return before acceptance"
        );
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(member.was_accepted());

        // Up sweeps the (resolved) entry away.
        let up = make_up_event(Offset::new(px(1.0), px(1.0)), PointerType::Touch);
        run_pointer_lifecycle(&arena, &up);
        assert!(!arena.contains(pointer), "up must sweep the entry");
    }

    #[test]
    fn a_held_entry_defers_the_binding_sweep_until_release() {
        // The double-tap lifecycle leans on this: a held entry must defer the
        // binding's first-up sweep (so a competing front-member tap cannot win
        // early), and `release` must then drain the deferred sweep — removing
        // the entry. The double-tap resolves the contended entries explicitly
        // before releasing, so the deferred sweep is pure cleanup here.
        let arena = GestureArena::binding_driven(Arc::new(SystemClock));
        let pointer = PointerId::PRIMARY;
        let first = Arc::new(MockMember::new());
        let second = Arc::new(MockMember::new());
        arena.add(pointer, first.clone());
        arena.add(pointer, second.clone());
        arena.close(pointer); // 2 members, unresolved
        arena.hold(pointer);

        // Sweep while held: deferred, nobody resolved.
        arena.sweep(pointer);
        assert!(arena.contains(pointer), "the held sweep is deferred");
        assert!(arena.has_pending_sweep(pointer));
        assert!(!first.was_accepted(), "no resolution while held");
        assert!(!second.was_accepted());

        // Release drains the deferred sweep and removes the entry.
        arena.release(pointer);
        assert!(
            !arena.contains(pointer),
            "release drains the deferred sweep"
        );
    }
}
