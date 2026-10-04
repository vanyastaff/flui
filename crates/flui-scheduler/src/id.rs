//! Type-safe identifiers for the scheduler subsystem.
//!
//! This module re-exports foundation ID types (`Id<T>`, markers) and provides
//! `IdGenerator` for atomic auto-increment ID generation.
//!
//! ## Foundation Unification
//!
//! All scheduler ID types (`FrameId`, `TaskId`, `TickerId`, `CallbackId`) are
//! aliases for `flui_foundation::Id<T>` with the corresponding marker from
//! `flui_foundation::markers`. This eliminates the previous parallel
//! `TypedId<M>` system.
//!
//! ## Example
//!
//! ```rust
//! use flui_scheduler::id::IdGenerator;
//! use flui_foundation::markers;
//!
//! // Generate unique IDs using atomic counter
//! let id_gen = IdGenerator::<markers::Frame>::new();
//! let id1 = id_gen.next();
//! let id2 = id_gen.next();
//! assert_ne!(id1, id2);
//! ```

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicUsize, Ordering},
};

// =============================================================================
// Re-exports from flui-foundation
// =============================================================================

pub use flui_foundation::{
    FrameCallbackId, FrameId, Id, Identifier, Marker, TaskId, TickerId, markers,
};

/// UpdateScheduler callback ID - alias for `FrameCallbackId` from foundation.
///
/// Identifies callbacks (transient, persistent, post-frame) in the scheduler.
pub type CallbackId = FrameCallbackId;

// =============================================================================
// ID Generation with Atomic Counters
// =============================================================================

/// ID generator for a specific marker type.
///
/// Produces unique `Id<M>` values via an atomic counter. Useful when you need
/// deterministic ID generation or want to reset counters (e.g., in tests).
///
/// Unlike `Id::new()`/`Id::zip()` which require an explicit index, the
/// generator auto-increments from 1.
///
/// ## Example
///
/// ```rust
/// use flui_scheduler::id::IdGenerator;
/// use flui_foundation::{FrameId, markers};
///
/// let id_gen = IdGenerator::<markers::Frame>::new();
/// let id1: FrameId = id_gen.next();
/// let id2: FrameId = id_gen.next();
/// assert_ne!(id1, id2);
/// assert_eq!(id1.get(), 1);
/// assert_eq!(id2.get(), 2);
///
/// id_gen.reset();
/// let id3: FrameId = id_gen.next();
/// assert_eq!(id3.get(), 1);
/// ```
pub struct IdGenerator<M: Marker> {
    counter: AtomicUsize,
    _marker: PhantomData<M>,
}

// Manual impl instead of `#[derive(Debug)]` so no `M: Debug` bound is
// required — marker types are never instantiated.
impl<M: Marker> std::fmt::Debug for IdGenerator<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdGenerator")
            .field("counter", &self.counter)
            .finish()
    }
}

impl<M: Marker> IdGenerator<M> {
    /// Create a new ID generator starting from 1.
    pub const fn new() -> Self {
        Self {
            counter: AtomicUsize::new(1),
            _marker: PhantomData,
        }
    }

    /// Create a generator starting from a specific value.
    ///
    /// If `start` is 0, it will be set to 1 to ensure non-zero IDs.
    pub fn starting_from(start: usize) -> Self {
        let start = if start == 0 { 1 } else { start };
        Self {
            counter: AtomicUsize::new(start),
            _marker: PhantomData,
        }
    }

    /// Generate the next ID.
    ///
    /// # Panics
    ///
    /// Panics when the next value reaches `usize::MAX`, which is reserved
    /// for exhaustion. Exhaustion remains until an explicit [`Self::reset`].
    pub fn next(&self) -> Id<M> {
        let value = self
            .counter
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("BUG: scheduler ID generator exhausted; refusing to reuse identities");
        Id::zip(value)
    }

    /// Get the current counter value (next ID that will be generated).
    pub fn current(&self) -> usize {
        self.counter.load(Ordering::Relaxed)
    }

    /// Reset the counter to 1.
    pub fn reset(&self) {
        self.counter.store(1, Ordering::Relaxed);
    }
}

impl<M: Marker> Default for IdGenerator<M> {
    fn default() -> Self {
        Self::new()
    }
}
