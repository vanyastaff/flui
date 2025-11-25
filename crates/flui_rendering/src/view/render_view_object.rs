//! RenderViewObject - Extension trait for render-specific ViewObject methods
//!
//! This trait extends ViewObject with render-specific operations.
//! Only RenderViewWrapper and RenderObjectWrapper implement this.

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::core::{Geometry, LayoutProtocol, RenderObject, RenderState, RuntimeArity};

/// Extension trait for ViewObjects that wrap render objects.
///
/// Provides access to:
/// - RenderObject for layout/paint
/// - RenderState for cached size/offset
/// - Layout and paint operations
///
/// # Design
///
/// This is a separate trait (not part of base ViewObject) because:
/// 1. Only render views need these methods
/// 2. Keeps base ViewObject in flui-view without rendering dependencies
/// 3. Interface Segregation Principle
///
/// # Implementors
///
/// - `RenderViewWrapper<V, P, A>` - For RenderView implementations
/// - `RenderObjectWrapper` - For raw RenderObject instances
pub trait RenderViewObject: Send + 'static {
    /// Get the render object.
    ///
    /// Returns `None` if the render object hasn't been created yet
    /// (i.e., `create_render_object()` hasn't been called).
    fn render_object(&self) -> Option<&dyn RenderObject>;

    /// Get mutable render object.
    ///
    /// Returns `None` if the render object hasn't been created yet.
    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject>;

    /// Get the render state (cached size, offset, dirty flags).
    fn render_state(&self) -> &RenderState;

    /// Get mutable render state.
    fn render_state_mut(&mut self) -> &mut RenderState;

    /// Get layout protocol (Box or Sliver).
    fn protocol(&self) -> LayoutProtocol;

    /// Get arity specification.
    fn arity(&self) -> RuntimeArity;

    /// Perform layout computation.
    ///
    /// # Arguments
    ///
    /// - `children`: Child element IDs
    /// - `constraints`: Layout constraints
    /// - `layout_child`: Callback to layout children
    ///
    /// # Returns
    ///
    /// Computed size.
    fn perform_layout(
        &mut self,
        children: &[ElementId],
        constraints: BoxConstraints,
        layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
    ) -> Size;

    /// Perform paint computation.
    ///
    /// # Arguments
    ///
    /// - `children`: Child element IDs
    /// - `offset`: Paint offset
    /// - `paint_child`: Callback to paint children
    ///
    /// # Returns
    ///
    /// Canvas with painted content.
    fn perform_paint(
        &self,
        children: &[ElementId],
        offset: Offset,
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas;

    /// Perform hit testing.
    ///
    /// # Arguments
    ///
    /// - `children`: Child element IDs
    /// - `position`: Hit test position
    /// - `geometry`: Element geometry
    /// - `hit_test_child`: Callback to hit test children
    ///
    /// # Returns
    ///
    /// `true` if hit, `false` otherwise.
    fn perform_hit_test(
        &self,
        children: &[ElementId],
        position: Offset,
        geometry: &Geometry,
        hit_test_child: &mut dyn FnMut(ElementId, Offset) -> bool,
    ) -> bool;
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Check if a protocol indicates a render view object.
pub fn is_render_protocol(protocol: LayoutProtocol) -> bool {
    matches!(protocol, LayoutProtocol::Box | LayoutProtocol::Sliver)
}
