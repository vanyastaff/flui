//! Offset storage and the `RenderState<P>` offset accessors.
//!
//! This file contains the private `OffsetCell` helper and the `offset()` /
//! `set_offset()` methods on `RenderState<P>`.

use std::cell::Cell;

use flui_foundation::geometry::Offset;

use super::RenderState;
use crate::protocol::Protocol;

// ============================================================================
// OFFSET CELL
// ============================================================================

/// The offset a parent assigns during layout and paint and hit-test read.
///
/// A plain `Cell`: the render tree is `!Send + !Sync` and a pipeline pass
/// touches it from one thread (see `RenderTree`), so the offset needs
/// interior mutability, not atomicity. Two `f64` components do not fit the
/// single `AtomicU64` the earlier `f32` pair was packed into.
#[derive(Debug)]
pub(super) struct OffsetCell {
    value: Cell<Offset>,
}

impl OffsetCell {
    /// Creates the cell with the given initial value.
    #[inline]
    pub(super) const fn new(offset: Offset) -> Self {
        Self {
            value: Cell::new(offset),
        }
    }

    /// Returns the current offset.
    #[inline]
    pub(super) fn load(&self) -> Offset {
        self.value.get()
    }

    /// Replaces the offset.
    #[inline]
    pub(super) fn store(&self, offset: Offset) {
        self.value.set(offset);
    }
}

// ============================================================================
// OFFSET
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the offset relative to parent.
    ///
    /// This is set by the parent during layout and read during paint
    /// and hit testing.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single load
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

    /// Sets the offset relative to parent.
    ///
    /// This is called by the parent during layout to position this
    /// render object.
    ///
    /// # Performance
    ///
    /// - O(1) time
    /// - Single store
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
