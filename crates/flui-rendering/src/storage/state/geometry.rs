//! Geometry storage and protocol-specific convenience methods.
//!
//! This file contains:
//! - Generic write-once `OnceCell<ProtocolGeometry<P>>` accessors
//!   (`geometry`, `set_geometry`, `clear_geometry`) on `RenderState<P>`
//! - Box protocol convenience methods (`compute_relayout_boundary`, `size`,
//!   `set_size`, `has_size`) on `RenderState<BoxProtocol>`
//! - Sliver protocol convenience methods (`scroll_extent`, `paint_extent`,
//!   `layout_extent`, `max_paint_extent`, `set_sliver_geometry`) on
//!   `RenderState<SliverProtocol>`

use once_cell::sync::OnceCell;

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
    pub fn geometry(&self) -> Option<ProtocolGeometry<P>>
    where
        ProtocolGeometry<P>: Copy,
    {
        self.geometry.get().copied()
    }

    /// Sets the computed geometry after layout.
    ///
    /// This should be called exactly once per layout pass. If geometry
    /// already exists, this will panic (use `clear_geometry()` first if
    /// you need to relayout).
    ///
    /// # Performance
    ///
    /// - First call: One atomic CAS operation
    /// - Subsequent calls: Panic (by design, to catch bugs)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let size = compute_size(constraints);
    /// state.set_geometry(size); // Write once
    ///
    /// // Later reads are zero-cost
    /// let cached = state.geometry().unwrap();
    /// ```
    pub fn set_geometry(&self, geometry: ProtocolGeometry<P>) {
        if self.geometry.set(geometry).is_err() {
            // Geometry already set - this is a bug!
            // You must call clear_geometry() before relayout
            panic!(
                "Geometry already set! Call clear_geometry() before relayout. \
                 This indicates a logic error in the layout code."
            );
        }
    }

    /// Clears the geometry to allow relayout.
    ///
    /// Must be called before `set_geometry()` if geometry already exists.
    /// Usually called automatically when `mark_needs_layout()` is called.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Force relayout
    /// state.clear_geometry();
    /// state.mark_needs_layout(element_id, tree);
    /// ```
    #[inline]
    pub fn clear_geometry(&mut self) {
        self.geometry = OnceCell::new();
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
    /// # Example
    ///
    /// ```rust,ignore
    /// state.set_size(Size::new(100.0, 50.0));
    /// ```
    #[inline]
    pub fn set_size(&self, size: flui_types::Size) {
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
    ///     state.mark_needs_layout(element_id, tree);
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
    pub fn set_sliver_geometry(&self, geometry: SliverGeometry) {
        self.set_geometry(geometry);
    }
}
