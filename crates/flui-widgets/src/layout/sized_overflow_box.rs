//! [`SizedOverflowBox`] — claims a fixed size while letting the child overflow.

use flui_foundation::geometry::Size;
use flui_objects::RenderSizedOverflowBox;
use flui_painting::{Alignment, AlignmentGeometry};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

use crate::Directionality;

/// Claims a specific size for itself while laying its child out under the
/// incoming (parent) constraints, which may allow the child to be a different
/// size and overflow.
///
/// The box's own size is always `constraints.constrain(requested_size)`.  The
/// child is aligned within that slot using `alignment`.
///
/// Backed by [`RenderSizedOverflowBox`].
#[derive(Clone, Debug)]
pub struct SizedOverflowBox {
    alignment: AlignmentGeometry,
    requested_size: Size,
    child: Child,
    site: CreationSite,
}

impl SizedOverflowBox {
    /// Creates the widget with center alignment.
    #[track_caller]
    pub fn new(requested_size: Size) -> Self {
        Self {
            alignment: Alignment::CENTER.into(),
            requested_size,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Sets the alignment of the child within the claimed slot.
    ///
    /// An [`Alignment`] is absolute, an
    /// [`AlignmentDirectional`](flui_painting::AlignmentDirectional) resolves
    /// against the ambient [`Directionality`].
    #[must_use]
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
        self
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SizedOverflowBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedOverflowBox;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedOverflowBox::new(
            Directionality::resolve_alignment(ctx, self.alignment),
            self.requested_size,
        )
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |=
            render_object.set_alignment(Directionality::resolve_alignment(ctx, self.alignment));
        impact |= render_object.set_requested_size(self.requested_size);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SizedOverflowBox, site = site);
