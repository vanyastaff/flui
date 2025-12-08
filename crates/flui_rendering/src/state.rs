//! Protocol-specific render state storage with Flutter-compliant dirty tracking.
//!
//! This module provides lock-free state management for render objects following
//! Flutter's exact dirty propagation semantics.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::ElementId;
use flui_types::{Offset, SliverGeometry};
use once_cell::sync::OnceCell;

use crate::flags::{AtomicRenderFlags, RenderFlags};
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Render state for Box protocol (uses Size and BoxConstraints).
pub type BoxRenderState = RenderState<BoxProtocol>;

/// Render state for Sliver protocol (uses SliverGeometry and SliverConstraints).
pub type SliverRenderState = RenderState<SliverProtocol>;

// ============================================================================
// ATOMIC OFFSET
// ============================================================================

/// Thread-safe offset storage using atomic operations.
#[derive(Debug)]
struct AtomicOffset {
    bits: AtomicU64,
}

impl AtomicOffset {
    #[inline]
    const fn new(offset: Offset) -> Self {
        let dx_bits = offset.dx.to_bits() as u64;
        let dy_bits = offset.dy.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        Self {
            bits: AtomicU64::new(packed),
        }
    }

    #[inline]
    fn load(&self) -> Offset {
        let packed = self.bits.load(Ordering::Acquire);
        let dx_bits = (packed & 0xFFFF_FFFF) as u32;
        let dy_bits = (packed >> 32) as u32;

        Offset {
            dx: f32::from_bits(dx_bits),
            dy: f32::from_bits(dy_bits),
        }
    }

    #[inline]
    fn store(&self, offset: Offset) {
        let dx_bits = offset.dx.to_bits() as u64;
        let dy_bits = offset.dy.to_bits() as u64;
        let packed = (dy_bits << 32) | dx_bits;

        self.bits.store(packed, Ordering::Release);
    }
}

// ============================================================================
// TREE OPERATIONS TRAIT
// ============================================================================

/// Minimal trait for tree operations needed by Flutter-style dirty propagation.
pub trait RenderDirtyPropagation {
    /// Gets the parent element ID, if any.
    fn parent(&self, id: ElementId) -> Option<ElementId>;

    /// Gets the render state for an element, if it exists.
    fn get_render_state<P: Protocol>(&self, id: ElementId) -> Option<&RenderState<P>>;

    /// Registers an element that needs layout in the next frame.
    fn register_needs_layout(&mut self, id: ElementId);

    /// Registers an element that needs paint in the next frame.
    fn register_needs_paint(&mut self, id: ElementId);

    /// Registers an element that needs compositing bits update.
    fn register_needs_compositing_bits_update(&mut self, id: ElementId);

    /// Gets the RenderObject for an element to check `is_repaint_boundary`.
    fn is_repaint_boundary(&self, id: ElementId) -> bool;

    /// Gets the previous repaint boundary status (for transition detection).
    fn was_repaint_boundary(&self, id: ElementId) -> bool;
}

// ============================================================================
// RENDER STATE
// ============================================================================

/// Protocol-specific render state storage with Flutter-compliant dirty tracking.
#[derive(Debug)]
pub struct RenderState<P: Protocol> {
    flags: AtomicRenderFlags,
    geometry: OnceCell<P::Geometry>,
    constraints: OnceCell<P::Constraints>,
    offset: AtomicOffset,
    _phantom: PhantomData<P>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Creates a new render state with default dirty flags.
    pub fn new() -> Self {
        Self {
            flags: AtomicRenderFlags::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT),
            geometry: OnceCell::new(),
            constraints: OnceCell::new(),
            offset: AtomicOffset::new(Offset::ZERO),
            _phantom: PhantomData,
        }
    }

    /// Creates a render state with custom initial flags.
    pub fn with_flags(flags: RenderFlags) -> Self {
        Self {
            flags: AtomicRenderFlags::new(flags),
            geometry: OnceCell::new(),
            constraints: OnceCell::new(),
            offset: AtomicOffset::new(Offset::ZERO),
            _phantom: PhantomData,
        }
    }
}

impl<P: Protocol> Default for RenderState<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Clone for RenderState<P>
where
    P::Geometry: Clone,
    P::Constraints: Clone,
{
    fn clone(&self) -> Self {
        Self {
            flags: AtomicRenderFlags::new(self.flags.load()),
            geometry: self
                .geometry
                .get()
                .cloned()
                .map_or_else(OnceCell::new, |g| {
                    let cell = OnceCell::new();
                    let _ = cell.set(g);
                    cell
                }),
            constraints: self
                .constraints
                .get()
                .cloned()
                .map_or_else(OnceCell::new, |c| {
                    let cell = OnceCell::new();
                    let _ = cell.set(c);
                    cell
                }),
            offset: AtomicOffset::new(self.offset.load()),
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// FLUTTER-STYLE DIRTY TRACKING
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Marks this render object as needing layout (Flutter-compliant).
    pub fn mark_needs_layout(&self, element_id: ElementId, tree: &mut impl RenderDirtyPropagation) {
        if self.flags.needs_layout() {
            return;
        }

        self.flags.mark_needs_layout();

        if self.is_relayout_boundary() {
            tree.register_needs_layout(element_id);
        } else {
            let parent_id = tree.parent(element_id);
            if let Some(parent_id) = parent_id {
                if let Some(parent_state) = tree.get_render_state::<P>(parent_id) {
                    parent_state.flags.mark_needs_layout();
                    if parent_state.is_relayout_boundary() {
                        tree.register_needs_layout(parent_id);
                    } else {
                        let mut current = tree.parent(parent_id);
                        while let Some(curr_id) = current {
                            if let Some(state) = tree.get_render_state::<P>(curr_id) {
                                if state.flags.needs_layout() {
                                    break;
                                }
                                state.flags.mark_needs_layout();
                                if state.is_relayout_boundary() {
                                    tree.register_needs_layout(curr_id);
                                    break;
                                }
                            } else {
                                break;
                            }
                            current = tree.parent(curr_id);
                        }
                    }
                }
            }
        }
    }

    /// Marks this render object's parent as needing layout (for intrinsic changes).
    pub fn mark_parent_needs_layout(
        &self,
        element_id: ElementId,
        tree: &mut impl RenderDirtyPropagation,
    ) {
        self.flags.mark_needs_layout();

        let parent_id = tree.parent(element_id);
        if let Some(parent_id) = parent_id {
            let mut current = Some(parent_id);
            while let Some(curr_id) = current {
                if let Some(state) = tree.get_render_state::<P>(curr_id) {
                    if state.flags.needs_layout() {
                        break;
                    }
                    state.flags.mark_needs_layout();
                    if state.is_relayout_boundary() {
                        tree.register_needs_layout(curr_id);
                        break;
                    }
                } else {
                    break;
                }
                current = tree.parent(curr_id);
            }
        }
    }

    /// Marks this render object as needing paint (Flutter-compliant).
    pub fn mark_needs_paint(&self, element_id: ElementId, tree: &mut impl RenderDirtyPropagation) {
        if self.flags.needs_paint() {
            return;
        }

        self.flags.mark_needs_paint();

        if self.is_repaint_boundary() {
            tree.register_needs_paint(element_id);
        } else {
            let parent_id = tree.parent(element_id);
            if let Some(parent_id) = parent_id {
                let mut current = Some(parent_id);
                while let Some(curr_id) = current {
                    if let Some(state) = tree.get_render_state::<P>(curr_id) {
                        if state.flags.needs_paint() {
                            break;
                        }
                        state.flags.mark_needs_paint();
                        if state.is_repaint_boundary() {
                            tree.register_needs_paint(curr_id);
                            break;
                        }
                    } else {
                        break;
                    }
                    current = tree.parent(curr_id);
                }
            }
        }
    }

    /// Marks compositing as dirty.
    #[inline]
    pub fn mark_needs_compositing(&self) {
        self.flags.set(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Marks this render object as needing compositing bits update (Flutter-compliant).
    pub fn mark_needs_compositing_bits_update(
        &self,
        element_id: ElementId,
        tree: &mut impl RenderDirtyPropagation,
    ) {
        if self.flags.needs_compositing() {
            return;
        }

        self.flags.set(RenderFlags::NEEDS_COMPOSITING);

        if let Some(parent_id) = tree.parent(element_id) {
            if let Some(parent_state) = tree.get_render_state::<P>(parent_id) {
                if parent_state.flags.needs_compositing() {
                    return;
                }
            }

            let was_repaint_boundary = tree.was_repaint_boundary(element_id);
            let is_repaint_boundary = tree.is_repaint_boundary(element_id);
            let parent_is_repaint_boundary = tree.is_repaint_boundary(parent_id);

            let should_propagate =
                (!was_repaint_boundary || !is_repaint_boundary) && !parent_is_repaint_boundary;

            if should_propagate {
                let mut current = Some(parent_id);
                while let Some(curr_id) = current {
                    if let Some(state) = tree.get_render_state::<P>(curr_id) {
                        if state.flags.needs_compositing() {
                            break;
                        }
                        state.flags.set(RenderFlags::NEEDS_COMPOSITING);

                        let curr_is_repaint_boundary = tree.is_repaint_boundary(curr_id);
                        if curr_is_repaint_boundary {
                            tree.register_needs_compositing_bits_update(curr_id);
                            break;
                        }

                        if let Some(parent_id) = tree.parent(curr_id) {
                            if tree.is_repaint_boundary(parent_id) {
                                tree.register_needs_compositing_bits_update(curr_id);
                                break;
                            }
                        } else {
                            tree.register_needs_compositing_bits_update(curr_id);
                            break;
                        }
                    } else {
                        break;
                    }
                    current = tree.parent(curr_id);
                }
            } else {
                tree.register_needs_compositing_bits_update(element_id);
            }
        } else {
            tree.register_needs_compositing_bits_update(element_id);
        }
    }
}

// ============================================================================
// BASIC DIRTY FLAGS (LOCK-FREE)
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Returns a reference to the atomic render flags.
    #[inline]
    pub fn flags(&self) -> &AtomicRenderFlags {
        &self.flags
    }

    /// Checks if layout is needed (lock-free, O(1)).
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_LAYOUT)
    }

    /// Checks if paint is needed (lock-free, O(1)).
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_PAINT)
    }

    /// Checks if compositing is needed (lock-free, O(1)).
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.flags.contains(RenderFlags::NEEDS_COMPOSITING)
    }

    /// Clears the layout dirty flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(RenderFlags::NEEDS_LAYOUT);
    }

    /// Clears the paint dirty flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(RenderFlags::NEEDS_PAINT);
    }

    /// Clears the compositing dirty flag.
    #[inline]
    pub fn clear_needs_compositing(&self) {
        self.flags.remove(RenderFlags::NEEDS_COMPOSITING);
    }

    /// Clears all dirty flags.
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
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_RELAYOUT_BOUNDARY)
    }

    /// Checks if this render object is a repaint boundary.
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.flags.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Sets whether this render object is a relayout boundary.
    #[inline]
    pub fn set_relayout_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.flags.set(RenderFlags::IS_RELAYOUT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_RELAYOUT_BOUNDARY);
        }
    }

    /// Sets whether this render object is a repaint boundary.
    #[inline]
    pub fn set_repaint_boundary(&self, is_boundary: bool) {
        if is_boundary {
            self.flags.set(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }
}

// ============================================================================
// GEOMETRY
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the computed geometry (if available).
    pub fn geometry(&self) -> Option<P::Geometry>
    where
        P::Geometry: Copy,
    {
        self.geometry.get().copied()
    }

    /// Sets the computed geometry after layout.
    pub fn set_geometry(&self, geometry: P::Geometry) {
        if self.geometry.set(geometry).is_err() {
            panic!(
                "Geometry already set! Call clear_geometry() before relayout. \
                 This indicates a logic error in the layout code."
            );
        }
    }

    /// Clears the geometry to allow relayout.
    #[inline]
    pub fn clear_geometry(&mut self) {
        self.geometry = OnceCell::new();
    }
}

// ============================================================================
// CONSTRAINTS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the last constraints used for layout.
    pub fn constraints(&self) -> Option<&P::Constraints> {
        self.constraints.get()
    }

    /// Sets the constraints used for layout.
    pub fn set_constraints(&self, constraints: P::Constraints) {
        if self.constraints.set(constraints).is_err() {
            panic!(
                "Constraints already set! Call clear_constraints() before relayout. \
                 This indicates a logic error in the layout code."
            );
        }
    }

    /// Clears the constraints to allow relayout.
    #[inline]
    pub fn clear_constraints(&mut self) {
        self.constraints = OnceCell::new();
    }

    /// Checks if constraints match the given value.
    pub fn has_constraints(&self, constraints: &P::Constraints) -> bool
    where
        P::Constraints: PartialEq,
    {
        self.constraints
            .get()
            .map(|c| c == constraints)
            .unwrap_or(false)
    }
}

// ============================================================================
// OFFSET
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Gets the offset relative to parent (atomic, lock-free).
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset.load()
    }

    /// Sets the offset relative to parent (atomic, lock-free).
    #[inline]
    pub fn set_offset(&self, offset: Offset) {
        self.offset.store(offset);
    }
}

// ============================================================================
// SAFE PROTOCOL CASTING (No unsafe required)
// ============================================================================

/// Trait for safe protocol state conversion.
///
/// This trait enables safe downcasting of `RenderState<P>` to protocol-specific
/// types without unsafe pointer casts. It uses the sealed `ProtocolCast` trait
/// to ensure type safety at compile time.
pub trait RenderStateCast<P: Protocol> {
    /// Attempts to get this state as BoxRenderState.
    ///
    /// Returns `Some` only if `P` is `BoxProtocol`.
    fn try_as_box_state(&self) -> Option<&BoxRenderState>;

    /// Attempts to get this state as mutable BoxRenderState.
    ///
    /// Returns `Some` only if `P` is `BoxProtocol`.
    fn try_as_box_state_mut(&mut self) -> Option<&mut BoxRenderState>;

    /// Attempts to get this state as SliverRenderState.
    ///
    /// Returns `Some` only if `P` is `SliverProtocol`.
    fn try_as_sliver_state(&self) -> Option<&SliverRenderState>;

    /// Attempts to get this state as mutable SliverRenderState.
    ///
    /// Returns `Some` only if `P` is `SliverProtocol`.
    fn try_as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState>;
}

// Safe implementation for BoxProtocol - no unsafe needed!
impl RenderStateCast<BoxProtocol> for RenderState<BoxProtocol> {
    #[inline]
    fn try_as_box_state(&self) -> Option<&BoxRenderState> {
        Some(self) // Direct return - P is BoxProtocol
    }

    #[inline]
    fn try_as_box_state_mut(&mut self) -> Option<&mut BoxRenderState> {
        Some(self) // Direct return - P is BoxProtocol
    }

    #[inline]
    fn try_as_sliver_state(&self) -> Option<&SliverRenderState> {
        None // BoxProtocol is not SliverProtocol
    }

    #[inline]
    fn try_as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState> {
        None // BoxProtocol is not SliverProtocol
    }
}

// Safe implementation for SliverProtocol - no unsafe needed!
impl RenderStateCast<SliverProtocol> for RenderState<SliverProtocol> {
    #[inline]
    fn try_as_box_state(&self) -> Option<&BoxRenderState> {
        None // SliverProtocol is not BoxProtocol
    }

    #[inline]
    fn try_as_box_state_mut(&mut self) -> Option<&mut BoxRenderState> {
        None // SliverProtocol is not BoxProtocol
    }

    #[inline]
    fn try_as_sliver_state(&self) -> Option<&SliverRenderState> {
        Some(self) // Direct return - P is SliverProtocol
    }

    #[inline]
    fn try_as_sliver_state_mut(&mut self) -> Option<&mut SliverRenderState> {
        Some(self) // Direct return - P is SliverProtocol
    }
}

// ============================================================================
// BOX PROTOCOL CONVENIENCE METHODS
// ============================================================================

impl RenderState<BoxProtocol> {
    /// Returns `Size::ZERO` if geometry is not set.
    #[inline]
    pub fn size(&self) -> flui_types::Size {
        self.geometry().unwrap_or(flui_types::Size::ZERO)
    }

    /// Convenience method for setting size (box protocol).
    #[inline]
    pub fn set_size(&self, size: flui_types::Size) {
        self.set_geometry(size);
    }

    /// Checks if size matches the given value.
    #[inline]
    pub fn has_size(&self, size: flui_types::Size) -> bool {
        self.geometry().map(|s| s == size).unwrap_or(false)
    }
}

// ============================================================================
// SLIVER PROTOCOL CONVENIENCE METHODS
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

    /// Returns layout extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn layout_extent(&self) -> f32 {
        self.geometry().and_then(|g| g.layout_extent).unwrap_or(0.0)
    }

    /// Returns max paint extent, or 0.0 if geometry is not set.
    #[inline]
    pub fn max_paint_extent(&self) -> f32 {
        self.geometry()
            .and_then(|g| g.max_paint_extent)
            .unwrap_or(0.0)
    }

    /// Sets sliver geometry.
    #[inline]
    pub fn set_sliver_geometry(&self, geometry: flui_types::SliverGeometry) {
        self.set_geometry(geometry);
    }
}

// ============================================================================
// RENDER STATE EXTENSION TRAIT
// ============================================================================

/// Extension trait for `dyn Any` that provides typed access to render states.
pub trait RenderStateExt {
    /// Attempts to downcast to `BoxRenderState`.
    fn as_box_state(&self) -> Option<&BoxRenderState>;

    /// Attempts to downcast to `SliverRenderState`.
    fn as_sliver_state(&self) -> Option<&SliverRenderState>;

    /// Gets the offset from either Box or Sliver protocol.
    fn offset(&self) -> Option<Offset>;

    /// Checks if layout is needed.
    fn needs_layout(&self) -> bool;

    /// Checks if paint is needed.
    fn needs_paint(&self) -> bool;

    /// Gets the render flags.
    fn render_flags(&self) -> Option<&AtomicRenderFlags>;

    /// Clears the needs_layout flag.
    fn clear_needs_layout(&self);

    /// Clears the needs_paint flag.
    fn clear_needs_paint(&self);

    /// Gets Box geometry (Size), if available.
    fn box_geometry(&self) -> Option<flui_types::Size>;

    /// Gets Sliver geometry, if available.
    fn sliver_geometry(&self) -> Option<SliverGeometry>;
}

impl RenderStateExt for dyn std::any::Any + Send + Sync {
    #[inline]
    fn as_box_state(&self) -> Option<&BoxRenderState> {
        self.downcast_ref::<BoxRenderState>()
    }

    #[inline]
    fn as_sliver_state(&self) -> Option<&SliverRenderState> {
        self.downcast_ref::<SliverRenderState>()
    }

    fn offset(&self) -> Option<Offset> {
        if let Some(box_state) = self.as_box_state() {
            return Some(box_state.offset());
        }
        if let Some(sliver_state) = self.as_sliver_state() {
            return Some(sliver_state.offset());
        }
        None
    }

    #[inline]
    fn needs_layout(&self) -> bool {
        self.as_box_state()
            .map(|s| s.needs_layout())
            .unwrap_or(false)
    }

    #[inline]
    fn needs_paint(&self) -> bool {
        self.as_box_state()
            .map(|s| s.needs_paint())
            .unwrap_or(false)
    }

    #[inline]
    fn render_flags(&self) -> Option<&AtomicRenderFlags> {
        self.as_box_state().map(|s| s.flags())
    }

    fn clear_needs_layout(&self) {
        if let Some(box_state) = self.as_box_state() {
            box_state.clear_needs_layout();
        }
    }

    fn clear_needs_paint(&self) {
        if let Some(box_state) = self.as_box_state() {
            box_state.clear_needs_paint();
        }
    }

    #[inline]
    fn box_geometry(&self) -> Option<flui_types::Size> {
        self.as_box_state().and_then(|s| s.geometry())
    }

    #[inline]
    fn sliver_geometry(&self) -> Option<SliverGeometry> {
        self.as_sliver_state().and_then(|s| s.geometry())
    }
}

impl RenderStateExt for dyn std::any::Any {
    #[inline]
    fn as_box_state(&self) -> Option<&BoxRenderState> {
        self.downcast_ref::<BoxRenderState>()
    }

    #[inline]
    fn as_sliver_state(&self) -> Option<&SliverRenderState> {
        self.downcast_ref::<SliverRenderState>()
    }

    fn offset(&self) -> Option<Offset> {
        if let Some(box_state) = self.as_box_state() {
            return Some(box_state.offset());
        }
        if let Some(sliver_state) = self.as_sliver_state() {
            return Some(sliver_state.offset());
        }
        None
    }

    #[inline]
    fn needs_layout(&self) -> bool {
        self.as_box_state()
            .map(|s| s.needs_layout())
            .unwrap_or(false)
    }

    #[inline]
    fn needs_paint(&self) -> bool {
        self.as_box_state()
            .map(|s| s.needs_paint())
            .unwrap_or(false)
    }

    #[inline]
    fn render_flags(&self) -> Option<&AtomicRenderFlags> {
        self.as_box_state().map(|s| s.flags())
    }

    fn clear_needs_layout(&self) {
        if let Some(box_state) = self.as_box_state() {
            box_state.clear_needs_layout();
        }
    }

    fn clear_needs_paint(&self) {
        if let Some(box_state) = self.as_box_state() {
            box_state.clear_needs_paint();
        }
    }

    #[inline]
    fn box_geometry(&self) -> Option<flui_types::Size> {
        self.as_box_state().and_then(|s| s.geometry())
    }

    #[inline]
    fn sliver_geometry(&self) -> Option<SliverGeometry> {
        self.as_sliver_state().and_then(|s| s.geometry())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_write_once() {
        let state = BoxRenderState::new();
        let size1 = flui_types::Size::new(100.0, 50.0);
        let size2 = flui_types::Size::new(200.0, 100.0);

        state.set_geometry(size1);
        assert_eq!(state.geometry(), Some(size1));

        let result = std::panic::catch_unwind(|| {
            state.set_geometry(size2);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_offset() {
        let state = BoxRenderState::new();
        let offset = Offset::new(10.0, 20.0);

        state.set_offset(offset);
        assert_eq!(state.offset(), offset);

        let offset2 = Offset::new(30.0, 40.0);
        state.set_offset(offset2);
        assert_eq!(state.offset(), offset2);
    }

    #[test]
    fn test_boundary_flags() {
        let state = BoxRenderState::new();

        assert!(!state.is_relayout_boundary());
        assert!(!state.is_repaint_boundary());

        state.set_relayout_boundary(true);
        assert!(state.is_relayout_boundary());

        state.set_repaint_boundary(true);
        assert!(state.is_repaint_boundary());

        state.set_relayout_boundary(false);
        assert!(!state.is_relayout_boundary());
        assert!(state.is_repaint_boundary());
    }
}
