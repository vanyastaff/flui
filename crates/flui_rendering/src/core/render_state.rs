//! Protocol-specific render state storage.
//!
//! - [`RenderState<P>`] - Type-safe state with lock-free dirty flags
//! - [`BoxRenderState`] - Alias for `RenderState<BoxProtocol>`
//! - [`SliverRenderState`] - Alias for `RenderState<SliverProtocol>`

use std::marker::PhantomData;

use flui_types::Offset;
use parking_lot::RwLock;

use super::protocol::{BoxProtocol, Protocol, SliverProtocol};
use super::render_flags::{AtomicRenderFlags, RenderFlags};

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Render state for Box protocol (uses Size and BoxConstraints)
pub type BoxRenderState = RenderState<BoxProtocol>;

/// Render state for Sliver protocol (uses SliverGeometry and SliverConstraints)
pub type SliverRenderState = RenderState<SliverProtocol>;

// ============================================================================
// RENDER STATE
// ============================================================================

/// Protocol-specific render state storage.
#[derive(Debug)]
pub struct RenderState<P: Protocol> {
    /// Atomic flags for lock-free dirty state checks.
    pub flags: AtomicRenderFlags,
    /// Computed geometry after layout.
    pub geometry: RwLock<Option<P::Geometry>>,
    /// Constraints used for last layout (for cache validation).
    pub constraints: RwLock<Option<P::Constraints>>,
    /// Paint offset in parent coordinate space.
    pub offset: RwLock<Offset>,
    _phantom: PhantomData<P>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Creates a new render state with dirty flags set.
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT),
            geometry: RwLock::new(None),
            constraints: RwLock::new(None),
            offset: RwLock::new(Offset::ZERO),
            _phantom: PhantomData,
        }
    }
}

impl<P: Protocol> Default for RenderState<P> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DIRTY FLAGS (LOCK-FREE)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Mark layout as dirty (also marks paint).
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags
            .insert(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT);
    }

    #[inline]
    pub fn mark_needs_paint(&self) {
        self.flags.insert(RenderFlags::NEEDS_PAINT);
    }

    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    #[inline]
    pub fn clear_all_flags(&self) {
        self.flags.clear();
    }
}

// ============================================================================
// GEOMETRY (PROTOCOL-SPECIFIC)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    pub fn geometry(&self) -> Option<P::Geometry> {
        self.geometry.read().clone()
    }

    pub fn set_geometry(&self, geometry: P::Geometry) {
        *self.geometry.write() = Some(geometry);
    }

    pub fn clear_geometry(&self) {
        *self.geometry.write() = None;
    }
}

// ============================================================================
// CONSTRAINTS (PROTOCOL-SPECIFIC)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    pub fn constraints(&self) -> Option<P::Constraints> {
        self.constraints.read().clone()
    }

    pub fn set_constraints(&self, constraints: P::Constraints) {
        *self.constraints.write() = Some(constraints);
    }

    pub fn clear_constraints(&self) {
        *self.constraints.write() = None;
    }
}

// ============================================================================
// OFFSET
// ============================================================================

impl<P: Protocol> RenderState<P> {
    pub fn offset(&self) -> Offset {
        *self.offset.read()
    }

    pub fn set_offset(&self, offset: Offset) {
        *self.offset.write() = offset;
    }
}

// ============================================================================
// CONVENIENCE METHODS FOR BOX PROTOCOL
// ============================================================================

impl RenderState<BoxProtocol> {
    /// Returns `Size::ZERO` if geometry is not set.
    #[inline]
    pub fn size(&self) -> flui_types::Size {
        self.geometry().unwrap_or(flui_types::Size::ZERO)
    }

    #[inline]
    pub fn set_size(&self, size: flui_types::Size) {
        self.set_geometry(size);
    }
}

// ============================================================================
// CONVENIENCE METHODS FOR SLIVER PROTOCOL
// ============================================================================

impl RenderState<SliverProtocol> {
    /// Returns scroll extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn scroll_extent(&self) -> f32 {
        self.geometry().map(|g| g.scroll_extent).unwrap_or(0.0)
    }

    /// Returns paint extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn paint_extent(&self) -> f32 {
        self.geometry().map(|g| g.paint_extent).unwrap_or(0.0)
    }
}

// ============================================================================
// CLONE
// ============================================================================

impl<P: Protocol> Clone for RenderState<P> {
    fn clone(&self) -> Self {
        Self {
            flags: AtomicRenderFlags::new(self.flags.get()),
            geometry: RwLock::new(self.geometry.read().clone()),
            constraints: RwLock::new(self.constraints.read().clone()),
            offset: RwLock::new(*self.offset.read()),
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::{BoxConstraints, Size, SliverConstraints, SliverGeometry};

    #[test]
    fn test_new_state() {
        let state = BoxRenderState::new();
        assert!(state.needs_layout());
        assert!(state.needs_paint());
        assert_eq!(state.geometry(), None);
        assert_eq!(state.constraints(), None);
        assert_eq!(state.offset(), Offset::ZERO);
    }

    #[test]
    fn test_dirty_flags() {
        let state = BoxRenderState::new();

        // Initial state
        assert!(state.needs_layout());
        assert!(state.needs_paint());

        // Clear layout
        state.clear_needs_layout();
        assert!(!state.needs_layout());
        assert!(state.needs_paint());

        // Clear paint
        state.clear_needs_paint();
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());

        // Mark layout dirty
        state.mark_needs_layout();
        assert!(state.needs_layout());
        assert!(state.needs_paint());

        // Clear all
        state.clear_all_flags();
        assert!(!state.needs_layout());
        assert!(!state.needs_paint());

        // Mark only paint dirty
        state.mark_needs_paint();
        assert!(!state.needs_layout());
        assert!(state.needs_paint());
    }

    #[test]
    fn test_box_geometry() {
        let state = BoxRenderState::new();

        // Initially None
        assert_eq!(state.geometry(), None);
        assert_eq!(state.size(), Size::ZERO);

        // Set geometry
        let size = Size::new(100.0, 50.0);
        state.set_geometry(size);
        assert_eq!(state.geometry(), Some(size));
        assert_eq!(state.size(), size);

        // Set via size convenience method
        let new_size = Size::new(200.0, 100.0);
        state.set_size(new_size);
        assert_eq!(state.size(), new_size);

        // Clear
        state.clear_geometry();
        assert_eq!(state.geometry(), None);
        assert_eq!(state.size(), Size::ZERO);
    }

    #[test]
    fn test_sliver_geometry() {
        let state = SliverRenderState::new();

        // Initially None
        assert_eq!(state.geometry(), None);

        // Set geometry
        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 500.0,
            max_paint_extent: 500.0,
            layout_extent: Some(500.0),
            ..Default::default()
        };
        state.set_geometry(geometry.clone());
        assert_eq!(state.geometry(), Some(geometry));

        // Clear
        state.clear_geometry();
        assert_eq!(state.geometry(), None);
    }

    #[test]
    fn test_box_constraints() {
        let state = BoxRenderState::new();

        // Initially None
        assert_eq!(state.constraints(), None);

        // Set constraints
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        state.set_constraints(constraints);
        assert_eq!(state.constraints(), Some(constraints));

        // Clear
        state.clear_constraints();
        assert_eq!(state.constraints(), None);
    }

    #[test]
    fn test_sliver_constraints() {
        let state = SliverRenderState::new();

        // Initially None
        assert_eq!(state.constraints(), None);

        // Set constraints
        let constraints = SliverConstraints::default();
        state.set_constraints(constraints.clone());
        assert_eq!(state.constraints(), Some(constraints));

        // Clear
        state.clear_constraints();
        assert_eq!(state.constraints(), None);
    }

    #[test]
    fn test_offset() {
        let state = BoxRenderState::new();

        // Initially zero
        assert_eq!(state.offset(), Offset::ZERO);

        // Set offset
        let offset = Offset::new(10.0, 20.0);
        state.set_offset(offset);
        assert_eq!(state.offset(), offset);
    }

    #[test]
    fn test_clone() {
        let state = BoxRenderState::new();
        state.set_size(Size::new(100.0, 50.0));
        state.set_offset(Offset::new(10.0, 20.0));
        state.clear_needs_layout();

        let cloned = state.clone();
        assert_eq!(cloned.size(), state.size());
        assert_eq!(cloned.offset(), state.offset());
        assert_eq!(cloned.needs_layout(), state.needs_layout());
        assert_eq!(cloned.needs_paint(), state.needs_paint());
    }

    #[test]
    fn test_type_aliases() {
        // Verify type aliases work
        let _box_state: BoxRenderState = RenderState::new();
        let _sliver_state: SliverRenderState = RenderState::new();
    }

    #[test]
    fn test_protocol_specific_types() {
        // Box protocol uses Size
        let box_state = BoxRenderState::new();
        box_state.set_geometry(Size::new(100.0, 50.0));
        let _size: Size = box_state.geometry().unwrap();

        // Sliver protocol uses SliverGeometry
        let sliver_state = SliverRenderState::new();
        sliver_state.set_geometry(SliverGeometry::default());
        let _geometry: SliverGeometry = sliver_state.geometry().unwrap();
    }

    // This test would fail to compile - that's the point!
    // #[test]
    // fn test_type_safety_compile_error() {
    //     let box_state = BoxRenderState::new();
    //     // This won't compile - type error!
    //     box_state.set_geometry(SliverGeometry::default());
    // }
}
