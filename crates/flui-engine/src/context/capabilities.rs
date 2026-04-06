//! GPU capability detection and feature queries.

/// Detected GPU capabilities, queried once at device creation.
#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    /// The graphics backend in use (Vulkan, Metal, DX12, etc.).
    pub backend: wgpu::Backend,
    /// Human-readable adapter/GPU name.
    pub adapter_name: String,
    /// GPU vendor name (resolved from PCI vendor ID).
    pub vendor: String,
    /// Maximum supported 2D texture dimension in pixels.
    pub max_texture_size: u32,
    /// Whether the adapter supports compute shaders.
    pub supports_compute: bool,
}

impl GpuCapabilities {
    /// Build capabilities from adapter info and the adapter itself.
    #[must_use]
    pub fn from_adapter_info(info: &wgpu::AdapterInfo, adapter: &wgpu::Adapter) -> Self {
        let limits = adapter.limits();
        Self {
            backend: info.backend,
            adapter_name: info.name.clone(),
            vendor: Self::vendor_name(info.vendor),
            max_texture_size: limits.max_texture_dimension_2d,
            // All modern adapters support compute; this checks that the empty
            // feature set is satisfied (always true), but kept for future gating.
            supports_compute: adapter.features().contains(wgpu::Features::empty()),
        }
    }

    /// Resolve a PCI vendor ID to a human-readable vendor name.
    fn vendor_name(vendor_id: u32) -> String {
        match vendor_id {
            0x1002 => "AMD".to_string(),
            0x10DE => "NVIDIA".to_string(),
            0x8086 => "Intel".to_string(),
            0x106B => "Apple".to_string(),
            _ => format!("Unknown(0x{vendor_id:04X})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_name_known_ids() {
        assert_eq!(GpuCapabilities::vendor_name(0x10DE), "NVIDIA");
        assert_eq!(GpuCapabilities::vendor_name(0x1002), "AMD");
        assert_eq!(GpuCapabilities::vendor_name(0x8086), "Intel");
        assert_eq!(GpuCapabilities::vendor_name(0x106B), "Apple");
    }

    #[test]
    fn vendor_name_unknown_id() {
        let name = GpuCapabilities::vendor_name(0xBEEF);
        assert!(name.starts_with("Unknown"));
        assert!(name.contains("BEEF"));
    }
}
