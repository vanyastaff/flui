//! [`DecoratedBox`] — paints a [`BoxDecoration`] around its child.

use flui_objects::{DecorationPosition, RenderDecoratedBox};
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::Pixels;
use flui_types::styling::BoxDecoration;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Paints a [`BoxDecoration`] (color, border, gradient, shadow, …) before or
/// after painting its child.
///
/// Flutter parity: `widgets/basic.dart` `DecoratedBox` over
/// `RenderDecoratedBox`. The decoration is painted in the background by default
/// (behind the child); use [`DecoratedBox::foreground`] to paint over the child.
#[derive(Clone, Debug)]
pub struct DecoratedBox {
    decoration: BoxDecoration<Pixels>,
    position: DecorationPosition,
    child: Child,
}

impl DecoratedBox {
    /// Paint `decoration` behind the child (the common case).
    pub fn new(decoration: BoxDecoration<Pixels>) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
            child: Child::empty(),
        }
    }

    /// Paint the decoration in the foreground, over the child.
    #[must_use]
    pub fn foreground(mut self) -> Self {
        self.position = DecorationPosition::Foreground;
        self
    }

    /// Set the decorated child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for DecoratedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderDecoratedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderDecoratedBox::new(self.decoration.clone()).with_position(self.position)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_decoration(self.decoration.clone());
        render_object.set_position(self.position);
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

impl_render_view!(DecoratedBox);
