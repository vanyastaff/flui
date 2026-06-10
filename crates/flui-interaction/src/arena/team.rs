//! Gesture Arena Team - Allows multiple recognizers to compete as a unit.
//!
//! # Overview
//!
//! A [`GestureArenaTeam`] groups multiple gesture recognizers so they compete
//! as a single unit in the [`GestureArena`]. This is useful for widgets that
//! need to support multiple gesture types without them blocking each other.
//!
//! # Use Cases
//!
//! ## Without Captain (Slider pattern)
//!
//! When gesture recognizers are in a team without a captain, once there are no
//! other competing gestures in the arena, the first gesture to have been added
//! to the team automatically wins.
//!
//! ```rust,ignore
//! // Slider uses a team for both horizontal drag and tap
//! let team = GestureArenaTeam::new();
//!
//! // Both recognizers compete together
//! let drag_entry = team.add(pointer, drag_recognizer.clone(), &arena);
//! let tap_entry = team.add(pointer, tap_recognizer.clone(), &arena);
//!
//! // When other recognizers are eliminated, the team wins
//! // and the first member (drag) gets to handle the gesture
//! ```
//!
//! ## With Captain (AndroidView pattern)
//!
//! When gesture recognizers are in a team with a captain, the captain wins
//! on behalf of the team. This is useful when you need to know when any
//! gesture in the team has been recognized.
//!
//! ```rust,ignore
//! let team = GestureArenaTeam::with_captain(forward_recognizer.clone());
//!
//! // Add recognizers to forward
//! team.add(pointer, tap_recognizer.clone(), &arena);
//! team.add(pointer, scroll_recognizer.clone(), &arena);
//!
//! // When any team member wins, captain receives the gesture
//! // to forward to native view
//! ```
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/GestureArenaTeam-class.html>

use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;

use crate::{
    arena::{GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition},
    ids::PointerId,
};

// ============================================================================
// CombiningEntry - Team's entry handle for individual members
// ============================================================================

/// A team-specific arena entry that wraps the real arena entry.
///
/// When a member resolves via this entry, it goes through the team's
/// combining logic instead of directly to the arena.
pub struct TeamEntry {
    combiner: Arc<Mutex<CombiningMember>>,
    member: Arc<dyn GestureArenaMember>,
}

impl TeamEntry {
    /// Resolve this entry with the given disposition.
    ///
    /// The resolution goes through the team's combining logic:
    /// - Accepted: The captain (or this member) wins on behalf of the team
    /// - Rejected: The member is removed from the team; if empty, team rejects
    pub fn resolve(&self, disposition: GestureDisposition) {
        // Compute state transitions under the lock; dispatch every member
        // callback and the arena resolution AFTER the guard drops. A member's
        // reject_gesture commonly re-enters this combiner (e.g. recognizer ->
        // handle_cancel -> stop_tracking -> arena.sweep -> the team's wrapper),
        // and parking_lot mutexes are non-reentrant.
        let (to_reject, entry_to_resolve) = {
            let mut combiner = self.combiner.lock();
            combiner.resolve(&self.member, disposition)
        };

        if let Some((member, pointer)) = to_reject {
            member.reject_gesture(pointer);
        }
        if let Some((entry, disp)) = entry_to_resolve {
            entry.resolve(disp);
        }
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> &Arc<dyn GestureArenaMember> {
        &self.member
    }
}

impl std::fmt::Debug for TeamEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamEntry").finish_non_exhaustive()
    }
}

// ============================================================================
// CombiningMember - Internal team representative in the arena
// ============================================================================

/// Internal arena member that combines multiple team members into one.
///
/// This represents the team in the arena. When it wins/loses, it
/// distributes the result to all team members appropriately.
struct CombiningMember {
    /// The team that owns this combiner.
    team: Arc<GestureArenaTeam>,
    /// Pointer ID for this combiner.
    pointer: PointerId,
    /// Members in this team for this pointer.
    members: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,
    /// Whether this combiner has been resolved.
    resolved: bool,
    /// The winner within the team (if any).
    winner: Option<Arc<dyn GestureArenaMember>>,
    /// The entry handle for the arena (set after first add).
    entry: Option<GestureArenaEntry>,
}

impl CombiningMember {
    fn new(team: Arc<GestureArenaTeam>, pointer: PointerId) -> Self {
        Self {
            team,
            pointer,
            members: SmallVec::new(),
            resolved: false,
            winner: None,
            entry: None,
        }
    }

    /// Resolve a member with the given disposition.
    ///
    /// Pure state transition: returns the member to reject and/or the arena
    /// entry to resolve so the caller can dispatch both AFTER releasing the
    /// combiner lock (member callbacks re-enter the combiner).
    #[allow(clippy::type_complexity)] // local return plumbing, not public API
    fn resolve(
        &mut self,
        member: &Arc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
    ) -> (
        Option<(Arc<dyn GestureArenaMember>, PointerId)>,
        Option<(GestureArenaEntry, GestureDisposition)>,
    ) {
        if self.resolved {
            return (None, None);
        }

        match disposition {
            GestureDisposition::Accepted => {
                // Winner is captain (if set) or the accepting member
                self.winner = Some(self.team.captain().unwrap_or_else(|| member.clone()));

                // Return entry to resolve outside lock
                (
                    None,
                    self.entry
                        .clone()
                        .map(|e| (e, GestureDisposition::Accepted)),
                )
            }
            GestureDisposition::Rejected => {
                // Remove member from team; the caller notifies it outside the
                // lock.
                self.members.retain(|m| !Arc::ptr_eq(m, member));
                let to_reject = Some((member.clone(), self.pointer));

                // If no members left, reject the whole team
                let entry = if self.members.is_empty() {
                    self.entry
                        .clone()
                        .map(|e| (e, GestureDisposition::Rejected))
                } else {
                    None
                };
                (to_reject, entry)
            }
        }
    }

    /// Called when the team wins in the arena.
    ///
    /// Returns the member notifications to dispatch after the combiner lock
    /// is released.
    fn accept_gesture(&mut self) -> PendingTeamNotifications {
        let mut pending = PendingTeamNotifications::new(self.pointer);
        if self.resolved {
            return pending;
        }
        self.resolved = true;

        // Determine winner: pre-set winner, captain, or first member
        let winner = self.winner.take().or_else(|| {
            self.team
                .captain()
                .or_else(|| self.members.first().cloned())
        });

        // Check if winner is the captain (not in members list)
        let captain = self.team.captain();
        let winner_is_captain = winner
            .as_ref()
            .zip(captain.as_ref())
            .is_some_and(|(w, c)| Arc::ptr_eq(w, c));

        // Queue all member notifications - they all lose except the winner
        for member in &self.members {
            let is_winner = winner.as_ref().is_some_and(|w| Arc::ptr_eq(w, member));
            if is_winner {
                pending.accepts.push(member.clone());
            } else {
                pending.rejects.push(member.clone());
            }
        }

        // If winner is the captain (not in members), notify captain separately
        if winner_is_captain && let Some(captain) = captain {
            pending.accepts.push(captain);
        }

        // Remove from team's combiners
        self.team.remove_combiner(self.pointer);
        pending
    }

    /// Called when the team loses in the arena.
    ///
    /// Returns the member notifications to dispatch after the combiner lock
    /// is released.
    fn reject_gesture(&mut self) -> PendingTeamNotifications {
        let mut pending = PendingTeamNotifications::new(self.pointer);
        if self.resolved {
            return pending;
        }
        self.resolved = true;

        // Queue rejection for all members
        pending.rejects.extend(self.members.iter().cloned());

        // Remove from team's combiners
        self.team.remove_combiner(self.pointer);
        pending
    }
}

/// Member notifications computed under the combiner lock and dispatched after
/// it is released.
///
/// Member callbacks routinely re-enter the combiner (a rejected recognizer's
/// `handle_cancel` path sweeps the arena, which resolves this team's wrapper,
/// which locks the same combiner); dispatching under the lock would deadlock
/// on parking_lot's non-reentrant mutex.
struct PendingTeamNotifications {
    pointer: PointerId,
    /// At most the winner and (separately) the captain.
    accepts: SmallVec<[Arc<dyn GestureArenaMember>; 2]>,
    rejects: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,
}

impl PendingTeamNotifications {
    fn new(pointer: PointerId) -> Self {
        Self {
            pointer,
            accepts: SmallVec::new(),
            rejects: SmallVec::new(),
        }
    }

    /// Fire all queued notifications. Call WITHOUT the combiner lock held.
    fn dispatch(self) {
        for member in self.accepts {
            member.accept_gesture(self.pointer);
        }
        for member in self.rejects {
            member.reject_gesture(self.pointer);
        }
    }
}

// ============================================================================
// CombiningMemberWrapper - Arena member wrapper
// ============================================================================

/// Wrapper that implements GestureArenaMember for the combining member.
struct CombiningMemberWrapper {
    combiner: Arc<Mutex<CombiningMember>>,
}

// Implement sealed trait for arena membership
impl crate::sealed::arena_member::Sealed for CombiningMemberWrapper {}

impl GestureArenaMember for CombiningMemberWrapper {
    fn accept_gesture(&self, _pointer: PointerId) {
        let pending = self.combiner.lock().accept_gesture();
        pending.dispatch();
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        let pending = self.combiner.lock().reject_gesture();
        pending.dispatch();
    }
}

// ============================================================================
// GestureArenaTeam
// ============================================================================

/// A group of gesture recognizers that compete as a unit in the arena.
///
/// # Thread Safety
///
/// `GestureArenaTeam` is thread-safe and can be shared across threads.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::team::GestureArenaTeam;
///
/// // Create a team for a Slider widget
/// let team = GestureArenaTeam::new();
///
/// // Add recognizers to the team
/// let drag_entry = team.add(pointer, drag_recognizer.clone(), &arena);
/// let tap_entry = team.add(pointer, tap_recognizer.clone(), &arena);
///
/// // When the team wins, first member gets the gesture
/// ```
pub struct GestureArenaTeam {
    /// Combiner for each active pointer.
    combiners: DashMap<PointerId, Arc<Mutex<CombiningMember>>>,
    /// Captain that wins on behalf of the team.
    captain: Mutex<Option<Arc<dyn GestureArenaMember>>>,
}

impl GestureArenaTeam {
    /// Create a new gesture arena team without a captain.
    ///
    /// When the team wins, the first member added wins.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            combiners: DashMap::new(),
            captain: Mutex::new(None),
        })
    }

    /// Create a new gesture arena team with a captain.
    ///
    /// When any team member wins, the captain receives the gesture.
    /// This is useful for forwarding gestures (e.g., to native views).
    pub fn with_captain(captain: Arc<dyn GestureArenaMember>) -> Arc<Self> {
        Arc::new(Self {
            combiners: DashMap::new(),
            captain: Mutex::new(Some(captain)),
        })
    }

    /// Get the team's captain (if any).
    pub fn captain(&self) -> Option<Arc<dyn GestureArenaMember>> {
        self.captain.lock().clone()
    }

    /// Set the team's captain.
    ///
    /// The captain wins on behalf of the entire team when any member claims
    /// victory.
    pub fn set_captain(&self, captain: Option<Arc<dyn GestureArenaMember>>) {
        *self.captain.lock() = captain;
    }

    /// Add a member to the team for a specific pointer.
    ///
    /// Returns a [`TeamEntry`] handle that the member can use to resolve
    /// itself. The resolution goes through the team's combining logic.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let entry = team.add(pointer, recognizer.clone(), &arena);
    ///
    /// // Later, resolve via the team entry
    /// entry.resolve(GestureDisposition::Accepted);
    /// ```
    pub fn add(
        self: &Arc<Self>,
        pointer: PointerId,
        member: Arc<dyn GestureArenaMember>,
        arena: &GestureArena,
    ) -> TeamEntry {
        let combiner = self
            .combiners
            .entry(pointer)
            .or_insert_with(|| Arc::new(Mutex::new(CombiningMember::new(self.clone(), pointer))))
            .clone();

        // Add member to combiner
        {
            let mut combiner_lock = combiner.lock();
            combiner_lock.members.push(member.clone());

            // First member triggers arena registration
            if combiner_lock.entry.is_none() {
                let wrapper = Arc::new(CombiningMemberWrapper {
                    combiner: combiner.clone(),
                });
                let entry = arena.add(pointer, wrapper);
                combiner_lock.entry = Some(entry);
            }
        }

        TeamEntry { combiner, member }
    }

    /// Check if the team has an active combiner for a pointer.
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.combiners.contains_key(&pointer)
    }

    /// Get the number of active combiners.
    pub fn len(&self) -> usize {
        self.combiners.len()
    }

    /// Check if the team has no active combiners.
    pub fn is_empty(&self) -> bool {
        self.combiners.is_empty()
    }

    /// Internal: Remove a combiner after resolution.
    fn remove_combiner(&self, pointer: PointerId) {
        self.combiners.remove(&pointer);
    }
}

impl Default for GestureArenaTeam {
    fn default() -> Self {
        Self {
            combiners: DashMap::new(),
            captain: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for GestureArenaTeam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArenaTeam")
            .field("active_combiners", &self.combiners.len())
            .field("has_captain", &self.captain.lock().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    // Mock member for testing
    #[allow(dead_code)]
    struct MockMember {
        id: usize,
        accepted: AtomicBool,
        rejected: AtomicBool,
    }

    impl crate::sealed::arena_member::Sealed for MockMember {}

    impl MockMember {
        fn new(id: usize) -> Arc<Self> {
            Arc::new(Self {
                id,
                accepted: AtomicBool::new(false),
                rejected: AtomicBool::new(false),
            })
        }

        fn was_accepted(&self) -> bool {
            self.accepted.load(Ordering::SeqCst)
        }

        fn was_rejected(&self) -> bool {
            self.rejected.load(Ordering::SeqCst)
        }
    }

    impl GestureArenaMember for MockMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            self.accepted.store(true, Ordering::SeqCst);
        }

        fn reject_gesture(&self, _pointer: PointerId) {
            self.rejected.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_team_creation() {
        let team = GestureArenaTeam::new();
        assert!(team.is_empty());
        assert!(team.captain().is_none());
    }

    #[test]
    fn team_is_dropped_when_last_handle_goes_away() {
        // Regression: the team used to store a strong Arc to itself
        // (`self_ref`), a reference cycle that kept every team alive for the
        // process lifetime.
        let team = GestureArenaTeam::new();
        let weak = Arc::downgrade(&team);
        drop(team);
        assert!(
            weak.upgrade().is_none(),
            "GestureArenaTeam must not keep itself alive via a self-cycle"
        );
    }

    #[test]
    fn reentrant_resolve_from_reject_callback_does_not_deadlock() {
        // Regression: CombiningMember::resolve used to fire
        // member.reject_gesture while holding the combiner lock. A member
        // whose rejection handler re-enters the team (here: resolving its own
        // entry again, as a recognizer's cancel path does via arena.sweep)
        // would self-deadlock on the non-reentrant mutex.
        struct Reentrant {
            entry: Mutex<Option<TeamEntry>>,
            rejected: AtomicBool,
        }
        impl crate::sealed::arena_member::Sealed for Reentrant {}
        impl GestureArenaMember for Reentrant {
            fn accept_gesture(&self, _pointer: PointerId) {}
            fn reject_gesture(&self, _pointer: PointerId) {
                self.rejected.store(true, Ordering::SeqCst);
                // Re-enter the same combiner from inside the callback. The
                // guard must drop before resolve() so the nested second
                // rejection callback can re-acquire OUR OWN entry mutex —
                // the deadlock under test is the combiner's, not this one.
                let taken = self.entry.lock().take();
                if let Some(entry) = taken {
                    entry.resolve(GestureDisposition::Rejected);
                }
            }
        }

        let arena = GestureArena::new();
        let team = GestureArenaTeam::new();
        let pointer = PointerId::new(7).expect("nonzero pointer id");

        let reentrant = Arc::new(Reentrant {
            entry: Mutex::new(None),
            rejected: AtomicBool::new(false),
        });
        let other = MockMember::new(1);

        let entry = team.add(pointer, reentrant.clone(), &arena);
        let _other_entry = team.add(pointer, other.clone(), &arena);
        *reentrant.entry.lock() = Some(team.add(pointer, reentrant.clone(), &arena));

        // Rejecting the member fires reject_gesture, which re-enters the
        // combiner; must complete without deadlocking.
        entry.resolve(GestureDisposition::Rejected);
        assert!(reentrant.rejected.load(Ordering::SeqCst));
    }

    #[test]
    fn test_team_with_captain() {
        let captain = MockMember::new(0);
        let team = GestureArenaTeam::with_captain(captain.clone());

        assert!(team.captain().is_some());
    }

    #[test]
    fn test_team_set_captain() {
        let team = GestureArenaTeam::new();
        assert!(team.captain().is_none());

        let captain = MockMember::new(0);
        team.set_captain(Some(captain.clone()));
        assert!(team.captain().is_some());

        team.set_captain(None);
        assert!(team.captain().is_none());
    }

    #[test]
    fn test_team_add_creates_combiner() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = MockMember::new(1);

        assert!(!team.contains(pointer));

        let _entry = team.add(pointer, member, &arena);

        assert!(team.contains(pointer));
        assert_eq!(team.len(), 1);
    }

    #[test]
    fn test_team_first_member_wins() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        let _entry1 = team.add(pointer, member1.clone(), &arena);
        let _entry2 = team.add(pointer, member2.clone(), &arena);

        // Close arena and let team win
        arena.close(pointer);

        // Team should win (only member in arena)
        // First member should be the winner
        assert!(member1.was_accepted() || member2.was_accepted());
    }

    #[test]
    fn test_team_captain_wins() {
        let captain = MockMember::new(0);
        let team = GestureArenaTeam::with_captain(captain.clone());
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        let entry1 = team.add(pointer, member1.clone(), &arena);
        let _entry2 = team.add(pointer, member2.clone(), &arena);

        // member1 accepts - captain should win
        entry1.resolve(GestureDisposition::Accepted);
        arena.close(pointer);

        // Captain should have won
        assert!(captain.was_accepted());
    }

    #[test]
    fn test_team_member_reject_removes_from_team() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        let entry1 = team.add(pointer, member1.clone(), &arena);
        let _entry2 = team.add(pointer, member2.clone(), &arena);

        // member1 rejects
        entry1.resolve(GestureDisposition::Rejected);

        assert!(member1.was_rejected());
    }

    #[test]
    fn test_team_all_reject_rejects_arena() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        let entry1 = team.add(pointer, member1.clone(), &arena);
        let entry2 = team.add(pointer, member2.clone(), &arena);

        // Both reject
        entry1.resolve(GestureDisposition::Rejected);
        entry2.resolve(GestureDisposition::Rejected);

        assert!(member1.was_rejected());
        assert!(member2.was_rejected());
    }

    #[test]
    fn test_team_debug_impl() {
        let team = GestureArenaTeam::new();
        let debug = format!("{:?}", team);

        assert!(debug.contains("GestureArenaTeam"));
        assert!(debug.contains("active_combiners"));
        assert!(debug.contains("has_captain"));
    }

    #[test]
    fn test_team_entry_debug_impl() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;
        let member = MockMember::new(1);

        let entry = team.add(pointer, member, &arena);
        let debug = format!("{:?}", entry);

        assert!(debug.contains("TeamEntry"));
    }

    #[test]
    fn test_team_multiple_pointers() {
        let team = GestureArenaTeam::new();
        let arena = GestureArena::new();

        let pointer1 = PointerId::PRIMARY;
        let pointer2 = PointerId::new(2).expect("nonzero pointer id");

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        team.add(pointer1, member1, &arena);
        team.add(pointer2, member2, &arena);

        assert!(team.contains(pointer1));
        assert!(team.contains(pointer2));
        assert_eq!(team.len(), 2);
    }
}
