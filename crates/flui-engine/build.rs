//! Build script for `flui-engine`.
//!
//! ## P1–P3: wgsl_bindgen on filter-pass and composed shaders
//!
//! Generates Rust bindings for the transfer-filter shaders (gamma, blur,
//! morphology, color_matrix) and the naga_oil-composed shaders (mode,
//! advanced_blend) into `OUT_DIR`.  Each generated file is `include!`d from
//! `src/wgpu/<name>/generated.rs`.
//!
//! ## Covered shaders (this file)
//!
//! | Shader              | OUT_DIR file                  | Uniform size | Imports        |
//! |---------------------|-------------------------------|--------------|----------------|
//! | gamma.wgsl          | gamma_generated.rs            | 16 bytes     | —              |
//! | blur.wgsl           | blur_generated.rs             | 32 bytes     | —              |
//! | morphology.wgsl     | morphology_generated.rs       | 48 bytes     | —              |
//! | color_matrix.wgsl   | color_matrix_generated.rs     | 80 bytes     | —              |
//! | effects/mode.wgsl   | mode_generated.rs             | 32 bytes     | blend_helpers  |
//! | advanced_blend.wgsl | advanced_blend_generated.rs   | 80 bytes     | blend_helpers  |
//!
//! ## `#import` resolution for the composed shaders
//!
//! `mode.wgsl` and `advanced_blend.wgsl` carry a
//! `#import blend_helpers::{…}` directive.  `wgsl_bindgen` resolves imports by
//! file-path convention (it maps the module name `blend_helpers` to a
//! `blend_helpers.wgsl` file found under the workspace root, the importing
//! file's directory, or an additional scan directory — see
//! `bevy_util::ModulePathResolver`).  `blend_helpers.wgsl` lives in
//! `src/wgpu/shaders/`, so:
//!
//! - `advanced_blend.wgsl` is in `shaders/` too → its workspace root is
//!   `shaders/` and the import resolves with no extra scan directory.
//! - `mode.wgsl` is in `shaders/effects/` → its workspace root stays `effects/`
//!   (so the generated module is named `mode`, not `effects::mode`) and
//!   `shaders/` is added as an import scan directory so `blend_helpers.wgsl` is
//!   discoverable.
//!
//! Only the uniform structs + bind-group layout are taken from the generated
//! bindings; the pipelines still build their shader module at runtime via
//! `compose_wgsl_shader` (naga_oil), so the runtime composition path is
//! unchanged.
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
    GlamWgslTypeMap,
    OverrideSamplerType,
    OverrideStructFieldType,
    OverrideTextureFilterability,
    SamplerType,
    WgslBindgenOptionBuilder,
    WgslTypeSerializeStrategy,
    // proc_macro2 TokenStream, re-exported by wgsl_bindgen — no extra build-dep needed.
    qs::TokenStream,
};

// ── Per-shader binding overrides ──────────────────────────────────────────────

/// Texture-filterability + sampler-type overrides for one shader.
///
/// `wgsl_bindgen` defaults every float texture to `filterable: true` and every
/// sampler to `SamplerBindingType::Filtering`.  Shaders that sample texel-aligned
/// data (gamma / morphology / color_matrix / mode / advanced_blend) must override
/// both to non-filtering, or wgpu rejects the bind group at runtime (a Filtering
/// sampler paired with a non-filterable texture is a validation error).  Blur is
/// the sole filterable shader (bilinear composes cleanly with the Gaussian).
struct BindingOverrides {
    /// Regex matching the texture binding name(s).  Use an alternation
    /// (`a|b`) for a shader with more than one sampled texture.
    texture_regex: &'static str,
    /// `true` → `Float { filterable: true }` + `Filtering`; `false` → nearest.
    texture_filterable: bool,
    /// Regex matching the sampler binding name.
    sampler_regex: &'static str,
    /// Sampler type, matched to `texture_filterable`.
    sampler_type: SamplerType,
}

/// All-nearest overrides shared by every shader except blur.
const NEAREST_SRC: BindingOverrides = BindingOverrides {
    texture_regex: "src_texture",
    texture_filterable: false,
    sampler_regex: "src_sampler",
    sampler_type: SamplerType::NonFiltering,
};

// ── Self-contained filter shaders (no imports) ────────────────────────────────

/// Configuration for one self-contained (import-free) shader.
struct ShaderConfig {
    /// Shader file name (relative to `src/wgpu/shaders/effects/`).
    shader_name: &'static str,
    /// Output file name written into `OUT_DIR`.
    out_file: &'static str,
    /// Binding overrides for this shader.
    overrides: BindingOverrides,
}

/// All import-free filter-pass shaders (P1 + P2a).
const SHADER_CONFIGS: &[ShaderConfig] = &[
    ShaderConfig {
        shader_name: "gamma.wgsl",
        out_file: "gamma_generated.rs",
        overrides: NEAREST_SRC,
    },
    ShaderConfig {
        shader_name: "blur.wgsl",
        out_file: "blur_generated.rs",
        // blur is the only filterable shader: the Gaussian kernel composes
        // cleanly with bilinear, and requires a Filtering sampler.
        overrides: BindingOverrides {
            texture_regex: "src_texture",
            texture_filterable: true,
            sampler_regex: "src_sampler",
            sampler_type: SamplerType::Filtering,
        },
    },
    ShaderConfig {
        shader_name: "morphology.wgsl",
        out_file: "morphology_generated.rs",
        overrides: NEAREST_SRC,
    },
    ShaderConfig {
        shader_name: "color_matrix.wgsl",
        out_file: "color_matrix_generated.rs",
        overrides: NEAREST_SRC,
    },
];

// ── naga_oil-composed shaders (#import blend_helpers) ─────────────────────────

/// Configuration for one composed shader whose `#import` must be resolved at
/// codegen time so `wgsl_bindgen` can parse it.
struct ComposedShaderConfig {
    /// Entry shader path relative to `src/wgpu/shaders/`.
    entry_rel: &'static str,
    /// Output file name written into `OUT_DIR`.
    out_file: &'static str,
    /// Workspace root relative to `src/wgpu/shaders/`.  Empty string = `shaders/`
    /// itself.  Chosen so the generated module name is flat (`mode`, not
    /// `effects::mode`).
    workspace_rel: &'static str,
    /// Import scan directories relative to `src/wgpu/shaders/` (empty string =
    /// `shaders/`).  Needed when `blend_helpers.wgsl` is not under the workspace
    /// root or the entry's own directory.
    scan_rel: &'static [&'static str],
    /// Binding overrides for this shader.
    overrides: BindingOverrides,
    /// Per-field Rust-type overrides as `(struct_regex, field_regex, rust_type)`.
    ///
    /// Required for the `vec3<f32>` tight-pack: `wgsl_bindgen` 0.22 maps a WGSL
    /// `vec3<f32>` to `[f32; 4]` (16 bytes, rounding to vec4) under the Bytemuck
    /// strategy, which shifts every following field — its own generated layout
    /// asserts then fail (naga reports the WGSL offset, the padded Rust struct
    /// has a different one).  Forcing the field to `[f32; 3]` restores the tight
    /// WGSL pack; the tool-derived offset asserts (computed from naga) still
    /// validate the full layout, so byte-identity is preserved.
    field_type_overrides: &'static [(&'static str, &'static str, &'static str)],
}

/// All naga_oil-composed shaders (P3).  Both sample texel-aligned premultiplied
/// data with a nearest sampler.
const COMPOSED_SHADER_CONFIGS: &[ComposedShaderConfig] = &[
    ComposedShaderConfig {
        entry_rel: "effects/mode.wgsl",
        out_file: "mode_generated.rs",
        // Workspace root = effects/ keeps the generated module named `mode`.
        workspace_rel: "effects",
        // blend_helpers.wgsl lives one level up in shaders/.
        scan_rel: &[""],
        overrides: NEAREST_SRC,
        // mode.wgsl has no vec3 field — no override needed.
        field_type_overrides: &[],
    },
    ComposedShaderConfig {
        entry_rel: "advanced_blend.wgsl",
        out_file: "advanced_blend_generated.rs",
        // advanced_blend.wgsl and blend_helpers.wgsl share shaders/ → no scan dir.
        workspace_rel: "",
        scan_rel: &[],
        overrides: BindingOverrides {
            texture_regex: "foreground_tex|backdrop_tex",
            texture_filterable: false,
            sampler_regex: "nearest_sampler",
            sampler_type: SamplerType::NonFiltering,
        },
        // BlendUniforms.tint_rgb is a `vec3<f32>` packed tight before `mode: u32`
        // — force [f32; 3] so the generated layout matches the WGSL (see the
        // field doc above).  The `BlendUniforms` regex is intentionally a
        // substring match: it must also retype the generated `BlendUniformsInit`
        // constructor-mirror struct's `tint_rgb` field to the same `[f32; 3]`.
        field_type_overrides: &[("BlendUniforms", "tint_rgb", "[f32; 3]")],
    },
];

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let shader_root = crate_root.join("src/wgpu/shaders");
    let effects_dir = shader_root.join("effects");
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // _pad0/_pad1/_pad2 fields are WGSL-required alignment padding: they are
    // omitted from the generated *Init constructor so callers need not supply
    // them; the main struct still includes them, preserving the correct layout.
    // Shared across all shaders — the regex is a compile-time constant.
    let padding_regex =
        Regex::new(r"^_pad\d*$").expect("literal regex '^_pad\\d*$' is always valid");

    for cfg in SHADER_CONFIGS {
        let entry_path = effects_dir.join(cfg.shader_name);
        let out_path = out_dir.join(cfg.out_file);
        run_wgsl_bindgen(
            &effects_dir,
            &entry_path,
            &out_path,
            &cfg.overrides,
            &[],
            &[],
            padding_regex.clone(),
        )?;
    }

    for cfg in COMPOSED_SHADER_CONFIGS {
        let entry_path = shader_root.join(cfg.entry_rel);
        let out_path = out_dir.join(cfg.out_file);
        let workspace_root = join_rel(&shader_root, cfg.workspace_rel);
        let scan_dirs: Vec<PathBuf> = cfg
            .scan_rel
            .iter()
            .map(|rel| join_rel(&shader_root, rel))
            .collect();
        let scan_refs: Vec<&Path> = scan_dirs.iter().map(PathBuf::as_path).collect();
        run_wgsl_bindgen(
            &workspace_root,
            &entry_path,
            &out_path,
            &cfg.overrides,
            &scan_refs,
            cfg.field_type_overrides,
            padding_regex.clone(),
        )?;
    }

    Ok(())
}

/// Join a path fragment relative to `base`, treating the empty string as `base`.
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        base.to_path_buf()
    } else {
        base.join(rel)
    }
}

/// Run `wgsl_bindgen` for one shader and post-process the output.
///
/// `scan_dirs` are extra import-resolution roots (empty for self-contained
/// shaders, so their generated output is identical to the pre-P3 single-shader
/// path).
fn run_wgsl_bindgen(
    workspace_root: &Path,
    entry_path: &Path,
    out_path: &Path,
    overrides: &BindingOverrides,
    scan_dirs: &[&Path],
    field_type_overrides: &[(&str, &str, &str)],
    padding_regex: Regex,
) -> Result<(), Box<dyn std::error::Error>> {
    // wgsl_bindgen emits cargo:rerun-if-changed for every resolved shader file
    // (including imports) via emit_rerun_if_change(true) below. This manual emit
    // for the entry is harmless but makes the dependency explicit at a glance.
    println!("cargo:rerun-if-changed={}", entry_path.display());

    // Texture-filterability override: wgsl_bindgen defaults to `filterable: true`,
    // so non-filterable textures must be overridden (and the sampler matched).
    let texture_override = OverrideTextureFilterability {
        binding_regex: Regex::new(overrides.texture_regex)
            .expect("shader texture binding regex is a valid literal"),
        filterable: overrides.texture_filterable,
    };
    let sampler_override = OverrideSamplerType {
        binding_regex: Regex::new(overrides.sampler_regex)
            .expect("shader sampler binding regex is a valid literal"),
        sampler_type: overrides.sampler_type,
    };

    // Per-field Rust-type overrides (e.g. forcing a `vec3<f32>` to `[f32; 3]`).
    let struct_field_overrides: Vec<OverrideStructFieldType> = field_type_overrides
        .iter()
        .map(|&(struct_regex, field_regex, rust_type)| {
            let rust_type: TokenStream = rust_type
                .parse()
                .expect("field-type override is a valid Rust type token stream");
            OverrideStructFieldType::from((struct_regex, field_regex, rust_type))
        })
        .collect();

    let mut builder = WgslBindgenOptionBuilder::default();
    builder
        .workspace_root(
            workspace_root
                .to_str()
                .expect("CARGO_MANIFEST_DIR + shader subpath is valid UTF-8"),
        )
        .add_entry_point(
            entry_path
                .to_str()
                .expect("CARGO_MANIFEST_DIR + shader path is valid UTF-8"),
        )
        .serialization_strategy(WgslTypeSerializeStrategy::Bytemuck)
        .type_map(GlamWgslTypeMap)
        .override_texture_filterability(vec![texture_override])
        .override_sampler_type(vec![sampler_override])
        .override_struct_field_type(struct_field_overrides)
        .add_custom_padding_field_regexp(padding_regex)
        .emit_rerun_if_change(true)
        .output(
            out_path
                .to_str()
                .expect("OUT_DIR + filename is valid UTF-8"),
        );
    for scan_dir in scan_dirs {
        builder.additional_scan_dir((
            None,
            scan_dir
                .to_str()
                .expect("CARGO_MANIFEST_DIR + scan subpath is valid UTF-8"),
        ));
    }
    builder.build()?.generate()?;

    // wgsl_bindgen 0.22 emits `#![allow(...)]` at the top of the generated file.
    // Inner attributes (`#!`) are only valid at the beginning of a *file*, not
    // inside an inline `mod { }` block.  Strip that line so the generated content
    // can be `include!`d inside the `<name>_gen` module without a compile error.
    // The suppressed lints are covered by the outer `#[allow(...)]` on the wrapping
    // `mod` item in each `generated.rs`.
    strip_inner_allow_from_generated(out_path)?;

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
