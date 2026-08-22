//! [`ColoredBox`] — paints a solid color behind its child.

use flui_objects::RenderDecoratedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Color;
use flui_types::geometry::Pixels;
use flui_types::styling::BoxDecoration;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

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

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderDecoratedBox::new(self.decoration())
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_decoration(self.decoration());
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ColoredBox);
