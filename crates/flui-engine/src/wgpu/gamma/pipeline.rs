//! Pipeline and uniform layout for the per-pixel gamma transfer filter pass.
//!
//! [`GammaPipeline`] owns the `wgpu::RenderPipeline` used by
//! [`super::apply_gamma`].  It is format-parametric so the same pipeline
//! serves both windowed surface format and pooled offscreen target format.
//!
//! ## Uniform layout
//!
//! `GammaUniform` packs a direction flag into a 16-byte WGSL-compatible block:
//!
//! | Byte offset | Size | Rust field     | WGSL member     |
//! |-------------|------|----------------|-----------------|
//! | 0           | 4    | `direction`    | `u32`           |
//! | 4           | 12   | `_pad`         | `u32` × 3       |
//!
//! Total = 16 bytes (multiple of 16 ✓).
//!
//! ## Direction encoding
//!
//! `direction`:
//! - `0` → `GammaDirection::SrgbToLinear` (sRGB → linear light)
//! - `1` → `GammaDirection::LinearToSrgb` (linear light → sRGB)
//!
//! Must stay in sync with the `if u.direction == 0u` branch in `gamma.wgsl`.

use std::mem;

use bytemuck::{Pod, Zeroable};

use super::super::command_ir::GammaDirection;

// ── Uniform layout ────────────────────────────────────────────────────────────

/// GPU uniform buffer layout for the gamma transfer filter pass.
///
/// **Explicit `#[repr(C)]` layout — must match `GammaUniforms` in `gamma.wgsl`
/// byte-for-byte.**
///
/// | Byte offset | Size | Rust field  | WGSL member |
/// |-------------|------|-------------|-------------|
/// | 0           | 4    | `direction` | `u32`       |
/// | 4           | 12   | `_pad`      | `u32` × 3   |
///
/// Total = 16 bytes (multiple of 16 ✓).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub(super) struct GammaUniform {
    /// Transfer direction: `0` = `SrgbToLinear`, `1` = `LinearToSrgb`.
    /// Must match `gamma_direction_to_u32` below.
    pub(super) direction: u32,
    /// Padding to satisfy WGSL 16-byte uniform block alignment.
    pub(super) _pad: [u32; 3],
}

const _GAMMA_UNIFORM_SIZE_CHECK: () = {
    assert!(mem::size_of::<GammaUniform>() == 16);
};

/// Map a [`GammaDirection`] to its WGSL `u.direction` integer.
///
/// **Must match the `if u.direction == …` constants in `gamma.wgsl`.**
/// This is an exhaustive match — adding a new variant without updating the
/// WGSL const block is a compile error (no `_` fallthrough).
pub(crate) fn gamma_direction_to_u32(dir: GammaDirection) -> u32 {
    match dir {
        GammaDirection::SrgbToLinear => 0,
        GammaDirection::LinearToSrgb => 1,
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the gamma transfer filter pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage | Type                  | Content                      |
/// |---------|-------|-----------------------|------------------------------|
/// | 0       | FS    | Uniform buffer        | [`GammaUniform`]             |
/// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
/// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
///
/// ## Blend state
///
/// `REPLACE` (no fixed-function blending): the fragment shader emits the full
/// premultiplied filtered texel directly.
#[allow(missing_debug_implementations)]
pub(crate) struct GammaPipeline {
    /// Bind-group layout shared between pipeline construction and per-draw
    /// bind-group creation in [`super::apply_gamma`].
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// The single render pipeline (format-parametric at construction time).
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl GammaPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// WGSL shader compilation happens here; a wgpu validation error surfaces
    /// at this call site, which the GPU construction test exercises.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Gamma Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Gamma Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/effects/gamma.wgsl").into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Gamma Pipeline"),
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
        label: Some("Gamma Bind Group Layout"),
        entries: &[
            // Binding 0: uniform buffer (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(mem::size_of::<GammaUniform>() as u64),
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
    use super::{GammaUniform, gamma_direction_to_u32};
    use crate::wgpu::command_ir::GammaDirection;

    /// The struct size must match the WGSL uniform-block size exactly.
    ///
    /// Layout: direction(4) + pad(12) = 16 bytes.
    #[test]
    fn gamma_uniform_size_is_16_bytes() {
        assert_eq!(
            std::mem::size_of::<GammaUniform>(),
            16,
            "GammaUniform must be 16 bytes to match the WGSL GammaUniforms struct"
        );
    }

    /// `gamma_direction_to_u32` maps each variant to a unique u32.
    #[test]
    fn direction_mapping_is_injective() {
        let s = gamma_direction_to_u32(GammaDirection::SrgbToLinear);
        let l = gamma_direction_to_u32(GammaDirection::LinearToSrgb);
        assert_ne!(
            s, l,
            "SrgbToLinear and LinearToSrgb must map to different integers"
        );
        assert_eq!(s, 0, "SrgbToLinear must map to 0");
        assert_eq!(l, 1, "LinearToSrgb must map to 1");
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::GammaPipeline;

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
                label: Some("GammaPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `GammaPipeline::new` completes without a wgpu validation error for
    /// `Rgba8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    #[test]
    fn pipeline_construction_succeeds_for_rgba8unorm() {
        let device = test_device();
        let _pipeline = GammaPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
