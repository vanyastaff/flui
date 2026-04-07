//! Command encoder wrapper for building GPU command buffers.
//!
//! [`FrameEncoder`] ties together the render surface, GPU device, batchers,
//! and state stacks into a single per-frame recording context. It is created
//! by [`RenderSurface::begin_frame`](crate::context::render_surface::RenderSurface::begin_frame)
//! and consumed by [`finish`](FrameEncoder::finish) which submits all recorded
//! GPU work and presents the frame.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::context::gpu_device::GpuDevice;
use crate::context::render_surface::RenderSurface;
use crate::error::RenderResult;
use crate::frame::dispatch::{traverse_scene, Batchers};
use crate::frame::state_stack::StateStack;
use crate::frame::submission::DrawOp;
use crate::pipelines::registry::PipelineId;
use crate::vertex::FrameUniforms;
use flui_layer::Scene;

// ---------------------------------------------------------------------------
// GPU-side gradient uniform structs
// ---------------------------------------------------------------------------

/// GPU uniform data for a linear gradient (matches `GradientUniforms` in
/// `linear.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct LinearGradientUniforms {
    /// Bounding rectangle (x, y, w, h).
    bounds: [f32; 4],
    /// Start and end points packed as (start.x, start.y, end.x, end.y).
    start_end: [f32; 4],
    /// Per-corner radii [tl, tr, br, bl].
    corner_radii: [f32; 4],
    /// Number of color stops.
    stop_count: u32,
    /// Padding to align to 16 bytes.
    _padding: [u32; 3],
}

/// GPU uniform data for a radial gradient (matches `GradientUniforms` in
/// `radial.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RadialGradientUniforms {
    /// Bounding rectangle (x, y, w, h).
    bounds: [f32; 4],
    /// Center and radius packed as (center.x, center.y, radius, 0.0).
    center_radius: [f32; 4],
    /// Per-corner radii [tl, tr, br, bl].
    corner_radii: [f32; 4],
    /// Number of color stops.
    stop_count: u32,
    /// Padding to align to 16 bytes.
    _padding: [u32; 3],
}

/// GPU uniform data for a sweep gradient (matches `GradientUniforms` in
/// `sweep.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SweepGradientUniforms {
    /// Bounding rectangle (x, y, w, h).
    bounds: [f32; 4],
    /// Center and angles packed as (center.x, center.y, start_angle, end_angle).
    center_angles: [f32; 4],
    /// Per-corner radii [tl, tr, br, bl].
    corner_radii: [f32; 4],
    /// Number of color stops.
    stop_count: u32,
    /// Padding to align to 16 bytes.
    _padding: [u32; 3],
}

/// Per-frame command encoder. Created by
/// [`RenderSurface::begin_frame`](crate::context::render_surface::RenderSurface::begin_frame).
///
/// Records draw commands via [`render_scene`](Self::render_scene), then submits
/// to the GPU and presents via [`finish`](Self::finish).
pub struct FrameEncoder<'surface> {
    surface: &'surface RenderSurface,
    gpu: Arc<GpuDevice>,
    surface_texture: wgpu::SurfaceTexture,
    surface_view: wgpu::TextureView,
    batchers: Batchers,
    state: StateStack,
    scale_factor: f32,
    /// Ordered draw operations recorded during scene traversal.
    /// These preserve painter's order across layer boundaries.
    draw_ops: Vec<DrawOp>,
}

impl<'surface> FrameEncoder<'surface> {
    /// Create a new frame encoder. Called by `RenderSurface::begin_frame`.
    pub(crate) fn new(
        surface: &'surface RenderSurface,
        surface_texture: wgpu::SurfaceTexture,
    ) -> Self {
        let gpu = Arc::clone(surface.gpu());
        let scale_factor = surface.scale_factor();
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            surface,
            gpu,
            surface_texture,
            surface_view,
            batchers: Batchers::new(),
            state: StateStack::new(),
            scale_factor,
            draw_ops: Vec::new(),
        }
    }

    /// Traverse a [`Scene`]'s layer tree and record all draw commands into
    /// internal batchers.
    pub fn render_scene(&mut self, scene: &Scene) -> RenderResult<()> {
        let _span = tracing::debug_span!("render_scene").entered();
        let ops = traverse_scene(
            scene,
            &mut self.batchers,
            &mut self.state,
            self.scale_factor,
        );
        self.draw_ops.extend(ops);
        Ok(())
    }

    /// Mutable access to the batchers for direct draw command recording.
    ///
    /// Useful for demos and tests that bypass the scene/layer pipeline
    /// and push primitives directly.
    pub fn batchers_mut(&mut self) -> &mut Batchers {
        &mut self.batchers
    }

    /// Submit all recorded GPU work and present the frame.
    ///
    /// Uploads batcher data to GPU buffers, executes draw calls for each
    /// pipeline (shapes, paths, shadows, text), submits the command buffer,
    /// and presents the frame.
    ///
    /// On error the frame is dropped -- the caller should call
    /// [`RenderSurface::resize`](crate::context::render_surface::RenderSurface::resize)
    /// and retry on the next frame.
    pub fn finish(self) -> RenderResult<()> {
        let _span = tracing::debug_span!("finish_frame").entered();

        // -- Update viewport uniform ----------------------------------------
        let uniforms = FrameUniforms::new(
            self.surface.width() as f32,
            self.surface.height() as f32,
            self.scale_factor,
        );
        self.gpu.queue().write_buffer(
            self.surface.viewport_buffer(),
            0,
            bytemuck::bytes_of(&uniforms),
        );

        // -- Reset buffer pool for this frame --------------------------------
        self.gpu.buffer_pool().lock().reset();

        // -- Upload shape instances to GPU buffers (from pool) -------------
        let rect_buf = if self.batchers.shapes.rect_count() > 0 {
            let data = bytemuck::cast_slice(self.batchers.shapes.rects());
            let buf = {
                let mut pool = self.gpu.buffer_pool().lock();
                pool.get_vertex_buffer(self.gpu.device(), data.len() as u64)
                    .clone()
            };
            self.gpu.queue().write_buffer(&buf, 0, data);
            Some(buf)
        } else {
            None
        };

        let circle_buf = if self.batchers.shapes.circle_count() > 0 {
            let data = bytemuck::cast_slice(self.batchers.shapes.circles());
            let buf = {
                let mut pool = self.gpu.buffer_pool().lock();
                pool.get_vertex_buffer(self.gpu.device(), data.len() as u64)
                    .clone()
            };
            self.gpu.queue().write_buffer(&buf, 0, data);
            Some(buf)
        } else {
            None
        };

        let arc_buf = if self.batchers.shapes.arc_count() > 0 {
            let data = bytemuck::cast_slice(self.batchers.shapes.arcs());
            let buf = {
                let mut pool = self.gpu.buffer_pool().lock();
                pool.get_vertex_buffer(self.gpu.device(), data.len() as u64)
                    .clone()
            };
            self.gpu.queue().write_buffer(&buf, 0, data);
            Some(buf)
        } else {
            None
        };

        let shadow_buf = if self.batchers.effects.shadow_count() > 0 {
            let data = bytemuck::cast_slice(self.batchers.effects.shadows());
            let buf = {
                let mut pool = self.gpu.buffer_pool().lock();
                pool.get_vertex_buffer(self.gpu.device(), data.len() as u64)
                    .clone()
            };
            self.gpu.queue().write_buffer(&buf, 0, data);
            Some(buf)
        } else {
            None
        };

        // -- Upload image instances to GPU buffers (from pool) ---------------
        let image_instance_stride = std::mem::size_of::<crate::vertex::ImageQuadInstance>() as u64;
        let image_buf = if self.batchers.images.total_instance_count() > 0 {
            // Flatten all image groups into a single contiguous buffer.
            // We track the byte offset per group so we can issue per-texture draws.
            let total_count = self.batchers.images.total_instance_count();
            let total_bytes = total_count as u64 * image_instance_stride;
            let buf = {
                let mut pool = self.gpu.buffer_pool().lock();
                pool.get_vertex_buffer(self.gpu.device(), total_bytes)
                    .clone()
            };
            let mut offset: u64 = 0;
            for (_texture_id, instances) in self.batchers.images.groups_in_order() {
                let data = bytemuck::cast_slice(instances);
                self.gpu.queue().write_buffer(&buf, offset, data);
                offset += instances.len() as u64 * image_instance_stride;
            }
            Some(buf)
        } else {
            None
        };

        // -- Upload path geometry (from pool) -------------------------------
        let (path_verts_buf, path_idxs_buf) = if self.batchers.paths.draw_range_count() > 0 {
            let vert_data = bytemuck::cast_slice(self.batchers.paths.vertices());
            let idx_data = bytemuck::cast_slice(self.batchers.paths.indices());
            let (verts, idxs) = {
                let mut pool = self.gpu.buffer_pool().lock();
                let v = pool
                    .get_vertex_buffer(self.gpu.device(), vert_data.len() as u64)
                    .clone();
                let i = pool
                    .get_index_buffer(self.gpu.device(), idx_data.len() as u64)
                    .clone();
                (v, i)
            };
            self.gpu.queue().write_buffer(&verts, 0, vert_data);
            self.gpu.queue().write_buffer(&idxs, 0, idx_data);
            (Some(verts), Some(idxs))
        } else {
            (None, None)
        };

        // -- Prepare text ---------------------------------------------------
        // Acquire the text system lock early; the guard must outlive the
        // render pass because `TextSystem::render` borrows into it.
        let has_text = self.batchers.text.run_count() > 0;
        let mut text_sys_guard = self.gpu.text_system().lock();
        if has_text {
            text_sys_guard.prepare(
                self.gpu.device(),
                self.gpu.queue(),
                self.batchers.text.runs(),
                self.surface.width(),
                self.surface.height(),
                self.scale_factor,
            );
        }

        // -- Create command encoder and render pass -------------------------
        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("flui_frame_encoder"),
                });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flui_main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.surface.stencil_view(),
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Set viewport
            render_pass.set_viewport(
                0.0,
                0.0,
                self.surface.width() as f32,
                self.surface.height() as f32,
                0.0,
                1.0,
            );

            let _span = tracing::debug_span!("submit_draws").entered();

            // Bind viewport uniform (shared by all pipelines)
            render_pass.set_bind_group(0, self.surface.viewport_bind_group(), &[]);

            // Set shared unit quad for instanced draws
            render_pass.set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
            render_pass.set_index_buffer(
                self.gpu.unit_quad_ibo().slice(..),
                wgpu::IndexFormat::Uint16,
            );

            // Cache instance strides for buffer offset computation.
            let rect_stride = std::mem::size_of::<crate::vertex::RectInstance>() as u64;
            let circle_stride = std::mem::size_of::<crate::vertex::CircleInstance>() as u64;
            let arc_stride = std::mem::size_of::<crate::vertex::ArcInstance>() as u64;
            let shadow_stride =
                std::mem::size_of::<crate::batchers::effects::ShadowInstance>() as u64;

            // If no draw_ops were recorded (e.g. direct batcher usage via
            // batchers_mut()), fall back to a single implicit group covering
            // everything.
            let fallback_ops;
            let ops: &[DrawOp] = if self.draw_ops.is_empty() && !self.batchers.is_all_empty() {
                use crate::frame::submission::BatcherSnapshot;
                fallback_ops = vec![DrawOp::DrawGroup {
                    before: BatcherSnapshot::default(),
                    after: self.batchers.snapshot(),
                }];
                &fallback_ops
            } else {
                &self.draw_ops
            };

            let surface_w = self.surface.width();
            let surface_h = self.surface.height();

            // Stencil reference value, incremented/decremented by stencil
            // push/pop operations for non-rectangular clipping.
            let mut stencil_ref: u32 = 0;

            // Iterate draw operations in painter's order.
            for op in ops {
                match op {
                    DrawOp::DrawGroup { before, after } => {
                        // === Rects (range: before.rects..after.rects) ===
                        let rect_count = after.rects - before.rects;
                        if rect_count > 0 {
                            if let Some(ref buf) = rect_buf {
                                if let Some(pipeline) =
                                    self.gpu.pipelines().get(PipelineId::RectInstanced)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    let offset = before.rects as u64 * rect_stride;
                                    render_pass.set_vertex_buffer(1, buf.slice(offset..));
                                    render_pass.draw_indexed(0..6, 0, 0..rect_count);
                                }
                            }
                        }

                        // === Circles (range: before.circles..after.circles) ===
                        let circle_count = after.circles - before.circles;
                        if circle_count > 0 {
                            if let Some(ref buf) = circle_buf {
                                if let Some(pipeline) =
                                    self.gpu.pipelines().get(PipelineId::CircleInstanced)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    let offset = before.circles as u64 * circle_stride;
                                    render_pass.set_vertex_buffer(1, buf.slice(offset..));
                                    render_pass.draw_indexed(0..6, 0, 0..circle_count);
                                }
                            }
                        }

                        // === Arcs (range: before.arcs..after.arcs) ===
                        let arc_count = after.arcs - before.arcs;
                        if arc_count > 0 {
                            if let Some(ref buf) = arc_buf {
                                if let Some(pipeline) =
                                    self.gpu.pipelines().get(PipelineId::ArcInstanced)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    let offset = before.arcs as u64 * arc_stride;
                                    render_pass.set_vertex_buffer(1, buf.slice(offset..));
                                    render_pass.draw_indexed(0..6, 0, 0..arc_count);
                                }
                            }
                        }

                        // === Paths (range: before.path_draw_ranges..after.path_draw_ranges) ===
                        let path_range_count = after.path_draw_ranges - before.path_draw_ranges;
                        if path_range_count > 0 {
                            if let (Some(ref verts), Some(ref idxs)) =
                                (&path_verts_buf, &path_idxs_buf)
                            {
                                if let Some(pipeline) =
                                    self.gpu.pipelines().get(PipelineId::PathFill)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    render_pass.set_vertex_buffer(0, verts.slice(..));
                                    render_pass.set_index_buffer(
                                        idxs.slice(..),
                                        wgpu::IndexFormat::Uint32,
                                    );
                                    let ranges = self.batchers.paths.draw_ranges();
                                    for i in before.path_draw_ranges..after.path_draw_ranges {
                                        let range = &ranges[i as usize];
                                        render_pass.draw_indexed(
                                            range.start_index
                                                ..(range.start_index + range.index_count),
                                            0,
                                            0..1,
                                        );
                                    }
                                    // Restore unit quad for subsequent instanced draws
                                    render_pass
                                        .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                    render_pass.set_index_buffer(
                                        self.gpu.unit_quad_ibo().slice(..),
                                        wgpu::IndexFormat::Uint16,
                                    );
                                }
                            }
                        }

                        // === Shadows (range: before.shadows..after.shadows) ===
                        let shadow_count = after.shadows - before.shadows;
                        if shadow_count > 0 {
                            if let Some(ref buf) = shadow_buf {
                                if let Some(pipeline) = self.gpu.pipelines().get(PipelineId::Shadow)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    let offset = before.shadows as u64 * shadow_stride;
                                    render_pass.set_vertex_buffer(1, buf.slice(offset..));
                                    render_pass.draw_indexed(0..6, 0, 0..shadow_count);
                                }
                            }
                        }

                        // === Images (instanced per texture group) ===
                        let img_before = before.images;
                        let img_after = after.images;
                        if img_after > img_before {
                            if let Some(ref buf) = image_buf {
                                if let Some(pipeline) = self.gpu.pipelines().get(PipelineId::Image)
                                {
                                    render_pass.set_pipeline(pipeline);
                                    // Restore unit quad for instanced draws
                                    render_pass
                                        .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                    render_pass.set_index_buffer(
                                        self.gpu.unit_quad_ibo().slice(..),
                                        wgpu::IndexFormat::Uint16,
                                    );

                                    let mut instance_offset: u32 = 0;
                                    for (texture_id, instances) in
                                        self.batchers.images.groups_in_order()
                                    {
                                        let count = instances.len() as u32;
                                        // Skip groups outside [img_before..img_after]
                                        let group_end = instance_offset + count;
                                        if group_end <= img_before || instance_offset >= img_after {
                                            instance_offset = group_end;
                                            continue;
                                        }

                                        let mut cache = self.gpu.texture_cache().lock();
                                        if let Some(cached) = cache.get(texture_id) {
                                            let bind_group =
                                                self.gpu
                                                    .device()
                                                    .create_bind_group(&wgpu::BindGroupDescriptor {
                                                    label: Some("image_texture_bind"),
                                                    layout: self.gpu.image_bind_group_layout(),
                                                    entries: &[
                                                        wgpu::BindGroupEntry {
                                                            binding: 0,
                                                            resource:
                                                                wgpu::BindingResource::Sampler(
                                                                    self.gpu.image_sampler(),
                                                                ),
                                                        },
                                                        wgpu::BindGroupEntry {
                                                            binding: 1,
                                                            resource:
                                                                wgpu::BindingResource::TextureView(
                                                                    &cached.view,
                                                                ),
                                                        },
                                                    ],
                                                });
                                            render_pass.set_bind_group(1, &bind_group, &[]);

                                            let buf_offset =
                                                instance_offset as u64 * image_instance_stride;
                                            render_pass
                                                .set_vertex_buffer(1, buf.slice(buf_offset..));
                                            render_pass.draw_indexed(0..6, 0, 0..count);
                                        }
                                        drop(cache);

                                        instance_offset = group_end;
                                    }

                                    // Restore viewport bind group after image bind group(1) usage
                                    render_pass.set_bind_group(
                                        0,
                                        self.surface.viewport_bind_group(),
                                        &[],
                                    );
                                }
                            }
                        }

                        // === Linear Gradients (range: before.linear_gradients..after.linear_gradients) ===
                        #[allow(clippy::cast_possible_truncation)]
                        if after.linear_gradients > before.linear_gradients {
                            if let Some(pipeline) =
                                self.gpu.pipelines().get(PipelineId::LinearGradient)
                            {
                                render_pass.set_pipeline(pipeline);
                                render_pass
                                    .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                render_pass.set_index_buffer(
                                    self.gpu.unit_quad_ibo().slice(..),
                                    wgpu::IndexFormat::Uint16,
                                );

                                let gradient_bgl =
                                    self.gpu.pipelines().gradient_bind_group_layout();
                                let grads = self.batchers.effects.linear_gradients();
                                for i in before.linear_gradients..after.linear_gradients {
                                    let grad = &grads[i as usize];
                                    let uniforms = LinearGradientUniforms {
                                        bounds: grad.bounds,
                                        start_end: [
                                            grad.start[0],
                                            grad.start[1],
                                            grad.end[0],
                                            grad.end[1],
                                        ],
                                        corner_radii: grad.corner_radii,
                                        stop_count: grad.stops.len() as u32,
                                        _padding: [0; 3],
                                    };

                                    let uniform_data = bytemuck::bytes_of(&uniforms);
                                    let uniform_buf = {
                                        let mut pool = self.gpu.buffer_pool().lock();
                                        pool.get_uniform_buffer(
                                            self.gpu.device(),
                                            uniform_data.len() as u64,
                                        )
                                        .clone()
                                    };
                                    self.gpu.queue().write_buffer(&uniform_buf, 0, uniform_data);

                                    let stops_data = bytemuck::cast_slice(&grad.stops);
                                    let stops_buf = self.gpu.device().create_buffer_init(
                                        &wgpu::util::BufferInitDescriptor {
                                            label: Some("linear_gradient_stops"),
                                            contents: stops_data,
                                            usage: wgpu::BufferUsages::STORAGE,
                                        },
                                    );

                                    let bind_group = self.gpu.device().create_bind_group(
                                        &wgpu::BindGroupDescriptor {
                                            label: Some("linear_gradient_bind_group"),
                                            layout: gradient_bgl,
                                            entries: &[
                                                wgpu::BindGroupEntry {
                                                    binding: 0,
                                                    resource: uniform_buf.as_entire_binding(),
                                                },
                                                wgpu::BindGroupEntry {
                                                    binding: 1,
                                                    resource: stops_buf.as_entire_binding(),
                                                },
                                            ],
                                        },
                                    );

                                    render_pass.set_bind_group(1, &bind_group, &[]);
                                    render_pass.draw_indexed(0..6, 0, 0..1);
                                }

                                render_pass.set_bind_group(
                                    0,
                                    self.surface.viewport_bind_group(),
                                    &[],
                                );
                            }
                        }

                        // === Radial Gradients (range: before.radial_gradients..after.radial_gradients) ===
                        #[allow(clippy::cast_possible_truncation)]
                        if after.radial_gradients > before.radial_gradients {
                            if let Some(pipeline) =
                                self.gpu.pipelines().get(PipelineId::RadialGradient)
                            {
                                render_pass.set_pipeline(pipeline);
                                render_pass
                                    .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                render_pass.set_index_buffer(
                                    self.gpu.unit_quad_ibo().slice(..),
                                    wgpu::IndexFormat::Uint16,
                                );

                                let gradient_bgl =
                                    self.gpu.pipelines().gradient_bind_group_layout();
                                let grads = self.batchers.effects.radial_gradients();
                                for i in before.radial_gradients..after.radial_gradients {
                                    let grad = &grads[i as usize];
                                    let uniforms = RadialGradientUniforms {
                                        bounds: grad.bounds,
                                        center_radius: [
                                            grad.center[0],
                                            grad.center[1],
                                            grad.radius,
                                            0.0,
                                        ],
                                        corner_radii: grad.corner_radii,
                                        stop_count: grad.stops.len() as u32,
                                        _padding: [0; 3],
                                    };

                                    let uniform_data = bytemuck::bytes_of(&uniforms);
                                    let uniform_buf = {
                                        let mut pool = self.gpu.buffer_pool().lock();
                                        pool.get_uniform_buffer(
                                            self.gpu.device(),
                                            uniform_data.len() as u64,
                                        )
                                        .clone()
                                    };
                                    self.gpu.queue().write_buffer(&uniform_buf, 0, uniform_data);

                                    let stops_data = bytemuck::cast_slice(&grad.stops);
                                    let stops_buf = self.gpu.device().create_buffer_init(
                                        &wgpu::util::BufferInitDescriptor {
                                            label: Some("radial_gradient_stops"),
                                            contents: stops_data,
                                            usage: wgpu::BufferUsages::STORAGE,
                                        },
                                    );

                                    let bind_group = self.gpu.device().create_bind_group(
                                        &wgpu::BindGroupDescriptor {
                                            label: Some("radial_gradient_bind_group"),
                                            layout: gradient_bgl,
                                            entries: &[
                                                wgpu::BindGroupEntry {
                                                    binding: 0,
                                                    resource: uniform_buf.as_entire_binding(),
                                                },
                                                wgpu::BindGroupEntry {
                                                    binding: 1,
                                                    resource: stops_buf.as_entire_binding(),
                                                },
                                            ],
                                        },
                                    );

                                    render_pass.set_bind_group(1, &bind_group, &[]);
                                    render_pass.draw_indexed(0..6, 0, 0..1);
                                }

                                render_pass.set_bind_group(
                                    0,
                                    self.surface.viewport_bind_group(),
                                    &[],
                                );
                            }
                        }

                        // === Sweep Gradients (range: before.sweep_gradients..after.sweep_gradients) ===
                        #[allow(clippy::cast_possible_truncation)]
                        if after.sweep_gradients > before.sweep_gradients {
                            if let Some(pipeline) =
                                self.gpu.pipelines().get(PipelineId::SweepGradient)
                            {
                                render_pass.set_pipeline(pipeline);
                                render_pass
                                    .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                render_pass.set_index_buffer(
                                    self.gpu.unit_quad_ibo().slice(..),
                                    wgpu::IndexFormat::Uint16,
                                );

                                let gradient_bgl =
                                    self.gpu.pipelines().gradient_bind_group_layout();
                                let grads = self.batchers.effects.sweep_gradients();
                                for i in before.sweep_gradients..after.sweep_gradients {
                                    let grad = &grads[i as usize];
                                    let uniforms = SweepGradientUniforms {
                                        bounds: grad.bounds,
                                        center_angles: [
                                            grad.center[0],
                                            grad.center[1],
                                            grad.start_angle,
                                            grad.end_angle,
                                        ],
                                        corner_radii: grad.corner_radii,
                                        stop_count: grad.stops.len() as u32,
                                        _padding: [0; 3],
                                    };

                                    let uniform_data = bytemuck::bytes_of(&uniforms);
                                    let uniform_buf = {
                                        let mut pool = self.gpu.buffer_pool().lock();
                                        pool.get_uniform_buffer(
                                            self.gpu.device(),
                                            uniform_data.len() as u64,
                                        )
                                        .clone()
                                    };
                                    self.gpu.queue().write_buffer(&uniform_buf, 0, uniform_data);

                                    let stops_data = bytemuck::cast_slice(&grad.stops);
                                    let stops_buf = self.gpu.device().create_buffer_init(
                                        &wgpu::util::BufferInitDescriptor {
                                            label: Some("sweep_gradient_stops"),
                                            contents: stops_data,
                                            usage: wgpu::BufferUsages::STORAGE,
                                        },
                                    );

                                    let bind_group = self.gpu.device().create_bind_group(
                                        &wgpu::BindGroupDescriptor {
                                            label: Some("sweep_gradient_bind_group"),
                                            layout: gradient_bgl,
                                            entries: &[
                                                wgpu::BindGroupEntry {
                                                    binding: 0,
                                                    resource: uniform_buf.as_entire_binding(),
                                                },
                                                wgpu::BindGroupEntry {
                                                    binding: 1,
                                                    resource: stops_buf.as_entire_binding(),
                                                },
                                            ],
                                        },
                                    );

                                    render_pass.set_bind_group(1, &bind_group, &[]);
                                    render_pass.draw_indexed(0..6, 0, 0..1);
                                }

                                render_pass.set_bind_group(
                                    0,
                                    self.surface.viewport_bind_group(),
                                    &[],
                                );
                            }
                        }

                        // === Text (range: before.text_runs..after.text_runs) ===
                        if after.text_runs > before.text_runs && has_text {
                            // glyphon renders all prepared text at once; we call
                            // render only on the first group that has text and
                            // rely on glyphon's internal ordering.
                            // TODO: support per-group text rendering when glyphon
                            //       exposes range-based render.
                            text_sys_guard.render(&mut render_pass);
                        }
                    }

                    DrawOp::SetScissor(scissor) => {
                        render_pass.set_scissor_rect(
                            scissor.x,
                            scissor.y,
                            scissor.width,
                            scissor.height,
                        );
                    }

                    DrawOp::ClearScissor => {
                        render_pass.set_scissor_rect(0, 0, surface_w, surface_h);
                    }

                    DrawOp::StencilPush {
                        clip_verts_start: _,
                        clip_verts_count: _,
                        clip_idx_start,
                        clip_idx_count,
                    } => {
                        if let (Some(ref verts), Some(ref idxs)) =
                            (&path_verts_buf, &path_idxs_buf)
                        {
                            // Write clip shape to stencil buffer (increment)
                            if let Some(pipeline) = self
                                .gpu
                                .pipelines()
                                .get(PipelineId::StencilWriteIncrement)
                            {
                                render_pass.set_pipeline(pipeline);
                                render_pass.set_vertex_buffer(0, verts.slice(..));
                                render_pass.set_index_buffer(
                                    idxs.slice(..),
                                    wgpu::IndexFormat::Uint32,
                                );
                                stencil_ref += 1;
                                render_pass.set_stencil_reference(stencil_ref);
                                render_pass.draw_indexed(
                                    *clip_idx_start
                                        ..(*clip_idx_start + *clip_idx_count),
                                    0,
                                    0..1,
                                );

                                // Restore unit quad for subsequent instanced draws
                                render_pass
                                    .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                render_pass.set_index_buffer(
                                    self.gpu.unit_quad_ibo().slice(..),
                                    wgpu::IndexFormat::Uint16,
                                );
                            }
                        }
                    }

                    DrawOp::StencilPop {
                        clip_verts_start: _,
                        clip_verts_count: _,
                        clip_idx_start,
                        clip_idx_count,
                    } => {
                        if let (Some(ref verts), Some(ref idxs)) =
                            (&path_verts_buf, &path_idxs_buf)
                        {
                            // Write clip shape to stencil buffer (decrement)
                            if let Some(pipeline) = self
                                .gpu
                                .pipelines()
                                .get(PipelineId::StencilWriteDecrement)
                            {
                                render_pass.set_pipeline(pipeline);
                                render_pass.set_vertex_buffer(0, verts.slice(..));
                                render_pass.set_index_buffer(
                                    idxs.slice(..),
                                    wgpu::IndexFormat::Uint32,
                                );
                                render_pass.draw_indexed(
                                    *clip_idx_start
                                        ..(*clip_idx_start + *clip_idx_count),
                                    0,
                                    0..1,
                                );
                                stencil_ref = stencil_ref.saturating_sub(1);
                                render_pass.set_stencil_reference(stencil_ref);

                                // Restore unit quad for subsequent instanced draws
                                render_pass
                                    .set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
                                render_pass.set_index_buffer(
                                    self.gpu.unit_quad_ibo().slice(..),
                                    wgpu::IndexFormat::Uint16,
                                );
                            }
                        }
                    }
                }
            }
        }

        // Submit
        self.gpu.queue().submit(std::iter::once(encoder.finish()));

        // Present
        self.surface_texture.present();

        // Trim text atlas after frame
        if has_text {
            text_sys_guard.trim();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batchers_start_empty() {
        let b = Batchers::new();
        assert!(b.is_all_empty());
    }

    // FrameEncoder needs a real GPU surface, so real tests go in integration
    // tests (Task 17).
}
