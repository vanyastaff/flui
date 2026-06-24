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

use flui_types::painting::BlendMode;

use super::generated::advanced_blend;
use crate::wgpu::shader_composer::{ComposableSource, compose_wgsl_shader};

// ── Byte-identity gate (generated `BlendUniforms` vs the WGSL block) ──────────

// These const assertions are the GO/NO-GO check.  If wgsl_bindgen generates a
// struct whose layout differs from the former hand-written `BlendUniformData`,
// this file fails to compile — the correct signal to stop and report BLOCKED.
//
// Generated `BlendUniforms` must match `BlendUniforms` in `advanced_blend.wgsl`
// and the former hand-written struct byte-for-byte.  WGSL §13.4.1 alignment:
// `vec4`→16, `vec2`→8, `f32`/`u32`→4, `vec3`→16 (rounded up); struct size is a
// multiple of 16.
//
// | Byte offset | Size | Field           | WGSL member            |
// |-------------|------|-----------------|------------------------|
// | 0           | 16   | `op_bounds`     | `vec4<f32>`            |
// | 16          | 8    | `viewport_size` | `vec2<f32>`            |
// | 24          | 8    | `copy_origin`   | `vec2<f32>`            |
// | 32          | 8    | `copy_extent`   | `vec2<f32>`            |
// | 40          | 4    | `opacity`       | `f32`                  |
// | 44          | 4    | `_pad0`         | `f32` (align gap)      |
// | 48          | 12   | `tint_rgb`      | `vec3<f32>` (align 16) |
// | 60          | 4    | `mode`          | `u32`                  |
// | 64          | 8    | `src_uv_min`    | `vec2<f32>`            |
// | 72          | 8    | `src_uv_max`    | `vec2<f32>`            |
//
// Total = 80 bytes (multiple of 16).

/// The generated `BlendUniforms` must be exactly 80 bytes.
const _GENERATED_BLEND_UNIFORMS_SIZE_CHECK: () = {
    assert!(std::mem::size_of::<advanced_blend::BlendUniforms>() == 80);
};

/// Field offsets must match the WGSL uniform-block layout exactly.  The
/// `tint_rgb` (offset 48) / `mode` (offset 60) pair is the tight `vec3`+`u32`
/// pack that a wrong type map (e.g. `Vec3A`) would break.
const _GENERATED_BLEND_FIELD_OFFSET_CHECKS: () = {
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, op_bounds) == 0);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, viewport_size) == 16);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, copy_origin) == 24);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, copy_extent) == 32);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, opacity) == 40);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, tint_rgb) == 48);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, mode) == 60);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, src_uv_min) == 64);
    assert!(std::mem::offset_of!(advanced_blend::BlendUniforms, src_uv_max) == 72);
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
/// | 0       | VS + FS  | Uniform buffer          | `BlendUniforms` (generated)  |
/// | 1       | FS       | 2D float texture        | Foreground (premultiplied)   |
/// | 2       | FS       | 2D float texture        | Backdrop copy (premultiplied)|
/// | 3       | FS       | Non-filtering sampler   | Nearest + ClampToEdge        |
///
/// The `Stage` column is the actual shader usage.  The generated
/// `WgpuBindGroup0` layout marks every binding `VERTEX_FRAGMENT` (wgsl_bindgen
/// does not derive per-stage visibility); this is wider than the former
/// hand-written descriptor (bindings 1–3 were FRAGMENT-only) but permissive —
/// wgpu accepts it and the GPU readback suite confirms bit-identical output.
///
/// ## Blend state
///
/// `REPLACE` (no fixed-function blending): the fragment shader emits the full
/// premultiplied composite directly; the GPU hardware must not re-blend it.
pub(crate) struct AdvancedBlendPipeline {
    /// The single render pipeline (format-parametric at construction time).
    ///
    /// The bind-group layout is created from the generated
    /// `advanced_blend::WgpuBindGroup0` and is not stored here —
    /// [`super::flush_advanced_layer`] uses
    /// `advanced_blend::WgpuBindGroup0::from_bindings` at draw time, which
    /// creates a compatible bind group internally.
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl AdvancedBlendPipeline {
    /// Build the pipeline for `surface_format`.
    ///
    /// Compilation of the WGSL shader module happens here; a wgpu validation
    /// error surfaces at this call site, which the synthetic-op GPU test
    /// exercises before any production caller exists.
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // The bind-group layout is sourced from the generated bindings, which
        // derive it directly from the WGSL source — no hand-maintained descriptor.
        let bind_group_layout = advanced_blend::WgpuBindGroup0::get_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Advanced Blend Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // Compose advanced_blend.wgsl by resolving its `#import blend_helpers` via naga_oil.
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
            include_str!("../shaders/advanced_blend.wgsl"),
            "shaders/advanced_blend.wgsl",
        )
        .expect("blend_helpers.wgsl #import in advanced_blend.wgsl must resolve: check for WGSL syntax errors or a missing #define_import_path");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Advanced Blend Shader"),
            source: composed_source,
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

        Self { pipeline }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod cpu_tests {
    use std::collections::HashSet;

    use flui_types::painting::BlendMode;

    use super::{advanced_blend, mode_to_u32};

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

    /// The generated struct size must match the WGSL uniform-block size exactly.
    ///
    /// Layout: op_bounds(16) + viewport_size(8) + copy_origin(8) +
    /// copy_extent(8) + opacity(4) + _pad0(4) + tint_rgb(12) + mode(4) +
    /// src_uv_min(8) + src_uv_max(8) = 80 bytes.
    ///
    /// The `const` asserts in this file fire at compile time; this test makes the
    /// requirement observable in test output.
    #[test]
    fn blend_uniforms_size_is_80_bytes() {
        assert_eq!(
            std::mem::size_of::<advanced_blend::BlendUniforms>(),
            80,
            "BlendUniforms must be 80 bytes to match BlendUniforms in advanced_blend.wgsl"
        );
    }

    /// Field offsets must match the WGSL uniform-block layout exactly — in
    /// particular the `tint_rgb`(48)/`mode`(60) tight `vec3`+`u32` pack.
    #[test]
    fn blend_uniforms_field_offsets_match_wgsl() {
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, op_bounds),
            0
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, viewport_size),
            16
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, copy_origin),
            24
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, copy_extent),
            32
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, opacity),
            40
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, tint_rgb),
            48
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, mode),
            60
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, src_uv_min),
            64
        );
        assert_eq!(
            std::mem::offset_of!(advanced_blend::BlendUniforms, src_uv_max),
            72
        );
    }

    /// Round-trip: values written via the pad-free `new` are readable from the
    /// same byte positions — proves the generated layout has no hidden reordering.
    #[allow(
        clippy::float_cmp,
        reason = "comparing floats just assigned from exact literals — bit identity is the invariant"
    )]
    #[test]
    fn blend_uniforms_field_round_trips() {
        let uniform = advanced_blend::BlendUniforms::new(
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0],
            [7.0, 8.0],
            [9.0, 10.0],
            0.5,
            [0.1, 0.2, 0.3],
            7,
            [0.0, 0.0],
            [1.0, 1.0],
        );
        assert_eq!(uniform.op_bounds, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(uniform.viewport_size, [5.0, 6.0]);
        assert_eq!(uniform.copy_origin, [7.0, 8.0]);
        assert_eq!(uniform.copy_extent, [9.0, 10.0]);
        assert_eq!(uniform.opacity, 0.5);
        assert_eq!(uniform.tint_rgb, [0.1, 0.2, 0.3]);
        assert_eq!(uniform.mode, 7);
        assert_eq!(uniform.src_uv_min, [0.0, 0.0]);
        assert_eq!(uniform.src_uv_max, [1.0, 1.0]);
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
