//! RenderState - per-render state storage.
//!
//! **CRITICAL FIX (v2024-12-19)**: Refactored to use type-erased Geometry enum
//! instead of generic Protocol parameter. This fixes the bug where `RenderElement`
//! stored `RenderState<BoxProtocol>` but could contain Sliver render objects.
//!
//! # Why Type-Erased?
//!
//! The element tree must store render state without knowing the protocol type
//! at compile time (protocol is a runtime `LayoutProtocol` enum). Type erasure
//! via the `Geometry` and `Constraints` enums enables this cleanly.
//!
//! # Performance Design
//!
//! - **Atomic flags**: Lock-free for hot checks (`needs_layout()`, `needs_paint()`)
//! - **RwLock data**: For geometry, constraints, offset (less frequent access)
//! - **Separate locks**: Minimize contention between layout and paint
//!
//! # Example
//!
//! ```rust,ignore
//! let state = RenderState::new();
//!
//! // Lock-free checks (fast!)
//! if state.needs_layout() {
//!     // Perform layout...
//!     state.set_geometry(Geometry::Box(computed_size));
//!     state.clear_needs_layout();
//! }
//! ```

use flui_types::Offset;
use parking_lot::RwLock;

use super::geometry::{Constraints, Geometry};
use super::render_flags::{AtomicRenderFlags, RenderFlags};

/// State for a Render object using type-erased geometry and constraints.
///
/// **Performance Critical Design**:
/// - Atomic flags for lock-free checks (10x faster than RwLock)
/// - RwLock for actual data (geometry, constraints, offset)
/// - Separate locks to minimize contention
///
/// # Type Erasure
///
/// Uses `Geometry` and `Constraints` enums instead of generic Protocol parameter.
/// This enables storage in `RenderElement` which has runtime protocol dispatch.
///
/// # Memory Layout
///
/// ```text
/// RenderState {
///     flags: 4 bytes (atomic)                           ← Lock-free!
///     geometry: RwLock<Option<Geometry>>                ← Type-erased
///     constraints: RwLock<Option<Constraints>>          ← Type-erased
///     offset: RwLock<Offset>                            ← 8 bytes
/// }
/// ```
#[derive(Debug)]
pub struct RenderState {
    /// Atomic flags for lock-free state checks
    ///
    /// **Critical for performance**: checking `needs_layout()` happens
    /// thousands of times per frame. Atomic operations are ~10x faster
    /// than RwLock for these hot paths.
    pub flags: AtomicRenderFlags,

    /// Computed geometry after layout (type-erased)
    ///
    /// `None` if layout hasn't been computed yet.
    /// Can be either `Geometry::Box(Size)` or `Geometry::Sliver(SliverGeometry)`.
    pub geometry: RwLock<Option<Geometry>>,

    /// Constraints used for last layout (type-erased)
    ///
    /// Used for cache validation and relayout decisions.
    /// Can be either `Constraints::Box(BoxConstraints)` or `Constraints::Sliver(SliverConstraints)`.
    pub constraints: RwLock<Option<Constraints>>,

    /// Offset in parent's coordinate space
    ///
    /// Set during parent's layout phase.
    pub offset: RwLock<Offset>,
}

impl RenderState {
    /// Create new RenderState with NEEDS_LAYOUT and NEEDS_PAINT flags
    ///
    /// New render objects always need initial layout and paint.
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT),
            geometry: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
        }
    }

    /// Create RenderState with initial flags
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

    /// Get computed geometry (type-erased)
    #[inline]
    pub fn geometry(&self) -> Option<Geometry> {
        self.geometry.read().clone()
    }

    /// Set computed geometry
    pub fn set_geometry(&self, geometry: Geometry) {
        *self.geometry.write() = Some(geometry);
        self.flags.set(RenderFlags::HAS_GEOMETRY);
    }

    /// Check if geometry has been computed
    #[inline]
    pub fn has_geometry(&self) -> bool {
        self.flags.contains(RenderFlags::HAS_GEOMETRY)
    }

    /// Get constraints (type-erased)
    #[inline]
    pub fn constraints(&self) -> Option<Constraints> {
        self.constraints.read().clone()
    }

    /// Set constraints
    pub fn set_constraints(&self, constraints: Constraints) {
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

    /// Get offset
    #[inline]
    pub fn offset(&self) -> Offset {
        *self.offset.read()
    }

    /// Set offset
    pub fn set_offset(&self, offset: Offset) {
        *self.offset.write() = offset;
    }

    // ========== Lifecycle ==========

    /// Reset all state (for reuse)
    pub fn reset(&self) {
        self.flags.clear();
        *self.geometry.write() = None;
        *self.constraints.write() = None;
        *self.offset.write() = Offset::ZERO;
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new()
    }
}

// Convenience methods for Box protocol
impl RenderState {
    /// Get computed size for Box protocol (panics if Sliver)
    #[inline]
    pub fn size(&self) -> flui_types::Size {
        self.geometry().map(|g| g.as_box()).unwrap_or_default()
    }

    /// Set computed size for Box protocol
    pub fn set_size(&self, size: flui_types::Size) {
        self.set_geometry(Geometry::Box(size));
    }

    /// Check if size (box geometry) has been computed
    #[inline]
    pub fn has_size(&self) -> bool {
        self.has_geometry()
    }
}

impl Clone for RenderState {
    fn clone(&self) -> Self {
        Self {
            flags: self.flags.clone(),
            geometry: RwLock::new(self.geometry.read().clone()),
            constraints: RwLock::new(self.constraints.read().clone()),
            offset: RwLock::new(*self.offset.read()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

    #[test]
    fn test_render_state_creation() {
        let state = RenderState::new();
        // New render states need layout and paint by default
        assert!(state.needs_layout());
        assert!(state.needs_paint());
        assert!(!state.has_geometry());
    }

    #[test]
    fn test_layout_flags() {
        let state = RenderState::new();

        state.mark_needs_layout();
        assert!(state.needs_layout());

        state.clear_needs_layout();
        assert!(!state.needs_layout());
    }

    #[test]
    fn test_box_geometry_management() {
        let state = RenderState::new();
        assert!(!state.has_geometry());

        let size = Size::new(100.0, 100.0);
        state.set_geometry(Geometry::Box(size));
        assert!(state.has_geometry());
        assert_eq!(state.geometry(), Some(Geometry::Box(size)));
    }

    #[test]
    fn test_relayout_boundary() {
        let state = RenderState::new();
        assert!(!state.is_relayout_boundary());

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
    }

    #[test]
    fn test_reset() {
        let state = RenderState::new();

        // New state already has needs_layout, set geometry
        state.set_geometry(Geometry::Box(Size::new(50.0, 50.0)));

        state.reset();

        // After reset, all flags should be cleared
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());
        assert!(!state.has_geometry());
        assert_eq!(state.geometry(), None);
    }

    #[test]
    fn test_size_convenience_methods() {
        let state = RenderState::new();

        let size = Size::new(200.0, 150.0);
        state.set_size(size);

        assert!(state.has_size());
        assert_eq!(state.size(), size);
    }
}
