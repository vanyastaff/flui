//! Decoded image filter parameters, shared by recording and both replay paths.

use flui_painting::paint::ColorFilter;
use wgpu::util::DeviceExt;

/// A uniform is also the batch identity: two draws only share a batch when all
/// effective filter parameters agree. Images keep their original texture key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImageSourceFilter(generated::color_filter::ImageSourceFilter);

// The generated module owns the WGSL layout and checks every uniform offset.
#[allow(
    missing_docs,
    missing_debug_implementations,
    unsafe_code,
    unused,
    dead_code,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    elided_lifetimes_in_paths,
    unused_doc_comments,
    unreachable_pub,
    clippy::all,
    clippy::pedantic
)]
mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/image_source_filter_generated.rs"
    ));
}

impl ImageSourceFilter {
    pub(crate) fn from_filter(filter: ColorFilter) -> Option<Self> {
        let mut value = generated::color_filter::ImageSourceFilter::zeroed();
        match filter {
            ColorFilter::Mode { color, blend_mode } => {
                value.color = color.to_rgba_f32_array();
                value.mode = crate::mode::blend_mode_to_u32(blend_mode);
            }
            ColorFilter::Matrix(matrix) => {
                if !matrix.values.iter().all(|value| value.is_finite()) {
                    return None;
                }
                value.kind = 1;
                for row in 0..4 {
                    for column in 0..4 {
                        value.matrix[column][row] = matrix.values[row * 5 + column];
                    }
                    value.offset[row] = matrix.values[row * 5 + 4];
                }
            }
            ColorFilter::SrgbToLinearGamma => {
                value.kind = 2;
            }
            ColorFilter::LinearToSrgbGamma => {
                value.kind = 2;
                value.mode = 1;
            }
            _ => return None,
        }
        Some(Self(value))
    }

    pub(crate) fn prepare_bind_group(
        self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        resources: &mut crate::resources::GpuResources,
    ) -> crate::error::EngineResult<wgpu::BindGroup> {
        let limits = device.limits();
        let uniform_bytes = size_of::<generated::color_filter::ImageSourceFilter>() as u64;
        for (resource, requested, limit) in [
            (
                "image filter bind groups",
                4,
                u64::from(limits.max_bind_groups),
            ),
            (
                "image filter uniform bytes",
                uniform_bytes,
                limits.max_uniform_buffer_binding_size,
            ),
            (
                "image filter buffer bytes",
                uniform_bytes,
                limits.max_buffer_size,
            ),
        ] {
            if requested > limit {
                return Err(crate::EngineError::PreparedResourceLimit {
                    resource,
                    requested: requested as usize,
                    limit: limit as usize,
                });
            }
        }
        let bytes = bytemuck::bytes_of(&self.0);
        resources.reserve_prepared(crate::device_domain::PreparedCost {
            gpu_bytes: bytes.len(),
            cpu_bytes: bytes.len(),
            objects: 2,
        })?;
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Decoded image source filter"),
            contents: bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Ok(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Decoded image source filter"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        }))
    }
}

use bytemuck::Zeroable;

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    generated::image_source_filter_layout::WgpuBindGroup0::get_bind_group_layout(device)
}
