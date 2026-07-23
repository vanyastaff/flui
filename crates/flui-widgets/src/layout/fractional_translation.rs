//! [`FractionalTranslation`] — translates its child by a fraction of the
//! child's own size when painting.

use flui_objects::{RenderFractionalTranslation, TranslationFraction};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

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
    ) {
        *render_object = self.build_render_object();
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

impl_render_view!(FractionalTranslation);
