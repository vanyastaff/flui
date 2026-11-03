//! [`FractionallySizedBox`] — sizes its child to a fraction of the available
//! space.

use flui_objects::{FractionFactor, RenderFractionallySizedBox};
use flui_painting::Alignment;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Sizes its child to a fraction of the available space along each axis.
///
/// Backed by `RenderFractionallySizedBox`. A `None` factor leaves that axis at the
/// incoming constraint; factors must be finite and `>= 0`. Defaults to
/// `Alignment::CENTER`.
#[derive(Clone, Debug)]
pub struct FractionallySizedBox {
    width_factor: Option<f64>,
    height_factor: Option<f64>,
    alignment: Option<Alignment>,
    child: Child,
    site: CreationSite,
}

impl FractionallySizedBox {
    /// An empty `FractionallySizedBox`; set factors with the builders below.
    #[track_caller]
    pub fn new() -> Self {
        Self {
            width_factor: None,
            height_factor: None,
            alignment: None,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Size the child's width to `factor` × the available width.
    #[must_use]
    pub fn width_factor(mut self, factor: f64) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size the child's height to `factor` × the available height.
    #[must_use]
    pub fn height_factor(mut self, factor: f64) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set how the sized child is aligned within the box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Set the fractionally-sized child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn factor(value: Option<f64>) -> Option<FractionFactor> {
        value.map(FractionFactor::new_unchecked)
    }

    fn build_render_object(&self) -> RenderFractionallySizedBox {
        let mut render_object = RenderFractionallySizedBox::new();
        if let Some(alignment) = self.alignment {
            render_object = render_object.with_alignment(alignment);
        }
        if let Some(factor) = Self::factor(self.width_factor) {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = Self::factor(self.height_factor) {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl Default for FractionallySizedBox {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::new()
    }
}

impl RenderView for FractionallySizedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFractionallySizedBox;

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
        impact |= render_object.set_width_factor(Self::factor(self.width_factor));
        impact |= render_object.set_height_factor(Self::factor(self.height_factor));
        impact |= render_object.set_alignment(self.alignment.unwrap_or(Alignment::CENTER));
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(FractionallySizedBox, site = site);
