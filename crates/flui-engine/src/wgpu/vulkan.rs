//! Vulkan 1.4 backend-specific features for Linux and Android
//!
//! This module provides access to Vulkan 1.4 features that are not exposed
//! through wgpu's cross-platform API, including:
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

use std::path::PathBuf;

use anyhow::{Result, anyhow};

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
    /// Uses wgpu's feature flags and adapter info to infer Vulkan capabilities
    /// where possible. Some Vulkan-specific features (explicit sync, dynamic
    /// rendering, extended dynamic state) are not directly exposed through wgpu
    /// and would require `ash` (Vulkan FFI) for accurate detection.
    ///
    /// # Errors
    ///
    /// Returns error if device is not Vulkan backend.
    pub fn detect(device: &wgpu::Device) -> Result<Self> {
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            let _ = device;
            return Err(anyhow!(
                "Vulkan backend detection only available on Linux/Android"
            ));
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let features = device.features();

            // Mesh shader support is queryable through wgpu experimental features
            let supports_mesh_shaders = features.contains(wgpu::Features::EXPERIMENTAL_MESH_SHADER);

            // Dynamic rendering (VK_KHR_dynamic_rendering) is core in Vulkan 1.3+.
            // wgpu uses dynamic rendering internally when available, but doesn't
            // expose it as a feature flag. Assume supported since wgpu requires Vulkan 1.3+.
            let supports_dynamic_rendering = true;

            // Pipeline caching is universally supported in Vulkan
            let supports_pipeline_cache = true;

            // Extended dynamic state (VK_EXT_extended_dynamic_state3) is not
            // queryable through wgpu. Return false conservatively.
            let supports_extended_dynamic_state = false;

            // Explicit sync (linux-drm-syncobj) requires Linux 6.8+ and
            // Wayland 1.34+. Not detectable through wgpu.
            let supports_explicit_sync = false;

            // Vendor detection from wgpu is not available without adapter info.
            // The detect() method takes a Device, not an Adapter, so we can't
            // call adapter.get_info(). Default to Unknown.
            let vendor = GpuVendor::Unknown;

            // API version: wgpu requires Vulkan 1.3 minimum. If mesh shaders
            // are available, it likely indicates a newer driver stack.
            let api_version = VulkanVersion::Vulkan1_3;

            tracing::debug!(
                ?api_version,
                ?vendor,
                supports_mesh_shaders,
                supports_dynamic_rendering,
                supports_pipeline_cache,
                supports_explicit_sync,
                "Detected Vulkan capabilities (some features require ash FFI for accurate detection)"
            );

            Ok(Self {
                api_version,
                driver_version: "Unknown (wgpu does not expose driver version)".to_string(),
                vendor,
                supports_explicit_sync,
                supports_pipeline_cache,
                supports_dynamic_rendering,
                supports_extended_dynamic_state,
                supports_mesh_shaders,
                mesa_version: None, // Requires VkPhysicalDeviceDriverProperties via ash
            })
        }
    }

    /// Check if all Vulkan 1.4 features are supported.
    pub fn is_vulkan_1_4(&self) -> bool {
        self.api_version >= VulkanVersion::Vulkan1_4
    }

    /// Get a human-readable version string.
    pub fn version_string(&self) -> String {
        format!(
            "Vulkan {} ({})",
            self.api_version.as_str(),
            self.driver_version
        )
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
        Self {
            major,
            minor,
            patch,
        }
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
/// Pipeline caches are driver-specific and include GPU-specific code. They
/// should:
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
            PathBuf::from(cache_dir)
                .join("flui")
                .join("vulkan_pipelines.bin")
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
    /// Returns cached data if valid, `None` if cache is invalid, too large,
    /// or doesn't exist. The cache file uses a simple format:
    /// - First 4 bytes: magic number (0x464C5549 = "FLUI")
    /// - Next 4 bytes: cache format version (1)
    /// - Remaining bytes: raw pipeline cache data
    ///
    /// Driver version invalidation is not yet implemented since wgpu does not
    /// expose the Vulkan driver version. Cache files will be used regardless
    /// of driver changes until ash FFI integration is added.
    pub fn load(&self) -> Option<Vec<u8>> {
        use std::io::Read;

        if !self.enabled {
            return None;
        }

        let path = &self.cache_path;
        if !path.exists() {
            tracing::debug!(?path, "Pipeline cache file does not exist");
            return None;
        }

        let metadata = std::fs::metadata(path).ok()?;
        let file_size = metadata.len();

        // Reject files exceeding max cache size
        if file_size > self.max_size_bytes {
            tracing::debug!(
                ?path,
                file_size,
                max = self.max_size_bytes,
                "Pipeline cache file exceeds max size, ignoring"
            );
            return None;
        }

        // Minimum valid size: 8 bytes header + at least 1 byte of data
        if file_size < 9 {
            tracing::debug!(?path, file_size, "Pipeline cache file too small, ignoring");
            return None;
        }

        let mut file = std::fs::File::open(path).ok()?;
        let mut data = Vec::with_capacity(file_size as usize);
        if file.read_to_end(&mut data).ok()? != file_size as usize {
            tracing::debug!(?path, "Pipeline cache file read incomplete");
            return None;
        }

        // Validate magic number
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != 0x464C5549 {
            tracing::debug!(?path, magic, "Pipeline cache file has invalid magic number");
            return None;
        }

        // Validate version
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 1 {
            tracing::debug!(?path, version, "Pipeline cache file has unsupported version");
            return None;
        }

        let cache_data = data[8..].to_vec();
        tracing::debug!(?path, size = cache_data.len(), "Loaded pipeline cache from disk");
        Some(cache_data)
    }

    /// Save pipeline cache to disk.
    ///
    /// Creates the parent directory if it doesn't exist. Writes atomically by
    /// first writing to a temporary file, then renaming. On Unix, sets file
    /// permissions to user-only read/write (0o600).
    ///
    /// # Errors
    ///
    /// Returns error if unable to create directories or write the cache file.
    pub fn save(&self, data: &[u8]) -> Result<()> {
        use std::io::Write;

        if !self.enabled {
            return Ok(());
        }

        if data.len() as u64 > self.max_size_bytes {
            tracing::debug!(
                size = data.len(),
                max = self.max_size_bytes,
                "Pipeline cache data exceeds max size, not saving"
            );
            return Ok(());
        }

        let path = &self.cache_path;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    anyhow!("Failed to create pipeline cache directory {:?}: {}", parent, e)
                })?;
                tracing::debug!(?parent, "Created pipeline cache directory");
            }
        }

        // Build cache file with header
        let mut file_data = Vec::with_capacity(8 + data.len());
        // Magic number: "FLUI" = 0x464C5549
        file_data.extend_from_slice(&0x464C5549_u32.to_le_bytes());
        // Format version: 1
        file_data.extend_from_slice(&1_u32.to_le_bytes());
        // Pipeline cache data
        file_data.extend_from_slice(data);

        // Write to a temporary file first, then rename for atomic operation
        let tmp_path = path.with_extension("tmp");
        let mut file = std::fs::File::create(&tmp_path).map_err(|e| {
            anyhow!("Failed to create temporary cache file {:?}: {}", tmp_path, e)
        })?;

        file.write_all(&file_data).map_err(|e| {
            anyhow!("Failed to write pipeline cache data: {}", e)
        })?;

        file.flush().map_err(|e| {
            anyhow!("Failed to flush pipeline cache data: {}", e)
        })?;
        drop(file);

        // Set user-only permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&tmp_path, perms);
        }

        // Atomic rename
        std::fs::rename(&tmp_path, path).map_err(|e| {
            anyhow!("Failed to rename pipeline cache file {:?} -> {:?}: {}", tmp_path, path, e)
        })?;

        tracing::debug!(?path, size = data.len(), "Saved pipeline cache to disk");
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
    ///
    /// Explicit sync requires:
    /// - Linux kernel 6.8+ (for `linux-drm-syncobj` protocol)
    /// - Wayland 1.34+ with `wp_linux_drm_syncobj_manager_v1`
    /// - Driver support: NVIDIA 565+, Mesa 24.1+
    ///
    /// These checks require system-level queries (uname, Wayland protocol
    /// negotiation, driver version inspection) that are not available through
    /// wgpu. Returns `false` as a conservative default.
    pub fn is_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Accurate detection would require:
            // 1. uname().release to check kernel >= 6.8
            // 2. Wayland registry query for wp_linux_drm_syncobj_manager_v1
            // 3. VkPhysicalDeviceDriverProperties for driver version
            // None of these are accessible through wgpu's API.
            tracing::debug!(
                "Explicit sync availability not detectable via wgpu \
                 (requires kernel version, Wayland protocol, and driver queries)"
            );
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
    ///
    /// Mesa detection requires querying `VkPhysicalDeviceDriverProperties.driverID`
    /// via Vulkan's `vkGetPhysicalDeviceProperties2`, which is not exposed through
    /// wgpu's public API. Returns `false` as a conservative default.
    ///
    /// Mesa driver IDs include: `VK_DRIVER_ID_MESA_RADV`, `VK_DRIVER_ID_MESA_TURNIP`,
    /// `VK_DRIVER_ID_MESA_NVK`, `VK_DRIVER_ID_MESA_VENUS`, etc.
    pub fn is_mesa_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            // Accurate detection requires ash (Vulkan FFI):
            //   vkGetPhysicalDeviceProperties2 -> VkPhysicalDeviceDriverProperties.driverID
            // Mesa-specific driver IDs: MESA_RADV, MESA_TURNIP, MESA_NVK, etc.
            tracing::debug!(
                "Mesa driver detection not available via wgpu \
                 (requires VkPhysicalDeviceDriverProperties query)"
            );
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
