//! Per-pixel color-matrix filter pass: applies a 5×4 color matrix to a
//! premultiplied layer offscreen, writing the filtered result into a 2nd
//! pooled texture (ping-pong).
//!
//! The only public entry point is `apply_color_matrix`, called by
//! `GpuReplay::flush_opacity_layer` when a `LayerFilter::ColorMatrix`
//! is present on the pending layer.
//!
//! ## Correctness contract
//!
//! - The source texture is premultiplied RGBA.
//! - The matrix operates on straight (un-premultiplied) RGBA — the WGSL shader
//!   unpremultiplies, applies the matrix, clamps to `[0, 1]` per channel, and
//!   re-premultiplies before writing.  This is bit-identical to
//!   [`flui_types::painting::ColorMatrix::apply`] on the straight color.
//! - The output texture is rendered with `LoadOp::Clear(TRANSPARENT)` so
//!   pixels outside the viewport are transparent.
//! - `BlendState::REPLACE` is used; the GPU must not re-blend the result.
//!
//! ## Ping-pong
//!
//! The source (`layer_tex`) is bound as a texture; the destination
//! (`filtered_tex`) is the render attachment.  They are distinct pooled
//! textures so there is no read/write aliasing.

use std::sync::Arc;

use bytemuck::cast_slice;

pub(crate) use pipeline::ColorMatrixPipeline;
use pipeline::color_matrix_uniform_from_values;

use super::{resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

use generated::color_matrix;

/// Apply a 5×4 color matrix to `source_tex`, writing filtered premultiplied
/// RGBA into a freshly-acquired pooled texture returned to the caller.
///
/// The caller must composite (or otherwise use) the returned texture before
/// dropping it.  Dropping without compositing is not a correctness error —
/// the texture returns to the pool — but produces an invisible layer.
///
/// ## Parameters
///
/// - `matrix_values` — flat row-major `[f32; 20]` color matrix:
///   `[r0..r3, off_r,  g0..g3, off_g,  b0..b3, off_b,  a0..a3, off_a]`.
/// - `source_tex` — premultiplied RGBA offscreen from `render_layer_to_offscreen`.
/// - `viewport_size` — `(width, height)` in physical pixels; the output
///   texture is the same size as the source.
#[allow(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/queue/pipeline/encoder plus the operation inputs"
)]
pub(crate) fn apply_color_matrix(
    matrix_values: [f32; 20],
    source_tex: &PooledTexture,
    viewport_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &ColorMatrixPipeline,
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

    // Build the uniform buffer from the flat 20-element matrix.
    // Per-filtered-layer-per-frame allocation is intentional: the buffer is tiny
    // (80 bytes) and bind-group-layout identity requires the uniform to be freshly
    // bound per draw.  Hoisting into the pipeline would couple the pipeline to a
    // single matrix value and break multi-layer filtering.
    let uniform = color_matrix_uniform_from_values(matrix_values);
    let uniform_buffer = resources.uniform_pool_mut().alloc(cast_slice(&[uniform]));

    // Nearest + ClampToEdge sampler — per-draw allocation (same rationale as above).
    // No filtering: source texels are pixel-aligned with the output; bilinear
    // filtering would introduce color error.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Color Matrix Nearest Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    // Per-draw bind group via the generated typed helper.  `WgpuBindGroup0::from_bindings`
    // recreates the layout from the WGSL-derived descriptor, so no shared layout object
    // needs to be threaded through from the pipeline.
    let bind_group = color_matrix::WgpuBindGroup0::from_bindings(
        device,
        color_matrix::WgpuBindGroup0Entries::new(color_matrix::WgpuBindGroup0EntriesParams {
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
    // LoadOp::Clear ensures pixels outside the source content are transparent,
    // matching the R3 invariant for offscreen passes.
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Color Matrix Filter Pass"),
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
