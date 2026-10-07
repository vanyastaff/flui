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

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
};

use dashmap::DashMap;
use parking_lot::Mutex;
use smallvec::SmallVec;

use crate::{
    arena::{
        GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition,
        PendingNotifications,
    },
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
    combiner: Rc<Mutex<CombiningMember>>,
    member: Rc<dyn GestureArenaMember>,
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

        // A panicking rejection must not leave the team's arena entry
        // unresolved: both steps run, and the first failure resumes after.
        let mut first_panic = to_reject.and_then(|(member, pointer)| {
            catch_unwind(AssertUnwindSafe(|| member.reject_gesture(pointer))).err()
        });
        if let Some((entry, disp)) = entry_to_resolve {
            let pointer = entry.pointer();
            let candidate = catch_unwind(AssertUnwindSafe(|| entry.resolve(disp))).err();
            GestureArena::preserve_first_panic(&mut first_panic, candidate, pointer);
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Get the member for this entry.
    #[inline]
    pub fn member(&self) -> &Rc<dyn GestureArenaMember> {
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
    team: Weak<GestureArenaTeam>,
    /// Pointer ID for this combiner.
    pointer: PointerId,
    /// Members in this team for this pointer.
    members: SmallVec<[Weak<dyn GestureArenaMember>; 4]>,
    /// Whether this combiner has been resolved.
    resolved: bool,
    /// The winner within the team (if any).
    winner: Option<Weak<dyn GestureArenaMember>>,
    /// The entry handle for the arena (set after first add).
    entry: Option<GestureArenaEntry>,
    arena_member: Option<Rc<CombiningMemberWrapper>>,
}

impl CombiningMember {
    fn new(team: Rc<GestureArenaTeam>, pointer: PointerId) -> Self {
        Self {
            team: Rc::downgrade(&team),
            pointer,
            members: SmallVec::new(),
            resolved: false,
            winner: None,
            entry: None,
            arena_member: None,
        }
    }

    /// Resolve a member with the given disposition.
    ///
    /// Pure state transition: returns the member to reject and/or the arena
    /// entry to resolve so the caller can dispatch both AFTER releasing the
    /// combiner lock (member callbacks re-enter the combiner).
    #[expect(clippy::type_complexity)] // local return plumbing, not public API
    fn resolve(
        &mut self,
        member: &Rc<dyn GestureArenaMember>,
        disposition: GestureDisposition,
    ) -> (
        Option<(Rc<dyn GestureArenaMember>, PointerId)>,
        Option<(GestureArenaEntry, GestureDisposition)>,
    ) {
        if self.resolved {
            return (None, None);
        }

        match disposition {
            GestureDisposition::Accepted => {
                // Winner is captain (if set) or the accepting member
                let winner = self
                    .team
                    .upgrade()
                    .and_then(|team| team.captain())
                    .unwrap_or_else(|| member.clone());
                self.winner = Some(Rc::downgrade(&winner));

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
                self.members
                    .retain(|m| !Weak::ptr_eq(m, &Rc::downgrade(member)) && m.strong_count() != 0);
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
        let team = self.team.upgrade();
        let captain = team.as_ref().and_then(|team| team.captain());
        let winner = self
            .winner
            .take()
            .and_then(|winner| winner.upgrade())
            .or_else(|| captain.clone())
            .or_else(|| self.members.iter().find_map(Weak::upgrade));

        // Check if winner is the captain (not in members list)
        let winner_is_captain = winner
            .as_ref()
            .zip(captain.as_ref())
            .is_some_and(|(w, c)| Rc::ptr_eq(w, c));

        // Queue all member notifications - they all lose except the winner
        for member in &self.members {
            let is_winner = winner
                .as_ref()
                .is_some_and(|winner| Weak::ptr_eq(&Rc::downgrade(winner), member));
            if is_winner {
                pending.accepts.push(member.clone());
            } else {
                pending.rejects.push(member.clone());
            }
        }

        // If winner is the captain (not in members), notify captain separately
        if winner_is_captain && let Some(captain) = captain {
            pending.accepts.push(Rc::downgrade(&captain));
        }

        // Remove from team's combiners
        if let Some(team) = team {
            team.remove_combiner(self.pointer);
        }
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
        if let Some(team) = self.team.upgrade() {
            team.remove_combiner(self.pointer);
        }
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
    accepts: SmallVec<[Weak<dyn GestureArenaMember>; 2]>,
    rejects: SmallVec<[Weak<dyn GestureArenaMember>; 4]>,
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
    ///
    /// Every surviving member is notified even when an earlier callback
    /// panics; the first panic resumes after delivery, as in the arena itself.
    fn dispatch(self) {
        let pending: PendingNotifications = self
            .accepts
            .into_iter()
            .map(|member| (member, GestureDisposition::Accepted))
            .chain(
                self.rejects
                    .into_iter()
                    .map(|member| (member, GestureDisposition::Rejected)),
            )
            .collect();
        GestureArena::dispatch_pending(pending, self.pointer);
    }
}

// ============================================================================
// CombiningMemberWrapper - Arena member wrapper
// ============================================================================

/// Wrapper that implements GestureArenaMember for the combining member.
struct CombiningMemberWrapper {
    combiner: Weak<Mutex<CombiningMember>>,
}

impl GestureArenaMember for CombiningMemberWrapper {
    fn accept_gesture(&self, _pointer: PointerId) {
        let Some(combiner) = self.combiner.upgrade() else {
            return;
        };
        let pending = combiner.lock().accept_gesture();
        pending.dispatch();
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        let Some(combiner) = self.combiner.upgrade() else {
            return;
        };
        let pending = combiner.lock().reject_gesture();
        pending.dispatch();
    }
}

// ============================================================================
// GestureArenaTeam
// ============================================================================

/// A group of gesture recognizers that compete as a unit in the arena.
///
/// # Ownership
///
/// Teams belong to the same UI-owner gesture lane as their arena members.
/// Internal locks protect re-entrant state transitions; they do not make the
/// executable team graph a cross-thread value.
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
    combiners: DashMap<PointerId, Rc<Mutex<CombiningMember>>>,
    /// Captain that wins on behalf of the team.
    captain: Mutex<Option<Rc<dyn GestureArenaMember>>>,
}

impl GestureArenaTeam {
    /// Create a new gesture arena team without a captain.
    ///
    /// When the team wins, the first member added wins.
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            combiners: DashMap::new(),
            captain: Mutex::new(None),
        })
    }

    /// Create a new gesture arena team with a captain.
    ///
    /// When any team member wins, the captain receives the gesture.
    /// This is useful for forwarding gestures (e.g., to native views).
    pub fn with_captain(captain: Rc<dyn GestureArenaMember>) -> Rc<Self> {
        Rc::new(Self {
            combiners: DashMap::new(),
            captain: Mutex::new(Some(captain)),
        })
    }

    /// Get the team's captain (if any).
    pub fn captain(&self) -> Option<Rc<dyn GestureArenaMember>> {
        self.captain.lock().clone()
    }

    /// Set the team's captain.
    ///
    /// The captain wins on behalf of the entire team when any member claims
    /// victory.
    pub fn set_captain(&self, captain: Option<Rc<dyn GestureArenaMember>>) {
        let _prev = std::mem::replace(&mut *self.captain.lock(), captain);
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
        self: &Rc<Self>,
        pointer: PointerId,
        member: Rc<dyn GestureArenaMember>,
        arena: &GestureArena,
    ) -> TeamEntry {
        let combiner = self
            .combiners
            .entry(pointer)
            .or_insert_with(|| Rc::new(Mutex::new(CombiningMember::new(self.clone(), pointer))))
            .clone();

        // Add member to combiner
        {
            let mut combiner_lock = combiner.lock();
            combiner_lock.members.push(Rc::downgrade(&member));

            // First member triggers arena registration
            if combiner_lock.entry.is_none() {
                let wrapper = Rc::new(CombiningMemberWrapper {
                    combiner: Rc::downgrade(&combiner),
                });
                let entry = arena.add(pointer, &wrapper);
                combiner_lock.entry = Some(entry);
                combiner_lock.arena_member = Some(wrapper);
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
    #[expect(dead_code)]
    struct MockMember {
        id: usize,
        accepted: AtomicBool,
        rejected: AtomicBool,
    }

    impl MockMember {
        fn new(id: usize) -> Rc<Self> {
            Rc::new(Self {
                id,
                accepted: AtomicBool::new(false),
                rejected: AtomicBool::new(false),
            })
        }

        fn was_accepted(&self) -> bool {
            self.accepted.load(Ordering::SeqCst)
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
    fn test_team_captain_wins() {
        let captain = MockMember::new(0);
        let team = GestureArenaTeam::with_captain(captain.clone());
        let arena = GestureArena::new();
        let pointer = PointerId::PRIMARY;

        let member1 = MockMember::new(1);
        let member2 = MockMember::new(2);

        let entry1 = team.add(pointer, member1, &arena);
        let _entry2 = team.add(pointer, member2, &arena);

        // member1 accepts - captain should win
        entry1.resolve(GestureDisposition::Accepted);
        arena.close(pointer);
        arena.drain_deferred_resolutions();

        // Captain should have won
        assert!(captain.was_accepted());
    }
}
