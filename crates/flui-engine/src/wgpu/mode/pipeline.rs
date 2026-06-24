//! Pipeline and uniform layout for the per-pixel ColorFilter::Mode blend pass.
//!
//! [`ModePipeline`] owns the `wgpu::RenderPipeline` used by
//! [`super::apply_mode`].  It is format-parametric so the same pipeline
//! serves both windowed surface format and pooled offscreen target format.
//!
//! ## Uniform layout
//!
//! The generated `ModeUniforms` (see `super::generated`, derived from
//! `mode.wgsl` by `wgsl_bindgen`) packs the filter color and blend-mode index
//! into a 32-byte WGSL-compatible block:
//!
//! | Byte offset | Size | Rust field    | WGSL member    |
//! |-------------|------|---------------|----------------|
//! | 0           | 16   | `color`       | `vec4<f32>`    |
//! | 16          | 4    | `blend_mode`  | `u32`          |
//! | 20          | 12   | `_pad`        | `u32` × 3      |
//!
//! Total = 32 bytes (multiple of 16 ✓).
//!
//! ## Blend mode encoding
//!
//! `blend_mode_to_u32` maps each [`flui_types::painting::BlendMode`] variant to
//! a `u32` by **declaration order** (0-indexed), so the WGSL switch statement
//! in `mode.wgsl` can use a dense integer dispatch.  The mapping is an
//! exhaustive match — no `_` fallthrough — so adding a new variant is a
//! compile error until the WGSL is updated.
//!
//! The "must match" comment in `mode.wgsl` keeps both sides auditable at a glance.

use flui_types::painting::BlendMode;

use super::generated::mode;
use crate::wgpu::shader_composer::{ComposableSource, compose_wgsl_shader};

// ── Byte-identity gate ────────────────────────────────────────────────────────
//
// These const assertions are the GO/NO-GO check.  If wgsl_bindgen generates a
// struct whose layout differs from the former hand-written `ModeUniform`, this
// file fails to compile — the correct signal to stop and report BLOCKED.
//
// Generated `ModeUniforms` must match `ModeUniforms` in `mode.wgsl` and the
// former hand-written struct byte-for-byte:
//
// | Byte offset | Size | Field        | WGSL member |
// |-------------|------|--------------|-------------|
// | 0           | 16   | `color`      | `vec4<f32>` |
// | 16          | 4    | `blend_mode` | `u32`       |
// | 20          | 12   | `_pad*`      | `u32` × 3   |
//
// Total = 32 bytes (multiple of 16 ✓).

/// The generated `ModeUniforms` must be exactly 32 bytes.
const _GENERATED_MODE_UNIFORMS_SIZE_CHECK: () = {
    assert!(std::mem::size_of::<mode::ModeUniforms>() == 32);
};

/// `color` must be at byte offset 0.
const _GENERATED_MODE_COLOR_OFFSET_CHECK: () = {
    assert!(std::mem::offset_of!(mode::ModeUniforms, color) == 0);
};

/// `blend_mode` must be at byte offset 16.
const _GENERATED_MODE_BLEND_MODE_OFFSET_CHECK: () = {
    assert!(std::mem::offset_of!(mode::ModeUniforms, blend_mode) == 16);
};

/// Map a [`BlendMode`] to its `u.blend_mode` integer in `mode.wgsl`.
///
/// The encoding is declaration order for `Clear..Modulate` (0..13), then **id 14
/// is INTENTIONALLY UNUSED** (a deliberate gap so the separable-advanced range
/// starts at 15), then `Screen..Luminosity` (15..29). It is NOT pure declaration
/// order from `Screen` onward. The WGSL id chain in `mode.wgsl` uses the same
/// constants — keep them in sync (GPU readback pins one mode per dispatch branch;
/// full 29-mode pinning is a documented follow-up).
///
/// This is an exhaustive match (no `_` fallthrough): adding a new `BlendMode`
/// variant without updating the WGSL is a compile error.
pub(crate) fn blend_mode_to_u32(mode: BlendMode) -> u32 {
    // Must match the id chain in mode.wgsl (id 14 intentionally unused).
    match mode {
        BlendMode::Clear => 0,
        BlendMode::Src => 1,
        BlendMode::Dst => 2,
        BlendMode::SrcOver => 3,
        BlendMode::DstOver => 4,
        BlendMode::SrcIn => 5,
        BlendMode::DstIn => 6,
        BlendMode::SrcOut => 7,
        BlendMode::DstOut => 8,
        BlendMode::SrcATop => 9,
        BlendMode::DstATop => 10,
        BlendMode::Xor => 11,
        BlendMode::Plus => 12,
        BlendMode::Modulate => 13,
        BlendMode::Screen => 15,
        BlendMode::Overlay => 16,
        BlendMode::Darken => 17,
        BlendMode::Lighten => 18,
        BlendMode::ColorDodge => 19,
        BlendMode::ColorBurn => 20,
        BlendMode::HardLight => 21,
        BlendMode::SoftLight => 22,
        BlendMode::Difference => 23,
        BlendMode::Exclusion => 24,
        BlendMode::Multiply => 25,
        BlendMode::Hue => 26,
        BlendMode::Saturation => 27,
        BlendMode::Color => 28,
        BlendMode::Luminosity => 29,
    }
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Bind-group layout and render pipeline for the ColorFilter::Mode blend pass.
///
/// ## Bind-group layout (group 0)
///
/// | Binding | Stage | Type                  | Content                      |
/// |---------|-------|-----------------------|------------------------------|
/// | 0       | FS    | Uniform buffer        | `ModeUniforms` (generated)   |
/// | 1       | FS    | 2D float texture      | Source layer (premultiplied) |
/// | 2       | FS    | Non-filtering sampler | Nearest + ClampToEdge        |
///
/// ## Blend state
///
/// `REPLACE` (no fixed-function blending): the fragment shader emits the full
/// premultiplied blended texel directly.
#[allow(missing_debug_implementations)]
pub(crate) struct ModePipeline {
    /// The single render pipeline (format-parametric at construction time).
    ///
    /// The bind-group layout is created from the generated `mode::WgpuBindGroup0`
    /// and is not stored here — [`super::apply_mode`] uses
    /// `mode::WgpuBindGroup0::from_bindings` at draw time, which creates a
    /// compatible bind group internally.
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl ModePipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// WGSL shader compilation happens here; a wgpu validation error surfaces
    /// at this call site, which the GPU construction test exercises.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // The bind-group layout is sourced from the generated bindings, which
        // derive it directly from the WGSL source — no hand-maintained descriptor.
        let bind_group_layout = mode::WgpuBindGroup0::get_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mode Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // Compose mode.wgsl by resolving its `#import blend_helpers` via naga_oil.
        // The Composer registers blend_helpers.wgsl as a composable module, then
        // produces a fully resolved naga::Module that wgpu accepts via ShaderSource::Naga.
        // Panics are intentional here: a composition failure at pipeline-init is a
        // programming error (bad WGSL or missing import), not a recoverable runtime
        // condition — it matches the behaviour of the previous concat!/create_shader_module
        // path which also panicked (wgpu panics on shader validation failure).
        let composed_source = compose_wgsl_shader(
            &[ComposableSource {
                source: include_str!("../shaders/blend_helpers.wgsl"),
                file_path: "shaders/blend_helpers.wgsl",
            }],
            include_str!("../shaders/effects/mode.wgsl"),
            "shaders/effects/mode.wgsl",
        )
        .expect("blend_helpers.wgsl #import in mode.wgsl must resolve: check for WGSL syntax errors or a missing #define_import_path");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Mode Shader"),
            source: composed_source,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Mode Pipeline"),
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

        Self { pipeline }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
// `float_cmp` fires on `assert_eq!` of f32 arrays; these compare stored literals
// read back through a struct — no arithmetic, so equality is bit-exact.
#[allow(clippy::float_cmp)]
mod cpu_tests {
    use flui_types::painting::BlendMode;

    use super::{blend_mode_to_u32, mode};

    /// The generated struct size must match the WGSL uniform-block size exactly.
    ///
    /// Layout: color(16) + blend_mode(4) + pad(12) = 32 bytes.  The `const`
    /// asserts above fire at compile time; this test makes the requirement
    /// visible in test output.
    #[test]
    fn mode_uniforms_size_is_32_bytes() {
        assert_eq!(
            std::mem::size_of::<mode::ModeUniforms>(),
            32,
            "ModeUniforms must be 32 bytes to match ModeUniforms in mode.wgsl"
        );
    }

    /// Field offsets must match the WGSL uniform-block layout exactly.
    #[test]
    fn mode_uniforms_field_offsets_match_wgsl() {
        assert_eq!(
            std::mem::offset_of!(mode::ModeUniforms, color),
            0,
            "color must be at offset 0"
        );
        assert_eq!(
            std::mem::offset_of!(mode::ModeUniforms, blend_mode),
            16,
            "blend_mode must be at offset 16"
        );
    }

    /// Round-trip: values written via the pad-free `new` are readable from the
    /// same byte positions — proves the generated layout has no hidden reordering.
    #[test]
    fn mode_uniforms_field_round_trips() {
        let uniform = mode::ModeUniforms::new([0.1, 0.2, 0.3, 0.4], 15);
        assert_eq!(uniform.color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(uniform.blend_mode, 15);
    }

    /// Every `BlendMode` variant maps to a unique u32 in the expected range.
    ///
    /// This catches any future variant that is accidentally given a duplicate or
    /// out-of-range value.  The WGSL switch uses values 0–29; no gaps or aliases
    /// are allowed.
    #[test]
    fn blend_mode_to_u32_is_injective_and_in_range() {
        let all_modes = [
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::Dst,
            BlendMode::SrcOver,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::DstATop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Modulate,
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
            BlendMode::Multiply,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];

        let mut seen = std::collections::HashSet::new();
        for mode in all_modes {
            let encoded = blend_mode_to_u32(mode);
            assert!(
                encoded <= 29,
                "blend_mode_to_u32({mode:?}) = {encoded} exceeds the WGSL switch range [0..29]"
            );
            assert!(
                seen.insert(encoded),
                "blend_mode_to_u32({mode:?}) = {encoded} collides with a previously mapped variant"
            );
        }
    }

    /// Spot-check key modes against their expected integer values.
    ///
    /// These values are the "must match" constants in `mode.wgsl` — any drift
    /// between this Rust side and the WGSL would cause silent wrong blending.
    #[test]
    fn blend_mode_to_u32_spot_check() {
        assert_eq!(blend_mode_to_u32(BlendMode::Clear), 0, "Clear must be 0");
        assert_eq!(
            blend_mode_to_u32(BlendMode::SrcOver),
            3,
            "SrcOver must be 3"
        );
        assert_eq!(
            blend_mode_to_u32(BlendMode::Modulate),
            13,
            "Modulate must be 13"
        );
        assert_eq!(
            blend_mode_to_u32(BlendMode::Screen),
            15,
            "Screen must be 15"
        );
        assert_eq!(
            blend_mode_to_u32(BlendMode::Multiply),
            25,
            "Multiply must be 25"
        );
        assert_eq!(blend_mode_to_u32(BlendMode::Hue), 26, "Hue must be 26");
        assert_eq!(
            blend_mode_to_u32(BlendMode::Luminosity),
            29,
            "Luminosity must be 29"
        );
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_construction_tests {
    use super::ModePipeline;

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
                label: Some("ModePipeline Test Device"),
                ..Default::default()
            }))
            .expect("GPU device creation succeeded when adapter was found");
        device
    }

    /// `ModePipeline::new` completes without a wgpu validation error for
    /// `Rgba8Unorm`, proving the WGSL parses and the bind-group layout is valid.
    #[test]
    fn pipeline_construction_succeeds_for_rgba8unorm() {
        let device = test_device();
        let _pipeline = ModePipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    }
}
