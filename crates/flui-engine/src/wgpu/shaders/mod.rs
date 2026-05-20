//! WGSL shader source bindings for the wgpu backend.
//!
//! Only shaders consumed by `painter.rs` via `wgpu::ShaderSource::Wgsl(super::
//! shaders::*.into())` are exposed as `pub const` aliases here. Other shader
//! files (`masks/*.wgsl`, `effects/*.wgsl`, `gradients/*.wgsl`, `common/*.wgsl`)
//! are loaded directly via `include_str!("shaders/...")` from their consumers
//! (`shader_compiler.rs` for the mask/blur/morph stack; `effects_pipeline.rs`
//! for the gradient and shadow stack); the const-alias indirection only earns
//! its place where multiple consumers reference the same shader.
//!
//! Removed in Mythos U10 (commit chain on `feat/flui-engine-mythos-redesign`):
//! 12 unused const aliases (`FILL`, `SOLID`, `LINEAR_GRADIENT`, `RADIAL_GRADIENT`,
//! `BLUR_HORIZONTAL`, `BLUR_VERTICAL`, `BLUR_DOWNSAMPLE`, `BLUR_UPSAMPLE`,
//! `SHADOW`, `LINEAR`, `RADIAL`, `SDF`) that pointed at WGSL files already
//! `include_str!`-loaded by `shader_compiler.rs` / `effects_pipeline.rs`. The
//! WGSL files themselves stay; only the redundant indirection went away.

// Basic shapes
/// Basic shape rendering shader.
pub const SHAPE: &str = include_str!("shape.wgsl");

// Instanced rendering
/// Instanced rectangle rendering shader.
pub const RECT_INSTANCED: &str = include_str!("rect_instanced.wgsl");
/// Instanced circle rendering shader.
pub const CIRCLE_INSTANCED: &str = include_str!("circle_instanced.wgsl");
/// Instanced arc rendering shader.
pub const ARC_INSTANCED: &str = include_str!("arc_instanced.wgsl");
/// Instanced texture rendering shader.
pub const TEXTURE_INSTANCED: &str = include_str!("texture_instanced.wgsl");
