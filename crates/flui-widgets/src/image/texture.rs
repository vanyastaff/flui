//! [`Texture`] — shows an external texture fed through the presentation's
//! texture registry.

use flui_foundation::CreationSite;
use flui_objects::RenderTexture;
use flui_painting::paint::{FilterQuality, TextureId};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatelessView};
use flui_view::{IntoView, RenderView, impl_render_view};

/// Shows the newest frame of an external texture over its whole box.
///
/// Register the texture with
/// [`LifecycleContext::texture_registry`](flui_view::prelude::LifecycleContext::texture_registry)
/// and feed it with
/// [`TextureRegistration::submit_frame`](flui_layer::TextureRegistration::submit_frame),
/// from any thread. A frame rebuilds and repaints nothing: the presentation
/// publishes the scene it holds with the frame, and the raster side redraws
/// the texture's box alone (ADR-0304 §8, §9). The widget rebuilds only when
/// its parent gives it another texture or filter. The texture is composited
/// in a layer of its own, and the box takes the largest size its constraints
/// allow.
#[derive(Clone, Debug, StatelessView)]
pub struct Texture {
    id: TextureId,
    filter_quality: FilterQuality,
    #[view(site)]
    site: CreationSite,
}

impl Texture {
    /// Shows texture `id`, sampled linearly.
    #[track_caller]
    pub fn new(id: TextureId) -> Self {
        Self {
            id,
            filter_quality: FilterQuality::Low,
            site: CreationSite::here(),
        }
    }

    /// How the texture is sampled when scaled.
    #[must_use]
    pub fn filter_quality(mut self, filter_quality: FilterQuality) -> Self {
        self.filter_quality = filter_quality;
        self
    }
}

impl StatelessView for Texture {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // A texture changes on its producer's clock, so it is composited in a
        // layer of its own, as a platform compositor would place it.
        crate::paint::RepaintBoundary::new().child(RawTexture {
            id: self.id,
            filter_quality: self.filter_quality,
        })
    }
}

/// The leaf a [`Texture`] builds.
#[derive(Clone, Debug)]
struct RawTexture {
    id: TextureId,
    filter_quality: FilterQuality,
}

impl RenderView for RawTexture {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTexture;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderTexture {
        RenderTexture::new(self.id, self.filter_quality)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render: &mut RenderTexture,
    ) -> flui_rendering::RenderUpdateImpact {
        render.set_texture(self.id) | render.set_filter_quality(self.filter_quality)
    }

    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn flui_view::View)) {}
}

impl_render_view!(RawTexture);
