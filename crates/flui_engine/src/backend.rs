//! Backend abstraction for different rendering platforms
//!
//! A Backend provides the platform-specific implementation for rendering.
//! It creates Surfaces and manages platform resources.

use crate::surface::Surface;
use flui_types::Size;

/// Backend trait for rendering platforms
///
/// Different backends implement this trait to provide rendering capabilities:
/// - EguiBackend: Renders using egui
/// - WgpuBackend: Renders using wgpu (GPU)
/// - SkiaBackend: Renders using Skia
/// - etc.
///
/// # Example
///
/// ```rust,ignore
/// // Create a backend
/// let backend = EguiBackend::new();
///
/// // Create a surface for a window
/// let surface = backend.create_surface(Size::new(800.0, 600.0));
///
/// // Use the surface for rendering
/// loop {
///     let mut frame = surface.begin_frame();
///     compositor.composite(&scene, frame.painter());
///     drop(frame);
///     surface.present();
/// }
/// ```
pub trait RenderBackend: Send + Sync + 'static {
    /// The type of Surface this backend creates
    type Surface: Surface;

    /// Create a new surface with the given size
    ///
    /// # Arguments
    /// * `size` - The initial size of the surface
    fn create_surface(&self, size: Size) -> Self::Surface;

    /// Get the name of this backend (for debugging)
    fn name(&self) -> &'static str;

    /// Get backend capabilities
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }
}

/// Capabilities of a rendering backend
#[derive(Debug, Clone, Copy)]
pub struct BackendCapabilities {
    /// Supports hardware acceleration (GPU)
    pub hardware_accelerated: bool,

    /// Supports offscreen rendering
    pub offscreen_rendering: bool,

    /// Supports custom shaders
    pub custom_shaders: bool,

    /// Supports HDR rendering
    pub hdr: bool,

    /// Maximum texture size
    pub max_texture_size: u32,

    /// Supports multisampling anti-aliasing
    pub msaa: bool,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            hardware_accelerated: false,
            offscreen_rendering: true,
            custom_shaders: false,
            hdr: false,
            max_texture_size: 4096,
            msaa: false,
        }
    }
}

/// Backend information for debugging and diagnostics
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// Backend name
    pub name: &'static str,

    /// Backend version
    pub version: &'static str,

    /// Backend capabilities
    pub capabilities: BackendCapabilities,

    /// Additional metadata
    pub metadata: Vec<(&'static str, String)>,
}

impl BackendInfo {
    /// Create backend info with just a name
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            version: env!("CARGO_PKG_VERSION"),
            capabilities: BackendCapabilities::default(),
            metadata: Vec::new(),
        }
    }

    /// Add metadata entry
    pub fn with_metadata(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.metadata.push((key, value.into()));
        self
    }

    /// Set capabilities
    pub fn with_capabilities(mut self, capabilities: BackendCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_capabilities_default() {
        let caps = BackendCapabilities::default();
        assert!(!caps.hardware_accelerated);
        assert!(caps.offscreen_rendering);
        assert_eq!(caps.max_texture_size, 4096);
    }

    #[test]
    fn test_backend_info() {
        let info = BackendInfo::new("test")
            .with_metadata("platform", "test")
            .with_metadata("renderer", "mock");

        assert_eq!(info.name, "test");
        assert_eq!(info.metadata.len(), 2);
    }
}
