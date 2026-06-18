//! Pipeline and mode-discriminant mapping for the advanced-blend composite pass.
//!
//! [`AdvancedBlendPipeline`] owns the single `wgpu::RenderPipeline` used by
//! [`super::flush_advanced_layer`].  It is format-parametric so the same pipeline
//! serves both the windowed surface format and any offscreen target format.
//!
//! [`mode_to_u32`] is the canonical mapping from the 15 advanced [`BlendMode`]
//! variants to the `u32` discriminants the WGSL `switch` consumes.  Passing a
//! non-advanced mode (Porter-Duff or Modulate) to `mode_to_u32` is a caller
//! contract violation; the function panics with an invariant message.

use std::mem;

use bytemuck::{Pod, Zeroable};
use flui_types::painting::BlendMode;

// ── Uniform layout (mirrors `BlendUniforms` in advanced_blend.wgsl) ──────────

/// GPU uniform buffer layout for the advanced-blend pass.
///
/// **Explicit `#[repr(C)]` layout — must match `BlendUniforms` in
/// `advanced_blend.wgsl` byte-for-byte.**
///
/// WGSL uniform-block alignment rules (WGSL §13.4.1, WebGPU §3.13.3):
/// - `vec4<f32>` → align 16
/// - `vec2<f32>` → align 8
/// - `f32`       → align 4
/// - `vec3<f32>` → align **16** (not 12; the spec rounds vec3 up to vec4 alignment)
/// - `u32`       → align 4
/// - Struct size must be a multiple of 16 (largest member alignment).
///
/// | Byte offset | Size | Rust field       | WGSL member                |
/// |-------------|------|------------------|----------------------------|
/// | 0           | 16   | `op_bounds`      | `vec4<f32>` (op_bounds)    |
/// | 16          | 8    | `viewport_size`  | `vec2<f32>`                |
/// | 24          | 8    | `copy_origin`    | `vec2<f32>`                |
/// | 32          | 8    | `copy_extent`    | `vec2<f32>`                |
/// | 40          | 4    | `opacity`        | `f32`                      |
/// | 44          | 4    | `_pad0`          | `f32` (align gap)          |
/// | 48          | 12   | `tint_rgb`       | `vec3<f32>` (align 16)     |
/// | 60          | 4    | `mode`           | `u32`                      |
/// | 64          | 16   | `_pad1`          | `vec4<u32>` (size pad)     |
///
/// Total = 80 bytes (multiple of 16).
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub(super) struct BlendUniformData {
    /// Device-space bounds [x, y, width, height] in pixels.
    pub(super) op_bounds: [f32; 4],
    /// Viewport size [width, height] in pixels.
    pub(super) viewport_size: [f32; 2],
    /// Backdrop copy rect origin [x, y] in pixels.
    pub(super) copy_origin: [f32; 2],
    /// Backdrop copy rect extent [width, height] in pixels.
    pub(super) copy_extent: [f32; 2],
    /// Group opacity in [0, 1].
    pub(super) opacity: f32,
    /// Alignment gap: `tint_rgb` (`vec3<f32>`) requires 16-byte WGSL alignment,
    /// so the 4 bytes between `opacity` (offset 40) and `tint_rgb` (offset 48)
    /// must be padded.  Must remain zero.
    pub(super) _pad0: u32,
    /// Per-channel RGB tint applied premultiplied.
    pub(super) tint_rgb: [f32; 3],
    /// Blend mode discriminant (see [`mode_to_u32`]).
    pub(super) mode: u32,
    /// Struct-size padding to reach 80 bytes (next multiple of 16 after 64+12=76).
    /// Must remain zero.
    pub(super) _pad1: [u32; 4],
}

const _BLEND_UNIFORM_SIZE_CHECK: () = {
    assert!(mem::size_of::<BlendUniformData>() == 80);
};

// ── Mode discriminant mapping ─────────────────────────────────────────────────

/// Map an advanced [`BlendMode`] to the `u32` discriminant consumed by the
/// WGSL `separable_blend` / `nonseparable_blend` switch statements.
///
/// ## Invariant
///
/// The caller **must** only pass one of the 15 advanced modes (Multiply through
/// Luminosity).  Passing a Porter-Duff or Modulate mode is a contract violation:
/// the function panics with a message naming the illegal mode, because those
/// modes must be dispatched through the Porter-Duff / Modulate paths — not the
/// advanced-blend shader.
///
/// The discriminant assignment (0–14) is an engine-internal contract shared
/// only between this function and `advanced_blend.wgsl`.  It is NOT derived
/// from `BlendMode`'s Rust enum discriminant.
pub(crate) fn mode_to_u32(mode: BlendMode) -> u32 {
    match mode {
        // ── Separable modes (0–10) ────────────────────────────────────────
        BlendMode::Multiply => 0,
        BlendMode::Screen => 1,
        BlendMode::Overlay => 2,
        BlendMode::Darken => 3,
        BlendMode::Lighten => 4,
        BlendMode::ColorDodge => 5,
        BlendMode::ColorBurn => 6,
        BlendMode::HardLight => 7,
        BlendMode::SoftLight => 8,
        BlendMode::Difference => 9,
        BlendMode::Exclusion => 10,
        // ── Non-separable modes (11–14) ───────────────────────────────────
        BlendMode::Hue => 11,
        BlendMode::Saturation => 12,
        BlendMode::Color => 13,
        BlendMode::Luminosity => 14,
        // ── Non-advanced (Porter-Duff + Modulate) modes: caller contract violation ──
        //
        // Listed exhaustively so adding a new BlendMode variant is a *compile error*,
        // not a silent runtime panic.  A wildcard arm would compile fine and only
        // panic at runtime — that defeats the static-exhaustiveness guarantee.
        BlendMode::Clear
        | BlendMode::Src
        | BlendMode::Dst
        | BlendMode::SrcOver
        | BlendMode::DstOver
        | BlendMode::SrcIn
        | BlendMode::DstIn
        | BlendMode::SrcOut
        | BlendMode::DstOut
        | BlendMode::SrcATop
        | BlendMode::DstATop
        | BlendMode::Xor
        | BlendMode::Plus
        | BlendMode::Modulate => unreachable!(
            "mode_to_u32 called with non-advanced blend mode {:?}; \
             Porter-Duff and Modulate modes must be dispatched through their \
             own fixed-function paths, not the advanced-blend shader",
            mode
        ),
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the advanced-blend composite pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage    | Type                    | Content                      |
/// |---------|----------|-------------------------|------------------------------|
/// | 0       | VS + FS  | Uniform buffer          | [`BlendUniformData`]         |
/// | 1       | FS       | 2D float texture        | Foreground (premultiplied)   |
/// | 2       | FS       | 2D float texture        | Backdrop copy (premultiplied)|
/// | 3       | FS       | Non-filtering sampler   | Nearest + ClampToEdge        |
///
/// ## Blend state
///
/// `REPLACE` (no fixed-function blending): the fragment shader emits the full
/// premultiplied composite directly; the GPU hardware must not re-blend it.
pub(crate) struct AdvancedBlendPipeline {
    /// Bind-group layout shared between pipeline construction and per-draw
    /// bind-group creation in [`super::flush_advanced_layer`].
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    /// The single render pipeline (format-parametric at construction time).
    pub(crate) pipeline: wgpu::RenderPipeline,
}

#[allow(
    dead_code,
    reason = "Driven by the renderer-layer advanced-blend interception; \
              exercised here by the synthetic-op GPU gate"
)]
impl AdvancedBlendPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// Compilation of the WGSL shader module happens here; a wgpu validation
    /// error surfaces at this call site, which the synthetic-op GPU test
    /// exercises before any production caller exists.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Advanced Blend Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Advanced Blend Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/advanced_blend.wgsl").into()),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Advanced Blend Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                // No vertex buffers: the VS synthesises 6 vertices from
                // `@builtin(vertex_index)`, forming a quad over `op_bounds`.
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    // REPLACE: the shader emits the full premultiplied composite.
                    // Fixed-function blending must not re-blend the result.
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

#[allow(
    dead_code,
    reason = "Called by AdvancedBlendPipeline::new which is gated behind the \
              renderer-layer interception; exercised by the synthetic-op GPU gate"
)]
fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Advanced Blend Bind Group Layout"),
        entries: &[
            // Binding 0: uniform buffer (VS + FS)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        mem::size_of::<BlendUniformData>() as u64
                    ),
                },
                count: None,
            },
            // Binding 1: foreground texture (FS) — non-filtering (Nearest sampler)
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
            // Binding 2: backdrop copy texture (FS) — non-filtering
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Binding 3: nearest-clamp sampler (FS)
            wgpu::BindGroupLayoutEntry {
                binding: 3,
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
    use std::collections::HashSet;

    use flui_types::painting::BlendMode;

    use super::mode_to_u32;

    /// All 15 advanced modes map to distinct `u32` values in `[0, 14]`.
    ///
    /// This is the runtime-observable exhaustiveness proof: if `mode_to_u32`
    /// duplicates a discriminant this test fails before a GPU round-trip.
    #[test]
    fn mode_to_u32_produces_distinct_values_for_all_15_advanced_modes() {
        let advanced_modes = [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];

        let discriminants: Vec<u32> = advanced_modes.iter().map(|&m| mode_to_u32(m)).collect();

        assert_eq!(
            discriminants.len(),
            15,
            "expected exactly 15 advanced modes"
        );

        for (mode, &disc) in advanced_modes.iter().zip(discriminants.iter()) {
            assert!(
                disc <= 14,
                "mode {mode:?} maps to discriminant {disc} which is out of range [0, 14]"
            );
        }

        let mut seen = HashSet::new();
        for (mode, &disc) in advanced_modes.iter().zip(discriminants.iter()) {
            assert!(
                seen.insert(disc),
                "mode {mode:?} has duplicate discriminant {disc}"
            );
        }
    }

    /// The struct size must match the WGSL uniform-block size exactly.
    ///
    /// This test makes the compile-time assert observable as a test failure
    /// (rather than a build failure) for CI configurations that compile without
    /// the const-eval size check being evaluated.
    #[test]
    fn blend_uniform_data_size_is_80_bytes() {
        assert_eq!(
            std::mem::size_of::<super::BlendUniformData>(),
            80,
            "BlendUniformData must be 80 bytes to match the WGSL BlendUniforms struct"
        );
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::AdvancedBlendPipeline;

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
                label: Some("AdvancedBlendPipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `AdvancedBlendPipeline::new` completes without a wgpu validation error for
    /// `Bgra8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    #[test]
    fn pipeline_construction_succeeds_for_bgra8unorm() {
        let device = test_device();
        let _pipeline = AdvancedBlendPipeline::new(&device, wgpu::TextureFormat::Bgra8Unorm);
    }
}
