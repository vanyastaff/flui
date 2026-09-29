//! Windowless GPU capture: rasterize a `LayerTree` to an offscreen texture and
//! read the pixels back to the CPU.
//!
//! The on-screen [`crate::Renderer`] hard-requires a `wgpu::Surface`
//! (its `render_scene` acquires a swapchain texture). Golden-image and
//! screenshot tooling needs the same raster path against a caller-owned
//! texture instead — so this module owns a surface-less device and the
//! layer-tree walk / readback that `Renderer::render_scene` performs between
//! surface-acquire and present.
//!
//! It renders through the sampleable `RenderTarget` (unlike the public
//! [`WgpuPainter::render_to_view`], which is `view_only`), so advanced
//! (dst-read) blends that sample the destination render correctly.
//!
//! What it does NOT render the way the windowed [`Renderer`] does: the three
//! layer kinds `Renderer` diverts to its own handlers because they need the
//! offscreen renderer or the surface, and which the generic `LayerRender`
//! arms only approximate —
//!
//! - [`Layer::BackdropFilter`] — no blur; the children paint unfiltered;
//! - [`Layer::ShaderMask`] — the children paint inside a save-layer clipped
//!   to the mask bounds, with no mask applied;
//! - [`Layer::Follower`] — no leader offset is resolved; the children paint
//!   at the follower's unlinked position.
//!
//! The readback suites capture through this renderer, so those three kinds
//! are pinned only by `renderer.rs`'s own tests, which build a
//! `LayerDispatcher::with_offscreen` over an `OffscreenRenderer` directly.
//!
//! [`Renderer`]: crate::Renderer
//! [`Layer::BackdropFilter`]: flui_layer::Layer::BackdropFilter
//! [`Layer::ShaderMask`]: flui_layer::Layer::ShaderMask
//! [`Layer::Follower`]: flui_layer::Layer::Follower

use std::sync::Arc;

use flui_layer::{LayerId, LayerTree};

use crate::error::{EngineError, EngineResult};
use crate::{
    layer_dispatcher::LayerDispatcher, layer_render::LayerRender, painter::WgpuPainter,
    render_target::RenderTarget,
};

/// The pixel format headless capture renders and reads back in. RGBA8 maps
/// straight to a PNG without a channel swizzle.
const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[cfg(test)]
thread_local! {
    /// `wgpu::Instance`s this thread's captures created. `wgpu::Adapter`'s
    /// equality cannot tell whether two renderers share an instance (an adapter
    /// from a fresh instance compares equal), so the twin test counts instead.
    /// Per thread, so tests running in parallel threads under `cargo test`
    /// cannot move each other's count.
    static INSTANCES_CREATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A windowless renderer that turns a [`LayerTree`] into raw RGBA8 pixels.
///
/// Construct once (device creation is the expensive step), then call
/// [`Self::render_layer_tree`] per capture.
#[expect(missing_debug_implementations)]
pub struct HeadlessRenderer {
    /// Kept so a test's feature-reduced twin comes from this same adapter and
    /// instance (see [`Self::without_dual_source_blending`]).
    #[cfg(test)]
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl HeadlessRenderer {
    /// Acquire a surface-less GPU device for offscreen capture.
    ///
    /// Requests [`wgpu::Features::DUAL_SOURCE_BLENDING`] where the adapter
    /// exposes it, so a capture shows the same clip fringe the windowed
    /// renderer does — `Renderer::required_features` makes the same request for
    /// the same reason. Nothing else about the device is negotiated.
    ///
    /// Async because wgpu's adapter and device requests are async. Calling
    /// `Renderer::new` and this from the same async context is the point: a
    /// blocking constructor would stall whichever executor thread it ran on,
    /// and the sync wrapper belongs to the caller (an example, a test) that
    /// owns its runtime, not to the library.
    ///
    /// # Errors
    /// Returns [`EngineError`] when no GPU adapter or device is available.
    pub async fn new() -> EngineResult<Self> {
        Self::acquire(wgpu::Features::DUAL_SOURCE_BLENDING).await
    }

    /// A second renderer on this one's adapter, with
    /// [`wgpu::Features::DUAL_SOURCE_BLENDING`] withheld from its device even
    /// where the adapter exposes it.
    ///
    /// This is how a test reaches the folded fallback on hardware that has the
    /// feature. Every adapter this workspace's CI and dev machines run on has
    /// it — DX12 exposes it unconditionally — so without this the fallback
    /// would ship untested on every device able to exercise it, and the
    /// feathered result would have nothing to be compared against.
    ///
    /// The twin shares this renderer's adapter, and with it the `wgpu::Instance`.
    /// A twin built from an instance of its own left two instances' devices on
    /// one adapter, and tearing the pair down blocked inside the driver on a
    /// Windows host, intermittently, for the full nextest timeout
    /// (`twin_renderers_tear_down_without_blocking`).
    ///
    /// # Errors
    /// Returns [`EngineError`] when no GPU adapter or device is available.
    #[cfg(test)]
    pub(crate) async fn without_dual_source_blending(&self) -> EngineResult<Self> {
        Self::device_on(self.adapter.clone(), wgpu::Features::empty()).await
    }

    /// Acquires the capture device, requesting whichever of `wanted_features`
    /// the adapter actually offers.
    async fn acquire(wanted_features: wgpu::Features) -> EngineResult<Self> {
        #[cfg(test)]
        INSTANCES_CREATED.with(|created| created.set(created.get() + 1));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&crate::adapter::trusted_adapter_options(
                wgpu::PowerPreference::HighPerformance,
                None,
            ))
            .await
            .map_err(EngineError::adapter_request)?;
        Self::device_on(adapter, wanted_features).await
    }

    /// A capture device on `adapter` with whichever of `wanted_features` it
    /// offers.
    async fn device_on(
        adapter: wgpu::Adapter,
        wanted_features: wgpu::Features,
    ) -> EngineResult<Self> {
        // Deliberately NOT `adapter::request_flui_device`: capture wants
        // wgpu's default (downlevel-friendly) device rather than the
        // renderer's capability-negotiated one — plus the features above,
        // which change what the captured pixels look like.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI Headless Capture Device"),
                required_features: adapter.features() & wanted_features,
                ..Default::default()
            })
            .await
            .map_err(EngineError::device_creation)?;

        Ok(Self {
            #[cfg(test)]
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    /// Whether this renderer's device can feather a coverage-destructive
    /// blend's clip edge — the same question
    /// `GpuCapabilities::supports_dual_source_blending` answers for the
    /// windowed renderer.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn supports_dual_source_blending(&self) -> bool {
        self.device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
    }

    /// Rasterize `tree` at `size` (device pixels) and return tightly-packed
    /// (no row padding) RGBA8 pixels, top row first — ready for
    /// `image::save_buffer(.., ColorType::Rgba8)`.
    ///
    /// The surface is cleared to opaque white before the tree is drawn, so any
    /// area the tree does not paint reads as white rather than uninitialized
    /// GPU memory.
    ///
    /// # Errors
    /// Returns [`EngineError`] when the render pass fails.
    pub fn render_layer_tree(&self, tree: &LayerTree, size: (u32, u32)) -> EngineResult<Vec<u8>> {
        let (width, height) = size;
        // wgpu rejects a zero-byte buffer by PANICKING (`wgpu-core`'s
        // `BufferSize::new(..).unwrap()`), and a zero-sized texture is
        // equally invalid. Reject here so a caller that derived the size from
        // user input or from a not-yet-laid-out window gets a `Result`.
        // Bound the request by the device's own limit HERE, where the input
        // enters. It has to be checked by hand rather than left to
        // `create_texture`: that call reports an over-limit size through
        // wgpu's uncaptured-error path, which panics by default instead of
        // returning, so an oversized request would abort rather than reach a
        // caller as a `Result`. With this check the readback row arithmetic
        // below is provably in range, and its conversions are invariants.
        let max_dim = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max_dim || height > max_dim {
            return Err(EngineError::InvalidTargetSize { width, height });
        }
        let texture = self.create_capture_texture(width, height);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.clear_to_background(&view);

        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&self.device),
            Arc::clone(&self.queue),
            CAPTURE_FORMAT,
            (width, height),
        );
        {
            let mut backend = LayerDispatcher::new(&mut painter);
            let mut visitor = CaptureVisitor {
                backend: &mut backend,
            };
            crate::layer_walk::walk_layer_tree(tree, tree.root(), &mut visitor);
            // `backend` drops here → its `Drop` flushes the active transform.
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("FLUI Headless Capture Render Encoder"),
            });
        painter.render(RenderTarget::sampleable(&view, &texture), &mut encoder)?;
        self.queue.submit(std::iter::once(encoder.finish()));

        self.readback_rgba(&texture, width, height)
    }

    fn create_capture_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FLUI Headless Capture Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn clear_to_background(&self, view: &wgpu::TextureView) {
        clear_to_background(&self.device, &self.queue, view);
    }

    /// [`readback_rgba`] on this renderer's device.
    fn readback_rgba(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> EngineResult<Vec<u8>> {
        readback_rgba(&self.device, &self.queue, texture, width, height)
    }

    /// A capture that keeps its frames: see [`RetainedCapture`].
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a zero or over-limit size.
    #[cfg(test)]
    pub(crate) fn retained_capture(&self, size: (u32, u32)) -> EngineResult<RetainedCapture> {
        let (width, height) = size;
        let max_dim = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > max_dim || height > max_dim {
            return Err(EngineError::InvalidTargetSize { width, height });
        }
        let surface = self.create_capture_texture(width, height);
        let surface_view = surface.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(RetainedCapture {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
            painter: WgpuPainter::with_shared_device(
                Arc::clone(&self.device),
                Arc::clone(&self.queue),
                CAPTURE_FORMAT,
                size,
            ),
            offscreen: crate::offscreen::OffscreenRenderer::new(
                Arc::clone(&self.device),
                Arc::clone(&self.queue),
                CAPTURE_FORMAT,
            ),
            surface,
            surface_view,
            frame: crate::frame_protocol::FrameProtocol::new(),
            size,
            intermediate_required: false,
            fail_after_begin: false,
            last_plan: None,
        })
    }
}

/// Clears `view` to the frame background with one submitted pass.
fn clear_to_background(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("FLUI Headless Capture Clear Encoder"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FLUI Headless Capture Clear Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(crate::frame_protocol::background_clear_value()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit(std::iter::once(encoder.finish()));
}

/// Copies `texture` to a mappable buffer and de-pads the 256-byte-aligned
/// rows into a tight `width * height * 4` RGBA8 buffer.
///
/// The row arithmetic is `u64` throughout, and that is hardening rather
/// than a fix: every caller bounds both axes by `max_texture_dimension_2d`
/// before any GPU work, so `width * 4` cannot reach `u32::MAX` and the `u32`
/// form would not wrap today. It is `u64` so that the invariant is local to
/// this function instead of resting on a check in a different one — the
/// failure mode it avoids is a *silent* wrap (`width = 2^30` makes
/// `width * 4` zero) that would size the staging buffer at zero bytes and
/// re-enter the `wgpu-core` panic, which is worth one conversion not to have
/// to re-derive later.
fn readback_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> EngineResult<Vec<u8>> {
    const BYTES_PER_PIXEL: u64 = 4;
    let align = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let unpadded_row_bytes = u64::from(width) * BYTES_PER_PIXEL;
    let padded_row_bytes = unpadded_row_bytes.div_ceil(align) * align;

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("FLUI Headless Capture Readback Staging"),
        size: padded_row_bytes * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("FLUI Headless Capture Readback Encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                // `u32` because wgpu's layout field is. In range by
                // construction: every caller bounds `width` by
                // `max_texture_dimension_2d` before any GPU work, so
                // `width * 4` padded to 256 is far below `u32::MAX`.
                bytes_per_row: Some(u32::try_from(padded_row_bytes).expect(
                    "BUG: every capture bounds width by max_texture_dimension_2d \
                         before readback",
                )),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let copied = queue.submit(std::iter::once(encoder.finish()));

    staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    // Wait for THIS copy, not the whole queue: the renderer is `&self`, so
    // another thread's slow capture must not run this one out of time.
    readback_wait_outcome(device.poll(readback_wait(copied)), READBACK_TIMEOUT)?;

    let mapped = staging.slice(..).get_mapped_range().expect(
        "BUG: readback staging buffer must be mapped — the poll above waited for the \
             map_async issued on this same slice",
    );
    // `usize` for indexing; the `u64` row math above is already known to
    // fit this address space or `create_buffer` would have failed first.
    let tight_size = (unpadded_row_bytes * u64::from(height)) as usize;
    let mut pixels = Vec::with_capacity(tight_size);
    for row in 0..u64::from(height) {
        let start = (row * padded_row_bytes) as usize;
        let end = start + unpadded_row_bytes as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    debug_assert_eq!(
        pixels.len(),
        tight_size,
        "the de-padded readback must be exactly width*height*4"
    );
    Ok(pixels)
}

/// A windowless stand-in for the windowed [`Renderer`]'s frame path, for
/// tests of partial frames: it keeps a [`FrameProtocol`] and a "surface"
/// texture across frames and runs each frame through the same
/// [`FrameProtocol::plan`] and [`FrameProtocol::run`] the windowed renderer
/// does, recording content through the same `Renderer::record_frame_content`
/// (the partial-frame scissor and clear included). Only the clear, the
/// content submission and the blit are its own ([`FrameSteps`]).
///
/// Crate-private and test-only: the public golden-image API is settled with
/// the CPU backend (ADR-0087 §2), and this exists to pin the GPU partial
/// path's pixels, which a swapchain cannot be read back to show.
///
/// [`Renderer`]: crate::Renderer
/// [`FrameProtocol`]: crate::frame_protocol::FrameProtocol
/// [`FrameProtocol::plan`]: crate::frame_protocol::FrameProtocol::plan
/// [`FrameProtocol::run`]: crate::frame_protocol::FrameProtocol::run
/// [`FrameSteps`]: crate::frame_protocol::FrameSteps
#[cfg(test)]
pub(crate) struct RetainedCapture {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    painter: WgpuPainter,
    offscreen: crate::offscreen::OffscreenRenderer,
    /// Stands in for the swapchain image the windowed path presents.
    surface: wgpu::Texture,
    surface_view: wgpu::TextureView,
    frame: crate::frame_protocol::FrameProtocol,
    size: (u32, u32),
    /// Stands in for a surface without `COPY_SRC`: every frame renders
    /// through the retained target.
    intermediate_required: bool,
    /// When set, the next frame's content pass fails (after the retained
    /// target was begun) and the flag clears itself.
    fail_after_begin: bool,
    last_plan: Option<crate::damage::FramePlan>,
}

#[cfg(test)]
impl RetainedCapture {
    /// The surface's pixels as tight RGBA8 rows, top row first.
    ///
    /// # Errors
    /// [`EngineError::ReadbackTimedOut`] when the GPU stalls.
    pub(crate) fn read_rgba(&self) -> EngineResult<Vec<u8>> {
        readback_rgba(
            &self.device,
            &self.queue,
            &self.surface,
            self.size.0,
            self.size.1,
        )
    }

    /// Writes `rgba` into `(x, y, width, height)` of the retained target, as
    /// a stale previous frame would have left it. Panics before the target
    /// exists.
    pub(crate) fn paint_retained(
        &self,
        (x, y, width, height): (u32, u32, u32, u32),
        rgba: [u8; 4],
    ) {
        let texture = self
            .frame
            .retained_texture()
            .expect("the retained target is allocated by a retained frame");
        let data: Vec<u8> = std::iter::repeat_n(rgba, (width * height) as usize)
            .flatten()
            .collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Makes the next frame's content pass fail after it began the retained
    /// target.
    pub(crate) fn fail_next_frame_after_begin(&mut self) {
        self.fail_after_begin = true;
    }

    /// Registers a one-texel external texture of `rgba` under `id`, or, when
    /// `id` is registered already, replaces its content through
    /// `ExternalTextureRegistry::update`, as a video decoder hands over its
    /// next frame behind the same id.
    pub(crate) fn set_solid_texture(&mut self, id: flui_painting::paint::TextureId, rgba: [u8; 4]) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("retained capture external texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let registry = self.painter.external_texture_registry_mut();
        if registry.get(id).is_some() {
            assert!(registry.update(id, texture), "the texture is registered");
        } else {
            registry.register(id, texture, 1, 1, true, false);
        }
    }

    /// Renders every later frame through the retained target, as on a
    /// surface without `COPY_SRC`.
    pub(crate) fn require_intermediate(&mut self) {
        self.intermediate_required = true;
    }

    /// The plan the last frame ran.
    pub(crate) fn last_plan(&self) -> Option<crate::damage::FramePlan> {
        self.last_plan
    }

    /// Renders `scene` the way `Renderer::render_scene` does: a frame no
    /// damage producer accounted for.
    ///
    /// # Errors
    /// The frame's failure.
    pub(crate) fn render_unmanaged(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<crate::raster::PresentDisposition, EngineError> {
        use crate::raster::RasterBackend;

        self.frame.begin_unmanaged();
        let result = self.render_scene(scene);
        self.frame.end_unmanaged();
        result
    }
}

/// The capture's side of `FrameProtocol::run`.
#[cfg(test)]
struct CaptureFrame<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    painter: &'a mut WgpuPainter,
    offscreen: &'a mut crate::offscreen::OffscreenRenderer,
    scene: &'a flui_layer::Scene,
    fail: &'a mut bool,
}

#[cfg(test)]
impl crate::frame_protocol::FrameSteps for CaptureFrame<'_> {
    fn clear(&mut self, view: &wgpu::TextureView) {
        clear_to_background(self.device, self.queue, view);
    }

    fn content(
        &mut self,
        view: &wgpu::TextureView,
        texture: &wgpu::Texture,
        retained: bool,
        partial: Option<flui_foundation::geometry::Rect<f64>>,
    ) -> EngineResult<bool> {
        if std::mem::take(self.fail) {
            return Err(EngineError::Timeout);
        }
        let straddled = crate::Renderer::record_frame_content(
            self.painter,
            self.offscreen,
            self.scene,
            (view, texture),
            crate::renderer::RenderContext {
                supports_copy_src: true,
                intermediate_active: retained,
            },
            partial,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("FLUI Retained Capture Encoder"),
            });
        let rendered = self
            .painter
            .render(RenderTarget::sampleable(view, texture), &mut encoder);
        if rendered.is_ok() {
            self.queue.submit(std::iter::once(encoder.finish()));
        }
        self.painter.end_frame_maintenance();
        rendered.map(|()| straddled)
    }

    fn blit(&mut self, retained: &wgpu::Texture, surface: &wgpu::TextureView) {
        self.offscreen
            .blit_to_surface(retained, surface, CAPTURE_FORMAT);
    }
}

#[cfg(test)]
impl crate::raster::RasterBackend for RetainedCapture {
    fn render_scene(
        &mut self,
        scene: &flui_layer::Scene,
    ) -> Result<crate::raster::PresentDisposition, EngineError> {
        use crate::damage::FramePlan;
        use crate::raster::PresentDisposition;

        let plan = self.frame.plan(self.intermediate_required);
        self.last_plan = Some(plan);
        if plan == FramePlan::Skip {
            return Ok(PresentDisposition::NoDamage);
        }
        let mut steps = CaptureFrame {
            device: &self.device,
            queue: &self.queue,
            painter: &mut self.painter,
            offscreen: &mut self.offscreen,
            scene,
            fail: &mut self.fail_after_begin,
        };
        self.frame.run(
            plan,
            &self.device,
            self.size,
            CAPTURE_FORMAT,
            (&self.surface_view, &self.surface),
            &mut steps,
        )?;
        self.frame.presented();
        Ok(PresentDisposition::Presented)
    }

    fn resize(&mut self, _width: u32, _height: u32) {
        // The capture's size is fixed at construction; a resize is modelled
        // by what the windowed renderer does to its frame state.
        self.frame.surface_changed();
    }

    fn is_device_lost(&self) -> bool {
        false
    }

    fn mark_dirty(&mut self, rect: flui_foundation::geometry::Rect<f64>) {
        self.frame.mark_dirty(rect);
    }

    fn mark_full_repaint(&mut self) {
        self.frame.mark_full_repaint();
    }

    fn has_damage(&self) -> bool {
        self.frame.has_damage()
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
        Ok(())
    }
}

/// How long a capture waits for its readback before reporting the GPU stalled.
///
/// Generous against a software rasterizer on a loaded CI runner, and far below
/// nextest's ten-minute kill, so a stall is a failure the caller sees, not a
/// process that never returns.
const READBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The readback's poll: wait for `submission` (the copy the capture just
/// submitted), at most [`READBACK_TIMEOUT`].
///
/// Generic over the index (`wgpu::PollType` is this enum at
/// `wgpu::SubmissionIndex`) so a test can inspect the wait without a device.
const fn readback_wait<T>(submission: T) -> wgpu::wgt::PollType<T> {
    wgpu::wgt::PollType::Wait {
        submission_index: Some(submission),
        timeout: Some(READBACK_TIMEOUT),
    }
}

/// The readback wait's outcome as the capture's result: a wait that ran out
/// is [`EngineError::ReadbackTimedOut`]; a wrong submission index cannot
/// happen, because the index is the one the copy's own submit returned.
fn readback_wait_outcome(
    polled: Result<wgpu::PollStatus, wgpu::PollError>,
    waited: std::time::Duration,
) -> EngineResult<()> {
    match polled {
        Ok(_) => Ok(()),
        Err(wgpu::PollError::Timeout) => Err(EngineError::ReadbackTimedOut { waited }),
        Err(wgpu::PollError::WrongSubmissionIndex(requested, completed)) => unreachable!(
            "BUG: the readback waits on the index its own submit returned, yet wgpu \
             reported index {requested} (last completed {completed}) as never submitted"
        ),
    }
}

/// The headless capture's visit steps: every node renders and cleans up the
/// same way, with no diverted subtree handlers, so this is the plain shape
/// [`walk_layer_tree`](crate::layer_walk::walk_layer_tree) drives.
struct CaptureVisitor<'a, 'b> {
    backend: &'a mut LayerDispatcher<'b>,
}

impl crate::layer_walk::LayerVisitor for CaptureVisitor<'_, '_> {
    fn enter(
        &mut self,
        _tree: &LayerTree,
        _id: LayerId,
        layer: &flui_layer::Layer,
    ) -> crate::layer_walk::Step {
        layer.render(self.backend);
        crate::layer_walk::Step::Descend
    }

    fn exit(&mut self, _tree: &LayerTree, _id: LayerId, layer: &flui_layer::Layer) {
        layer.cleanup(self.backend);
    }
}

#[cfg(test)]
mod target_size_tests {
    use super::HeadlessRenderer;
    use crate::error::EngineError;
    use flui_layer::LayerTree;

    /// A zero-sized capture is a `Result`, not a panic.
    ///
    /// wgpu rejects a zero-byte `MAP_READ` buffer by panicking inside
    /// `wgpu-core` (`BufferSize::new(..).unwrap()`), so without this guard a
    /// caller that took the size from user input — `cargo run -p flui
    /// --example screenshot -- material 0 0` — aborts the process. The guard
    /// runs before any GPU work, so this needs no device: the assertions below
    /// are reached even where `HeadlessRenderer::new` would fail.
    pub(super) fn zero_sized_capture_is_a_typed_error() {
        let Ok(renderer) = pollster::block_on(HeadlessRenderer::new()) else {
            // No adapter on this host: the guard is still reachable through
            // the error variant's own classification test in `error.rs`.
            return;
        };
        let tree = LayerTree::default();

        // `2^30` is the case that matters most: it is NOT zero, so it passes a
        // naive zero-check, and `width * 4` in `u32` wraps to exactly zero —
        // which would size the staging buffer at zero bytes and re-enter the
        // panic this guard exists to prevent.
        for size in [(0, 760), (900, 0), (0, 0), (1 << 30, 1), (65536, 65536)] {
            match renderer.render_layer_tree(&tree, size) {
                Err(EngineError::InvalidTargetSize { width, height }) => {
                    assert_eq!((width, height), size, "the error carries the request");
                }
                other => panic!("expected InvalidTargetSize for {size:?}, got {other:?}"),
            }
        }
    }
}

/// Serialized with the other GPU readbacks: its module name matches the
/// `gpu-readback` test group in `.config/nextest.toml`.
#[cfg(test)]
mod twin_readback_tests {
    use flui_layer::LayerTree;

    /// A renderer and its feature-reduced twin, created, used and dropped over
    /// and over, never block.
    ///
    /// The twin used to come from a `wgpu::Instance` of its own, which left two
    /// instances' devices on one adapter; on a Windows host tearing such a pair
    /// down blocked inside the driver about once in eight, until nextest killed
    /// the test after ten minutes. Twelve cycles make a regression near-certain
    /// to show there.
    ///
    /// The instance count is the deterministic half: the hang only shows on
    /// some hosts and some runs, but a twin built from a fresh instance makes
    /// the pair cost two instances every time. (`wgpu::Adapter` equality cannot
    /// say this: an adapter from a fresh instance compares equal.)
    ///
    /// Red-check: build the twin with `HeadlessRenderer::acquire` (a fresh
    /// instance) instead of from the first renderer's adapter.
    #[test]
    fn twin_renderers_tear_down_without_blocking() {
        // Also here, so the one headless GPU test carries both contracts: a
        // zero-sized or overflowing capture is a typed error, not a panic.
        super::target_size_tests::zero_sized_capture_is_a_typed_error();
        let instances = || super::INSTANCES_CREATED.with(std::cell::Cell::get);
        for _ in 0..12 {
            let before = instances();
            let Some(renderer) = crate::test_support::renderer_or_skip() else {
                return;
            };
            let twin = pollster::block_on(renderer.without_dual_source_blending())
                .expect("an adapter that answered once must answer again with fewer features");
            let created = instances() - before;
            if created != 1 {
                // Leaked, not dropped: dropping two instances' devices is the
                // teardown that blocks, and the failure should report at once.
                std::mem::forget(twin);
                std::mem::forget(renderer);
                panic!("a renderer and its twin must share one wgpu::Instance; created {created}");
            }
            let tree = LayerTree::default();
            for capture in [&renderer, &twin] {
                let pixels = capture
                    .render_layer_tree(&tree, (8, 8))
                    .expect("an empty tree captures");
                assert_eq!(pixels.len(), 8 * 8 * 4);
            }
            drop(twin);
            drop(renderer);
        }
    }
}
