//! Build script for `flui-engine`.
//!
//! ## P1 pilot: wgsl_bindgen on `gamma.wgsl` only
//!
//! Generates Rust bindings for the gamma transfer filter shader into `OUT_DIR`.
//! The generated file is `include!`d from `src/wgpu/gamma/generated.rs`.
//!
//! Only `gamma.wgsl` is processed here.  The naga_oil-composed shaders
//! (blur, morphology, mode, advanced_blend) remain hand-written until the
//! pilot proves byte-identical bindings — that is P2/P3.
//!
//! ## Inner-attribute post-processing
//!
//! `wgsl_bindgen` 0.22 emits `#![allow(...)]` at the top of the generated file.
//! That form is only valid when `include!`d at the root of a Rust file, not
//! inside an inline `mod { }` block.  We strip the line after generation so the
//! `include!` compiles cleanly inside the `generated::gamma_gen` inline module
//! (where suppression is applied via the wrapping `#[allow(...)]` attribute on
//! the `mod` item).

use std::{env, fs, path::PathBuf};

use regex::Regex;
use wgsl_bindgen::{
    GlamWgslTypeMap, OverrideSamplerType, OverrideTextureFilterability, SamplerType,
    WgslBindgenOptionBuilder, WgslTypeSerializeStrategy,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let shader_path = crate_root.join("src/wgpu/shaders/effects/gamma.wgsl");
    let out_path = PathBuf::from(env::var("OUT_DIR")?).join("gamma_generated.rs");

    // wgsl_bindgen emits cargo:rerun-if-changed for every resolved shader file
    // via emit_rerun_if_change(true) below. Cargo deduplicates the directive, so
    // this manual emit is harmless but makes the dependency explicit at a glance.
    println!("cargo:rerun-if-changed={}", shader_path.display());

    // gamma.wgsl lives alongside the other shaders; its workspace_root must be
    // the directory containing it so that relative-import resolution (unused here,
    // but required by the API) has a valid base.
    let shader_dir = crate_root.join("src/wgpu/shaders/effects");

    // Literal regex patterns: these are compile-time string constants and cannot
    // fail to compile — the expect messages state the invariant.
    let texture_override = OverrideTextureFilterability {
        binding_regex: Regex::new(r"src_texture")
            .expect("literal regex 'src_texture' is always valid"),
        // gamma.wgsl declares the source texture as non-filterable (Float {
        // filterable: false }). Without this, wgsl_bindgen defaults to
        // filterable=true, producing a bind-group layout mismatch.
        filterable: false,
    };
    let sampler_override = OverrideSamplerType {
        binding_regex: Regex::new(r"src_sampler")
            .expect("literal regex 'src_sampler' is always valid"),
        // gamma.wgsl uses a NonFiltering sampler. Without this, the generated
        // layout defaults to Filtering, causing a wgpu validation error.
        sampler_type: SamplerType::NonFiltering,
    };
    let padding_regex =
        Regex::new(r"^_pad\d*$").expect("literal regex '^_pad\\d*$' is always valid");

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
        // _pad0/_pad1/_pad2 fields are WGSL-required alignment padding: they are
        // omitted from the generated *Init constructor so callers need not supply
        // them; the main struct still includes them, preserving the 16-byte layout.
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
    // can be `include!`d inside the `gamma_gen` module without a compile error.
    // The suppressed lints are covered by the outer `#[allow(...)]` on the wrapping
    // `mod gamma_gen` item in `generated.rs`.
    strip_inner_allow_from_generated(&out_path)?;

    Ok(())
}

/// Remove the `#![allow(...)]` inner-attribute line that wgsl_bindgen 0.22 emits
/// at the top of every generated file.  Replacing it with an empty line preserves
/// line numbers for the rest of the file (useful when diagnosing compiler errors
/// against the generated source).
fn strip_inner_allow_from_generated(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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
