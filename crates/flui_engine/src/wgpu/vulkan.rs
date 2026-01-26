//! Vulkan 1.4 backend-specific features for Linux and Android
//!
//! This module provides access to Vulkan 1.4 features that are not exposed through wgpu's
//! cross-platform API, including:
//! - Pipeline binary caching for faster startup
//! - NVIDIA explicit sync for Wayland compositors
//! - Mesa 25.x optimizations
//! - Dynamic rendering (VK_KHR_dynamic_rendering)
//! - Extended dynamic state
//!
//! # Platform Requirements
//!
//! - Vulkan 1.4 driver (Mesa 25.0+, NVIDIA 565+, AMD AMDVLK)
//! - Linux kernel 6.8+ (for explicit sync)
//! - Wayland 1.34+ or X11
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::vulkan::{VulkanFeatures, PipelineCacheConfig};
//!
//! // Enable pipeline binary caching
//! let cache = PipelineCacheConfig::new()
//!     .with_file_path("/var/cache/flui/pipelines.bin")
//!     .with_enabled(true);
//!
//! // Check Vulkan 1.4 features
//! let features = VulkanFeatures::detect(device)?;
//! if features.supports_explicit_sync {
//!     // Use explicit sync for better Wayland performance
//! }
//! ```

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;

// ============================================================================
// Vulkan Feature Detection
// ============================================================================

/// Vulkan feature set and capabilities.
///
/// This struct contains information about which Vulkan features are available
/// on the current GPU and driver.
#[derive(Debug, Clone)]
pub struct VulkanFeatures {
    /// Vulkan API version (e.g., 1.3, 1.4).
    pub api_version: VulkanVersion,

    /// Driver version string.
    pub driver_version: String,

    /// GPU vendor (NVIDIA, AMD, Intel, etc.).
    pub vendor: GpuVendor,

    /// Supports explicit sync for Wayland (Linux 6.8+).
    pub supports_explicit_sync: bool,

    /// Supports pipeline binary caching.
    pub supports_pipeline_cache: bool,

    /// Supports dynamic rendering (no render passes).
    pub supports_dynamic_rendering: bool,

    /// Supports extended dynamic state (VK_EXT_extended_dynamic_state3).
    pub supports_extended_dynamic_state: bool,

    /// Supports mesh shaders (VK_EXT_mesh_shader).
    pub supports_mesh_shaders: bool,

    /// Mesa version (if using Mesa drivers).
    pub mesa_version: Option<MesaVersion>,
}

impl VulkanFeatures {
    /// Detect Vulkan features from a wgpu device.
    ///
    /// # Errors
    ///
    /// Returns error if device is not Vulkan backend.
    pub fn detect(device: &wgpu::Device) -> Result<Self> {
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            return Err(anyhow!("Vulkan backend detection only available on Linux/Android"));
        }

        // TODO: Query actual Vulkan device capabilities via ash (Vulkan FFI)
        // For now, return conservative defaults
        Ok(Self {
            api_version: VulkanVersion::Vulkan1_3,
            driver_version: "Unknown".to_string(),
            vendor: GpuVendor::Unknown,
            supports_explicit_sync: false,
            supports_pipeline_cache: true,
            supports_dynamic_rendering: false,
            supports_extended_dynamic_state: false,
            supports_mesh_shaders: false,
            mesa_version: None,
        })
    }

    /// Check if all Vulkan 1.4 features are supported.
    pub fn is_vulkan_1_4(&self) -> bool {
        self.api_version >= VulkanVersion::Vulkan1_4
    }

    /// Get a human-readable version string.
    pub fn version_string(&self) -> String {
        format!("Vulkan {} ({})", self.api_version.as_str(), self.driver_version)
    }

    /// Check if running on Mesa drivers.
    pub fn is_mesa(&self) -> bool {
        self.mesa_version.is_some()
    }
}

/// Vulkan API version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VulkanVersion {
    /// Vulkan 1.3
    Vulkan1_3,
    /// Vulkan 1.4
    Vulkan1_4,
}

impl VulkanVersion {
    /// Get version as string.
    pub fn as_str(&self) -> &str {
        match self {
            VulkanVersion::Vulkan1_3 => "1.3",
            VulkanVersion::Vulkan1_4 => "1.4",
        }
    }
}

/// GPU vendor identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuVendor {
    /// NVIDIA GPU.
    Nvidia,
    /// AMD GPU.
    Amd,
    /// Intel GPU.
    Intel,
    /// ARM Mali GPU.
    Mali,
    /// Qualcomm Adreno GPU.
    Adreno,
    /// Unknown vendor.
    Unknown,
}

impl GpuVendor {
    /// Get vendor name as string.
    pub fn as_str(&self) -> &str {
        match self {
            GpuVendor::Nvidia => "NVIDIA",
            GpuVendor::Amd => "AMD",
            GpuVendor::Intel => "Intel",
            GpuVendor::Mali => "ARM Mali",
            GpuVendor::Adreno => "Qualcomm Adreno",
            GpuVendor::Unknown => "Unknown",
        }
    }
}

/// Mesa driver version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MesaVersion {
    /// Major version (e.g., 25 for Mesa 25.0.1).
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
}

impl MesaVersion {
    /// Create new Mesa version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    /// Get version as string (e.g., "25.0.1").
    pub fn as_str(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ============================================================================
// Pipeline Binary Caching
// ============================================================================

/// Pipeline binary cache configuration.
///
/// Pipeline caching dramatically reduces startup time by storing compiled
/// pipeline binaries to disk. This can reduce startup from 2-5 seconds to
/// 100-300ms on subsequent runs.
///
/// # Security Considerations
///
/// Pipeline caches are driver-specific and include GPU-specific code. They should:
/// - Be stored in user-specific cache directories (XDG_CACHE_HOME)
/// - Be invalidated when driver version changes
/// - Be readable only by the current user
#[derive(Debug, Clone)]
pub struct PipelineCacheConfig {
    /// Enable pipeline caching.
    pub enabled: bool,

    /// Path to cache file (e.g., ~/.cache/flui/pipelines.bin).
    pub cache_path: PathBuf,

    /// Maximum cache size in bytes (default: 100 MB).
    pub max_size_bytes: u64,

    /// Invalidate cache if driver version changes.
    pub invalidate_on_driver_change: bool,
}

impl PipelineCacheConfig {
    /// Create default pipeline cache configuration.
    pub fn new() -> Self {
        Self {
            enabled: true,
            cache_path: Self::default_cache_path(),
            max_size_bytes: 100 * 1024 * 1024, // 100 MB
            invalidate_on_driver_change: true,
        }
    }

    /// Get default cache path based on XDG_CACHE_HOME.
    pub fn default_cache_path() -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::env;
            let cache_dir = env::var("XDG_CACHE_HOME")
                .unwrap_or_else(|_| format!("{}/.cache", env::var("HOME").unwrap_or_default()));
            PathBuf::from(cache_dir).join("flui").join("vulkan_pipelines.bin")
        }

        #[cfg(target_os = "android")]
        {
            PathBuf::from("/data/local/tmp/flui_pipelines.bin")
        }

        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            PathBuf::from("vulkan_pipelines.bin")
        }
    }

    /// Enable pipeline caching.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set cache file path.
    pub fn with_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cache_path = path.into();
        self
    }

    /// Set maximum cache size in bytes.
    pub fn with_max_size(mut self, bytes: u64) -> Self {
        self.max_size_bytes = bytes;
        self
    }

    /// Load pipeline cache from disk.
    ///
    /// Returns cached data if valid, None if cache is invalid or doesn't exist.
    pub fn load(&self) -> Option<Vec<u8>> {
        if !self.enabled {
            return None;
        }

        // TODO: Implement actual cache loading
        // This requires:
        // 1. Read cache file
        // 2. Verify driver version if invalidate_on_driver_change is true
        // 3. Validate cache integrity
        None
    }

    /// Save pipeline cache to disk.
    ///
    /// # Errors
    ///
    /// Returns error if unable to write cache file.
    pub fn save(&self, _data: &[u8]) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // TODO: Implement actual cache saving
        // This requires:
        // 1. Create cache directory if it doesn't exist
        // 2. Write cache data atomically
        // 3. Set appropriate permissions (user-only read/write)
        Ok(())
    }
}

impl Default for PipelineCacheConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Explicit Sync for Wayland
// ============================================================================

/// Explicit sync configuration for Wayland compositors.
///
/// Explicit sync (Linux 6.8+, Wayland 1.34+) provides:
/// - Better frame pacing (no stuttering)
/// - Lower latency (5-10ms improvement)
/// - Reliable VRR/FreeSync support
/// - No more implicit sync overhead
///
/// Requires NVIDIA 565+ or Mesa 24.1+ drivers.
#[derive(Debug, Clone, Copy)]
pub struct ExplicitSyncConfig {
    /// Enable explicit sync.
    pub enabled: bool,

    /// Fallback to implicit sync if not supported.
    pub fallback_to_implicit: bool,
}

impl ExplicitSyncConfig {
    /// Create default explicit sync configuration.
    pub fn new() -> Self {
        Self {
            enabled: true,
            fallback_to_implicit: true,
        }
    }

    /// Disable explicit sync (use implicit sync).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            fallback_to_implicit: false,
        }
    }

    /// Check if explicit sync is available on this system.
    pub fn is_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // TODO: Check for:
            // - Linux kernel 6.8+
            // - Wayland 1.34+ with wp_linux_drm_syncobj_v1
            // - Driver support (NVIDIA 565+, Mesa 24.1+)
            false
        }

        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

impl Default for ExplicitSyncConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Dynamic Rendering
// ============================================================================

/// Dynamic rendering configuration (VK_KHR_dynamic_rendering).
///
/// Dynamic rendering eliminates the need for VkRenderPass and VkFramebuffer,
/// simplifying code and improving performance. This is a Vulkan 1.3+ feature
/// promoted to core in Vulkan 1.4.
#[derive(Debug, Clone, Copy)]
pub struct DynamicRenderingConfig {
    /// Enable dynamic rendering.
    pub enabled: bool,

    /// Fallback to render passes if not supported.
    pub fallback_to_render_pass: bool,
}

impl DynamicRenderingConfig {
    /// Create default dynamic rendering configuration.
    pub fn new() -> Self {
        Self {
            enabled: true,
            fallback_to_render_pass: true,
        }
    }

    /// Disable dynamic rendering (use render passes).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            fallback_to_render_pass: false,
        }
    }
}

impl Default for DynamicRenderingConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Extended Dynamic State
// ============================================================================

/// Extended dynamic state configuration (VK_EXT_extended_dynamic_state3).
///
/// Extended dynamic state allows more pipeline state to be changed dynamically
/// without creating new pipelines, reducing memory usage and startup time.
#[derive(Debug, Clone, Copy)]
pub struct ExtendedDynamicStateConfig {
    /// Enable extended dynamic state.
    pub enabled: bool,
}

impl ExtendedDynamicStateConfig {
    /// Create default extended dynamic state configuration.
    pub fn new() -> Self {
        Self { enabled: true }
    }

    /// Disable extended dynamic state.
    pub fn disabled() -> Self {
        Self { enabled: false }
    }
}

impl Default for ExtendedDynamicStateConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Mesa-Specific Optimizations
// ============================================================================

/// Mesa driver optimizations.
///
/// Mesa 25.x includes several performance improvements:
/// - NVK (nouveau) reaches feature parity with proprietary NVIDIA drivers
/// - RADV performance improvements (5-15%)
/// - Zink (OpenGL over Vulkan) improvements
/// - Better shader compilation times
#[derive(Debug, Clone, Copy)]
pub struct MesaOptimizations {
    /// Enable ACO shader compiler for RADV (AMD GPUs).
    pub use_aco: bool,

    /// Enable NVK driver for NVIDIA GPUs (instead of nouveau).
    pub use_nvk: bool,

    /// Enable Zink for OpenGL compatibility.
    pub use_zink: bool,
}

impl MesaOptimizations {
    /// Create default Mesa optimizations.
    pub fn new() -> Self {
        Self {
            use_aco: true,
            use_nvk: true,
            use_zink: false,
        }
    }

    /// Check if running on Mesa drivers.
    pub fn is_mesa_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // TODO: Check for Mesa drivers via VkPhysicalDeviceProperties
            false
        }

        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

impl Default for MesaOptimizations {
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
    fn test_vulkan_version_ordering() {
        assert!(VulkanVersion::Vulkan1_4 > VulkanVersion::Vulkan1_3);
    }

    #[test]
    fn test_vulkan_version_string() {
        assert_eq!(VulkanVersion::Vulkan1_4.as_str(), "1.4");
        assert_eq!(VulkanVersion::Vulkan1_3.as_str(), "1.3");
    }

    #[test]
    fn test_gpu_vendor_string() {
        assert_eq!(GpuVendor::Nvidia.as_str(), "NVIDIA");
        assert_eq!(GpuVendor::Amd.as_str(), "AMD");
        assert_eq!(GpuVendor::Intel.as_str(), "Intel");
    }

    #[test]
    fn test_mesa_version_ordering() {
        let v25_0 = MesaVersion::new(25, 0, 0);
        let v25_1 = MesaVersion::new(25, 1, 0);
        let v26_0 = MesaVersion::new(26, 0, 0);

        assert!(v25_1 > v25_0);
        assert!(v26_0 > v25_1);
    }

    #[test]
    fn test_mesa_version_string() {
        let version = MesaVersion::new(25, 0, 1);
        assert_eq!(version.as_str(), "25.0.1");
    }

    #[test]
    fn test_pipeline_cache_default() {
        let config = PipelineCacheConfig::new();
        assert!(config.enabled);
        assert!(config.invalidate_on_driver_change);
        assert_eq!(config.max_size_bytes, 100 * 1024 * 1024);
    }

    #[test]
    fn test_pipeline_cache_custom_path() {
        let config = PipelineCacheConfig::new()
            .with_file_path("/tmp/test_cache.bin")
            .with_max_size(50 * 1024 * 1024);

        assert_eq!(config.cache_path, PathBuf::from("/tmp/test_cache.bin"));
        assert_eq!(config.max_size_bytes, 50 * 1024 * 1024);
    }

    #[test]
    fn test_explicit_sync_default() {
        let config = ExplicitSyncConfig::new();
        assert!(config.enabled);
        assert!(config.fallback_to_implicit);
    }

    #[test]
    fn test_explicit_sync_disabled() {
        let config = ExplicitSyncConfig::disabled();
        assert!(!config.enabled);
        assert!(!config.fallback_to_implicit);
    }

    #[test]
    fn test_dynamic_rendering_default() {
        let config = DynamicRenderingConfig::new();
        assert!(config.enabled);
        assert!(config.fallback_to_render_pass);
    }

    #[test]
    fn test_dynamic_rendering_disabled() {
        let config = DynamicRenderingConfig::disabled();
        assert!(!config.enabled);
        assert!(!config.fallback_to_render_pass);
    }

    #[test]
    fn test_extended_dynamic_state_default() {
        let config = ExtendedDynamicStateConfig::new();
        assert!(config.enabled);
    }

    #[test]
    fn test_mesa_optimizations_default() {
        let opt = MesaOptimizations::new();
        assert!(opt.use_aco);
        assert!(opt.use_nvk);
        assert!(!opt.use_zink);
    }
}
