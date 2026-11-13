//! The web runner's frame sink until the web draws through the raster
//! executor (ADR-0305 §12): it renders a publication through the web
//! window's own renderer on the calling thread, in full, with no stamp and no
//! generation check. Every native runner draws through its window's slot on
//! the loop's raster executor (`runner::raster_host`).

#![expect(
    deprecated,
    reason = "ADR-0305: the web runner's renderer, until the web draws through the raster executor"
)]

use flui_engine::{EngineError, PresentDisposition, RasterBackend};
use flui_layer::{Scene, ScenePublication};
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// Renders through a borrowed [`RasterBackend`] on the calling thread, with
/// no stamping and no generation checks: the web's renderer arrives
/// asynchronously and recovers across an `.await`, which the inline raster
/// executor does not accommodate yet.
pub(crate) struct DirectSink<'a, R: RasterBackend> {
    renderer: &'a mut R,
    textures: Option<flui_layer::TextureRegistry>,
}

impl<'a, R: RasterBackend> DirectSink<'a, R> {
    pub(crate) fn new(renderer: &'a mut R) -> Self {
        Self {
            renderer,
            textures: None,
        }
    }

    /// Applies `textures`' frames and retirements before each frame renders.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "only the web runner hands this sink its textures")
    )]
    #[must_use]
    pub(crate) fn with_textures(mut self, textures: Option<flui_layer::TextureRegistry>) -> Self {
        self.textures = textures;
        self
    }
}

impl<R: RasterBackend> FrameSink for DirectSink<'_, R> {
    fn surface_size(&mut self) -> (u32, u32) {
        self.renderer.size()
    }

    fn submit(&mut self, publication: ScenePublication) -> SubmitVerdict {
        if let Some(textures) = &self.textures {
            let updates = textures.take_updates();
            if !updates.is_empty()
                && let Err(error) = self.renderer.apply_texture_updates(updates)
            {
                tracing::warn!(%error, "external texture frame refused");
            }
        }
        // Every frame is full on this path: it tracks no version on screen, so the web
        // runner repaints in full until it moves onto the lane.
        self.renderer.mark_full_repaint();
        let scene = Scene::of_version(publication.scene);
        match self.renderer.render_scene(&scene) {
            Ok(PresentDisposition::Presented) => SubmitVerdict::Presented,
            Ok(PresentDisposition::NoDamage) => SubmitVerdict::NoPresent,
            Ok(PresentDisposition::NotShown) => SubmitVerdict::NotShown,
            Err(EngineError::SurfaceLost) => {
                tracing::debug!("surface lost during render");
                SubmitVerdict::SurfaceStale
            }
            Err(EngineError::SurfaceValidation) => {
                tracing::error!("surface validation error - surface misconfig");
                SubmitVerdict::SurfaceStale
            }
            Err(
                EngineError::DeviceLost
                | EngineError::GpuUnavailable
                | EngineError::GpuProgress { .. },
            ) => SubmitVerdict::DeviceLost,
            Err(error) if error.recoverability() == flui_engine::Recoverability::Recoverable => {
                SubmitVerdict::Retry
            }
            Err(error) => {
                tracing::error!(?error, "render error (non-recoverable this frame)");
                SubmitVerdict::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A minimal non-empty publication: a root slotting one canvas chunk.
    fn test_scene() -> ScenePublication {
        use flui_layer::{BoundaryContent, SceneOp};
        let mut owner = flui_layer::SceneOwner::new();
        {
            let mut writer = owner.writer();
            let root = writer.current().root();
            let version = writer.next_content_version().expect("a version");
            let mut canvas = flui_painting::Canvas::new();
            canvas.draw_rect(
                flui_foundation::geometry::Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
                &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
            );
            let mut content = BoundaryContent::builder();
            content.chunk(Arc::new(flui_layer::Chunk::new(
                flui_foundation::RenderId::new(1),
                0,
                version,
                canvas.finish(),
            )));
            writer.push(SceneOp::Publish {
                node: root,
                content: Arc::new(content.finish()),
            });
        }
        owner
            .apply(flui_layer::SurfaceKey {
                size: (100, 100),
                generation: flui_foundation::SurfaceGeneration::ZERO,
            })
            .expect("the first version");
        owner.publication()
    }

    /// A backend whose render answers are queued up front.
    #[derive(Default)]
    struct ScriptedBackend {
        outcomes: std::collections::VecDeque<Result<PresentDisposition, EngineError>>,
    }

    impl RasterBackend for ScriptedBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<PresentDisposition, EngineError> {
            self.outcomes
                .pop_front()
                .unwrap_or(Ok(PresentDisposition::Presented))
        }
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn is_device_lost(&self) -> bool {
            false
        }
        fn mark_dirty(&mut self, _rect: flui_foundation::geometry::Rect<f64>) {}
        fn mark_full_repaint(&mut self) {}
        fn has_damage(&self) -> bool {
            true
        }
        fn size(&self) -> (u32, u32) {
            (100, 100)
        }
        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    /// Each answer a backend gives for a rendered frame, an error included,
    /// maps to one verdict: frame-aligned haptics and the commit read it, and
    /// a transient failure retries while a terminal one does not.
    #[test]
    fn every_render_answer_maps_to_its_verdict() {
        for (answer, expected) in [
            (Ok(PresentDisposition::Presented), SubmitVerdict::Presented),
            (Ok(PresentDisposition::NoDamage), SubmitVerdict::NoPresent),
            (Ok(PresentDisposition::NotShown), SubmitVerdict::NotShown),
            (Err(EngineError::GpuBackpressure), SubmitVerdict::Retry),
            (
                Err(EngineError::MissingRetainedSource),
                SubmitVerdict::Retry,
            ),
            (Err(EngineError::Timeout), SubmitVerdict::Retry),
            (Err(EngineError::SurfaceLost), SubmitVerdict::SurfaceStale),
            (Err(EngineError::GpuUnavailable), SubmitVerdict::DeviceLost),
            (
                Err(EngineError::DeviceDomainMismatch),
                SubmitVerdict::Failed,
            ),
            (
                Err(EngineError::PreparedResourceLimit {
                    resource: "test",
                    requested: 2,
                    limit: 1,
                }),
                SubmitVerdict::Failed,
            ),
        ] {
            let label = format!("{answer:?}");
            let mut backend = ScriptedBackend::default();
            backend.outcomes.push_back(answer);
            let mut direct = DirectSink::new(&mut backend);
            assert_eq!(direct.submit(test_scene()), expected, "{label}");
            assert_eq!(
                direct.submit(test_scene()),
                SubmitVerdict::Presented,
                "{label}: the next frame renders"
            );
        }
    }
}
