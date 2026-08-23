//! [`FractionalTranslation`] — translates its child by a fraction of the
//! child's own size when painting.

use flui_objects::{RenderFractionalTranslation, TranslationFraction};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Translates its child by `(dx, dy)` × the child's size before painting (e.g.
/// `dx = -0.5` shifts the child left by half its width). Layout is unaffected.
///
/// Flutter parity: `widgets/basic.dart` `FractionalTranslation` over
/// `RenderFractionalTranslation`. `transform_hit_tests` (default `true`) also
/// shifts the hit-test region with the paint.
#[derive(Clone, Debug)]
pub struct FractionalTranslation {
    dx: f32,
    dy: f32,
    transform_hit_tests: bool,
    child: Child,
}

impl FractionalTranslation {
    /// Translate by `(dx, dy)` fractions of the child's size.
    pub fn new(dx: f32, dy: f32) -> Self {
        Self {
            dx,
            dy,
            transform_hit_tests: true,
            child: Child::empty(),
        }
    }

    /// Set whether hit-testing follows the painted translation (default `true`).
    #[must_use]
    pub fn transform_hit_tests(mut self, transform_hit_tests: bool) -> Self {
        self.transform_hit_tests = transform_hit_tests;
        self
    }

    /// Set the translated child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderFractionalTranslation {
        RenderFractionalTranslation::new(
            TranslationFraction::new(self.dx, self.dy),
            self.transform_hit_tests,
        )
    }
}

impl RenderView for FractionalTranslation {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFractionalTranslation;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_translation(TranslationFraction::new(self.dx, self.dy))
            | render_object.set_transform_hit_tests(self.transform_hit_tests)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(FractionalTranslation);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;

    #[test]
    fn geometry_and_hit_test_updates_report_independent_exact_impacts() {
        let initial = FractionalTranslation::new(0.25, 0.5);
        let mut render_object =
            initial.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            initial.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            initial
                .clone()
                .transform_hit_tests(false)
                .update_render_object(
                    &flui_view::RenderObjectContext::detached(),
                    &mut render_object,
                ),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            FractionalTranslation::new(0.5, 0.75)
                .transform_hit_tests(false)
                .update_render_object(
                    &flui_view::RenderObjectContext::detached(),
                    &mut render_object,
                ),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }
}
