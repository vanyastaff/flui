//! [`CustomPaint`] — delegates drawing to user-supplied [`CustomPainter`]s.

use std::sync::Arc;

use flui_objects::RenderCustomPaint;
use flui_rendering::delegates::CustomPainter;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Size;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Provides a canvas for a background and/or foreground [`CustomPainter`] to
/// draw on, around an optional child.
///
/// Flutter parity: `widgets/basic.dart` `CustomPaint` over
/// `RenderCustomPaint`. Paint order is background painter → child →
/// foreground painter. Sizes to the child when present, else to
/// [`Self::size`] (default [`Size::ZERO`]) constrained by the incoming
/// layout constraints.
#[derive(Clone, Debug, Default)]
pub struct CustomPaint {
    painter: Option<Arc<dyn CustomPainter>>,
    foreground_painter: Option<Arc<dyn CustomPainter>>,
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
    pub fn painter(mut self, painter: Arc<dyn CustomPainter>) -> Self {
        self.painter = Some(painter);
        self
    }

    /// Sets the painter that draws in front of the child.
    #[must_use]
    pub fn foreground_painter(mut self, painter: Arc<dyn CustomPainter>) -> Self {
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

    fn create_render_object(&self) -> Self::RenderObject {
        RenderCustomPaint::new(
            self.painter.clone(),
            self.foreground_painter.clone(),
            self.size,
        )
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_painter(self.painter.clone());
        render_object.set_foreground_painter(self.foreground_painter.clone());
        render_object.set_preferred_size(self.size);
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

impl_render_view!(CustomPaint);
