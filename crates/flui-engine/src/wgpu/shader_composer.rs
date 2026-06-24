//! naga_oil shader composition helper.
//!
//! Provides `compose_wgsl_shader`, a thin wrapper over the naga_oil
//! `Composer` that:
//!
//! 1. Registers one or more composable WGSL modules (e.g. `blend_helpers.wgsl`
//!    marked with `#define_import_path blend_helpers`).
//! 2. Composes a top-level WGSL source that carries `#import` directives into a
//!    resolved `naga::Module`.
//! 3. Returns that `naga::Module` wrapped in a `Cow::Owned` ready for
//!    [`wgpu::ShaderSource::Naga`].
//!
//! ## Why `ShaderSource::Naga` and not WGSL re-emit?
//!
//! The `wgpu/naga-ir` feature (already enabled in the workspace `wgpu` dep)
//! lets us hand the composed `naga::Module` directly to wgpu, skipping the
//! naga → WGSL back-end round-trip.  That path has lower overhead (no string
//! serialisation) and zero extra dependencies beyond naga_oil itself, which
//! already pulls `naga ^29` matching wgpu 29's vendored naga.
//!
//! naga_oil's `Composer::make_naga_module` returns `naga::Module`, and
//! `wgpu::ShaderSource::Naga` accepts `std::borrow::Cow<'static, naga::Module>`
//! — so `Cow::Owned(module)` is the natural bridge.

use std::borrow::Cow;

use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, ComposerError, NagaModuleDescriptor, ShaderLanguage,
    ShaderType,
};

/// A composable module source to register before composing the entry shader.
///
/// Pass one entry per `#define_import_path` module that the entry shader imports.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ComposableSource {
    /// The WGSL source text (must contain a `#define_import_path` directive).
    pub source: &'static str,
    /// A short label used in error messages (e.g. `"blend_helpers.wgsl"`).
    pub file_path: &'static str,
}

/// Compose a WGSL shader by resolving its `#import` directives via naga_oil.
///
/// # Arguments
///
/// * `composables` — slice of library modules to register (each must have a
///   `#define_import_path <name>` at the top).  Modules are registered in
///   order; a module may only import modules registered before it.
/// * `entry_source` — the top-level WGSL source that contains `#import`
///   directives referencing the composable modules.
/// * `entry_file_path` — a label for the entry source, used in error messages
///   (e.g. `"effects/mode.wgsl"`).
///
/// # Returns
///
/// A `wgpu::ShaderSource::Naga` containing the fully resolved `naga::Module`.
///
/// # Errors
///
/// Returns a boxed `ComposerError` if any import is missing, or if naga
/// validation of the composed module fails.  The error is boxed because
/// `ComposerError` is 240+ bytes — large enough to trigger
/// `clippy::result_large_err` and cause stack-bloat at every call site.
pub(crate) fn compose_wgsl_shader(
    composables: &[ComposableSource],
    entry_source: &'static str,
    entry_file_path: &'static str,
) -> Result<wgpu::ShaderSource<'static>, Box<ComposerError>> {
    let mut composer = Composer::default();

    for composable in composables {
        composer
            .add_composable_module(ComposableModuleDescriptor {
                source: composable.source,
                file_path: composable.file_path,
                language: ShaderLanguage::Wgsl,
                ..Default::default()
            })
            .map_err(Box::new)?;
    }

    let naga_module = composer
        .make_naga_module(NagaModuleDescriptor {
            source: entry_source,
            file_path: entry_file_path,
            shader_type: ShaderType::Wgsl,
            ..Default::default()
        })
        .map_err(Box::new)?;

    Ok(wgpu::ShaderSource::Naga(Cow::Owned(naga_module)))
}
