//! Atomic offset storage and `RenderState<P>` offset accessors.
//!
//! This file contains the private `AtomicOffset` helper (lock-free f32 pair
//! packed into an `AtomicU64`) and the `offset()` / `set_offset()` methods
//! on `RenderState<P>`.

use std::sync::atomic::{AtomicU64, Ordering};

use flui_types::Offset;

use super::RenderState;
use crate::protocol::Protocol;

// ============================================================================
// ATOMIC OFFSET
// ============================================================================

/// Thread-safe offset storage using atomic operations.
///
/// Stores two f32 values in a single AtomicU64 for lock-free updates.
/// This is safe because we treat the bits as opaque data and use atomic
/// operations to ensure consistency.
#[derive(Debug)]
pub(super) struct AtomicOffset {
    bits: AtomicU64,
}

impl AtomicOffset {
    /// Creates a new atomic offset with the given initial value.
    #[inline]
    pub(super) const fn new(offset: Offset) -> Self {
        // Pack two f32s into a u64
        // Use .0.to_bits() instead of .to_bits() because Pixels::to_bits()
        // is not available in const context.
        let dx_bits = offset.dx.0.to_bits() as u64;
        let dy_bits = offset.dy.0.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        Self {
            bits: AtomicU64::new(packed),
        }
    }

    /// Loads the current offset atomically.
    #[inline]
    pub(super) fn load(&self) -> Offset {
        let packed = self.bits.load(Ordering::Acquire);
        let dx_bits = (packed & 0xFFFF_FFFF) as u32;
        let dy_bits = (packed >> 32) as u32;

        Offset {
            dx: flui_types::Pixels(f32::from_bits(dx_bits)),
            dy: flui_types::Pixels(f32::from_bits(dy_bits)),
        }
    }

    /// Stores a new offset atomically.
    #[inline]
    pub(super) fn store(&self, offset: Offset) {
        let dx_bits = offset.dx.0.to_bits() as u64;
        let dy_bits = offset.dy.0.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        self.bits.store(packed, Ordering::Release);
    }
}

// ============================================================================
// OFFSET (ATOMIC, LOCK-FREE)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the offset relative to parent (atomic, lock-free).
    ///
    /// This is set by the parent during layout and read during paint
    /// and hit testing.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single atomic load
    /// - No allocation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let screen_position = parent_offset + state.offset();
    /// ```
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset.load()
    }

    /// Sets the offset relative to parent (atomic, lock-free).
    ///
    /// This is called by the parent during layout to position this
    /// render object. Uses atomic operations for lock-free updates.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single atomic store
    /// - No allocation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Parent positioning child during layout
    /// child_state.set_offset(Offset::new(10.0, 20.0));
    /// ```
    #[inline]
    pub fn set_offset(&self, offset: Offset) {
        self.offset.store(offset);
    }

    /// This node's current layout generation — bumped once per real layout of
    /// this node, and stamped onto the children it lays out.
    #[inline]
    pub fn layout_generation(&self) -> u64 {
        self.layout_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Advance to a fresh generation and return it.
    ///
    /// Called at the layout **commit**, in the same block that stamps the
    /// children — deliberately not at layout entry. Splitting the two lets any
    /// early return between them (a protocol error, a poisoned descendant)
    /// advance the parent while stamping nobody, which silently unplaces every
    /// child and makes the whole subtree stop painting. Advancing here means
    /// the parent's value and its children's stamps are always written
    /// together or not at all.
    ///
    /// Wrapping is deliberate and harmless: the comparison is equality against
    /// the parent's *current* value, so a wrap would have to coincide with a
    /// child untouched for exactly 2^64 of its parent's layouts.
    #[inline]
    pub fn advance_layout_generation(&self) -> u64 {
        let next = self
            .layout_generation
            .load(std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        self.layout_generation
            .store(next, std::sync::atomic::Ordering::Relaxed);
        next
    }

    /// Record that the parent identified by `parent` laid this node out as one
    /// of its children during its `parent_generation` pass.
    ///
    /// Both halves are stored because the counter is per-parent: the same
    /// number is issued by every parent that has laid out the same number of
    /// times, so a child reparented between two of them would match on the
    /// number alone.
    #[inline]
    pub fn set_placed_by(&self, parent: flui_foundation::RenderId, parent_generation: u64) {
        self.placed_generation
            .store(parent_generation, std::sync::atomic::Ordering::Relaxed);
        self.placed_by
            .store(parent.as_u64(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether this node was laid out as a child during the parent's current
    /// pass.
    ///
    /// `false` for a child a multi-child object skipped — a lazy sliver's
    /// out-of-band item, an indexed stack's hidden pages once they stop being
    /// laid out — so paint and hit-test can leave it alone rather than reading
    /// an offset from a pass that no longer describes the tree.
    ///
    /// The parent's identity is part of the comparison, not just its counter:
    /// a child reparented by a `GlobalKey` relocation carries a number its old
    /// parent issued, and a new parent reaching that same number would
    /// otherwise accept it without ever having laid it out.
    ///
    /// A node stamped by nobody counts as placed. Absent evidence the gate
    /// must not remove anything — see `RenderState::placed_generation`.
    #[inline]
    pub fn was_placed_by(&self, parent: flui_foundation::RenderId, parent_generation: u64) -> bool {
        let stamped_by = self.placed_by.load(std::sync::atomic::Ordering::Relaxed);
        if stamped_by == 0 {
            return true;
        }
        stamped_by == parent.as_u64()
            && self
                .placed_generation
                .load(std::sync::atomic::Ordering::Relaxed)
                == parent_generation
    }
}
