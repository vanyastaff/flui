//! Destination-sensitive primitive replay with independent geometric coverage.
use super::{GpuReplay, PreparedTess, clamp_scissor_to_attachment, set_clamped_scissor};
use crate::{
    command_ir::{DrawRun, DrawSegment, RecordedRun, ScissorRect},
    effects_pipeline::GradientKind,
    error::EngineResult,
    pipeline_set::PipelineSet,
    portable_coverage::PreparedCoverage,
    render_target::RenderTarget,
    resources::GpuResources,
};
use flui_painting::paint::BlendMode;
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub(super) fn needs_portable(device: &wgpu::Device, mode: BlendMode) -> bool {
    mode == BlendMode::Plus
        || (!device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
            && crate::pipeline_cache::destination_alpha_scale_for(mode).is_some())
}

// Decoded image pipelines have one color output. Even on dual-source devices,
// destination-sensitive image coverage uses the shared portable compositor.
pub(super) fn image_needs_portable(mode: BlendMode) -> bool {
    mode == BlendMode::Plus || crate::pipeline_cache::destination_alpha_scale_for(mode).is_some()
}

pub(super) fn run_needs_portable(
    device: &wgpu::Device,
    segment: &DrawSegment,
    run: &RecordedRun,
) -> bool {
    match &run.kind {
        DrawRun::CachedImage(range) => {
            run.clip.has_antialias()
                && segment.cached_images[range.clone()]
                    .iter()
                    .any(|(_, _, _, mode)| image_needs_portable(*mode))
        }
        DrawRun::Tess(range) => {
            run.clip.has_antialias()
                && segment.tess_batches[range.clone()]
                    .iter()
                    .any(|batch| needs_portable(device, batch.pipeline_key.blend_mode()))
        }
        DrawRun::LinearGradient(range) => {
            gradient_needs_portable(device, range, &segment.linear_gradient_runs)
        }
        DrawRun::RadialGradient(range) => {
            gradient_needs_portable(device, range, &segment.radial_gradient_runs)
        }
        DrawRun::SweepGradient(range) => {
            gradient_needs_portable(device, range, &segment.sweep_gradient_runs)
        }
        _ => false,
    }
}

fn gradient_needs_portable(
    device: &wgpu::Device,
    range: &std::ops::Range<usize>,
    runs: &[crate::command_ir::GradientRun],
) -> bool {
    // All gradient shaders apply SDF edge AA, including zero-radius rectangles.
    // Neither corner radii nor the clip chain can rule out partial coverage;
    // Paint::anti_alias is not carried by gradient instances.
    runs.iter().any(|run| {
        (run.start as usize) < range.end
            && (run.start + run.count) as usize > range.start
            && needs_portable(device, run.blend)
    })
}

// Bounds come from the geometry sent to the vertex stage, scaled by the same
// requested viewport. Scissor is a conservative intersection, never coverage.
struct CoverageCrop {
    region: (u32, u32, u32, u32),
    scissor: (u32, u32, u32, u32),
}

fn crop(
    points: impl Iterator<Item = [f32; 2]>,
    uniform: (u32, u32),
    viewport: (u32, u32),
    scissor: ScissorRect,
    origin: (i64, i64),
) -> Option<CoverageCrop> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for point in points {
        let x =
            (f64::from(point[0]) - origin.0 as f64) * f64::from(viewport.0) / f64::from(uniform.0);
        let y =
            (f64::from(point[1]) - origin.1 as f64) * f64::from(viewport.1) / f64::from(uniform.1);
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    }
    let cut = match scissor {
        Some((x, y, w, h)) => {
            clamp_scissor_to_attachment(x, y, w, h, viewport.0, viewport.1, origin)?
        }
        None => (0, 0, viewport.0, viewport.1),
    };
    let left = (bounds[0].floor() - 1.0).max(f64::from(cut.0));
    let top = (bounds[1].floor() - 1.0).max(f64::from(cut.1));
    let right = (bounds[2].ceil() + 1.0).min(f64::from(cut.0 + cut.2));
    let bottom = (bounds[3].ceil() + 1.0).min(f64::from(cut.1 + cut.3));
    if right <= left || bottom <= top {
        return None;
    }
    let scissor = (
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    );
    // Preserve the attachment's 2x2 derivative quads at rounded SDF edges.
    // Alignment may extend before the original scissor, which stays separate.
    let x = scissor.0 & !1;
    let y = scissor.1 & !1;
    Some(CoverageCrop {
        region: (x, y, right as u32 - x, bottom as u32 - y),
        scissor,
    })
}

fn mapping_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    region: (u32, u32, u32, u32),
    viewport: (u32, u32),
) -> wgpu::BindGroup {
    let data = [
        region.0 as f32,
        region.1 as f32,
        region.2 as f32,
        region.3 as f32,
        viewport.0 as f32,
        viewport.1 as f32,
        0.0,
        0.0,
    ];
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Immutable primitive crop mapping"),
        contents: bytemuck::cast_slice(&data),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Primitive crop mapping"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn isolation_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    scratch: &'a PreparedCoverage,
    crop: &CoverageCrop,
) -> wgpu::RenderPass<'a> {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Isolate paint and geometric coverage"),
        color_attachments: &[
            Some(wgpu::RenderPassColorAttachment {
                view: scratch.source_view(),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: scratch.coverage_view(),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            }),
        ],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_scissor_rect(
        crop.scissor.0 - crop.region.0,
        crop.scissor.1 - crop.region.1,
        crop.scissor.2,
        crop.scissor.3,
    );
    pass
}

impl GpuReplay {
    #[expect(
        clippy::too_many_arguments,
        reason = "Shares frame-owned buffers and mutable GPU resources with the ordered replay visitor"
    )]
    pub(super) fn flush_coverage_run(
        &mut self,
        segment: &DrawSegment,
        run: &DrawRun,
        tess: Option<&PreparedTess>,
        buffers: &[Option<wgpu::Buffer>; 8],
        stops: Option<&wgpu::BindGroup>,
        viewport: (u32, u32),
        device: &Arc<wgpu::Device>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
    ) -> EngineResult<()> {
        if let DrawRun::CachedImage(range) = run {
            for (key, instance, scissor, mode) in &segment.cached_images[range.clone()] {
                let Some(crop) = crop(
                    instance.corners().into_iter(),
                    self.uniform_size,
                    viewport,
                    *scissor,
                    self.attachment_origin,
                ) else {
                    continue;
                };
                let Some(texture) = resources
                    .texture_cache_mut()
                    .get(key)
                    .map(|cached| cached.view.clone())
                else {
                    continue;
                };
                let scratch =
                    PreparedCoverage::prepare(device, resources, target, crop.region, encoder)?;
                let bytes = bytemuck::bytes_of(instance);
                resources.reserve_prepared(crate::device_domain::PreparedCost {
                    gpu_bytes: bytes.len(),
                    cpu_bytes: bytes.len(),
                    objects: 2,
                })?;
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Isolated image quad"),
                    contents: bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let texture_binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Isolated image source"),
                    layout: &pipelines.texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&texture),
                        },
                    ],
                });
                let (mapping_layout, pipeline) = pipelines.image_isolation(device);
                let mapping = mapping_binding(device, mapping_layout, crop.region, viewport);
                {
                    let mut pass = isolation_pass(encoder, &scratch, &crop);
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &texture_binding, &[]);
                    pass.set_bind_group(2, &mapping, &[]);
                    pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
                    pass.set_vertex_buffer(1, buffer.slice(..));
                    pass.set_index_buffer(
                        self.unit_quad_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.draw_indexed(0..6, 0, 0..1);
                }
                scratch.composite(
                    pipelines.portable_coverage(device),
                    device,
                    encoder,
                    target,
                    *mode,
                    Some(crop.scissor),
                )?;
            }
            return Ok(());
        }
        if let DrawRun::Tess(range) = run {
            let Some(tess) = tess else {
                return Ok(());
            };
            for index in range.clone() {
                let batch = &segment.tess_batches[index];
                let mode = batch.pipeline_key.blend_mode();
                if !needs_portable(device, mode) {
                    self.flush_tessellated_geometry(
                        segment,
                        index..index + 1,
                        Some(tess),
                        viewport,
                        device,
                        pipelines,
                        encoder,
                        target.view,
                    );
                    continue;
                }
                let points = segment.indices
                    [batch.index_start as usize..(batch.index_start + batch.index_count) as usize]
                    .iter()
                    .map(|index| segment.vertices[*index as usize].position);
                let Some(crop) = crop(
                    points,
                    self.uniform_size,
                    viewport,
                    batch.scissor,
                    self.attachment_origin,
                ) else {
                    continue;
                };
                let region = crop.region;
                let scratch =
                    PreparedCoverage::prepare(device, resources, target, region, encoder)?;
                let cache = pipelines.shape_cache_mut();
                cache.ensure_isolation(device, batch.pipeline_key);
                let mapping = mapping_binding(
                    device,
                    cache.isolation_bind_group_layout(),
                    region,
                    viewport,
                );
                {
                    let mut pass = isolation_pass(encoder, &scratch, &crop);
                    pass.set_pipeline(cache.get_isolation(batch.pipeline_key));
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_bind_group(1, &tess.clip_bind_groups[index], &[]);
                    pass.set_bind_group(2, &mapping, &[]);
                    pass.set_vertex_buffer(0, tess.vertex_buffer.slice(..));
                    pass.set_index_buffer(tess.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(
                        batch.index_start..batch.index_start + batch.index_count,
                        0,
                        0..1,
                    );
                }
                scratch.composite(
                    pipelines.portable_coverage(device),
                    device,
                    encoder,
                    target,
                    mode,
                    Some(crop.scissor),
                )?;
            }
            return Ok(());
        }
        let (kind, range, runs, buffer) = match run {
            DrawRun::LinearGradient(range) => (
                GradientKind::Linear,
                range,
                &segment.linear_gradient_runs,
                &buffers[4],
            ),
            DrawRun::RadialGradient(range) => (
                GradientKind::Radial,
                range,
                &segment.radial_gradient_runs,
                &buffers[5],
            ),
            DrawRun::SweepGradient(range) => (
                GradientKind::Sweep,
                range,
                &segment.sweep_gradient_runs,
                &buffers[6],
            ),
            _ => return Ok(()),
        };
        let (Some(buffer), Some(stops)) = (buffer, stops) else {
            return Ok(());
        };
        for run in runs.iter().filter(|run| {
            (run.start as usize) < range.end && (run.start + run.count) as usize > range.start
        }) {
            let start = range.start.max(run.start as usize);
            let end = range.end.min((run.start + run.count) as usize);
            // Isolation is per instance: a shared mode does not make overlapping
            // operations one operator. Native runs retain their instancing.
            if !needs_portable(device, run.blend) {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Native gradient between coverage operations"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipelines.gradients.get(kind, run.blend));
                pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                pass.set_bind_group(1, stops, &[]);
                pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
                pass.set_vertex_buffer(1, buffer.slice(..));
                pass.set_index_buffer(
                    self.unit_quad_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.set_viewport(0.0, 0.0, viewport.0 as f32, viewport.1 as f32, 0.0, 1.0);
                if set_clamped_scissor(
                    &mut pass,
                    run.scissor,
                    viewport.0,
                    viewport.1,
                    self.attachment_origin,
                ) {
                    pass.draw_indexed(0..6, 0, start as u32..end as u32);
                }
                continue;
            }
            for index in start..end {
                let corners = match kind {
                    GradientKind::Linear => {
                        segment.linear_gradient_batch.instances[index].device_corners()
                    }
                    GradientKind::Radial => {
                        segment.radial_gradient_batch.instances[index].device_corners()
                    }
                    GradientKind::Sweep => {
                        segment.sweep_gradient_batch.instances[index].device_corners()
                    }
                };
                let Some(crop) = crop(
                    corners.into_iter(),
                    self.uniform_size,
                    viewport,
                    run.scissor,
                    self.attachment_origin,
                ) else {
                    continue;
                };
                let region = crop.region;
                let scratch =
                    PreparedCoverage::prepare(device, resources, target, region, encoder)?;
                pipelines.gradients.ensure_isolation(device, kind);
                let mapping = mapping_binding(
                    device,
                    pipelines.gradients.isolation_bind_group_layout(),
                    region,
                    viewport,
                );
                {
                    let mut pass = isolation_pass(encoder, &scratch, &crop);
                    pass.set_pipeline(pipelines.gradients.get_isolation(kind));
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_bind_group(1, stops, &[]);
                    pass.set_bind_group(2, &mapping, &[]);
                    pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
                    pass.set_vertex_buffer(1, buffer.slice(..));
                    pass.set_index_buffer(
                        self.unit_quad_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.draw_indexed(0..6, 0, index as u32..index as u32 + 1);
                }
                scratch.composite(
                    pipelines.portable_coverage(device),
                    device,
                    encoder,
                    target,
                    run.blend,
                    Some(crop.scissor),
                )?;
            }
        }
        Ok(())
    }
}
