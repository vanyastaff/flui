//! [`UnconstrainedBox`] — removes the constraints imposed on its child (on
//! one axis or both), letting it render at its natural size.

use flui_objects::RenderConstraintsTransformBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Axis, painting::Clip};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Removes the constraints imposed on `child` — on both axes, or, when
/// `constrained_axis` is set, on every axis *except* that one (the named
/// axis keeps its incoming constraint) — letting the child render at its
/// natural size on the freed axis.
///
/// This box then attempts to adopt the child's size, within the limits of
/// its own incoming constraints. If it ends up a different size than the
/// child, the child is positioned via `alignment`; if the child still does
/// not fit, it is clipped per `clip_behavior`.
///
/// Flutter parity: `widgets/basic.dart` `UnconstrainedBox` (tag `3.44.0`) —
/// itself a `StatelessWidget` built by selecting one of
/// `ConstraintsTransformBox.unconstrained` / `.widthUnconstrained` /
/// `.heightUnconstrained` from `constrainedAxis` and handing it to a
/// `ConstraintsTransformBox`. FLUI carries no `ConstraintsTransformBox`
/// widget (no general `BoxConstraintsTransform`-function surface) — this
/// widget targets [`RenderConstraintsTransformBox`] directly, whose own
/// `constrained_axis: Option<Axis>` field already IS that narrowed
/// three-way choice (see that render object's module doc for the exact
/// per-axis transform mapping and the shared clip/overflow contract).
/// `create_render_object`/`update_render_object` push every field, mirroring
/// the oracle's own `createRenderObject`/`updateRenderObject` (both
/// `..`-cascade every property on every call).
///
/// `textDirection`/`AlignmentDirectional` are not exposed: no widget in
/// FLUI's shifted-box family (`Align`, `OverflowBox`, `SizedOverflowBox`,
/// `FractionallySizedBox`, and now this widget) resolves an ambient
/// `Directionality` yet — a pre-existing, systemic gap, not something new
/// here; see `docs/ROADMAP.md` Cross.H.
#[derive(Clone, Debug)]
pub struct UnconstrainedBox {
    alignment: Alignment,
    constrained_axis: Option<Axis>,
    clip_behavior: Clip,
    child: Child,
}

impl UnconstrainedBox {
    /// A center-aligned, unclipped box removing constraints on both axes —
    /// Flutter's `UnconstrainedBox()` with every other parameter left at its
    /// oracle default.
    #[must_use]
    pub fn new() -> Self {
        Self {
            alignment: Alignment::CENTER,
            constrained_axis: None,
            clip_behavior: Clip::None,
            child: Child::empty(),
        }
    }

    /// Sets the alignment used to position the child when its size differs
    /// from this box's own.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Retains the incoming constraint on `axis`, freeing the other axis.
    /// Unset (the default) frees both axes.
    #[must_use]
    pub fn constrained_axis(mut self, axis: Axis) -> Self {
        self.constrained_axis = Some(axis);
        self
    }

    /// Sets how the child is clipped when it overflows this box. Defaults
    /// to [`Clip::None`] (oracle default).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Sets the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderConstraintsTransformBox {
        RenderConstraintsTransformBox::new(
            self.alignment,
            self.constrained_axis,
            self.clip_behavior,
        )
    }
}

impl Default for UnconstrainedBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView for UnconstrainedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstraintsTransformBox;

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
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_alignment(self.alignment);
        impact |= render_object.set_constrained_axis(self.constrained_axis);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact
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

impl_render_view!(UnconstrainedBox);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn detached() -> flui_view::RenderObjectContext<'static> {
        flui_view::RenderObjectContext::detached()
    }

    #[test]
    fn create_render_object_applies_oracle_defaults() {
        let render_object = UnconstrainedBox::new().create_render_object(&detached());
        assert_eq!(render_object.alignment(), Alignment::CENTER);
        assert_eq!(render_object.constrained_axis(), None);
        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn create_render_object_applies_every_configured_field() {
        let render_object = UnconstrainedBox::new()
            .alignment(Alignment::TOP_LEFT)
            .constrained_axis(Axis::Horizontal)
            .clip_behavior(Clip::AntiAlias)
            .create_render_object(&detached());

        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
        assert_eq!(render_object.constrained_axis(), Some(Axis::Horizontal));
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn update_render_object_pushes_every_field() {
        let mut render_object = UnconstrainedBox::new().create_render_object(&detached());

        let impact = UnconstrainedBox::new()
            .alignment(Alignment::BOTTOM_RIGHT)
            .constrained_axis(Axis::Vertical)
            .clip_behavior(Clip::HardEdge)
            .update_render_object(&detached(), &mut render_object);
        assert_eq!(
            impact,
            flui_rendering::RenderUpdateImpact::LAYOUT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        );

        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
        assert_eq!(render_object.constrained_axis(), Some(Axis::Vertical));
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn update_render_object_clears_a_previously_set_constrained_axis() {
        let mut render_object = UnconstrainedBox::new()
            .constrained_axis(Axis::Horizontal)
            .create_render_object(&detached());
        assert_eq!(render_object.constrained_axis(), Some(Axis::Horizontal));

        let impact = UnconstrainedBox::new().update_render_object(&detached(), &mut render_object);
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::LAYOUT);
        assert_eq!(render_object.constrained_axis(), None);
    }

    #[test]
    fn clip_only_update_reports_paint_and_semantics() {
        let original = UnconstrainedBox::new();
        let mut render_object = original.create_render_object(&detached());
        assert_eq!(
            original.update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            original
                .clone()
                .clip_behavior(Clip::HardEdge)
                .update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!UnconstrainedBox::new().has_children());
        assert!(
            UnconstrainedBox::new()
                .child(SizedBox::shrink())
                .has_children()
        );
    }
}
