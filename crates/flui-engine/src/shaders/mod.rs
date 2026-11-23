//! WGSL shader source bindings for the wgpu backend.
//!
//! A shader appears here when its source is ASSEMBLED rather than merely read:
//! every entry below is a concatenation whose piece order is load-bearing, and
//! the assembly is what earns the indirection. Shaders that are a single file
//! (`effects/*.wgsl`, the remaining `common/*.wgsl`) are included by their
//! consumers through [`wgsl_source!`], including `pipelines/effects.rs` for
//! the shadow pipeline.
//!
//! No pipeline includes a file of `src/shaders/` itself: the text a pipeline
//! hands to the GPU API is linked into every application, so `build.rs` writes
//! each file without its comments, indentation and blank lines, and
//! [`wgsl_source!`] includes that copy.
//!
//! A shader that imports a shared module (`import package::color_filter::…;`)
//! is not read from here at all: `build.rs` resolves it with WESL and its
//! consumer includes the result through [`composed_wgsl!`].

/// The code of `src/shaders/$path` (`"effects/blit.wgsl"`), as `build.rs`
/// strips it of comments, indentation and blank lines, as a `&'static str`
/// literal, so it also nests in `concat!`.
macro_rules! wgsl_source {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/wgsl/", $path))
    };
}
pub(crate) use wgsl_source;

/// The plain WGSL `build.rs` composed from the WESL module at `$path` under
/// `src/shaders/` (`"effects/mode"` is `effects/mode.wgsl`), as a `&'static str`
/// literal, so it also nests in `concat!`.
///
/// Only the modules `build.rs` lists in `WESL_ENTRIES` exist; any other path
/// fails to compile with a missing-file error.
macro_rules! composed_wgsl {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/shaders/", $path, ".wgsl"))
    };
}
pub(crate) use composed_wgsl;

// Coverage-correct assemblies
//
// Every shader whose blend mode is chosen per draw comes in TWO assemblies
// differing only in the fragment entry point, because a device without
// `wgpu::Features::DUAL_SOURCE_BLENDING` cannot compile the second one at all:
// naga rejects `@blend_src` without the matching validator capability. The
// owning pipeline cache picks between them per device and per blend mode —
// `PipelineVariants` for the tessellated shape, `GradientPipelines` for the three
// gradients.
//
// The pieces are the same in every assembly and in the same order: the clip
// block, the `ShadedFragment` contract, the module's own vertex stage and
// `shadeFragment`, and finally one of the three entry points. The module
// supplies `VertexOutput`, `shadeFragment` and `vertexStage`, a plain function
// that both `vs_main` and `vs_isolation` call (WGSL forbids calling an entry
// point); `common/coverage.wgsl` and the entry-point files supply everything
// else.

/// Assemble the three variants of one coverage-correct shader from the module's
/// own source.
///
/// A macro rather than four hand-written `concat!` pairs because the order of
/// the pieces is load-bearing in one non-obvious way — the `enable` directive
/// must precede every declaration in the module, and the clip block that would
/// otherwise come first opens with a `const` — and twelve hand-written orderings
/// are twelve chances to get it wrong once.
macro_rules! coverage_correct_shader {
    // A gradient: the module, after the stops it reads (`gradients/stops.wgsl`).
    (gradient $module:literal, $vertex:literal) => {
        crate::pipelines::variants::CoverageShaderSources {
            folded: concat!(
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!("gradients/stops.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_folded.wgsl"),
            ),
            isolation: concat!(
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!("gradients/stops.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_isolation.wgsl"),
                $vertex,
            ),
            second_source: concat!(
                "enable dual_source_blending;\n",
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!("gradients/stops.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_second_source.wgsl"),
            ),
        }
    };
    ($module:literal, $vertex:literal) => {
        crate::pipelines::variants::CoverageShaderSources {
            folded: concat!(
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_folded.wgsl"),
            ),
            isolation: concat!(
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_isolation.wgsl"),
                $vertex,
            ),
            second_source: concat!(
                "enable dual_source_blending;\n",
                wgsl_source!("common/clip.wgsl"),
                wgsl_source!("common/coverage.wgsl"),
                wgsl_source!($module),
                wgsl_source!("common/fragment_second_source.wgsl"),
            ),
        }
    };
}

/// Tessellated shape shader — folded, dual-source and isolation assemblies.
///
/// Tessellated geometry has no instances to hang a clip slot on, so its clip
/// arrives in a per-batch uniform rather than per instance; the clip block is
/// the same either way.
pub(crate) const SHAPE: crate::pipelines::variants::CoverageShaderSources = coverage_correct_shader!(
    "shape.wgsl",
    "@vertex fn vs_isolation(input: VertexInput) -> VertexOutput { return isolateVertex(vertexStage(input)); }\n"
);

/// Instanced linear gradient shader — folded, dual-source and isolation assemblies.
pub(crate) const LINEAR_GRADIENT: crate::pipelines::variants::CoverageShaderSources = coverage_correct_shader!(
    gradient "gradients/linear.wgsl",
    "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
);

/// Instanced radial gradient shader — folded, dual-source and isolation assemblies.
pub(crate) const RADIAL_GRADIENT: crate::pipelines::variants::CoverageShaderSources = coverage_correct_shader!(
    gradient "gradients/radial.wgsl",
    "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
);

/// Instanced sweep gradient shader — folded, dual-source and isolation assemblies.
pub(crate) const SWEEP_GRADIENT: crate::pipelines::variants::CoverageShaderSources = coverage_correct_shader!(
    gradient "gradients/sweep.wgsl",
    "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
);

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
// Arcs also consume the final clip mask through the shared viewport group.
//
// `concat!` takes the `include_str!` calls directly rather than a named
// `CLIP_SDF` const: it concatenates literals, and a `const` is not one.

/// Instanced rectangle rendering shader.
pub(crate) const RECT_INSTANCED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    wgsl_source!("rect_instanced.wgsl")
);
/// Instanced circle rendering shader.
pub(crate) const CIRCLE_INSTANCED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    wgsl_source!("circle_instanced.wgsl")
);
/// Instanced arc rendering shader.
pub(crate) const ARC_INSTANCED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    wgsl_source!("arc_instanced.wgsl")
);
/// Analytical shadow shader with shared final clip coverage.
pub(crate) const SHADOW: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    wgsl_source!("effects/shadow.wgsl")
);
/// Instanced texture rendering shader.
pub(crate) const TEXTURE_INSTANCED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    "fn filterSourceTexel(t: vec4<f32>) -> vec4<f32> { return vec4<f32>(t.rgb * t.a, t.a); }\n",
    wgsl_source!("texture_instanced.wgsl"),
    "\n@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> { return folded_image(input); }\n"
);
/// Image color and independent clip coverage over a cropped attachment.
pub(crate) const TEXTURE_ISOLATION: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    "fn filterSourceTexel(t: vec4<f32>) -> vec4<f32> { return vec4<f32>(t.rgb * t.a, t.a); }\n",
    wgsl_source!("texture_instanced.wgsl"),
    wgsl_source!("texture_isolation.wgsl")
);
/// Decoded source filtering preserves floating channels until composition.
pub(crate) const TEXTURE_FILTERED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    composed_wgsl!("image_source_filter"),
    wgsl_source!("texture_instanced.wgsl"),
    "\n@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> { return folded_image(input); }\n"
);
pub(crate) const TEXTURE_FILTERED_ISOLATION: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    composed_wgsl!("image_source_filter"),
    wgsl_source!("texture_instanced.wgsl"),
    wgsl_source!("texture_isolation.wgsl")
);
/// Instanced glyph rendering shader (the glyph atlas' consumer).
pub(crate) const GLYPH_INSTANCED: &str = concat!(
    wgsl_source!("common/clip.wgsl"),
    wgsl_source!("glyph_instanced.wgsl")
);

#[cfg(test)]
mod tests {
    use super::{
        ARC_INSTANCED, CIRCLE_INSTANCED, GLYPH_INSTANCED, LINEAR_GRADIENT, RADIAL_GRADIENT,
        RECT_INSTANCED, SHADOW, SHAPE, SWEEP_GRADIENT, TEXTURE_FILTERED,
        TEXTURE_FILTERED_ISOLATION, TEXTURE_INSTANCED, TEXTURE_ISOLATION,
    };

    /// Every shader text a pipeline includes, by the name a failure reports.
    fn included_sources() -> Vec<(&'static str, &'static str)> {
        let mut sources = vec![
            ("rect_instanced", RECT_INSTANCED),
            ("circle_instanced", CIRCLE_INSTANCED),
            ("arc_instanced", ARC_INSTANCED),
            ("shadow", SHADOW),
            ("texture_instanced", TEXTURE_INSTANCED),
            ("texture_isolation", TEXTURE_ISOLATION),
            ("texture_filtered", TEXTURE_FILTERED),
            ("texture_filtered_isolation", TEXTURE_FILTERED_ISOLATION),
            ("glyph_instanced", GLYPH_INSTANCED),
            ("clip_mask", wgsl_source!("clip_mask.wgsl")),
            ("portable_coverage", wgsl_source!("portable_coverage.wgsl")),
            ("effects/blit", wgsl_source!("effects/blit.wgsl")),
            ("effects/blur", wgsl_source!("effects/blur.wgsl")),
            (
                "effects/box_downsample",
                wgsl_source!("effects/box_downsample.wgsl"),
            ),
            (
                "effects/morphology",
                wgsl_source!("effects/morphology.wgsl"),
            ),
            (
                "effects/present_transform",
                wgsl_source!("effects/present_transform.wgsl"),
            ),
            ("effects/resample", wgsl_source!("effects/resample.wgsl")),
            ("advanced_blend", composed_wgsl!("advanced_blend")),
            (
                "effects/color_matrix",
                composed_wgsl!("effects/color_matrix"),
            ),
            ("effects/gamma", composed_wgsl!("effects/gamma")),
            ("effects/mode", composed_wgsl!("effects/mode")),
        ];
        for (name, coverage) in [
            ("shape", SHAPE),
            ("gradients/linear", LINEAR_GRADIENT),
            ("gradients/radial", RADIAL_GRADIENT),
            ("gradients/sweep", SWEEP_GRADIENT),
        ] {
            sources.push((name, coverage.folded));
            sources.push((name, coverage.second_source));
            sources.push((name, coverage.isolation));
        }
        sources
    }

    /// The shader text is linked into every application, so it carries code
    /// and nothing for a reader: `build.rs` strips each source and each
    /// composed shader before a pipeline includes it.
    #[test]
    fn a_pipeline_includes_shader_code_without_comments_or_indentation() {
        for (name, source) in included_sources() {
            for line in source.lines() {
                assert!(!line.contains("//"), "{name} keeps a comment: {line:?}");
                assert_eq!(line, line.trim(), "{name} keeps a line's indentation");
            }
        }
    }

    /// Stripping removes nothing a shader needs: each text still parses, on a
    /// host with no GPU to compile it.
    #[test]
    fn every_included_shader_parses_as_wgsl() {
        for (name, source) in included_sources() {
            if let Err(error) = wgpu::naga::front::wgsl::parse_str(source) {
                panic!("{name} does not parse:\n{}", error.emit_to_string(source));
            }
        }
    }
}
