//! Basic dirty-flag accessors and boundary configuration for `RenderState<P>`.
//!
//! The flag accessors (`needs_layout`,
//! `needs_paint`, `clear_*`, `flags()`) and boundary configuration methods
//! (`is_relayout_boundary`, `is_repaint_boundary`, `set_*_boundary`,
//! `was_repaint_boundary`, `set_was_repaint_boundary`).

use super::RenderState;
use crate::protocol::Protocol;
use crate::storage::flags::{RenderFlags, RenderFlagsCell};

// ============================================================================
// BASIC DIRTY FLAGS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// The node's flag set.
    ///
    /// This provides direct access to the flags for callers that need
    /// fine-grained control over flag operations.
    #[inline]
    pub(crate) fn flags(&self) -> &RenderFlagsCell {
        &self.flags
    }

    /// Checks if layout is needed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::storage::BoxRenderState;
    ///
    /// // A node that has never been laid out needs layout.
    /// assert!(BoxRenderState::new().needs_layout());
    /// ```
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Checks if paint is needed.
    ///
    /// Similar performance characteristics to `needs_layout()`.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Checks if this node's own composited-layer properties changed while
    /// nothing it painted did.
    #[inline]
    pub fn needs_composited_layer_update(&self) -> bool {
        self.flags
            .contains(RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE)
    }

    /// Checks if compositing is needed.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Marks the layout dirty flag.
    ///
    /// Sets `NEEDS_LAYOUT`. Idempotent: re-marking an already-dirty node
    /// changes nothing.
    ///
    /// Used by [`PipelineOwner::mark_needs_layout`]
    /// to walk the ancestor chain marking each node up to the nearest
    /// relayout boundary.
    ///
    /// [`PipelineOwner::mark_needs_layout`]: crate::pipeline::PipelineOwner::mark_needs_layout
    #[inline]
    pub(crate) fn mark_needs_layout(&self) {
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
    pub(crate) fn clear_needs_layout(&self) {
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
    pub(crate) fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Clears the composited-layer-update flag.
    ///
    /// Cleared wherever [`Self::clear_needs_paint`] is: a repaint rebuilds the
    /// layer from current properties anyway, so it subsumes the update.
    #[inline]
    pub(crate) fn clear_needs_composited_layer_update(&self) {
        self.flags
            .remove(RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE);
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

    /// Whether the last committed geometry came from a pass that read a
    /// stand-in (see [`RenderFlags::GEOMETRY_DEGRADED`]).
    #[inline]
    pub fn geometry_degraded(&self) -> bool {
        self.flags.geometry_degraded()
    }

    /// Records whether the pass that just committed this node's geometry read
    /// a stand-in (see [`RenderFlags::GEOMETRY_DEGRADED`]).
    #[inline]
    pub(crate) fn set_geometry_degraded(&self, degraded: bool) {
        self.flags.set_geometry_degraded(degraded);
    }

    /// Sets whether this render object is a relayout boundary.
    #[inline]
    pub(crate) fn set_relayout_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.flags.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Returns the previous-frame `IS_REPAINT_BOUNDARY` value.
    ///
    /// Written by the paint phase after a node is painted. Read by
    /// compositing-bits propagation to detect repaint-boundary transitions.
    ///
    /// Hoisted off the `RenderObject<P>` trait surface: it lives on
    /// `RenderState` so the paint phase flips it through `&self` without
    /// borrowing the render object mutably.
    #[inline]
    pub fn was_repaint_boundary(&self) -> bool {
        self.flags.was_repaint_boundary()
    }
}
