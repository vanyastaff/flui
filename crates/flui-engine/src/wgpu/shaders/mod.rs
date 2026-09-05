//! WGSL shader source bindings for the wgpu backend.
//!
//! A shader appears here when its source is ASSEMBLED rather than merely read:
//! every entry below is a concatenation whose piece order is load-bearing, and
//! the assembly is what earns the indirection. Shaders that are a single file
//! (`masks/*.wgsl`, `effects/*.wgsl`, the remaining `common/*.wgsl`) stay
//! `include_str!`-loaded by their consumers — `shader_compiler.rs` for the
//! mask/blur/morph stack, `effects_pipeline.rs` for the shadow pipeline.

// Coverage-correct assemblies
//
// Every shader whose blend mode is chosen per draw comes in TWO assemblies
// differing only in the fragment entry point, because a device without
// `wgpu::Features::DUAL_SOURCE_BLENDING` cannot compile the second one at all:
// naga rejects `@blend_src` without the matching validator capability. The
// owning pipeline cache picks between them per device and per blend mode —
// `PipelineCache` for the tessellated shape, `GradientPipelines` for the three
// gradients.
//
// The pieces are the same in every assembly and in the same order: the clip
// block, the `ShadedFragment` contract, the module's own vertex stage and
// `shadeFragment`, and finally one of the two entry points. The module
// supplies `VertexOutput` and `shadeFragment`; `common/coverage.wgsl` and the
// entry-point files supply everything else.

/// Assemble the two variants of one coverage-correct shader from the module's
/// own source.
///
/// A macro rather than four hand-written `concat!` pairs because the order of
/// the pieces is load-bearing in one non-obvious way — the `enable` directive
/// must precede every declaration in the module, and the clip block that would
/// otherwise come first opens with a `const` — and eight hand-written orderings
/// are eight chances to get it wrong once.
macro_rules! coverage_correct_shader {
    ($module:literal) => {
        super::pipeline::CoverageShaderSources {
            folded: concat!(
                include_str!("common/clip.wgsl"),
                include_str!("common/coverage.wgsl"),
                include_str!($module),
                include_str!("common/fragment_folded.wgsl"),
            ),
            second_source: concat!(
                "enable dual_source_blending;\n",
                include_str!("common/clip.wgsl"),
                include_str!("common/coverage.wgsl"),
                include_str!($module),
                include_str!("common/fragment_second_source.wgsl"),
            ),
        }
    };
}

/// Tessellated shape shader — both assemblies.
///
/// Tessellated geometry has no instances to hang a clip slot on, so its clip
/// arrives in a per-batch uniform rather than per instance; the clip block is
/// the same either way.
pub const SHAPE: super::pipeline::CoverageShaderSources = coverage_correct_shader!("shape.wgsl");

/// Instanced linear gradient shader — both assemblies.
pub const LINEAR_GRADIENT: super::pipeline::CoverageShaderSources =
    coverage_correct_shader!("gradients/linear.wgsl");

/// Instanced radial gradient shader — both assemblies.
pub const RADIAL_GRADIENT: super::pipeline::CoverageShaderSources =
    coverage_correct_shader!("gradients/radial.wgsl");

/// Instanced sweep gradient shader — both assemblies.
pub const SWEEP_GRADIENT: super::pipeline::CoverageShaderSources =
    coverage_correct_shader!("gradients/sweep.wgsl");

// Instanced rendering
//
// Every shader that evaluates a per-instance SDF clip is prepended with
// `common/clip.wgsl`. WGSL has no include directive, but module-scope
// declarations are order-independent, so plain concatenation is the whole
// mechanism. These three shaders each carried a byte-for-byte copy of the
// clip helpers; a distance function copied per shader drifts silently, and
// the same `ClipRRect` would then round different primitives by different
// amounts with nothing failing.
//
// `arc_instanced` is NOT prepended: `ArcInstance` carries no clip slot, and
// adding unused functions to a shader is noise, not symmetry.
//
// `concat!` takes the `include_str!` calls directly rather than a named
// `CLIP_SDF` const: it concatenates literals, and a `const` is not one.

/// Instanced rectangle rendering shader.
pub const RECT_INSTANCED: &str = concat!(
    include_str!("common/clip.wgsl"),
    include_str!("rect_instanced.wgsl")
);
/// Instanced circle rendering shader.
pub const CIRCLE_INSTANCED: &str = concat!(
    include_str!("common/clip.wgsl"),
    include_str!("circle_instanced.wgsl")
);
/// Instanced arc rendering shader.
pub const ARC_INSTANCED: &str = include_str!("arc_instanced.wgsl");
/// Instanced texture rendering shader.
pub const TEXTURE_INSTANCED: &str = concat!(
    include_str!("common/clip.wgsl"),
    include_str!("texture_instanced.wgsl")
);
