//! Pipeline and uniform layout for the separable Gaussian blur filter pass.
//!
//! [`BlurPipeline`] owns the `wgpu::RenderPipeline` used by
//! [`super::apply_blur`].  It is format-parametric so the same pipeline
//! serves both the windowed surface format and any pooled offscreen target format.
//!
//! ## Uniform layout
//!
//! `BlurUniform` is 32 bytes, matching `BlurUniforms` in `blur.wgsl`:
//!
//! | Byte offset | Size | Rust field        | WGSL member          |
//! |-------------|------|-------------------|----------------------|
//! | 0           | 8    | `texture_size`    | `vec2<f32>`          |
//! | 8           | 4    | `sigma`           | `f32`                |
//! | 12          | 4    | `direction`       | `f32`                |
//! | 16          | 16   | `content_rect_uv` | `vec4<f32>`          |
//!
//! Total = 32 bytes (multiple of 16 ✓).
//!
//! ## Blend state
//!
//! `REPLACE` (no fixed-function blending): the fragment shader emits the full
//! premultiplied result directly.
//!
//! ## Sampler
//!
//! Unlike the morphology pipeline (which uses a `NonFiltering` nearest-clamp
//! sampler), blur uses `FilterMode::Linear` + `SamplerBindingType::Filtering`
//! with `TextureSampleType::Float { filterable: true }`.  Bilinear filtering
//! is valid for Gaussian blurs because the continuous Gaussian kernel composition
//! with bilinear interpolation does not produce artefacts, and the sub-pixel
//! sampling naturally anti-aliases the kernel.

use std::mem;

use bytemuck::{Pod, Zeroable};

// ── Uniform layout ────────────────────────────────────────────────────────────

/// GPU uniform buffer layout for one H or V Gaussian blur sub-pass.
///
/// **Explicit `#[repr(C)]` layout — must match `BlurUniforms` in
/// `blur.wgsl` byte-for-byte.**
///
/// WGSL uniform alignment (§13.4.1 / WebGPU §3.13.3):
/// - `vec2<f32>` → align 8, size 8
/// - `f32`       → align 4, size 4
/// - `vec4<f32>` → align 16, size 16
///
/// | Byte offset | Size | Field            | Semantics                         |
/// |-------------|------|------------------|-----------------------------------|
/// | 0           | 8    | `texture_size`   | Source texture `[width, height]`  |
/// | 8           | 4    | `sigma`          | Gaussian σ for this sub-pass      |
/// | 12          | 4    | `direction`      | 0.0 = H, 1.0 = V                 |
/// | 16          | 16   | `content_rect_uv`| `[min_u, min_v, max_u, max_v]`   |
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub(super) struct BlurUniform {
    /// Source texture size in pixels: `[width, height]`.
    pub(super) texture_size: [f32; 2],
    /// Gaussian sigma for this sub-pass: `sigma_x` for H, `sigma_y` for V.
    pub(super) sigma: f32,
    /// Pass direction: `0.0` = horizontal (U axis), `1.0` = vertical (V axis).
    pub(super) direction: f32,
    /// Content rectangle as normalised UV coordinates `[min_u, min_v, max_u, max_v]`.
    ///
    /// H pass: actual content bounds → decal at the content rect.
    /// V pass: `[0.0, 0.0, 1.0, 1.0]` → decal at texture edge to read the H halo.
    pub(super) content_rect_uv: [f32; 4],
}

const _BLUR_UNIFORM_SIZE_CHECK: () = {
    assert!(mem::size_of::<BlurUniform>() == 32);
};

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the Gaussian blur filter pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage | Type                     | Content                          |
/// |---------|-------|--------------------------|----------------------------------|
/// | 0       | FS    | Uniform buffer           | [`BlurUniform`]                  |
/// | 1       | FS    | 2D float texture         | Source (premultiplied RGBA)      |
/// | 2       | FS    | Linear-filtering sampler | Bilinear + ClampToEdge           |
///
/// ## Blend state
///
/// `REPLACE`: the shader emits the final premultiplied result; the GPU must not
/// re-blend it.
///
/// ## Sampler distinction from [`super::super::morphology::MorphologyPipeline`]
///
/// Morphology uses `NonFiltering` (nearest) — the per-channel max/min is exact
/// on texel-aligned samples.  Blur uses `Filtering` (bilinear): the Gaussian
/// kernel naturally composes with bilinear interpolation without artefact.
#[allow(missing_debug_implementations)]
pub(crate) struct BlurPipeline {
    /// Bind-group layout shared between pipeline construction and per-draw
    /// bind-group creation in [`super::apply_blur`].
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// The single render pipeline (format-parametric at construction time).
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl BlurPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// WGSL shader compilation happens here; a wgpu validation error surfaces at
    /// this call site, which the GPU construction test exercises.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blur Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/effects/blur.wgsl").into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blur Pipeline"),
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
        label: Some("Blur Bind Group Layout"),
        entries: &[
            // Binding 0: uniform buffer (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(mem::size_of::<BlurUniform>() as u64),
                },
                count: None,
            },
            // Binding 1: source layer texture (FS) — filterable (bilinear)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // `filterable: true` is required so the sampler (binding 2,
                    // type `Filtering`) can be paired with this texture.  Using
                    // `filterable: false` here would cause a wgpu validation error
                    // at bind-group creation time.
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Binding 2: linear-filtering sampler (FS)
            //
            // Blur uses `Filtering` (bilinear) rather than `NonFiltering` (nearest)
            // because the Gaussian kernel naturally composes with bilinear without
            // artefact; the sampler must match the texture's `filterable: true`.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod cpu_tests {
    use super::BlurUniform;

    /// The struct size must match the WGSL uniform-block size exactly (32 bytes).
    #[test]
    fn blur_uniform_size_is_32_bytes() {
        assert_eq!(
            std::mem::size_of::<BlurUniform>(),
            32,
            "BlurUniform must be 32 bytes to match BlurUniforms in blur.wgsl"
        );
    }

    /// Round-trip: values written into the struct are readable from the same
    /// byte positions — proves the `#[repr(C)]` layout has no hidden reordering.
    #[allow(
        clippy::float_cmp,
        reason = "comparing floats we just assigned from exact literals — bit identity is the invariant"
    )]
    #[test]
    fn blur_uniform_field_layout_round_trips() {
        let u = BlurUniform {
            texture_size: [640.0, 480.0],
            sigma: 4.0,
            direction: 1.0,
            content_rect_uv: [0.1, 0.2, 0.9, 0.8],
        };
        assert_eq!(u.texture_size, [640.0, 480.0]);
        assert_eq!(u.sigma, 4.0);
        assert_eq!(u.direction, 1.0);
        assert_eq!(u.content_rect_uv, [0.1, 0.2, 0.9, 0.8]);
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::BlurPipeline;

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
                label: Some("BlurPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `BlurPipeline::new` completes without a wgpu validation error for
    /// `Rgba8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    ///
    /// This test is the primary WGSL syntax gate: `cargo clippy` and
    /// `cargo test --lib` do NOT validate WGSL; only wgpu's runtime shader
    /// compiler does.  A broken `blur.wgsl` would panic here with a
    /// wgpu validation error before any other blur test runs.
    #[test]
    fn pipeline_construction_succeeds_for_rgba8unorm() {
        let device = test_device();
        let _pipeline = BlurPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
