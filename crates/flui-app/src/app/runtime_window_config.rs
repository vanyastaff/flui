//! Destination-local options for a dedicated runtime's first window.

use super::{
    AppCapabilities, AppConfig, CloseRequestHandler, FrameFailureDetail, FrameFailureHandler,
    MotionOverride, PointerResampling,
};

/// Window and UI policy constructed by a dedicated factory on its destination.
/// Application identity, services, executors, storage roots and global exit
/// policy belong to `Application` and cannot be configured here.
#[derive(Debug, Default)]
pub struct RuntimeWindowConfig {
    pub(crate) window: AppConfig,
    pub(crate) capabilities: AppCapabilities,
}

impl RuntimeWindowConfig {
    /// Default local window options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// The window title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.window = self.window.with_title(title);
        self
    }
    /// The initial logical size.
    #[must_use]
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.window = self.window.with_size(width, height);
        self
    }
    /// The minimum logical size.
    #[must_use]
    pub fn with_min_size(mut self, width: u32, height: u32) -> Self {
        self.window = self.window.with_min_size(width, height);
        self
    }
    /// The maximum logical size.
    #[must_use]
    pub fn with_max_size(mut self, width: u32, height: u32) -> Self {
        self.window = self.window.with_max_size(width, height);
        self
    }
    /// The initial outer position.
    #[must_use]
    pub fn with_position(mut self, position: flui_foundation::geometry::Point<f64>) -> Self {
        self.window.position = Some(position);
        self
    }
    /// The initial native window mode.
    #[must_use]
    pub fn with_mode(mut self, mode: flui_platform::WindowMode) -> Self {
        self.window.mode = mode;
        self
    }
    /// Whether the window is resizable.
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.window.resizable = resizable;
        self
    }
    /// Whether the native window has decorations.
    #[must_use]
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.window.decorations = decorations;
        self
    }
    /// Native creation options read only by the corresponding host.
    #[must_use]
    pub fn with_os_options(mut self, options: flui_platform::os_options::OsWindowOptions) -> Self {
        self.window.os_options = options;
        self
    }
    /// The window's swapchain latency.
    #[must_use]
    pub fn with_frame_latency(mut self, latency: flui_engine::FrameLatency) -> Self {
        self.window.frame_latency = latency;
        self
    }
    /// The native pointer delivery policy.
    #[must_use]
    pub fn with_input_latency(mut self, latency: flui_platform::InputLatency) -> Self {
        self.window.input_latency = latency;
        self
    }
    /// The native surface colour policy.
    #[must_use]
    pub fn with_surface_color(mut self, color: flui_engine::SurfaceColor) -> Self {
        self.window.surface_color = color;
        self
    }
    /// The local motion preference.
    #[must_use]
    pub fn with_motion_override(mut self, preference: MotionOverride) -> Self {
        self.window.motion_override = preference;
        self
    }
    /// Pointer sample resampling for this runtime.
    #[must_use]
    pub fn with_pointer_resampling(mut self, policy: PointerResampling) -> Self {
        self.window.pointer_resampling = policy;
        self
    }
    /// Whether to paint the performance overlay.
    #[must_use]
    pub fn with_performance_overlay(mut self, show: bool) -> Self {
        self.window.show_performance_overlay = show;
        self
    }
    /// Failure detail, bounded by the application's privacy policy.
    #[must_use]
    pub fn with_frame_failure_detail(mut self, detail: FrameFailureDetail) -> Self {
        self.window.frame_failure_detail = detail;
        self
    }
    /// A failure callback created and retained on this owner.
    #[must_use]
    pub fn with_frame_failure_handler(mut self, handler: FrameFailureHandler) -> Self {
        self.window.frame_failure_handler = Some(handler);
        self
    }
    /// A local input recording factory.
    #[must_use]
    pub fn with_input_tap(mut self, tap: flui_runtime::InputTapFactory) -> Self {
        self.window.input_tap = Some(tap);
        self
    }
    /// The local fallback error view.
    #[must_use]
    pub fn with_error_view_builder(mut self, builder: flui_view::ErrorViewBuilder) -> Self {
        self.window.error_view_builder = Some(builder);
        self
    }
    /// The local close-request handler.
    #[must_use]
    pub fn with_close_request_handler(mut self, handler: CloseRequestHandler) -> Self {
        self.window.close_request_handler = Some(handler);
        self
    }
    /// Restore this window's placement from the application's shared storage.
    #[must_use]
    pub fn restore_placement(mut self, name: flui_platform::StorageName) -> Self {
        self.window.placement = Some(name);
        self
    }
    /// Explicitly reconstruct the application's plugin/override policy locally.
    /// Missing registrations refuse publication; providers are never acquired
    /// speculatively to check their optional capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: AppCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}
