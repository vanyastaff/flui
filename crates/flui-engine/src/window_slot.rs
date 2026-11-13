//! A slot that presents to a window (ADR-0305 §2–§6): the window's surface,
//! its configuration and its frame protocol, rendered with the context's
//! shared painter and presentation blit.
//!
//! What is keyed by the presentation lives here: the surface and the target it
//! needs, the size the owner configured, the colour request and latency, the
//! damage and retained target of the frame protocol, and what the acquire path
//! owes the surface between frames. Everything keyed by content (pipelines,
//! glyphs, images, the blit pipelines) is the context's.

use std::sync::Arc;

use flui_layer::{DamageRegion, Scene};
use flui_platform::{DisplayColor, SurfaceTarget};

#[cfg(all(test, feature = "testing"))]
use crate::testing::ScriptedAcquire;
use crate::{
    error::{EngineError, EngineResult},
    frame_latency::FrameLatency,
    frame_protocol::{FontSource, FrameProtocol},
    offscreen::OffscreenRenderer,
    painter::WgpuPainter,
    raster::PresentDisposition,
    surface_color::{ColorRequest, OutputEncoding, ResolvedSurfaceColor, SurfaceColor},
    surface_factory::UnconfiguredSurface,
    surface_lease::SurfaceLease,
    surface_present::{
        SurfaceAcquireBackend, SurfaceAcquireOutcome, SurfaceRecovery, SwapchainFrame,
        acquire_surface_texture_with, classify_frame, reconfigure_after_suboptimal_present,
    },
};

/// The context's parts a window slot works with.
pub(crate) struct Gpu<'a> {
    pub(crate) adapter: &'a wgpu::Adapter,
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
}

/// The configuration a surface was given.
struct Configured {
    config: wgpu::SurfaceConfiguration,
    color: ResolvedSurfaceColor,
    supports_copy_src: bool,
}

/// One window's slot.
pub(crate) struct WindowSlot {
    lease: SurfaceLease<wgpu::Surface<'static>>,
    /// Told immediately before each present (ADR-0305 §5).
    target: Arc<dyn SurfaceTarget>,
    /// The size the owner configured, in device pixels; `None` until the
    /// first one arrives.
    size: Option<(u32, u32)>,
    /// The held surface's configuration; `None` while the slot has no size
    /// or no surface.
    configured: Option<Configured>,
    color: ColorRequest,
    latency: FrameLatency,
    frame: FrameProtocol,
    recovery: SurfaceRecovery,
    /// What every acquire answers instead of the surface, in a test of a
    /// surface no GPU presents to.
    #[cfg(all(test, feature = "testing"))]
    acquire_answers: Option<ScriptedAcquire>,
    /// Make the next resize's configure panic, in a test.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) panic_in_next_configure: bool,
}

impl WindowSlot {
    /// A slot over `surface`, which the adapter must present to.
    ///
    /// # Errors
    /// [`EngineError::IncompatibleSurface`] when the adapter cannot present
    /// to `surface`.
    pub(crate) fn attach(
        gpu: &Gpu<'_>,
        surface: UnconfiguredSurface,
        color: SurfaceColor,
        display: Option<DisplayColor>,
        latency: FrameLatency,
    ) -> EngineResult<Self> {
        let UnconfiguredSurface { lease, target } = surface;
        presentable(gpu, &lease)?;
        Ok(Self {
            lease,
            target,
            size: None,
            configured: None,
            color: ColorRequest {
                requested: color,
                display,
            },
            latency,
            frame: FrameProtocol::new(),
            recovery: SurfaceRecovery::default(),
            #[cfg(all(test, feature = "testing"))]
            acquire_answers: None,
            #[cfg(all(test, feature = "testing"))]
            panic_in_next_configure: false,
        })
    }

    /// A slot over `surface`, configured at `size` in 8-bit sRGB as if the
    /// adapter offered it: wgpu's no-op adapter reports no capabilities, so a
    /// test reaches the acquire path of a surface no GPU presents to, and
    /// every acquire answers `answers`.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn configured_without_capabilities(
        surface: UnconfiguredSurface,
        size: (u32, u32),
        answers: ScriptedAcquire,
    ) -> Self {
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![format],
            format_capabilities: vec![wgpu::SurfaceFormatCapabilities {
                format,
                color_spaces: wgpu::SurfaceColorSpaces::SRGB,
            }],
            ..Default::default()
        };
        let color = ResolvedSurfaceColor::resolve(
            SurfaceColor::Srgb8,
            &caps,
            None,
            &wgpu::DisplayHdrInfo::default(),
            false,
        )
        .expect("BUG: a surface offering BGRA8 in sRGB resolves to 8-bit sRGB");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: FrameLatency::default().frames(),
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            color_space: color.color_space(),
        };
        let UnconfiguredSurface { lease, target } = surface;
        Self {
            lease,
            target,
            size: Some(size),
            configured: Some(Configured {
                config,
                color,
                supports_copy_src: false,
            }),
            color: color.request(),
            latency: FrameLatency::default(),
            frame: FrameProtocol::new(),
            recovery: SurfaceRecovery::default(),
            #[cfg(all(test, feature = "testing"))]
            acquire_answers: Some(answers),
            #[cfg(all(test, feature = "testing"))]
            panic_in_next_configure: false,
        }
    }

    /// Give the surface `size` device pixels, both sides positive and within
    /// the device's limit.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a size outside the device's
    /// limit, which leaves the slot as it was; a configuration error of the
    /// surface.
    pub(crate) fn resize(&mut self, gpu: &Gpu<'_>, size: (u32, u32)) -> EngineResult<()> {
        let max = gpu.device.limits().max_texture_dimension_2d;
        let (width, height) = size;
        if width == 0 || height == 0 || width > max || height > max {
            return Err(EngineError::InvalidTargetSize { width, height });
        }
        if self.size == Some(size) && self.configured.is_some() {
            return Ok(());
        }
        let Some(configured) = self.configured.as_ref() else {
            self.size = Some(size);
            return self.configure(gpu);
        };
        let mut config = configured.config.clone();
        config.width = width;
        config.height = height;
        if let Some(surface) = self.lease.surface() {
            #[cfg(all(test, feature = "testing"))]
            assert!(
                !std::mem::take(&mut self.panic_in_next_configure),
                "injected panic in a surface configure"
            );
            surface.configure(gpu.device, &config);
            self.recovery.configured();
        }
        // Committed once the surface took it: a configure that unwinds
        // leaves the slot at its old size, so the retry configures again.
        self.size = Some(size);
        if let Some(configured) = self.configured.as_mut() {
            configured.config = config;
        }
        self.frame.surface_changed();
        Ok(())
    }

    /// Drop the surface; the target stays, and frames settle not shown until
    /// a [`Self::restore`].
    pub(crate) fn release(&mut self) {
        self.recovery.reset();
        if self.lease.has_surface() {
            let _released = self.lease.release();
        }
        self.configured = None;
        self.frame.release_target();
        self.frame.surface_changed();
    }

    /// The surface the slot holds; `None` while it is released.
    pub(crate) fn surface(&self) -> Option<&wgpu::Surface<'static>> {
        self.lease.surface()
    }

    /// The context rebuilt its device: configure the held surface on the new
    /// one from the new adapter's capabilities (ADR-0063 decision 6), at the
    /// slot's size. What the frame protocol retained was the old device's
    /// and is dropped, so the next frame repaints in full. A released slot
    /// stays released until its restore.
    ///
    /// # Errors
    /// [`EngineError::IncompatibleSurface`] when the new adapter cannot
    /// present to the surface, which is then dropped for the owner to
    /// replace; [`EngineError::InvalidTargetSize`] when the slot's size is over
    /// the new device's limit, which leaves the surface unconfigured until a
    /// resize fits; a configuration error.
    pub(crate) fn rebuild(&mut self, gpu: &Gpu<'_>) -> EngineResult<()> {
        self.configured = None;
        self.recovery.reset();
        self.frame.release_target();
        self.frame.surface_changed();
        if !self.lease.has_surface() {
            return Ok(());
        }
        if let Err(error) = presentable(gpu, &self.lease) {
            let _released = self.lease.release();
            return Err(error);
        }
        self.configure(gpu)
    }

    /// Present to `surface` from now on, configured at the slot's size.
    ///
    /// # Errors
    /// [`EngineError::IncompatibleSurface`] when the adapter cannot present
    /// to `surface`, which leaves the slot released; a configuration error.
    pub(crate) fn restore(
        &mut self,
        gpu: &Gpu<'_>,
        surface: UnconfiguredSurface,
    ) -> EngineResult<()> {
        self.release();
        let UnconfiguredSurface { lease, target } = surface;
        presentable(gpu, &lease)?;
        self.lease = lease;
        self.target = target;
        self.configure(gpu)
    }

    /// The window's display now reports `display` as its colour: the held
    /// surface is configured again when its colour was resolved for another.
    ///
    /// # Errors
    /// The surface offers no configuration for the request; it keeps its old
    /// one.
    pub(crate) fn set_display_color(
        &mut self,
        gpu: &Gpu<'_>,
        display: DisplayColor,
    ) -> EngineResult<()> {
        self.color.display = Some(display);
        if self
            .configured
            .as_ref()
            .is_some_and(|configured| configured.color.request() == self.color)
        {
            return Ok(());
        }
        self.configure(gpu)
    }

    /// Hold `latency` frames ahead of the display from the next configuration,
    /// which happens now for a configured surface.
    pub(crate) fn set_latency(&mut self, gpu: &Gpu<'_>, latency: FrameLatency) {
        self.latency = latency;
        let Some(configured) = self.configured.as_ref() else {
            return;
        };
        let mut config = configured.config.clone();
        config.desired_maximum_frame_latency = latency.frames();
        if let Some(surface) = self.lease.surface() {
            surface.configure(gpu.device, &config);
            self.recovery.configured();
        }
        // Committed once the surface took it, as a resize is. A reconfigured
        // swapchain may hold none of the pixels shown, so the next frame
        // repaints in full, as after a resize.
        if let Some(configured) = self.configured.as_mut() {
            configured.config = config;
        }
        self.frame.surface_changed();
    }

    /// The working format and size the painter renders the next frame in;
    /// `None` while the slot has nothing to present into.
    pub(crate) fn target_format(&self) -> Option<(wgpu::TextureFormat, (u32, u32))> {
        let configured = self.configured.as_ref()?;
        self.lease.surface()?;
        Some((
            configured.color.working_format(),
            (configured.config.width, configured.config.height),
        ))
    }

    /// Render `scene` with `painter` (retargeted to [`Self::target_format`])
    /// and present it, repainting at least `damage`.
    ///
    /// # Errors
    /// The frame's acquire, render or submission error.
    pub(crate) fn render(
        &mut self,
        gpu: &Gpu<'_>,
        painter: &mut WgpuPainter,
        offscreen: &mut OffscreenRenderer,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        match damage {
            DamageRegion::Region(region) => self.frame.mark_dirty(region.bounds().to_rect()),
            DamageRegion::Unchanged => {}
            DamageRegion::Full(_) => self.frame.mark_full_repaint(),
        }
        self.frame
            .select_font_source(painter, FontSource::Ordinary, false);
        self.present(gpu, painter, offscreen, scene)
    }

    /// Render `scene`, lent outside the slot's frames, in full with the fonts
    /// it carries (`reset_fonts` drops the ones an earlier lent scene
    /// registered), and present it. The screen then shows a scene no frame
    /// described, so the slot's next frame renders in full.
    ///
    /// # Errors
    /// As [`Self::render`].
    pub(crate) fn render_lent(
        &mut self,
        gpu: &Gpu<'_>,
        painter: &mut WgpuPainter,
        offscreen: &mut OffscreenRenderer,
        scene: &Scene,
        reset_fonts: bool,
    ) -> EngineResult<PresentDisposition> {
        self.frame
            .select_font_source(painter, FontSource::Plugin, reset_fonts);
        self.frame.begin_unmanaged();
        let presented = self.present(gpu, painter, offscreen, scene);
        self.frame.end_unmanaged();
        presented
    }

    fn present(
        &mut self,
        gpu: &Gpu<'_>,
        painter: &mut WgpuPainter,
        offscreen: &mut OffscreenRenderer,
        scene: &Scene,
    ) -> EngineResult<PresentDisposition> {
        let Some((working_format, size)) = self.target_format() else {
            return Ok(PresentDisposition::NotShown);
        };
        self.frame.include_backdrop_dependencies(scene, size);
        if self.frame.plan(self.uses_intermediate()) == crate::damage::FramePlan::Skip {
            return Ok(classify_frame(false, false));
        }
        let Some(output) = acquire_surface_texture_with(&mut Acquire { slot: self, gpu })? else {
            // The slot held a surface when the frame began: one gone now was
            // lost for good and dropped, and only the owner makes another.
            if !self.lease.has_surface() {
                return Err(EngineError::SurfaceReplacementNeeded);
            }
            return Ok(classify_frame(true, false));
        };
        // Acquiring may have configured the surface again, which owes a full
        // repaint: plan again.
        let plan = self.frame.plan(self.uses_intermediate());
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let output_encoding = self
            .configured
            .as_ref()
            .map_or(OutputEncoding::Identity, |configured| {
                configured.color.output()
            });
        let domain = Arc::clone(painter.domain());
        #[cfg(feature = "gpu-profiler")]
        let mut no_profiler = None;
        let mut steps = SwapchainFrame {
            painter,
            offscreen,
            output: output_encoding,
            scene,
            #[cfg(feature = "gpu-profiler")]
            gpu_profiler: &mut no_profiler,
        };
        self.frame.run(
            plan,
            &domain,
            size,
            working_format,
            (&view, &output.texture),
            &mut steps,
        )?;
        let SwapchainFrame {
            painter, offscreen, ..
        } = steps;
        // The platform's frame-pacing signal, strictly before the present and
        // only for a frame that presents.
        self.target
            .pre_present_notify(crate::surface_present::swap_chain_statistics(
                self.lease.surface(),
            ));
        tracing::trace!(
            target: "flui.gpu",
            event = "pre_present_notified",
            "platform notified before present"
        );
        gpu.queue.present(output);
        tracing::trace!(
            target: "flui.gpu",
            event = "present_submitted",
            "surface frame submitted and presented"
        );
        self.frame.presented();
        match reconfigure_after_suboptimal_present(&mut Acquire { slot: self, gpu }) {
            // The configure owes a repaint nothing else asks for: present the
            // same scene again, at most once (the episode is answered).
            Ok(true) => return self.present(gpu, painter, offscreen, scene),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(target: "flui.gpu", %error, "reconfigure after a suboptimal frame failed");
            }
        }
        Ok(classify_frame(true, true))
    }

    /// Whether a frame renders into the retained target and reaches the
    /// surface through the presentation blit.
    fn uses_intermediate(&self) -> bool {
        self.configured.as_ref().is_some_and(|configured| {
            !configured.supports_copy_src || configured.color.output() != OutputEncoding::Identity
        })
    }

    /// Configure the held surface at the slot's size from its own
    /// capabilities; nothing while it has no size or no surface.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] when the size is over the device's
    /// limit, which leaves the slot unconfigured; a configuration error.
    fn configure(&mut self, gpu: &Gpu<'_>) -> EngineResult<()> {
        let (Some(surface), Some((width, height))) = (self.lease.surface(), self.size) else {
            return Ok(());
        };
        // Every configuration checks the size against the device it is made
        // on: a rebuilt device's limit can be below the size an older one
        // took, and the slot then stays unconfigured until a resize fits.
        let max = gpu.device.limits().max_texture_dimension_2d;
        if width > max || height > max {
            return Err(EngineError::InvalidTargetSize { width, height });
        }
        let (config, supports_copy_src, color) = crate::Renderer::derive_surface_config(
            surface,
            gpu.adapter,
            width,
            height,
            self.latency,
            self.color,
        )?;
        surface.configure(gpu.device, &config);
        self.configured = Some(Configured {
            config,
            color,
            supports_copy_src,
        });
        self.recovery.reset();
        self.frame.surface_changed();
        Ok(())
    }
}

/// Whether the context's adapter presents to the surface `lease` holds.
fn presentable(gpu: &Gpu<'_>, lease: &SurfaceLease<wgpu::Surface<'static>>) -> EngineResult<()> {
    match lease.surface() {
        Some(surface) if gpu.adapter.is_surface_supported(surface) => Ok(()),
        Some(_) | None => Err(EngineError::IncompatibleSurface),
    }
}

/// The acquire path's view of a slot.
struct Acquire<'a, 'g> {
    slot: &'a mut WindowSlot,
    gpu: &'a Gpu<'g>,
}

impl SurfaceAcquireBackend for Acquire<'_, '_> {
    type Frame = wgpu::SurfaceTexture;

    fn acquire(&mut self) -> Result<SurfaceAcquireOutcome<Self::Frame>, EngineError> {
        #[cfg(all(test, feature = "testing"))]
        if let Some(answer) = self.slot.acquire_answers
            && self.slot.lease.has_surface()
        {
            return Ok(match answer {
                ScriptedAcquire::Timeout => SurfaceAcquireOutcome::Timeout,
                ScriptedAcquire::Lost => SurfaceAcquireOutcome::Lost,
            });
        }
        let Some(surface) = self.slot.lease.surface() else {
            return Ok(SurfaceAcquireOutcome::Released);
        };
        Ok(SurfaceAcquireOutcome::from(surface.get_current_texture()))
    }

    fn reconfigure(&mut self) -> Result<(), EngineError> {
        if let (Some(surface), Some(configured)) =
            (self.slot.lease.surface(), self.slot.configured.as_ref())
        {
            surface.configure(self.gpu.device, &configured.config);
            self.slot.frame.surface_changed();
        }
        Ok(())
    }

    /// Drop the lost surface and make none: a surface is made on the thread
    /// the window system serves, which the executor may not be on (Apple and
    /// Android hosts answer `Unavailable` elsewhere), so the owner makes the
    /// replacement with its factory and sends it in a `Restore` (ADR-0305 §2,
    /// §8). The frame then answers [`EngineError::SurfaceReplacementNeeded`].
    fn recreate(&mut self) -> Result<(), EngineError> {
        self.slot.release();
        Ok(())
    }

    fn recovery(&mut self) -> &mut SurfaceRecovery {
        &mut self.slot.recovery
    }
}

#[cfg(all(test, feature = "testing"))]
impl WindowSlot {
    /// Pay the damage owed, as a presented frame does.
    pub(crate) fn settle_presented(&mut self) {
        let _plan = self.frame.plan(self.uses_intermediate());
        self.frame.presented();
    }

    /// Whether the next frame of an unchanged scene would render.
    pub(crate) fn owes_a_frame(&mut self) -> bool {
        self.frame.plan(self.uses_intermediate()) != crate::damage::FramePlan::Skip
    }
}
