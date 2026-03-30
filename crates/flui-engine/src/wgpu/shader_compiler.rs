//! Shader compilation and caching for shader mask effects
//!
//! This module provides shader compilation, caching, and uniform buffer
//! management for ShaderMaskLayer rendering.

use std::{collections::HashMap, sync::Arc};

use bytemuck::{Pod, Zeroable};
use flui_types::{painting::Shader, styling::Color};
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
    /// Gaussian blur horizontal pass shader (compute)
    GaussianBlurHorizontal,
    /// Gaussian blur vertical pass shader (compute)
    GaussianBlurVertical,
}

impl ShaderType {
    /// Get the WGSL source code for this shader type
    pub fn source_code(&self) -> &'static str {
        match self {
            ShaderType::SolidMask => include_str!("shaders/masks/solid.wgsl"),
            ShaderType::LinearGradientMask => include_str!("shaders/masks/linear_gradient.wgsl"),
            ShaderType::RadialGradientMask => include_str!("shaders/masks/radial_gradient.wgsl"),
            ShaderType::GaussianBlurHorizontal => {
                include_str!("shaders/effects/blur_horizontal.wgsl")
            }
            ShaderType::GaussianBlurVertical => {
                include_str!("shaders/effects/blur_vertical.wgsl")
            }
        }
    }

    /// Get the shader label (for debugging)
    pub fn label(&self) -> &'static str {
        match self {
            ShaderType::SolidMask => "Solid Mask Shader",
            ShaderType::LinearGradientMask => "Linear Gradient Mask Shader",
            ShaderType::RadialGradientMask => "Radial Gradient Mask Shader",
            ShaderType::GaussianBlurHorizontal => "Gaussian Blur Horizontal Shader",
            ShaderType::GaussianBlurVertical => "Gaussian Blur Vertical Shader",
        }
    }

    /// Get the shader type from a Shader
    pub fn from_shader(shader: &Shader) -> Self {
        match shader {
            Shader::LinearGradient { .. } => ShaderType::LinearGradientMask,
            Shader::RadialGradient { .. } => ShaderType::RadialGradientMask,
            Shader::Solid { .. } => ShaderType::SolidMask,
            // SweepGradient, Image, and any future variants fall back to solid mask
            _ => ShaderType::SolidMask,
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
            if let Some(shader) = cache.get(&shader_type) {
                if shader.module.is_some() {
                    tracing::trace!("Shader module cache hit: {:?}", shader_type);
                    return Arc::clone(shader);
                }
            }
        }

        // Slow path: need to compile the module (write lock)
        let mut cache = self.cache.write();

        // Double-check: another thread may have compiled it while we waited
        if let Some(shader) = cache.get(&shader_type) {
            if shader.module.is_some() {
                return Arc::clone(shader);
            }
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
    }

    /// Clear the cache
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

/// Uniform data for solid mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SolidMaskUniforms {
    pub mask_color: [f32; 4], // RGBA
}

impl SolidMaskUniforms {
    pub fn from_color(color: Color) -> Self {
        Self {
            mask_color: [
                f32::from(color.r) / 255.0,
                f32::from(color.g) / 255.0,
                f32::from(color.b) / 255.0,
                f32::from(color.a) / 255.0,
            ],
        }
    }
}

/// Uniform data for linear gradient mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LinearGradientUniforms {
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
}

impl LinearGradientUniforms {
    pub fn new(
        start: (f32, f32),
        end: (f32, f32),
        start_color: Color,
        end_color: Color,
    ) -> Self {
        Self {
            start: [start.0, start.1],
            end: [end.0, end.1],
            start_color: [
                f32::from(start_color.r) / 255.0,
                f32::from(start_color.g) / 255.0,
                f32::from(start_color.b) / 255.0,
                f32::from(start_color.a) / 255.0,
            ],
            end_color: [
                f32::from(end_color.r) / 255.0,
                f32::from(end_color.g) / 255.0,
                f32::from(end_color.b) / 255.0,
                f32::from(end_color.a) / 255.0,
            ],
        }
    }
}

/// Uniform data for radial gradient mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RadialGradientUniforms {
    pub center: [f32; 2],
    pub radius: f32,
    pub _padding: f32, // For 16-byte alignment
    pub center_color: [f32; 4],
    pub edge_color: [f32; 4],
}

impl RadialGradientUniforms {
    pub fn new(
        center: (f32, f32),
        radius: f32,
        center_color: Color,
        edge_color: Color,
    ) -> Self {
        Self {
            center: [center.0, center.1],
            radius,
            _padding: 0.0,
            center_color: [
                f32::from(center_color.r) / 255.0,
                f32::from(center_color.g) / 255.0,
                f32::from(center_color.b) / 255.0,
                f32::from(center_color.a) / 255.0,
            ],
            edge_color: [
                f32::from(edge_color.r) / 255.0,
                f32::from(edge_color.g) / 255.0,
                f32::from(edge_color.b) / 255.0,
                f32::from(edge_color.a) / 255.0,
            ],
        }
    }
}

/// Create uniform buffer data from Shader
///
/// Uses `bytemuck` for safe type-to-bytes conversion without unsafe code.
/// Coordinates in the Shader are absolute (typed Pixels); this function
/// normalizes them relative to `bounds` for the GPU.
#[must_use]
pub fn create_uniforms_from_shader(
    shader: &Shader,
    bounds: flui_types::geometry::Rect<flui_types::geometry::Pixels>,
) -> Vec<u8> {
    match shader {
        Shader::Solid { color } => {
            let uniforms = SolidMaskUniforms::from_color(*color);
            bytemuck::bytes_of(&uniforms).to_vec()
        }
        Shader::LinearGradient { from, to, colors, .. } => {
            let w = bounds.width().0;
            let h = bounds.height().0;
            let bx = bounds.left().0;
            let by = bounds.top().0;
            let start = (
                if w > 0.0 { (from.dx.0 - bx) / w } else { 0.0 },
                if h > 0.0 { (from.dy.0 - by) / h } else { 0.0 },
            );
            let end = (
                if w > 0.0 { (to.dx.0 - bx) / w } else { 0.0 },
                if h > 0.0 { (to.dy.0 - by) / h } else { 0.0 },
            );
            let start_color = colors.first().copied().unwrap_or(Color::WHITE);
            let end_color = colors.last().copied().unwrap_or(Color::BLACK);
            let uniforms = LinearGradientUniforms::new(start, end, start_color, end_color);
            bytemuck::bytes_of(&uniforms).to_vec()
        }
        Shader::RadialGradient { center, radius, colors, .. } => {
            let w = bounds.width().0;
            let h = bounds.height().0;
            let bx = bounds.left().0;
            let by = bounds.top().0;
            let cx = if w > 0.0 { (center.dx.0 - bx) / w } else { 0.5 };
            let cy = if h > 0.0 { (center.dy.0 - by) / h } else { 0.5 };
            let avg = (w + h) / 2.0;
            let nr = if avg > 0.0 { *radius / avg } else { 0.5 };
            let center_color = colors.first().copied().unwrap_or(Color::WHITE);
            let edge_color = colors.last().copied().unwrap_or(Color::BLACK);
            let uniforms = RadialGradientUniforms::new((cx, cy), nr, center_color, edge_color);
            bytemuck::bytes_of(&uniforms).to_vec()
        }
        // Fallback for SweepGradient, Image, and any future variants
        _ => {
            let uniforms = SolidMaskUniforms::from_color(Color::WHITE);
            bytemuck::bytes_of(&uniforms).to_vec()
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
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

    #[test]
    fn test_solid_mask_uniforms() {
        let color = Color::rgba(255, 128, 64, 200);
        let uniforms = SolidMaskUniforms::from_color(color);

        // Test that conversion to normalized floats works
        assert!(uniforms.mask_color[0] >= 0.0 && uniforms.mask_color[0] <= 1.0); // R
        assert!(uniforms.mask_color[1] >= 0.0 && uniforms.mask_color[1] <= 1.0); // G
        assert!(uniforms.mask_color[2] >= 0.0 && uniforms.mask_color[2] <= 1.0); // B
        assert!(uniforms.mask_color[3] >= 0.0 && uniforms.mask_color[3] <= 1.0); // A

        // Also test with simple white color
        let white = Color::WHITE;
        let white_uniforms = SolidMaskUniforms::from_color(white);
        assert!((white_uniforms.mask_color[0] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[1] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[2] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_linear_gradient_uniforms() {
        let start_color = Color::RED;
        let end_color = Color::BLUE;
        let uniforms = LinearGradientUniforms::new((0.0, 0.0), (1.0, 1.0), start_color, end_color);

        assert_eq!(uniforms.start, [0.0, 0.0]);
        assert_eq!(uniforms.end, [1.0, 1.0]);
        assert_eq!(uniforms.start_color[0], 1.0); // Red
        assert_eq!(uniforms.end_color[2], 1.0); // Blue
    }

    #[test]
    fn test_radial_gradient_uniforms() {
        let center_color = Color::WHITE;
        let edge_color = Color::BLACK;
        let uniforms = RadialGradientUniforms::new((0.5, 0.5), 1.0, center_color, edge_color);

        assert_eq!(uniforms.center, [0.5, 0.5]);
        assert_eq!(uniforms.radius, 1.0);
    }

    #[test]
    fn test_create_uniforms_from_shader() {
        use flui_types::geometry::{Offset, Rect, px};

        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let solid = Shader::solid(Color::WHITE);
        let data = create_uniforms_from_shader(&solid, bounds);
        assert_eq!(data.len(), std::mem::size_of::<SolidMaskUniforms>());

        let linear = Shader::simple_linear(
            Offset::ZERO,
            Offset::new(px(100.0), px(100.0)),
            vec![Color::RED, Color::BLUE],
        );
        let data = create_uniforms_from_shader(&linear, bounds);
        assert_eq!(data.len(), std::mem::size_of::<LinearGradientUniforms>());

        let radial = Shader::simple_radial(
            Offset::new(px(50.0), px(50.0)),
            50.0,
            vec![Color::WHITE, Color::BLACK],
        );
        let data = create_uniforms_from_shader(&radial, bounds);
        assert_eq!(data.len(), std::mem::size_of::<RadialGradientUniforms>());
    }
}
