//! [`Opacity`] — makes its child partially transparent.

use flui_objects::RenderOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Makes its child partially transparent.
///
/// Flutter parity: `widgets/basic.dart` `Opacity` over `RenderOpacity`.
/// `opacity` is clamped to `0.0..=1.0`; `0.0` paints nothing (but the child is
/// still laid out and interactive unless wrapped in `IgnorePointer`).
#[derive(Clone, Debug)]
pub struct Opacity {
    opacity: f32,
    child: Child,
}

impl Opacity {
    /// Create an `Opacity` with the given opacity (clamped to `0.0..=1.0`).
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity,
            child: Child::empty(),
        }
    }

    /// Set the child to fade.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Opacity {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOpacity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderOpacity::new(self.opacity)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_opacity(self.opacity);
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

impl_render_view!(Opacity);
