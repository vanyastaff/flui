//! RenderViewObject - Extension trait for render-specific ViewObject methods
//!
//! This trait extends ViewObject with render-specific operations.
//! Only RenderViewWrapper and RenderObjectWrapper implement this.
//!
//! # Tree Integration
//!
//! The trait is generic over `T: FullRenderTree`, providing type-safe
//! access to tree operations for layout, paint, and hit testing.

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::core::{render_tree::FullRenderTree, LayoutProtocol, RenderState, RuntimeArity};

/// Extension trait for ViewObjects that wrap render objects.
///
/// Provides access to:
/// - RenderState for cached size/offset
/// - Layout, paint, and hit test operations with tree access
///
/// # Type Parameters
///
/// - `T`: Tree type implementing `FullRenderTree` (LayoutTree + PaintTree + HitTestTree)
///
/// # Design
///
/// This is a separate trait (not part of base ViewObject) because:
/// 1. Only render views need these methods
/// 2. Keeps base ViewObject in flui-view without rendering dependencies
/// 3. Interface Segregation Principle
///
/// Having `T` at trait level (not method level) provides:
/// - dyn-compatibility for concrete tree types
/// - Better IDE support and error messages
/// - Consistent tree type across all methods
///
/// # Implementors
///
/// - `RenderViewWrapper<T, V, P, A>` - For RenderView implementations
/// - `RenderObjectWrapper<T, A, R>` - For raw RenderBox instances
pub trait RenderViewObject<T: FullRenderTree>: Send + 'static {
    /// Get the render state (cached size, offset, dirty flags).
    fn render_state(&self) -> &RenderState;

    /// Get mutable render state.
    fn render_state_mut(&mut self) -> &mut RenderState;

    /// Get layout protocol (Box or Sliver).
    fn protocol(&self) -> LayoutProtocol;

    /// Get arity specification.
    fn arity(&self) -> RuntimeArity;

    /// Perform layout computation using tree access.
    ///
    /// # Arguments
    ///
    /// - `tree`: Mutable reference to layout tree
    /// - `self_id`: This element's ID (for tree operations)
    /// - `children`: Child element IDs
    /// - `constraints`: Layout constraints
    ///
    /// # Returns
    ///
    /// Computed size.
    fn perform_layout(
        &mut self,
        tree: &mut T,
        self_id: ElementId,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size;

    /// Perform paint computation using tree access.
    ///
    /// # Arguments
    ///
    /// - `tree`: Mutable reference to paint tree
    /// - `self_id`: This element's ID
    /// - `children`: Child element IDs
    /// - `offset`: Paint offset
    fn perform_paint(
        &self,
        tree: &mut T,
        self_id: ElementId,
        children: &[ElementId],
        offset: Offset,
    );

    /// Perform hit testing using tree access.
    ///
    /// # Arguments
    ///
    /// - `tree`: Reference to hit test tree
    /// - `self_id`: This element's ID
    /// - `children`: Child element IDs
    /// - `position`: Hit test position
    /// - `result`: Hit test result accumulator
    ///
    /// # Returns
    ///
    /// `true` if hit, `false` otherwise.
    fn perform_hit_test(
        &self,
        tree: &T,
        self_id: ElementId,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool;
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Check if a protocol indicates a render view object.
pub fn is_render_protocol(protocol: LayoutProtocol) -> bool {
    matches!(protocol, LayoutProtocol::Box | LayoutProtocol::Sliver)
}
