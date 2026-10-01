use super::{
    Arc, DrawRun, DrawSegment, GpuResources, GradientKind, PipelineSet, ScissorRect,
    clamp_scissor_to_attachment,
};

// Render-pass state is local: barriers start a fresh cache. Pipeline families
// share group zero, while glyph and gradient group one have distinct layouts.
#[derive(Default)]
struct QuadReplayState {
    scissor: Option<(u32, u32, u32, u32)>,
    secondary: Option<u8>,
    gradient_pipeline: Option<(GradientKind, flui_painting::BlendMode)>,
    vertex_slot: Option<usize>,
}

impl QuadReplayState {
    fn scissor(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        rect: ScissorRect,
        w: u32,
        h: u32,
    ) -> bool {
        let rect = match rect {
            Some((x, y, rw, rh)) => clamp_scissor_to_attachment(x, y, rw, rh, w, h),
            None => Some((0, 0, w, h)),
        };
        let Some(rect) = rect else {
            return false;
        };
        if self.scissor != Some(rect) {
            pass.set_scissor_rect(rect.0, rect.1, rect.2, rect.3);
            self.scissor = Some(rect);
        }
        true
    }
}

// Ordered replay over immutable typed arena slices.
impl super::GpuReplay {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn flush_segment(
        &mut self,
        segment: &DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> crate::error::EngineResult<()> {
        if let Some(crate::command_ir::RecordError::Limit {
            resource,
            requested,
            limit,
        }) = segment
            .record_error
            .clone()
            .or_else(|| segment.budget.error())
        {
            return Err(crate::error::EngineError::PreparedResourceLimit {
                resource,
                requested,
                limit,
            });
        }
        self.prepare_viewport_binding(device, pipelines, resources)?;
        let stops = bytemuck::cast_slice(&segment.current_gradient_stops);
        let stop_binding = if stops.is_empty() {
            None
        } else {
            let limit = device.limits().max_storage_buffer_binding_size as usize;
            if stops.len() > limit {
                return Err(crate::error::EngineError::PreparedResourceLimit {
                    resource: "gradient storage bytes",
                    requested: stops.len(),
                    limit,
                });
            }
            resources.reserve_prepared(crate::device_domain::PreparedCost {
                gpu_bytes: stops.len(),
                cpu_bytes: stops.len(),
                objects: 2,
            })?;
            Some(pipelines.prepare_gradient_bind_group(device, stops))
        };
        // Upload each typed arena once. Runs address the complete arena, so
        // alternating pipeline families neither copy nor re-upload instances.
        let arenas: [&[u8]; 8] = [
            bytemuck::cast_slice(&segment.rect_batch.instances),
            bytemuck::cast_slice(&segment.circle_batch.instances),
            bytemuck::cast_slice(&segment.arc_batch.instances),
            bytemuck::cast_slice(&segment.shadow_batch.instances),
            bytemuck::cast_slice(&segment.linear_gradient_batch.instances),
            bytemuck::cast_slice(&segment.radial_gradient_batch.instances),
            bytemuck::cast_slice(&segment.sweep_gradient_batch.instances),
            bytemuck::cast_slice(&segment.glyph_batch.instances),
        ];
        let buffers = arenas.map(|bytes| {
            (!bytes.is_empty()).then(|| {
                resources
                    .buffer_pool_mut()
                    .get_vertex_buffer(device, queue, "Ordered Typed Arena", bytes)
                    .clone()
            })
        });
        for (kind, runs) in [
            (GradientKind::Linear, &segment.linear_gradient_runs),
            (GradientKind::Radial, &segment.radial_gradient_runs),
            (GradientKind::Sweep, &segment.sweep_gradient_runs),
        ] {
            for run in runs {
                pipelines.gradients.ensure(device, kind, run.blend);
            }
        }
        let tess =
            Self::prepare_tessellated_geometry(segment, device, queue, pipelines, resources)?;
        let mut cursor = 0;
        while cursor < segment.runs.len() {
            let run = &segment.runs[cursor];
            match run {
                DrawRun::Tess(range) => self.flush_tessellated_geometry(
                    segment,
                    range.clone(),
                    tess.as_ref(),
                    viewport_size,
                    device,
                    pipelines,
                    encoder,
                    view,
                ),
                DrawRun::CachedImage(range) => self.flush_segment_cached_images(
                    segment,
                    range.clone(),
                    viewport_size,
                    device,
                    queue,
                    pipelines,
                    resources,
                    encoder,
                    view,
                ),
                DrawRun::ExternalImage(range) => self.flush_segment_external_images(
                    segment,
                    range.clone(),
                    viewport_size,
                    device,
                    queue,
                    pipelines,
                    resources,
                    encoder,
                    view,
                ),
                _ => {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Ordered Quad Runs"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                    pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
                    pass.set_index_buffer(
                        self.unit_quad_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    pass.set_viewport(
                        0.0,
                        0.0,
                        viewport_size.0 as f32,
                        viewport_size.1 as f32,
                        0.0,
                        1.0,
                    );
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    let mut state = QuadReplayState::default();
                    while cursor < segment.runs.len() {
                        let run = &segment.runs[cursor];
                        let (slot, kind) = match run {
                            DrawRun::Rect(_) => (0, None),
                            DrawRun::Circle(_) => (1, None),
                            DrawRun::Arc(_) => (2, None),
                            DrawRun::Shadow(_) => (3, None),
                            DrawRun::LinearGradient(_) => (4, Some(GradientKind::Linear)),
                            DrawRun::RadialGradient(_) => (5, Some(GradientKind::Radial)),
                            DrawRun::SweepGradient(_) => (6, Some(GradientKind::Sweep)),
                            DrawRun::Glyph(_) => (7, None),
                            _ => break,
                        };
                        if let Some(buffer) = &buffers[slot] {
                            if state.vertex_slot != Some(slot) {
                                pass.set_vertex_buffer(1, buffer.slice(..));
                                state.vertex_slot = Some(slot);
                            }
                            if let Some(kind) = kind {
                                Self::replay_gradient_run(
                                    segment,
                                    kind,
                                    run,
                                    stop_binding.as_ref(),
                                    viewport_size,
                                    pipelines,
                                    &mut pass,
                                    &mut state,
                                );
                            } else {
                                self.replay_instanced_run(
                                    segment,
                                    run,
                                    viewport_size,
                                    pipelines,
                                    &mut pass,
                                    &mut state,
                                );
                            }
                        }
                        cursor += 1;
                    }
                    continue;
                }
            }
            cursor += 1;
        }
        Ok(())
    }

    fn replay_instanced_run(
        &self,
        segment: &DrawSegment,
        run: &DrawRun,
        viewport_size: (u32, u32),
        pipelines: &PipelineSet,
        pass: &mut wgpu::RenderPass<'_>,
        state: &mut QuadReplayState,
    ) {
        let (pipeline, regions, range): (
            &wgpu::RenderPipeline,
            &[crate::command_ir::ScissorRegion],
            &std::ops::Range<usize>,
        ) = match run {
            DrawRun::Rect(r) => (&pipelines.instanced_rect, &segment.rect_scissors, r),
            DrawRun::Circle(r) => (&pipelines.instanced_circle, &segment.circle_scissors, r),
            DrawRun::Arc(r) => (&pipelines.instanced_arc, &segment.arc_scissors, r),
            DrawRun::Shadow(r) => (&pipelines.shadow, &[], r),
            DrawRun::Glyph(r) => (&pipelines.instanced_glyph, &segment.glyph_scissors, r),
            _ => return,
        };
        pass.set_pipeline(pipeline);
        state.gradient_pipeline = None;
        if matches!(run, DrawRun::Glyph(_)) {
            let Some(binding) = &self.glyph_bind_group else {
                return;
            };
            if state.secondary != Some(1) {
                pass.set_bind_group(1, binding, &[]);
                state.secondary = Some(1);
            }
        } else {
            // A pipeline without group one is a layout barrier for its reuse.
            state.secondary = None;
        }
        let (w, h) = viewport_size;
        if matches!(run, DrawRun::Shadow(_)) {
            if state.scissor(pass, None, w, h) {
                pass.draw_indexed(0..6, 0, range.start as u32..range.end as u32);
            }
        } else {
            let first = regions
                .partition_point(|region| (region.start + region.count) as usize <= range.start);
            for region in regions[first..]
                .iter()
                .take_while(|region| (region.start as usize) < range.end)
            {
                let start = range.start.max(region.start as usize);
                let end = range.end.min((region.start + region.count) as usize);
                if start < end && state.scissor(pass, region.scissor, w, h) {
                    pass.draw_indexed(0..6, 0, start as u32..end as u32);
                }
            }
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn replay_gradient_run(
        segment: &DrawSegment,
        kind: GradientKind,
        draw: &DrawRun,
        stops: Option<&wgpu::BindGroup>,
        viewport_size: (u32, u32),
        pipelines: &PipelineSet,
        pass: &mut wgpu::RenderPass<'_>,
        state: &mut QuadReplayState,
    ) {
        let (range, runs) = match draw {
            DrawRun::LinearGradient(range) => (range, &segment.linear_gradient_runs),
            DrawRun::RadialGradient(range) => (range, &segment.radial_gradient_runs),
            DrawRun::SweepGradient(range) => (range, &segment.sweep_gradient_runs),
            _ => return,
        };
        let (w, h) = viewport_size;
        let first = runs.partition_point(|run| (run.start + run.count) as usize <= range.start);
        for run in runs[first..]
            .iter()
            .take_while(|run| (run.start as usize) < range.end)
        {
            let start = range.start.max(run.start as usize);
            let end = range.end.min((run.start + run.count) as usize);
            if start >= end {
                continue;
            }
            if state.gradient_pipeline != Some((kind, run.blend)) {
                pass.set_pipeline(pipelines.gradients.get(kind, run.blend));
                state.gradient_pipeline = Some((kind, run.blend));
            }
            if state.secondary != Some(2)
                && let Some(stops) = stops
            {
                pass.set_bind_group(1, stops, &[]);
                state.secondary = Some(2);
            }
            if state.scissor(pass, run.scissor, w, h) {
                pass.draw_indexed(0..6, 0, start as u32..end as u32);
            }
        }
    }
}
