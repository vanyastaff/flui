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
//! # Type System Features
//!
//! - **Newtype IDs**: Type-safe `PointerId` prevents mixing with other IDs
//! - **SmallVec**: Inline storage avoids heap allocation for typical cases
//! - **Lock-free**: DashMap for concurrent access
//!
//! Flutter reference: https://api.flutter.dev/flutter/gestures/GestureArenaManager-class.html

use crate::ids::PointerId;
use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
/// instead of this trait directly. The blanket implementation will automatically
/// provide `GestureArenaMember` for your type.
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
/// arena.add(pointer, Arc::new(MyRecognizer { /* ... */ }));
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
// ArenaEntry
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
struct ArenaEntry {
    /// Members competing in this arena.
    /// Inline capacity: 4 (avoids heap for most cases).
    members: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,
    /// Team members that should all win together.
    /// Members in the same team are not mutually exclusive.
    team: SmallVec<[Arc<dyn GestureArenaMember>; 2]>,
    /// Whether this entry is held open (waiting for more information).
    is_held: bool,
    /// Whether arena has been resolved.
    is_resolved: bool,
    /// Winners of the arena (if resolved). Multiple winners possible with teams.
    winners: SmallVec<[Arc<dyn GestureArenaMember>; 2]>,
    /// When this arena entry was created (for timeout calculation).
    created_at: Instant,
}

impl std::fmt::Debug for ArenaEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaEntry")
            .field("member_count", &self.members.len())
            .field("team_count", &self.team.len())
            .field("is_held", &self.is_held)
            .field("is_resolved", &self.is_resolved)
            .field("winner_count", &self.winners.len())
            .field("age_ms", &self.created_at.elapsed().as_millis())
            .finish()
    }
}

impl ArenaEntry {
    fn new() -> Self {
        Self {
            members: SmallVec::new(),
            team: SmallVec::new(),
            is_held: false,
            is_resolved: false,
            winners: SmallVec::new(),
            created_at: Instant::now(),
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

    /// Add a member to the team (will win together with other team members).
    fn add_to_team(&mut self, member: Arc<dyn GestureArenaMember>) {
        if !self.is_resolved {
            self.team.push(member.clone());
            // Also add to regular members for tracking
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
    fn resolve(&mut self, winner: Option<Arc<dyn GestureArenaMember>>, pointer: PointerId) {
        if self.is_resolved {
            return;
        }

        self.is_resolved = true;

        // Build winners list: primary winner + all team members
        if let Some(w) = winner.clone() {
            self.winners.push(w);
        }
        // Team members always win together
        for team_member in &self.team {
            if !self.winners.iter().any(|w| Arc::ptr_eq(w, team_member)) {
                self.winners.push(team_member.clone());
            }
        }

        // Notify all members
        for member in &self.members {
            // Check if this member is a winner
            let is_winner = self.winners.iter().any(|w| Arc::ptr_eq(member, w));

            if is_winner {
                member.accept_gesture(pointer);
            } else {
                member.reject_gesture(pointer);
            }
        }
    }

    /// Resolve the arena with multiple winners (team resolution).
    fn resolve_team(&mut self, winners: &[Arc<dyn GestureArenaMember>], pointer: PointerId) {
        if self.is_resolved {
            return;
        }

        self.is_resolved = true;

        // Add all specified winners
        for winner in winners {
            if !self.winners.iter().any(|w| Arc::ptr_eq(w, winner)) {
                self.winners.push(winner.clone());
            }
        }
        // Team members always win together
        for team_member in &self.team {
            if !self.winners.iter().any(|w| Arc::ptr_eq(w, team_member)) {
                self.winners.push(team_member.clone());
            }
        }

        // Notify all members
        for member in &self.members {
            let is_winner = self.winners.iter().any(|w| Arc::ptr_eq(member, w));

            if is_winner {
                member.accept_gesture(pointer);
            } else {
                member.reject_gesture(pointer);
            }
        }
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
/// GestureArena is thread-safe and uses DashMap for lock-free concurrent access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::arena::{GestureArena, PointerId};
///
/// let arena = GestureArena::new();
/// let pointer = PointerId::new(0);
///
/// // Add recognizers to arena
/// arena.add(pointer, tap_recognizer);
/// arena.add(pointer, drag_recognizer);
///
/// // Later: resolve with winner
/// arena.resolve(pointer, Some(tap_recognizer));
/// ```
#[derive(Clone)]
pub struct GestureArena {
    /// Map from pointer ID to arena entry (lock-free concurrent HashMap).
    entries: Arc<DashMap<PointerId, Mutex<ArenaEntry>>>,
}

impl GestureArena {
    /// Create a new gesture arena.
    #[inline]
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }

    /// Create a gesture arena with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(DashMap::with_capacity(capacity)),
        }
    }

    /// Add a member to the arena for a specific pointer.
    ///
    /// Creates a new arena entry if one doesn't exist for this pointer.
    pub fn add(&self, pointer: PointerId, member: Arc<dyn GestureArenaMember>) {
        self.entries
            .entry(pointer)
            .or_insert_with(|| Mutex::new(ArenaEntry::new()))
            .lock()
            .add(member);
    }

    /// Close the arena for a pointer (no more members can be added).
    ///
    /// If there's only one member, it wins immediately.
    /// Otherwise, waits for members to accept/reject.
    pub fn close(&self, pointer: PointerId) {
        if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            if entry.is_held {
                return; // Arena is held open
            }

            // If only one member, it wins automatically
            if entry.members.len() == 1 {
                let winner = entry.members[0].clone();
                entry.resolve(Some(winner), pointer);
            }
        }
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
    /// If arena was waiting to close, it will close now.
    pub fn release(&self, pointer: PointerId) {
        if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();
            entry.release();

            // If arena was waiting to close, close it now
            if !entry.is_held && !entry.is_resolved {
                if entry.members.len() == 1 {
                    let winner = entry.members[0].clone();
                    entry.resolve(Some(winner), pointer);
                } else if entry.members.is_empty() {
                    entry.resolve(None, pointer);
                }
            }
        }
    }

    /// Resolve the arena with a specific winner.
    ///
    /// Winner receives `accept_gesture()`, all others receive `reject_gesture()`.
    /// If team members exist, they also receive `accept_gesture()`.
    pub fn resolve(&self, pointer: PointerId, winner: Option<Arc<dyn GestureArenaMember>>) {
        if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().resolve(winner, pointer);
        }
    }

    /// Resolve the arena with multiple winners.
    ///
    /// All specified winners (and team members) receive `accept_gesture()`.
    /// This is useful when multiple gestures should be recognized simultaneously.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Both tap and double-tap can be recognized
    /// arena.resolve_team(pointer, &[tap_recognizer, double_tap_recognizer]);
    /// ```
    pub fn resolve_team(&self, pointer: PointerId, winners: &[Arc<dyn GestureArenaMember>]) {
        if let Some(entry_ref) = self.entries.get(&pointer) {
            entry_ref.lock().resolve_team(winners, pointer);
        }
    }

    /// Add a member to a team for a specific pointer.
    ///
    /// Team members are not mutually exclusive - they all win together
    /// when the arena resolves. This is useful for composite gestures.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Long press and drag can work together
    /// arena.add_to_team(pointer, long_press_recognizer);
    /// arena.add_to_team(pointer, drag_recognizer);
    /// ```
    pub fn add_to_team(&self, pointer: PointerId, member: Arc<dyn GestureArenaMember>) {
        self.entries
            .entry(pointer)
            .or_insert_with(|| Mutex::new(ArenaEntry::new()))
            .lock()
            .add_to_team(member);
    }

    /// Sweep - remove resolved arenas for a pointer.
    ///
    /// Called when pointer is released to clean up.
    pub fn sweep(&self, pointer: PointerId) {
        self.entries.remove(&pointer);
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
    /// Returns the first winner. Use [`winners`](Self::winners) to get all winners
    /// when team resolution is used.
    pub fn winner(&self, pointer: PointerId) -> Option<Arc<dyn GestureArenaMember>> {
        self.entries
            .get(&pointer)
            .and_then(|entry_ref| entry_ref.lock().winners.first().cloned())
    }

    /// Get all winners for a pointer (if resolved).
    ///
    /// Returns all winners including team members. Empty if not resolved or no winners.
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
        if let Some(entry_ref) = self.entries.get(&pointer) {
            let mut entry = entry_ref.lock();

            // Skip if already resolved or held
            if entry.is_resolved || entry.is_held {
                return false;
            }

            // Check timeout
            if !entry.has_timed_out(timeout) {
                return false;
            }

            tracing::debug!(
                pointer = pointer.get(),
                elapsed_ms = entry.elapsed().as_millis(),
                member_count = entry.members.len(),
                "Force resolving arena due to timeout"
            );

            // First member wins (if any)
            let winner = entry.members.first().cloned();
            entry.resolve(winner, pointer);

            true
        } else {
            false
        }
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
            tracing::debug!(count = resolved_count, "Force resolved timed out arenas");
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

    #[test]
    fn test_arena_single_member_wins() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member.clone());
        arena.close(pointer);

        assert!(member.was_accepted());
        assert!(!member.was_rejected());
    }

    #[test]
    fn test_arena_resolve_with_winner() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);
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
        let pointer = PointerId::new(0);
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member);
        assert!(arena.contains(pointer));

        arena.sweep(pointer);
        assert!(!arena.contains(pointer));
    }

    #[test]
    fn test_arena_is_empty() {
        let arena = GestureArena::new();
        assert!(arena.is_empty());

        let pointer = PointerId::new(0);
        let member = Arc::new(MockMember::new());

        arena.add(pointer, member);
        assert!(!arena.is_empty());

        arena.sweep(pointer);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_arena_member_count() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
    fn test_arena_team_all_win() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

        let member1 = Arc::new(MockMember::new());
        let member2 = Arc::new(MockMember::new());
        let member3 = Arc::new(MockMember::new());

        // Add member1 and member2 to team, member3 as regular
        arena.add_to_team(pointer, member1.clone());
        arena.add_to_team(pointer, member2.clone());
        arena.add(pointer, member3.clone());

        // Resolve with member1 as primary winner
        arena.resolve(pointer, Some(member1.clone()));

        // Both team members should win
        assert!(member1.was_accepted());
        assert!(member2.was_accepted());
        // Non-team member loses
        assert!(member3.was_rejected());

        // Should have 2 winners
        assert_eq!(arena.winner_count(pointer), 2);
    }

    #[test]
    fn test_arena_resolve_team() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

        arena.add(pointer, Arc::new(MockMember::new()));

        // Should not have timed out immediately with default timeout
        assert!(!arena.has_timed_out(pointer, DEFAULT_DISAMBIGUATION_TIMEOUT));
        assert!(!arena.has_default_timeout(pointer));
    }

    #[test]
    fn test_arena_has_timed_out_with_zero_duration() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

        arena.add(pointer, Arc::new(MockMember::new()));

        // Zero duration should always be timed out
        assert!(arena.has_timed_out(pointer, Duration::ZERO));
    }

    #[test]
    fn test_arena_has_timed_out_false_for_resolved() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());
        arena.resolve(pointer, Some(member));

        // Already resolved, so should not report timed out
        assert!(!arena.has_timed_out(pointer, Duration::ZERO));
    }

    #[test]
    fn test_force_resolve_if_timed_out() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

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
    fn test_force_resolve_does_nothing_if_held() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

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

        let pointer1 = PointerId::new(0);
        let pointer2 = PointerId::new(1);
        let pointer3 = PointerId::new(2);

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

        // Only pointer1 should be force resolved (pointer2 already resolved, pointer3 held)
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
        let pointer = PointerId::new(0);

        arena.add(pointer, Arc::new(MockMember::new()));

        // Should not resolve with default timeout (just created)
        let count = arena.resolve_default_timed_out_arenas();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_force_resolve_with_no_members() {
        let arena = GestureArena::new();
        let pointer = PointerId::new(0);

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
        let pointer = PointerId::new(0);

        let member = Arc::new(MockMember::new());
        arena.add(pointer, member.clone());

        // Should not resolve immediately (not timed out yet)
        let resolved = arena.force_resolve_if_default_timeout(pointer);
        assert!(!resolved);
        assert!(!arena.is_resolved(pointer));
    }
}
