//! [`FittedBox`] — scales and positions its child within itself per a [`BoxFit`].

use flui_objects::RenderFittedBox;
use flui_painting::BoxFit;
use flui_painting::paint::Clip;
use flui_painting::{Alignment, AlignmentGeometry};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

use crate::localization::{Directionality, resolve_alignment};

/// Scales and positions its child within itself according to a [`BoxFit`].
///
/// Backed by `RenderFittedBox`. Defaults: `BoxFit::Contain`,
/// `Alignment::CENTER`, `Clip::None`.
#[derive(Clone, Debug)]
pub struct FittedBox {
    fit: BoxFit,
    alignment: AlignmentGeometry,
    clip: Clip,
    child: Child,
    site: CreationSite,
}

impl Default for FittedBox {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl FittedBox {
    /// A `FittedBox` with the default fit (`Contain` / centered / no clip).
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    fn at(site: CreationSite) -> Self {
        Self {
            fit: BoxFit::Contain,
            alignment: Alignment::CENTER.into(),
            clip: Clip::None,
            child: Child::empty(),
            site,
        }
    }

    /// Set how the child is inscribed into the available space.
    #[must_use]
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set how the scaled child is aligned within the box: an [`Alignment`] is
    /// absolute, an [`AlignmentDirectional`](flui_painting::AlignmentDirectional)
    /// resolves against the ambient [`Directionality`].
    #[must_use]
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
        self
    }

    /// Set the clip behavior applied when the child overflows.
    #[must_use]
    pub fn clip(mut self, clip: Clip) -> Self {
        self.clip = clip;
        self
    }

    /// Set the fitted child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// The alignment resolved against the ambient [`Directionality`], read only
    /// for a directional one.
    fn resolved_alignment(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Alignment {
        resolve_alignment(self.alignment, || Directionality::of_render_object(ctx))
    }
}

impl RenderView for FittedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFittedBox;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderFittedBox::new(self.fit, self.resolved_alignment(ctx), self.clip)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_fit(self.fit)
            | render_object.set_alignment(self.resolved_alignment(ctx))
            | render_object.set_clip_behavior(self.clip)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(FittedBox, site = site);
