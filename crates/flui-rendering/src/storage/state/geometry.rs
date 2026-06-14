//! Geometry storage and protocol-specific convenience methods.
//!
//! This file contains:
//! - Generic `Option<ProtocolGeometry<P>>`-backed accessors (`geometry`,
//!   `set_geometry`, `clear_geometry`) on `RenderState<P>`
//! - Box protocol convenience methods (`compute_relayout_boundary`, `size`,
//!   `set_size`, `has_size`) on `RenderState<BoxProtocol>`
//! - Sliver protocol convenience methods (`scroll_extent`, `paint_extent`,
//!   `layout_extent`, `max_paint_extent`, `set_sliver_geometry`) on
//!   `RenderState<SliverProtocol>`
//!
//! **D-block PR-A1 U14 migration (2026-05-23):** the prior `OnceCell`-backed
//! `set_geometry` panicked on second invocation, which made frame-2 re-layout
//! a crash. Flutter `.flutter/.../object.dart` straight-assigns `_size` each
//! layout pass; we mirror that semantics via `Option<T>`. `set_geometry`,
//! `set_size`, `set_sliver_geometry` now take `&mut self`; production callers
//! (`RenderEntry::layout`, RenderBox/RenderSliver impls) already hold a mut
//! state borrow.

use super::RenderState;
use crate::constraints::{Constraints, SliverGeometry};
use crate::protocol::{BoxProtocol, Protocol, ProtocolGeometry, SliverProtocol};

// ============================================================================
// GEOMETRY (PROTOCOL-SPECIFIC, WRITE-ONCE READ-MANY)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the computed geometry (if available).
    ///
    /// Returns `None` if layout has not been performed yet.
    ///
    /// # Performance
    ///
    /// After first `set_geometry()`:
    /// - O(1) time
    /// - Single pointer load
    /// - No allocation or cloning
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(size) = state.geometry() {
    ///     // Use cached size
    /// } else {
    ///     // Need to perform layout first
    /// }
    /// ```
    #[inline]
    pub fn geometry(&self) -> Option<ProtocolGeometry<P>>
    where
        ProtocolGeometry<P>: Copy,
    {
        self.geometry
    }

    /// Sets (or replaces) the computed geometry after layout.
    ///
    /// Idempotent — overwrites any prior value. Flutter `_size = size`
    /// straight-assignment semantics.
    ///
    /// **D-block PR-A1 U14**: prior `OnceCell`-backed implementation panicked
    /// on second invocation, which made re-layout a crash. See module-level
    /// doc for rationale.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let size = compute_size(constraints);
    /// state.set_geometry(size); // first layout pass
    /// state.set_geometry(new_size); // re-layout (no panic)
    /// ```
    #[inline]
    pub fn set_geometry(&mut self, geometry: ProtocolGeometry<P>) {
        self.geometry = Some(geometry);
    }

    /// Clears the geometry, signalling no prior layout has run.
    ///
    /// Equivalent to `set_geometry` with no value; production re-layout no
    /// longer requires an explicit clear (the `OnceCell`-era invariant is
    /// gone). Useful in tests and eviction paths.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Reset state — useful in tests
    /// state.clear_geometry();
    /// ```
    #[inline]
    pub fn clear_geometry(&mut self) {
        self.geometry = None;
    }
}

// ============================================================================
// CONVENIENCE METHODS FOR BOX PROTOCOL
// ============================================================================

impl RenderState<BoxProtocol> {
    /// Computes and updates the relayout boundary status based on layout
    /// parameters.
    ///
    /// This implements Flutter's exact relayout boundary detection logic for
    /// Box protocol:
    ///
    /// ```text
    /// is_boundary = !parent_uses_size || sized_by_parent || constraints.is_tight() || has_no_parent
    /// ```
    ///
    /// # Flutter Protocol
    ///
    /// From Flutter's `RenderObject.layout()`:
    /// ```dart
    /// void layout(Constraints constraints, { bool parentUsesSize = false }) {
    ///   // ...
    ///   _relayoutBoundary = _isRelayoutBoundary(constraints, parentUsesSize);
    /// }
    ///
    /// bool _isRelayoutBoundary(Constraints constraints, bool parentUsesSize) {
    ///   return !parentUsesSize || sizedByParent || constraints.isTight || parent == null;
    /// }
    /// ```
    ///
    /// # Parameters
    ///
    /// - `parent_uses_size`: Whether parent's layout depends on this element's
    ///   size
    /// - `sized_by_parent`: Whether size is determined purely by constraints
    /// - `has_parent`: Whether this element has a parent (root is always a
    ///   boundary)
    ///
    /// # When Each Condition Triggers
    ///
    /// 1. **`!parent_uses_size`** - Parent doesn't care about size changes
    ///    - Example: Fixed-size container ignoring child size
    ///    - Most powerful optimization case
    ///
    /// 2. **`sized_by_parent`** - Size determined by constraints alone
    ///    - Example: Container that always fills available space
    ///    - Size won't change even if children change
    ///
    /// 3. **`constraints.is_tight()`** - Only one valid size
    ///    - Example: `BoxConstraints.tight(Size(100, 50))`
    ///    - Size mathematically cannot change
    ///
    /// 4. **`!has_parent`** - Root of tree
    ///    - No parent to propagate to
    ///    - Always a boundary by definition
    ///
    /// # Performance Impact
    ///
    /// When this element becomes a relayout boundary:
    /// - ✅ Layout changes stop here (don't propagate to parent)
    /// - ✅ O(1) relayout instead of O(tree height)
    /// - ✅ Massive performance win for deep trees
    /// - ✅ Enables incremental layout updates
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // During layout, compute boundary status
    /// state.compute_relayout_boundary(
    ///     parent_uses_size,
    ///     sized_by_parent,
    ///     has_parent
    /// );
    ///
    /// // Later, check if we're a boundary
    /// if state.is_relayout_boundary() {
    ///     // Don't propagate layout changes to parent
    ///     owner.register_needs_layout(element_id);
    /// }
    /// ```
    pub fn compute_relayout_boundary(
        &self,
        parent_uses_size: bool,
        sized_by_parent: bool,
        has_parent: bool,
    ) {
        // Flutter's exact logic:
        // is_boundary = !parent_uses_size || sized_by_parent || constraints.is_tight()
        // || !has_parent

        let constraints_are_tight = self.constraints().map(|c| c.is_tight()).unwrap_or(false);

        let is_boundary = !parent_uses_size  // Parent doesn't use size
            || sized_by_parent                // Size determined by constraints
            || constraints_are_tight          // Only one valid size
            || !has_parent; // Root of tree

        self.set_relayout_boundary(is_boundary);
    }

    /// Returns `Size::ZERO` if geometry is not set.
    ///
    /// Convenience method for box protocol that provides a safe fallback.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let size = state.size(); // Never panics, returns ZERO if not laid out
    /// ```
    #[inline]
    pub fn size(&self) -> flui_types::Size {
        self.geometry().unwrap_or(flui_types::Size::ZERO)
    }

    /// Convenience method for setting size (box protocol).
    ///
    /// **D-block PR-A1 U14**: signature changed to `&mut self` alongside
    /// `set_geometry` migration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// state.set_size(Size::new(100.0, 50.0));
    /// ```
    #[inline]
    pub fn set_size(&mut self, size: flui_types::Size) {
        self.set_geometry(size);
    }

    /// Checks if size matches the given value.
    ///
    /// Useful for change detection and optimization.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if !state.has_size(new_size) {
    ///     state.clear_geometry();
    ///     pipeline_owner.add_node_needing_layout(element_id);
    /// }
    /// ```
    #[inline]
    pub fn has_size(&self, size: flui_types::Size) -> bool {
        self.geometry().map(|s| s == size).unwrap_or(false)
    }
}

// ============================================================================
// CONVENIENCE METHODS FOR SLIVER PROTOCOL
// ============================================================================

impl RenderState<SliverProtocol> {
    /// Returns scroll extent, or 0.0 if geometry is not set.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let total_scroll = state.scroll_extent();
    /// ```
    #[inline]
    pub fn scroll_extent(&self) -> f32 {
        self.geometry().map(|g| g.scroll_extent).unwrap_or(0.0)
    }

    /// Returns paint extent, or 0.0 if geometry is not set.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let visible = state.paint_extent();
    /// if visible > 0.0 {
    ///     // Paint visible portion
    /// }
    /// ```
    #[inline]
    pub fn paint_extent(&self) -> f32 {
        self.geometry().map(|g| g.paint_extent).unwrap_or(0.0)
    }

    /// Returns layout extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn layout_extent(&self) -> f32 {
        self.geometry().map(|g| g.layout_extent).unwrap_or(0.0)
    }

    /// Returns max paint extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn max_paint_extent(&self) -> f32 {
        self.geometry().map(|g| g.max_paint_extent).unwrap_or(0.0)
    }

    /// Sets sliver geometry (convenience wrapper for `set_geometry()`).
    ///
    /// **D-block PR-A1 U14**: signature changed to `&mut self` alongside
    /// `set_geometry` migration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let geom = SliverGeometry {
    ///     scroll_extent: 1000.0,
    ///     paint_extent: 500.0,
    ///     ..Default::default()
    /// };
    /// state.set_sliver_geometry(geom);
    /// ```
    #[inline]
    pub fn set_sliver_geometry(&mut self, geometry: SliverGeometry) {
        self.set_geometry(geometry);
    }

    /// The sliver's absolute paint size in box pixels.
    ///
    /// Maps the main-axis `paint_extent` to width/height per the
    /// laid-out `axis_direction`, with the cross axis taken from
    /// `cross_axis_extent` (Flutter `RenderSliver.getAbsoluteSize`).
    /// Returns `Size::ZERO` before the first layout (no committed
    /// geometry or constraints).
    ///
    /// The paint / hit drivers thread this into the protocol blanket so
    /// a sliver reads `ctx.size()` instead of caching its own geometry
    /// (2B field dedup — `RenderState` is geometry's sole owner). O(1).
    #[inline]
    pub fn absolute_paint_size(&self) -> flui_types::Size {
        use flui_types::geometry::px;
        use flui_types::prelude::AxisDirection;

        let (Some(geometry), Some(constraints)) = (self.geometry(), self.constraints()) else {
            return flui_types::Size::ZERO;
        };
        let cross = constraints.cross_axis_extent;
        let main = geometry.paint_extent;
        match constraints.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                flui_types::Size::new(px(cross), px(main))
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                flui_types::Size::new(px(main), px(cross))
            }
        }
    }
}
