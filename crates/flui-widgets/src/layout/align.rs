//! [`Align`] — aligns its child within itself and optionally sizes itself to a
//! multiple of the child's size.

use flui_objects::RenderAlign;
use flui_painting::Alignment;
use flui_painting::AlignmentGeometry;
use flui_painting::typography::TextDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, InheritedView, IntoView, RenderView, impl_render_view};

use crate::localization::Directionality;

/// Aligns its child within itself.
///
/// Backed by `RenderAlign`. With no size factors the box expands to fill the
/// incoming constraints (or shrinks to the child when a dimension is
/// unbounded); a `width_factor`/`height_factor` sizes the box to that multiple
/// of the child's corresponding dimension.
#[derive(Clone, Debug)]
pub struct Align {
    alignment: AlignmentGeometry,
    width_factor: Option<f64>,
    height_factor: Option<f64>,
    child: Child,
    site: CreationSite,
}

impl Align {
    /// Align a child at the given [`AlignmentGeometry`] — an
    /// [`Alignment`] converts to an absolute geometry, an
    /// [`AlignmentDirectional`](flui_painting::AlignmentDirectional) to a
    /// direction-sensitive one resolved against the ambient [`Directionality`].
    #[track_caller]
    pub fn new(alignment: impl Into<AlignmentGeometry>) -> Self {
        Self {
            alignment: alignment.into(),
            width_factor: None,
            height_factor: None,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Size this box to `factor` × the child's width (must be `>= 0`).
    #[must_use]
    pub fn width_factor(mut self, factor: f64) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size this box to `factor` × the child's height (must be `>= 0`).
    #[must_use]
    pub fn height_factor(mut self, factor: f64) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set the aligned child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Resolve the geometry against the ambient [`Directionality`]. At mount the
    /// inherited read is not yet available, so a directional alignment defaults
    /// to Ltr and the first rebuild re-resolves it (T09).
    fn resolve(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Alignment {
        self.alignment.resolve(
            ctx.depend_on::<Directionality, _>(|d| *d.data())
                .unwrap_or(TextDirection::Ltr)
                .is_ltr(),
        )
    }

    fn build_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> RenderAlign {
        let mut render_object = RenderAlign::new(self.resolve(ctx));
        if let Some(factor) = self.width_factor {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = self.height_factor {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for Align {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAlign;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.build_render_object(ctx)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.update_configuration(self.resolve(ctx), self.width_factor, self.height_factor)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Align, site = site);
