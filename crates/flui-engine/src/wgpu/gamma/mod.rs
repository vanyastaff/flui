//! Per-pixel sRGB ↔ linear-light gamma transfer filter pass.
//!
//! Applies the IEC 61966-2-1 piecewise transfer function to each RGB channel of a
//! premultiplied layer offscreen.  Alpha is always passed through unchanged.
//!
//! The only public entry point is `apply_gamma`, called by
//! `fold_layer_filter_chain` in `opacity_layer.rs` when a
//! [`command_ir::LayerFilter::Gamma`] is present on a pending layer.
//!
//! ## Correctness contract
//!
//! - The source texture is premultiplied RGBA.
//! - The transfer function operates on **straight** (un-premultiplied) RGB — the
//!   WGSL shader unpremultiplies, applies the per-channel transfer, clamps to
//!   `[0, 1]`, and re-premultiplies before writing.
//! - Alpha is left unchanged (it is not part of the gamma transfer).
//! - The CPU oracle is [`flui_types::styling::color::srgb_to_linear`] /
//!   [`flui_types::styling::color::linear_to_srgb`] — the same functions used in
//!   the GPU readback tests.
//!
//! ## Ping-pong
//!
//! The source (`layer_tex`) is bound as a texture; the destination
//! (`filtered_tex`) is the render attachment.  They are distinct pooled
//! textures so there is no read/write aliasing.

use std::sync::Arc;

use bytemuck::cast_slice;

pub(crate) use pipeline::GammaPipeline;
use pipeline::gamma_direction_to_u32;

use super::{command_ir::GammaDirection, resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

use generated::gamma;

/// Apply the sRGB ↔ linear-light gamma transfer to `source_tex`, writing the
/// filtered premultiplied RGBA into a freshly-acquired pooled texture.
///
/// The caller must composite (or otherwise use) the returned texture before
/// dropping it.  Dropping without compositing is not a correctness error —
/// the texture returns to the pool — but produces an invisible layer.
///
/// ## Parameters
///
/// - `direction` — which transfer direction to apply (sRGB→linear or linear→sRGB).
/// - `source_tex` — premultiplied RGBA offscreen from `render_layer_to_offscreen`.
/// - `viewport_size` — `(width, height)` in physical pixels; the output texture
///   is the same size as the source.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/encoder/pipeline/resources plus the operation inputs"
)]
pub(crate) fn apply_gamma(
    direction: GammaDirection,
    source_tex: &PooledTexture,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &GammaPipeline,
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

    // Build the uniform: direction flag; padding is zero-filled by the generated ctor.
    // The reusable pool writes it into a frame-distinct buffer (no per-call alloc).
    let uniform = gamma::GammaUniforms::new(gamma_direction_to_u32(direction));
    let uniform_buffer = resources.uniform_pool_mut().alloc(cast_slice(&[uniform]));

    // Nearest + ClampToEdge sampler — no filtering: source texels are pixel-aligned.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Gamma Nearest Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // Per-draw bind group: uniform (0) + source texture (1) + sampler (2).
    // The generated `WgpuBindGroup0::from_bindings` creates a compatible layout
    // internally — wgpu validates bind-group / pipeline-layout compatibility by
    // descriptor equality, so no layout object needs to be shared.
    let bind_group = gamma::WgpuBindGroup0::from_bindings(
        device,
        gamma::WgpuBindGroup0Entries::new(gamma::WgpuBindGroup0EntriesParams {
            u: wgpu::BufferBinding {
                buffer: uniform_buffer,
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
            label: Some("Gamma Filter Pass"),
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
