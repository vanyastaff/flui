//! [`CustomPaint`] — delegates drawing to user-supplied [`CustomPainter`]s.

use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_objects::RenderCustomPaint;
use flui_rendering::delegates::CustomPainter;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Provides a canvas for a background and/or foreground [`CustomPainter`] to
/// draw on, around an optional child.
///
/// Paint order is background painter → child →
/// foreground painter. Sizes to the child when present, else to
/// [`Self::size`] (default [`Size::ZERO`]) constrained by the incoming
/// layout constraints.
#[derive(Clone, Debug, Default)]
pub struct CustomPaint {
    painter: Option<Rc<dyn CustomPainter>>,
    foreground_painter: Option<Rc<dyn CustomPainter>>,
    size: Size,
    child: Child,
}

impl CustomPaint {
    /// Creates a `CustomPaint` with no painters, a zero preferred size, and
    /// no child.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the painter that draws behind the child.
    #[must_use]
    pub fn painter(mut self, painter: Rc<dyn CustomPainter>) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Sets the painter that draws in front of the child.
    #[must_use]
    pub fn foreground_painter(mut self, painter: Rc<dyn CustomPainter>) -> Self {
        self.foreground_painter = Some(painter);
        self
    }

    /// Sets the size to use when there is no child.
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Sets the child to paint around.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for CustomPaint {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCustomPaint;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderCustomPaint::new(
            self.painter.clone(),
            self.foreground_painter.clone(),
            self.size,
        )
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_painter(self.painter.clone())
            | render_object.set_foreground_painter(self.foreground_painter.clone())
            | render_object.set_preferred_size(self.size)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(CustomPaint);
