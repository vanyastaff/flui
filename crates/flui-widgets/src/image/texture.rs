//! [`Texture`] — shows an external texture fed through the presentation's
//! texture registry.

use flui_objects::RenderTexture;
use flui_painting::paint::{FilterQuality, TextureId};
use flui_rendering::layer::{TextureListener, TextureRegistry};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView, ViewState};
use flui_view::{IntoView, RebuildHandle, RebuildReason, RenderView, impl_render_view};

/// Shows the newest frame of an external texture over its whole box.
///
/// Register the texture with
/// [`LifecycleContext::texture_registry`](flui_view::prelude::LifecycleContext::texture_registry)
/// and feed it with
/// [`TextureRegistration::submit_frame`](flui_rendering::layer::TextureRegistration::submit_frame),
/// from any thread. Every frame repaints this widget alone; the box takes the
/// largest size its constraints allow.
#[derive(Clone, Debug, StatefulView)]
pub struct Texture {
    id: TextureId,
    filter_quality: FilterQuality,
}

impl Texture {
    /// Shows texture `id`, sampled linearly.
    pub fn new(id: TextureId) -> Self {
        Self {
            id,
            filter_quality: FilterQuality::Low,
        }
    }

    /// How the texture is sampled when scaled.
    #[must_use]
    pub fn filter_quality(mut self, filter_quality: FilterQuality) -> Self {
        self.filter_quality = filter_quality;
        self
    }
}

/// Follows the shown texture's frames.
#[derive(Debug)]
pub struct TextureState {
    id: TextureId,
    registry: Option<TextureRegistry>,
    rebuild: Option<RebuildHandle>,
    listener: Option<TextureListener>,
}

impl TextureState {
    /// Rebuild after each frame the shown texture receives, replacing any
    /// earlier listener.
    fn follow(&mut self) {
        self.listener = None;
        let (Some(registry), Some(rebuild)) = (&self.registry, &self.rebuild) else {
            return;
        };
        let rebuild = rebuild.clone();
        self.listener = registry.listen(self.id, move || {
            rebuild.schedule(RebuildReason::StateChange);
        });
    }
}

impl StatefulView for Texture {
    type State = TextureState;

    fn create_state(&self) -> Self::State {
        TextureState {
            id: self.id,
            registry: None,
            rebuild: None,
            listener: None,
        }
    }
}

impl ViewState<Texture> for TextureState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.registry = ctx.texture_registry();
        self.rebuild = Some(ctx.rebuild_handle());
        self.follow();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.registry = ctx.texture_registry();
        self.follow();
    }

    fn did_update_view(&mut self, _old_view: &Texture, new_view: &Texture) {
        if self.id != new_view.id {
            self.id = new_view.id;
            self.follow();
        }
    }

    fn build(&self, view: &Texture, _ctx: &dyn BuildContext) -> impl IntoView {
        RawTexture {
            id: view.id,
            filter_quality: view.filter_quality,
            revision: self
                .registry
                .as_ref()
                .and_then(|registry| registry.revision(view.id))
                .unwrap_or(0),
        }
    }

    fn dispose(&mut self) {
        self.listener = None;
    }
}

/// The leaf a [`Texture`] builds once it knows the frame to show.
#[derive(Clone, Debug)]
struct RawTexture {
    id: TextureId,
    filter_quality: FilterQuality,
    revision: u64,
}

impl RenderView for RawTexture {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTexture;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderTexture {
        let mut render = RenderTexture::new(self.id, self.filter_quality);
        let _ = render.set_revision(self.revision);
        render
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render: &mut RenderTexture,
    ) -> flui_rendering::RenderUpdateImpact {
        render.set_texture(self.id)
            | render.set_filter_quality(self.filter_quality)
            | render.set_revision(self.revision)
    }

    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn flui_view::View)) {}
}

impl_render_view!(RawTexture);
