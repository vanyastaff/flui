//! RenderObjectWrapper - Wrapper for raw RenderObject instances
//!
//! Used when you have a RenderObject directly without a RenderView.

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::core::{
    geometry::{Constraints, Geometry},
    LayoutProtocol, RenderObject, RenderState, RuntimeArity,
};

use super::RenderViewObject;

/// Wrapper that holds a raw RenderObject.
///
/// Use this when you have a RenderObject instance directly,
/// rather than going through RenderView.
///
/// # Example
///
/// ```rust,ignore
/// let render_box = MyRenderBox::new();
/// let wrapper = RenderObjectWrapper::new_box(render_box, RuntimeArity::Exact(0));
/// ```
pub struct RenderObjectWrapper {
    /// The render object
    render_object: Box<dyn RenderObject>,

    /// Render state (cached geometry, dirty flags)
    render_state: RenderState,

    /// Layout protocol
    protocol: LayoutProtocol,

    /// Arity specification
    arity: RuntimeArity,
}

impl RenderObjectWrapper {
    /// Create a new wrapper for a box protocol render object.
    pub fn new_box<R: RenderObject + 'static>(render: R, arity: RuntimeArity) -> Self {
        Self {
            render_object: Box::new(render),
            render_state: RenderState::new(),
            protocol: LayoutProtocol::Box,
            arity,
        }
    }

    /// Create a new wrapper for a sliver protocol render object.
    pub fn new_sliver<R: RenderObject + 'static>(render: R, arity: RuntimeArity) -> Self {
        Self {
            render_object: Box::new(render),
            render_state: RenderState::new(),
            protocol: LayoutProtocol::Sliver,
            arity,
        }
    }

    /// Create from boxed render object.
    pub fn from_boxed(
        render_object: Box<dyn RenderObject>,
        protocol: LayoutProtocol,
        arity: RuntimeArity,
    ) -> Self {
        Self {
            render_object,
            render_state: RenderState::new(),
            protocol,
            arity,
        }
    }

    /// Get the debug name.
    pub fn debug_name(&self) -> &'static str {
        self.render_object.debug_name()
    }

    /// Downcast render object to concrete type.
    pub fn downcast_ref<R: 'static>(&self) -> Option<&R> {
        self.render_object.as_any().downcast_ref::<R>()
    }

    /// Downcast render object to concrete type (mutable).
    pub fn downcast_mut<R: 'static>(&mut self) -> Option<&mut R> {
        self.render_object.as_any_mut().downcast_mut::<R>()
    }
}

impl std::fmt::Debug for RenderObjectWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderObjectWrapper")
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("render_object", &self.render_object.debug_name())
            .finish()
    }
}

// ============================================================================
// RenderViewObject IMPLEMENTATION
// ============================================================================

impl RenderViewObject for RenderObjectWrapper {
    fn render_object(&self) -> Option<&dyn RenderObject> {
        Some(self.render_object.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        Some(self.render_object.as_mut())
    }

    fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    fn render_state_mut(&mut self) -> &mut RenderState {
        &mut self.render_state
    }

    fn protocol(&self) -> LayoutProtocol {
        self.protocol
    }

    fn arity(&self) -> RuntimeArity {
        self.arity
    }

    fn perform_layout(
        &mut self,
        children: &[ElementId],
        constraints: BoxConstraints,
        layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
    ) -> Size {
        let type_erased = Constraints::Box(constraints);
        let geometry = self.render_object.layout(
            children,
            &type_erased,
            &mut |child_id, child_constraints| {
                let child_size = layout_child(child_id, *child_constraints.as_box());
                Geometry::Box(child_size)
            },
        );

        let size = geometry.as_box();

        // Cache the geometry
        *self.render_state.geometry.write() = Some(geometry);
        self.render_state.clear_needs_layout();

        size
    }

    fn perform_paint(
        &self,
        children: &[ElementId],
        offset: Offset,
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas {
        self.render_object.paint(children, offset, paint_child)
    }

    fn perform_hit_test(
        &self,
        children: &[ElementId],
        position: Offset,
        geometry: &Geometry,
        hit_test_child: &mut dyn FnMut(ElementId, Offset) -> bool,
    ) -> bool {
        self.render_object
            .hit_test(children, position, geometry, hit_test_child)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::Geometry;
    use std::any::Any;

    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
    }

    impl RenderObject for TestRenderBox {
        fn layout(
            &mut self,
            _children: &[ElementId],
            constraints: &Constraints,
            _layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
        ) -> Geometry {
            let box_constraints = constraints.as_box();
            Geometry::Box(box_constraints.constrain(self.size))
        }

        fn paint(
            &self,
            _children: &[ElementId],
            _offset: Offset,
            _paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
        ) -> Canvas {
            Canvas::new()
        }

        fn hit_test(
            &self,
            _children: &[ElementId],
            position: Offset,
            geometry: &Geometry,
            _hit_test_child: &mut dyn FnMut(ElementId, Offset) -> bool,
        ) -> bool {
            let size = geometry.as_box();
            position.dx >= 0.0
                && position.dy >= 0.0
                && position.dx < size.width
                && position.dy < size.height
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = RenderObjectWrapper::new_box(
            TestRenderBox {
                size: Size::new(100.0, 50.0),
            },
            RuntimeArity::Exact(0),
        );

        assert_eq!(wrapper.protocol(), LayoutProtocol::Box);
        assert_eq!(wrapper.arity(), RuntimeArity::Exact(0));
    }

    #[test]
    fn test_downcast() {
        let wrapper = RenderObjectWrapper::new_box(
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
