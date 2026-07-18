//! [`Transform`] — applies a 2D/3D matrix transform to its child when painting.

use flui_geometry::Matrix4;
use flui_objects::RenderTransform;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Offset};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Applies a [`Matrix4`] transform to its child before painting.
///
/// Flutter parity: `widgets/basic.dart` `Transform` over `RenderTransform`.
/// The transform affects painting and hit-testing but not layout — the child
/// is laid out as if untransformed.
///
/// `alignment` defaults to [`Alignment::CENTER`] — matching Flutter's
/// `Transform.rotate`/`Transform.scale`/`Transform.flip` factory defaults,
/// but **not** Flutter's bare `Transform(transform:, origin:)` constructor,
/// whose `alignment` defaults to `null` (no contribution at all). An
/// `origin` set here without an explicit [`alignment`](Self::alignment) call
/// therefore combines with the CENTER default instead of acting alone —
/// see `docs/ROADMAP.md` Cross.H for the parity-port finding this surfaced.
// `transform` names the Flutter-parity concept the struct wraps (matches
// `RenderTransform`'s own field of the same name); renaming it to dodge the
// lint would trade a clear name for a weaker one.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Debug)]
pub struct Transform {
    transform: Matrix4,
    alignment: Alignment,
    origin: Option<Offset>,
    child: Child,
}

impl Transform {
    /// Apply an arbitrary [`Matrix4`].
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            alignment: Alignment::CENTER,
            origin: None,
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

    /// Sets the alignment of the transform's pivot, relative to the child's
    /// size (Flutter parity: `Transform.alignment`). Combines additively with
    /// [`origin`](Self::origin) when both are set.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets an explicit pivot offset, on top of [`alignment`](Self::alignment)'s
    /// contribution (Flutter parity: `Transform.origin`).
    #[must_use]
    pub fn origin(mut self, origin: Offset) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Set the transformed child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderTransform {
        let render_object = RenderTransform::new(self.transform).with_alignment(self.alignment);
        match self.origin {
            Some(origin) => render_object.with_origin(origin),
            None => render_object,
        }
    }
}

impl RenderView for Transform {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTransform;

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

impl_render_view!(Transform);
