//! Build script for `flui-engine`.
//!
//! ## Generated filter bindings
//!
//! Generates Rust bindings for the transfer-filter shaders (gamma, blur,
//! morphology, resample, color_matrix) and the WESL-composed shaders (mode,
//! advanced_blend), plus decoded-image source filters, into `OUT_DIR`. The
//! consumer includes each generated file and owns its runtime pipeline.
//!
//! ## Covered shaders (this file)
//!
//! | Shader              | OUT_DIR file                  | Uniform size | Imports        |
//! |---------------------|-------------------------------|--------------|----------------|
//! | gamma.wgsl          | gamma_generated.rs            | 16 bytes     | color_filter   |
//! | blur.wgsl           | blur_generated.rs             | 32 bytes     | —              |
//! | morphology.wgsl     | morphology_generated.rs       | 48 bytes     | —              |
//! | resample.wgsl       | resample_generated.rs         | 64 bytes     | —              |
//! | color_matrix.wgsl   | color_matrix_generated.rs     | 80 bytes     | color_filter   |
//! | effects/mode.wgsl   | mode_generated.rs             | 32 bytes     | color_filter   |
//! | image_source_filter_layout.wgsl | image_source_filter_generated.rs | 112 bytes | color_filter |
//! | advanced_blend.wgsl | advanced_blend_generated.rs   | 128 bytes    | blend_helpers  |
//!
//! ## Import resolution for the composed shaders
//!
//! Composed shaders import `color_filter` or `blend_helpers` with WESL
//! (`import package::color_filter::filter_mode;`, a module per file under
//! `src/shaders/`). `compose_wesl_shaders` resolves them with the `wesl`
//! compiler into plain WGSL at `OUT_DIR/shaders/<same path>.wgsl`, before any
//! binding is generated, so:
//!
//! - the pipelines include that finished WGSL (`shaders::composed_wgsl!`), and
//!   no shader composer runs or ships at run time;
//! - `wgsl_bindgen` reads the composed copy and never resolves an import
//!   itself. Its workspace root for `effects/mode.wgsl` is the composed
//!   `effects/`, so the generated module is named `mode`, not `effects::mode`.
//!
//! Only the uniform structs + bind-group layout are taken from the generated
//! bindings.
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
/// data (gamma / morphology / color_matrix / mode) must override
/// both to non-filtering, or wgpu rejects the bind group at runtime (a Filtering
/// sampler paired with a non-filterable texture is a validation error).  Blur,
/// resample and advanced_blend are filterable: bilinear composes with the
/// Gaussian, a resample reads between texels, and advanced_blend reads its
/// foreground with the sampling its caller picks.
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
    /// Shader file name (relative to `src/shaders/effects/`).
    shader_name: &'static str,
    /// Output file name written into `OUT_DIR`.
    out_file: &'static str,
    /// Binding overrides for this shader.
    overrides: BindingOverrides,
}

/// All import-free filter-pass shaders (P1 + P2a).
const SHADER_CONFIGS: &[ShaderConfig] = &[
    ShaderConfig {
        shader_name: "blur.wgsl",
        out_file: "blur_generated.rs",
        // blur is filterable: the Gaussian kernel composes
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
        shader_name: "resample.wgsl",
        out_file: "resample_generated.rs",
        // The resample reads between texels through a nearest or bilinear
        // sampler the caller picks, so its texture is filterable.
        overrides: BindingOverrides {
            texture_regex: "src_texture",
            texture_filterable: true,
            sampler_regex: "src_sampler",
            sampler_type: SamplerType::Filtering,
        },
    },
];

// ── WESL-composed shaders (`import package::…`) ───────────────────────────────

/// Every shader that imports a shared module, as a WESL module path under
/// `src/shaders/` (`effects/mode` is `effects/mode.wgsl`).
///
/// `compose_wesl_shaders` resolves each one into plain WGSL at
/// `OUT_DIR/shaders/<path>.wgsl`; the runtime pipelines include that output
/// through `shaders::composed_wgsl!`, and `wgsl_bindgen` reads it too.
const WESL_ENTRIES: &[&str] = &[
    "advanced_blend",
    "effects/color_matrix",
    "effects/gamma",
    "effects/mode",
    // A fragment of the decoded-image assemblies in `shaders/mod.rs`: it has
    // no entry point, so the compiler keeps every declaration it makes.
    "image_source_filter",
    "image_source_filter_layout",
];

/// Configuration for one composed shader whose bindings `wgsl_bindgen`
/// generates from its composed output.
struct ComposedShaderConfig {
    /// Entry shader path relative to `OUT_DIR/shaders/` (the composed copy of
    /// `src/shaders/`).
    entry_rel: &'static str,
    /// Output file name written into `OUT_DIR`.
    out_file: &'static str,
    /// Workspace root relative to `OUT_DIR/shaders/`.  Empty string = that
    /// directory itself.  Chosen so the generated module name is flat (`mode`,
    /// not `effects::mode`).
    workspace_rel: &'static str,
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

/// The composed shaders whose bindings are generated.
const COMPOSED_SHADER_CONFIGS: &[ComposedShaderConfig] = &[
    ComposedShaderConfig {
        entry_rel: "image_source_filter_layout.wgsl",
        out_file: "image_source_filter_generated.rs",
        workspace_rel: "",
        overrides: NEAREST_SRC,
        field_type_overrides: &[],
    },
    ComposedShaderConfig {
        entry_rel: "effects/gamma.wgsl",
        out_file: "gamma_generated.rs",
        workspace_rel: "effects",
        overrides: NEAREST_SRC,
        field_type_overrides: &[],
    },
    ComposedShaderConfig {
        entry_rel: "effects/color_matrix.wgsl",
        out_file: "color_matrix_generated.rs",
        workspace_rel: "effects",
        overrides: NEAREST_SRC,
        field_type_overrides: &[],
    },
    ComposedShaderConfig {
        entry_rel: "effects/mode.wgsl",
        out_file: "mode_generated.rs",
        // Workspace root = effects/ keeps the generated module named `mode`.
        workspace_rel: "effects",
        overrides: NEAREST_SRC,
        // mode.wgsl has no vec3 field — no override needed.
        field_type_overrides: &[],
    },
    ComposedShaderConfig {
        entry_rel: "advanced_blend.wgsl",
        out_file: "advanced_blend_generated.rs",
        // advanced_blend.wgsl sits at the root of shaders/.
        // Filterable: a backdrop resampled through a transform is read
        // bilinearly as its foreground; the rest bind a nearest sampler.
        workspace_rel: "",
        overrides: BindingOverrides {
            texture_regex: "foreground_tex|backdrop_tex",
            texture_filterable: true,
            sampler_regex: "blend_sampler",
            sampler_type: SamplerType::Filtering,
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
    let shader_root = crate_root.join("src/shaders");
    let effects_dir = shader_root.join("effects");
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // _pad0/_pad1/_pad2 fields are WGSL-required alignment padding: they are
    // omitted from the generated *Init constructor so callers need not supply
    // them; the main struct still includes them, preserving the correct layout.
    // Shared across all shaders — the regex is a compile-time constant.
    let padding_regex =
        Regex::new(r"^_pad\d*$").expect("literal regex '^_pad\\d*$' is always valid");

    // One watch over the whole shader tree: a change to any module can change
    // a composed shader, and every output below derives from these sources.
    println!("cargo:rerun-if-changed={}", shader_root.display());

    let composed_root = out_dir.join("shaders");
    compose_wesl_shaders(&shader_root, &composed_root)?;

    for cfg in SHADER_CONFIGS {
        let entry_path = effects_dir.join(cfg.shader_name);
        let out_path = out_dir.join(cfg.out_file);
        run_wgsl_bindgen(
            &effects_dir,
            &entry_path,
            &out_path,
            &cfg.overrides,
            &[],
            padding_regex.clone(),
        )?;
    }

    for cfg in COMPOSED_SHADER_CONFIGS {
        let entry_path = composed_root.join(cfg.entry_rel);
        let out_path = out_dir.join(cfg.out_file);
        let workspace_root = join_rel(&composed_root, cfg.workspace_rel);
        run_wgsl_bindgen(
            &workspace_root,
            &entry_path,
            &out_path,
            &cfg.overrides,
            cfg.field_type_overrides,
            padding_regex.clone(),
        )?;
    }

    Ok(())
}

/// Resolve every [`WESL_ENTRIES`] module into plain WGSL under `composed_root`.
///
/// - Names are not mangled: the composed shader keeps the entry points,
///   uniform structs and bindings as written, which the pipelines and the
///   generated bindings name. Two modules defining one name is then a
///   redefinition error from naga, at build time for the generated shaders and
///   in the pipeline tests for the rest.
/// - Declarations no entry point reaches are stripped, except in the entry
///   module itself (`keep_main`), so a fragment such as `image_source_filter`
///   keeps the functions its assembly calls.
/// - WESL's own validation needs its `eval` feature; naga validates instead.
fn compose_wesl_shaders(
    shader_root: &Path,
    composed_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = wesl::CompileOptions {
        mangler: wesl::ManglerKind::None,
        keep_main: true,
        validate: false,
        ..wesl::CompileOptions::default()
    };
    let compiler =
        wesl::Compiler::new_with_resolver(options, wesl::resolver::FileResolver::new(shader_root));
    for entry in WESL_ENTRIES {
        let module_path = format!("package::{}", entry.replace('/', "::"))
            .parse::<wesl::syntax::ModulePath>()
            .map_err(|error| format!("WESL module path for `{entry}`: {error}"))?;
        let composed = compiler
            .compile_module(&module_path)
            .map_err(|error| format!("composing `{entry}` with WESL:\n{error}"))?;
        let out_path = composed_root.join(format!("{entry}.wgsl"));
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, composed.to_string())?;
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
/// The shader has no imports left: it is either self-contained or the
/// composed output of [`compose_wesl_shaders`].
fn run_wgsl_bindgen(
    workspace_root: &Path,
    entry_path: &Path,
    out_path: &Path,
    overrides: &BindingOverrides,
    field_type_overrides: &[(&str, &str, &str)],
    padding_regex: Regex,
) -> Result<(), Box<dyn std::error::Error>> {
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
        // `main` watches the whole source tree; a watch on the composed copies in
        // OUT_DIR, which this script rewrites, would rerun it on every build.
        .emit_rerun_if_change(false)
        .output(
            out_path
                .to_str()
                .expect("OUT_DIR + filename is valid UTF-8"),
        );
    builder.build()?.generate()?;

    // wgsl_bindgen 0.22 emitted `#![allow(...)]` at the top of every generated
    // file. Inner attributes (`#!`) are only valid at the beginning of a *file*,
    // not inside an inline `mod { }` block, so that line had to go before the
    // generated content could be `include!`d inside the `<name>_gen` module.
    // 0.23.3 no longer emits it (checked against the actual generated output, not
    // its changelog), which makes this pass a no-op today; it is kept as a guard
    // so a future version reintroducing the attribute cannot break the include.
    // Either way the suppressed lints are covered by the outer `#[allow(...)]` on
    // the wrapping `mod` item in each `generated.rs`.
    strip_inner_allow_from_generated(out_path)?;

    Ok(())
}

/// Remove the `#![allow(...)]` inner-attribute line that wgsl_bindgen emitted at
/// the top of every generated file through 0.22 (0.23.3 does not — see the call
/// site).  Replacing it with an empty line preserves line numbers for the rest of
/// the file (useful when diagnosing compiler errors against the generated
/// source).
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
