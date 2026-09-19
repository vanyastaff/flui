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

/// A windowless renderer that turns a [`LayerTree`] into raw RGBA8 pixels.
///
/// Construct once (device creation is the expensive step), then call
/// [`Self::render_layer_tree`] per capture.
#[expect(missing_debug_implementations)]
pub struct HeadlessRenderer {
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

    /// [`Self::new`] with [`wgpu::Features::DUAL_SOURCE_BLENDING`] withheld
    /// from the device even where the adapter exposes it.
    ///
    /// This is how a test reaches the folded fallback on hardware that has the
    /// feature. Every adapter this workspace's CI and dev machines run on has
    /// it — DX12 exposes it unconditionally — so without this the fallback
    /// would ship untested on every device able to exercise it, and the
    /// feathered result would have nothing to be compared against.
    ///
    /// # Errors
    /// Returns [`EngineError`] when no GPU adapter or device is available.
    #[cfg(test)]
    pub(crate) async fn without_dual_source_blending() -> EngineResult<Self> {
        Self::acquire(wgpu::Features::empty()).await
    }

    /// Acquires the capture device, requesting whichever of `wanted_features`
    /// the adapter actually offers.
    async fn acquire(wanted_features: wgpu::Features) -> EngineResult<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&crate::adapter::trusted_adapter_options(
                wgpu::PowerPreference::HighPerformance,
                None,
            ))
            .await
            .map_err(EngineError::adapter_request)?;

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

        self.clear_to_white(&view);

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

        Ok(self.readback_rgba(&texture, width, height))
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

    fn clear_to_white(&self, view: &wgpu::TextureView) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Copy the texture to a mappable buffer and de-pad the 256-byte-aligned
    /// rows into a tight `width * height * 4` RGBA8 buffer.
    ///
    /// The row arithmetic is `u64` throughout, and that is hardening rather
    /// than a fix: `render_layer_tree` bounds both axes by
    /// `max_texture_dimension_2d` before any GPU work, so `width * 4` cannot
    /// reach `u32::MAX` and the `u32` form would not wrap today. It is `u64`
    /// so that the invariant is local to this function instead of resting on
    /// a check in a different one — the failure mode it avoids is a *silent*
    /// wrap (`width = 2^30` makes `width * 4` zero) that would size the
    /// staging buffer at zero bytes and re-enter the `wgpu-core` panic, which
    /// is worth one conversion not to have to re-derive later.
    fn readback_rgba(&self, texture: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
        const BYTES_PER_PIXEL: u64 = 4;
        let align = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let unpadded_row_bytes = u64::from(width) * BYTES_PER_PIXEL;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(align) * align;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FLUI Headless Capture Readback Staging"),
            size: padded_row_bytes * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                    // construction: `render_layer_tree` bounds `width` by
                    // `max_texture_dimension_2d` before any GPU work, so
                    // `width * 4` padded to 256 is far below `u32::MAX`.
                    bytes_per_row: Some(
                        u32::try_from(padded_row_bytes).expect(
                            "BUG: render_layer_tree bounds width by                              max_texture_dimension_2d before readback",
                        ),
                    ),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect(
                "headless readback poll must complete: the submit above is the only pending work",
            );

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
        pixels
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
    #[test]
    fn zero_sized_capture_is_a_typed_error() {
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
