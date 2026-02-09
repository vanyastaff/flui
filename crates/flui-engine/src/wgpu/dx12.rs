//! DirectX 12 backend-specific features for Windows
//!
//! This module provides access to DirectX 12 features that are not exposed through wgpu's
//! cross-platform API, including:
//! - Work Graphs (DX12 Ultimate)
//! - Shader Execution Reordering (SER)
//! - Auto HDR configuration
//! - DirectStorage for fast asset loading
//! - Variable Rate Shading (VRS)
//!
//! # Platform Requirements
//!
//! - Windows 10 20H1+ (build 19041) or Windows 11
//! - DirectX 12 Ultimate GPU (NVIDIA RTX 20xx+, AMD RDNA2+, Intel Arc)
//! - WDDM 2.7+ driver
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::dx12::{Dx12Features, AutoHdrConfig};
//!
//! // Check DX12 Ultimate support
//! let features = Dx12Features::detect(device)?;
//! if features.supports_work_graphs {
//!     // Enable Work Graphs for GPU-driven rendering
//! }
//!
//! // Configure Auto HDR
//! let hdr = AutoHdrConfig::new()
//!     .with_enabled(true)
//!     .with_target_luminance(1000.0);  // 1000 nits
//! ```

#[cfg(not(target_os = "windows"))]
use anyhow::anyhow;
use anyhow::Result;

// ============================================================================
// DirectX 12 Feature Detection
// ============================================================================

/// DirectX 12 feature set and capabilities.
///
/// This struct contains information about which DX12 features are available
/// on the current GPU and driver.
#[derive(Debug, Clone)]
pub struct Dx12Features {
    /// DirectX feature level (e.g., 12_0, 12_1, 12_2).
    pub feature_level: Dx12FeatureLevel,

    /// Supports Work Graphs (DX12 Ultimate, requires NVIDIA Ada/AMD RDNA3+).
    pub supports_work_graphs: bool,

    /// Supports Shader Execution Reordering (SER) for ray tracing.
    pub supports_ser: bool,

    /// Supports Variable Rate Shading (VRS) Tier 1 or higher.
    pub supports_vrs: bool,

    /// Variable Rate Shading tier (None, Tier1, Tier2).
    pub vrs_tier: VrsTier,

    /// Supports Mesh Shaders (DX12 Ultimate).
    pub supports_mesh_shaders: bool,

    /// Supports Sampler Feedback (DX12 Ultimate).
    pub supports_sampler_feedback: bool,

    /// Supports DirectStorage for fast SSD asset loading.
    pub supports_direct_storage: bool,

    /// Auto HDR is enabled at system level.
    pub auto_hdr_enabled: bool,

    /// Maximum shader model version (e.g., 6.6, 6.7).
    pub shader_model: ShaderModel,
}

impl Dx12Features {
    /// Detect DirectX 12 features from a wgpu device.
    ///
    /// # Errors
    ///
    /// Returns error if device is not DirectX 12 backend.
    pub fn detect(_device: &wgpu::Device) -> Result<Self> {
        #[cfg(not(target_os = "windows"))]
        {
            return Err(anyhow!("DirectX 12 is only available on Windows"));
        }

        // TODO: Query actual D3D12 device capabilities via FFI
        // For now, return conservative defaults
        Ok(Self {
            feature_level: Dx12FeatureLevel::Level12_0,
            supports_work_graphs: false,
            supports_ser: false,
            supports_vrs: false,
            vrs_tier: VrsTier::NotSupported,
            supports_mesh_shaders: false,
            supports_sampler_feedback: false,
            supports_direct_storage: false,
            auto_hdr_enabled: false,
            shader_model: ShaderModel::SM6_0,
        })
    }

    /// Check if GPU supports DX12 Ultimate (all advanced features).
    pub fn supports_dx12_ultimate(&self) -> bool {
        self.feature_level >= Dx12FeatureLevel::Level12_2
            && self.supports_mesh_shaders
            && self.supports_vrs
            && self.supports_sampler_feedback
    }

    /// Get a human-readable description of the feature level.
    pub fn feature_level_name(&self) -> &str {
        match self.feature_level {
            Dx12FeatureLevel::Level12_0 => "DirectX 12.0",
            Dx12FeatureLevel::Level12_1 => "DirectX 12.1",
            Dx12FeatureLevel::Level12_2 => "DirectX 12.2 (Ultimate)",
        }
    }
}

/// DirectX 12 feature level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dx12FeatureLevel {
    /// DirectX 12.0 (Windows 10 1507+)
    Level12_0,
    /// DirectX 12.1 (Windows 10 1607+)
    Level12_1,
    /// DirectX 12.2 / Ultimate (Windows 10 2004+)
    Level12_2,
}

/// Variable Rate Shading tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrsTier {
    /// VRS not supported.
    NotSupported,
    /// Tier 1: Per-draw shading rate.
    Tier1,
    /// Tier 2: Per-draw + screen-space image shading rate.
    Tier2,
}

/// Shader Model version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderModel {
    /// Shader Model 6.0 (minimum for DX12)
    SM6_0,
    /// Shader Model 6.1
    SM6_1,
    /// Shader Model 6.2
    SM6_2,
    /// Shader Model 6.3
    SM6_3,
    /// Shader Model 6.4
    SM6_4,
    /// Shader Model 6.5
    SM6_5,
    /// Shader Model 6.6
    SM6_6,
    /// Shader Model 6.7 (Work Graphs)
    SM6_7,
    /// Shader Model 6.8 (latest)
    SM6_8,
}

// ============================================================================
// Work Graphs
// ============================================================================

/// Work Graphs configuration for GPU-driven rendering.
///
/// Work Graphs (Shader Model 6.7+) enable GPU-driven rendering pipelines where
/// the GPU can schedule its own work without CPU intervention. This is useful for:
/// - GPU culling and LOD selection
/// - Particle systems
/// - Procedural generation
/// - Complex rendering graphs
///
/// Requires NVIDIA Ada (RTX 40xx+) or AMD RDNA3 (RX 7xxx+) GPUs.
#[derive(Debug, Clone)]
pub struct WorkGraphsConfig {
    /// Enable Work Graphs.
    pub enabled: bool,

    /// Maximum node count in work graph.
    pub max_nodes: u32,

    /// Maximum recursion depth.
    pub max_recursion_depth: u32,
}

impl WorkGraphsConfig {
    /// Create default Work Graphs configuration (disabled).
    pub fn new() -> Self {
        Self {
            enabled: false,
            max_nodes: 256,
            max_recursion_depth: 4,
        }
    }

    /// Enable Work Graphs.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            max_nodes: 256,
            max_recursion_depth: 4,
        }
    }

    /// Set maximum node count.
    pub fn with_max_nodes(mut self, max_nodes: u32) -> Self {
        self.max_nodes = max_nodes.clamp(1, 4096);
        self
    }

    /// Set maximum recursion depth.
    pub fn with_max_recursion_depth(mut self, depth: u32) -> Self {
        self.max_recursion_depth = depth.clamp(1, 32);
        self
    }
}

impl Default for WorkGraphsConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Shader Execution Reordering (SER)
// ============================================================================

/// Shader Execution Reordering configuration for ray tracing.
///
/// SER improves ray tracing performance by reordering shader threads to improve
/// coherence. This can provide 2-3x speedup for ray tracing workloads.
///
/// Requires NVIDIA Ada (RTX 40xx+) GPUs.
#[derive(Debug, Clone, Copy)]
pub struct SerConfig {
    /// Enable Shader Execution Reordering.
    pub enabled: bool,

    /// Reordering mode (coarse vs fine-grained).
    pub mode: SerMode,
}

impl SerConfig {
    /// Create default SER configuration (disabled).
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: SerMode::Coarse,
        }
    }

    /// Enable SER with coarse-grained reordering.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            mode: SerMode::Coarse,
        }
    }

    /// Set reordering mode.
    pub fn with_mode(mut self, mode: SerMode) -> Self {
        self.mode = mode;
        self
    }
}

impl Default for SerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// SER reordering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SerMode {
    /// Coarse-grained reordering (faster, less coherence improvement).
    Coarse,
    /// Fine-grained reordering (slower, better coherence improvement).
    Fine,
}

// ============================================================================
// Auto HDR
// ============================================================================

/// Auto HDR configuration for Windows 11.
///
/// Auto HDR automatically converts SDR content to HDR on compatible displays.
/// This is a system-level feature that works with all DirectX 11/12 games.
///
/// # Display Support
///
/// Requires Windows 11 and an HDR-capable display (HDR10, DisplayHDR 400+).
#[derive(Debug, Clone, Copy)]
pub struct AutoHdrConfig {
    /// Enable Auto HDR (requires system support).
    pub enabled: bool,

    /// Target peak luminance in nits (100-10000).
    pub target_luminance: f32,

    /// HDR metadata signaling.
    pub metadata: HdrMetadata,
}

impl AutoHdrConfig {
    /// Create default Auto HDR configuration (disabled).
    pub fn new() -> Self {
        Self {
            enabled: false,
            target_luminance: 1000.0,
            metadata: HdrMetadata::default(),
        }
    }

    /// Enable Auto HDR.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            target_luminance: 1000.0,
            metadata: HdrMetadata::default(),
        }
    }

    /// Set target luminance in nits.
    pub fn with_target_luminance(mut self, nits: f32) -> Self {
        self.target_luminance = nits.clamp(100.0, 10000.0);
        self
    }

    /// Set HDR metadata.
    pub fn with_metadata(mut self, metadata: HdrMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Check if Auto HDR is available on this system.
    pub fn is_available() -> bool {
        #[cfg(target_os = "windows")]
        {
            // TODO: Query Windows version and display capabilities
            // For now, assume available on Windows 11+
            false
        }

        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }
}

impl Default for AutoHdrConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// HDR metadata for content signaling.
#[derive(Debug, Clone, Copy)]
pub struct HdrMetadata {
    /// Maximum content light level in nits.
    pub max_cll: f32,

    /// Maximum frame-average light level in nits.
    pub max_fall: f32,

    /// Display's minimum luminance in nits.
    pub min_luminance: f32,

    /// Display's maximum luminance in nits.
    pub max_luminance: f32,
}

impl Default for HdrMetadata {
    fn default() -> Self {
        Self {
            max_cll: 1000.0,
            max_fall: 400.0,
            min_luminance: 0.001,
            max_luminance: 1000.0,
        }
    }
}

// ============================================================================
// Variable Rate Shading
// ============================================================================

/// Variable Rate Shading configuration.
///
/// VRS allows rendering different parts of the screen at different rates,
/// improving performance by reducing shading in less important areas (e.g., periphery).
#[derive(Debug, Clone)]
pub struct VrsConfig {
    /// Enable Variable Rate Shading.
    pub enabled: bool,

    /// VRS tier to use (Tier1 or Tier2).
    pub tier: VrsTier,

    /// Shading rate for different screen regions.
    pub shading_rate: ShadingRate,
}

impl VrsConfig {
    /// Create default VRS configuration (disabled).
    pub fn new() -> Self {
        Self {
            enabled: false,
            tier: VrsTier::NotSupported,
            shading_rate: ShadingRate::Rate1x1,
        }
    }

    /// Enable VRS with the given tier.
    pub fn with_tier(mut self, tier: VrsTier) -> Self {
        self.tier = tier;
        self.enabled = tier != VrsTier::NotSupported;
        self
    }

    /// Set shading rate.
    pub fn with_shading_rate(mut self, rate: ShadingRate) -> Self {
        self.shading_rate = rate;
        self
    }
}

impl Default for VrsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Shading rate for Variable Rate Shading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadingRate {
    /// 1 shading sample per pixel (no VRS).
    Rate1x1,
    /// 1 shading sample per 2x1 pixels.
    Rate1x2,
    /// 1 shading sample per 2x2 pixels.
    Rate2x2,
    /// 1 shading sample per 2x4 pixels.
    Rate2x4,
    /// 1 shading sample per 4x4 pixels.
    Rate4x4,
}

// ============================================================================
// DirectStorage
// ============================================================================

/// DirectStorage configuration for fast SSD asset loading.
///
/// DirectStorage enables GPU-direct I/O from NVMe SSDs, bypassing CPU decompression
/// and memory copies. This can provide 2-3x faster asset loading.
///
/// Requires Windows 10 2004+ and NVMe SSD.
#[derive(Debug, Clone)]
pub struct DirectStorageConfig {
    /// Enable DirectStorage.
    pub enabled: bool,

    /// Use GPU decompression (requires compatible GPU).
    pub gpu_decompression: bool,

    /// Queue depth for I/O operations.
    pub queue_depth: u32,
}

impl DirectStorageConfig {
    /// Create default DirectStorage configuration (disabled).
    pub fn new() -> Self {
        Self {
            enabled: false,
            gpu_decompression: false,
            queue_depth: 32,
        }
    }

    /// Enable DirectStorage.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            gpu_decompression: true,
            queue_depth: 32,
        }
    }

    /// Enable GPU decompression.
    pub fn with_gpu_decompression(mut self, enabled: bool) -> Self {
        self.gpu_decompression = enabled;
        self
    }

    /// Set queue depth.
    pub fn with_queue_depth(mut self, depth: u32) -> Self {
        self.queue_depth = depth.clamp(1, 256);
        self
    }

    /// Check if DirectStorage is available.
    pub fn is_available() -> bool {
        #[cfg(target_os = "windows")]
        {
            // TODO: Query Windows version and DirectStorage runtime
            false
        }

        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }
}

impl Default for DirectStorageConfig {
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
    fn test_feature_level_ordering() {
        assert!(Dx12FeatureLevel::Level12_2 > Dx12FeatureLevel::Level12_1);
        assert!(Dx12FeatureLevel::Level12_1 > Dx12FeatureLevel::Level12_0);
    }

    #[test]
    fn test_shader_model_ordering() {
        assert!(ShaderModel::SM6_8 > ShaderModel::SM6_7);
        assert!(ShaderModel::SM6_0 < ShaderModel::SM6_6);
    }

    #[test]
    fn test_work_graphs_config_default() {
        let config = WorkGraphsConfig::new();
        assert!(!config.enabled);
        assert_eq!(config.max_nodes, 256);
        assert_eq!(config.max_recursion_depth, 4);
    }

    #[test]
    fn test_work_graphs_config_enabled() {
        let config = WorkGraphsConfig::enabled()
            .with_max_nodes(512)
            .with_max_recursion_depth(8);

        assert!(config.enabled);
        assert_eq!(config.max_nodes, 512);
        assert_eq!(config.max_recursion_depth, 8);
    }

    #[test]
    fn test_work_graphs_clamping() {
        let config = WorkGraphsConfig::enabled()
            .with_max_nodes(10000)  // Should clamp to 4096
            .with_max_recursion_depth(100); // Should clamp to 32

        assert_eq!(config.max_nodes, 4096);
        assert_eq!(config.max_recursion_depth, 32);
    }

    #[test]
    fn test_ser_config_default() {
        let config = SerConfig::new();
        assert!(!config.enabled);
        assert_eq!(config.mode, SerMode::Coarse);
    }

    #[test]
    fn test_ser_config_enabled() {
        let config = SerConfig::enabled().with_mode(SerMode::Fine);
        assert!(config.enabled);
        assert_eq!(config.mode, SerMode::Fine);
    }

    #[test]
    fn test_auto_hdr_config_default() {
        let config = AutoHdrConfig::new();
        assert!(!config.enabled);
        assert_eq!(config.target_luminance, 1000.0);
    }

    #[test]
    fn test_auto_hdr_config_enabled() {
        let config = AutoHdrConfig::enabled().with_target_luminance(1600.0);
        assert!(config.enabled);
        assert_eq!(config.target_luminance, 1600.0);
    }

    #[test]
    fn test_auto_hdr_luminance_clamping() {
        let config = AutoHdrConfig::enabled().with_target_luminance(50.0); // Below min (100)

        assert_eq!(config.target_luminance, 100.0);

        let config = AutoHdrConfig::enabled().with_target_luminance(20000.0); // Above max (10000)

        assert_eq!(config.target_luminance, 10000.0);
    }

    #[test]
    fn test_vrs_config_default() {
        let config = VrsConfig::new();
        assert!(!config.enabled);
        assert_eq!(config.tier, VrsTier::NotSupported);
        assert_eq!(config.shading_rate, ShadingRate::Rate1x1);
    }

    #[test]
    fn test_vrs_config_tier2() {
        let config = VrsConfig::new()
            .with_tier(VrsTier::Tier2)
            .with_shading_rate(ShadingRate::Rate2x2);

        assert!(config.enabled);
        assert_eq!(config.tier, VrsTier::Tier2);
        assert_eq!(config.shading_rate, ShadingRate::Rate2x2);
    }

    #[test]
    fn test_direct_storage_config_default() {
        let config = DirectStorageConfig::new();
        assert!(!config.enabled);
        assert!(!config.gpu_decompression);
        assert_eq!(config.queue_depth, 32);
    }

    #[test]
    fn test_direct_storage_config_enabled() {
        let config = DirectStorageConfig::enabled()
            .with_gpu_decompression(true)
            .with_queue_depth(64);

        assert!(config.enabled);
        assert!(config.gpu_decompression);
        assert_eq!(config.queue_depth, 64);
    }

    #[test]
    fn test_direct_storage_queue_depth_clamping() {
        let config = DirectStorageConfig::enabled().with_queue_depth(1000);
        assert_eq!(config.queue_depth, 256);
    }

    #[test]
    fn test_hdr_metadata_default() {
        let metadata = HdrMetadata::default();
        assert_eq!(metadata.max_cll, 1000.0);
        assert_eq!(metadata.max_fall, 400.0);
        assert_eq!(metadata.min_luminance, 0.001);
        assert_eq!(metadata.max_luminance, 1000.0);
    }
}
