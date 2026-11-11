//! Affine resample: a texture seen through an affine map, read nearest or
//! bilinearly, into a new texture.
//!
//! [`apply_resample`](crate::resample::apply_resample) is the pass a transform image filter lowers to. Each
//! output texel shows the source at the map of its centre, so rotation,
//! shear and scale need no special case. Past the source's edge the sampler
//! clamps, and past a decal rect the caller gives it reads transparent
//! black.

use std::sync::Arc;

use bytemuck::cast_slice;
use flui_foundation::geometry::Rect;
use flui_painting::paint::{ImageSampling, ImageTransform};

pub(crate) use pipeline::ResamplePipeline;

use crate::{error::GeometryError, resources::GpuResources, texture_pool::PooledTexture};

mod generated;
mod pipeline;

use generated::resample;

/// `source` (of `source_size` texels) resampled into a new texture of
/// `output_size` texels: the output texel whose centre is `p` shows the
/// source at `map · p`, both in texels from each texture's top-left corner,
/// read through `filter`. A point outside `decal` (source texels) reads
/// transparent black.
#[expect(
    clippy::too_many_arguments,
    reason = "GPU pass functions require device/encoder/pipeline/resources plus the operation inputs"
)]
pub(crate) fn apply_resample(
    map: glam::DAffine2,
    filter: wgpu::FilterMode,
    source: &PooledTexture,
    source_size: (u32, u32),
    decal: Rect<f64>,
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
        [
            decal.left() as f32,
            decal.top() as f32,
            decal.right() as f32,
            decal.bottom() as f32,
        ],
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

/// How a resample reads between texels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResampleFilter {
    /// The texel the sample point falls in.
    Nearest,
    /// Bilinear interpolation between the four texels around it.
    Linear,
}

impl ResampleFilter {
    /// The filter `sampling` asks for, if it has one.
    pub(crate) fn of(sampling: ImageSampling) -> Option<Self> {
        match sampling {
            ImageSampling::Nearest => Some(Self::Nearest),
            ImageSampling::Linear => Some(Self::Linear),
            _ => None,
        }
    }

    /// How far past a sample point the texels it reads reach, as
    /// [`ImageSampling::reach`] gives it.
    pub(crate) fn reach(self) -> f64 {
        match self {
            Self::Nearest => ImageSampling::Nearest.reach(),
            Self::Linear => ImageSampling::Linear.reach(),
        }
    }

    /// The texels one output pixel reads.
    pub(crate) fn taps(self) -> usize {
        match self {
            Self::Nearest => 1,
            Self::Linear => 4,
        }
    }

    pub(crate) fn filter_mode(self) -> wgpu::FilterMode {
        match self {
            Self::Nearest => wgpu::FilterMode::Nearest,
            Self::Linear => wgpu::FilterMode::Linear,
        }
    }
}

/// `transform`, given in the space the 4x4 column-major `matrix` maps to
/// device pixels, as a map from a device output pixel to the device pixel
/// it shows: `matrix · transform⁻¹ · matrix⁻¹`.
pub(crate) fn device_map(
    transform: &ImageTransform,
    matrix: &[f64; 16],
) -> Result<glam::DAffine2, GeometryError> {
    let device = glam::DAffine2::from_cols_array(&[
        matrix[0], matrix[1], matrix[4], matrix[5], matrix[12], matrix[13],
    ]);
    let shown_to_source = glam::DAffine2::from_cols_array(&transform.inverse_coefficients());
    let map = device * shown_to_source * device.inverse();
    let determinant = map.matrix2.determinant();
    if !map.to_cols_array().iter().all(|value| value.is_finite()) || !determinant.is_normal() {
        return Err(GeometryError::NonFinite {
            context: "image transform",
        });
    }
    Ok(map)
}

/// The bounding box of `rect` mapped through `map`.
pub(crate) fn mapped_bounds(map: glam::DAffine2, rect: Rect<f64>) -> Rect<f64> {
    let corners = [
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
    ]
    .map(|(x, y)| map.transform_point2(glam::DVec2::new(x, y)));
    let low = corners
        .iter()
        .fold(glam::DVec2::INFINITY, |low, &corner| low.min(corner));
    let high = corners
        .iter()
        .fold(glam::DVec2::NEG_INFINITY, |high, &corner| high.max(corner));
    Rect::from_ltrb(low.x, low.y, high.x, high.y)
}
