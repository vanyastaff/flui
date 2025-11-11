//! RenderSliverState - per-RenderSliver state storage
//!
//! Similar to RenderState but designed for sliver-based rendering.
//! Uses SliverGeometry and SliverConstraints instead of Size and BoxConstraints.

use flui_types::constraints::SliverConstraints;
use flui_types::{Offset, SliverGeometry};
use parking_lot::RwLock;

use super::render_flags::{AtomicRenderFlags, RenderFlags};

/// State for a RenderSliver
///
/// **Performance Critical Design**:
/// - Atomic flags for lock-free checks (10x faster than RwLock)
/// - RwLock for actual data (geometry, constraints, offset)
/// - Separate locks to minimize contention
///
/// # Memory Layout
///
/// ```text
/// RenderSliverState {
///     flags: 4 bytes (atomic)                    ← Lock-free!
///     geometry: RwLock<Option<SliverGeometry>>   ← ~48 bytes + lock
///     constraints: RwLock<...>                   ← ~32 bytes + lock
///     offset: RwLock<Offset>                     ← 8 bytes + lock
/// }
/// ```
///
/// Total: ~100 bytes per RenderSliver (acceptable overhead)
#[derive(Debug)]
pub struct RenderSliverState {
    /// Atomic flags for lock-free state checks
    ///
    /// **Critical for performance**: checking `needs_layout()` happens
    /// thousands of times per frame. Atomic operations are ~10x faster
    /// than RwLock for these hot paths.
    pub flags: AtomicRenderFlags,

    /// Computed sliver geometry after layout
    ///
    /// `None` if layout hasn't been computed yet.
    /// RwLock allows concurrent reads from multiple threads.
    pub geometry: RwLock<Option<SliverGeometry>>,

    /// Constraints used for last layout
    ///
    /// Used for cache validation and relayout decisions.
    pub constraints: RwLock<Option<SliverConstraints>>,

    /// Offset in viewport's coordinate space
    ///
    /// Set during viewport's layout phase.
    /// This represents the scroll offset for this sliver.
    pub offset: RwLock<Offset>,
}

impl RenderSliverState {
    /// Create new RenderSliverState with empty flags
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::empty(),
            geometry: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
        }
    }

    /// Create RenderSliverState with initial flags
    pub fn with_flags(flags: RenderFlags) -> Self {
        Self {
            flags: AtomicRenderFlags::new(flags),
            geometry: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
        }
    }

    // ========== Layout State ==========

    /// Check if layout is needed (lock-free!)
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Mark as needing layout
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags.set(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clear needs_layout flag
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Check if this is a relayout boundary
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }

    /// Set relayout boundary flag
    #[inline]
    pub fn set_relayout_boundary(&self, value: bool) {
        if value {
            self.flags.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    // ========== Paint State ==========

    /// Check if paint is needed (lock-free!)
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Mark as needing paint
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.flags.set(RenderFlags::NEEDS_PAINT);
    }

    /// Clear needs_paint flag
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Check if this is a repaint boundary
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Set repaint boundary flag
    #[inline]
    pub fn set_repaint_boundary(&self, value: bool) {
        if value {
            self.flags.set(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    // ========== Compositing State ==========

    /// Check if compositing is needed
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Mark as needing compositing
    #[inline]
    pub fn mark_needs_compositing(&self) {
        self.flags.set(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Clear needs_compositing flag
    #[inline]
    pub fn clear_needs_compositing(&self) {
        self.flags.remove(RenderFlags::NEEDS_COMPOSITING);
    }

    // ========== Geometry & Constraints ==========

    /// Get computed sliver geometry
    #[inline]
    pub fn geometry(&self) -> Option<SliverGeometry> {
        *self.geometry.read()
    }

    /// Set computed sliver geometry
    pub fn set_geometry(&self, geometry: SliverGeometry) {
        *self.geometry.write() = Some(geometry);
        self.flags.set(RenderFlags::HAS_SIZE); // Reuse HAS_SIZE flag for "has geometry"
    }

    /// Check if geometry has been computed
    #[inline]
    pub fn has_geometry(&self) -> bool {
        self.flags.contains(RenderFlags::HAS_SIZE) // Reuse HAS_SIZE flag
    }

    /// Get constraints
    #[inline]
    pub fn constraints(&self) -> Option<SliverConstraints> {
        *self.constraints.read()
    }

    /// Set constraints
    pub fn set_constraints(&self, constraints: SliverConstraints) {
        *self.constraints.write() = Some(constraints);
    }

    /// Clear constraints
    ///
    /// This is used when window resizes or layout needs to be fully recalculated.
    /// Clearing constraints ensures that layout_pipeline uses fresh constraints
    /// from flush_layout() instead of cached constraints.
    pub fn clear_constraints(&self) {
        *self.constraints.write() = None;
    }

    // ========== Offset ==========

    /// Get offset (scroll position in viewport)
    #[inline]
    pub fn offset(&self) -> Offset {
        *self.offset.read()
    }

    /// Set offset (scroll position in viewport)
    pub fn set_offset(&self, offset: Offset) {
        *self.offset.write() = offset;
    }

    // ========== Lifecycle ==========

    /// Check if detached from tree
    #[inline]
    pub fn is_detached(&self) -> bool {
        self.flags.contains(RenderFlags::IS_DETACHED)
    }

    /// Mark as detached
    #[inline]
    pub fn mark_detached(&self) {
        self.flags.set(RenderFlags::IS_DETACHED);
    }

    /// Mark as attached
    #[inline]
    pub fn mark_attached(&self) {
        self.flags.remove(RenderFlags::IS_DETACHED);
    }

    /// Reset all state (for reuse)
    pub fn reset(&self) {
        self.flags.clear();
        *self.geometry.write() = None;
        *self.constraints.write() = None;
        *self.offset.write() = Offset::ZERO;
    }
}

impl Default for RenderSliverState {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RenderSliverState {
    fn clone(&self) -> Self {
        Self {
            flags: self.flags.clone(),
            geometry: RwLock::new(*self.geometry.read()),
            constraints: RwLock::new(*self.constraints.read()),
            offset: RwLock::new(*self.offset.read()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_state_creation() {
        let state = RenderSliverState::new();
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
        assert!(!state.has_geometry());
    }

    #[test]
    fn test_layout_flags() {
        let state = RenderSliverState::new();

        state.mark_needs_layout();
        assert!(state.needs_layout());

        state.clear_needs_layout();
        assert!(!state.needs_layout());
    }

    #[test]
    fn test_geometry_management() {
        let state = RenderSliverState::new();
        assert!(!state.has_geometry());

        let geometry = SliverGeometry::default();
        state.set_geometry(geometry);
        assert!(state.has_geometry());
        assert_eq!(state.geometry(), Some(geometry));
    }

    #[test]
    fn test_relayout_boundary() {
        let state = RenderSliverState::new();
        assert!(!state.is_relayout_boundary());

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
    }

    #[test]
    fn test_reset() {
        let state = RenderSliverState::new();

        state.mark_needs_layout();
        state.set_geometry(SliverGeometry::default());

        state.reset();

        assert!(!state.needs_layout());
        assert!(!state.has_geometry());
        assert_eq!(state.geometry(), None);
    }
}
