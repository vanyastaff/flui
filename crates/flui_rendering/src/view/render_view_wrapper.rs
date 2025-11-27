//! RenderViewWrapper - Wrapper for RenderView implementations
//!
//! Wraps a RenderView and its created RenderObject, implementing RenderViewObject.

use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::core::{
    arity::{Arity, Leaf, Optional, Single, Variable},
    contexts::{HitTestContext, LayoutContext, PaintContext},
    geometry::Geometry,
    protocol::{BoxProtocol, Protocol},
    render_tree::{HitTestTree, LayoutTree, PaintTree},
    LayoutProtocol, RenderBox, RenderState, RuntimeArity,
};

use super::{RenderView, RenderViewObject, UpdateResult};

// ============================================================================
// ArityToRuntime - Convert compile-time Arity to RuntimeArity
// ============================================================================

/// Helper trait to convert compile-time Arity to RuntimeArity
pub trait ArityToRuntime: Arity {
    /// Convert to runtime arity representation
    fn to_runtime() -> RuntimeArity;
}

impl ArityToRuntime for Leaf {
    fn to_runtime() -> RuntimeArity {
        RuntimeArity::Exact(0)
    }
}

impl ArityToRuntime for Single {
    fn to_runtime() -> RuntimeArity {
        RuntimeArity::Exact(1)
    }
}

impl ArityToRuntime for Optional {
    fn to_runtime() -> RuntimeArity {
        RuntimeArity::Optional
    }
}

impl ArityToRuntime for Variable {
    fn to_runtime() -> RuntimeArity {
        RuntimeArity::Variable
    }
}

// ============================================================================
// RenderViewWrapper
// ============================================================================

/// Wrapper for RenderView that implements RenderViewObject.
///
/// Manages the render object lifecycle and state.
///
/// # Type Parameters
///
/// - `V`: The RenderView type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
/// - `A`: Arity (Leaf, Single, Optional, Variable)
pub struct RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// The view configuration
    view: V,

    /// The render object created from view
    render_object: Option<V::RenderObject>,

    /// Render state (layout flags, geometry)
    render_state: RenderState,

    /// Type markers
    _protocol: PhantomData<P>,
    _arity: PhantomData<A>,
}

impl<V, P, A> RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// Create a new wrapper.
    pub fn new(view: V) -> Self {
        Self {
            view,
            render_object: None,
            render_state: RenderState::new(),
            _protocol: PhantomData,
            _arity: PhantomData,
        }
    }

    /// Get reference to view configuration.
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get mutable reference to view configuration.
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    /// Check if render object has been created.
    pub fn has_render_object(&self) -> bool {
        self.render_object.is_some()
    }

    /// Create the render object (called on first mount).
    pub fn create_render_object(&mut self) {
        if self.render_object.is_none() {
            self.render_object = Some(self.view.create());
            self.render_state.mark_needs_layout();
            self.render_state.mark_needs_paint();
        }
    }

    /// Update the render object with new view configuration.
    ///
    /// Returns the update result.
    pub fn update_render_object(&mut self) -> UpdateResult {
        if let Some(ref mut render) = self.render_object {
            let result = self.view.update(render);
            match result {
                UpdateResult::Unchanged => {}
                UpdateResult::NeedsLayout => {
                    self.render_state.mark_needs_layout();
                }
                UpdateResult::NeedsPaint => {
                    self.render_state.mark_needs_paint();
                }
            }
            result
        } else {
            UpdateResult::Unchanged
        }
    }

    /// Dispose of the render object.
    pub fn dispose_render_object(&mut self) {
        if let Some(ref mut render) = self.render_object {
            self.view.dispose(render);
        }
        self.render_object = None;
    }

    /// Get typed reference to render object.
    pub fn typed_render_object(&self) -> Option<&V::RenderObject> {
        self.render_object.as_ref()
    }

    /// Get typed mutable reference to render object.
    pub fn typed_render_object_mut(&mut self) -> Option<&mut V::RenderObject> {
        self.render_object.as_mut()
    }
}

impl<V, P, A> std::fmt::Debug for RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderViewWrapper")
            .field("has_render_object", &self.render_object.is_some())
            .field("needs_layout", &self.render_state.needs_layout())
            .field("needs_paint", &self.render_state.needs_paint())
            .finish()
    }
}

// ============================================================================
// RenderViewObject IMPLEMENTATION for BoxProtocol
// ============================================================================

impl<V, A> RenderViewObject for RenderViewWrapper<V, BoxProtocol, A>
where
    V: RenderView<BoxProtocol, A>,
    V::RenderObject: RenderBox<A>,
    A: ArityToRuntime + 'static,
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
        A::to_runtime()
    }

    fn perform_layout<T: LayoutTree>(
        &mut self,
        tree: &mut T,
        _self_id: ElementId,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        let Some(render) = self.render_object.as_mut() else {
            tracing::warn!("perform_layout called before create_render_object");
            return Size::ZERO;
        };

        // Create arity-aware children accessor
        let children_accessor = A::from_slice(children);

        // Create layout context
        let ctx = LayoutContext::new(tree, constraints, children_accessor);

        // Call the RenderBox layout
        let size = render.layout(ctx);

        // Cache the geometry
        let geometry = Geometry::Box(size);
        *self.render_state.geometry.write() = Some(geometry);
        self.render_state.clear_needs_layout();

        size
    }

    fn perform_paint<T: PaintTree>(
        &self,
        tree: &mut T,
        _self_id: ElementId,
        children: &[ElementId],
        offset: Offset,
    ) {
        let Some(render) = self.render_object.as_ref() else {
            tracing::warn!("perform_paint called before create_render_object");
            return;
        };

        // Create arity-aware children accessor
        let children_accessor = A::from_slice(children);

        // Create paint context
        let mut ctx = PaintContext::new(tree, offset, children_accessor);

        // Call the RenderBox paint
        render.paint(&mut ctx);
    }

    fn perform_hit_test<T: HitTestTree>(
        &self,
        tree: &T,
        self_id: ElementId,
        children: &[ElementId],
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        let Some(render) = self.render_object.as_ref() else {
            tracing::warn!("perform_hit_test called before create_render_object");
            return false;
        };

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
        render.hit_test(&ctx, result)
    }
}

// ============================================================================
// ViewObject IMPLEMENTATION
// ============================================================================

use flui_element::{BuildContext, Element, ViewMode, ViewObject};

impl<V, A> ViewObject for RenderViewWrapper<V, BoxProtocol, A>
where
    V: RenderView<BoxProtocol, A>,
    V::RenderObject: RenderBox<A>,
    A: ArityToRuntime + 'static,
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

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
    }

    impl RenderBox<Leaf> for TestRenderBox {
        fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
        where
            T: LayoutTree,
        {
            ctx.constraints.constrain(self.size)
        }

        fn paint<T>(&self, _ctx: &mut PaintContext<'_, T, Leaf>)
        where
            T: PaintTree,
        {
            // No painting needed for test
        }
    }

    struct TestRenderView {
        size: Size,
    }

    impl RenderView<BoxProtocol, Leaf> for TestRenderView {
        type RenderObject = TestRenderBox;

        fn create(&self) -> TestRenderBox {
            TestRenderBox { size: self.size }
        }

        fn update(&self, render: &mut TestRenderBox) -> UpdateResult {
            if render.size != self.size {
                render.size = self.size;
                UpdateResult::NeedsLayout
            } else {
                UpdateResult::Unchanged
            }
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = RenderViewWrapper::<TestRenderView, BoxProtocol, Leaf>::new(TestRenderView {
            size: Size::new(100.0, 50.0),
        });

        assert!(!wrapper.has_render_object());
    }

    #[test]
    fn test_create_render_object() {
        let mut wrapper =
            RenderViewWrapper::<TestRenderView, BoxProtocol, Leaf>::new(TestRenderView {
                size: Size::new(100.0, 50.0),
            });

        wrapper.create_render_object();

        assert!(wrapper.has_render_object());
        assert!(wrapper.render_state().needs_layout());
        assert!(wrapper.render_state().needs_paint());
    }

    // Note: perform_layout test requires a mock LayoutTree implementation.
    // The actual integration is tested in flui_core with ElementTree.
}
