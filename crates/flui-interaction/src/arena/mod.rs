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
//! - **Lock-free**: DashMap for concurrent access
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/GestureArenaManager-class.html>

// Submodules — these are part of the crate's public surface (they're
// referenced from recognizer code) so they're `pub` rather than `pub(crate)`.
pub mod signal_resolver;
pub mod team;

pub use signal_resolver::{PointerSignalResolver, SignalPriority};
pub use team::{GestureArenaTeam, TeamEntry};

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;
use tracing::instrument;

use crate::clock::{MonotonicClock, SystemClock};
use crate::ids::PointerId;

/// Default timeout for gesture disambiguation (100ms).
///
/// If no recognizer accepts within this time, the first member wins.
/// This matches Flutter's default arena timeout behavior.
pub const DEFAULT_DISAMBIGUATION_TIMEOUT: Duration = Duration::from_millis(100);

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
pub trait GestureArenaMember: crate::sealed::arena_member::Sealed + Send + Sync {
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
/// # Thread Safety
///
/// `GestureArenaEntry` is `Send + Sync` and can be safely shared across
/// threads. Multiple calls to `resolve` are safe (subsequent calls are no-ops
/// if the arena is already resolved).
#[derive(Clone)]
pub struct GestureArenaEntry {
    arena: GestureArena,
    pointer: PointerId,
    member: Arc<dyn GestureArenaMember>,
}

impl GestureArenaEntry {
    /// Create a new arena entry handle.
    fn new(arena: GestureArena, pointer: PointerId, member: Arc<dyn GestureArenaMember>) -> Self {
        Self {
            arena,
            pointer,
            member,
        }
    }

    /// Resolve this entry with the given disposition.
    ///
    /// Call with [`GestureDisposition::Accepted`] to claim victory, or
    /// [`GestureDisposition::Rejected`] to admit defeat.
    ///
    /// It's safe to call this on an arena that has already been resolved.
    pub fn resolve(&self, disposition: GestureDisposition) {
        self.arena
            .resolve_entry(self.pointer, &self.member, disposition);
    }

    /// Get the pointer ID for this entry.
    #[inline]
    pub fn pointer(&self) -> PointerId {
        self.pointer
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> &Arc<dyn GestureArenaMember> {
        &self.member
    }
}

impl std::fmt::Debug for GestureArenaEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaEntry")
            .field("pointer", &self.pointer)
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
    /// Winners of the arena (if resolved). Multiple winners possible with
    /// teams.
    winners: SmallVec<[Arc<dyn GestureArenaMember>; 2]>,
    /// When this arena entry was created (for timeout calculation).
    created_at: Instant,
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
            .field("winner_count", &self.winners.len())
            .field("age_ms", &self.created_at.elapsed().as_millis())
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

impl ArenaEntryData {
    fn new() -> Self {
        Self {
            members: SmallVec::new(),
            is_open: true,
            is_held: false,
            is_resolved: false,
            eager_winner: None,
            has_pending_sweep: false,
            winners: SmallVec::new(),
            created_at: Instant::now(),
        }
    }

    /// Close the arena - no more members can be added.
    /// If there's an eager winner, resolve immediately.
    #[must_use]
    fn close(&mut self) -> PendingNotifications {
        if !self.is_open || self.is_resolved {
            return SmallVec::new();
        }
        self.is_open = false;

        // If we have an eager winner, resolve in their favor
        if let Some(winner) = self.eager_winner.take() {
            self.resolve(Some(winner))
        } else if self.members.len() == 1 {
            // Single member wins automatically
            let winner = self.members[0].clone();
            self.resolve(Some(winner))
        } else {
            SmallVec::new()
        }
    }

    /// Accept gesture for a member.
    /// If arena is open, store as eager winner. If closed, resolve immediately.
    #[must_use]
    fn accept(&mut self, member: Arc<dyn GestureArenaMember>) -> PendingNotifications {
        if self.is_resolved {
            return SmallVec::new();
        }

        if self.is_open {
            // Store as eager winner - will win when arena closes
            if self.eager_winner.is_none() {
                self.eager_winner = Some(member);
            }
            // If already have eager winner, ignore subsequent accepts
            SmallVec::new()
        } else {
            // Arena closed, resolve immediately
            self.resolve(Some(member))
        }
    }

    /// Reject gesture for a member.
    #[must_use]
    fn reject(&mut self, member: &Arc<dyn GestureArenaMember>) -> PendingNotifications {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return pending;
        }

        // Remove from members
        self.members.retain(|m| !Arc::ptr_eq(m, member));

        // Remove from eager winner if it was this member
        if let Some(ref eager) = self.eager_winner
            && Arc::ptr_eq(eager, member)
        {
            self.eager_winner = None;
        }

        // Defer the member's rejection callback (dispatched after the entry
        // lock is released to avoid arena re-entrancy deadlock).
        pending.push((member.clone(), GestureDisposition::Rejected));

        // If only one member left and arena is closed, they win
        if !self.is_open && self.members.len() == 1 {
            let winner = self.members[0].clone();
            pending.extend(self.resolve(Some(winner)));
        }

        pending
    }

    /// Try to resolve the arena if conditions are met.
    /// Called after close or reject operations.
    #[must_use]
    fn try_to_resolve(&mut self) -> PendingNotifications {
        if self.is_resolved || self.is_open {
            return SmallVec::new();
        }

        if self.members.len() == 1 {
            // Single member wins automatically
            let winner = self.members[0].clone();
            self.resolve(Some(winner))
        } else if self.members.is_empty() {
            // No members left - resolve with no winner
            self.is_resolved = true;
            SmallVec::new()
        } else if let Some(eager) = self.eager_winner.take() {
            // Eager winner wins
            self.resolve(Some(eager))
        } else {
            SmallVec::new()
        }
    }

    /// Check if this arena has exceeded the given timeout.
    #[inline]
    fn has_timed_out(&self, timeout: Duration) -> bool {
        self.created_at.elapsed() >= timeout
    }

    /// Get the elapsed time since this arena was created.
    #[inline]
    fn elapsed(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Add a member to this arena.
    fn add(&mut self, member: Arc<dyn GestureArenaMember>) {
        if !self.is_resolved {
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
    /// Returns the member notifications to dispatch once the entry lock is
    /// released (winner -> `Accepted`, everyone else -> `Rejected`).
    #[must_use]
    fn resolve(&mut self, winner: Option<Arc<dyn GestureArenaMember>>) -> PendingNotifications {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return pending;
        }

        self.is_resolved = true;

        // Build winners list
        if let Some(w) = winner {
            self.winners.push(w);
        }

        // Collect member notifications; the caller dispatches them after the
        // entry lock is released (arena re-entrancy safety).
        for member in &self.members {
            // Check if this member is a winner
            let is_winner = self.winners.iter().any(|w| Arc::ptr_eq(member, w));
            let disposition = if is_winner {
                GestureDisposition::Accepted
            } else {
                GestureDisposition::Rejected
            };
            pending.push((member.clone(), disposition));
        }

        pending
    }

    /// Resolve the arena with multiple winners (team resolution).
    #[must_use]
    fn resolve_team(&mut self, winners: &[Arc<dyn GestureArenaMember>]) -> PendingNotifications {
        let mut pending = SmallVec::new();
        if self.is_resolved {
            return pending;
        }

        self.is_resolved = true;

        // Add all specified winners
        for winner in winners {
            if !self.winners.iter().any(|w| Arc::ptr_eq(w, winner)) {
                self.winners.push(winner.clone());
            }
        }

        // Collect member notifications (dispatched after the entry lock is
        // released).
        for member in &self.members {
            let is_winner = self.winners.iter().any(|w| Arc::ptr_eq(member, w));
            let disposition = if is_winner {
                GestureDisposition::Accepted
            } else {
                GestureDisposition::Rejected
            };
            pending.push((member.clone(), disposition));
        }

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
/// - [`SelfDriven`](Self::SelfDriven) — the historical private-arena default. A
///   recognizer (or a detector owning a private arena) closes/sweeps the arena
///   itself, so [`RecognizerBase::stop_tracking`] sweeps on up. Used by every
///   standalone recognizer path and a `GestureDetector` with no
///   `GestureArenaScope` above it.
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
    /// The recognizer / detector owns the lifecycle (private arena).
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
/// Shared by the headless binding and any future production `GestureBinding`.
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
/// # Thread Safety
///
/// GestureArena is thread-safe and uses DashMap for lock-free concurrent
/// access.
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
    /// Map from pointer ID to arena entry (lock-free concurrent HashMap).
    entries: Arc<DashMap<PointerId, Mutex<ArenaEntryData>>>,
    /// The time source deadline-driven recognizers read `now()` from. Defaults
    /// to the OS clock; a headless frame driver injects a `ManualClock` so a
    /// deadline (e.g. long-press) elapses deterministically without sleeping.
    clock: Arc<dyn MonotonicClock>,
    /// Who owns the close/sweep lifecycle. Immutable per arena (like the clock);
    /// recognizers read it to decide whether `stop_tracking` should self-sweep.
    sweep_model: SweepModel,
}

impl GestureArena {
    /// Create a new gesture arena driven by the real OS clock.
    #[inline]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// Create a gesture arena with an explicit time source.
    ///
    /// Production uses [`new`](Self::new) (the OS clock); a headless frame driver
    /// passes a [`ManualClock`](crate::clock::ManualClock) it advances per frame
    /// so deadline-driven recognizers resolve deterministically with no sleep.
    #[inline]
    pub fn with_clock(clock: Arc<dyn MonotonicClock>) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
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
    /// (or a future production `GestureBinding`) hands down to a subtree.
    #[inline]
    pub fn binding_driven(clock: Arc<dyn MonotonicClock>) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            clock,
            sweep_model: SweepModel::BindingDriven,
        }
    }

    /// Create a gesture arena with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(DashMap::with_capacity(capacity)),
            clock: Arc::new(SystemClock),
            sweep_model: SweepModel::SelfDriven,
        }
    }

    /// Who owns this arena's close/sweep lifecycle.
    ///
    /// [`SelfDriven`](SweepModel::SelfDriven) for a private arena (the
    /// recognizer sweeps itself); [`BindingDriven`](SweepModel::BindingDriven)
    /// for a binding-owned shared arena.
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
        // Insert-or-get and add the member under the SAME entry handle, so a
        // concurrent sweep cannot remove the slot between creation and member
        // insertion (a second `get()` could miss, silently dropping the
        // member while still returning a live `GestureArenaEntry`). Lock
        // ordering (shard guard then inner `Mutex`) matches every other call
        // site, and the critical section is a single `SmallVec` push.
        self.entries
            .entry(pointer)
            .or_insert_with(|| Mutex::new(ArenaEntryData::new()))
            .lock()
            .add(member.clone());

        GestureArenaEntry::new(self.clone(), pointer, member)
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            if entry.is_held {
                return; // Arena is held open
            }

            entry.close()
        } else {
            return;
        };
        Self::dispatch_pending(pending, pointer);
    }

    /// Dispatch deferred member notifications after the per-entry `Mutex` has
    /// been released. Keeping member callbacks out of the locked region is
    /// what makes the arena re-entrancy-safe (a handler may call back into the
    /// arena). See [`PendingNotifications`].
    #[inline]
    fn dispatch_pending(pending: PendingNotifications, pointer: PointerId) {
        for (member, disposition) in pending {
            match disposition {
                GestureDisposition::Accepted => member.accept_gesture(pointer),
                GestureDisposition::Rejected => member.reject_gesture(pointer),
            }
        }
    }

    /// Internal method: resolve an entry with given disposition.
    ///
    /// Called by [`GestureArenaEntry::resolve`].
    fn resolve_entry(
        &self,
        pointer: PointerId,
        member: &Arc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
    ) {
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            match disposition {
                GestureDisposition::Accepted => entry.accept(member.clone()),
                GestureDisposition::Rejected => {
                    let mut pending = entry.reject(member);
                    if !entry.is_open {
                        pending.extend(entry.try_to_resolve());
                    }
                    pending
                }
            }
        } else {
            return;
        };
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().accept(member)
        } else {
            return;
        };
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();
            let mut pending = entry.reject(member);
            if !entry.is_open {
                pending.extend(entry.try_to_resolve());
            }
            pending
        } else {
            return;
        };
        Self::dispatch_pending(pending, pointer);
    }

    /// Hold the arena open for a pointer (delay resolution).
    ///
    /// Used when a recognizer needs more time to decide.
    pub fn hold(&self, pointer: PointerId) {
        if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().hold();
        }
    }

    /// Release the hold on an arena.
    ///
    /// If the arena was waiting to close, it will close now. If a sweep was
    /// pending (deferred while held), the deferred sweep drains and the entry
    /// is removed.
    ///
    /// # Contract
    ///
    /// The caller is responsible for ensuring the entry is already resolved
    /// (or has no members) before the deferred sweep drains. Releasing an
    /// unresolved multi-member entry causes the entry to be silently removed
    /// without invoking `accept_gesture` or `reject_gesture` on any member.
    /// The correct pattern: resolve (or `reject_member` until one winner
    /// remains) *before* calling `release`, so `has_pending_sweep` drains a
    /// settled entry.
    pub fn release(&self, pointer: PointerId) {
        let mut pending = PendingNotifications::new();
        let should_sweep = {
            if let Some(entry_ref) = self.entries.get(&pointer) {
                let mut entry = entry_ref.lock();
                entry.release();

                // If arena was waiting to close, close it now
                if !entry.is_held && !entry.is_resolved {
                    pending = entry.close();
                }

                // Check if sweep was pending
                if entry.has_pending_sweep {
                    entry.has_pending_sweep = false;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        // Dispatch deferred notifications after releasing the entry lock.
        Self::dispatch_pending(pending, pointer);

        // Execute pending sweep outside the lock
        if should_sweep {
            self.entries.remove(&pointer);
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().resolve(winner)
        } else {
            return;
        };
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().reject(member)
        } else {
            return;
        };
        Self::dispatch_pending(pending, pointer);
    }

    /// Remove the entry for `pointer` only if it has **settled** — a winner has
    /// been resolved, or no members remain.
    ///
    /// Unlike [`sweep`](Self::sweep), this never force-resolves a still-open
    /// competition (first-member-wins): an entry that still has rivals is left
    /// untouched. It is the teardown a withdrawing member uses (after
    /// [`reject_member`](Self::reject_member)) to clean up the shared entry
    /// without disturbing the members still competing for the pointer.
    pub fn remove_if_settled(&self, pointer: PointerId) {
        let settled = if let Some(entry_ref) = self.entries.get(&pointer) {
            let entry = entry_ref.lock();
            entry.is_resolved || entry.members.is_empty()
        } else {
            return;
        };
        if settled {
            self.entries.remove(&pointer);
        }
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
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().resolve_team(winners)
        } else {
            return;
        };
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
        // Check if held - if so, mark pending sweep
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            if entry.is_held {
                entry.has_pending_sweep = true;
                return;
            }

            // Force resolve if not resolved yet (first member wins)
            if !entry.is_resolved && !entry.members.is_empty() {
                let winner = entry.members[0].clone();
                entry.resolve(Some(winner))
            } else {
                PendingNotifications::new()
            }
        } else {
            PendingNotifications::new()
        };

        // Dispatch notifications after releasing the entry lock, before
        // removing the slot.
        Self::dispatch_pending(pending, pointer);

        self.entries.remove(&pointer);
    }

    /// Poll every active member's time-based deadline (e.g. long-press hold).
    ///
    /// Call once per frame from the UI thread. Members are snapshotted out of
    /// the per-entry locks *before* polling, because a deadline hook may fire
    /// user callbacks and re-enter the arena to resolve — invoking it under the
    /// entry lock would re-introduce the arena re-entrancy deadlock. A member
    /// that tracks several pointers is polled once per pointer; `poll_deadline`
    /// is contractually idempotent, so the duplicate polls are harmless.
    ///
    /// Complexity: O(P + M) where P is the number of open arenas and M the
    /// total active members — both bounded by the simultaneous-pointer cap.
    pub fn poll_deadlines(&self) {
        let mut members: SmallVec<[Arc<dyn GestureArenaMember>; 8]> = SmallVec::new();
        for entry in self.entries.iter() {
            members.extend(entry.value().lock().members.iter().cloned());
        }
        for member in members {
            member.poll_deadline();
        }
    }

    /// Get the number of active arenas.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if arena is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if an arena exists for a pointer.
    #[inline]
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.entries.contains_key(&pointer)
    }

    /// Get the primary winner for a pointer (if resolved).
    ///
    /// Returns the first winner. Use [`winners`](Self::winners) to get all
    /// winners when team resolution is used.
    pub fn winner(&self, pointer: PointerId) -> Option<Arc<dyn GestureArenaMember>> {
        self.entries
            .get(&pointer)
            .and_then(|entry_ref| entry_ref.lock().winners.first().cloned())
    }

    /// Get all winners for a pointer (if resolved).
    ///
    /// Returns all winners including team members. Empty if not resolved or no
    /// winners.
    pub fn winners(&self, pointer: PointerId) -> Vec<Arc<dyn GestureArenaMember>> {
        self.entries
            .get(&pointer)
            .map(|entry_ref| entry_ref.lock().winners.to_vec())
            .unwrap_or_default()
    }

    /// Get the number of winners for a pointer.
    pub fn winner_count(&self, pointer: PointerId) -> usize {
        self.entries
            .get(&pointer)
            .map(|entry_ref| entry_ref.lock().winners.len())
            .unwrap_or(0)
    }

    /// Check if an arena is resolved.
    pub fn is_resolved(&self, pointer: PointerId) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry_ref| entry_ref.lock().is_resolved)
    }

    /// Check if an arena is held.
    pub fn is_held(&self, pointer: PointerId) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry_ref| entry_ref.lock().is_held)
    }

    /// Check if an arena is open (accepting new members).
    pub fn is_open(&self, pointer: PointerId) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry_ref| entry_ref.lock().is_open)
    }

    /// Check if an arena has an eager winner.
    pub fn has_eager_winner(&self, pointer: PointerId) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry_ref| entry_ref.lock().eager_winner.is_some())
    }

    /// Check if sweep is pending for an arena.
    pub fn has_pending_sweep(&self, pointer: PointerId) -> bool {
        self.entries
            .get(&pointer)
            .is_some_and(|entry_ref| entry_ref.lock().has_pending_sweep)
    }

    /// Get the number of members in an arena.
    pub fn member_count(&self, pointer: PointerId) -> usize {
        self.entries
            .get(&pointer)
            .map(|entry_ref| entry_ref.lock().members.len())
            .unwrap_or(0)
    }

    // ========================================================================
    // Timeout-based disambiguation
    // ========================================================================

    /// Check if an arena has exceeded its timeout.
    ///
    /// Returns `true` if the arena exists, is not resolved, and has been
    /// waiting longer than the specified timeout.
    pub fn has_timed_out(&self, pointer: PointerId, timeout: Duration) -> bool {
        self.entries.get(&pointer).is_some_and(|entry_ref| {
            let entry = entry_ref.lock();
            !entry.is_resolved && entry.has_timed_out(timeout)
        })
    }

    /// Check if an arena has exceeded the default timeout.
    ///
    /// Uses [`DEFAULT_DISAMBIGUATION_TIMEOUT`] (100ms).
    #[inline]
    pub fn has_default_timeout(&self, pointer: PointerId) -> bool {
        self.has_timed_out(pointer, DEFAULT_DISAMBIGUATION_TIMEOUT)
    }

    /// Get the elapsed time for an arena.
    ///
    /// Returns `None` if the arena doesn't exist.
    pub fn elapsed(&self, pointer: PointerId) -> Option<Duration> {
        self.entries
            .get(&pointer)
            .map(|entry_ref| entry_ref.lock().elapsed())
    }

    /// Force resolve an arena due to timeout.
    ///
    /// If the arena is not held and has timed out:
    /// - If there's at least one member, the first member wins
    /// - If there are no members, the arena is resolved with no winner
    ///
    /// Returns `true` if the arena was force-resolved.
    pub fn force_resolve_if_timed_out(&self, pointer: PointerId, timeout: Duration) -> bool {
        let pending = if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            // Skip if already resolved or held
            if entry.is_resolved || entry.is_held {
                return false;
            }

            // Check timeout
            if !entry.has_timed_out(timeout) {
                return false;
            }

            tracing::trace!(
                ?pointer,
                elapsed_ms = entry.elapsed().as_millis(),
                member_count = entry.members.len(),
                "Force resolving arena due to timeout"
            );

            // First member wins (if any)
            let winner = entry.members.first().cloned();
            entry.resolve(winner)
        } else {
            return false;
        };

        Self::dispatch_pending(pending, pointer);
        true
    }

    /// Force resolve with default timeout.
    ///
    /// Uses [`DEFAULT_DISAMBIGUATION_TIMEOUT`] (100ms).
    #[inline]
    pub fn force_resolve_if_default_timeout(&self, pointer: PointerId) -> bool {
        self.force_resolve_if_timed_out(pointer, DEFAULT_DISAMBIGUATION_TIMEOUT)
    }

    /// Check all arenas and force resolve any that have timed out.
    ///
    /// Returns the number of arenas that were force-resolved.
    ///
    /// This should be called periodically (e.g., on each frame) to handle
    /// disambiguation timeouts.
    pub fn resolve_timed_out_arenas(&self, timeout: Duration) -> usize {
        let mut resolved_count = 0;

        // Collect pointers to check (avoid holding iteration lock during resolve)
        let pointers: Vec<PointerId> = self.entries.iter().map(|e| *e.key()).collect();

        for pointer in pointers {
            if self.force_resolve_if_timed_out(pointer, timeout) {
                resolved_count += 1;
            }
        }

        if resolved_count > 0 {
            tracing::trace!(count = resolved_count, "Force resolved timed out arenas");
        }

        resolved_count
    }

    /// Check all arenas with default timeout.
    ///
    /// Uses [`DEFAULT_DISAMBIGUATION_TIMEOUT`] (100ms).
    #[inline]
    pub fn resolve_default_timed_out_arenas(&self) -> usize {
        self.resolve_timed_out_arenas(DEFAULT_DISAMBIGUATION_TIMEOUT)
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
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Err(_) => panic!("arena deadlocked on reentrant reject_gesture"),
        }
    }

    #[test]
    fn test_arena_single_member_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());

        let _entry = arena.add(pointer, member.clone());
        arena.close(pointer);

        assert!(member.was_accepted());
        assert!(!member.was_rejected());
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

        // member2 wins by default (only one left)
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

        // Should not resolve yet (held)
        assert!(!member.was_accepted());
        assert!(arena.is_held(pointer));

        arena.release(pointer);

        // Should resolve now
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

        // Should have 2 winners
        let winners = arena.winners(pointer);
        assert_eq!(winners.len(), 2);
    }

    #[test]
    fn test_arena_winners_empty_when_not_resolved() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        arena.add(pointer, Arc::new(MockMember::new()));

        assert!(arena.winners(pointer).is_empty());
        assert_eq!(arena.winner_count(pointer), 0);
    }

    // ========================================================================
    // Timeout tests
    // ========================================================================

    #[test]
    fn test_default_disambiguation_timeout_is_100ms() {
        assert_eq!(DEFAULT_DISAMBIGUATION_TIMEOUT, Duration::from_millis(100));
    }

    #[test]
    fn test_arena_elapsed_returns_duration() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        // No arena yet
        assert!(arena.elapsed(pointer).is_none());

        arena.add(pointer, Arc::new(MockMember::new()));

        // Should have elapsed time
        let elapsed = arena.elapsed(pointer);
        assert!(elapsed.is_some());
        assert!(elapsed.unwrap() < Duration::from_secs(1)); // Should be very short
    }

    #[test]
    fn test_arena_has_timed_out_false_initially() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        arena.add(pointer, Arc::new(MockMember::new()));

        // Should not have timed out immediately with default timeout
        assert!(!arena.has_timed_out(pointer, DEFAULT_DISAMBIGUATION_TIMEOUT));
        assert!(!arena.has_default_timeout(pointer));
    }

    #[test]
    fn test_arena_has_timed_out_with_zero_duration() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        arena.add(pointer, Arc::new(MockMember::new()));

        // Zero duration should always be timed out
        assert!(arena.has_timed_out(pointer, Duration::ZERO));
    }

    #[test]
    fn test_arena_has_timed_out_false_for_resolved() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.resolve(pointer, Some(member));

        // Already resolved, so should not report timed out
        assert!(!arena.has_timed_out(pointer, Duration::ZERO));
    }

    #[test]
    fn test_force_resolve_if_timed_out() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // With zero timeout, should force resolve immediately
        let resolved = arena.force_resolve_if_timed_out(pointer, Duration::ZERO);

        assert!(resolved);
        assert!(arena.is_resolved(pointer));
        assert!(member.was_accepted()); // First member wins
    }

    #[test]
    fn test_force_resolve_first_member_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());
        let member3 = Arc::new(MockMember::new());

        arena.add(pointer, member1.clone());
        arena.add(pointer, member2.clone());
        arena.add(pointer, member3.clone());

        // Force resolve with zero timeout
        arena.force_resolve_if_timed_out(pointer, Duration::ZERO);

        // First member should win
        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
        assert!(member3.was_rejected());
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
        // withdrawing-member teardown: `remove_if_settled` must NOT tear down an
        // unresolved entry that still has rivals (the latent bug a recogniser's
        // `stop_tracking()`→`sweep()` would have caused for 3+ members).
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
        arena.remove_if_settled(pointer); // the teardown a withdrawing member runs

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
        assert!(
            !arena.is_resolved(pointer),
            "the entry is not force-resolved"
        );
        assert!(
            !arena.is_empty(),
            "the entry survives — two rivals are still in it"
        );
    }

    #[test]
    fn test_force_resolve_does_nothing_if_held() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.hold(pointer);

        // Should not force resolve when held
        let resolved = arena.force_resolve_if_timed_out(pointer, Duration::ZERO);

        assert!(!resolved);
        assert!(!arena.is_resolved(pointer));
        assert!(!member.was_accepted());
    }

    #[test]
    fn test_force_resolve_does_nothing_if_already_resolved() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.resolve(pointer, Some(member.clone()));

        // Already resolved, should return false
        let resolved = arena.force_resolve_if_timed_out(pointer, Duration::ZERO);
        assert!(!resolved);
    }

    #[test]
    fn test_force_resolve_does_nothing_if_not_timed_out() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // With a very long timeout, should not force resolve
        let resolved = arena.force_resolve_if_timed_out(pointer, Duration::from_secs(3600));

        assert!(!resolved);
        assert!(!arena.is_resolved(pointer));
    }

    #[test]
    fn test_resolve_timed_out_arenas() {
        let arena = GestureArena::new();

        let pointer1 = PointerId::PRIMARY;
        let pointer2 = PointerId::new(2).expect("nonzero pointer id");
        let pointer3 = PointerId::new(3).expect("nonzero pointer id");

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());
        let member3 = Arc::new(MockMember::new());

        arena.add(pointer1, member1.clone());
        arena.add(pointer2, member2.clone());
        arena.add(pointer3, member3.clone());

        // Already resolve pointer2
        arena.resolve(pointer2, Some(member2.clone()));

        // Hold pointer3
        arena.hold(pointer3);

        // Resolve all timed out arenas with zero timeout
        let count = arena.resolve_timed_out_arenas(Duration::ZERO);

        // Only pointer1 should be force resolved (pointer2 already resolved, pointer3
        // held)
        assert_eq!(count, 1);
        assert!(arena.is_resolved(pointer1));
        assert!(arena.is_resolved(pointer2)); // Was already resolved
        assert!(!arena.is_resolved(pointer3)); // Still held

        assert!(member1.was_accepted());
        assert!(!member3.was_accepted());
    }

    #[test]
    fn test_resolve_default_timed_out_arenas() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        arena.add(pointer, Arc::new(MockMember::new()));

        // Should not resolve with default timeout (just created)
        let count = arena.resolve_default_timed_out_arenas();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_force_resolve_with_no_members() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        // Create empty arena entry by adding and then we test empty case
        // Note: We need to create an arena entry first
        arena.add(pointer, Arc::new(MockMember::new()));

        // This tests the force resolve path, member should win
        let resolved = arena.force_resolve_if_timed_out(pointer, Duration::ZERO);
        assert!(resolved);
    }

    #[test]
    fn test_force_resolve_if_default_timeout() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // Should not resolve immediately (not timed out yet)
        let resolved = arena.force_resolve_if_default_timeout(pointer);
        assert!(!resolved);
        assert!(!arena.is_resolved(pointer));
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
        assert!(arena.is_resolved(pointer));
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
        assert!(!arena.is_resolved(pointer));

        // Accept after close resolves immediately
        entry1.resolve(GestureDisposition::Accepted);

        assert!(member1.was_accepted());
        assert!(member2.was_rejected());
        assert!(arena.is_resolved(pointer));
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
    fn test_release_closes_arena_if_open() {
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.hold(pointer);

        // Arena still open and held
        assert!(arena.is_open(pointer));
        assert!(arena.is_held(pointer));

        arena.release(pointer);

        // Release should close and resolve single member
        assert!(member.was_accepted());
        assert!(arena.is_resolved(pointer));
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

        assert!(Arc::ptr_eq(entry.member(), &member_dyn));
    }

    #[test]
    fn test_entry_debug_impl() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(124).expect("nonzero pointer id");
        let member = Arc::new(MockMember::new());

        let entry = arena.add(pointer, member);
        let debug = format!("{:?}", entry);

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
    fn run_pointer_lifecycle_closes_on_down_and_sweeps_on_up() {
        use crate::events::{PointerType, make_down_event, make_up_event};
        use flui_types::{Offset, geometry::px};

        let arena = GestureArena::binding_driven(Arc::new(SystemClock));
        let pointer = PointerId::PRIMARY;
        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // Down closes the arena (single member wins on close).
        let down = make_down_event(Offset::new(px(1.0), px(1.0)), PointerType::Touch);
        run_pointer_lifecycle(&arena, &down);
        assert!(!arena.is_open(pointer), "down must close the arena");
        assert!(member.was_accepted(), "the single member wins on close");

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
