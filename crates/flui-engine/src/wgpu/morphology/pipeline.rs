//! Pipeline and uniform layout for the separable morphology filter pass.
//!
//! [`MorphologyPipeline`] owns the `wgpu::RenderPipeline` used by
//! [`super::apply_morphology`].  It is format-parametric so the same pipeline
//! serves both the windowed surface format and any pooled offscreen target format.
//!
//! ## Uniform layout
//!
//! `MorphUniform` is 48 bytes, matching `MorphUniforms` in `morphology.wgsl`:
//!
//! | Byte offset | Size | Rust field        | WGSL member          |
//! |-------------|------|-------------------|----------------------|
//! | 0           | 8    | `texture_size`    | `vec2<f32>`          |
//! | 8           | 4    | `radius`          | `f32`                |
//! | 12          | 4    | `direction`       | `f32`                |
//! | 16          | 16   | `content_rect_uv` | `vec4<f32>`          |
//! | 32          | 4    | `op`              | `f32`                |
//! | 36          | 12   | `_pad`            | `f32 × 3`            |
//!
//! Total = 48 bytes (multiple of 16 ✓).
//!
//! ## Blend state
//!
//! `REPLACE` (no fixed-function blending): the fragment shader emits the full
//! premultiplied result directly.

use std::mem;

use bytemuck::{Pod, Zeroable};

// ── Uniform layout ────────────────────────────────────────────────────────────

/// GPU uniform buffer layout for one H or V morphology sub-pass.
///
/// **Explicit `#[repr(C)]` layout — must match `MorphUniforms` in
/// `morphology.wgsl` byte-for-byte.**
///
/// WGSL uniform alignment (§13.4.1 / WebGPU §3.13.3):
/// - `vec2<f32>` → align 8, size 8
/// - `f32`       → align 4, size 4
/// - `vec4<f32>` → align 16, size 16
///
/// | Byte offset | Size | Field            | Semantics                         |
/// |-------------|------|------------------|-----------------------------------|
/// | 0           | 8    | `texture_size`   | Source texture `[width, height]`  |
/// | 8           | 4    | `radius`         | Kernel half-radius in pixels      |
/// | 12          | 4    | `direction`      | 0.0 = H, 1.0 = V                 |
/// | 16          | 16   | `content_rect_uv`| `[min_u, min_v, max_u, max_v]`   |
/// | 32          | 4    | `op`             | 0.0 = dilate, 1.0 = erode        |
/// | 36          | 12   | `_pad`           | alignment padding                 |
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub(super) struct MorphUniform {
    /// Source texture size in pixels: `[width, height]`.
    pub(super) texture_size: [f32; 2],
    /// Kernel half-radius in physical pixels.
    pub(super) radius: f32,
    /// Pass direction: `0.0` = horizontal (U axis), `1.0` = vertical (V axis).
    pub(super) direction: f32,
    /// Content rectangle as normalised UV coordinates `[min_u, min_v, max_u, max_v]`.
    ///
    /// Samples outside this rectangle are replaced by the neutral element (decal).
    pub(super) content_rect_uv: [f32; 4],
    /// Operation selector: `0.0` = dilate (max), `1.0` = erode (min).
    pub(super) op: f32,
    /// Alignment padding — three `f32` fields so the struct is a multiple of 16.
    pub(super) _pad: [f32; 3],
}

const _MORPH_UNIFORM_SIZE_CHECK: () = {
    assert!(mem::size_of::<MorphUniform>() == 48);
};

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the morphology filter pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage | Type                  | Content                          |
/// |---------|-------|-----------------------|----------------------------------|
/// | 0       | FS    | Uniform buffer        | [`MorphUniform`]                 |
/// | 1       | FS    | 2D float texture      | Source (premultiplied RGBA)      |
/// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge            |
///
/// ## Blend state
///
/// `REPLACE`: the shader emits the final premultiplied result; the GPU must not
/// re-blend it.
#[allow(missing_debug_implementations)]
pub(crate) struct MorphologyPipeline {
    /// Bind-group layout shared between pipeline construction and per-draw
    /// bind-group creation in [`super::apply_morphology`].
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// The single render pipeline (format-parametric at construction time).
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl MorphologyPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// WGSL shader compilation happens here; a wgpu validation error surfaces at
    /// this call site, which the GPU construction test exercises.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Morphology Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Morphology Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/effects/morphology.wgsl").into(),
            ),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Morphology Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                // No vertex buffers: the VS synthesises 6 vertices from
                // `@builtin(vertex_index)`, forming a full-viewport quad.
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // REPLACE: the shader emits the full premultiplied result.
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            bind_group_layout,
            pipeline,
        }
    }
}

// ── Bind-group layout factory (private) ──────────────────────────────────────

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Morphology Bind Group Layout"),
        entries: &[
            // Binding 0: uniform buffer (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(mem::size_of::<MorphUniform>() as u64),
                },
                count: None,
            },
            // Binding 1: source layer texture (FS) — non-filtering (Nearest)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Binding 2: nearest-clamp sampler (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod cpu_tests {
    use super::MorphUniform;

    /// The struct size must match the WGSL uniform-block size exactly (48 bytes).
    #[test]
    fn morph_uniform_size_is_48_bytes() {
        assert_eq!(
            std::mem::size_of::<MorphUniform>(),
            48,
            "MorphUniform must be 48 bytes to match MorphUniforms in morphology.wgsl"
        );
    }

    /// Round-trip: values written into the struct are readable from the same
    /// byte positions — proves the `#[repr(C)]` layout has no hidden reordering.
    #[allow(
        clippy::float_cmp,
        reason = "comparing floats we just assigned from exact literals — bit identity is the invariant"
    )]
    #[test]
    fn morph_uniform_field_layout_round_trips() {
        let u = MorphUniform {
            texture_size: [640.0, 480.0],
            radius: 3.0,
            direction: 1.0,
            content_rect_uv: [0.1, 0.2, 0.9, 0.8],
            op: 0.0,
            _pad: [0.0; 3],
        };
        assert_eq!(u.texture_size, [640.0, 480.0]);
        assert_eq!(u.radius, 3.0);
        assert_eq!(u.direction, 1.0);
        assert_eq!(u.content_rect_uv, [0.1, 0.2, 0.9, 0.8]);
        assert_eq!(u.op, 0.0);
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::MorphologyPipeline;

    fn test_device() -> wgpu::Device {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("MorphologyPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `MorphologyPipeline::new` completes without a wgpu validation error for
    /// `Rgba8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    #[test]
    fn pipeline_construction_succeeds_for_rgba8unorm() {
        let device = test_device();
        let _pipeline = MorphologyPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
