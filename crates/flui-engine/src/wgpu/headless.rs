//! Windowless GPU capture: rasterize a `LayerTree` to an offscreen texture and
//! read the pixels back to the CPU.
//!
//! The on-screen [`Renderer`](super::Renderer) hard-requires a `wgpu::Surface`
//! (its `render_scene` acquires a swapchain texture). Golden-image and
//! screenshot tooling needs the same raster path against a caller-owned
//! texture instead — so this module owns a surface-less device and the
//! layer-tree walk / readback that `Renderer::render_scene` performs between
//! surface-acquire and present.
//!
//! It renders through the sampleable [`RenderTarget`] (unlike the public
//! [`WgpuPainter::render_to_view`], which is `view_only`), so backdrop-filter
//! and advanced-blend layers that sample the destination render correctly.

use std::sync::Arc;

use flui_layer::{LayerId, LayerTree};

use super::{
    Backend, layer_render::LayerRender, painter::WgpuPainter, render_target::RenderTarget,
};
use crate::error::{EngineError, EngineResult};

/// The pixel format headless capture renders and reads back in. RGBA8 maps
/// straight to a PNG without a channel swizzle.
const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A windowless renderer that turns a [`LayerTree`] into raw RGBA8 pixels.
///
/// Construct once (device creation is the expensive step), then call
/// [`Self::render_layer_tree`] per capture.
#[allow(missing_debug_implementations)]
pub struct HeadlessRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl HeadlessRenderer {
    /// Acquire a surface-less GPU device for offscreen capture.
    ///
    /// # Errors
    /// Returns [`EngineError`] when no GPU adapter or device is available.
    pub fn new() -> EngineResult<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(EngineError::adapter_request)?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("FLUI Headless Capture Device"),
            ..Default::default()
        }))
        .map_err(EngineError::device_creation)?;

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
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
            let mut backend = Backend::new(&mut painter);
            if let Some(root) = tree.root() {
                walk_layer_tree(tree, root, &mut backend);
            }
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
    fn readback_rgba(&self, texture: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
        const BYTES_PER_PIXEL: u32 = 4;
        let unpadded_row_bytes = width * BYTES_PER_PIXEL;
        let padded_row_bytes = unpadded_row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FLUI Headless Capture Readback Staging"),
            size: u64::from(padded_row_bytes * height),
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
                    bytes_per_row: Some(padded_row_bytes),
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

        let mapped = staging.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded_row_bytes * height) as usize);
        for row in 0..height {
            let start = (row * padded_row_bytes) as usize;
            let end = start + unpadded_row_bytes as usize;
            pixels.extend_from_slice(&mapped[start..end]);
        }
        pixels
    }
}

/// Depth-first walk mirroring `Renderer::render_layer_recursive`: render a
/// node, recurse into its children, then run the node's post-children cleanup
/// (e.g. a filter container popping its offscreen scope).
fn walk_layer_tree(tree: &LayerTree, node_id: LayerId, backend: &mut Backend<'_>) {
    let Some(layer) = tree.get_layer(node_id) else {
        return;
    };
    layer.render(backend);

    let children: Vec<LayerId> = tree.children(node_id).unwrap_or_default().to_vec();
    for child_id in children {
        walk_layer_tree(tree, child_id, backend);
    }

    if let Some(layer) = tree.get_layer(node_id) {
        layer.cleanup(backend);
    }
}
