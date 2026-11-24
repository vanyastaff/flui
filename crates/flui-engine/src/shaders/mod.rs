//! WGSL shader text for the wgpu backend.
//!
//! Every shader a pipeline compiles is a constant of this module, and nothing
//! outside it includes a shader file. The text a pipeline hands to the GPU API
//! is linked into every application, so `build.rs` writes each file of
//! `src/shaders/` without its comments, indentation and blank lines, and the
//! two macros below, which stay private to this module, include those copies.
//!
//! A constant is one file, an assembly of several whose order is load-bearing,
//! or a module that imports a shared one (`import package::color_filter::…;`),
//! which `build.rs` resolves with WESL first. They are declared through
//! `shader_texts!` and `coverage_shaders!`, which list them for the tests at
//! the end of this file: a shader added here is checked without being added
//! to a list.

#[cfg(test)]
mod strip;

use crate::pipelines::variants::CoverageShaderSources;

/// The code of `src/shaders/$path` (`"effects/blit.wgsl"`), as `build.rs`
/// strips it of comments, indentation and blank lines, as a `&'static str`
/// literal, so it also nests in `concat!`.
macro_rules! wgsl_source {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/wgsl/", $path))
    };
}

/// The plain WGSL `build.rs` composed from the WESL module at `$path` under
/// `src/shaders/` (`"effects/mode"` is `effects/mode.wgsl`), stripped the same
/// way, as a `&'static str` literal, so it also nests in `concat!`.
///
/// Only the modules `build.rs` lists in `WESL_ENTRIES` exist; any other path
/// fails to compile with a missing-file error.
macro_rules! composed_wgsl {
    ($path:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/shaders/", $path, ".wgsl"))
    };
}

/// Declares each shader text as a `pub(crate) const` and lists all of them, by
/// name, in `TEXTS` for the tests.
macro_rules! shader_texts {
    ($($(#[$attribute:meta])* $name:ident = $text:expr;)+) => {
        $($(#[$attribute])* pub(crate) const $name: &str = $text;)+

        /// Every shader text above, by its constant's name.
        #[cfg(test)]
        const TEXTS: &[(&str, &str)] = &[$((stringify!($name), $name)),+];
    };
}

/// Declares each coverage-correct shader as a `pub(crate) const` and lists all
/// of them, by name, in `COVERAGE_SHADERS` for the tests.
macro_rules! coverage_shaders {
    ($($(#[$attribute:meta])* $name:ident = $sources:expr;)+) => {
        $($(#[$attribute])* pub(crate) const $name: CoverageShaderSources = $sources;)+

        /// Every coverage-correct shader above, by its constant's name.
        #[cfg(test)]
        const COVERAGE_SHADERS: &[(&str, CoverageShaderSources)] =
            &[$((stringify!($name), $name)),+];
    };
}

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

coverage_shaders! {
    /// Tessellated shape shader — folded, dual-source and isolation assemblies.
    ///
    /// Tessellated geometry has no instances to hang a clip slot on, so its clip
    /// arrives in a per-batch uniform rather than per instance; the clip block is
    /// the same either way.
    SHAPE = coverage_correct_shader!(
        "shape.wgsl",
        "@vertex fn vs_isolation(input: VertexInput) -> VertexOutput { return isolateVertex(vertexStage(input)); }\n"
    );

    /// Instanced linear gradient shader — folded, dual-source and isolation assemblies.
    LINEAR_GRADIENT = coverage_correct_shader!(
        gradient "gradients/linear.wgsl",
        "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
    );

    /// Instanced radial gradient shader — folded, dual-source and isolation assemblies.
    RADIAL_GRADIENT = coverage_correct_shader!(
        gradient "gradients/radial.wgsl",
        "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
    );

    /// Instanced sweep gradient shader — folded, dual-source and isolation assemblies.
    SWEEP_GRADIENT = coverage_correct_shader!(
        gradient "gradients/sweep.wgsl",
        "@vertex fn vs_isolation(input: VertexInput, instance: InstanceInput) -> VertexOutput { return isolateVertex(vertexStage(input, instance)); }\n"
    );
}

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
// `concat!` takes the `wgsl_source!` calls directly rather than a named
// `CLIP_SDF` const: it concatenates literals, which a macro call expands to
// and a `const` is not.

shader_texts! {
    /// Instanced rectangle rendering shader.
    RECT_INSTANCED = concat!(
        wgsl_source!("common/clip.wgsl"),
        wgsl_source!("rect_instanced.wgsl")
    );
    /// Instanced circle rendering shader.
    CIRCLE_INSTANCED = concat!(
        wgsl_source!("common/clip.wgsl"),
        wgsl_source!("circle_instanced.wgsl")
    );
    /// Instanced arc rendering shader.
    ARC_INSTANCED = concat!(
        wgsl_source!("common/clip.wgsl"),
        wgsl_source!("arc_instanced.wgsl")
    );
    /// Analytical shadow shader with shared final clip coverage.
    SHADOW = concat!(
        wgsl_source!("common/clip.wgsl"),
        wgsl_source!("effects/shadow.wgsl")
    );
    /// Instanced texture rendering shader.
    TEXTURE_INSTANCED = concat!(
        wgsl_source!("common/clip.wgsl"),
        "fn filterSourceTexel(t: vec4<f32>) -> vec4<f32> { return vec4<f32>(t.rgb * t.a, t.a); }\n",
        wgsl_source!("texture_instanced.wgsl"),
        "@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> { return folded_image(input); }\n"
    );
    /// Image color and independent clip coverage over a cropped attachment.
    TEXTURE_ISOLATION = concat!(
        wgsl_source!("common/clip.wgsl"),
        "fn filterSourceTexel(t: vec4<f32>) -> vec4<f32> { return vec4<f32>(t.rgb * t.a, t.a); }\n",
        wgsl_source!("texture_instanced.wgsl"),
        wgsl_source!("texture_isolation.wgsl")
    );
    /// Decoded source filtering preserves floating channels until composition.
    TEXTURE_FILTERED = concat!(
        wgsl_source!("common/clip.wgsl"),
        composed_wgsl!("image_source_filter"),
        wgsl_source!("texture_instanced.wgsl"),
        "@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> { return folded_image(input); }\n"
    );
    /// [`TEXTURE_FILTERED`] over a cropped attachment, as [`TEXTURE_ISOLATION`].
    TEXTURE_FILTERED_ISOLATION = concat!(
        wgsl_source!("common/clip.wgsl"),
        composed_wgsl!("image_source_filter"),
        wgsl_source!("texture_instanced.wgsl"),
        wgsl_source!("texture_isolation.wgsl")
    );
    /// Instanced glyph rendering shader (the glyph atlas' consumer).
    GLYPH_INSTANCED = concat!(
        wgsl_source!("common/clip.wgsl"),
        wgsl_source!("glyph_instanced.wgsl")
    );

    // Single files and composed modules, each compiled by one pipeline.

    /// The clip mask's shader (`replay::clip_mask`).
    CLIP_MASK = wgsl_source!("clip_mask.wgsl");
    /// The portable coverage composite (`replay::portable_coverage`).
    PORTABLE_COVERAGE = wgsl_source!("portable_coverage.wgsl");
    /// Box-filter downsample of the supersampled target (`replay::ssaa`).
    BOX_DOWNSAMPLE = wgsl_source!("effects/box_downsample.wgsl");
    /// Fullscreen blit of an intermediate texture onto the surface (`offscreen::blit`).
    BLIT = wgsl_source!("effects/blit.wgsl");
    /// Presentation blit with conversion to the surface's gamut and transfer
    /// function (`offscreen::transform_blit`).
    PRESENT_TRANSFORM = wgsl_source!("effects/present_transform.wgsl");
    /// Separable Gaussian blur (`filters::blur`).
    BLUR = wgsl_source!("effects/blur.wgsl");
    /// Separable dilate or erode (`filters::morphology`).
    MORPHOLOGY = wgsl_source!("effects/morphology.wgsl");
    /// A source resampled through an affine map (`filters::resample`).
    RESAMPLE = wgsl_source!("effects/resample.wgsl");
    /// Advanced blend-mode composite (`filters::advanced_blend`).
    ADVANCED_BLEND = composed_wgsl!("advanced_blend");
    /// Per-pixel 5×4 colour-matrix filter (`filters::color_matrix`).
    COLOR_MATRIX = composed_wgsl!("effects/color_matrix");
    /// Per-pixel sRGB gamma transfer function (`filters::gamma`).
    GAMMA = composed_wgsl!("effects/gamma");
    /// Per-pixel `ColorFilter::Mode` (`filters::mode`).
    MODE = composed_wgsl!("effects/mode");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{COVERAGE_SHADERS, TEXTS};

    /// Every text a pipeline can compile, by the name a failure reports.
    fn shader_texts() -> Vec<(String, &'static str)> {
        let mut texts: Vec<(String, &'static str)> = TEXTS
            .iter()
            .map(|(name, text)| ((*name).to_owned(), *text))
            .collect();
        for (name, sources) in COVERAGE_SHADERS {
            texts.push((format!("{name}.folded"), sources.folded));
            texts.push((format!("{name}.second_source"), sources.second_source));
            texts.push((format!("{name}.isolation"), sources.isolation));
        }
        texts
    }

    /// The shader text is linked into every application, so it carries code
    /// and nothing for a reader: `build.rs` strips each source and each
    /// composed shader, and the pieces an assembly adds are written the same
    /// way.
    #[test]
    fn a_pipeline_compiles_shader_code_without_comments_indentation_or_blank_lines() {
        for (name, text) in shader_texts() {
            for line in text.lines() {
                assert!(!line.contains("//"), "{name} keeps a comment: {line:?}");
                assert_eq!(line, line.trim(), "{name} keeps a line's indentation");
                assert!(!line.is_empty(), "{name} keeps a blank line");
            }
        }
    }

    /// Stripping removes nothing a shader needs: each text still parses, on a
    /// host with no GPU to compile it.
    #[test]
    fn every_shader_text_parses_as_wgsl() {
        for (name, text) in shader_texts() {
            if let Err(error) = wgpu::naga::front::wgsl::parse_str(text) {
                panic!("{name} does not parse:\n{}", error.emit_to_string(text));
            }
        }
    }

    /// The constants above are the only way to a shader file: a Rust source
    /// outside this module that names one includes it with its comments, past
    /// both tests above.
    #[test]
    fn no_source_outside_this_module_names_a_shader_file() {
        fn rust_sources(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(directory).expect("a source directory") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    rust_sources(&path, found);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    found.push(path);
                }
            }
        }

        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let this_module = source_root.join("shaders").join("mod.rs");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        assert!(
            sources.contains(&this_module),
            "the walk reaches this module"
        );
        for path in sources {
            if path == this_module {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a UTF-8 source file");
            for (index, line) in source.lines().enumerate() {
                assert!(
                    !line.contains(".wgsl\""),
                    "{}:{} names a shader file; compile a constant of `shaders` instead",
                    path.display(),
                    index + 1
                );
            }
        }
    }
}
