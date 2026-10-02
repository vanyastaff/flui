//! `GpuReplay` segment-flush machinery, split out of `replay.rs` for the C1 cap.
//!
//! Holds ordered `flush_segment` and every
//! typed-arena replay helper it drives (instanced batches, gradients, tessellated
//! geometry, cached/external images, and the texture-batch blend variants). The
//! dispatch core (`new` / `submit` /
//! `reintegrate_offscreen_content`) stays in the parent `replay` module.
//!
//! These are inherent `impl GpuReplay` methods on a descendant module of
//! `replay`, so they retain access to `GpuReplay`'s private fields.

use std::sync::Arc;

use super::GpuReplay;
use crate::{
    command_ir::{DrawRun, DrawSegment, ScissorRect},
    effects_pipeline::GradientKind,
    pipeline_cache::PipelineKey,
    pipeline_set::PipelineSet,
    resources::GpuResources,
};

struct PreparedTess {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    clip_bind_groups: Vec<wgpu::BindGroup>,
}

// =============================================================================
// Scissor clamping
// =============================================================================
//
// Every recorded scissor is captured against the frame's full viewport, but a
// flush can target a smaller offscreen attachment (a grown-bounds opacity
// layer, an SSAA supersample tile). `opacity_layer.rs::render_segment_to_grown_offscreen`
// and `ssaa.rs`'s tile remap intersect each recorded scissor with the
// attachment's local bounds and, on an empty intersection, emit a deliberate
// off-target sentinel — `(full_w, full_h, 1, 1)` — to mean "fully clipped,
// draw nothing". That sentinel's origin sits exactly on the attachment's far
// edge, so `x + w` / `y + h` overshoot the attachment by one pixel: passed
// straight to `set_scissor_rect` it fails wgpu's scissor-containment
// validation. `TexturePool::acquire` sizes the offscreen target to the exact
// requested bounds, so no margin absorbs the overshoot by accident — every
// consumer of a recorded scissor must clamp it before calling
// `set_scissor_rect`, not just the tessellated-geometry path.

/// Clamp a `(x, y, w, h)` scissor rect (physical pixels) to fit inside a
/// `(full_w, full_h)` render attachment.
///
/// Returns `None` when the clamped rect has zero area — the region has no
/// visible intersection with the attachment and the caller must skip its
/// draw call. Returns `Some` with the rect clamped to
/// `[0, full_w) × [0, full_h)` otherwise (a no-op for an already in-bounds
/// rect).
fn clamp_scissor_to_attachment(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    full_w: u32,
    full_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    let clamped_x = x.min(full_w);
    let clamped_y = y.min(full_h);
    let clamped_w = w.min(full_w - clamped_x);
    let clamped_h = h.min(full_h - clamped_y);
    if clamped_w == 0 || clamped_h == 0 {
        None
    } else {
        Some((clamped_x, clamped_y, clamped_w, clamped_h))
    }
}

/// Set `render_pass`'s scissor rect for one draw region, clamped to the
/// attachment, and report whether the caller should issue its draw call.
///
/// `scissor` is `None` for "no active clip" (the full attachment) or
/// `Some((x, y, w, h))` for a recorded per-region clip in full-viewport
/// device-pixel space. Returns `false` when the clamped region has no
/// visible area, in which case the caller must skip its `draw_indexed` call
/// rather than pass a possibly out-of-bounds rect to wgpu.
///
/// Texture and tessellation replay use this helper directly; ordered quad
/// replay caches the same normalized rectangle within its render pass.
fn set_clamped_scissor(
    render_pass: &mut wgpu::RenderPass<'_>,
    scissor: ScissorRect,
    full_w: u32,
    full_h: u32,
) -> bool {
    let clamped = match scissor {
        Some((x, y, w, h)) => clamp_scissor_to_attachment(x, y, w, h, full_w, full_h),
        None => Some((0, 0, full_w, full_h)),
    };
    let Some((x, y, w, h)) = clamped else {
        return false;
    };
    render_pass.set_scissor_rect(x, y, w, h);
    true
}

// GPU rendering routinely converts between numeric types for pixel coordinates,
// color channels, buffer indices, and instance counts; flush methods also carry
// many GPU-handle parameters.
#[expect(clippy::too_many_arguments)]
impl GpuReplay {
    // =========================================================================
    // Segment-flush entry point
    // =========================================================================

    fn prepare_tessellated_geometry(
        segment: &DrawSegment,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
    ) -> crate::error::EngineResult<Option<PreparedTess>> {
        if segment.vertices.is_empty() || segment.tess_batches.is_empty() {
            return Ok(None);
        }

        resources.reserve_prepared(crate::device_domain::PreparedCost {
            gpu_bytes: segment.tess_batches.len()
                * std::mem::size_of::<crate::command_ir::ClipUniform>(),
            cpu_bytes: segment.tess_batches.len() * std::mem::size_of::<wgpu::BindGroup>(),
            objects: segment.tess_batches.len() * 2,
        })?;
        // Build every batch's clip bind group BEFORE the vertex/index buffers.
        //
        // Two reasons it cannot happen inside the loop: `alloc` needs the
        // resources borrow that the vertex/index buffers below hold for the
        // rest of this function, and the pool hands out a buffer that must
        // stay distinct for the whole submit — every tessellated batch lands
        // in ONE submit, so a reused buffer would give them all the last clip
        // written.
        let clip_bind_groups: Vec<wgpu::BindGroup> = segment
            .tess_batches
            .iter()
            .map(|batch| {
                let uniform = crate::command_ir::ClipUniform::from_resolved(batch.clip);
                let buffer = resources
                    .uniform_pool_mut()
                    .alloc(bytemuck::bytes_of(&uniform));
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Tessellated Clip Bind Group"),
                    layout: pipelines.shape_cache_mut().clip_bind_group_layout(),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            })
            .collect();

        let (vertex_buffer, index_buffer) =
            resources.buffer_pool_mut().get_vertex_and_index_buffers(
                device,
                queue,
                "Shape Vertex Buffer",
                bytemuck::cast_slice(&segment.vertices),
                "Shape Index Buffer",
                bytemuck::cast_slice(&segment.indices),
            );

        Ok(Some(PreparedTess {
            vertex_buffer: vertex_buffer.clone(),
            index_buffer: index_buffer.clone(),
            clip_bind_groups,
        }))
    }

    #[expect(clippy::too_many_arguments)]
    fn flush_tessellated_geometry(
        &mut self,
        segment: &DrawSegment,
        range: std::ops::Range<usize>,
        prepared: Option<&PreparedTess>,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        pipelines: &mut PipelineSet,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let Some(prepared) = prepared else {
            return;
        };
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shape Render Pass"),
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

        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_vertex_buffer(0, prepared.vertex_buffer.slice(..));
        render_pass.set_index_buffer(prepared.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        let (full_w, full_h) = viewport_size;

        // Pin the viewport to (full_w, full_h) so that NDC [-1,+1] always maps to
        // exactly the requested viewport extent, regardless of the wgpu attachment
        // size.  Without this explicit set, wgpu's default viewport equals the
        // attachment dimensions — which diverges from full_w/full_h when rendering
        // into an oversized pool-bucket texture (e.g. SSAA bucket-aligned tiles).
        // Setting it explicitly is a no-op for the normal case where attachment ==
        // viewport, but is load-bearing for SSAA bucket rendering.
        render_pass.set_viewport(0.0, 0.0, full_w as f32, full_h as f32, 0.0, 1.0);

        let mut active_key: Option<PipelineKey> = None;
        for (batch, clip_bind_group) in segment.tess_batches[range.clone()]
            .iter()
            .zip(&prepared.clip_bind_groups[range])
        {
            if active_key != Some(batch.pipeline_key) {
                // `pipelines` and `device` are disjoint from the encoder/render_pass
                // borrows — no borrow conflict.
                let pipeline = pipelines
                    .shape_cache_mut()
                    .get_or_create(device, batch.pipeline_key);
                render_pass.set_pipeline(pipeline);
                active_key = Some(batch.pipeline_key);
            }

            if !set_clamped_scissor(&mut render_pass, batch.scissor, full_w, full_h) {
                // Fully clipped (a zero-area clamp, or the SSAA/opacity-layer
                // remap's off-target sentinel) — nothing to draw for this batch.
                continue;
            }

            render_pass.set_bind_group(1, clip_bind_group, &[]);

            let start = batch.index_start;
            let end = start + batch.index_count;
            render_pass.draw_indexed(start..end, 0, 0..1);
        }

        drop(render_pass);
    }

    // =========================================================================
    // Phase 4: segment-cached images
    // =========================================================================

    /// Flush all texture-cache image draws recorded in the segment.
    ///
    /// Groups consecutive draws by `TextureKey` to minimise draw calls.  When
    /// a texture-ID change forces an early flush, the previous batch is
    /// submitted before the new `TextureKey` takes over.
    fn flush_segment_cached_images(
        &mut self,
        segment: &DrawSegment,
        range: std::ops::Range<usize>,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let pending_images = &segment.cached_images[range];

        if pending_images.is_empty() {
            return;
        }

        let mut active_texture_id: Option<crate::texture_cache::TextureKey> = None;
        let mut active_texture_view: Option<wgpu::TextureView> = None;
        // Scissor of the most-recently buffered instance — forwarded when a
        // texture-change forces an early flush.
        let mut active_scissor: ScissorRect = None;

        for (texture_id, instance, scissor) in pending_images.iter().cloned() {
            if active_texture_id.as_ref() != Some(&texture_id) || active_scissor != scissor {
                if let Some(texture_view) = active_texture_view.as_ref() {
                    self.flush_texture_batch(
                        device,
                        queue,
                        pipelines,
                        resources,
                        viewport_size,
                        encoder,
                        view,
                        texture_view,
                        active_scissor,
                    );
                }
                active_texture_id = Some(texture_id.clone());
                active_texture_view = resources
                    .texture_cache_mut()
                    .get(&texture_id)
                    .map(|cached| cached.view.clone());
            }

            active_scissor = scissor;
            if let Some(texture_view) = active_texture_view.as_ref()
                && self.texture_batch.add(instance)
            {
                self.flush_texture_batch(
                    device,
                    queue,
                    pipelines,
                    resources,
                    viewport_size,
                    encoder,
                    view,
                    texture_view,
                    active_scissor,
                );
            }
        }

        if let Some(texture_view) = active_texture_view.as_ref() {
            self.flush_texture_batch(
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                view,
                texture_view,
                active_scissor,
            );
        }
    }

    // =========================================================================
    // Phase 5: external (registered) textures
    // =========================================================================

    /// Replay captured allocations in order. Bindings are cached per frame by
    /// allocation identity and effective sampling, using the active pipeline
    /// layout. Strong leases survive both recorder disposal and GPU completion.
    fn flush_segment_external_images(
        &mut self,
        segment: &DrawSegment,
        range: std::ops::Range<usize>,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> crate::error::EngineResult<()> {
        for (lease, instance, scissor, sampling) in &segment.external_images[range] {
            let epoch = resources.prepared_epoch();
            let key = (lease.allocation_key(), *sampling);
            if !self.external_bindings.contains_key(&key) {
                // Owner rejection and completion enrollment precede binding allocation.
                resources.retain_external_lease(lease.clone())?;
                let requested = std::mem::size_of::<(
                    super::CapturedExternalBinding,
                    (usize, crate::external_texture_registry::ExternalSampling),
                )>();
                let charge =
                    resources.reserve_external_binding(crate::device_domain::PreparedCost {
                        gpu_bytes: 0,
                        cpu_bytes: requested,
                        objects: 1,
                    })?;
                self.external_bindings.try_reserve(1).map_err(|source| {
                    crate::error::EngineError::PreparedResourceAllocation {
                        resource: "external binding cache",
                        source,
                    }
                })?;
                let sampler = match sampling {
                    crate::external_texture_registry::ExternalSampling::Nearest => {
                        &self.nearest_sampler
                    }
                    crate::external_texture_registry::ExternalSampling::Linear => {
                        &self.external_linear_sampler
                    }
                };
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Captured External Texture Binding"),
                    layout: &pipelines.texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(lease.view()),
                        },
                    ],
                });
                self.external_bindings.insert(
                    key,
                    super::CapturedExternalBinding {
                        _lease: lease.clone(),
                        binding,
                        charge,
                        last_enrolled_epoch: epoch,
                    },
                );
            }
            let captured = self
                .external_bindings
                .get_mut(&key)
                .expect("BUG: external binding was inserted");
            // Every pending submission must own both references; repeated draws
            // in that same batch need no extra permit allocation or ledger lock.
            // Exhausted epochs disable the optimization instead of wrapping.
            if epoch.is_none() || captured.last_enrolled_epoch != epoch {
                resources.retain_external_lease(lease.clone())?;
                resources.retain_external_binding_charge(&captured.charge)?;
                captured.last_enrolled_epoch = epoch;
            }
            let binding = captured.binding.clone();
            let _ = self.texture_batch.add(*instance);
            self.flush_texture_batch_with_blend(
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                view,
                lease.view(),
                *scissor,
                matches!(
                    lease.descriptor().alpha,
                    crate::external_texture_registry::ExternalAlpha::Premultiplied
                ) || instance.filters_straight_as_premultiplied(),
                Some(&binding),
            );
        }
        Ok(())
    }

    // =========================================================================
    // Texture-batch flush methods (shared plumbing now lives on self)
    // =========================================================================

    /// Flush the texture instance batch with straight-alpha blending.
    ///
    /// Used for normal decoded-image draws whose samples carry straight
    /// (non-premultiplied) alpha.  Offscreen layer composites must use
    /// `flush_texture_batch_premultiplied` instead.
    ///
    /// `scissor` is the clip rect to apply.  Pass `None` for full-viewport
    /// (unclipped), matching the rect/circle instanced batches.
    pub(crate) fn flush_texture_batch(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        self.flush_texture_batch_with_blend(
            device,
            queue,
            pipelines,
            resources,
            viewport_size,
            encoder,
            view,
            texture_view,
            scissor,
            false,
            None,
        );
    }

    /// Flush the texture instance batch using **premultiplied** source-over
    /// blending.
    ///
    /// Used to composite offscreen layer textures (opacity / ColorFilter /
    /// ShaderMask / backdrop results) whose texels are premultiplied
    /// (`rgb = straight_rgb * a`).  Compositing with the straight pipeline
    /// would re-multiply rgb by alpha, darkening translucent/AA content.
    /// Routes through `PipelineSet::instanced_texture_premul` (src factor
    /// `One`); the per-channel tint carries group opacity and any ColorFilter
    /// chroma as `(C.r*O, C.g*O, C.b*O, O)`.
    pub(crate) fn flush_texture_batch_premultiplied(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        self.flush_texture_batch_with_blend(
            device,
            queue,
            pipelines,
            resources,
            viewport_size,
            encoder,
            view,
            texture_view,
            scissor,
            true,
            None,
        );
    }

    /// Shared body for the two public texture-batch flush methods.
    ///
    /// `premultiplied` selects the blend pipeline: `false` = straight-alpha
    /// (decoded images), `true` = premultiplied source-over (offscreen-layer
    /// composites).
    fn flush_texture_batch_with_blend(
        &mut self,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
        premultiplied: bool,
        captured_binding: Option<&wgpu::BindGroup>,
    ) {
        if self.texture_batch.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuReplay::flush_texture_batch: {} instances",
            self.texture_batch.len()
        );

        let texture_bind_group = captured_binding.cloned().unwrap_or_else(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Texture Instance Bind Group"),
                layout: &pipelines.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                ],
            })
        });

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Texture Instance Buffer",
            self.texture_batch.as_bytes(),
        );

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Instanced Texture Render Pass"),
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

        // Offscreen-layer composites use the premultiplied pipeline; normal
        // decoded-image draws use straight alpha.  Selection logic is
        // behavior-preserving (round-5c color-correctness fix).
        let pipeline = if premultiplied {
            &pipelines.instanced_texture_premul
        } else {
            &pipelines.instanced_texture
        };
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_bind_group(1, &texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_vertex_buffer(1, instance_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;
        if set_clamped_scissor(&mut render_pass, scissor, full_w, full_h) {
            render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);
        }
        drop(render_pass);
        self.texture_batch.clear();
    }

    /// Flush the texture instance batch with the **exact blend mode** specified.
    ///
    /// Unlike [`Self::flush_texture_batch_premultiplied`] (which always uses the
    /// SrcOver premultiplied pipeline), this method uses
    /// [`PipelineSet::ensure_texture_composite`] /
    /// [`PipelineSet::texture_composite_for`] to obtain a pipeline whose
    /// `wgpu::BlendState` matches `mode` exactly.
    ///
    /// SSAA tile and group composites share this path. Every source is
    /// premultiplied, so `src_factor = One` is correct.
    ///
    /// Takes `pipelines: &mut PipelineSet` because lazy pipeline creation may
    /// be needed on the first call for a given mode.
    pub(crate) fn flush_texture_batch_premultiplied_with_mode(
        &mut self,
        mode: flui_painting::paint::BlendMode,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        viewport_size: (u32, u32),
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        texture_view: &wgpu::TextureView,
        scissor: ScissorRect,
    ) {
        if self.texture_batch.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            ?mode,
            "GpuReplay::flush_texture_batch_premultiplied_with_mode: {} instances",
            self.texture_batch.len()
        );

        // Ensure the per-mode pipeline is in the cache. The `&mut` borrow of
        // `pipelines` ends at the semicolon; subsequent accesses are `&`.
        pipelines.ensure_texture_composite(device, mode);

        // Both of these are now `&pipelines` (shared) borrows — no conflict.
        let pipeline = pipelines.texture_composite_for(mode);
        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SSAA Tile Composite Bind Group"),
            layout: &pipelines.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&self.default_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
            ],
        });

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "SSAA Tile Composite Instance Buffer",
            self.texture_batch.as_bytes(),
        );

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("SSAA Tile Composite Render Pass"),
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

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        render_pass.set_bind_group(1, &texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_vertex_buffer(1, instance_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;
        if set_clamped_scissor(&mut render_pass, scissor, full_w, full_h) {
            render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);
        }
        drop(render_pass);
        self.texture_batch.clear();
    }
}

#[path = "ordered.rs"]
mod ordered;

#[path = "coverage.rs"]
mod coverage;
