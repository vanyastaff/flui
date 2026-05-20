//! Type-safe identifiers for the scheduler subsystem.
//!
//! This module re-exports foundation ID types (`Id<T>`, markers) and provides
//! scheduler-specific utilities: `IdGenerator` for atomic auto-increment ID
//! generation, and `Handle` for generation-counted slot-map references.
//!
//! ## Foundation Unification
//!
//! All scheduler ID types (`FrameId`, `TaskId`, `TickerId`, `CallbackId`) are
//! now aliases for `flui_foundation::Id<T>` with the corresponding marker from
//! `flui_foundation::markers`. This eliminates the previous parallel
//! `TypedId<M>` system.
//!
//! ## Example
//!
//! ```rust
//! use flui_scheduler::id::{IdGenerator, Handle};
//! use flui_foundation::markers;
//!
//! // Generate unique IDs using atomic counter
//! let id_gen = IdGenerator::<markers::Frame>::new();
//! let id1 = id_gen.next();
//! let id2 = id_gen.next();
//! assert_ne!(id1, id2);
//!
//! // Handles for ABA-safe slot-map references
//! let handle = Handle::<markers::Frame>::new(42, 1);
//! assert_eq!(handle.index(), 42);
//! ```

use std::{
    fmt,
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
// Handle Pattern (ID + Generation for ABA problem prevention)
// =============================================================================

/// A handle that includes a generation number.
///
/// Useful for detecting stale references in slot-map style data structures.
/// The generation is incremented each time a slot is reused.
pub struct Handle<M: Marker> {
    index: u32,
    generation: u32,
    _marker: PhantomData<M>,
}

// Manual trait implementations to avoid requiring M: Copy/Clone/etc bounds
// (foundation markers are empty enums that cannot implement Copy/Clone)

impl<M: Marker> Copy for Handle<M> {}

impl<M: Marker> Clone for Handle<M> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<M: Marker> PartialEq for Handle<M> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<M: Marker> Eq for Handle<M> {}

impl<M: Marker> std::hash::Hash for Handle<M> {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<M: Marker> Handle<M> {
    /// Create a new handle.
    pub const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _marker: PhantomData,
        }
    }

    /// Get the index.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Get the generation.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Create a handle with incremented generation (for slot reuse).
    #[inline]
    pub const fn next_generation(self) -> Self {
        Self {
            index: self.index,
            generation: self.generation.wrapping_add(1),
            _marker: PhantomData,
        }
    }

    /// Pack into a single u64 for efficient storage.
    #[inline]
    pub const fn pack(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }

    /// Unpack from a u64.
    #[inline]
    pub const fn unpack(packed: u64) -> Self {
        Self {
            index: packed as u32,
            generation: (packed >> 32) as u32,
            _marker: PhantomData,
        }
    }
}

impl<M: Marker> fmt::Debug for Handle<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<M>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(
            f,
            "{}Handle({}, gen={})",
            marker_name, self.index, self.generation
        )
    }
}

impl<M: Marker> fmt::Display for Handle<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<M>();
        let marker_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(f, "{}[{}:{}]", marker_name, self.index, self.generation)
    }
}

/// Type-safe frame handle.
pub type FrameHandle = Handle<markers::Frame>;

/// Type-safe task handle.
pub type TaskHandle = Handle<markers::Task>;

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

    #[test]
    fn test_handle() {
        let handle = FrameHandle::new(42, 1);

        assert_eq!(handle.index(), 42);
        assert_eq!(handle.generation(), 1);

        let next = handle.next_generation();
        assert_eq!(next.index(), 42);
        assert_eq!(next.generation(), 2);
    }

    #[test]
    fn test_handle_pack_unpack() {
        let original = TaskHandle::new(12345, 67890);
        let packed = original.pack();
        let unpacked = TaskHandle::unpack(packed);

        assert_eq!(original.index(), unpacked.index());
        assert_eq!(original.generation(), unpacked.generation());
    }
}
