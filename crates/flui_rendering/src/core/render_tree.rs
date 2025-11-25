//! Render tree traits for layout and paint operations.
//!
//! These traits extend the basic tree traits from `flui-tree` with
//! render-specific functionality that requires concrete types.
//!
//! # Trait Hierarchy
//!
//! ```text
//! flui-tree (type-erased, no concrete types):
//!     TreeNav
//!         └── RenderTreeAccess (dyn Any access to render objects)
//!             └── DirtyTracking (needs_layout/needs_paint)
//!
//! flui-rendering (this module, concrete types):
//!     RenderTreeAccess + RenderState
//!         ├── LayoutTree (Size, BoxConstraints, SliverConstraints)
//!         ├── PaintTree (Canvas, Offset)
//!         └── HitTestTree (HitTestResult)
//! ```
//!
//! # Re-exports from flui-tree
//!
//! This module re-exports the base traits from `flui-tree`:
//! - [`RenderTreeAccess`] - Type-erased access via `dyn Any`
//! - [`RenderTreeAccessExt`] - Typed access via downcasting
//! - [`DirtyTracking`] - Layout/paint dirty flag management
//! - [`DirtyTrackingExt`] - Extended dirty tracking operations
//! - [`AtomicDirtyFlags`] - Lock-free atomic dirty flags

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

use crate::core::{BoxConstraints, Geometry, RenderState};
use crate::error::RenderError;

// Re-export base traits from flui-tree
pub use flui_tree::{
    AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt, RenderTreeAccess, RenderTreeAccessExt,
};

/// Layout operations on the render tree.
///
/// This trait extends [`DirtyTracking`] with concrete layout operations
/// that use FLUI's specific types (`Size`, `BoxConstraints`, etc.).
///
/// # Implementation Note
///
/// Implementors must also implement [`RenderTreeAccess`] from `flui-tree`.
/// The typed access methods use [`RenderTreeAccessExt`] for downcasting.
pub trait LayoutTree: DirtyTracking + RenderTreeAccessExt {
    /// Perform layout on an element with box constraints.
    ///
    /// Returns the computed size.
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError>;

    /// Perform layout on an element with sliver constraints.
    ///
    /// Returns the computed sliver geometry.
    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError>;

    /// Layout a child element (called from RenderObject::layout).
    ///
    /// This is the method render objects use to layout their children.
    fn layout_child(
        &mut self,
        child: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError> {
        self.perform_layout(child, constraints)
    }

    /// Layout a sliver child element.
    fn layout_sliver_child(
        &mut self,
        child: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        self.perform_sliver_layout(child, constraints)
    }

    /// Get the cached size of an element.
    ///
    /// Uses [`RenderTreeAccessExt::render_state_typed`] to downcast to [`RenderState`].
    fn get_size(&self, id: ElementId) -> Option<Size> {
        self.render_state_typed::<RenderState>(id)?
            .geometry()
            .and_then(|g| g.try_as_box())
    }

    /// Get the cached geometry of an element.
    ///
    /// Uses [`RenderTreeAccessExt::render_state_typed`] to downcast to [`RenderState`].
    fn get_geometry(&self, id: ElementId) -> Option<Geometry> {
        self.render_state_typed::<RenderState>(id)?.geometry()
    }

    /// Set the offset of an element (position relative to parent).
    fn set_offset(&mut self, id: ElementId, offset: Offset);

    /// Get the offset of an element.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;
}

/// Paint operations on the render tree.
pub trait PaintTree: RenderTreeAccess {
    /// Perform paint on an element.
    ///
    /// Returns the canvas with all drawing operations.
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> Result<Canvas, RenderError>;

    /// Paint a child element (called from RenderObject::paint).
    ///
    /// This appends the child's canvas to the parent's canvas.
    fn paint_child(&mut self, child: ElementId, offset: Offset) -> Result<Canvas, RenderError> {
        self.perform_paint(child, offset)
    }
}

/// Hit testing operations on the render tree.
pub trait HitTestTree: RenderTreeAccess {
    /// Perform hit test on an element.
    ///
    /// Returns true if the element or any child was hit.
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool;

    /// Hit test a child element.
    fn hit_test_child(
        &self,
        child: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        self.hit_test(child, position, result)
    }
}

/// Combined trait for full render tree functionality.
///
/// This is a convenience trait that combines all render tree operations.
pub trait FullRenderTree: LayoutTree + PaintTree + HitTestTree {}

// Blanket implementation
impl<T> FullRenderTree for T where T: LayoutTree + PaintTree + HitTestTree {}
