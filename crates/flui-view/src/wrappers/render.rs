//! `RenderViewWrapper` - Wrapper that holds a `RenderView`
//!
//! Implements `ViewObject` for `RenderView` types, enabling render views
//! to be used in the element tree alongside component views.

use std::any::Any;

use flui_rendering::core::{
    arity::{Arity, Leaf},
    protocol::{BoxProtocol, Protocol},
    RenderObject,
};

use crate::traits::{RenderObjectFor, RenderView};
use crate::{BuildContext, IntoView, ViewMode, ViewObject};

/// Wrapper for `RenderView` that implements `ViewObject`
///
/// This wrapper bridges render views (which create RenderObjects) with
/// the view/element system. It stores the view configuration and the
/// created render object.
///
/// # Type Parameters
///
/// - `V`: The RenderView type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
/// - `A`: Arity (Leaf, Single, Optional, Variable)
pub struct RenderViewWrapper<V, P: Protocol = BoxProtocol, A: Arity = Leaf>
where
    V: RenderView<P, A>,
{
    /// The view configuration (consumed on create)
    view: Option<V>,
    /// The created render object
    render_object: Option<Box<dyn RenderObject>>,
    /// Protocol marker
    _protocol: std::marker::PhantomData<P>,
    /// Arity marker
    _arity: std::marker::PhantomData<A>,
}

impl<V, P, A> RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// Create a new wrapper with the view configuration
    pub fn new(view: V) -> Self {
        Self {
            view: Some(view),
            render_object: None,
            _protocol: std::marker::PhantomData,
            _arity: std::marker::PhantomData,
        }
    }

    /// Get the view configuration (if not yet consumed)
    pub fn view(&self) -> Option<&V> {
        self.view.as_ref()
    }

    /// Get the render object (if created)
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable render object (if created)
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object.as_mut().map(|r| r.as_mut())
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
            .field("has_view", &self.view.is_some())
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

impl<V, P, A> ViewObject for RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol + 'static,
    A: Arity + 'static,
    V::RenderObject: RenderObjectFor<P, A> + 'static,
{
    #[inline]
    fn mode(&self) -> ViewMode {
        // Determine mode based on protocol
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            ViewMode::RenderBox
        } else {
            ViewMode::RenderSliver
        }
    }

    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Render views don't build children through ViewObject::build()
        // Instead, they create RenderObjects which are stored separately
        //
        // If we haven't created the render object yet, do it now
        if self.render_object.is_none() {
            if let Some(view) = &self.view {
                let render_obj = view.create();
                self.render_object = Some(Box::new(render_obj));
            }
        }

        // Return None - render views don't have view children
        None
    }

    fn render_state(&self) -> Option<&dyn Any> {
        self.render_object.as_ref().map(|r| r.as_any())
    }

    fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        self.render_object.as_mut().map(|r| r.as_any_mut())
    }

    #[inline]
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to convert `RenderView` into `ViewObject`
///
/// Use `Render(my_render_view)` to create a view object from a render view.
#[derive(Debug)]
pub struct Render<V, P: Protocol = BoxProtocol, A: Arity = Leaf>(
    pub V,
    std::marker::PhantomData<P>,
    std::marker::PhantomData<A>,
)
where
    V: RenderView<P, A>;

impl<V, P, A> Render<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// Create a new Render wrapper
    pub fn new(view: V) -> Self {
        Self(view, std::marker::PhantomData, std::marker::PhantomData)
    }
}

impl<V, P, A> IntoView for Render<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol + 'static,
    A: Arity + 'static,
    V::RenderObject: RenderObjectFor<P, A> + 'static,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(RenderViewWrapper::new(self.0))
    }
}

/// Convenience: RenderViewWrapper itself implements IntoView
impl<V, P, A> IntoView for RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol + 'static,
    A: Arity + 'static,
    V::RenderObject: RenderObjectFor<P, A> + 'static,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(self)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use crate::traits::UpdateResult;
    use flui_foundation::ElementId;
    use flui_rendering::core::arity::Leaf;
    use flui_rendering::core::protocol::BoxProtocol;
    use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, RenderResult};
    use flui_types::Size;

    // Test render object
    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    impl RenderObject for TestRenderObject {}

    impl RenderBox<Leaf> for TestRenderObject {
        fn layout(&mut self, ctx: BoxLayoutCtx<'_, Leaf>) -> RenderResult<Size> {
            Ok(ctx.constraints.constrain(Size::new(100.0, 50.0)))
        }

        fn paint(&self, _ctx: &mut BoxPaintCtx<'_, Leaf>) {
            // No-op for test
        }
    }

    // Test render view
    struct TestRenderView {
        value: i32,
    }

    impl RenderView<BoxProtocol, Leaf> for TestRenderView {
        type RenderObject = TestRenderObject;

        fn create(&self) -> TestRenderObject {
            TestRenderObject { value: self.value }
        }

        fn update(&self, render: &mut TestRenderObject) -> UpdateResult {
            if render.value != self.value {
                render.value = self.value;
                UpdateResult::NeedsLayout
            } else {
                UpdateResult::Unchanged
            }
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = RenderViewWrapper::new(TestRenderView { value: 42 });
        assert!(wrapper.view.is_some());
        assert!(wrapper.render_object.is_none());
        assert_eq!(wrapper.mode(), ViewMode::RenderBox);
    }

    #[test]
    fn test_build_creates_render_object() {
        let mut wrapper = RenderViewWrapper::new(TestRenderView { value: 42 });
        let ctx = MockBuildContext::new(ElementId::new(1));

        // Build should create render object and return None
        let result = wrapper.build(&ctx);
        assert!(result.is_none());
        assert!(wrapper.render_object.is_some());

        // Check render object was created correctly
        let render_obj = wrapper.render_object().unwrap();
        let test_obj = render_obj.as_any().downcast_ref::<TestRenderObject>();
        assert!(test_obj.is_some());
        assert_eq!(test_obj.unwrap().value, 42);
    }

    #[test]
    fn test_render_state_access() {
        let mut wrapper = RenderViewWrapper::new(TestRenderView { value: 42 });
        let ctx = MockBuildContext::new(ElementId::new(1));

        // Before build, no render state
        assert!(wrapper.render_state().is_none());

        // After build, render state available
        wrapper.build(&ctx);
        assert!(wrapper.render_state().is_some());
        assert!(wrapper.render_state_mut().is_some());
    }

    #[test]
    fn test_into_view() {
        let view = TestRenderView { value: 42 };
        let view_obj = Render::new(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::RenderBox);
    }
}
