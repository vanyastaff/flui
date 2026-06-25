//! [`Transform`] — applies a 2D/3D matrix transform to its child when painting.

use flui_geometry::Matrix4;
use flui_objects::RenderTransform;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Applies a [`Matrix4`] transform to its child before painting.
///
/// Flutter parity: `widgets/basic.dart` `Transform` over `RenderTransform`.
/// The transform affects painting and hit-testing but not layout — the child
/// is laid out as if untransformed.
#[derive(Clone, Debug)]
pub struct Transform {
    transform: Matrix4,
    child: Child,
}

impl Transform {
    /// Apply an arbitrary [`Matrix4`].
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            child: Child::empty(),
        }
    }

    /// Translate the child by `(x, y)` device pixels.
    pub fn translate(x: f32, y: f32) -> Self {
        Self::new(*RenderTransform::translate(x, y).transform())
    }

    /// Scale the child by `(sx, sy)`.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::new(*RenderTransform::scale(sx, sy).transform())
    }

    /// Rotate the child by `radians` about the Z axis.
    pub fn rotation(radians: f32) -> Self {
        Self::new(*RenderTransform::rotation(radians).transform())
    }

    /// Set the transformed child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Transform {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTransform;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderTransform::new(self.transform)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = RenderTransform::new(self.transform);
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

impl_render_view!(Transform);
