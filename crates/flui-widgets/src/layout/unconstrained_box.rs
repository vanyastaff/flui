//! [`UnconstrainedBox`] — removes the constraints imposed on its child (on
//! one axis or both), letting it render at its natural size.

use flui_foundation::geometry::Axis;
use flui_objects::RenderConstraintsTransformBox;
use flui_painting::paint::Clip;
use flui_painting::{Alignment, AlignmentGeometry};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

use crate::localization::{Directionality, resolve_alignment};

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
/// This widget targets [`RenderConstraintsTransformBox`] directly, whose
/// `constrained_axis: Option<Axis>` field is the three-way choice between
/// unconstrained on both axes, width only, or height only (see that render
/// object's module doc for the exact per-axis transform mapping and the
/// shared clip/overflow contract). There is no general
/// constraints-transform-function widget.
/// `create_render_object`/`update_render_object` push every field on every
/// call.
#[derive(Clone, Debug)]
pub struct UnconstrainedBox {
    alignment: AlignmentGeometry,
    constrained_axis: Option<Axis>,
    clip_behavior: Clip,
    child: Child,
    site: CreationSite,
}

impl UnconstrainedBox {
    /// A center-aligned, unclipped box removing constraints on both axes —
    /// with every other parameter left at its default.
    #[must_use]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            alignment: Alignment::CENTER.into(),
            constrained_axis: None,
            clip_behavior: Clip::None,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Sets the alignment used to position the child when its size differs
    /// from this box's own.
    ///
    /// An [`Alignment`] is absolute, an
    /// [`AlignmentDirectional`](flui_painting::AlignmentDirectional) resolves
    /// against the ambient [`Directionality`].
    #[must_use]
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
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
    /// to [`Clip::None`].
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

    fn build_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
    ) -> RenderConstraintsTransformBox {
        RenderConstraintsTransformBox::new(
            self.resolved_alignment(ctx),
            self.constrained_axis,
            self.clip_behavior,
        )
    }
}

impl Default for UnconstrainedBox {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::new()
    }
}

impl UnconstrainedBox {
    /// The alignment resolved against the ambient [`Directionality`], read only
    /// for a directional one.
    fn resolved_alignment(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Alignment {
        resolve_alignment(self.alignment, || Directionality::of_render_object(ctx))
    }
}

impl RenderView for UnconstrainedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderConstraintsTransformBox;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.build_render_object(ctx)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_alignment(self.resolved_alignment(ctx));
        impact |= render_object.set_constrained_axis(self.constrained_axis);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(UnconstrainedBox, site = site);
