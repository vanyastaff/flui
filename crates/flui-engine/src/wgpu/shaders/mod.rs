//! WGSL shader source bindings for the wgpu backend.
//!
//! Only shaders consumed by `painter` via `wgpu::ShaderSource::Wgsl(super::
//! shaders::*.into())` are exposed as `pub const` aliases here. Other shader
//! files (`masks/*.wgsl`, `effects/*.wgsl`, `gradients/*.wgsl`, `common/*.wgsl`)
//! are loaded directly via `include_str!("shaders/...")` from their consumers
//! (`shader_compiler.rs` for the mask/blur/morph stack; `effects_pipeline.rs`
//! for the gradient and shadow stack); the const-alias indirection only earns
//! its place where multiple consumers reference the same shader.
//!
//! Removed (commit chain on `feat/flui-engine-mythos-redesign`):
//! 12 unused const aliases (`FILL`, `SOLID`, `LINEAR_GRADIENT`, `RADIAL_GRADIENT`,
//! `BLUR_HORIZONTAL`, `BLUR_VERTICAL`, `BLUR_DOWNSAMPLE`, `BLUR_UPSAMPLE`,
//! `SHADOW`, `LINEAR`, `RADIAL`, `SDF`) that pointed at WGSL files already
//! `include_str!`-loaded by `shader_compiler.rs` / `effects_pipeline.rs`. The
//! WGSL files themselves stay; only the redundant indirection went away.

// Basic shapes
/// Basic shape rendering shader.
///
/// Prepended with the shared clip block: tessellated geometry evaluates the
/// same SDF clip the instanced primitives do, from a per-batch uniform.
pub const SHAPE: &str = concat!(include_str!("common/clip.wgsl"), include_str!("shape.wgsl"));

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
