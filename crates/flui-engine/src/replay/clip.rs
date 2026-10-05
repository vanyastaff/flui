//! Immutable per-target clip bindings; mask coordinates belong to the attachment.
use super::GpuReplay;
use crate::{
    clip_chain::ClipChain,
    clip_mask::{ClipMaskPipeline, MaskEdge, MaskMapping, MaskNode},
    command_ir::DrawSegment,
    device_domain::PreparedCost,
    error::{EngineResult, GeometryError},
    pipeline_set::PipelineSet,
    resources::GpuResources,
};
use wgpu::util::DeviceExt;

impl GpuReplay {
    pub(super) fn create_target_binding(
        &self,
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        mask: &wgpu::TextureView,
        origin: (i32, i32),
        enabled: bool,
    ) -> EngineResult<wgpu::BindGroup> {
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: 32,
            cpu_bytes: 32,
            objects: 3,
        })?;
        let viewport = [
            self.uniform_size.0 as f32,
            self.uniform_size.1 as f32,
            self.attachment_origin.0 as f32,
            self.attachment_origin.1 as f32,
        ];
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Immutable clip viewport"),
            contents: bytemuck::cast_slice(&viewport),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let consumer = [origin.0, origin.1, i32::from(enabled), 0];
        let consumer_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Immutable clip consumer"),
            contents: bytemuck::cast_slice(&consumer),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        Ok(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Immutable target clip binding"),
            layout: pipelines.viewport_bind_group_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: consumer_buffer.as_entire_binding(),
                },
            ],
        }))
    }

    #[expect(clippy::too_many_arguments)]
    pub(crate) fn prepare_clip_binding(
        &mut self,
        segment: &DrawSegment,
        clip: &ClipChain,
        attachment_size: (u32, u32),
        device: &wgpu::Device,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<wgpu::BindGroup> {
        if clip.is_unclipped() {
            return self.create_target_binding(
                device,
                pipelines,
                resources,
                &self.dummy_mask_view,
                (0, 0),
                false,
            );
        }
        let mut m = segment.attachment_to_root;
        let (x, y) = self.attachment_origin;
        m[4] += m[0] * x as f64 + m[2] * y as f64;
        m[5] += m[1] * x as f64 + m[3] * y as f64;
        if m.iter().any(|v| !v.is_finite()) {
            return Err(GeometryError::NonFinite {
                context: "clip attachment mapping",
            }
            .into());
        }
        let determinant = m[0] * m[3] - m[1] * m[2];
        if !determinant.is_finite() || determinant == 0.0 {
            return Err(GeometryError::Unrepresentable {
                context: "clip attachment inverse",
            }
            .into());
        }
        let mut crop = [
            0.0,
            0.0,
            f64::from(attachment_size.0),
            f64::from(attachment_size.1),
        ];
        if let Some(bounds) = clip.root_bounds()? {
            let mut mapped = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for x in [bounds.left(), bounds.right()] {
                for y in [bounds.top(), bounds.bottom()] {
                    let x = x - m[4];
                    let y = y - m[5];
                    let p = [
                        (m[3] * x - m[2] * y) / determinant,
                        (-m[1] * x + m[0] * y) / determinant,
                    ];
                    mapped[0] = mapped[0].min(p[0]);
                    mapped[1] = mapped[1].min(p[1]);
                    mapped[2] = mapped[2].max(p[0]);
                    mapped[3] = mapped[3].max(p[1]);
                }
            }
            crop = [
                mapped[0].floor().max(0.0),
                mapped[1].floor().max(0.0),
                mapped[2].ceil().min(crop[2]),
                mapped[3].ceil().min(crop[3]),
            ];
            if bounds.is_empty() {
                crop = [0.0; 4];
            }
        }
        let empty = crop[2] <= crop[0] || crop[3] <= crop[1];
        let origin = if empty {
            (0, 0)
        } else {
            (crop[0] as i32, crop[1] as i32)
        };
        let extent = if empty {
            (1, 1)
        } else {
            ((crop[2] - crop[0]) as u32, (crop[3] - crop[1]) as u32)
        };
        let scratch = 64 * std::mem::size_of::<MaskNode>()
            + 512 * std::mem::size_of::<MaskEdge>()
            + (2 * 512 + 1) * std::mem::size_of::<lyon::path::PathEvent>();
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: 0,
            cpu_bytes: scratch,
            objects: 3,
        })?;
        let mut tape = clip.lower()?;
        if empty {
            tape.nodes.clear();
            tape.edges.clear();
            tape.nodes.push(MaskNode {
                bounds: [0.0; 4],
                radii_x: [0.0; 4],
                radii_y: [0.0; 4],
                inverse: [1.0, 0.0, 0.0, 1.0],
                translation: [0.0; 4],
                meta: [0, 0, 1, 0],
                path_range: [0; 4],
            });
        }
        let edge_visits = tape
            .nodes
            .iter()
            .filter(|n| n.meta[0] == 3)
            .map(|n| n.path_range[1] as usize)
            .sum();
        let all_hard = tape.nodes.iter().all(|node| node.meta[2] == 1);
        let work = crate::clip_mask::work_units(extent, tape.nodes.len(), edge_visits, all_hard)?;
        segment.budget.admit_clip_work(work)?;
        let mapping = MaskMapping {
            attachment_to_root: [m[0] as f32, m[1] as f32, m[2] as f32, m[3] as f32],
            translation: [m[4] as f32, m[5] as f32, origin.0 as f32, origin.1 as f32],
            extent_counts: [extent.0, extent.1, 0, 0],
        };
        crate::clip_mask::validate_device_limits(device)?;
        if self.clip_mask_pipeline.is_none() {
            resources.reserve_prepared(PreparedCost {
                gpu_bytes: 0,
                cpu_bytes: 0,
                objects: 3,
            })?;
        }
        let pipeline = self
            .clip_mask_pipeline
            .get_or_insert_with(|| ClipMaskPipeline::new(device));
        let mask = pipeline.prepare(
            device,
            resources,
            encoder,
            &tape.nodes,
            &tape.edges,
            mapping,
            work,
        )?;
        self.create_target_binding(device, pipelines, resources, &mask.view, mask.origin, true)
    }
}
