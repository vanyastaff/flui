//! Bounded rasterisation of an immutable root-to-leaf geometric clip tape.
//! Coverage is resolved once, after Boolean membership on a common sample grid.

use crate::{
    device_domain::PreparedCost,
    error::{EngineError, EngineResult, GeometryError},
    resources::GpuResources,
};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskNode {
    pub(crate) bounds: [f32; 4],
    pub(crate) radii_x: [f32; 4],
    pub(crate) radii_y: [f32; 4],
    pub(crate) inverse: [f32; 4],
    pub(crate) translation: [f32; 4],
    /// Shape, operation, hard flag, fill rule. Corner order TL/TR/BR/BL.
    pub(crate) meta: [u32; 4],
    pub(crate) path_range: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskEdge(pub(crate) [f32; 4]);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskMapping {
    pub(crate) attachment_to_root: [f32; 4],
    /// Root translation XY, integer mask origin in attachment coordinates ZW.
    pub(crate) translation: [f32; 4],
    /// Mask width/height; prepare overwrites node count and all-hard flag.
    pub(crate) extent_counts: [u32; 4],
}

pub(crate) struct PreparedClipMask {
    // Not pooled: encoded wgpu commands retain the immutable allocation.
    _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) origin: (i32, i32),
}

pub(crate) struct ClipMaskPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

fn limit(resource: &'static str, requested: usize, cap: usize) -> EngineError {
    EngineError::PreparedResourceLimit {
        resource,
        requested,
        limit: cap,
    }
}

/// Reject insufficient capabilities before creating the clip pipeline or buffers.
pub(crate) fn validate_device_limits(device: &wgpu::Device) -> EngineResult<()> {
    let limits = device.limits();
    if limits.max_uniform_buffers_per_shader_stage < 3 {
        return Err(limit(
            "clip fragment uniform buffers",
            3,
            limits.max_uniform_buffers_per_shader_stage as usize,
        ));
    }
    if limits.max_uniform_buffer_binding_size < 8192 {
        return Err(limit(
            "clip uniform binding bytes",
            8192,
            usize::try_from(limits.max_uniform_buffer_binding_size).unwrap_or(usize::MAX),
        ));
    }
    if limits.max_buffer_size < 8192 {
        return Err(limit(
            "clip uniform buffer bytes",
            8192,
            usize::try_from(limits.max_buffer_size).unwrap_or(usize::MAX),
        ));
    }
    Ok(())
}

/// Conservative membership count: one sample for all-hard chains, otherwise
/// 64 correlated samples for mixed or antialiased chains.
/// The caller must admit this cumulatively, not independently per mask.
pub(crate) fn work_units(
    extent: (u32, u32),
    nodes: usize,
    edges: usize,
    all_hard: bool,
) -> EngineResult<usize> {
    (extent.0 as usize)
        .checked_mul(extent.1 as usize)
        .and_then(|v| v.checked_mul(if all_hard { 1 } else { 64 }))
        .and_then(|v| {
            nodes
                .checked_add(edges)
                .and_then(|n| v.checked_mul(n.max(1)))
        })
        .ok_or_else(|| limit("clip membership work", usize::MAX, usize::MAX - 1))
}

impl ClipMaskPipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let uniform_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Clip Mask Tape Layout"),
            entries: &[
                uniform_entry(0),
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Clip Mask Pipeline Layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Clip Mask Membership"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/clip_mask.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Clip Mask Resolve"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
    }

    #[expect(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &self,
        device: &wgpu::Device,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        nodes: &[MaskNode],
        edges: &[MaskEdge],
        mut mapping: MaskMapping,
        admitted_work: usize,
    ) -> EngineResult<PreparedClipMask> {
        validate_device_limits(device)?;
        if nodes.len() > 64 {
            return Err(limit("clip mask nodes", nodes.len(), 64));
        }
        if edges.len() > 512 {
            return Err(limit("clip mask edges", edges.len(), 512));
        }
        let extent = (mapping.extent_counts[0], mapping.extent_counts[1]);
        if extent.0 == 0 || extent.1 == 0 {
            return Err(EngineError::InvalidTargetSize {
                width: extent.0,
                height: extent.1,
            });
        }
        let max_extent = device.limits().max_texture_dimension_2d;
        if extent.0 > max_extent || extent.1 > max_extent {
            return Err(limit(
                "clip mask extent",
                extent.0.max(extent.1) as usize,
                max_extent as usize,
            ));
        }
        if extent.0 > 16_384 || extent.1 > 16_384 {
            return Err(limit(
                "clip numeric attachment extent",
                extent.0.max(extent.1) as usize,
                16_384,
            ));
        }
        // Ranges may share edges. Charge traversal, not just distinct storage.
        let edge_visits = nodes
            .iter()
            .filter(|node| node.meta[0] == 3)
            .try_fold(0_usize, |sum, node| {
                sum.checked_add(node.path_range[1] as usize)
            })
            .ok_or_else(|| limit("clip edge traversal", usize::MAX, usize::MAX - 1))?;
        let all_hard = nodes.iter().all(|node| node.meta[2] == 1);
        let work = work_units(extent, nodes.len(), edge_visits, all_hard)?;
        if work > admitted_work {
            return Err(limit("clip membership work", work, admitted_work));
        }
        if !mapping
            .attachment_to_root
            .iter()
            .chain(&mapping.translation)
            .all(|v| v.is_finite())
        {
            return Err(GeometryError::NonFinite {
                context: "clip mask mapping",
            }
            .into());
        }
        for &value in mapping
            .attachment_to_root
            .iter()
            .chain(&mapping.translation)
        {
            crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask mapping range")?;
        }
        let mut origin = [0_i32; 2];
        for (i, v) in mapping.translation[2..].iter().copied().enumerate() {
            if v.fract() != 0.0
                || f64::from(v) < f64::from(i32::MIN)
                || f64::from(v) > f64::from(i32::MAX)
            {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask integer origin",
                }
                .into());
            }
            origin[i] = v as i32;
            if i64::from(origin[i]) + i64::from(mapping.extent_counts[i]) > i64::from(i32::MAX) {
                return Err(GeometryError::InvalidExtent.into());
            }
        }
        for node in nodes {
            for &value in node
                .bounds
                .iter()
                .chain(&node.radii_x)
                .chain(&node.radii_y)
                .chain(&node.inverse)
                .chain(&node.translation)
            {
                crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask node range")?;
            }
            if !node
                .bounds
                .iter()
                .chain(&node.radii_x)
                .chain(&node.radii_y)
                .chain(&node.inverse)
                .chain(&node.translation)
                .all(|v| v.is_finite())
            {
                return Err(GeometryError::NonFinite {
                    context: "clip mask node",
                }
                .into());
            }
            if node.meta[0] > 3 || node.meta[1] > 1 || node.meta[2] > 1 || node.meta[3] > 1 {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask node tag",
                }
                .into());
            }
            if node.bounds[2] < 0.0
                || node.bounds[3] < 0.0
                || !(node.bounds[0] + node.bounds[2]).is_finite()
                || !(node.bounds[1] + node.bounds[3]).is_finite()
            {
                return Err(GeometryError::InvalidExtent.into());
            }
            if node.radii_x.iter().chain(&node.radii_y).any(|v| *v < 0.0) {
                return Err(GeometryError::InvalidRadius.into());
            }
            let end = node.path_range[0].checked_add(node.path_range[1]);
            if node.meta[0] == 3 && end.is_none_or(|end| end as usize > edges.len()) {
                return Err(GeometryError::Unrepresentable {
                    context: "clip mask path range",
                }
                .into());
            }
        }
        if edges
            .iter()
            .any(|edge| edge.0.iter().any(|v| !v.is_finite()))
        {
            return Err(GeometryError::NonFinite {
                context: "clip mask path edge",
            }
            .into());
        }
        for edge in edges {
            for &value in &edge.0 {
                crate::clip_geometry::pack_clip_value(f64::from(value), "clip mask edge range")?;
            }
        }
        mapping.extent_counts[2] = u32::try_from(nodes.len())
            .map_err(|_| limit("clip node count", nodes.len(), u32::MAX as usize))?;
        mapping.extent_counts[3] = u32::from(all_hard);
        // Fixed uniform arrays work on baseline WebGL2 without storage buffers.
        let payload = 7168 + 8192 + std::mem::size_of::<MaskMapping>();
        let texture_bytes = (extent.0 as usize)
            .checked_mul(extent.1 as usize)
            .ok_or_else(|| limit("clip mask bytes", usize::MAX, usize::MAX - 1))?;
        let gpu_bytes = payload
            .checked_add(texture_bytes)
            .ok_or_else(|| limit("clip prepared bytes", usize::MAX, usize::MAX - 1))?;
        resources.reserve_prepared(PreparedCost {
            gpu_bytes,
            cpu_bytes: payload,
            objects: 6,
        })?;
        let mut padded_nodes = [0_u8; 7168];
        let mut padded_edges = [0_u8; 8192];
        let node_bytes = bytemuck::cast_slice(nodes);
        let edge_bytes = bytemuck::cast_slice(edges);
        padded_nodes[..node_bytes.len()].copy_from_slice(node_bytes);
        padded_edges[..edge_bytes.len()].copy_from_slice(edge_bytes);
        // Every allocation below follows admission; no pooled mask can be overwritten.
        let buffer = |label, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let nodes_buffer = buffer(
            "Immutable Clip Nodes",
            &padded_nodes,
            wgpu::BufferUsages::UNIFORM,
        );
        let edges_buffer = buffer(
            "Immutable Clip Edges",
            &padded_edges,
            wgpu::BufferUsages::UNIFORM,
        );
        let uniform = buffer(
            "Clip Mask Mapping",
            bytemuck::bytes_of(&mapping),
            wgpu::BufferUsages::UNIFORM,
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Clip Mask Tape"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: nodes_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edges_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Immutable Clip Coverage"),
            size: wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clip Boolean Membership Resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(PreparedClipMask {
            _texture: texture,
            view,
            origin: (origin[0], origin[1]),
        })
    }
}
