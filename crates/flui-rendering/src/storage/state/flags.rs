//! Basic dirty-flag accessors and boundary configuration for `RenderState<P>`.
//!
//! This file contains the lock-free flag accessors (`needs_layout`,
//! `needs_paint`, `clear_*`, `flags()`) and boundary configuration methods
//! (`is_relayout_boundary`, `is_repaint_boundary`, `set_*_boundary`,
//! `was_repaint_boundary`, `set_was_repaint_boundary`).

use super::RenderState;
use crate::protocol::Protocol;
use crate::storage::flags::{AtomicRenderFlags, RenderFlags};

// ============================================================================
// BASIC DIRTY FLAGS (LOCK-FREE)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Returns a reference to the atomic render flags.
    ///
    /// This provides direct access to the flags for callers that need
    /// fine-grained control over flag operations.
    #[inline]
    pub fn flags(&self) -> &AtomicRenderFlags {
        &self.flags
    }

    /// Checks if layout is needed (lock-free, O(1)).
    ///
    /// This is called frequently in hot paths, so it's optimized for speed:
    /// - Single atomic load operation
    /// - No locks or blocking
    /// - Inlined for zero-cost abstraction
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if state.needs_layout() {
    ///     perform_layout();
    ///     state.clear_needs_layout();
    /// }
    /// ```
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Checks if paint is needed (lock-free, O(1)).
    ///
    /// Similar performance characteristics to `needs_layout()`.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Checks if compositing is needed (lock-free, O(1)).
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Marks the layout dirty flag.
    ///
    /// Sets `NEEDS_LAYOUT` on the underlying atomic flags. Idempotent —
    /// re-marking an already-dirty node is a no-op at the atomic level.
    ///
    /// Lock-free, `O(1)`. Used by [`PipelineOwner::mark_needs_layout`]
    /// (added in D-block PR-A1 U15) to walk the ancestor chain marking
    /// each node up to the nearest relayout boundary.
    ///
    /// [`PipelineOwner::mark_needs_layout`]: crate::pipeline::PipelineOwner::mark_needs_layout
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let state = BoxRenderState::new();
    /// state.clear_needs_layout();
    /// state.mark_needs_layout();
    /// assert!(state.needs_layout());
    /// ```
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags.mark_needs_layout();
    }

    /// Clears the layout dirty flag.
    ///
    /// Call this after successfully completing layout.
    /// Layout flag is cleared independently of paint flag.
    ///
    /// # Safety
    ///
    /// Only call this after layout succeeds. Clearing prematurely
    /// will cause incorrect rendering.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clears the paint dirty flag.
    ///
    /// Call this after successfully completing paint.
    ///
    /// # Safety
    ///
    /// Only call this after paint succeeds. Clearing prematurely
    /// will cause visual artifacts.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Clears the compositing dirty flag.
    #[inline]
    pub fn clear_needs_compositing(&self) {
        self.flags.remove(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Clears all dirty flags (after all phases complete).
    ///
    /// Use this sparingly - usually you want to clear flags individually
    /// as each phase completes.
    #[inline]
    pub fn clear_all_flags(&self) {
        self.flags.clear();
    }
}

// ============================================================================
// BOUNDARY CONFIGURATION
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Checks if this render object is a relayout boundary.
    ///
    /// Relayout boundaries prevent layout propagation upward in the tree,
    /// improving performance by limiting relayout scope. Production callers
    /// inspect this flag during pipeline-owner-driven traversal; the
    /// `mark_needs_layout`-style chain hangs off `PipelineOwner`, not off
    /// `RenderState`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if state.is_relayout_boundary() {
    ///     // boundary owns its layout pass
    ///     pipeline_owner.add_node_needing_layout(element_id);
    /// }
    /// ```
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }

    /// Checks if this render object is a repaint boundary.
    ///
    /// Repaint boundaries prevent paint propagation upward, enabling
    /// layer caching and more efficient repainting.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Sets whether this render object is a relayout boundary.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Make this a relayout boundary to isolate layout changes
    /// state.set_relayout_boundary(true);
    /// ```
    #[inline]
    pub fn set_relayout_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.flags.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Sets whether this render object is a repaint boundary.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Make this a repaint boundary to isolate paint changes
    /// state.set_repaint_boundary(true);
    /// ```
    #[inline]
    pub fn set_repaint_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.flags.set(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Returns the previous-frame `IS_REPAINT_BOUNDARY` value.
    ///
    /// Written by the paint phase after a node is painted. Read by
    /// compositing-bits propagation to detect repaint-boundary transitions.
    ///
    /// Hoisted off the `RenderObject<P>` trait surface (Flutter stores
    /// this as `_wasRepaintBoundary` on the render object; in FLUI it
    /// lives on `RenderState` so the paint phase flips a single atomic
    /// bit rather than acquiring a write lock on the trait object). See
    /// `docs/PORT.md` Refusal trigger 1 and the U2 exemplar refactor.
    ///
    /// Flutter equivalent: `_wasRepaintBoundary` (field read).
    #[inline]
    pub fn was_repaint_boundary(&self) -> bool {
        self.flags.was_repaint_boundary()
    }

    /// Sets the previous-frame `IS_REPAINT_BOUNDARY` value.
    ///
    /// Flutter equivalent: `_wasRepaintBoundary = value`.
    #[inline]
    pub fn set_was_repaint_boundary(&self, was_boundary: bool) {
        self.flags.set_was_repaint_boundary(was_boundary);
    }
}
