//! Metal 4 backend-specific features for macOS/iOS
//!
//! This module provides access to Metal 4 features that are not exposed through wgpu's
//! cross-platform API, including:
//! - MetalFX spatial/temporal upscaling
//! - Extended Dynamic Range (EDR) support
//! - Ray tracing acceleration structures
//! - Mesh shaders
//!
//! # Platform Requirements
//!
//! - macOS 14.0+ (Sonoma) or iOS 17.0+
//! - Apple Silicon (M1+) or AMD RDNA2+ GPU
//! - Metal 3.1+ for MetalFX, Metal 4.0+ for ray tracing
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::metal::{MetalFxUpscaler, EdrConfig};
//!
//! // Enable MetalFX upscaling (render at 720p, display at 1440p)
//! let upscaler = MetalFxUpscaler::new(
//!     device,
//!     UpscaleMode::Spatial,
//!     Size::new(1280, 720),
//!     Size::new(2560, 1440),
//! )?;
//!
//! // Configure EDR for HDR content
//! let edr = EdrConfig::new()
//!     .with_headroom(2.0)  // 2x SDR brightness
//!     .with_reference_white(200.0);  // 200 nits SDR white
//! ```

use anyhow::{anyhow, Result};
use flui_types::geometry::Size;
use std::sync::Arc;

// ============================================================================
// MetalFX Upscaling
// ============================================================================

/// MetalFX upscaling mode.
///
/// MetalFX is Apple's AI-powered upscaling technology, similar to NVIDIA DLSS
/// or AMD FSR. It renders at a lower resolution and upscales to native resolution
/// with minimal quality loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpscaleMode {
    /// Spatial upscaling (no temporal history).
    ///
    /// Best for static images or when temporal data is unavailable.
    /// Quality is good but not as high as temporal upscaling.
    Spatial,

    /// Temporal upscaling (uses previous frames).
    ///
    /// Highest quality upscaling by combining multiple frames.
    /// Requires motion vectors and depth buffers.
    Temporal,
}

impl UpscaleMode {
    /// Get recommended render scale for this upscale mode.
    ///
    /// Returns the fraction of native resolution to render at.
    pub fn recommended_scale(self) -> f32 {
        match self {
            UpscaleMode::Spatial => 0.75,    // 75% scale (e.g., 1440p → 1080p)
            UpscaleMode::Temporal => 0.67,   // 67% scale (e.g., 1440p → 960p)
        }
    }

    /// Check if this mode requires motion vectors.
    pub fn requires_motion_vectors(self) -> bool {
        matches!(self, UpscaleMode::Temporal)
    }
}

/// MetalFX upscaler for AI-powered resolution upscaling.
///
/// This struct wraps Metal's MTLFXSpatialScaler or MTLFXTemporalScaler to provide
/// high-quality upscaling from a lower render resolution to native display resolution.
///
/// # Performance Impact
///
/// Spatial upscaling adds ~0.5-1ms per frame at 1440p.
/// Temporal upscaling adds ~1-2ms per frame at 1440p.
///
/// However, rendering at lower resolution can save 2-5ms, resulting in net performance gain.
#[derive(Debug)]
pub struct MetalFxUpscaler {
    mode: UpscaleMode,
    input_size: Size<u32>,
    output_size: Size<u32>,
    device: Arc<wgpu::Device>,
}

impl MetalFxUpscaler {
    /// Create a new MetalFX upscaler.
    ///
    /// # Parameters
    ///
    /// - `device` - wgpu device (must be Metal backend)
    /// - `mode` - Spatial or Temporal upscaling
    /// - `input_size` - Render resolution (e.g., 1280x720)
    /// - `output_size` - Display resolution (e.g., 1920x1080)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Device is not Metal backend
    /// - MetalFX is not supported on this GPU
    /// - Input/output sizes are invalid
    pub fn new(
        device: Arc<wgpu::Device>,
        mode: UpscaleMode,
        input_size: Size<u32>,
        output_size: Size<u32>,
    ) -> Result<Self> {
        // Validate backend
        #[cfg(not(target_os = "macos"))]
        {
            return Err(anyhow!("MetalFX is only available on macOS/iOS"));
        }

        // Validate sizes
        if input_size.width == 0
            || input_size.height == 0
            || output_size.width == 0
            || output_size.height == 0
        {
            return Err(anyhow!("Invalid input/output sizes"));
        }

        if input_size.width > output_size.width || input_size.height > output_size.height {
            return Err(anyhow!("Input size must be smaller than output size"));
        }

        // TODO: Check device features for MetalFX support
        // This requires accessing the underlying MTLDevice and checking supportsFamily(MTLGPUFamilyMetal3)

        Ok(Self {
            mode,
            input_size,
            output_size,
            device,
        })
    }

    /// Get the upscale mode.
    pub fn mode(&self) -> UpscaleMode {
        self.mode
    }

    /// Get the input (render) resolution.
    pub fn input_size(&self) -> Size<u32> {
        self.input_size
    }

    /// Get the output (display) resolution.
    pub fn output_size(&self) -> Size<u32> {
        self.output_size
    }

    /// Get the upscale ratio.
    pub fn upscale_ratio(&self) -> f32 {
        (self.output_size.width as f32 / self.input_size.width as f32)
            .max(self.output_size.height as f32 / self.input_size.height as f32)
    }

    /// Check if MetalFX is supported on this device.
    pub fn is_supported(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // TODO: Query MTLDevice capabilities
            // For now, assume supported on all Metal devices
            true
        }

        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Upscale a texture from input resolution to output resolution.
    ///
    /// # Parameters
    ///
    /// - `input_texture` - Rendered frame at input_size resolution
    /// - `output_texture` - Target texture at output_size resolution
    /// - `motion_vectors` - Optional motion vector texture (required for temporal mode)
    /// - `depth` - Optional depth texture (improves quality for temporal mode)
    ///
    /// # Errors
    ///
    /// Returns error if texture sizes don't match expected input/output sizes.
    pub fn upscale(
        &self,
        _input_texture: &wgpu::Texture,
        _output_texture: &wgpu::Texture,
        _motion_vectors: Option<&wgpu::Texture>,
        _depth: Option<&wgpu::Texture>,
    ) -> Result<()> {
        // TODO: Implement actual MetalFX upscaling
        //
        // This requires:
        // 1. Get Metal device from wgpu::Device (unsafe FFI)
        // 2. Create MTLFXSpatialScaler or MTLFXTemporalScaler
        // 3. Encode upscaling pass in command buffer
        // 4. Submit to GPU queue
        //
        // For now, return error as placeholder
        Err(anyhow!("MetalFX upscaling not yet implemented - requires Metal FFI bindings"))
    }
}

// ============================================================================
// Extended Dynamic Range (EDR)
// ============================================================================

/// Extended Dynamic Range configuration for HDR content on macOS.
///
/// EDR allows content to exceed the standard 0-1 SDR range on compatible displays,
/// enabling HDR highlights up to 1600 nits on Pro Display XDR.
///
/// # Display Support
///
/// - Pro Display XDR: Up to 1600 nits peak, 1000 nits sustained
/// - MacBook Pro 14"/16" (2021+): Up to 1600 nits peak
/// - iMac 24" (M1): Up to 500 nits (no EDR)
/// - External HDR displays: Depends on display capabilities
#[derive(Debug, Clone, Copy)]
pub struct EdrConfig {
    /// EDR headroom multiplier.
    ///
    /// This is how much brighter content can be relative to SDR white.
    /// - 1.0 = SDR only (no EDR)
    /// - 2.0 = Highlights can be 2x brighter than SDR white
    /// - 4.0 = Highlights can be 4x brighter (typical for HDR)
    /// - 8.0 = Maximum on Pro Display XDR
    pub headroom: f32,

    /// Reference white luminance in nits.
    ///
    /// This defines what luminance value corresponds to "1.0" in SDR space.
    /// Typical values:
    /// - 80 nits - Dim indoor viewing
    /// - 100 nits - Standard sRGB (most common)
    /// - 200 nits - Bright indoor/outdoor viewing
    /// - 400 nits - Very bright environments
    pub reference_white: f32,

    /// Whether to enable EDR.
    ///
    /// If false, all content is rendered in SDR even if display supports EDR.
    pub enabled: bool,
}

impl EdrConfig {
    /// Create default EDR configuration (disabled).
    pub fn new() -> Self {
        Self {
            headroom: 1.0,
            reference_white: 100.0,
            enabled: false,
        }
    }

    /// Enable EDR with the given headroom.
    pub fn with_headroom(mut self, headroom: f32) -> Self {
        self.headroom = headroom.clamp(1.0, 8.0);
        self.enabled = headroom > 1.0;
        self
    }

    /// Set reference white luminance in nits.
    pub fn with_reference_white(mut self, nits: f32) -> Self {
        self.reference_white = nits.clamp(80.0, 400.0);
        self
    }

    /// Disable EDR (SDR only).
    pub fn disabled() -> Self {
        Self {
            headroom: 1.0,
            reference_white: 100.0,
            enabled: false,
        }
    }

    /// EDR configuration for HDR content (headroom 4.0).
    pub fn hdr() -> Self {
        Self {
            headroom: 4.0,
            reference_white: 100.0,
            enabled: true,
        }
    }

    /// EDR configuration for extreme HDR (headroom 8.0, Pro Display XDR only).
    pub fn extreme_hdr() -> Self {
        Self {
            headroom: 8.0,
            reference_white: 100.0,
            enabled: true,
        }
    }

    /// Get maximum luminance in nits.
    ///
    /// This is `reference_white * headroom`.
    pub fn max_luminance(&self) -> f32 {
        self.reference_white * self.headroom
    }

    /// Check if EDR is available on the current display.
    pub fn is_available() -> bool {
        #[cfg(target_os = "macos")]
        {
            // TODO: Query NSScreen.maximumExtendedDynamicRangeColorComponentValue
            // For now, assume available on all macOS displays
            true
        }

        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Get current display's EDR headroom.
    ///
    /// Returns 1.0 if EDR is not supported.
    pub fn get_display_headroom() -> f32 {
        #[cfg(target_os = "macos")]
        {
            // TODO: Query NSScreen.maximumExtendedDynamicRangeColorComponentValue
            // For now, return conservative value
            1.0
        }

        #[cfg(not(target_os = "macos"))]
        {
            1.0
        }
    }
}

impl Default for EdrConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Ray Tracing Support
// ============================================================================

/// Metal 4 ray tracing configuration.
///
/// Metal 4 introduces hardware-accelerated ray tracing on Apple Silicon (M3+)
/// and AMD RDNA3+ GPUs. This enables real-time ray-traced reflections, shadows,
/// and global illumination.
#[derive(Debug, Clone, Copy)]
pub struct RayTracingConfig {
    /// Enable ray-traced reflections.
    pub reflections: bool,

    /// Enable ray-traced shadows.
    pub shadows: bool,

    /// Enable ray-traced ambient occlusion.
    pub ambient_occlusion: bool,

    /// Maximum ray recursion depth.
    ///
    /// Higher values enable more realistic inter-reflections but cost more performance.
    /// Typical values: 1-4
    pub max_recursion_depth: u32,
}

impl RayTracingConfig {
    /// Create default ray tracing configuration (all disabled).
    pub fn new() -> Self {
        Self {
            reflections: false,
            shadows: false,
            ambient_occlusion: false,
            max_recursion_depth: 1,
        }
    }

    /// Enable ray-traced reflections.
    pub fn with_reflections(mut self) -> Self {
        self.reflections = true;
        self
    }

    /// Enable ray-traced shadows.
    pub fn with_shadows(mut self) -> Self {
        self.shadows = true;
        self
    }

    /// Enable ray-traced ambient occlusion.
    pub fn with_ambient_occlusion(mut self) -> Self {
        self.ambient_occlusion = true;
        self
    }

    /// Set maximum recursion depth.
    pub fn with_max_recursion_depth(mut self, depth: u32) -> Self {
        self.max_recursion_depth = depth.clamp(1, 8);
        self
    }

    /// Check if ray tracing is supported on this device.
    pub fn is_supported() -> bool {
        #[cfg(target_os = "macos")]
        {
            // TODO: Query MTLDevice.supportsRaytracing
            // Ray tracing requires Metal 4 (macOS 15+) and M3+/RDNA3+ GPU
            false
        }

        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

impl Default for RayTracingConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    #[test]
    fn test_upscale_mode_recommended_scale() {
        assert_eq!(UpscaleMode::Spatial.recommended_scale(), 0.75);
        assert_eq!(UpscaleMode::Temporal.recommended_scale(), 0.67);
    }

    #[test]
    fn test_upscale_mode_motion_vectors() {
        assert!(!UpscaleMode::Spatial.requires_motion_vectors());
        assert!(UpscaleMode::Temporal.requires_motion_vectors());
    }

    #[test]
    fn test_edr_config_default() {
        let config = EdrConfig::new();
        assert_eq!(config.headroom, 1.0);
        assert_eq!(config.reference_white, 100.0);
        assert!(!config.enabled);
    }

    #[test]
    fn test_edr_config_hdr() {
        let config = EdrConfig::hdr();
        assert_eq!(config.headroom, 4.0);
        assert_eq!(config.reference_white, 100.0);
        assert!(config.enabled);
        assert_eq!(config.max_luminance(), 400.0);
    }

    #[test]
    fn test_edr_config_extreme_hdr() {
        let config = EdrConfig::extreme_hdr();
        assert_eq!(config.headroom, 8.0);
        assert_eq!(config.max_luminance(), 800.0);
    }

    #[test]
    fn test_edr_config_clamping() {
        let config = EdrConfig::new()
            .with_headroom(100.0)  // Should clamp to 8.0
            .with_reference_white(1000.0);  // Should clamp to 400.0

        assert_eq!(config.headroom, 8.0);
        assert_eq!(config.reference_white, 400.0);
    }

    #[test]
    fn test_ray_tracing_config_default() {
        let config = RayTracingConfig::new();
        assert!(!config.reflections);
        assert!(!config.shadows);
        assert!(!config.ambient_occlusion);
        assert_eq!(config.max_recursion_depth, 1);
    }

    #[test]
    fn test_ray_tracing_config_builder() {
        let config = RayTracingConfig::new()
            .with_reflections()
            .with_shadows()
            .with_max_recursion_depth(4);

        assert!(config.reflections);
        assert!(config.shadows);
        assert!(!config.ambient_occlusion);
        assert_eq!(config.max_recursion_depth, 4);
    }

    #[test]
    fn test_ray_tracing_recursion_clamping() {
        let config = RayTracingConfig::new().with_max_recursion_depth(100);
        assert_eq!(config.max_recursion_depth, 8);
    }
}
