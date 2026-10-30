//! The renderer a desktop window's raster lane owns, and where a loop gets it.
//!
//! A window's lane needs more from its renderer than
//! [`RasterBackend`]: the device rebuild of
//! [`DeviceRecovery`], the display colour of [`DisplayColorFollow`] and the
//! frame latency. [`WindowRenderer`] is that set. Production builds the wgpu
//! [`Renderer`] from the window's surface target; the
//! runner's test kit gives every window a stand-in that needs no GPU, so a
//! headless run opens the windows an application opens and runs their frames.

use flui_engine::{EngineError, FrameLatency, PresentDisposition, RasterBackend, Renderer};
use flui_foundation::geometry::Rect;

use super::device_recovery::DeviceRecovery;
use super::surface_lifecycle::DisplayColorFollow;

/// What a desktop window's raster lane needs from its renderer.
pub(super) trait WindowRenderer:
    RasterBackend + DeviceRecovery + DisplayColorFollow
{
    /// How many frames the renderer may queue ahead of the display.
    fn set_frame_latency(&mut self, latency: FrameLatency);
}

impl WindowRenderer for Renderer {
    fn set_frame_latency(&mut self, latency: FrameLatency) {
        Renderer::set_frame_latency(self, latency);
    }
}

/// A window's renderer, whichever the loop builds.
pub(super) type LaneRenderer = Box<dyn WindowRenderer + Send>;

impl RasterBackend for LaneRenderer {
    fn render_scene(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<PresentDisposition, EngineError> {
        (**self).render_scene(scene)
    }
    fn resize(&mut self, width: u32, height: u32) {
        (**self).resize(width, height);
    }
    fn is_device_lost(&self) -> bool {
        (**self).is_device_lost()
    }
    fn mark_dirty(&mut self, rect: Rect<f64>) {
        (**self).mark_dirty(rect);
    }
    fn mark_full_repaint(&mut self) {
        (**self).mark_full_repaint();
    }
    fn apply_texture_updates(
        &mut self,
        updates: flui_layer::TextureUpdates,
    ) -> Result<(), EngineError> {
        (**self).apply_texture_updates(updates)
    }
    fn has_damage(&self) -> bool {
        (**self).has_damage()
    }
    fn size(&self) -> (u32, u32) {
        (**self).size()
    }
    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        (**self).reconfigure_surface()
    }
    fn set_pre_present_hook(&mut self, hook: Option<flui_engine::PrePresentHook>) {
        (**self).set_pre_present_hook(hook);
    }
    fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        (**self).reclaim(level);
    }
}

impl DeviceRecovery for LaneRenderer {
    fn try_recover_device(&mut self) -> Result<(), EngineError> {
        (**self).try_recover_device()
    }
}

impl DisplayColorFollow for LaneRenderer {
    fn follow_display_color(
        &mut self,
        display: Option<flui_platform::DisplayColor>,
    ) -> Result<bool, EngineError> {
        (**self).follow_display_color(display)
    }
}

/// Where a loop's windows get their renderers. Set for each loop when it
/// starts.
#[derive(Clone, Default)]
pub(crate) enum WindowRenderers {
    /// The wgpu renderer on the window's surface.
    #[default]
    Gpu,
    /// The test kit's stand-in, one per window, kept where the test finds it.
    #[cfg(feature = "testing")]
    StandIn(super::stand_in_renderer::StandInRenderers),
}

impl WindowRenderers {
    /// The renderer for `window`, set up for its surface, `config`'s colour
    /// and the display it is on.
    pub(super) fn create(
        &self,
        window: &dyn flui_platform::PlatformWindow,
        config: &crate::app::AppConfig,
    ) -> Result<LaneRenderer, EngineError> {
        Ok(match self {
            Self::Gpu => Box::new(pollster::block_on(Renderer::with_surface_color(
                window.surface_target(),
                config.surface_color,
                super::host::display_color_of(window),
            ))?),
            #[cfg(feature = "testing")]
            Self::StandIn(renderers) => Box::new(renderers.open(window.id())),
        })
    }
}

impl std::fmt::Debug for WindowRenderers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu => f.write_str("Gpu"),
            #[cfg(feature = "testing")]
            Self::StandIn(_) => f.write_str("StandIn"),
        }
    }
}
