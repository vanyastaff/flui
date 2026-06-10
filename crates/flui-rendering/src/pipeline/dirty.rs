//! Co-located dirty sets for the pipeline owner.
//!
//! `PipelineOwner` used to carry four parallel `Vec<DirtyNode>` fields,
//! scattered across the struct between unrelated bookkeeping. Mythos Step 2
//! (2026-05-20) consolidates them into a single [`DirtySets`] struct so the
//! one cache line of four `Vec` pointers lives together, the names line up,
//! and "what dirty work is pending" reads as one concept rather than four
//! adjacent ones.
//!
//! Each phase's vector has a stable sort discipline applied at flush time:
//!
//! - **Layout / compositing-bits / semantics** sort shallow-first (root
//!   toward leaves) so ancestor layout can stamp constraints before
//!   descendants are visited.
//! - **Paint** sorts deep-first so leaves emit their layers before
//!   ancestor compositing decisions are taken.
//!
//! See `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md`
//! Section 6 for the broader rationale.

use flui_foundation::RenderId;

// ============================================================================
// DirtyNode
// ============================================================================

/// A node that needs processing in one of the pipeline phases.
///
/// Stores both the node's `RenderId` (1-based) and its depth in the tree
/// for efficient sorting. The `id` field is typed as `RenderId` to enforce
/// the ID offset convention at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyNode {
    /// The render object identifier (1-based `RenderId`).
    pub id: RenderId,
    /// The depth of the node in the render tree (root = 0).
    pub depth: usize,
}

impl DirtyNode {
    /// Creates a new dirty node entry.
    #[inline]
    pub fn new(id: RenderId, depth: usize) -> Self {
        Self { id, depth }
    }
}

// ============================================================================
// DirtySets
// ============================================================================

/// Co-located dirty sets for the four pipeline phases that produce them.
///
/// Each `Vec<DirtyNode>` is appended-to when a node is marked dirty for the
/// corresponding phase, then drained-and-sorted at the start of that phase's
/// flush. The vectors are non-shrinking on purpose -- once they grow to the
/// peak working set, they keep that capacity for the next frame.
#[derive(Debug, Default)]
pub struct DirtySets {
    /// Nodes needing layout (sorted shallow-first during flush).
    pub needs_layout: Vec<DirtyNode>,

    /// Nodes needing compositing-bits update (sorted shallow-first during flush).
    pub needs_compositing: Vec<DirtyNode>,

    /// Nodes needing paint (sorted deep-first during flush).
    pub needs_paint: Vec<DirtyNode>,

    /// Nodes needing semantics update (sorted shallow-first during flush).
    pub needs_semantics: Vec<DirtyNode>,
}

impl DirtySets {
    /// Creates an empty `DirtySets`. All four vectors are empty.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Evicts every entry whose id is in `removed` from all four
    /// queues.
    ///
    /// The dispose half of node removal: a freed slot's queue entries
    /// must die WITH the node, or the next phase walks ids whose
    /// generation no longer resolves (the residue scan would warn on
    /// every removal). O(queue lengths) per call, average and worst
    /// case — removal batches are rare relative to frames.
    pub fn evict(&mut self, removed: &rustc_hash::FxHashSet<flui_foundation::RenderId>) {
        self.needs_layout.retain(|d| !removed.contains(&d.id));
        self.needs_compositing.retain(|d| !removed.contains(&d.id));
        self.needs_paint.retain(|d| !removed.contains(&d.id));
        self.needs_semantics.retain(|d| !removed.contains(&d.id));
    }

    /// Returns the total number of dirty entries across all four sets.
    #[inline]
    pub fn total(&self) -> usize {
        self.needs_layout.len()
            + self.needs_paint.len()
            + self.needs_compositing.len()
            + self.needs_semantics.len()
    }

    /// Returns `true` when any phase has at least one dirty entry.
    #[inline]
    pub fn any(&self) -> bool {
        self.total() > 0
    }

    /// Clears every dirty set. Vectors retain their capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing.clear();
        self.needs_semantics.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    // Mythos Step 14: static memory-footprint assertions.

    #[test]
    fn dirty_node_is_two_usize() {
        // RenderId(NonZeroUsize, 8) + usize(8) = 16 bytes on 64-bit.
        assert!(size_of::<DirtyNode>() <= 16);
    }

    #[test]
    fn dirty_sets_fits_one_cache_line_of_pointers() {
        // Four Vec headers (24 bytes each on 64-bit) = 96 bytes; should
        // never grow beyond that without an explicit re-budget.
        assert!(size_of::<DirtySets>() <= 96);
    }
}
