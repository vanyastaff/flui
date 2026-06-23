//! `GpuReplay` segment-flush machinery, split out of `replay.rs` for the C1 cap.
//!
//! Holds `flush_segment` (the canonical five-phase entry point) and every
//! per-bucket flush helper it drives (instanced batches, gradients, tessellated
//! geometry, cached/external images, and the texture-batch blend variants). The
//! dispatch core (`new` / `update_viewport` / `submit` /
//! `reintegrate_offscreen_content`) stays in the parent `replay` module.
//!
//! These are inherent `impl GpuReplay` methods on a descendant module of
//! `replay`, so they retain access to `GpuReplay`'s private fields.

use std::sync::Arc;

use super::super::{
    command_ir::{DrawSegment, ScissorRect},
    pipeline::PipelineKey,
    pipelines::PipelineSet,
    resources::GpuResources,
};
use super::GpuReplay;

// GPU rendering routinely converts between numeric types for pixel coordinates,
// color channels, buffer indices, and instance counts; flush methods also carry
// many GPU-handle parameters.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl GpuReplay {
    // =========================================================================
    // Segment-flush entry point
    // =========================================================================

    /// Flush a single `DrawSegment` to the GPU in the canonical five-phase order.
    ///
    /// ## R1 — flush order invariant
    ///
    /// The five phases are executed in this exact sequence.  This order is
    /// **load-bearing** for z-ordering correctness — a reorder silently corrupts
    /// draw results with no compile error:
    ///
    /// 1. `flush_all_instanced_batches`   — rect / circle / arc / shadow
    /// 2. `flush_gradient_batches`        — linear / radial / sweep
    /// 3. `flush_tessellated_geometry`    — lyon tessellated paths / vertices
    /// 4. `flush_segment_cached_images`   — texture-cache images
    /// 5. `flush_segment_external_images` — external (registered) textures
    pub(in crate::wgpu) fn flush_segment(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        // R1: five-phase order is load-bearing — do not reorder.
        self.flush_all_instanced_batches(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_gradient_batches(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_tessellated_geometry(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_segment_cached_images(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
        self.flush_segment_external_images(
            segment,
            viewport_size,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            view,
        );
    }

    // =========================================================================
    // Phase 1: instanced batches (rect / circle / arc / shadow)
    // =========================================================================

    /// Flush all instanced batches in a single render pass (Phase 9 optimisation).
    ///
    /// Combines shadow → rect → circle → arc into one combined instance buffer
    /// and one render pass, switching pipelines dynamically.  The shadow
    /// instances are prepended first for correct z-ordering (background →
    /// foreground).
    ///
    /// Before (Phase 8): 1 buffer upload + 3 render passes + 3 draw calls.
    /// After  (Phase 9): 1 buffer upload + 1 render pass  + 3 draw calls.
    fn flush_all_instanced_batches(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        use super::super::multi_draw::{MultiDrawBatcher, PipelineId};

        let has_rects = !segment.rect_batch.is_empty();
        let has_circles = !segment.circle_batch.is_empty();
        let has_arcs = !segment.arc_batch.is_empty();
        let has_shadows = !segment.shadow_batch.is_empty();

        if !has_rects && !has_circles && !has_arcs && !has_shadows {
            return;
        }

        let rect_size = segment.rect_batch.len()
            * std::mem::size_of::<super::super::instancing::RectInstance>();
        let circle_size = segment.circle_batch.len()
            * std::mem::size_of::<super::super::instancing::CircleInstance>();
        let arc_size =
            segment.arc_batch.len() * std::mem::size_of::<super::super::instancing::ArcInstance>();
        let shadow_size = segment.shadow_batch.len()
            * std::mem::size_of::<super::super::instancing::ShadowInstance>();

        // IMPORTANT: Shadows FIRST for correct z-ordering (background → foreground).
        let mut combined_buffer =
            Vec::with_capacity(shadow_size + rect_size + circle_size + arc_size);
        let mut multi_batcher = MultiDrawBatcher::new();

        let shadow_offset = combined_buffer.len() as u64;
        if has_shadows {
            combined_buffer.extend_from_slice(segment.shadow_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Rectangle, // shadow pipeline rendered first for z-order
                segment.shadow_batch.len() as u32,
                shadow_offset,
                shadow_size as u64,
            );
        }

        let rect_offset = combined_buffer.len() as u64;
        if has_rects {
            combined_buffer.extend_from_slice(segment.rect_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Rectangle,
                segment.rect_batch.len() as u32,
                rect_offset,
                rect_size as u64,
            );
        }

        let circle_offset = combined_buffer.len() as u64;
        if has_circles {
            combined_buffer.extend_from_slice(segment.circle_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Circle,
                segment.circle_batch.len() as u32,
                circle_offset,
                circle_size as u64,
            );
        }

        let arc_offset = combined_buffer.len() as u64;
        if has_arcs {
            combined_buffer.extend_from_slice(segment.arc_batch.as_bytes());
            multi_batcher.add_quad_draw(
                PipelineId::Arc,
                segment.arc_batch.len() as u32,
                arc_offset,
                arc_size as u64,
            );
        }

        #[cfg(debug_assertions)]
        {
            let stats = multi_batcher.stats();
            tracing::trace!(
                "GpuReplay::flush_all_instanced_batches: draws={}, instances={}, buffer={}B",
                stats.active_draws,
                stats.active_instances,
                combined_buffer.len()
            );
        }

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Combined Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL PRIMITIVES =====
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Combined Instanced Primitives Pass"),
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
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;

        // --- Shadows (rendered first for correct z-ordering) ---
        if has_shadows {
            render_pass.set_pipeline(&pipelines.shadow);
            let buf_start = shadow_offset;
            let buf_end = buf_start + shadow_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
            render_pass.draw_indexed(0..6, 0, 0..segment.shadow_batch.len() as u32);
        }

        // --- Rectangles (per-scissor-region) ---
        if has_rects {
            render_pass.set_pipeline(&pipelines.instanced_rect);
            let buf_start = rect_offset;
            let buf_end = buf_start + rect_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.rect_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Circles (per-scissor-region) ---
        if has_circles {
            render_pass.set_pipeline(&pipelines.instanced_circle);
            let buf_start = circle_offset;
            let buf_end = buf_start + circle_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.circle_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // --- Arcs (per-scissor-region) ---
        if has_arcs {
            render_pass.set_pipeline(&pipelines.instanced_arc);
            let buf_start = arc_offset;
            let buf_end = buf_start + arc_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(buf_start..buf_end));

            for region in &segment.arc_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        drop(render_pass);

        // Clear batches for next frame.
        segment.rect_batch.clear();
        segment.circle_batch.clear();
        segment.arc_batch.clear();
        segment.shadow_batch.clear();
        segment.rect_scissors.clear();
        segment.circle_scissors.clear();
        segment.arc_scissors.clear();
    }

    // =========================================================================
    // Phase 2: gradient batches (linear / radial / sweep)
    // =========================================================================

    /// Flush all gradient batches (linear, radial, sweep) in a single render pass.
    ///
    /// Uploads gradient stops and the combined instance buffer, then renders
    /// all three gradient types in one render pass with pipeline switches.
    fn flush_gradient_batches(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let has_linear = !segment.linear_gradient_batch.is_empty();
        let has_radial = !segment.radial_gradient_batch.is_empty();
        let has_sweep = !segment.sweep_gradient_batch.is_empty();

        if !has_linear && !has_radial && !has_sweep {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuReplay::flush_gradient_batches: linear={}, radial={}, sweep={}, stops={}",
            segment.linear_gradient_batch.len(),
            segment.radial_gradient_batch.len(),
            segment.sweep_gradient_batch.len(),
            segment.current_gradient_stops.len()
        );

        // ===== Upload gradient stops to GPU =====
        if !segment.current_gradient_stops.is_empty() {
            pipelines.refresh_gradient_bind_group(
                device,
                queue,
                bytemuck::cast_slice(&segment.current_gradient_stops),
            );
        }

        let linear_size = segment.linear_gradient_batch.len()
            * std::mem::size_of::<super::super::instancing::LinearGradientInstance>();
        let radial_size = segment.radial_gradient_batch.len()
            * std::mem::size_of::<super::super::instancing::RadialGradientInstance>();
        let sweep_size = segment.sweep_gradient_batch.len()
            * std::mem::size_of::<super::super::instancing::SweepGradientInstance>();

        let mut combined_buffer = Vec::with_capacity(linear_size + radial_size + sweep_size);

        let linear_offset = 0;
        if has_linear {
            combined_buffer.extend_from_slice(segment.linear_gradient_batch.as_bytes());
        }

        let radial_offset = combined_buffer.len();
        if has_radial {
            combined_buffer.extend_from_slice(segment.radial_gradient_batch.as_bytes());
        }

        let sweep_offset = combined_buffer.len();
        if has_sweep {
            combined_buffer.extend_from_slice(segment.sweep_gradient_batch.as_bytes());
        }

        let instance_buffer = resources.buffer_pool_mut().get_vertex_buffer(
            device,
            queue,
            "Gradient Instance Buffer",
            &combined_buffer,
        );

        // ===== SINGLE RENDER PASS FOR ALL GRADIENTS =====
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Gradient Render Pass"),
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
        if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
            render_pass.set_bind_group(1, gradient_bind_group, &[]);
        }
        render_pass.set_vertex_buffer(0, self.unit_quad_buffer.slice(..));
        render_pass.set_index_buffer(
            self.unit_quad_index_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );

        let (full_w, full_h) = viewport_size;

        // ===== Draw Linear Gradients (per-scissor-region) =====
        if has_linear {
            render_pass.set_pipeline(&pipelines.linear_gradient);
            // Re-set bind groups after pipeline switch (WebGPU invalidates bind
            // groups when the new pipeline's PipelineLayout is a different object).
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let linear_start = linear_offset as u64;
            let linear_end = linear_start + linear_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(linear_start..linear_end));

            for region in &segment.linear_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Radial Gradients (per-scissor-region) =====
        if has_radial {
            render_pass.set_pipeline(&pipelines.radial_gradient);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let radial_start = radial_offset as u64;
            let radial_end = radial_start + radial_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(radial_start..radial_end));

            for region in &segment.radial_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        // ===== Draw Sweep Gradients (per-scissor-region) =====
        if has_sweep {
            render_pass.set_pipeline(&pipelines.sweep_gradient);
            render_pass.set_bind_group(0, &self.viewport_bind_group, &[]);
            if let Some(ref gradient_bind_group) = pipelines.gradient_bind_group {
                render_pass.set_bind_group(1, gradient_bind_group, &[]);
            }

            let sweep_start = sweep_offset as u64;
            let sweep_end = sweep_start + sweep_size as u64;
            render_pass.set_vertex_buffer(1, instance_buffer.slice(sweep_start..sweep_end));

            for region in &segment.sweep_grad_scissors {
                if let Some((x, y, w, h)) = region.scissor {
                    render_pass.set_scissor_rect(x, y, w, h);
                } else {
                    render_pass.set_scissor_rect(0, 0, full_w, full_h);
                }
                render_pass.draw_indexed(0..6, 0, region.start..region.start + region.count);
            }
        }

        drop(render_pass);

        // Clear batches for next frame.
        segment.linear_gradient_batch.clear();
        segment.radial_gradient_batch.clear();
        segment.sweep_gradient_batch.clear();
        segment.current_gradient_stops.clear();
        segment.linear_grad_scissors.clear();
        segment.radial_grad_scissors.clear();
        segment.sweep_grad_scissors.clear();
    }

    // =========================================================================
    // Phase 3: tessellated geometry
    // =========================================================================

    /// Flush tessellated geometry from the segment.
    ///
    /// Uploads vertices/indices and renders all recorded tessellated batches in
    /// a single render pass, switching pipelines as needed.
    fn flush_tessellated_geometry(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if segment.vertices.is_empty() || segment.tess_batches.is_empty() {
            return;
        }

        let (vertex_buffer, index_buffer) =
            resources.buffer_pool_mut().get_vertex_and_index_buffers(
                device,
                queue,
                "Shape Vertex Buffer",
                bytemuck::cast_slice(&segment.vertices),
                "Shape Index Buffer",
                bytemuck::cast_slice(&segment.indices),
            );

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
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        let (full_w, full_h) = viewport_size;

        // Pin the viewport to (full_w, full_h) so that NDC [-1,+1] always maps to
        // exactly the requested viewport extent, regardless of the wgpu attachment
        // size.  Without this explicit set, wgpu's default viewport equals the
        // attachment dimensions — which diverges from full_w/full_h when rendering
        // into an oversized pool-bucket texture (e.g. SSAA bucket-aligned tiles).
        // Setting it explicitly is a no-op for the normal case where attachment ==
        // viewport, but is load-bearing for SSAA bucket rendering.
        #[allow(
            clippy::cast_precision_loss,
            reason = "full_w/full_h are small surface dimensions; f32 is sufficient"
        )]
        render_pass.set_viewport(0.0, 0.0, full_w as f32, full_h as f32, 0.0, 1.0);

        let mut active_key: Option<PipelineKey> = None;
        for batch in &segment.tess_batches {
            if active_key != Some(batch.pipeline_key) {
                // `pipelines` and `device` are disjoint from the encoder/render_pass
                // borrows — no borrow conflict.
                let pipeline = pipelines
                    .shape_cache_mut()
                    .get_or_create(device, batch.pipeline_key);
                render_pass.set_pipeline(pipeline);
                active_key = Some(batch.pipeline_key);
            }

            if let Some((x, y, w, h)) = batch.scissor {
                render_pass.set_scissor_rect(x, y, w, h);
            } else {
                render_pass.set_scissor_rect(0, 0, full_w, full_h);
            }

            let start = batch.index_start;
            let end = start + batch.index_count;
            render_pass.draw_indexed(start..end, 0, 0..1);
        }

        drop(render_pass);

        segment.vertices.clear();
        segment.indices.clear();
        segment.tess_batches.clear();
        segment.current_pipeline_key = None;
    }

    // =========================================================================
    // Phase 4: segment-cached images
    // =========================================================================

    /// Flush all texture-cache image draws recorded in the segment.
    ///
    /// Groups consecutive draws by `TextureId` to minimise draw calls.  When
    /// a texture-ID change forces an early flush, the previous batch is
    /// submitted before the new `TextureId` takes over.
    fn flush_segment_cached_images(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let mut pending_images: Vec<(
            super::super::texture_cache::TextureId,
            super::super::instancing::TextureInstance,
            ScissorRect,
        )> = segment.cached_images.drain(..).collect();

        if pending_images.is_empty() {
            return;
        }

        let mut active_texture_id: Option<super::super::texture_cache::TextureId> = None;
        let mut active_texture_view: Option<wgpu::TextureView> = None;
        // Scissor of the most-recently buffered instance — forwarded when a
        // texture-change forces an early flush.
        let mut active_scissor: ScissorRect = None;

        for (texture_id, instance, scissor) in pending_images.drain(..) {
            if active_texture_id.as_ref() != Some(&texture_id) {
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

    /// Flush all external-texture draws recorded in the segment.
    ///
    /// Each entry carries a `flui_types::painting::TextureId` stored at
    /// record time.  Here, at replay time, each ID is resolved to a
    /// `wgpu::TextureView` via the external texture registry.  If an ID is not
    /// found (texture was unregistered between record and flush), a warning is
    /// emitted and the entry is skipped — identical behavior to before, now on
    /// the correct replay side of the record/replay seam.
    ///
    /// Because `wgpu::TextureView` is not `PartialEq`, instances are flushed
    /// individually (one draw call per instance) rather than grouped by view
    /// equality.  External textures are uncommon in typical UI; the extra draw
    /// calls are not a hot path.
    fn flush_segment_external_images(
        &mut self,
        segment: &mut DrawSegment,
        viewport_size: (u32, u32),
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        if segment.external_images.is_empty() {
            return;
        }

        // Drain into a local vec so we can call `&mut self` methods
        // (`flush_texture_batch`) while iterating without holding a borrow on
        // `segment.external_images`.
        let pending: Vec<(
            flui_types::painting::TextureId,
            super::super::instancing::TextureInstance,
            ScissorRect,
        )> = segment.external_images.drain(..).collect();

        for (texture_id, instance, scissor) in pending {
            // Resolve ID → view at replay time.  Clone the view to release the
            // borrow on `resources` before calling `flush_texture_batch` (which
            // takes `&mut resources`).  External textures are uncommon; the
            // clone is not a hot-path concern.
            let tex_view =
                if let Some(entry) = resources.external_texture_registry().get(texture_id) {
                    entry.view.clone()
                } else {
                    tracing::warn!(
                        "External texture {} not found at flush time — skipping draw",
                        texture_id.get()
                    );
                    continue;
                };

            let _ = self.texture_batch.add(instance);
            self.flush_texture_batch(
                device,
                queue,
                pipelines,
                resources,
                viewport_size,
                encoder,
                view,
                &tex_view,
                scissor,
            );
        }
    }

    // =========================================================================
    // Texture-batch flush methods (from T10b, updated: 5 params → self fields)
    // =========================================================================

    /// Flush the texture instance batch with straight-alpha blending.
    ///
    /// Used for normal decoded-image draws whose samples carry straight
    /// (non-premultiplied) alpha.  Offscreen layer composites must use
    /// `flush_texture_batch_premultiplied` instead.
    ///
    /// `scissor` is the clip rect to apply.  Pass `None` for full-viewport
    /// (unclipped), matching the rect/circle instanced batches.
    pub(in crate::wgpu) fn flush_texture_batch(
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
    pub(in crate::wgpu) fn flush_texture_batch_premultiplied(
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
    ) {
        if self.texture_batch.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "GpuReplay::flush_texture_batch: {} instances",
            self.texture_batch.len()
        );

        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        if let Some((x, y, w, h)) = scissor {
            render_pass.set_scissor_rect(x, y, w, h);
        } else {
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
        }

        render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);
        drop(render_pass);
        self.texture_batch.clear();
    }

    /// Flush the texture instance batch with the **exact blend mode** specified.
    ///
    /// Unlike [`Self::flush_texture_batch_premultiplied`] (which always uses the
    /// SrcOver premultiplied pipeline), this method uses
    /// [`PipelineSet::ensure_ssaa_tile_composite`] /
    /// [`PipelineSet::ssaa_tile_composite_for`] to obtain a pipeline whose
    /// `wgpu::BlendState` matches `mode` exactly.
    ///
    /// Used by [`Self::render_ssaa_path`] to composite the
    /// SSAA 1× tile with the correct blend mode. The source texel is
    /// premultiplied (box-downsample output), so `src_factor = One` is correct
    /// for all tile-safe variants.
    ///
    /// Takes `pipelines: &mut PipelineSet` because lazy pipeline creation may
    /// be needed on the first call for a given mode.
    pub(in crate::wgpu) fn flush_texture_batch_premultiplied_with_mode(
        &mut self,
        mode: flui_types::painting::BlendMode,
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
        pipelines.ensure_ssaa_tile_composite(device, mode);

        // Both of these are now `&pipelines` (shared) borrows — no conflict.
        let pipeline = pipelines.ssaa_tile_composite_for(mode);
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
        if let Some((x, y, w, h)) = scissor {
            render_pass.set_scissor_rect(x, y, w, h);
        } else {
            render_pass.set_scissor_rect(0, 0, full_w, full_h);
        }

        render_pass.draw_indexed(0..6, 0, 0..self.texture_batch.len() as u32);
        drop(render_pass);
        self.texture_batch.clear();
    }
}
