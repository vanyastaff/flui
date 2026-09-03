//! [`Transform`] — applies a 2D/3D matrix transform to its child when painting.

use flui_geometry::Matrix4;
use flui_objects::RenderTransform;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Offset};
use flui_view::{Child, IntoView, RenderView, impl_render_view};

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
#[expect(clippy::struct_field_names)]
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
        // Order is load-bearing: `RenderTransform::with_alignment` clears
        // `origin` back to `None` (it's a "set the pivot mode to alignment"
        // call, not a field-merge), so it must run BEFORE `with_origin`.
        // Reversing this — `.with_origin(..)` then `.with_alignment(..)` —
        // would silently drop any explicit `origin` this widget was given.
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
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_transform(self.transform)
            | render_object.set_alignment(self.alignment)
            | render_object.set_origin(self.origin)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Transform);

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;
    use flui_view::RenderView;

    use super::*;

    #[test]
    fn update_reports_exact_geometry_impact_and_dedupes_identical_configuration() {
        let initial = Transform::translate(2.0, 3.0);
        let mut render_object =
            initial.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            initial.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
        );

        let changed = Transform::scale(2.0, 2.0)
            .alignment(Alignment::BOTTOM_RIGHT)
            .origin(Offset::new(px(4.0), px(5.0)));
        assert_eq!(
            changed.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }
}
