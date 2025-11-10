//! Gesture Arena - Conflict resolution between competing gesture recognizers
//!
//! When multiple gesture recognizers compete for the same pointer (e.g., a tap
//! and a drag recognizer both want to handle the same touch), the GestureArena
//! determines which recognizer wins.
//!
//! # Arena Lifecycle
//!
//! ```text
//! 1. Pointer Down → Create arena entry
//! 2. Recognizers add themselves to arena
//! 3. Recognizers compete (accept/reject)
//! 4. Arena resolves winner
//! 5. Winner receives all future events for that pointer
//! ```

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Unique identifier for a pointer device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(pub i32);

impl PointerId {
    /// Create a new pointer ID
    pub fn new(id: i32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn raw(&self) -> i32 {
        self.0
    }
}

/// Gesture disposition - how a recognizer voted in the arena
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureDisposition {
    /// Recognizer wants to handle this gesture
    Accepted,
    /// Recognizer does not want to handle this gesture
    Rejected,
}

/// Trait for objects that can participate in gesture arena
///
/// Implemented by all gesture recognizers.
pub trait GestureArenaMember: Send + Sync {
    /// Accept the gesture for this pointer
    ///
    /// Called when this recognizer wins the arena for the given pointer.
    fn accept_gesture(&self, pointer: PointerId);

    /// Reject the gesture for this pointer
    ///
    /// Called when another recognizer wins the arena, or this recognizer
    /// explicitly rejects the gesture.
    fn reject_gesture(&self, pointer: PointerId);
}

/// Arena entry for a single pointer
///
/// Tracks which recognizers are competing for this pointer.
struct ArenaEntry {
    /// Members competing in this arena
    members: Vec<Arc<dyn GestureArenaMember>>,
    /// Whether this entry is held open (waiting for more information)
    is_held: bool,
    /// Whether arena has been resolved
    is_resolved: bool,
    /// Winner of the arena (if resolved)
    winner: Option<Arc<dyn GestureArenaMember>>,
}

impl std::fmt::Debug for ArenaEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaEntry")
            .field("member_count", &self.members.len())
            .field("is_held", &self.is_held)
            .field("is_resolved", &self.is_resolved)
            .field("has_winner", &self.winner.is_some())
            .finish()
    }
}

impl ArenaEntry {
    fn new() -> Self {
        Self {
            members: Vec::new(),
            is_held: false,
            is_resolved: false,
            winner: None,
        }
    }

    /// Add a member to this arena
    fn add(&mut self, member: Arc<dyn GestureArenaMember>) {
        if !self.is_resolved {
            self.members.push(member);
        }
    }

    /// Hold the arena open (delay resolution)
    fn hold(&mut self) {
        self.is_held = true;
    }

    /// Release the hold on this arena
    fn release(&mut self) {
        self.is_held = false;
    }

    /// Resolve the arena with a winner
    fn resolve(&mut self, winner: Option<Arc<dyn GestureArenaMember>>, pointer: PointerId) {
        if self.is_resolved {
            return;
        }

        self.is_resolved = true;
        self.winner = winner.clone();

        // Notify all members
        for member in &self.members {
            // Check if this member is the winner using Arc::ptr_eq
            let is_winner = winner
                .as_ref()
                .map(|w| Arc::ptr_eq(member, w))
                .unwrap_or(false);

            if is_winner {
                member.accept_gesture(pointer);
            } else {
                member.reject_gesture(pointer);
            }
        }
    }
}

/// The Gesture Arena
///
/// Manages conflict resolution between competing gesture recognizers.
///
/// # Thread Safety
///
/// GestureArena is thread-safe and uses interior mutability (Arc<Mutex>).
///
/// # Example
///
/// ```rust,ignore
/// use flui_gestures::arena::{GestureArena, PointerId};
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
    /// Map from pointer ID to arena entry
    entries: Arc<Mutex<HashMap<PointerId, ArenaEntry>>>,
}

impl GestureArena {
    /// Create a new gesture arena
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a member to the arena for a specific pointer
    ///
    /// Creates a new arena entry if one doesn't exist for this pointer.
    pub fn add(&self, pointer: PointerId, member: Arc<dyn GestureArenaMember>) {
        let mut entries = self.entries.lock();
        let entry = entries.entry(pointer).or_insert_with(ArenaEntry::new);
        entry.add(member);
    }

    /// Close the arena for a pointer (no more members can be added)
    ///
    /// If there's only one member, it wins immediately.
    /// Otherwise, waits for members to accept/reject.
    pub fn close(&self, pointer: PointerId) {
        let mut entries = self.entries.lock();

        if let Some(entry) = entries.get_mut(&pointer) {
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

    /// Hold the arena open for a pointer (delay resolution)
    ///
    /// Used when a recognizer needs more time to decide.
    pub fn hold(&self, pointer: PointerId) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&pointer) {
            entry.hold();
        }
    }

    /// Release the hold on an arena
    ///
    /// If arena was waiting to close, it will close now.
    pub fn release(&self, pointer: PointerId) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&pointer) {
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

    /// Resolve the arena with a specific winner
    ///
    /// Winner receives accept_gesture(), all others receive reject_gesture().
    pub fn resolve(&self, pointer: PointerId, winner: Option<Arc<dyn GestureArenaMember>>) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&pointer) {
            entry.resolve(winner, pointer);
        }
    }

    /// Sweep - remove resolved arenas for a pointer
    ///
    /// Called when pointer is released to clean up.
    pub fn sweep(&self, pointer: PointerId) {
        let mut entries = self.entries.lock();
        entries.remove(&pointer);
    }

    /// Get the number of active arenas
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Check if arena is empty
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Check if an arena exists for a pointer
    pub fn contains(&self, pointer: PointerId) -> bool {
        self.entries.lock().contains_key(&pointer)
    }

    /// Get the winner for a pointer (if resolved)
    pub fn winner(&self, pointer: PointerId) -> Option<Arc<dyn GestureArenaMember>> {
        self.entries
            .lock()
            .get(&pointer)
            .and_then(|entry| entry.winner.clone())
    }

    /// Check if an arena is resolved
    pub fn is_resolved(&self, pointer: PointerId) -> bool {
        self.entries
            .lock()
            .get(&pointer)
            .map(|entry| entry.is_resolved)
            .unwrap_or(false)
    }
}

impl Default for GestureArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entries = self.entries.lock();
        f.debug_struct("GestureArena")
            .field("active_arenas", &entries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock arena member for testing
    struct MockMember {
        accepted: Arc<Mutex<bool>>,
        rejected: Arc<Mutex<bool>>,
    }

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
}
