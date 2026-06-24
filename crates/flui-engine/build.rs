//! Build script for `flui-engine`.
//!
//! ## P1–P2a: wgsl_bindgen on filter-pass shaders
//!
//! Generates Rust bindings for the gamma, blur, morphology, and color_matrix
//! transfer-filter shaders into `OUT_DIR`.  Each generated file is `include!`d
//! from `src/wgpu/<name>/generated.rs`.
//!
//! ## Covered shaders (this file)
//!
//! | Shader          | OUT_DIR file                    | Uniform size |
//! |-----------------|--------------------------------|--------------|
//! | gamma.wgsl      | gamma_generated.rs             | 16 bytes     |
//! | blur.wgsl       | blur_generated.rs              | 32 bytes     |
//! | morphology.wgsl | morphology_generated.rs        | 48 bytes     |
//! | color_matrix.wgsl | color_matrix_generated.rs    | 80 bytes     |
//!
//! The naga_oil-composed shaders (mode, advanced_blend) remain hand-written
//! until wgsl_bindgen gains `#import` support — that is P3.
//!
//! ## Inner-attribute post-processing
//!
//! `wgsl_bindgen` 0.22 emits `#![allow(...)]` at the top of the generated file.
//! That form is only valid when `include!`d at the root of a Rust file, not
//! inside an inline `mod { }` block.  We strip the line after generation so the
//! `include!` compiles cleanly inside the `<name>_gen` inline module
//! (where suppression is applied via the wrapping `#[allow(...)]` attribute on
//! the `mod` item).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use regex::Regex;
use wgsl_bindgen::{
    GlamWgslTypeMap, OverrideSamplerType, OverrideTextureFilterability, SamplerType,
    WgslBindgenOptionBuilder, WgslTypeSerializeStrategy,
};

// ── Per-shader build configuration ────────────────────────────────────────────

/// Configuration for one wgsl_bindgen invocation.
struct ShaderConfig {
    /// Shader file name (relative to `src/wgpu/shaders/effects/`).
    shader_name: &'static str,
    /// Output file name written into `OUT_DIR`.
    out_file: &'static str,
    /// Override for `src_texture` filterability.
    ///
    /// - `true` → `Float { filterable: true }` + `SamplerBindingType::Filtering`.
    ///   Bilinear, for blur — the Gaussian kernel composes cleanly with bilinear
    ///   interpolation and requires a Filtering sampler.
    /// - `false` → `Float { filterable: false }` + `SamplerBindingType::NonFiltering`.
    ///   Nearest, for gamma / morphology / color_matrix — pixel-aligned texels;
    ///   a NonFiltering sampler avoids the wgpu validation error that fires when a
    ///   Filtering sampler is paired with a non-filterable texture binding.
    texture_filterable: bool,
    /// Matching sampler type for `src_sampler`.
    sampler_type: SamplerType,
}

/// All filter-pass shaders processed by wgsl_bindgen in P1 + P2a.
///
/// Each entry produces one `OUT_DIR/<out_file>` and one `rerun-if-changed`
/// directive.  The order here matches the logical pipeline order (gamma last in
/// the filter chain, but the list order is irrelevant to correctness).
const SHADER_CONFIGS: &[ShaderConfig] = &[
    ShaderConfig {
        shader_name: "gamma.wgsl",
        out_file: "gamma_generated.rs",
        // gamma.wgsl uses a non-filterable float texture + NonFiltering sampler.
        // wgsl_bindgen defaults to filterable=true/Filtering, so the override is
        // required to match the hand-written bind-group layout (and avoid a wgpu
        // validation error at runtime).
        texture_filterable: false,
        sampler_type: SamplerType::NonFiltering,
    },
    ShaderConfig {
        shader_name: "blur.wgsl",
        out_file: "blur_generated.rs",
        // blur.wgsl uses a filterable float texture + Filtering (bilinear) sampler.
        // This matches the wgsl_bindgen default, but the override is kept explicit
        // so the intent is self-documenting and safe against future default changes.
        texture_filterable: true,
        sampler_type: SamplerType::Filtering,
    },
    ShaderConfig {
        shader_name: "morphology.wgsl",
        out_file: "morphology_generated.rs",
        // morphology.wgsl uses a non-filterable float texture + NonFiltering sampler.
        // Nearest-clamp sampling is correct for max/min morphology: the per-channel
        // extremum is over exact texel values, not interpolated ones.
        texture_filterable: false,
        sampler_type: SamplerType::NonFiltering,
    },
    ShaderConfig {
        shader_name: "color_matrix.wgsl",
        out_file: "color_matrix_generated.rs",
        // color_matrix.wgsl uses a non-filterable float texture + NonFiltering sampler.
        // The color matrix operates per-texel with no spatial kernel; nearest sampling
        // avoids interpolation error between the source and the transformed output.
        texture_filterable: false,
        sampler_type: SamplerType::NonFiltering,
    },
];

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let shader_dir = crate_root.join("src/wgpu/shaders/effects");
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // _pad0/_pad1/_pad2 fields are WGSL-required alignment padding: they are
    // omitted from the generated *Init constructor so callers need not supply
    // them; the main struct still includes them, preserving the correct layout.
    // Shared across all shaders — the regex is a compile-time constant.
    let padding_regex =
        Regex::new(r"^_pad\d*$").expect("literal regex '^_pad\\d*$' is always valid");

    for cfg in SHADER_CONFIGS {
        generate_shader_bindings(cfg, &shader_dir, &out_dir, padding_regex.clone())?;
    }

    Ok(())
}

/// Run wgsl_bindgen for one shader and post-process the output.
fn generate_shader_bindings(
    cfg: &ShaderConfig,
    shader_dir: &Path,
    out_dir: &Path,
    padding_regex: Regex,
) -> Result<(), Box<dyn std::error::Error>> {
    let shader_path = shader_dir.join(cfg.shader_name);
    let out_path = out_dir.join(cfg.out_file);

    // wgsl_bindgen emits cargo:rerun-if-changed for every resolved shader file
    // via emit_rerun_if_change(true) below. This manual emit is harmless but
    // makes the dependency explicit at a glance.
    println!("cargo:rerun-if-changed={}", shader_path.display());

    // Texture-filterability override: every filter shader uses `src_texture`.
    // The regex matches the binding name exactly; wgsl_bindgen's default is
    // `filterable: true`, so non-filterable textures must be overridden.
    let texture_override = OverrideTextureFilterability {
        binding_regex: Regex::new(r"src_texture")
            .expect("literal regex 'src_texture' is always valid"),
        filterable: cfg.texture_filterable,
    };

    // Sampler-type override: every filter shader uses `src_sampler`.
    // The sampler type must match the texture filterability:
    // - filterable=true  → SamplerType::Filtering    (bilinear, for blur)
    // - filterable=false → SamplerType::NonFiltering  (nearest, for the rest)
    let sampler_override = OverrideSamplerType {
        binding_regex: Regex::new(r"src_sampler")
            .expect("literal regex 'src_sampler' is always valid"),
        sampler_type: cfg.sampler_type,
    };

    WgslBindgenOptionBuilder::default()
        .workspace_root(
            shader_dir
                .to_str()
                .expect("CARGO_MANIFEST_DIR + shader subpath is valid UTF-8"),
        )
        .add_entry_point(
            shader_path
                .to_str()
                .expect("CARGO_MANIFEST_DIR + shader path is valid UTF-8"),
        )
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(GlamWgslTypeMap)
        .override_texture_filterability(vec![texture_override])
        .override_sampler_type(vec![sampler_override])
        .add_custom_padding_field_regexp(padding_regex)
        .emit_rerun_if_change(true)
        .output(
            out_path
                .to_str()
                .expect("OUT_DIR + filename is valid UTF-8"),
        )
        .build()?
        .generate()?;

    // wgsl_bindgen 0.22 emits `#![allow(...)]` at the top of the generated file.
    // Inner attributes (`#!`) are only valid at the beginning of a *file*, not
    // inside an inline `mod { }` block.  Strip that line so the generated content
    // can be `include!`d inside the `<name>_gen` module without a compile error.
    // The suppressed lints are covered by the outer `#[allow(...)]` on the wrapping
    // `mod` item in each `generated.rs`.
    strip_inner_allow_from_generated(&out_path)?;

    Ok(())
}

/// Remove the `#![allow(...)]` inner-attribute line that wgsl_bindgen 0.22 emits
/// at the top of every generated file.  Replacing it with an empty line preserves
/// line numbers for the rest of the file (useful when diagnosing compiler errors
/// against the generated source).
fn strip_inner_allow_from_generated(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let patched = content
        .lines()
        .map(|line| {
            // Only strip the specific inner-allow line wgsl_bindgen emits.
            if line.starts_with("#![allow(") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, patched)?;
    Ok(())
}
