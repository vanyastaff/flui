//! RenderObjectWrapper - Generic wrapper for RenderBox instances
//!
//! Used when you have a RenderBox directly without a RenderView.
//! This is a generic wrapper that preserves the concrete type for
//! efficient Context-based layout/paint operations.

use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::core::{
    arity::Arity,
    contexts::{LayoutContext, PaintContext},
    geometry::Geometry,
    FullRenderTree, LayoutProtocol, LayoutTree, PaintTree, RenderBox, RenderState, RuntimeArity,
};

use super::RenderViewObject;

/// Generic wrapper that holds a RenderBox with known arity.
///
/// Use this when you have a RenderBox instance directly,
/// rather than going through RenderView.
///
/// # Type Parameters
///
/// - `A`: Arity (Leaf, Single, Optional, Variable)
/// - `R`: The concrete RenderBox type (must implement `RenderBox<T, A>` for some T)
///
/// # Note
///
/// The tree type `T` is not part of the struct definition - it's only constrained
/// in the `RenderViewObject` implementation. This allows creating wrappers without
/// knowing the concrete tree type upfront.
///
/// # Example
///
/// ```rust,ignore
/// let render_box = MyRenderBox::new();
/// let wrapper = RenderObjectWrapper::new(render_box, RuntimeArity::Exact(0));
/// ```
pub struct RenderObjectWrapper<A, R>
where
    A: Arity,
    R: Send + Sync + std::fmt::Debug + 'static,
{
    /// The render object (concrete type, not boxed)
    render_object: R,

    /// Render state (cached geometry, dirty flags)
    render_state: RenderState,

    /// Arity specification (runtime mirror of compile-time A)
    arity: RuntimeArity,

    /// Phantom for arity type
    _phantom: PhantomData<A>,
}

impl<A, R> RenderObjectWrapper<A, R>
where
    A: Arity,
    R: Send + Sync + std::fmt::Debug + 'static,
{
    /// Create a new wrapper for a RenderBox.
    ///
    /// # Arguments
    ///
    /// * `render` - The RenderBox instance
    /// * `arity` - Runtime arity specification (should match compile-time A)
    pub fn new(render: R, arity: RuntimeArity) -> Self {
        Self {
            render_object: render,
            render_state: RenderState::new(),
            arity,
            _phantom: PhantomData,
        }
    }

    /// Get reference to the inner render object.
    pub fn inner(&self) -> &R {
        &self.render_object
    }

    /// Get mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.render_object
    }

    /// Downcast render object to concrete type (for inspection).
    pub fn downcast_ref<U: 'static>(&self) -> Option<&U> {
        (&self.render_object as &dyn std::any::Any).downcast_ref::<U>()
    }

    /// Downcast render object to concrete type (mutable).
    pub fn downcast_mut<U: 'static>(&mut self) -> Option<&mut U> {
        (&mut self.render_object as &mut dyn std::any::Any).downcast_mut::<U>()
    }
}

impl<A, R> std::fmt::Debug for RenderObjectWrapper<A, R>
where
    A: Arity,
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderObjectWrapper")
            .field("arity", &self.arity)
            .field("render_object", &self.render_object)
            .finish()
    }
}

// ============================================================================
// RenderViewObject IMPLEMENTATION
// ============================================================================

use crate::core::contexts::HitTestContext;
use flui_interaction::HitTestResult;

impl<T, A, R> RenderViewObject<T> for RenderObjectWrapper<A, R>
where
    T: FullRenderTree,
    A: Arity + 'static,
    R: RenderBox<T, A> + 'static,
{
    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn protocol(&self) -> LayoutProtocol {
        LayoutProtocol::Box
    }

    fn arity(&self) -> RuntimeArity {
        self.arity
    }

    fn perform_layout(
        &mut self,
        tree: &mut T,
        _self_id: ElementId,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        // Create arity-aware children accessor
        let children_accessor = A::from_slice(children);

        // Create layout context
        let ctx = LayoutContext::new(tree, constraints, children_accessor);

        // Call the RenderBox layout
        let size = self.render_object.layout(ctx);

        // Cache the geometry
        let geometry = Geometry::Box(size);
        *self.render_state.geometry.write() = Some(geometry);
        self.render_state.clear_needs_layout();

        size
    }

    fn perform_paint(
        &self,
        tree: &mut T,
        _self_id: ElementId,
        children: &[ElementId],
        offset: Offset,
    ) {
        // Create arity-aware children accessor
        let children_accessor = A::from_slice(children);

        // Create paint context
        let mut ctx = PaintContext::new(tree, offset, children_accessor);

        // Call the RenderBox paint
        self.render_object.paint(&mut ctx);
    }

    fn perform_hit_test(
        &self,
        tree: &T,
        self_id: ElementId,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        // Get cached geometry for size
        let size = self
            .render_state
            .geometry()
            .and_then(|g| g.try_as_box())
            .unwrap_or(Size::ZERO);

        // Create arity-aware children accessor
        let children_accessor = A::from_slice(children);

        // Create hit test context (order: tree, position, geometry, element_id, children)
        let ctx = HitTestContext::new(tree, position, size, self_id, children_accessor);

        // Call the RenderBox hit_test
        self.render_object.hit_test(&ctx, result)
    }
}

// ============================================================================
// ViewObject IMPLEMENTATION
// ============================================================================

use flui_element::{BuildContext, Element, ViewMode, ViewObject};

impl<A, R> ViewObject for RenderObjectWrapper<A, R>
where
    A: Arity + 'static,
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn mode(&self) -> ViewMode {
        ViewMode::RenderBox
    }

    fn build(&mut self, _ctx: &dyn BuildContext) -> Element {
        // Render objects don't build children - they just render
        // Children are managed by the framework
        Element::empty()
    }

    fn render_state(&self) -> Option<&dyn std::any::Any> {
        Some(&self.render_state)
    }

    fn render_state_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(&mut self.render_state)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::Leaf;
    use crate::core::protocol::BoxProtocol;

    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
    }

    // Note: Tests for RenderBox<T, A> require a concrete FullRenderTree implementation.
    // Basic wrapper tests work without RenderBox bound since struct only requires Send+Sync+Debug.

    #[test]
    fn test_wrapper_creation() {
        let wrapper = RenderObjectWrapper::<Leaf, _>::new(
            TestRenderBox {
                size: Size::new(100.0, 50.0),
            },
            RuntimeArity::Exact(0),
        );

        assert_eq!(wrapper.arity, RuntimeArity::Exact(0));
    }

    #[test]
    fn test_inner_access() {
        let wrapper = RenderObjectWrapper::<Leaf, _>::new(
            TestRenderBox {
                size: Size::new(100.0, 50.0),
            },
            RuntimeArity::Exact(0),
        );

        assert_eq!(wrapper.inner().size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_downcast() {
        let wrapper = RenderObjectWrapper::<Leaf, _>::new(
            TestRenderBox {
                size: Size::new(100.0, 50.0),
            },
            RuntimeArity::Exact(0),
        );

        let downcasted = wrapper.downcast_ref::<TestRenderBox>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().size, Size::new(100.0, 50.0));
    }
}
