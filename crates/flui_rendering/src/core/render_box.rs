//! Unified RenderBox - single generic type for all RenderObjects
//!
//! This replaces LeafRenderBox, SingleRenderBox, and ContainerRenderBox with a single
//! generic type. Children are accessed via RenderContext, so no child/children fields needed.
//! Also eliminates RenderBoxMixin trait - all methods implemented directly.

use flui_types::constraints::BoxConstraints;
use super::{RenderState, RenderFlags};

/// Unified generic RenderBox
///
/// After migrating to RenderContext pattern, RenderObjects no longer store children directly.
/// Children are accessed via `ctx.children()`, `ctx.layout_child()`, `ctx.paint_child()`.
///
/// This eliminates the need for:
/// - `child: Option<BoxedRenderObject>` (was always None after RenderContext migration)
/// - `children: Vec<BoxedRenderObject>` (was always empty after RenderContext migration)
/// - Three separate types (LeafRenderBox, SingleRenderBox, ContainerRenderBox)
/// - RenderBoxMixin trait (methods now directly on RenderBox<T>)
///
/// # Type Parameter
///
/// - `T`: The data specific to this RenderObject type
///
/// # Memory savings
///
/// Removes 16-24 bytes per RenderObject (size of empty Vec/Option fields)
///
/// # Examples
///
/// ```rust,ignore
/// // Leaf RenderObject (no children)
/// let paragraph = RenderBox::new(ParagraphData { ... });
///
/// // Single-child RenderObject
/// let padding = RenderBox::new(PaddingData { ... });
///
/// // Multi-child RenderObject
/// let flex = RenderBox::new(FlexData { ... });
/// ```
#[derive(Debug)]
pub struct RenderBox<T> {
    /// Shared state (size, constraints, flags)
    pub state: RenderState,

    /// Type-specific data
    pub data: T,
}

impl<T> RenderBox<T> {
    /// Create a new RenderBox
    pub fn new(data: T) -> Self {
        Self {
            state: RenderState::new(),
            data,
        }
    }

    // ===== Data access =====

    /// Get reference to type-specific data
    #[inline]
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get mutable reference to type-specific data
    #[inline]
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    // ===== State access =====
    // These replace the RenderBoxMixin trait methods

    /// Get immutable reference to the shared state
    #[inline]
    pub fn state(&self) -> &RenderState {
        &self.state
    }

    /// Get mutable reference to the shared state
    #[inline]
    pub fn state_mut(&mut self) -> &mut RenderState {
        &mut self.state
    }

    // ===== Helper methods =====
    // These were previously provided by RenderBoxMixin trait

    /// Get the constraints used in the last layout
    ///
    /// Returns the constraints from the last layout pass.
    /// Returns `None` if layout hasn't been performed yet.
    #[inline]
    pub fn constraints(&self) -> Option<BoxConstraints> {
        *self.state.constraints.lock()
    }

    /// Get the dirty state flags
    ///
    /// Returns the full bitflags with all dirty state information.
    /// Use this when you need to check multiple flags at once.
    #[inline]
    pub fn flags(&self) -> RenderFlags {
        *self.state.flags.lock()
    }

    /// Check if layout has been performed
    ///
    /// Returns `true` if this RenderObject has been laid out at least once.
    #[inline]
    pub fn has_size(&self) -> bool {
        self.state.has_size()
    }

    /// Mark as repaint boundary
    ///
    /// A repaint boundary isolates painting - when this object needs repaint,
    /// it doesn't cause ancestors to repaint. Useful for expensive widgets.
    #[inline]
    pub fn set_is_repaint_boundary(&self, value: bool) {
        let mut flags = self.state.flags.lock();
        if value {
            flags.insert(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Check if this is a repaint boundary
    #[inline]
    pub fn is_repaint_boundary(&self) -> bool {
        self.state.flags.lock().contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Clear the needs_layout flag
    ///
    /// Called after layout is performed.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.state.clear_needs_layout();
    }

    /// Clear the needs_paint flag
    ///
    /// Called after painting is performed.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.state.clear_needs_paint();
    }
}
