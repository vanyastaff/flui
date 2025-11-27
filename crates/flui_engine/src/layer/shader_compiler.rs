// Shader compilation and caching for shader mask effects
//!
//! This module provides shader compilation, caching, and uniform buffer management
//! for ShaderMaskLayer rendering.

use flui_types::painting::ShaderSpec;
use flui_types::styling::Color32;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
            ShaderType::SolidMask => include_str!("shaders/solid_mask.wgsl"),
            ShaderType::LinearGradientMask => include_str!("shaders/linear_gradient_mask.wgsl"),
            ShaderType::RadialGradientMask => include_str!("shaders/radial_gradient_mask.wgsl"),
            ShaderType::GaussianBlurHorizontal => include_str!("shaders/gaussian_blur_horizontal.wgsl"),
            ShaderType::GaussianBlurVertical => include_str!("shaders/gaussian_blur_vertical.wgsl"),
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

    /// Get the shader type from a ShaderSpec
    pub fn from_spec(spec: &ShaderSpec) -> Self {
        match spec {
            ShaderSpec::Solid(_) => ShaderType::SolidMask,
            ShaderSpec::LinearGradient { .. } => ShaderType::LinearGradientMask,
            ShaderSpec::RadialGradient { .. } => ShaderType::RadialGradientMask,
        }
    }
}

/// Compiled shader module (placeholder for wgpu::ShaderModule)
///
/// In full implementation, this would hold the actual wgpu::ShaderModule.
/// For now, it's a placeholder to establish the API.
#[derive(Debug, Clone)]
pub struct CompiledShader {
    pub shader_type: ShaderType,
    pub source: String,
    // TODO: Add actual wgpu::ShaderModule when integrating with renderer
    // pub module: Arc<wgpu::ShaderModule>,
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
    pub fn get_or_compile(&self, shader_type: ShaderType) -> Arc<CompiledShader> {
        // Try to get from cache first (read lock)
        {
            let cache = self.cache.read().unwrap();
            if let Some(shader) = cache.get(&shader_type) {
                tracing::debug!("Shader cache hit: {:?}", shader_type);
                return Arc::clone(shader);
            }
        }

        // Not in cache, compile it (write lock)
        let mut cache = self.cache.write().unwrap();

        // Double-check in case another thread compiled it while we waited for the lock
        if let Some(shader) = cache.get(&shader_type) {
            return Arc::clone(shader);
        }

        // Compile the shader
        tracing::info!("Compiling shader: {}", shader_type.label());
        let compiled = Arc::new(CompiledShader {
            shader_type,
            source: shader_type.source_code().to_string(),
            // TODO: Create actual wgpu::ShaderModule here
        });

        cache.insert(shader_type, Arc::clone(&compiled));
        compiled
    }

    /// Pre-compile all shaders
    ///
    /// Useful for avoiding frame time spikes on first use.
    pub fn precompile_all(&self) {
        tracing::info!("Pre-compiling all shader mask shaders");
        self.get_or_compile(ShaderType::SolidMask);
        self.get_or_compile(ShaderType::LinearGradientMask);
        self.get_or_compile(ShaderType::RadialGradientMask);
        tracing::info!("All shaders pre-compiled");
    }

    /// Clear the cache
    pub fn clear(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        tracing::info!("Shader cache cleared");
    }
}

impl Default for ShaderCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Uniform data for solid mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SolidMaskUniforms {
    pub mask_color: [f32; 4], // RGBA
}

impl SolidMaskUniforms {
    pub fn from_color(color: Color32) -> Self {
        Self {
            mask_color: [
                color.r() as f32 / 255.0,
                color.g() as f32 / 255.0,
                color.b() as f32 / 255.0,
                color.a() as f32 / 255.0,
            ],
        }
    }
}

/// Uniform data for linear gradient mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy)]
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
        start_color: Color32,
        end_color: Color32,
    ) -> Self {
        Self {
            start: [start.0, start.1],
            end: [end.0, end.1],
            start_color: [
                start_color.r() as f32 / 255.0,
                start_color.g() as f32 / 255.0,
                start_color.b() as f32 / 255.0,
                start_color.a() as f32 / 255.0,
            ],
            end_color: [
                end_color.r() as f32 / 255.0,
                end_color.g() as f32 / 255.0,
                end_color.b() as f32 / 255.0,
                end_color.a() as f32 / 255.0,
            ],
        }
    }
}

/// Uniform data for radial gradient mask shader
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RadialGradientUniforms {
    pub center: [f32; 2],
    pub radius: f32,
    pub _padding: f32, // For 16-byte alignment
    pub center_color: [f32; 4],
    pub edge_color: [f32; 4],
}

impl RadialGradientUniforms {
    pub fn new(center: (f32, f32), radius: f32, center_color: Color32, edge_color: Color32) -> Self {
        Self {
            center: [center.0, center.1],
            radius,
            _padding: 0.0,
            center_color: [
                center_color.r() as f32 / 255.0,
                center_color.g() as f32 / 255.0,
                center_color.b() as f32 / 255.0,
                center_color.a() as f32 / 255.0,
            ],
            edge_color: [
                edge_color.r() as f32 / 255.0,
                edge_color.g() as f32 / 255.0,
                edge_color.b() as f32 / 255.0,
                edge_color.a() as f32 / 255.0,
            ],
        }
    }
}

/// Create uniform buffer data from ShaderSpec
pub fn create_uniforms_from_spec(spec: &ShaderSpec) -> Vec<u8> {
    match spec {
        ShaderSpec::Solid(color) => {
            let uniforms = SolidMaskUniforms::from_color(*color);
            unsafe {
                std::slice::from_raw_parts(
                    &uniforms as *const _ as *const u8,
                    std::mem::size_of::<SolidMaskUniforms>(),
                )
                .to_vec()
            }
        }
        ShaderSpec::LinearGradient {
            start,
            end,
            colors,
        } => {
            let start_color = colors.first().copied().unwrap_or(Color32::WHITE);
            let end_color = colors.last().copied().unwrap_or(Color32::BLACK);
            let uniforms = LinearGradientUniforms::new(*start, *end, start_color, end_color);
            unsafe {
                std::slice::from_raw_parts(
                    &uniforms as *const _ as *const u8,
                    std::mem::size_of::<LinearGradientUniforms>(),
                )
                .to_vec()
            }
        }
        ShaderSpec::RadialGradient {
            center,
            radius,
            colors,
        } => {
            let center_color = colors.first().copied().unwrap_or(Color32::WHITE);
            let edge_color = colors.last().copied().unwrap_or(Color32::BLACK);
            let uniforms = RadialGradientUniforms::new(*center, *radius, center_color, edge_color);
            unsafe {
                std::slice::from_raw_parts(
                    &uniforms as *const _ as *const u8,
                    std::mem::size_of::<RadialGradientUniforms>(),
                )
                .to_vec()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_type_from_spec() {
        let solid = ShaderSpec::Solid(Color32::WHITE);
        assert_eq!(ShaderType::from_spec(&solid), ShaderType::SolidMask);

        let linear = ShaderSpec::LinearGradient {
            start: (0.0, 0.0),
            end: (1.0, 1.0),
            colors: vec![Color32::RED, Color32::BLUE],
        };
        assert_eq!(
            ShaderType::from_spec(&linear),
            ShaderType::LinearGradientMask
        );

        let radial = ShaderSpec::RadialGradient {
            center: (0.5, 0.5),
            radius: 1.0,
            colors: vec![Color32::WHITE, Color32::BLACK],
        };
        assert_eq!(
            ShaderType::from_spec(&radial),
            ShaderType::RadialGradientMask
        );
    }

    #[test]
    fn test_shader_source_code() {
        assert!(ShaderType::SolidMask.source_code().contains("Solid Color Mask"));
        assert!(ShaderType::LinearGradientMask
            .source_code()
            .contains("Linear Gradient"));
        assert!(ShaderType::RadialGradientMask
            .source_code()
            .contains("Radial Gradient"));
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
        let color = Color32::from_rgba_unmultiplied(255, 128, 64, 200);
        let uniforms = SolidMaskUniforms::from_color(color);

        // Test that conversion to normalized floats works
        assert!(uniforms.mask_color[0] >= 0.0 && uniforms.mask_color[0] <= 1.0); // R
        assert!(uniforms.mask_color[1] >= 0.0 && uniforms.mask_color[1] <= 1.0); // G
        assert!(uniforms.mask_color[2] >= 0.0 && uniforms.mask_color[2] <= 1.0); // B
        assert!(uniforms.mask_color[3] >= 0.0 && uniforms.mask_color[3] <= 1.0); // A

        // Also test with simple white color
        let white = Color32::WHITE;
        let white_uniforms = SolidMaskUniforms::from_color(white);
        assert!((white_uniforms.mask_color[0] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[1] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[2] - 1.0).abs() < 0.01);
        assert!((white_uniforms.mask_color[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_linear_gradient_uniforms() {
        let start_color = Color32::RED;
        let end_color = Color32::BLUE;
        let uniforms =
            LinearGradientUniforms::new((0.0, 0.0), (1.0, 1.0), start_color, end_color);

        assert_eq!(uniforms.start, [0.0, 0.0]);
        assert_eq!(uniforms.end, [1.0, 1.0]);
        assert_eq!(uniforms.start_color[0], 1.0); // Red
        assert_eq!(uniforms.end_color[2], 1.0); // Blue
    }

    #[test]
    fn test_radial_gradient_uniforms() {
        let center_color = Color32::WHITE;
        let edge_color = Color32::BLACK;
        let uniforms = RadialGradientUniforms::new((0.5, 0.5), 1.0, center_color, edge_color);

        assert_eq!(uniforms.center, [0.5, 0.5]);
        assert_eq!(uniforms.radius, 1.0);
    }

    #[test]
    fn test_create_uniforms_from_spec() {
        let solid = ShaderSpec::Solid(Color32::WHITE);
        let data = create_uniforms_from_spec(&solid);
        assert_eq!(data.len(), std::mem::size_of::<SolidMaskUniforms>());

        let linear = ShaderSpec::LinearGradient {
            start: (0.0, 0.0),
            end: (1.0, 1.0),
            colors: vec![Color32::RED, Color32::BLUE],
        };
        let data = create_uniforms_from_spec(&linear);
        assert_eq!(data.len(), std::mem::size_of::<LinearGradientUniforms>());

        let radial = ShaderSpec::RadialGradient {
            center: (0.5, 0.5),
            radius: 1.0,
            colors: vec![Color32::WHITE, Color32::BLACK],
        };
        let data = create_uniforms_from_spec(&radial);
        assert_eq!(data.len(), std::mem::size_of::<RadialGradientUniforms>());
    }
}
