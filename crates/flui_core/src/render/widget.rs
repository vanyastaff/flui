//! RenderObjectWidget - widgets that directly manage RenderObjects
//!
//! These widgets create and configure RenderObjects for layout and painting.

use std::fmt;

use crate::{DynWidget, Widget};

/// Base trait for widgets that create RenderObjects
///
/// a RenderObject for layout and painting, rather than composing other widgets.
pub trait RenderObjectWidget: Widget {
    /// Create the RenderObject for this widget
    ///
    /// Called when element is first mounted. Should create and return a new
    /// RenderObject instance.
    fn create_render_object(&self) -> Box<dyn crate::DynRenderObject>;

    /// Update the RenderObject with new configuration
    ///
    /// Called when widget is updated. Should apply new configuration to the
    /// existing RenderObject.
    fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject);
}

/// Widget that creates a RenderObject without children
///
/// have any children, like Image, Text (in some cases), or custom painters.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderObject;
///
/// #[derive(Debug, Clone)]
/// struct CustomPaint {
///     color: Color,
/// }
///
/// impl LeafRenderObjectWidget for CustomPaint {
///     fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
///         Box::new(RenderCustomPaint::new(self.color))
///     }
///
///     fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
///         if let Some(render) = render_object.downcast_mut::<RenderCustomPaint>() {
///             render.set_color(self.color);
///         }
///     }
/// }
/// ```
pub trait LeafRenderObjectWidget: RenderObjectWidget + fmt::Debug + Clone + Send + Sync + 'static {
    // Inherits create_render_object and update_render_object from RenderObjectWidget
}

/// Widget that creates a RenderObject with a single child
///
/// have exactly one child, like Padding, Opacity, or Transform.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderObject;
///
/// #[derive(Debug, Clone)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: Box<dyn DynWidget>,
/// }
///
/// impl SingleChildRenderObjectWidget for Padding {
///     fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
///         Box::new(RenderPadding::new(self.padding))
///     }
///
///     fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
///         if let Some(render) = render_object.downcast_mut::<RenderPadding>() {
///             render.set_padding(self.padding);
///         }
///     }
///
///     fn child(&self) -> &dyn DynWidget {
///         &*self.child
///     }
/// }
/// ```
pub trait SingleChildRenderObjectWidget:
    RenderObjectWidget + fmt::Debug + Clone + Send + Sync + 'static
{
    /// Get the child widget
    fn child(&self) -> &dyn DynWidget;
}

/// Widget that creates a RenderObject with multiple children
///
/// have multiple children, like Row, Column, Stack, or Wrap.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderObject;
///
/// #[derive(Debug, Clone)]
/// struct Row {
///     children: Vec<Box<dyn DynWidget>>,
///     main_axis_alignment: MainAxisAlignment,
/// }
///
/// impl MultiChildRenderObjectWidget for Row {
///     fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
///         Box::new(RenderFlex::new(Axis::Horizontal, self.main_axis_alignment))
///     }
///
///     fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
///         if let Some(render) = render_object.downcast_mut::<RenderFlex>() {
///             render.set_main_axis_alignment(self.main_axis_alignment);
///         }
///     }
///
///     fn children(&self) -> &[Box<dyn DynWidget>] {
///         &self.children
///     }
/// }
/// ```
pub trait MultiChildRenderObjectWidget:
    RenderObjectWidget + fmt::Debug + Clone + Send + Sync + 'static
{
    /// Get the children widgets
    fn children(&self) -> &[Box<dyn DynWidget>];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynRenderObject, DynWidget, BoxConstraints, Context, Offset, Size, StatelessWidget, Widget};

    // Simple test RenderObject
    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
        constraints: Option<BoxConstraints>,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
    }

    impl TestRenderBox {
        fn new() -> Self {
            Self {
                size: Size::zero(),
                constraints: None,
                needs_layout_flag: true,
                needs_paint_flag: true,
            }
        }
    }

    impl DynRenderObject for TestRenderBox {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.constraints = Some(constraints);
            self.size = constraints.biggest();
            self.needs_layout_flag = false;
            self.size
        }

        fn paint(&self, _painter: &egui::Painter, _offset: Offset) {
            // Test implementation - no-op
        }

        fn size(&self) -> Size {
            self.size
        }

        fn constraints(&self) -> Option<BoxConstraints> {
            self.constraints
        }

        fn needs_layout(&self) -> bool {
            self.needs_layout_flag
        }

        fn mark_needs_layout(&mut self) {
            self.needs_layout_flag = true;
        }

        fn needs_paint(&self) -> bool {
            self.needs_paint_flag
        }

        fn mark_needs_paint(&mut self) {
            self.needs_paint_flag = true;
        }
    }

    // Test leaf widget
    #[derive(Debug, Clone)]
    struct TestLeafWidget {
        width: f32,
        height: f32,
    }

    impl Widget for TestLeafWidget {
        type Element = crate::LeafRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            crate::LeafRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for TestLeafWidget {
        fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
            Box::new(TestRenderBox::new())
        }

        fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
            if let Some(render_box) = render_object.downcast_mut::<TestRenderBox>() {
                render_box.mark_needs_layout();
            }
        }
    }

    impl LeafRenderObjectWidget for TestLeafWidget {}

    // Helper stateless widget for testing
    #[derive(Debug, Clone, Copy)]
    struct TestStatelessWidget;

    impl StatelessWidget for TestStatelessWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(TestStatelessWidget)
        }
    }

    #[test]
    fn test_leaf_render_object_widget_create() {
        let widget = TestLeafWidget {
            width: 100.0,
            height: 50.0,
        };
        let render_object = widget.create_render_object();

        // Verify it's a TestRenderBox
        assert!(render_object.is::<TestRenderBox>());
        let render_box = render_object.downcast_ref::<TestRenderBox>().unwrap();
        assert!(render_box.needs_layout());
    }

    #[test]
    fn test_leaf_render_object_widget_update() {
        let widget = TestLeafWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut render_object: Box<dyn crate::DynRenderObject> = Box::new(TestRenderBox::new());

        // Layout first
        render_object.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));
        assert!(!render_object.needs_layout());

        // Update should mark as needing layout
        widget.update_render_object(&mut *render_object);
        assert!(render_object.needs_layout());
    }
}
