//! Per-pixel ColorFilter::Mode blend pass: composites a solid filter color
//! (SRC) over each layer pixel (DST) using one of the 28 Porter-Duff / W3C
//! blend modes.
//!
//! The only public entry point is `apply_mode`, called by
//! `fold_layer_filter_chain` in `opacity_layer.rs` when a
//! [`command_ir::LayerFilter::Mode`] is present on a pending layer.
//!
//! ## Correctness contract
//!
//! - The source texture is premultiplied RGBA (DST).
//! - `filter_color` is the SRC in straight sRGB `[f32; 4]` (pre-converted by
//!   the caller via [`flui_types::Color::to_f32_array`]).
//! - The GPU shader unpremultiplies the DST pixel, computes
//!   `blend(src=filter_color, dst=straight_pixel, mode)` in straight sRGB space,
//!   and emits a premultiplied result via `BlendState::REPLACE`.
//! - The CPU oracle is [`flui_types::Color::blend`] with `self = filter_color`
//!   (SRC) and `dst = pixel_color` — the same function used in the GPU readback
//!   tests.
//!
//! ## Ping-pong
//!
//! The source (`layer_tex`) is bound as a texture; the destination
//! (`filtered_tex`) is the render attachment.  They are distinct pooled
//! textures so there is no read/write aliasing.

use std::sync::Arc;

use bytemuck::cast_slice;
use flui_types::painting::BlendMode;
use wgpu::util::DeviceExt as _;

pub(crate) use pipeline::ModePipeline;
use pipeline::blend_mode_to_u32;

use generated::mode;

use super::{resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

/// Apply a Porter-Duff / W3C blend of `filter_color` (SRC) over each pixel of
/// `source_tex` (DST), writing the filtered premultiplied RGBA into a
/// freshly-acquired pooled texture.
///
/// The caller must composite (or otherwise use) the returned texture before
/// dropping it.  Dropping without compositing is not a correctness error —
/// the texture returns to the pool — but produces an invisible layer.
///
/// ## Parameters
///
/// - `filter_color` — the filter color in straight sRGB `[r, g, b, a]` where
///   each channel is in `[0, 1]`.  This is the SRC of the blend.
/// - `blend_mode` — which Porter-Duff or W3C blend to apply.
/// - `source_tex` — premultiplied RGBA offscreen from `render_layer_to_offscreen`.
/// - `viewport_size` — `(width, height)` in physical pixels; the output texture
///   is the same size as the source.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/encoder/pipeline/resources plus the operation inputs"
)]
pub(crate) fn apply_mode(
    filter_color: [f32; 4],
    blend_mode: BlendMode,
    source_tex: &PooledTexture,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &ModePipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let (vp_w, vp_h) = viewport_size;

    // Acquire the destination (filtered) texture — same size and format as source.
    let filtered_tex = resources
        .layer_texture_pool_mut()
        .acquire(vp_w, vp_h, surface_format);
    let filtered_view = filtered_tex.view();

    // Build the uniform: filter color + blend mode index.  The generated
    // `ModeUniforms::new` zero-fills the WGSL alignment padding.
    let uniform = mode::ModeUniforms::new(filter_color, blend_mode_to_u32(blend_mode));
    let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Mode Uniform Buffer"),
        contents: cast_slice(&[uniform]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    // Nearest + ClampToEdge sampler — no filtering: source texels are pixel-aligned.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Mode Nearest Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // Per-draw bind group via the generated typed helper:
    // uniform (0) + source texture (1) + sampler (2).
    let bind_group = mode::WgpuBindGroup0::from_bindings(
        device,
        mode::WgpuBindGroup0Entries::new(mode::WgpuBindGroup0EntriesParams {
            u: wgpu::BufferBinding {
                buffer: &uniform_buffer,
                offset: 0,
                size: None,
            },
            src_texture: source_tex.view(),
            src_sampler: &sampler,
        }),
    );

    // Render pass: clear to TRANSPARENT then draw the full-viewport quad.
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Mode Filter Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: filtered_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&pipeline.pipeline);
        bind_group.set(&mut render_pass);
        // 6 vertices synthesised in the VS — no vertex buffer.
        render_pass.draw(0..6, 0..1);
    }

    filtered_tex
}
