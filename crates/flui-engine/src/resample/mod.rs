//! Affine resample: a texture seen through an affine map, read nearest or
//! bilinearly, into a new texture.
//!
//! [`apply_resample`](crate::resample::apply_resample) is the pass a transform image filter lowers to. Each
//! output texel shows the source at the map of its centre, so rotation,
//! shear and scale need no special case. Past the source's edge the sampler
//! clamps: a caller that wants transparency there pads the source with
//! transparent texels as far as the sampler reaches.

use std::sync::Arc;

use bytemuck::cast_slice;

pub(crate) use pipeline::ResamplePipeline;

use crate::{resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

use generated::resample;

/// `source` (of `source_size` texels) resampled into a new texture of
/// `output_size` texels: the output texel whose centre is `p` shows the
/// source at `map · p`, both in texels from each texture's top-left corner,
/// read through `filter`.
#[expect(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/encoder/pipeline/resources plus the operation inputs"
)]
pub(crate) fn apply_resample(
    map: glam::DAffine2,
    filter: wgpu::FilterMode,
    source: &PooledTexture,
    source_size: (u32, u32),
    output_size: (u32, u32),
    surface_format: wgpu::TextureFormat,
    pipeline: &ResamplePipeline,
    resources: &mut GpuResources,
    device: &Arc<wgpu::Device>,
    encoder: &mut wgpu::CommandEncoder,
) -> PooledTexture {
    let output =
        resources
            .layer_texture_pool_mut()
            .acquire(output_size.0, output_size.1, surface_format);
    let [xx, yx, xy, yy, x0, y0] = map.to_cols_array();
    let uniform = resample::ResampleUniforms::new(
        [xx as f32, xy as f32, x0 as f32, 0.0],
        [yx as f32, yy as f32, y0 as f32, 0.0],
        [source_size.0 as f32, source_size.1 as f32],
    );
    let uniform_buffer = resources.uniform_pool_mut().alloc(cast_slice(&[uniform]));
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Resample Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let bind_group = resample::WgpuBindGroup0::from_bindings(
        device,
        resample::WgpuBindGroup0Entries::new(resample::WgpuBindGroup0EntriesParams {
            u: wgpu::BufferBinding {
                buffer: uniform_buffer,
                offset: 0,
                size: None,
            },
            src_texture: source.view(),
            src_sampler: &sampler,
        }),
    );
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FLUI Resample Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output.view(),
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
        render_pass.draw(0..6, 0..1);
    }
    output
}
