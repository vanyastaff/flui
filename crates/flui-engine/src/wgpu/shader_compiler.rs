//! Shader compilation and caching for shader mask effects
//!
//! This module provides shader compilation, caching, and uniform buffer
//! management for ShaderMaskLayer rendering.

use std::{collections::HashMap, sync::Arc};

// Cycle 4 E-7 (extended): `bytemuck::{Pod, Zeroable}` imports
// dropped alongside the deletion of the 5 dead uniform helpers (see
// comment block above the `#[cfg(test)] mod tests` declaration).
// `Shader` is retained -- `ShaderType::from_shader` (live, 1
// callsite in offscreen.rs) still pattern-matches on the enum
// variants. `Color` is needed only by the test module below; it is
// imported there under the same cfg gate so default builds skip it.
use flui_types::painting::Shader;
use parking_lot::RwLock;

/// Shader type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderType {
    /// Solid color mask shader
    SolidMask,
    /// Linear gradient mask shader
    LinearGradientMask,
    /// Radial gradient mask shader
    RadialGradientMask,
    /// Sweep (angular/conic) gradient mask shader
    SweepGradientMask,
    /// Gaussian blur horizontal pass shader (compute)
    GaussianBlurHorizontal,
    /// Gaussian blur vertical pass shader (compute)
    GaussianBlurVertical,
    /// Dual Kawase blur downsample pass shader
    DualKawaseDownsample,
    /// Dual Kawase blur upsample pass shader
    DualKawaseUpsample,
    // MorphDilate / MorphErode removed: morphology filters now use their own
    // pipeline in `morphology/pipeline.rs` (MorphologyPipeline) and embed
    // their WGSL via `include_str!` directly — not through ShaderCache.
}

impl ShaderType {
    /// Get the WGSL source code for this shader type
    pub fn source_code(self) -> &'static str {
        match self {
            ShaderType::SolidMask => include_str!("shaders/masks/solid.wgsl"),
            ShaderType::LinearGradientMask => include_str!("shaders/masks/linear_gradient.wgsl"),
            ShaderType::RadialGradientMask => include_str!("shaders/masks/radial_gradient.wgsl"),
            ShaderType::SweepGradientMask => include_str!("shaders/masks/sweep_gradient.wgsl"),
            ShaderType::GaussianBlurHorizontal => {
                include_str!("shaders/effects/blur_horizontal.wgsl")
            }
            ShaderType::GaussianBlurVertical => {
                include_str!("shaders/effects/blur_vertical.wgsl")
            }
            ShaderType::DualKawaseDownsample => {
                include_str!("shaders/effects/blur_downsample.wgsl")
            }
            ShaderType::DualKawaseUpsample => {
                include_str!("shaders/effects/blur_upsample.wgsl")
            }
        }
    }

    /// Get the shader label (for debugging)
    pub fn label(self) -> &'static str {
        match self {
            ShaderType::SolidMask => "Solid Mask Shader",
            ShaderType::LinearGradientMask => "Linear Gradient Mask Shader",
            ShaderType::RadialGradientMask => "Radial Gradient Mask Shader",
            ShaderType::SweepGradientMask => "Sweep Gradient Mask Shader",
            ShaderType::GaussianBlurHorizontal => "Gaussian Blur Horizontal Shader",
            ShaderType::GaussianBlurVertical => "Gaussian Blur Vertical Shader",
            ShaderType::DualKawaseDownsample => "Dual Kawase Downsample",
            ShaderType::DualKawaseUpsample => "Dual Kawase Upsample",
        }
    }

    /// Get the shader type from a Shader
    pub fn from_shader(shader: &Shader) -> Self {
        match shader {
            Shader::LinearGradient { .. } => ShaderType::LinearGradientMask,
            Shader::RadialGradient { .. } => ShaderType::RadialGradientMask,
            Shader::SweepGradient { .. } => ShaderType::SweepGradientMask,
            Shader::Solid { .. } => ShaderType::SolidMask,
            // Image shader masks use full-opacity (white) solid mask because texture-based
            // masking requires a separate texture binding slot that the current mask pipeline
            // does not support. A dedicated image-mask pipeline is future work.
            _ => {
                tracing::debug!(
                    "ShaderType::from_shader: unsupported shader variant, using SolidMask (full opacity)"
                );
                ShaderType::SolidMask
            }
        }
    }
}

/// Compiled shader with optional cached GPU module
///
/// Stores the WGSL source and optionally the compiled `wgpu::ShaderModule`.
/// The module field is `None` when created via `get_or_compile` (source-only),
/// and populated when created via `get_or_compile_module` (GPU-ready).
#[derive(Clone)]
pub struct CompiledShader {
    pub shader_type: ShaderType,
    pub source: String,
    /// Cached GPU shader module. `None` for source-only inspection.
    /// Wrapped in `Arc` because `wgpu::ShaderModule` does not implement `Clone`.
    pub module: Option<Arc<wgpu::ShaderModule>>,
}

impl std::fmt::Debug for CompiledShader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledShader")
            .field("shader_type", &self.shader_type)
            .field("source", &format!("({} bytes)", self.source.len()))
            .field("module", &self.module.as_ref().map(|_| "<ShaderModule>"))
            .finish()
    }
}

/// Shader cache for compiled shaders
///
/// Caches compiled shader modules to avoid recompilation.
/// Thread-safe via RwLock.
#[derive(Debug)]
pub struct ShaderCache {
    cache: RwLock<HashMap<ShaderType, Arc<CompiledShader>>>,
}

impl ShaderCache {
    /// Create new empty shader cache
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get or compile a shader
    ///
    /// Returns cached shader if available, otherwise compiles and caches it.
    #[must_use]
    pub fn get_or_compile(&self, shader_type: ShaderType) -> Arc<CompiledShader> {
        // Try to get from cache first (read lock)
        {
            let cache = self.cache.read();
            if let Some(shader) = cache.get(&shader_type) {
                tracing::trace!("Shader cache hit: {:?}", shader_type);
                return Arc::clone(shader);
            }
        }

        // Not in cache, compile it (write lock)
        let mut cache = self.cache.write();

        // Double-check in case another thread compiled it while we waited for the lock
        if let Some(shader) = cache.get(&shader_type) {
            return Arc::clone(shader);
        }

        // Compile the shader (source-only, no GPU module)
        let compiled = Arc::new(CompiledShader {
            shader_type,
            source: shader_type.source_code().to_string(),
            module: None,
        });

        cache.insert(shader_type, Arc::clone(&compiled));
        compiled
    }

    /// Get or compile a shader with its GPU module
    ///
    /// Returns cached shader with a compiled `wgpu::ShaderModule`. If the shader
    /// source is already cached but lacks a module, compiles and caches the module.
    /// Uses double-check locking to avoid redundant compilation under contention.
    #[must_use]
    pub fn get_or_compile_module(
        &self,
        shader_type: ShaderType,
        device: &wgpu::Device,
    ) -> Arc<CompiledShader> {
        // Fast path: check if we already have a compiled module (read lock)
        {
            let cache = self.cache.read();
            if let Some(shader) = cache.get(&shader_type)
                && shader.module.is_some()
            {
                tracing::trace!("Shader module cache hit: {:?}", shader_type);
                return Arc::clone(shader);
            }
        }

        // Slow path: need to compile the module (write lock)
        let mut cache = self.cache.write();

        // Double-check: another thread may have compiled it while we waited
        if let Some(shader) = cache.get(&shader_type)
            && shader.module.is_some()
        {
            return Arc::clone(shader);
        }

        // Get or create the source
        let source = shader_type.source_code().to_string();

        // Compile the GPU shader module
        let module = Arc::new(device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(shader_type.label()),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(shader_type.source_code())),
        }));

        let compiled = Arc::new(CompiledShader {
            shader_type,
            source,
            module: Some(module),
        });

        cache.insert(shader_type, Arc::clone(&compiled));
        tracing::debug!("Compiled and cached shader module: {:?}", shader_type);
        compiled
    }

    /// Pre-compile all shaders
    ///
    /// Useful for avoiding frame time spikes on first use.
    pub fn precompile_all(&self) {
        let _ = self.get_or_compile(ShaderType::SolidMask);
        let _ = self.get_or_compile(ShaderType::LinearGradientMask);
        let _ = self.get_or_compile(ShaderType::RadialGradientMask);
        let _ = self.get_or_compile(ShaderType::SweepGradientMask);
    }

    /// Clear the shader cache.
    ///
    /// Zero production call sites; reserved for devtools hot-reload / debug
    /// flush flows. Suppressed from dead-code lint: the `devtools` feature
    /// integration is deferred; re-enable this method when a concrete consumer
    /// lands in `flui-devtools`.
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        cache.clear();
    }
}

impl Default for ShaderCache {
    fn default() -> Self {
        Self::new()
    }
}

// Cycle 4 E-7 (extended): the 5 forward-looking uniform helpers
// (`SolidMaskUniforms`, `LinearGradientUniforms`,
// `RadialGradientUniforms`, `SweepGradientUniforms`, and the
// `create_uniforms_from_shader` dispatcher) were deleted alongside
// dropping the module-level `#[allow(dead_code)]` mask. The 4
// `*Uniforms` structs existed only to be constructed from
// `create_uniforms_from_shader`, which itself had zero workspace
// consumers -- the shader-mask integration that was supposed to
// drive them never materialized. When that integration lands it
// will define its own uniform buffer shapes inline next to the
// concrete bind-group layout consumer, not as forward-bait helpers.
//
// PR #112 review fix: the previous version of this comment block
// had three orphan attributes above it -- `/// Uniform data for
// solid mask shader`, `#[repr(C)]`, `#[derive(Debug, Clone, Copy,
// Pod, Zeroable)]` -- left behind when `SolidMaskUniforms` was
// deleted. Under `--features enable-wgpu-tests` those attributes
// attached to the `mod tests` declaration below (where `#[derive]`
// is not valid + `Pod`/`Zeroable` are no longer in scope) and
// blocked compilation. Removed.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    // Cycle 4 PR #112 review fix: `Color` was dropped from the
    // file-level imports in the E-7 cleanup but the tests below
    // still reference `Color::WHITE` / `Color::RED` / etc. Bring
    // the import back here under the same cfg gate so default
    // builds skip it.
    use flui_types::styling::Color;

    use super::*;

    #[test]
    fn test_shader_type_from_shader() {
        use flui_types::geometry::{Offset, px};

        let solid = Shader::solid(Color::WHITE);
        assert_eq!(ShaderType::from_shader(&solid), ShaderType::SolidMask);

        let linear = Shader::simple_linear(
            Offset::ZERO,
            Offset::new(px(1.0), px(1.0)),
            vec![Color::RED, Color::BLUE],
        );
        assert_eq!(
            ShaderType::from_shader(&linear),
            ShaderType::LinearGradientMask
        );

        let radial = Shader::simple_radial(
            Offset::new(px(0.5), px(0.5)),
            1.0,
            vec![Color::WHITE, Color::BLACK],
        );
        assert_eq!(
            ShaderType::from_shader(&radial),
            ShaderType::RadialGradientMask
        );
    }

    #[test]
    fn test_shader_source_code() {
        assert!(
            ShaderType::SolidMask
                .source_code()
                .contains("Solid Color Mask")
        );
        assert!(
            ShaderType::LinearGradientMask
                .source_code()
                .contains("Linear Gradient")
        );
        assert!(
            ShaderType::RadialGradientMask
                .source_code()
                .contains("Radial Gradient")
        );
    }

    #[test]
    fn test_shader_cache() {
        let cache = ShaderCache::new();

        // First access should compile
        let shader1 = cache.get_or_compile(ShaderType::SolidMask);
        assert_eq!(shader1.shader_type, ShaderType::SolidMask);

        // Second access should hit cache
        let shader2 = cache.get_or_compile(ShaderType::SolidMask);
        assert!(Arc::ptr_eq(&shader1, &shader2));
    }

    #[test]
    fn test_shader_cache_precompile() {
        let cache = ShaderCache::new();
        cache.precompile_all();

        // All shaders should be in cache now
        let solid = cache.get_or_compile(ShaderType::SolidMask);
        let linear = cache.get_or_compile(ShaderType::LinearGradientMask);
        let radial = cache.get_or_compile(ShaderType::RadialGradientMask);

        assert_eq!(solid.shader_type, ShaderType::SolidMask);
        assert_eq!(linear.shader_type, ShaderType::LinearGradientMask);
        assert_eq!(radial.shader_type, ShaderType::RadialGradientMask);
    }

    // Cycle 4 PR #112 review fix: the 4 tests
    // (`test_solid_mask_uniforms`, `test_linear_gradient_uniforms`,
    // `test_radial_gradient_uniforms`, `test_create_uniforms_from_shader`)
    // that exercised the deleted `SolidMaskUniforms` /
    // `LinearGradientUniforms` / `RadialGradientUniforms` /
    // `create_uniforms_from_shader` items were removed alongside the
    // E-7 production-side deletion. The pre-fix commit landed the
    // production deletion but left the test bodies referencing
    // unresolved symbols -- only visible under
    // `--features enable-wgpu-tests`.
}
