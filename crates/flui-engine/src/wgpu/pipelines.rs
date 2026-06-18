//! Device-scoped pipeline collection for `WgpuPainter`.
//!
//! [`PipelineSet`] is the single owner of the nine named instanced/gradient/shadow
//! `wgpu::RenderPipeline`s that were previously held as separate fields on
//! [`super::painter::WgpuPainter`], plus the on-demand shape-pipeline cache
//! ([`PipelineCache`]) composed as a sub-field.
//!
//! | Previous painter field                   | Location in `PipelineSet`               |
//! |------------------------------------------|-----------------------------------------|
//! | `pipeline_cache`                         | `PipelineSet::shape_cache`              |
//! | `instanced_rect_pipeline`                | `PipelineSet::instanced_rect`           |
//! | `instanced_circle_pipeline`              | `PipelineSet::instanced_circle`         |
//! | `instanced_arc_pipeline`                 | `PipelineSet::instanced_arc`            |
//! | `instanced_texture_pipeline`             | `PipelineSet::instanced_texture`        |
//! | `instanced_texture_premul_pipeline`      | `PipelineSet::instanced_texture_premul` |
//! | `linear_gradient_pipeline`               | `PipelineSet::linear_gradient`          |
//! | `radial_gradient_pipeline`               | `PipelineSet::radial_gradient`          |
//! | `sweep_gradient_pipeline`                | `PipelineSet::sweep_gradient`           |
//! | `shadow_pipeline`                        | `PipelineSet::shadow`                   |
//! | `texture_bind_group_layout`              | `PipelineSet::texture_bind_group_layout`|
//! | `gradient_bind_group_layout`             | `PipelineSet::gradient_bind_group_layout` (private) |
//! | `gradient_stops_buffer`                  | `PipelineSet::gradient_stops_buffer` (private) |
//! | `gradient_bind_group`                    | `PipelineSet::gradient_bind_group`      |
//!
//! ## Disambiguation from the deleted `pipelines.rs`
//!
//! A previous `pipelines.rs` (deleted in cycle 4, E-6 — see `mod.rs` comment)
//! contained a colliding `PipelineCache` + `PipelineBuilder` pair with zero
//! non-self consumers. **This file is not a resurrection of that module.** It
//! introduces a distinct type (`PipelineSet`) that *composes* the live
//! [`PipelineCache`] from `pipeline.rs` (singular) and adds the nine named
//! pipelines previously scattered across painter fields.
//!
//! ## Viewport bind-group layout identity (HAZARD — must read)
//!
//! wgpu requires that a `BindGroup` and every `RenderPipeline` it is bound to
//! share the **exact same** `BindGroupLayout` object (identity, not structural
//! equality). The layout is owned by [`PipelineCache`] and exposed via
//! [`PipelineSet::viewport_bind_group_layout`], which delegates to
//! [`PipelineCache::viewport_bind_group_layout`]. All pipelines that bind group 0
//! (viewport uniform) are constructed in [`PipelineSet::new`] against that same
//! accessor — ensuring a single shared object. The `viewport_bind_group` on the
//! painter must also be created against this accessor; substituting any other
//! layout object causes a wgpu validation error at the first draw.
//!
//! ## Borrow-split safety
//!
//! The gradient bind-group update is encapsulated in
//! [`PipelineSet::refresh_gradient_bind_group`], which takes `device` and `queue`
//! as shared references and manages both the buffer write and bind-group
//! recreation internally. This prevents a borrow conflict that would arise if the
//! caller needed `&mut self.gradient_bind_group` while also holding
//! `&self.gradient_bind_group_layout`. The pattern mirrors
//! [`super::resources::GpuResources`].

use super::{advanced_blend::AdvancedBlendPipeline, pipeline::PipelineCache};

/// Device-scoped collection of all `wgpu::RenderPipeline`s used by [`super::painter::WgpuPainter`].
///
/// Construction: [`PipelineSet::new`] is format-parametric so a single call
/// serves both windowed and offscreen painter paths.
///
/// # Viewport bind-group layout identity
///
/// Every pipeline in this set was built against the layout returned by
/// [`Self::viewport_bind_group_layout`]. The painter's `viewport_bind_group`
/// **must** be created against the same reference — see the module doc.
// `wgpu::RenderPipeline` / `wgpu::BindGroup` are opaque GPU handles with no
// useful `Debug` impl. The `#[allow]` avoids the compiler error while still
// documenting the reason.
#[allow(missing_debug_implementations)]
pub(crate) struct PipelineSet {
    // ── On-demand shape pipeline cache ───────────────────────────────────────
    //
    // Owns the `viewport_bind_group_layout` that all instanced/gradient/shadow
    // pipelines in this set share.
    shape_cache: PipelineCache,

    // ── Nine named render pipelines ──────────────────────────────────────────
    /// Instanced rect — straight `ALPHA_BLENDING` for UI shapes.
    pub(crate) instanced_rect: wgpu::RenderPipeline,

    /// Instanced circle — straight `ALPHA_BLENDING`.
    pub(crate) instanced_circle: wgpu::RenderPipeline,

    /// Instanced arc — straight `ALPHA_BLENDING`.
    pub(crate) instanced_arc: wgpu::RenderPipeline,

    /// Instanced texture — straight `ALPHA_BLENDING` for decoded images whose
    /// samples carry straight (non-premultiplied) alpha.
    pub(crate) instanced_texture: wgpu::RenderPipeline,

    /// Instanced texture — **premultiplied** source-over blending exclusively
    /// for compositing offscreen layer textures (`flush_opacity_layer`).
    ///
    /// A layer offscreen is cleared to transparent and drawn into with straight
    /// `ALPHA_BLENDING`, leaving every texel premultiplied (`rgb = straight_rgb * a`).
    /// Compositing such a texel with the straight pipeline would re-multiply rgb by
    /// alpha a second time, darkening translucent/AA content. `PREMULTIPLIED_ALPHA_BLENDING`
    /// (src factor `One`) composites correctly; a per-channel tint of
    /// `(C.r*O, C.g*O, C.b*O, O)` applies group opacity `O` and ColorFilter
    /// chroma `C` uniformly across the already-premultiplied texel.
    ///
    /// **Selection invariant:** `flush_texture_batch_with_blend` picks this pipeline
    /// when `premultiplied == true`; the straight pipeline otherwise. Do not
    /// change this selection logic — it is a round-5c color-correctness fix.
    pub(crate) instanced_texture_premul: wgpu::RenderPipeline,

    /// Linear gradient pipeline.
    pub(crate) linear_gradient: wgpu::RenderPipeline,

    /// Radial gradient pipeline.
    pub(crate) radial_gradient: wgpu::RenderPipeline,

    /// Sweep gradient pipeline.
    pub(crate) sweep_gradient: wgpu::RenderPipeline,

    /// Shadow pipeline — analytical shadows with single-pass rendering.
    pub(crate) shadow: wgpu::RenderPipeline,

    // ── Pipeline-adjacent GPU objects ─────────────────────────────────────────
    /// Bind group layout for texture pipelines (sampler at binding 0, texture view at 1).
    ///
    /// Used by `flush_texture_batch_with_blend` to create a per-draw `BindGroup`
    /// that pairs the painter's `default_sampler` with the per-draw `TextureView`.
    pub(crate) texture_bind_group_layout: wgpu::BindGroupLayout,

    /// Bind group layout for gradient pipelines (gradient-stops buffer at binding 0).
    ///
    /// Private: callers reach it via [`Self::refresh_gradient_bind_group`] to
    /// avoid the borrow-split described in the module doc.
    gradient_bind_group_layout: wgpu::BindGroupLayout,

    /// Persistent gradient-stops storage buffer shared across all gradient pipelines.
    ///
    /// Written on every frame that has gradient draws via
    /// [`Self::refresh_gradient_bind_group`].
    gradient_stops_buffer: wgpu::Buffer,

    /// Current gradient-stops bind group, or `None` before the first gradient frame.
    ///
    /// Recreated each frame by [`Self::refresh_gradient_bind_group`] when
    /// `current_gradient_stops` is non-empty.
    pub(crate) gradient_bind_group: Option<wgpu::BindGroup>,

    // ── Advanced-blend composite pipeline ────────────────────────────────────
    /// Backdrop-read advanced-blend pipeline used by `flush_opacity_layer`
    /// (via `flush_advanced_layer` in replay.rs) when a `PendingOpacityLayer`
    /// carries an advanced (non-Porter-Duff) blend mode.
    ///
    /// Format-matched to `surface_format` at construction time; shared across
    /// every `flush_advanced_layer` call for this painter.
    pub(crate) advanced_blend: AdvancedBlendPipeline,
}

impl PipelineSet {
    /// Construct the full pipeline set for `surface_format`.
    ///
    /// `surface_format` is the texture format of the render target (windowed
    /// or offscreen), making this constructor suitable for both paths.
    ///
    /// # Viewport bind-group layout identity
    ///
    /// [`PipelineCache::new`] creates the viewport bind-group layout and owns it.
    /// Every subsequent pipeline layout in this function borrows it via
    /// [`Self::viewport_bind_group_layout`], ensuring a single shared object —
    /// see the module-level hazard note.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // ── Shape pipeline cache (also creates the viewport bind-group layout) ──
        let shape_cache = PipelineCache::new(
            device,
            super::shaders::SHAPE,
            surface_format,
            create_viewport_bind_group_layout(device),
        );

        // ── Shared instanced pipeline layout (rect / circle / arc) ────────────
        //
        // All three instanced shape pipelines share the same layout (viewport at
        // group 0 only).
        let instanced_shape_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Instanced Shape Pipeline Layout"),
                bind_group_layouts: &[Some(shape_cache.viewport_bind_group_layout())],
                immediate_size: 0,
            });

        let instanced_rect =
            create_instanced_rect_pipeline(device, surface_format, &instanced_shape_layout);
        let instanced_circle =
            create_instanced_circle_pipeline(device, surface_format, &instanced_shape_layout);
        let instanced_arc =
            create_instanced_arc_pipeline(device, surface_format, &instanced_shape_layout);

        // ── Texture pipelines ─────────────────────────────────────────────────
        let texture_bind_group_layout = create_texture_bind_group_layout(device);
        let texture_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Instanced Texture Pipeline Layout"),
                bind_group_layouts: &[
                    Some(shape_cache.viewport_bind_group_layout()),
                    Some(&texture_bind_group_layout),
                ],
                immediate_size: 0,
            });

        let instanced_texture =
            create_instanced_texture_pipeline(device, surface_format, &texture_pipeline_layout);
        let instanced_texture_premul = create_instanced_texture_premul_pipeline(
            device,
            surface_format,
            &texture_pipeline_layout,
        );

        // ── Gradient + shadow pipelines ───────────────────────────────────────
        let gradient_stops_buffer = super::effects_pipeline::create_gradient_stops_buffer(device);
        let gradient_bind_group_layout =
            super::effects_pipeline::create_gradient_bind_group_layout(device);
        let gradient_pipeline_layout = super::effects_pipeline::create_gradient_pipeline_layout(
            device,
            shape_cache.viewport_bind_group_layout(),
            &gradient_bind_group_layout,
        );

        let linear_gradient = super::effects_pipeline::create_linear_gradient_pipeline(
            device,
            surface_format,
            &gradient_pipeline_layout,
        );
        let radial_gradient = super::effects_pipeline::create_radial_gradient_pipeline(
            device,
            surface_format,
            &gradient_pipeline_layout,
        );
        let sweep_gradient = super::effects_pipeline::create_sweep_gradient_pipeline(
            device,
            surface_format,
            &gradient_pipeline_layout,
        );
        let shadow = super::effects_pipeline::create_shadow_pipeline(
            device,
            surface_format,
            shape_cache.viewport_bind_group_layout(),
        );

        let advanced_blend = AdvancedBlendPipeline::new(device, surface_format);

        Self {
            shape_cache,
            instanced_rect,
            instanced_circle,
            instanced_arc,
            instanced_texture,
            instanced_texture_premul,
            linear_gradient,
            radial_gradient,
            sweep_gradient,
            shadow,
            texture_bind_group_layout,
            gradient_bind_group_layout,
            gradient_stops_buffer,
            gradient_bind_group: None,
            advanced_blend,
        }
    }

    // ── Viewport bind-group layout ─────────────────────────────────────────────

    /// Reference to the viewport bind-group layout shared by every pipeline in
    /// this set.
    ///
    /// **Identity contract:** the `viewport_bind_group` on the painter **must**
    /// be created against this exact object. Using any other layout — even a
    /// structurally identical one — produces a wgpu validation error at the
    /// first bind call.
    pub(crate) fn viewport_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        self.shape_cache.viewport_bind_group_layout()
    }

    // ── Shape pipeline cache ───────────────────────────────────────────────────

    /// Exclusive reference to the on-demand shape pipeline cache.
    ///
    /// Used in `flush_tessellated_geometry` to retrieve or lazily create a
    /// tessellated shape pipeline for a given [`super::pipeline::PipelineKey`].
    pub(crate) fn shape_cache_mut(&mut self) -> &mut PipelineCache {
        &mut self.shape_cache
    }

    // ── Gradient bind-group refresh ────────────────────────────────────────────

    /// Upload `stops_bytes` to the persistent gradient-stops buffer and
    /// recreate the gradient bind group.
    ///
    /// Must be called once per frame before any gradient pipeline draw, whenever
    /// the current frame has gradient draws. Encapsulates the write + rebind so
    /// that the caller holds neither `&mut gradient_bind_group` nor
    /// `&gradient_bind_group_layout` simultaneously — preventing the borrow
    /// conflict described in the module doc.
    pub(crate) fn refresh_gradient_bind_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stops_bytes: &[u8],
    ) {
        queue.write_buffer(&self.gradient_stops_buffer, 0, stops_bytes);
        self.gradient_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gradient Stops Bind Group"),
            layout: &self.gradient_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.gradient_stops_buffer.as_entire_binding(),
            }],
        }));
    }
}

// ── Viewport bind-group layout factory ────────────────────────────────────────
//
// Single definition; `PipelineCache::new` consumes the produced layout and
// becomes its owner. Must not be called more than once per `PipelineSet`.

fn create_viewport_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Viewport Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

// ── Texture bind-group layout factory ─────────────────────────────────────────

fn create_texture_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Texture Bind Group Layout"),
        entries: &[
            // Sampler (binding 0)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // Texture view (binding 1)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

// ── Per-pipeline factory functions ─────────────────────────────────────────────
//
// Each function is a pure constructor: device + format + layout → one pipeline.
// The shared primitive and multisample state for all instanced-quad pipelines is
// captured in helpers below to avoid subtle divergence.

fn instanced_quad_primitive_state() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        polygon_mode: wgpu::PolygonMode::Fill,
        unclipped_depth: false,
        conservative: false,
    }
}

fn single_sample_multisample_state() -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: 1,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

/// Vertex buffer layout for the shared unit-quad (2-float position only).
fn unit_quad_vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 8, // 2 × f32
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
    }
}

fn create_instanced_rect_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Instanced Rect Shader"),
        source: wgpu::ShaderSource::Wgsl(super::shaders::RECT_INSTANCED.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Rect Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                unit_quad_vertex_buffer_layout(),
                super::instancing::RectInstance::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: instanced_quad_primitive_state(),
        depth_stencil: None,
        multisample: single_sample_multisample_state(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_instanced_circle_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Instanced Circle Shader"),
        source: wgpu::ShaderSource::Wgsl(super::shaders::CIRCLE_INSTANCED.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Circle Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                unit_quad_vertex_buffer_layout(),
                super::instancing::CircleInstance::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: instanced_quad_primitive_state(),
        depth_stencil: None,
        multisample: single_sample_multisample_state(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_instanced_arc_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Instanced Arc Shader"),
        source: wgpu::ShaderSource::Wgsl(super::shaders::ARC_INSTANCED.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Arc Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                unit_quad_vertex_buffer_layout(),
                super::instancing::ArcInstance::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: instanced_quad_primitive_state(),
        depth_stencil: None,
        multisample: single_sample_multisample_state(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_instanced_texture_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Instanced Texture Shader"),
        source: wgpu::ShaderSource::Wgsl(super::shaders::TEXTURE_INSTANCED.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Texture Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                unit_quad_vertex_buffer_layout(),
                super::instancing::TextureInstance::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: instanced_quad_primitive_state(),
        depth_stencil: None,
        multisample: single_sample_multisample_state(),
        multiview_mask: None,
        cache: None,
    })
}

/// Creates the **premultiplied** source-over texture pipeline used exclusively
/// for compositing offscreen layer textures (`flush_opacity_layer`).
///
/// Identical to [`create_instanced_texture_pipeline`] except for the blend state:
/// `PREMULTIPLIED_ALPHA_BLENDING` (src factor `One`) composites premultiplied
/// texels without re-multiplying by alpha. See [`PipelineSet::instanced_texture_premul`]
/// for the full rationale. Do not change the blend state here.
fn create_instanced_texture_premul_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Instanced Texture Premultiplied Shader"),
        source: wgpu::ShaderSource::Wgsl(super::shaders::TEXTURE_INSTANCED.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Instanced Texture Premultiplied Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                unit_quad_vertex_buffer_layout(),
                super::instancing::TextureInstance::desc(),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                // PREMULTIPLIED_ALPHA_BLENDING (src factor One): composites a
                // premultiplied-alpha texel correctly. This is the defining
                // distinction from `create_instanced_texture_pipeline`.
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: instanced_quad_primitive_state(),
        depth_stencil: None,
        multisample: single_sample_multisample_state(),
        multiview_mask: None,
        cache: None,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use wgpu::util::DeviceExt as _;

    use super::super::pipeline::PipelineKey;
    use super::PipelineSet;

    fn test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("PipelineSet Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available when an adapter was found");
        (Arc::new(device), Arc::new(queue))
    }

    /// `PipelineSet::new` completes without panic for `Bgra8Unorm`.
    /// All nine named pipeline fields are reachable (live GPU handles).
    #[test]
    fn all_nine_pipelines_reachable_after_construction() {
        let (device, _queue) = test_device_and_queue();
        let pipeline_set = PipelineSet::new(&device, wgpu::TextureFormat::Bgra8Unorm);

        let _ = &pipeline_set.instanced_rect;
        let _ = &pipeline_set.instanced_circle;
        let _ = &pipeline_set.instanced_arc;
        let _ = &pipeline_set.instanced_texture;
        let _ = &pipeline_set.instanced_texture_premul;
        let _ = &pipeline_set.linear_gradient;
        let _ = &pipeline_set.radial_gradient;
        let _ = &pipeline_set.sweep_gradient;
        let _ = &pipeline_set.shadow;
    }

    /// A `BindGroup` created with the layout from [`PipelineSet::viewport_bind_group_layout`]
    /// is accepted by wgpu — proving the identity contract (HAZARD 1).
    ///
    /// wgpu validates layout identity at bind-group creation time; if the layout
    /// object does not match, the call panics with a validation error.
    #[test]
    fn viewport_bind_group_layout_identity_preserved() {
        let (device, queue) = test_device_and_queue();
        let pipeline_set = PipelineSet::new(&device, wgpu::TextureFormat::Bgra8Unorm);

        // Build a viewport uniform buffer.
        let viewport_data: [f32; 4] = [800.0, 600.0, 0.0, 0.0];
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Test Viewport Uniform Buffer"),
            contents: bytemuck::cast_slice(&viewport_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create the bind group from the exact layout PipelineSet exposes.
        // A successful call proves layout identity — wgpu would panic otherwise.
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Test Viewport Bind Group"),
            layout: pipeline_set.viewport_bind_group_layout(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let _ = viewport_bind_group;

        // Flush wgpu validation.
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Test Flush Encoder"),
        });
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// `shape_cache_mut().get_or_create` does not panic for the default opaque key.
    #[test]
    fn shape_cache_get_or_create_succeeds_for_opaque_key() {
        let (device, _queue) = test_device_and_queue();
        let mut pipeline_set = PipelineSet::new(&device, wgpu::TextureFormat::Bgra8Unorm);
        let _pipeline = pipeline_set
            .shape_cache_mut()
            .get_or_create(&device, PipelineKey::opaque());
    }

    /// `refresh_gradient_bind_group` writes the buffer and sets `gradient_bind_group`
    /// to `Some(…)`.
    #[test]
    fn refresh_gradient_bind_group_produces_non_none_bind_group() {
        let (device, queue) = test_device_and_queue();
        let mut pipeline_set = PipelineSet::new(&device, wgpu::TextureFormat::Bgra8Unorm);

        assert!(
            pipeline_set.gradient_bind_group.is_none(),
            "gradient_bind_group must be None before the first refresh"
        );

        // Two RGBA f32 stops — minimal valid data for the gradient buffer.
        let stop_data: [f32; 8] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        pipeline_set.refresh_gradient_bind_group(&device, &queue, bytemuck::cast_slice(&stop_data));

        assert!(
            pipeline_set.gradient_bind_group.is_some(),
            "gradient_bind_group must be Some after refresh"
        );
    }
}
