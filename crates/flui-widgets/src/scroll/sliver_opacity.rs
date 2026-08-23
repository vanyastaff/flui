//! [`SliverOpacity`] — fades a sliver child within a scroll viewport.

use flui_objects::RenderSliverOpacity;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Makes a **sliver** child partially transparent inside a
/// [`Viewport`](crate::Viewport).
///
/// Flutter parity: `widgets/sliver.dart` `SliverOpacity` over
/// `RenderSliverOpacity`. `opacity` is clamped to `0.0..=1.0`.
#[derive(Clone, Debug)]
pub struct SliverOpacity {
    opacity: f32,
    child: Child,
}

impl SliverOpacity {
    /// Create a `SliverOpacity` with the given opacity (clamped to `0.0..=1.0`).
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity,
            child: Child::empty(),
        }
    }

    /// Set the faded sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverOpacity {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverOpacity;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverOpacity::new(self.opacity)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_opacity(self.opacity)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SliverOpacity);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;

    #[test]
    fn update_reports_exact_alpha_transition_impacts() {
        let original = SliverOpacity::new(1.0);
        let context = flui_view::RenderObjectContext::detached();
        let mut render_object = original.create_render_object(&context);

        assert_eq!(
            original.update_render_object(&context, &mut render_object),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            SliverOpacity::new(0.5).update_render_object(&context, &mut render_object),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
        assert_eq!(
            SliverOpacity::new(0.25).update_render_object(&context, &mut render_object),
            flui_rendering::RenderUpdateImpact::PAINT,
        );
        assert_eq!(
            SliverOpacity::new(0.0).update_render_object(&context, &mut render_object),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }
}
