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
    FrameCallbackId, FrameId, Id, Identifier, Index, Marker, RawId, TaskId, TickerId, markers,
};

/// Scheduler callback ID - alias for `FrameCallbackId` from foundation.
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
    /// Panics if the counter overflows (after `usize::MAX - 1` IDs).
    pub fn next(&self) -> Id<M> {
        let value = self.counter.fetch_add(1, Ordering::Relaxed);
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_uniqueness_via_generator() {
        let id_gen = IdGenerator::<markers::Frame>::new();
        let id1 = id_gen.next();
        let id2 = id_gen.next();

        assert_ne!(id1, id2);
        assert!(id2.get() > id1.get());
    }

    #[test]
    fn test_id_type_safety() {
        // These are different types - can't be mixed!
        let _frame_id: FrameId = Id::zip(1);
        let _task_id: TaskId = Id::zip(2);

        assert_ne!(
            std::any::TypeId::of::<FrameId>(),
            std::any::TypeId::of::<TaskId>()
        );
    }

    #[test]
    fn test_id_display() {
        let frame_id = FrameId::zip(42);
        let display = format!("{}", frame_id);
        assert!(display.contains("Frame"));
        assert!(display.contains("42"));
    }

    #[test]
    fn test_option_niche_optimization() {
        use std::mem::size_of;

        // NonZeroUsize enables niche optimization
        assert_eq!(size_of::<FrameId>(), size_of::<usize>());
        assert_eq!(size_of::<Option<FrameId>>(), size_of::<usize>());
    }

    #[test]
    fn test_id_generator() {
        let generator = IdGenerator::<markers::Frame>::new();

        let id1 = generator.next();
        let id2 = generator.next();
        let id3 = generator.next();

        assert_eq!(id1.get(), 1);
        assert_eq!(id2.get(), 2);
        assert_eq!(id3.get(), 3);

        generator.reset();
        let id4 = generator.next();
        assert_eq!(id4.get(), 1);
    }
}
