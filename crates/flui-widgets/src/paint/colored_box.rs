//! [`ColoredBox`] — paints a solid color behind its child.

use flui_objects::RenderDecoratedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Color;
use flui_types::geometry::Pixels;
use flui_types::styling::BoxDecoration;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Paints a solid `color` filling its bounds, behind its child.
///
/// Flutter parity: `widgets/basic.dart` `ColoredBox`. In Flutter this is a
/// dedicated single-child proxy; FLUI realises the same behavior as a
/// `RenderDecoratedBox` with a color-only `BoxDecoration` (a `ColoredBox` is a
/// `DecoratedBox(decoration: BoxDecoration(color: color))`). It sizes to its
/// child, or fills the incoming constraints when childless.
#[derive(Clone, Debug)]
pub struct ColoredBox {
    color: Color,
    child: Child,
}

impl ColoredBox {
    /// Create a `ColoredBox` painting the given solid `color`.
    pub fn new(color: Color) -> Self {
        Self {
            color,
            child: Child::empty(),
        }
    }

    /// Set the child painted over the color.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn decoration(&self) -> BoxDecoration<Pixels> {
        BoxDecoration::<Pixels>::with_color(self.color)
    }
}

impl RenderView for ColoredBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderDecoratedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderDecoratedBox::new(self.decoration())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_decoration(self.decoration());
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(ColoredBox);
