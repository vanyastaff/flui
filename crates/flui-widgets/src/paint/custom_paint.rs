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
    ) {
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

#[cfg(test)]
mod tests {
    use std::any::Any;

    use flui_rendering::pipeline::Canvas;
    use flui_types::geometry::px;
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    /// A minimal painter -- only needed to satisfy `CustomPainter`'s object
    /// safety; these tests exercise the widget's own wiring, not painter
    /// behavior.
    #[derive(Debug)]
    struct NoopPainter;

    impl CustomPainter for NoopPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {}

        fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
            false
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn painter() -> Arc<dyn CustomPainter> {
        Arc::new(NoopPainter)
    }

    #[test]
    fn create_render_object_defaults_to_no_painters_and_zero_size() {
        let render_object =
            CustomPaint::new().create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(render_object.painter().is_none());
        assert!(render_object.foreground_painter().is_none());
        assert_eq!(render_object.preferred_size(), Size::ZERO);
    }

    #[test]
    fn create_render_object_installs_the_configured_painters_and_size() {
        let render_object = CustomPaint::new()
            .painter(painter())
            .foreground_painter(painter())
            .size(Size::new(px(30.0), px(20.0)))
            .create_render_object(&flui_view::RenderObjectContext::detached());

        assert!(render_object.painter().is_some());
        assert!(render_object.foreground_painter().is_some());
        assert_eq!(
            render_object.preferred_size(),
            Size::new(px(30.0), px(20.0))
        );
    }

    #[test]
    fn update_render_object_reinstalls_painters_and_size() {
        let mut render_object =
            CustomPaint::new().create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(render_object.painter().is_none());

        CustomPaint::new()
            .painter(painter())
            .size(Size::new(px(5.0), px(5.0)))
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );

        assert!(render_object.painter().is_some());
        assert!(render_object.foreground_painter().is_none());
        assert_eq!(render_object.preferred_size(), Size::new(px(5.0), px(5.0)));
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!CustomPaint::new().has_children());
        assert!(CustomPaint::new().child(SizedBox::shrink()).has_children());
    }
}
